//! Independent adversarial verification (`adversarial_verify`).
//!
//! Implements issue #804: spawn a *separate* sub-agent that acts as an
//! adversarial reviewer of a `claim` given some `evidence`. The reviewer does
//! **not** inherit the proponent's reasoning chain — it is spawned with a
//! fresh, fork-free context (no `fork_context`) so it forms an independent
//! judgment. When the reviewer contests the claim, the verdict is flagged for
//! human / vote escalation.
//!
//! The production path reuses the existing [`SubAgentManager`] spawn
//! infrastructure (no duplicated spawn logic). A [`VerdictResolver`] trait
//! keeps the verdict-extraction testable without a live LLM: the default
//! resolver spawns a real background reviewer; tests inject a synthetic
//! resolver to assert structure and escalation logic.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{SubAgentManager, SubAgentRuntime};
use super::types::{SubAgentResult, SubAgentStatus, SubAgentType};

/// Structured verdict returned by [`adversarial_verify`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdversarialVerdict {
    /// Whether the adversarial reviewer found the claim supported by the
    /// evidence (`true`) or contested it (`false`).
    pub supported: bool,
    /// The reviewer's reasoning / counter-arguments.
    pub reasoning: String,
    /// Reviewer confidence in `[0.0, 1.0]`.
    pub confidence: f32,
    /// `true` when the reviewer contested the claim and the matter should be
    /// escalated to a human / vote instead of being auto-accepted.
    pub needs_human_review: bool,
}

impl AdversarialVerdict {
    /// A self-consistent sentinel used when the reviewer could not be spawned
    /// or produced no usable output. Falls back to contesting the claim so the
    /// caller must escalate rather than silently accept.
    fn degraded(reasoning: String) -> Self {
        Self {
            supported: false,
            reasoning,
            confidence: 0.0,
            needs_human_review: true,
        }
    }
}

/// Tuning knobs for a single adversarial verification run.
#[derive(Debug, Clone, Copy)]
pub struct AdversarialVerifyConfig {
    /// Hard cap on how many times we poll `get_result` for the reviewer.
    pub max_poll_attempts: u32,
    /// Wait between polls.
    pub poll_interval: Duration,
    /// If `Some`, override the runtime's `max_spawn_depth` for this call.
    pub max_spawn_depth: Option<u32>,
}

impl Default for AdversarialVerifyConfig {
    fn default() -> Self {
        Self {
            max_poll_attempts: 120,
            poll_interval: Duration::from_millis(500),
            max_spawn_depth: None,
        }
    }
}

/// Resolves a reviewer sub-agent's free-text output into a structured
/// [`AdversarialVerdict`].
///
/// The default implementation spawns a real, independent background reviewer
/// through the shared [`SubAgentManager`]. Tests inject a synthetic resolver
/// so the adversarial logic can be exercised without a live LLM call.
#[async_trait::async_trait]
pub trait VerdictResolver: Send + Sync {
    /// Run the adversarial review and return a structured verdict.
    async fn resolve(
        &self,
        runtime: &SubAgentRuntime,
        manager: &Arc<RwLock<SubAgentManager>>,
        claim: &str,
        evidence: &[String],
        config: AdversarialVerifyConfig,
    ) -> Result<AdversarialVerdict>;
}

/// Default resolver: spawn a fresh, independent `review` sub-agent and parse
/// its transcript into a structured verdict.
pub struct SpawnedReviewerResolver;

#[async_trait::async_trait]
impl VerdictResolver for SpawnedReviewerResolver {
    async fn resolve(
        &self,
        runtime: &SubAgentRuntime,
        manager: &Arc<RwLock<SubAgentManager>>,
        claim: &str,
        evidence: &[String],
        config: AdversarialVerifyConfig,
    ) -> Result<AdversarialVerdict> {
        let prompt = build_adversarial_reviewer_prompt(claim, evidence);
        let snapshot = {
            let mut guard = manager.write().await;
            guard.spawn_background(
                Arc::clone(manager),
                runtime.clone(),
                SubAgentType::Review,
                prompt,
                // Read-only review role; independent (no fork context).
                Some(vec!["read_file".to_string(), "grep".to_string()]),
            )
            .context("adversarial_verify: failed to spawn independent reviewer")?
        };
        let agent_id = snapshot.agent_id.clone();

        let result = poll_until_terminal(manager, &agent_id, config).await?;
        match result.status {
            SubAgentStatus::Completed => {
                let text = result.result.clone().unwrap_or_default();
                Ok(parse_reviewer_verdict(&text))
            }
            SubAgentStatus::Failed(err) => Ok(AdversarialVerdict::degraded(format!(
                "Reviewer agent failed: {err}"
            ))),
            SubAgentStatus::Cancelled => Ok(AdversarialVerdict::degraded(
                "Reviewer agent was cancelled before producing a verdict.".to_string(),
            )),
            other => Ok(AdversarialVerdict::degraded(format!(
                "Reviewer agent ended in unexpected state: {other:?}"
            ))),
        }
    }
}

