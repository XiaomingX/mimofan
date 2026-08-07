//! Goal tools for the model-visible LLM-as-judge loop.
//!
//! The TUI already has a `/goal` command and passes its objective into the
//! engine prompt. This module keeps the runtime slice separate: a small
//! session-scoped state object plus tools the model can use to inspect and
//! close out that state.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};

/// Maximum number of automatic goal-continuation prompt injections in one
/// engine turn. This is intra-turn granularity only — it prevents a stuck spin
/// within a single turn from making no progress. The cross-turn loop has **no
/// cap**: a goal runs until complete/blocked/paused, or an optional budget is
/// exhausted. See `goal_loop::decide_continuation`.
pub const MAX_GOAL_CONTINUATIONS_PER_TURN: u32 = 3;

/// Shared reference to the current runtime goal.
///
/// 使用 `std::sync::Mutex` 是有意为之：目标状态的读写**全部发生在同步代码块内**
/// （见各 `execute` 方法，守卫在 `{}` 块内即 drop，绝不跨 `.await`）。
/// ⚠️ 红线：此守卫**绝不能**在持有期间跨 `.await`——`std` 锁跨 await 会阻塞整个
/// tokio worker 线程。若未来需在 async 上下文长持锁，应整体换为 `tokio::sync::Mutex`
/// （届时需要把所有 `.lock()` 同步调用点改为 `.lock().await`，涉及 engine_messages/
/// turn_loop/goal.rs 等多处，属独立评估项）。详见 ARCHITECTURE_STABILITY.md §8.3。
pub type SharedGoalState = Arc<Mutex<GoalState>>;

/// Create an empty shared goal state.
#[must_use]
pub fn new_shared_goal_state() -> SharedGoalState {
    Arc::new(Mutex::new(GoalState::default()))
}

/// Create shared state seeded from the host goal surface with an explicit status.
#[must_use]
pub fn new_shared_goal_state_from_host_status(
    objective: Option<String>,
    token_budget: Option<u32>,
    status: GoalStatus,
) -> SharedGoalState {
    let mut state = GoalState::default();
    state.sync_from_host_status(objective.as_deref(), token_budget, status);
    Arc::new(Mutex::new(state))
}

/// Runtime status for a goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Active,
    Paused,
    Complete,
    Blocked,
}

impl GoalStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Complete => "complete",
            Self::Blocked => "blocked",
        }
    }
}

/// Signal reported by the engine at the end of each turn, used to drive the
/// anti-drift guardrails (no-progress / repeated-error circuit breakers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressSignal {
    /// At least one file changed during the turn — forward progress.
    FileChanged,
    /// No file changes this turn (and no tool error worth counting).
    NoChange,
    /// A tool errored; `fingerprint` is a stable hash-ish key (e.g. the
    /// error message trimmed to a bounded length) used to detect repeats.
    ToolError { fingerprint: String },
}

/// Default circuit-breaker thresholds (kept loose so normal long tasks are
/// not interrupted). Tunable via env `MIMOFAN_GOAL_NO_PROGRESS_ROUNDS` /
/// `MIMOFAN_GOAL_REPEATED_ERROR_ROUNDS`.
pub const DEFAULT_NO_PROGRESS_ROUNDS: u32 = 8;
pub const DEFAULT_REPEATED_ERROR_ROUNDS: u32 = 5;

/// Session-local goal state. `Instant` stays runtime-only; snapshots expose
/// elapsed seconds so tool output remains serializable and stable.
#[derive(Debug, Clone, Default)]
pub struct GoalState {
    objective: Option<String>,
    token_budget: Option<u32>,
    status: Option<GoalStatus>,
    tokens_used: u64,
    time_used_seconds: u64,
    continuation_count: u32,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    evidence: Option<String>,
    blocker: Option<String>,
    completion_verification: Option<GoalCompletionVerification>,
    /// Human-readable progress checklist (done / todo lines) shown in the
    /// goal contract injected into the system prompt.
    progress_checklist: Option<String>,
    /// Consecutive turns with no file changes — feeds the no-progress breaker.
    no_progress_rounds: u32,
    /// Last tool-error fingerprint; `None` resets the repeated-error counter.
    last_tool_error_fingerprint: Option<String>,
    /// Consecutive turns ending on the *same* tool-error fingerprint.
    repeated_error_rounds: u32,
    /// Optional wall-clock budget in seconds (wired into `decide_continuation`).
    time_budget_seconds: Option<u64>,
}

impl GoalState {
    #[must_use]
    pub fn objective(&self) -> Option<&str> {
        self.objective.as_deref()
    }

