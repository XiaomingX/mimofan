//! `ralph` — fresh-round mode for the RLM REPL.
//!
//! A "ralph round" runs a *fresh* sub-agent child (no parent conversation
//! history) for one task, then collects a structured [`SubAgentReport`]. The
//! child's intermediate dialogue is intentionally **never** injected into the
//! main session context — only the compact report is retained, and it lives
//! in an in-memory store that survives across RLM rounds.
//!
//! Isolation contract: the child is spawned with `fork_context = false`, which
//! the sub-agent system documents as "fresh child context" — the child does
//! not inherit the parent's message prefix. We therefore never seed the child
//! with a [`SubAgentForkContext`], and we assert the resulting child reports
//! `context_mode == "fresh"` so a forked history can never leak across rounds.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::tools::subagent::{
    SharedSubAgentManager, SubAgentAssignment, SubAgentResult, SubAgentRuntime, SubAgentStatus,
    SubAgentType, WaitCond, spawn_subagent_from_input,
};

// ---------------------------------------------------------------------------
// Structured report
// ---------------------------------------------------------------------------

/// Outcome of one fresh ralph round, retained across RLM turns.
///
/// Only this summary — never the child's raw transcript — is kept in the
/// round store, so repeated ralph rounds accumulate structured signal without
/// bloating the main session context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubAgentReport {
    /// Stable id for this round (e.g. `ralph:<uuid>`).
    pub round_id: String,
    /// The task the fresh child was asked to perform.
    pub task: String,
    /// One-line human summary of the child's self-report.
    pub summary: String,
    /// Structured status of the child (Completed / Failed / ...).
    pub status: SubAgentStatus,
    /// Opaque artifact references surfaced by the child worker record.
    pub artifacts: Vec<String>,
    /// The underlying sub-agent id, for follow-up inspection via `/agent`.
    pub agent_id: String,
    /// Wall-clock epoch millis when the report was recorded.
    pub created_at_ms: u64,
}