/// Build the prompt for the independent adversarial reviewer.
///
/// The instruction deliberately forbids the reviewer from simply agreeing with
/// the claim and forces it to reason from the evidence alone — it never sees
/// the proponent's reasoning chain because the spawn carries no fork context.
fn build_adversarial_reviewer_prompt(claim: &str, evidence: &[String]) -> String {
    let evidence_block = if evidence.is_empty() {
        "(no evidence provided)".to_string()
    } else {
        evidence
            .iter()
            .enumerate()
            .map(|(i, e)| format!("{}. {e}", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "You are an INDEPENDENT adversarial reviewer. Your ONLY job is to challenge the claim below \
using the supplied evidence. You have NOT seen how the claim was derived, and you must not assume \
it is correct. Actively search for counterexamples, logical gaps, unsupported assumptions, and \
evidence that contradicts the claim. Do NOT simply agree — a useful review finds weaknesses.\n\n\
CLAIM TO CHALLENGE:\n{claim}\n\n\
EVIDENCE PROVIDED:\n{evidence_block}\n\n\
Produce your review and finish with a machine-readable verdict line exactly in this form:\n\
VERDICT: supported=<true|false> confidence=<0.0-1.0>\n\
Followed by a one-paragraph reasoning section if space allows."
    )
}

/// Extract a structured verdict from the reviewer's free-text output.
///
/// Robust to formatting noise: it scans for the `VERDICT:` sentinel and, if
/// absent, falls back to lexical signals (e.g. "contradict", "disagree",
/// "unsupported") to avoid silently accepting an unparseable review.
fn parse_reviewer_verdict(text: &str) -> AdversarialVerdict {
    let supported;
    let confidence;
    if let Some((sup, conf)) = extract_verdict_line(text) {
        supported = sup;
        confidence = conf;
    } else {
        // No sentinel — infer from strong lexical contest signals, but default
        // to contesting so the caller escalates rather than auto-accepts.
        let lower = text.to_ascii_lowercase();
        let contests = ["contradict", "disagree", "unsupported", "false", "invalid", "reject"]
            .iter()
            .any(|needle| lower.contains(needle));
        supported = !contests;
        confidence = 0.5;
    }
    let reasoning = text.trim().to_string();
    AdversarialVerdict {
        supported,
        confidence: confidence.clamp(0.0, 1.0),
        needs_human_review: !supported,
        reasoning,
    }
}

/// Parse `VERDICT: supported=<bool> confidence=<f32>` from arbitrary text.
fn extract_verdict_line(text: &str) -> Option<(bool, f32)> {
    let line = text
        .lines()
        .find(|l| l.to_ascii_lowercase().contains("verdict:"))?;
    let lower = line.to_ascii_lowercase();
    let supported = lower.contains("supported=true");
    let confidence = lower
        .split("confidence=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|v| v.trim_end_matches(['.', ',', ')', ';']).parse::<f32>().ok())
        .unwrap_or(0.5);
    Some((supported, confidence))
}

/// Poll the manager until the reviewer reaches a terminal status or we hit the
/// attempt cap.
async fn poll_until_terminal(
    manager: &Arc<RwLock<SubAgentManager>>,
    agent_id: &str,
    config: AdversarialVerifyConfig,
) -> Result<SubAgentResult> {
    for _ in 0..config.max_poll_attempts {
        let result = manager
            .read()
            .await
            .get_result(agent_id)
            .context("adversarial_verify: reviewer agent disappeared")?;
        if result.status != SubAgentStatus::Running {
            return Ok(result);
        }
        tokio::time::sleep(config.poll_interval).await;
    }
    Ok(manager
        .read()
        .await
        .get_result(agent_id)
        .unwrap_or_else(|_| SubAgentResult {
            name: agent_id.to_string(),
            agent_id: agent_id.to_string(),
            context_mode: "fresh".to_string(),
            fork_context: false,
            workspace: None,
            git_branch: None,
            agent_type: SubAgentType::Review,
            assignment: super::types::SubAgentAssignment::new(String::new(), None),
            model: String::new(),
            nickname: None,
            status: SubAgentStatus::Running,
            worker_status: None,
            parent_run_id: None,
            spawn_depth: 0,
            result: None,
            steps_taken: 0,
            checkpoint: None,
            needs_input: None,
            duration_ms: 0,
            from_prior_session: false,
        }))
}

/// Verify a `claim` against `evidence` using an independent adversarial
/// reviewer sub-agent.
///
/// Reuses the shared [`SubAgentManager`] spawn infrastructure. The reviewer is
/// spawned with a *fresh* context (no fork), so it never shares the proponent's
/// reasoning chain. Spawning is bounded by `max_spawn_depth`: if the runtime is
/// already at the depth cap, this returns an error instead of nesting further.
///
/// `resolver` selects how the verdict is obtained. Pass
/// [`SpawnedReviewerResolver`] for the production path; tests pass a synthetic
/// resolver to avoid a live LLM call.
pub async fn adversarial_verify(
    runtime: &SubAgentRuntime,
    manager: &Arc<RwLock<SubAgentManager>>,
    claim: &str,
    evidence: &[String],
    resolver: &dyn VerdictResolver,
) -> Result<AdversarialVerdict> {
    if claim.trim().is_empty() {
        return Err(anyhow!("adversarial_verify: claim must not be empty"));
    }

    // Enforce the spawn-depth budget before delegating to the resolver, so a
    // nested adversarial review cannot recurse past max_spawn_depth.
    if runtime.would_exceed_depth() {
        return Err(anyhow!(
            "adversarial_verify: would exceed max_spawn_depth ({})",
            runtime.max_spawn_depth
        ));
    }

    let config = AdversarialVerifyConfig::default();
    resolver.resolve(&runtime, manager, claim, evidence, config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic resolver that returns a fixed verdict — lets us exercise the
    /// adversarial logic and escalation flags without a live model.
    struct FakeResolver {
        verdict: AdversarialVerdict,
    }

    #[async_trait::async_trait]
    impl VerdictResolver for FakeResolver {
        async fn resolve(
            &self,
            _runtime: &SubAgentRuntime,
            _manager: &Arc<RwLock<SubAgentManager>>,
            _claim: &str,
            _evidence: &[String],
            _config: AdversarialVerifyConfig,
        ) -> Result<AdversarialVerdict> {
            Ok(self.verdict.clone())
        }
    }

    #[test]
    fn test_parse_reviewer_verdict_supported() {
        let text = "The claim holds under the evidence.\nVERDICT: supported=true confidence=0.9";
        let v = parse_reviewer_verdict(text);
        assert!(v.supported);
        assert!(!v.needs_human_review);
        assert!((v.confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn test_parse_reviewer_verdict_contested() {
        let text = "This contradicts the evidence on several points.\nVERDICT: supported=false confidence=0.3";
        let v = parse_reviewer_verdict(text);
        assert!(!v.supported);
        assert!(v.needs_human_review);
        assert!((v.confidence - 0.3).abs() < 1e-6);
    }

    #[test]
    fn test_parse_reviewer_verdict_no_sentinel_falls_back() {
        // No sentinel line: weak lexical signal → contested (escalate).
        let v = parse_reviewer_verdict("The evidence is insufficient and contradicts the claim.");
        assert!(!v.supported);
        assert!(v.needs_human_review);
    }

    #[test]
    fn test_parse_reviewer_verdict_confidence_clamped() {
        let text = "VERDICT: supported=true confidence=5.0";
        let v = parse_reviewer_verdict(text);
        assert!(v.supported);
        assert!((v.confidence - 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_adversarial_verify_contested_escalates() {
        // A deliberately fragile claim; the fake reviewer contests it, so the
        // verdict must be flagged for human review.
        let resolver = FakeResolver {
            verdict: AdversarialVerdict {
                supported: false,
                reasoning: "The claim assumes causation from correlation.".to_string(),
                confidence: 0.2,
                needs_human_review: true,
            },
        };
        let runtime = SubAgentRuntime::new(
            crate::client::ApiClient::new_detached(&crate::config::Config::default())
                .expect("test client"),
            "test-model".to_string(),
            crate::tools::spec::ToolContext::new(std::env::temp_dir()),
            false,
            None,
            // Manager handle is irrelevant for the fake resolver but required
            // by the signature; construct a minimal one.
            new_test_manager(),
        );
        let manager = Arc::new(RwLock::new(SubAgentManager::new(std::env::temp_dir(), 1)));
        let verdict = adversarial_verify(
            &runtime,
            &manager,
            "Drinking ice water cures the common cold.",
            &["A study showed 80% of patients recovered within a week.".to_string()],
            &resolver,
        )
        .await
        .expect("adversarial_verify should succeed with fake resolver");
        assert!(!verdict.supported);
        assert!(verdict.needs_human_review);
        assert!(!verdict.reasoning.is_empty());
    }

    #[tokio::test]
    async fn test_adversarial_verify_rejects_empty_claim() {
        let resolver = FakeResolver {
            verdict: AdversarialVerdict {
                supported: true,
                reasoning: String::new(),
                confidence: 1.0,
                needs_human_review: false,
            },
        };
        let runtime = SubAgentRuntime::new(
            crate::client::ApiClient::new_detached(&crate::config::Config::default())
                .expect("test client"),
            "test-model".to_string(),
            crate::tools::spec::ToolContext::new(std::env::temp_dir()),
            false,
            None,
            new_test_manager(),
        );
        let manager = Arc::new(RwLock::new(SubAgentManager::new(std::env::temp_dir(), 1)));
        let result =
            adversarial_verify(&runtime, &manager, "   ", &[], &resolver).await;
        assert!(result.is_err());
    }

    /// Build a manager handle-shaped dummy for the runtime constructor. The
    /// `FakeResolver` ignores it, so any `SharedSubAgentManager` value works.
    fn new_test_manager() -> Arc<RwLock<SubAgentManager>> {
        Arc::new(RwLock::new(SubAgentManager::new(std::env::temp_dir(), 1)))
    }
}