    /// The completion verification attached to the goal, if any.
    #[must_use]
    pub fn completion_verification(&self) -> Option<&GoalCompletionVerification> {
        self.completion_verification.as_ref()
    }

    /// The progress checklist, if set.
    #[must_use]
    pub fn progress_checklist(&self) -> Option<&str> {
        self.progress_checklist.as_deref()
    }

    #[must_use]
    pub fn token_budget(&self) -> Option<u32> {
        self.token_budget
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.objective.is_some() && self.status == Some(GoalStatus::Active)
    }

    pub fn sync_from_host_status(
        &mut self,
        objective: Option<&str>,
        token_budget: Option<u32>,
        status: GoalStatus,
    ) {
        let objective = objective.map(str::trim).filter(|value| !value.is_empty());
        match objective {
            Some(objective) => {
                let changed = self.objective.as_deref() != Some(objective);
                let status_changed = self.status != Some(status);
                if changed {
                    self.objective = Some(objective.to_string());
                    self.token_budget = token_budget;
                    self.tokens_used = 0;
                    self.time_used_seconds = 0;
                    self.continuation_count = 0;
                    self.started_at = Some(Instant::now());
                    self.evidence = None;
                    self.blocker = None;
                    self.completion_verification = None;
                    self.progress_checklist = None;
                    self.no_progress_rounds = 0;
                    self.last_tool_error_fingerprint = None;
                    self.repeated_error_rounds = 0;
                    self.time_budget_seconds = None;
                } else if self.token_budget != token_budget {
                    self.token_budget = token_budget;
                }

                if changed || status_changed || self.status.is_none() {
                    self.status = Some(status);
                    self.finished_at = if status == GoalStatus::Active {
                        None
                    } else {
                        Some(Instant::now())
                    };
                }
            }
            None => self.clear(),
        }
    }

    pub fn create(&mut self, objective: String, token_budget: Option<u32>) {
        self.objective = Some(objective);
        self.token_budget = token_budget;
        self.status = Some(GoalStatus::Active);
        self.tokens_used = 0;
        self.time_used_seconds = 0;
        self.continuation_count = 0;
        self.started_at = Some(Instant::now());
        self.finished_at = None;
        self.evidence = None;
        self.blocker = None;
        self.completion_verification = None;
        self.progress_checklist = None;
        self.no_progress_rounds = 0;
        self.last_tool_error_fingerprint = None;
        self.repeated_error_rounds = 0;
        self.time_budget_seconds = None;
    }

    /// Set or clear the progress checklist shown in the goal contract.
    pub fn set_progress_checklist(&mut self, checklist: Option<String>) {
        self.progress_checklist = checklist.filter(|value| !value.trim().is_empty());
    }

    /// Set the wall-clock budget in seconds (wired into `decide_continuation`).
    pub fn set_time_budget_seconds(&mut self, seconds: Option<u64>) {
        self.time_budget_seconds = seconds;
    }