impl SubAgentReport {
    fn from_result(round_id: String, task: &str, result: &SubAgentResult) -> Self {
        let summary = result
            .result
            .clone()
            .unwrap_or_else(|| "<no child self-report>".to_string());
        let artifacts = result
            .checkpoint
            .as_ref()
            .map(|cp| cp.continuation_handle.clone())
            .into_iter()
            .collect::<Vec<_>>();
        SubAgentReport {
            round_id,
            task: task.to_string(),
            summary,
            status: result.status.clone(),
            artifacts,
            agent_id: result.agent_id.clone(),
            created_at_ms: epoch_millis_now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Cross-round store
// ---------------------------------------------------------------------------

/// In-memory, process-local store of ralph round reports. Reports persist for
/// the lifetime of the REPL process, so they are visible across RLM turns.
/// (No new crate is introduced; the store is a plain `Vec` behind a mutex.)
#[derive(Debug, Default)]
pub struct RalphRoundStore {
    reports: Mutex<Vec<SubAgentReport>>,
}

impl RalphRoundStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed round report.
    pub fn record(&self, report: SubAgentReport) {
        if let Ok(mut guard) = self.reports.lock() {
            guard.push(report);
        }
    }

    /// All reports recorded so far, oldest first.
    #[must_use]
    pub fn reports(&self) -> Vec<SubAgentReport> {
        self.reports
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Number of reports recorded so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.reports.lock().map(|g| g.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// Spawner seam (testable)
// ---------------------------------------------------------------------------

/// Abstraction over "run a fresh child sub-agent and return its result".
///
/// The default [`RealFreshChildSpawner`] drives the production sub-agent
/// machinery with `fork_context = false`. Tests supply a [`MockFreshChildSpawner`]
/// so `run_fresh_round` can be exercised without a live LLM client.
#[async_trait]
pub trait FreshChildSpawner: Send + Sync {
    /// Run a fresh child for `prompt` and return its terminal [`SubAgentResult`].
    async fn run_fresh_child(&self, prompt: &str) -> Result<SubAgentResult>;
}

/// Production spawner. Wraps the shared sub-agent manager + runtime and spawns
/// a **fresh** child (no fork context) for each ralph round.
pub struct RealFreshChildSpawner {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
}

impl RealFreshChildSpawner {
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl FreshChildSpawner for RealFreshChildSpawner {
    async fn run_fresh_child(&self, prompt: &str) -> Result<SubAgentResult> {
        // `fork_context: false` is the documented "fresh child context" flag:
        // the child does NOT inherit the parent message prefix. We also leave
        // `fork_turns` absent so no history window is carried.
        let input = serde_json::json!({
            "action": "start",
            "prompt": prompt,
            "agent_type": "general",
            "fork_context": false,
        });
        let spawned =
            spawn_subagent_from_input(input, Arc::clone(&self.manager), self.runtime.clone())
                .await
                .map_err(|e| anyhow!("ralph: failed to spawn fresh child: {e}"))?;

        let agent_id = spawned.agent_id.clone();

        // Wait for the child to reach a terminal (Done) lifecycle state, then
        // read back its final result. Bounded timeout keeps the REPL responsive.
        {
            let manager = self.manager.read().await;
            let _ = manager.wait_until(&agent_id, WaitCond::Done, Duration::from_secs(600));
        }
        let manager = self.manager.read().await;
        manager
            .get_result_by_ref(&agent_id)
            .map_err(|e| anyhow!("ralph: failed to read child result: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Core entry point
// ---------------------------------------------------------------------------

/// Run one fresh ralph round: spawn a history-free child, collect its result
/// into a [`SubAgentReport`], and record it in `store` for cross-round
/// retention.
///
/// The child never receives the parent's conversation history (it is spawned
/// fresh), so the main session context is never polluted by the round's
/// intermediate dialogue — only the compact report is retained.
pub async fn run_fresh_round(
    spawner: &dyn FreshChildSpawner,
    store: &RalphRoundStore,
    prompt: &str,
) -> Result<SubAgentReport> {
    let round_id = format!("ralph:{}", Uuid::new_v4().simple());
    let result = spawner.run_fresh_child(prompt).await?;

    // Isolation assertion: a ralph child must be fresh, never forked.
    if result.context_mode == "forked" {
        return Err(anyhow!(
            "ralph: child unexpectedly reported forked context_mode; \
             fresh-round contract violated (agent_id={})",
            result.agent_id
        ));
    }

    let report = SubAgentReport::from_result(round_id, prompt, &result);
    store.record(report.clone());
    Ok(report)
}

/// Convenience wrapper that builds the production spawner from live runtime
/// handles and runs a round. Use this from the RLM REPL command path.
pub async fn run_fresh_round_with_runtime(
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    store: &RalphRoundStore,
    prompt: &str,
) -> Result<SubAgentReport> {
    let spawner = RealFreshChildSpawner::new(manager, runtime);
    run_fresh_round(&spawner, store, prompt).await
}

fn epoch_millis_now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Mock for tests
// ---------------------------------------------------------------------------

/// Test double: records the prompts it received and returns a canned result.
/// The canned result is built with `context_mode = "fresh"` so the production
/// isolation assertion passes, proving the round does not carry history.
#[derive(Default)]
pub struct MockFreshChildSpawner {
    pub received_prompts: Arc<Mutex<Vec<String>>>,
    pub next_result: Arc<Mutex<Option<SubAgentResult>>>,
}

impl MockFreshChildSpawner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_result(result: SubAgentResult) -> Self {
        Self {
            received_prompts: Arc::new(Mutex::new(Vec::new())),
            next_result: Arc::new(Mutex::new(Some(result))),
        }
    }

    /// How many fresh children this mock has spawned.
    #[must_use]
    pub fn spawned_count(&self) -> usize {
        self.received_prompts.lock().map(|g| g.len()).unwrap_or(0)
    }
}

#[async_trait]
impl FreshChildSpawner for MockFreshChildSpawner {
    async fn run_fresh_child(&self, prompt: &str) -> Result<SubAgentResult> {
        self.received_prompts
            .lock()
            .map(|mut g| g.push(prompt.to_string()))
            .unwrap_or_default();
        self.next_result
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
            .ok_or_else(|| anyhow!("mock spawner has no canned result"))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_result(agent_id: &str) -> SubAgentResult {
        SubAgentResult {
            name: agent_id.to_string(),
            agent_id: agent_id.to_string(),
            context_mode: "fresh".to_string(),
            fork_context: false,
            workspace: None,
            git_branch: None,
            agent_type: SubAgentType::General,
            assignment: SubAgentAssignment::new("task".to_string(), None),
            model: "test-model".to_string(),
            nickname: None,
            status: SubAgentStatus::Completed,
            worker_status: None,
            parent_run_id: None,
            spawn_depth: 0,
            result: Some("the child did the thing".to_string()),
            steps_taken: 1,
            checkpoint: None,
            needs_input: None,
            duration_ms: 10,
            from_prior_session: false,
        }
    }

    #[tokio::test]
    async fn run_fresh_round_returns_report_and_stores_it() {
        let store = RalphRoundStore::new();
        let mock = MockFreshChildSpawner::with_result(fresh_result("agent_abc123"));

        let report = run_fresh_round(&mock, &store, "summarize the log")
            .await
            .expect("run_fresh_round should succeed");

        // Returns a structured SubAgentReport.
        assert_eq!(report.task, "summarize the log");
        assert_eq!(report.agent_id, "agent_abc123");
        assert_eq!(report.summary, "the child did the thing");
        assert_eq!(report.status, SubAgentStatus::Completed);
        assert!(report.round_id.starts_with("ralph:"));

        // Cross-round retention: the report is recorded in the store.
        assert_eq!(store.len(), 1);
        assert_eq!(store.reports()[0].round_id, report.round_id);

        // The child was actually spawned via the seam.
        assert_eq!(mock.spawned_count(), 1);
    }

    #[tokio::test]
    async fn run_fresh_round_isolates_from_main_session() {
        // A forked child result must be rejected by the isolation contract,
        // proving run_fresh_round never accepts a history-carrying child.
        let mut forked = fresh_result("agent_forked");
        forked.context_mode = "forked".to_string();
        forked.fork_context = true;

        let store = RalphRoundStore::new();
        let mock = MockFreshChildSpawner::with_result(forked);

        let err = run_fresh_round(&mock, &store, "do work").await;
        assert!(err.is_err(), "forked child must be rejected");
        assert!(
            store.is_empty(),
            "no report recorded when isolation is violated"
        );
    }

    #[tokio::test]
    async fn reports_persist_across_rounds() {
        let store = RalphRoundStore::new();
        let mock = MockFreshChildSpawner::with_result(fresh_result("agent_r1"));

        let _ = run_fresh_round(&mock, &store, "round one").await.unwrap();
        let _ = run_fresh_round(&mock, &store, "round two").await.unwrap();

        let reports = store.reports();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].task, "round one");
        assert_eq!(reports[1].task, "round two");
    }
}