    /// Apply a per-turn progress signal to the circuit-breaker counters.
    ///
    /// Must be called inside a synchronous block — never hold the `Mutex`
    /// guard across an `.await` (see `ARCHITECTURE_STABILITY.md` §8.3).
    pub fn record_progress_signal(&mut self, signal: &ProgressSignal) {
        if !self.is_active() {
            return;
        }
        match signal {
            ProgressSignal::FileChanged => {
                self.no_progress_rounds = 0;
                self.last_tool_error_fingerprint = None;
                self.repeated_error_rounds = 0;
            }
            ProgressSignal::NoChange => {
                self.no_progress_rounds = self.no_progress_rounds.saturating_add(1);
            }
            ProgressSignal::ToolError { fingerprint } => {
                if self.last_tool_error_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                    self.repeated_error_rounds = self.repeated_error_rounds.saturating_add(1);
                } else {
                    self.repeated_error_rounds = 1;
                    self.last_tool_error_fingerprint = Some(fingerprint.clone());
                }
                // A tool error still counts as a turn that made no file progress.
                self.no_progress_rounds = self.no_progress_rounds.saturating_add(1);
            }
        }
    }

    /// Consecutive no-progress turns (exposed for `decide_continuation`).
    #[must_use]
    pub fn no_progress_rounds(&self) -> u32 {
        self.no_progress_rounds
    }

    /// Consecutive repeated same-error turns (exposed for `decide_continuation`).
    #[must_use]
    pub fn repeated_error_rounds(&self) -> u32 {
        self.repeated_error_rounds
    }

    /// Wall-clock budget in seconds, if set (exposed for `decide_continuation`).
    #[must_use]
    pub fn time_budget_seconds(&self) -> Option<u64> {
        self.time_budget_seconds
    }

    pub fn record_usage(&mut self, token_delta: u64, time_delta_seconds: u64) {
        if self.is_active() {
            self.tokens_used = self.tokens_used.saturating_add(token_delta);
            self.time_used_seconds = self.time_used_seconds.saturating_add(time_delta_seconds);
        }
    }

    pub fn record_continuation(&mut self) {
        if self.is_active() {
            self.continuation_count = self.continuation_count.saturating_add(1);
        }
    }

    pub fn mark_complete(
        &mut self,
        evidence: String,
        verification: GoalCompletionVerification,
    ) -> Result<(), &'static str> {
        if self.objective.is_none() {
            return Err("No active goal exists to complete.");
        }
        self.status = Some(GoalStatus::Complete);
        self.finished_at = Some(Instant::now());
        self.evidence = Some(evidence);
        self.blocker = None;
        self.completion_verification = Some(verification);
        Ok(())
    }

    pub fn mark_blocked(&mut self, blocker: String) -> Result<(), &'static str> {
        if self.objective.is_none() {
            return Err("No active goal exists to block.");
        }
        self.status = Some(GoalStatus::Blocked);
        self.finished_at = Some(Instant::now());
        self.blocker = Some(blocker);
        self.evidence = None;
        self.completion_verification = None;
        Ok(())
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub fn snapshot(&self) -> GoalSnapshot {
        GoalSnapshot {
            objective: self.objective.clone(),
            status: self
                .status
                .map(GoalStatus::as_str)
                .unwrap_or("none")
                .to_string(),
            token_budget: self.token_budget,
            tokens_used: self.tokens_used,
            time_used_seconds: self.time_used_seconds,
            continuation_count: self.continuation_count,
            elapsed_seconds: self.started_at.map(|started| started.elapsed().as_secs()),
            evidence: self.evidence.clone(),
            blocker: self.blocker.clone(),
            completion_verification: self.completion_verification.clone(),
            progress_checklist: self.progress_checklist.clone(),
            no_progress_rounds: self.no_progress_rounds,
            repeated_error_rounds: self.repeated_error_rounds,
            time_budget_seconds: self.time_budget_seconds,
        }
    }
}

/// Serializable tool output and prompt input for the current goal.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GoalSnapshot {
    pub objective: Option<String>,
    pub status: String,
    pub token_budget: Option<u32>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    pub continuation_count: u32,
    pub elapsed_seconds: Option<u64>,
    pub evidence: Option<String>,
    pub blocker: Option<String>,
    pub completion_verification: Option<GoalCompletionVerification>,
    pub progress_checklist: Option<String>,
    pub no_progress_rounds: u32,
    pub repeated_error_rounds: u32,
    pub time_budget_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalCompletionVerification {
    pub status: String,
    pub check: String,
    pub summary: String,
}

impl GoalSnapshot {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.objective.is_some() && self.status == GoalStatus::Active.as_str()
    }

    #[must_use]
    pub fn from_thread_goal(goal: &mimofan_protocol::ThreadGoal) -> Self {
        Self {
            objective: Some(goal.objective.clone()),
            status: thread_goal_status_as_goal_status(goal.status.clone())
                .as_str()
                .to_string(),
            token_budget: goal
                .token_budget
                .and_then(|value| u32::try_from(value.max(0)).ok()),
            tokens_used: u64::try_from(goal.tokens_used.max(0)).unwrap_or(u64::MAX),
            time_used_seconds: u64::try_from(goal.time_used_seconds.max(0)).unwrap_or(u64::MAX),
            continuation_count: u32::try_from(goal.continuation_count.max(0)).unwrap_or(u32::MAX),
            elapsed_seconds: None,
            evidence: None,
            blocker: None,
            completion_verification: None,
            progress_checklist: None,
            no_progress_rounds: 0,
            repeated_error_rounds: 0,
            time_budget_seconds: None,
        }
    }
}

#[must_use]
pub fn thread_goal_status_as_goal_status(status: mimofan_protocol::ThreadGoalStatus) -> GoalStatus {
    match status {
        mimofan_protocol::ThreadGoalStatus::Active => GoalStatus::Active,
        mimofan_protocol::ThreadGoalStatus::Paused => GoalStatus::Paused,
        mimofan_protocol::ThreadGoalStatus::Complete => GoalStatus::Complete,
        mimofan_protocol::ThreadGoalStatus::Blocked
        | mimofan_protocol::ThreadGoalStatus::UsageLimited
        | mimofan_protocol::ThreadGoalStatus::BudgetLimited => GoalStatus::Blocked,
    }
}

/// Render the continuation prompt injected when a goal is still active after a
/// turn. There is no run-level cap, so this shows progress (turn count, tokens)
/// rather than a "N/max" meter — the loop runs until done, blocked, or paused.
#[must_use]
pub fn render_continuation_prompt(snapshot: &GoalSnapshot, continuation_index: u32) -> String {
    let goal_json = serde_json::to_string_pretty(snapshot).unwrap_or_else(|_| "{}".to_string());
    format!(
        "{}\n\n## Active Goal State\n\n```json\n{}\n```\n\nContinuation pass #{}.\nIf the goal is complete, first run or cite a concrete verifier/check, then call `update_goal` with `status: \"complete\"`, concrete evidence, and `verification: {{\"status\":\"passed\",\"check\":\"...\",\"summary\":\"...\"}}`. If it is blocked, call `update_goal` with `status: \"blocked\"` and the blocker. Otherwise continue making progress toward the objective.",
        crate::prompts::GOAL_CONTINUATION_PROMPT.trim(),
        goal_json,
        continuation_index,
    )
}

fn lock_goal_state(
    state: &SharedGoalState,
) -> Result<std::sync::MutexGuard<'_, GoalState>, ToolError> {
    state
        .lock()
        .map_err(|_| ToolError::execution_failed("goal state lock poisoned"))
}

fn parse_token_budget(input: &Value) -> Result<Option<u32>, ToolError> {
    let Some(raw) = input.get("token_budget") else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let Some(value) = raw.as_u64() else {
        return Err(ToolError::invalid_input(
            "token_budget must be a non-negative integer",
        ));
    };
    u32::try_from(value)
        .map(Some)
        .map_err(|_| ToolError::invalid_input("token_budget is too large"))
}

fn parse_completion_verification(input: &Value) -> Result<GoalCompletionVerification, ToolError> {
    let Some(raw) = input.get("verification") else {
        return Err(ToolError::invalid_input(
            "verification is required when status is complete; run a verifier/check and pass verification: {status, check, summary}",
        ));
    };
    let verification: GoalCompletionVerification = serde_json::from_value(raw.clone())
        .map_err(|err| ToolError::invalid_input(format!("invalid verification: {err}")))?;
    if verification.status.trim() != "passed" {
        return Err(ToolError::invalid_input(
            "verification.status must be 'passed' before update_goal can mark a goal complete",
        ));
    }
    if verification.check.trim().is_empty() {
        return Err(ToolError::invalid_input("verification.check is required"));
    }
    if verification.summary.trim().is_empty() {
        return Err(ToolError::invalid_input("verification.summary is required"));
    }
    Ok(GoalCompletionVerification {
        status: "passed".to_string(),
        check: verification.check.trim().to_string(),
        summary: verification.summary.trim().to_string(),
    })
}

fn json_result(snapshot: &GoalSnapshot) -> Result<ToolResult, ToolError> {
    ToolResult::json(snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
}

pub struct CreateGoalTool {
    goal_state: SharedGoalState,
}

impl CreateGoalTool {
    #[must_use]
    pub fn new(goal_state: SharedGoalState) -> Self {
        Self { goal_state }
    }
}

#[async_trait]
impl ToolSpec for CreateGoalTool {
    fn name(&self) -> &'static str {
        "create_goal"
    }

    fn description(&self) -> &'static str {
        "Create the current runtime goal. Use this only when the user explicitly asks to pursue a persistent objective."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "objective": {
                    "type": "string",
                    "description": "The full objective to pursue. Keep the complete user goal, not a shortened one-turn version."
                },
                "token_budget": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Optional soft token budget for the goal."
                }
            },
            "required": ["objective"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let objective = required_str(&input, "objective")?.trim().to_string();
        if objective.is_empty() {
            return Err(ToolError::invalid_input("objective cannot be empty"));
        }
        let token_budget = parse_token_budget(&input)?;
        let snapshot = {
            let mut state = lock_goal_state(&self.goal_state)?;
            state.create(objective, token_budget);
            state.snapshot()
        };
        json_result(&snapshot)
    }
}

pub struct GetGoalTool {
    goal_state: SharedGoalState,
}

impl GetGoalTool {
    #[must_use]
    pub fn new(goal_state: SharedGoalState) -> Self {
        Self { goal_state }
    }
}

#[async_trait]
impl ToolSpec for GetGoalTool {
    fn name(&self) -> &'static str {
        "get_goal"
    }

    fn description(&self) -> &'static str {
        "Inspect the current runtime goal state, including objective, status, token budget, elapsed time, evidence, and blocker."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let snapshot = {
            let state = lock_goal_state(&self.goal_state)?;
            state.snapshot()
        };
        json_result(&snapshot)
    }
}

pub struct UpdateGoalTool {
    goal_state: SharedGoalState,
}

impl UpdateGoalTool {
    #[must_use]
    pub fn new(goal_state: SharedGoalState) -> Self {
        Self { goal_state }
    }
}

#[async_trait]
impl ToolSpec for UpdateGoalTool {
    fn name(&self) -> &'static str {
        "update_goal"
    }

    fn description(&self) -> &'static str {
        "Update the runtime goal completion gate. Only mark complete when the objective has verified evidence; mark blocked only after a real blocker prevents progress."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["complete", "blocked"],
                    "description": "Use complete only when the goal is fully satisfied; blocked when meaningful progress cannot continue. Pause, resume, and budget-limit states are controlled by the user or system."
                },
                "evidence": {
                    "type": "string",
                    "description": "Required when status is complete. Briefly cite the proof that the goal is done."
                },
                "verification": {
                    "type": "object",
                    "description": "Required when status is complete. A verifier-as-judge receipt from a concrete check, such as run_verifiers or an equivalent project-specific gate.",
                    "properties": {
                        "status": {
                            "type": "string",
                            "enum": ["passed"],
                            "description": "Must be passed before the goal can be marked complete."
                        },
                        "check": {
                            "type": "string",
                            "description": "The verifier/check that passed."
                        },
                        "summary": {
                            "type": "string",
                            "description": "Brief result summary from the verifier/check."
                        }
                    },
                    "required": ["status", "check", "summary"],
                    "additionalProperties": false
                },
                "blocker": {
                    "type": "string",
                    "description": "Required when status is blocked. Explain the condition preventing progress."
                },
                "objective": {
                    "type": "string",
                    "description": "Reserved for future host-controlled goal edits; ignored by update_goal."
                }
            },
            "required": ["status"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let status = required_str(&input, "status")?.trim().to_ascii_lowercase();
        let snapshot = {
            let mut state = lock_goal_state(&self.goal_state)?;
            match status.as_str() {
                "complete" => {
                    let evidence = input
                        .get("evidence")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    if evidence.is_empty() {
                        return Err(ToolError::invalid_input(
                            "evidence is required when status is complete",
                        ));
                    }
                    let verification = parse_completion_verification(&input)?;
                    state
                        .mark_complete(evidence, verification)
                        .map_err(ToolError::invalid_input)?;
                }
                "blocked" => {
                    let blocker = input
                        .get("blocker")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    if blocker.is_empty() {
                        return Err(ToolError::invalid_input(
                            "blocker is required when status is blocked",
                        ));
                    }
                    state
                        .mark_blocked(blocker)
                        .map_err(ToolError::invalid_input)?;
                }
                other => {
                    return Err(ToolError::invalid_input(format!(
                        "unsupported goal status '{other}'; update_goal can only mark complete or blocked"
                    )));
                }
            }
            state.snapshot()
        };
        json_result(&snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_state() -> GoalState {
        let mut s = GoalState::default();
        s.create("ship the refactor".to_string(), None);
        s
    }

    #[test]
    fn file_changed_resets_counters() {
        let mut s = active_state();
        s.record_progress_signal(&ProgressSignal::NoChange);
        s.record_progress_signal(&ProgressSignal::NoChange);
        assert_eq!(s.no_progress_rounds(), 2);
        s.record_progress_signal(&ProgressSignal::FileChanged);
        assert_eq!(s.no_progress_rounds(), 0);
        assert_eq!(s.repeated_error_rounds(), 0);
    }

    #[test]
    fn repeated_same_error_counts_then_resets_on_new() {
        let mut s = active_state();
        s.record_progress_signal(&ProgressSignal::ToolError {
            fingerprint: "E1".to_string(),
        });
        s.record_progress_signal(&ProgressSignal::ToolError {
            fingerprint: "E1".to_string(),
        });
        assert_eq!(s.repeated_error_rounds(), 2);
        // A different error resets the repeat counter to 1.
        s.record_progress_signal(&ProgressSignal::ToolError {
            fingerprint: "E2".to_string(),
        });
        assert_eq!(s.repeated_error_rounds(), 1);
        assert_eq!(s.last_tool_error_fingerprint.as_deref(), Some("E2"));
    }

    #[test]
    fn no_progress_also_increments_on_error() {
        let mut s = active_state();
        s.record_progress_signal(&ProgressSignal::ToolError {
            fingerprint: "E1".to_string(),
        });
        assert_eq!(s.no_progress_rounds(), 1);
    }
}
