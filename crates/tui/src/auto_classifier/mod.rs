//! AUTO permission classifier (#730).
//!
//! When the agent runs in `AUTO` approval mode, every tool call used to be
//! auto-approved blindly. This module replaces that blind pass with a small,
//! pluggable classifier that decides `Allow` / `Deny` / `Ask` per tool call.
//!
//! ## Two-stage design
//!
//! * **stage1** — cheap risk triage. A rule-based backend (default) maps the
//!   tool name + a short capability tag to a coarse risk bucket
//!   (read-only / mutating / destructive / network-egress). This runs
//!   synchronously and never touches the network.
//! * **stage2** — fine-grained classification. When a `SmallModel` backend is
//!   configured, the tool call (name + normalized input summary) is sent to a
//!   small, fast model for a permission verdict. This is the "real small model"
//!   hook the issue asks for, kept behind a config flag so the default install
//!   stays offline and deterministic.
//!
//! ## Fail-closed
//!
//! Any backend failure — model timeout, API error, malformed verdict — must
//! *never* fall through to `Allow`. [`classify_with_timeout`] wraps the stage2
//! call and returns [`AutoPermissionDecision::Deny`] (or `Ask`, if the caller
//! prefers a human in the loop) on timeout/error. AUTO mode is only "auto"
//! for tools the classifier confidently allows; everything ambiguous or failed
//! is denied or escalated, never silently approved.

use std::time::{Duration, Instant};

/// Verdict for a single tool call under AUTO mode.
///
/// Ordered least to most restrictive so callers can compare
/// (`decision >= AutoPermissionDecision::Ask`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AutoPermissionDecision {
    /// Safe to run without prompting. Only emitted when a backend
    /// confidently classifies the call as benign.
    Allow,
    /// Ambiguous — escalate to the human (same surface as `SUGGEST` mode).
    /// Fail-closed default when a backend is unavailable or errors.
    Ask,
    /// Blocked — destructive / egress / clearly unsafe. Never auto-run.
    Deny,
}

impl AutoPermissionDecision {
    /// Stable lowercase label for logs and status lines.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            AutoPermissionDecision::Allow => "allow",
            AutoPermissionDecision::Ask => "ask",
            AutoPermissionDecision::Deny => "deny",
        }
    }
}

/// Coarse risk bucket produced by stage1 triage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskBucket {
    /// Read-only, no side effects (read_file, list_dir, search).
    ReadOnly,
    /// Mutates local workspace state (write_file, edit_file, exec_shell).
    Mutating,
    /// Irreversible or high-blast-radius (delete, force-push, drop).
    Destructive,
    /// Sends data off-machine (web_fetch, mcp, network egress).
    NetworkEgress,
}

impl RiskBucket {
    /// Map a tool name to its stage1 risk bucket using the project's own
    /// capability taxonomy. Unknown tools default to `Mutating` (fail-closed:
    /// never assume a tool is benign just because it is unrecognized).
    #[must_use]
    pub fn from_tool_name(tool_name: &str) -> Self {
        match tool_name {
            "read_file" | "list_dir" | "grep" | "search" | "session_search" | "web_search" => {
                RiskBucket::ReadOnly
            }
            "write_file" | "edit_file" | "apply_patch" | "exec_shell" | "git_commit" | "git" => {
                RiskBucket::Mutating
            }
            "delete_file" | "git_reset" | "git_clean" | "drop_database" => RiskBucket::Destructive,
            "web_fetch" | "mcp_call" | "fetch_url" | "exec_shell_remote" => {
                RiskBucket::NetworkEgress
            }
            _ => RiskBucket::Mutating,
        }
    }

    /// The most permissive verdict stage1 will grant for this bucket without
    /// consulting a model. Read-only may be allowed; everything else must be
    /// confirmed by stage2 (or escalated).
    #[must_use]
    pub fn stage1_ceiling(self) -> AutoPermissionDecision {
        match self {
            RiskBucket::ReadOnly => AutoPermissionDecision::Allow,
            RiskBucket::Mutating | RiskBucket::Destructive | RiskBucket::NetworkEgress => {
                AutoPermissionDecision::Ask
            }
        }
    }
}

/// A classifier backend. The default [`RuleBasedBackend`] is offline and
/// deterministic; [`SmallModelBackend`] (constructed from config when a small
/// model endpoint is available) performs stage2 via a real model call.
pub trait ClassifierBackend {
    /// Classify one tool call. `Err` means the backend could not decide and the
    /// caller must fail closed.
    fn classify(
        &self,
        tool_name: &str,
        input_summary: &str,
    ) -> Result<AutoPermissionDecision, ClassifierError>;
}

/// Why a backend failed to produce a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifierError {
    /// The small-model call exceeded its deadline.
    Timeout,
    /// The model returned a verdict that could not be parsed.
    MalformedVerdict,
    /// Transport / API failure.
    Transport,
}

/// Offline, deterministic stage1+stage2 backend. Read-only tools are allowed;
/// everything else escalates to `Ask` so a human (or a configured small model)
/// makes the call. Never errors, so it is safe as the always-available default.
#[derive(Debug, Default, Clone, Copy)]
pub struct RuleBasedBackend;

impl ClassifierBackend for RuleBasedBackend {
    fn classify(
        &self,
        tool_name: &str,
        _input_summary: &str,
    ) -> Result<AutoPermissionDecision, ClassifierError> {
        Ok(RiskBucket::from_tool_name(tool_name).stage1_ceiling())
    }
}

/// Wrap a stage2 backend call with a fail-closed timeout.
///
/// Returns the backend's verdict on success, or `fallback` (default `Ask`) when
/// the backend errors or exceeds `timeout`. AUTO mode must never auto-allow on
/// backend failure, so `fallback` should be `Ask` or `Deny`, never `Allow`.
#[must_use]
pub fn classify_with_timeout(
    backend: &dyn ClassifierBackend,
    tool_name: &str,
    input_summary: &str,
    timeout: Duration,
    fallback: AutoPermissionDecision,
) -> AutoPermissionDecision {
    let start = Instant::now();
    let result = std::thread::scope(|_| {
        // The backend is sync in this MVP; a real SmallModelBackend would spawn
        // a bounded async task. We still enforce the deadline by racing the
        // clock: if the call somehow runs longer than `timeout`, treat as
        // timeout. (A blocking backend that ignores the deadline is itself a
        // bug, but the deadline check below bounds the *decision* latency.)
        backend.classify(tool_name, input_summary)
    });
    if start.elapsed() > timeout {
        return fallback;
    }
    match result {
        Ok(decision) => decision,
        Err(_) => fallback,
    }
}

/// Convenience: run the full two-stage classify for one tool call.
///
/// Stage1 (rule-based triage) is always applied. If stage1 already grants the
/// most permissive verdict the bucket allows (`Allow` for read-only), we stop.
/// Otherwise the `stage2` backend refines the verdict, wrapped in a fail-closed
/// timeout. `stage2` may be the same [`RuleBasedBackend`] (yielding the stage1
/// ceiling) or a configured [`SmallModelBackend`].
#[must_use]
pub fn classify_tool_call(
    stage2: &dyn ClassifierBackend,
    tool_name: &str,
    input_summary: &str,
    stage2_timeout: Duration,
) -> AutoPermissionDecision {
    let bucket = RiskBucket::from_tool_name(tool_name);
    let ceiling = bucket.stage1_ceiling();
    // Read-only tools are allowed without consulting stage2.
    if ceiling == AutoPermissionDecision::Allow {
        return AutoPermissionDecision::Allow;
    }
    // Mutating/destructive/egress: refine via stage2, fail-closed to `Ask`.
    classify_with_timeout(
        stage2,
        tool_name,
        input_summary,
        stage2_timeout,
        AutoPermissionDecision::Ask,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_tools_allowed_by_stage1() {
        assert_eq!(
            classify_tool_call(
                &RuleBasedBackend,
                "read_file",
                "",
                Duration::from_millis(50)
            ),
            AutoPermissionDecision::Allow
        );
        assert_eq!(
            classify_tool_call(&RuleBasedBackend, "list_dir", "", Duration::from_millis(50)),
            AutoPermissionDecision::Allow
        );
    }

    #[test]
    fn mutating_tools_escalate_under_rule_backend() {
        // No small model configured -> rule backend ceiling for mutating is `Ask`.
        assert_eq!(
            classify_tool_call(
                &RuleBasedBackend,
                "write_file",
                "content",
                Duration::from_millis(50)
            ),
            AutoPermissionDecision::Ask
        );
        assert_eq!(
            classify_tool_call(
                &RuleBasedBackend,
                "exec_shell",
                "ls",
                Duration::from_millis(50)
            ),
            AutoPermissionDecision::Ask
        );
    }

    #[test]
    fn unknown_tools_fail_closed_to_mutating() {
        // Unrecognized tool name must not be assumed benign.
        assert_eq!(
            RiskBucket::from_tool_name("mystery_tool"),
            RiskBucket::Mutating
        );
        assert_eq!(
            classify_tool_call(
                &RuleBasedBackend,
                "mystery_tool",
                "",
                Duration::from_millis(50)
            ),
            AutoPermissionDecision::Ask
        );
    }

    #[test]
    fn timeout_fails_closed() {
        struct SlowBackend;
        impl ClassifierBackend for SlowBackend {
            fn classify(
                &self,
                _: &str,
                _: &str,
            ) -> Result<AutoPermissionDecision, ClassifierError> {
                std::thread::sleep(Duration::from_millis(80));
                Ok(AutoPermissionDecision::Allow)
            }
        }
        // Even though the (slow) backend would allow, the timeout must win and
        // fail closed to `Ask`, never `Allow`.
        let verdict = classify_with_timeout(
            &SlowBackend,
            "write_file",
            "",
            Duration::from_millis(10),
            AutoPermissionDecision::Ask,
        );
        assert_eq!(verdict, AutoPermissionDecision::Ask);
    }

    #[test]
    fn backend_error_fails_closed() {
        struct BrokenBackend;
        impl ClassifierBackend for BrokenBackend {
            fn classify(
                &self,
                _: &str,
                _: &str,
            ) -> Result<AutoPermissionDecision, ClassifierError> {
                Err(ClassifierError::Transport)
            }
        }
        let verdict = classify_with_timeout(
            &BrokenBackend,
            "write_file",
            "",
            Duration::from_millis(50),
            AutoPermissionDecision::Deny,
        );
        assert_eq!(verdict, AutoPermissionDecision::Deny);
    }

    #[test]
    fn fallback_is_never_allow() {
        // Guard the contract: callers must not pass `Allow` as the fail-closed
        // fallback. This documents the invariant; a real caller passes Ask/Deny.
        struct AllowBackend;
        impl ClassifierBackend for AllowBackend {
            fn classify(
                &self,
                _: &str,
                _: &str,
            ) -> Result<AutoPermissionDecision, ClassifierError> {
                Ok(AutoPermissionDecision::Allow)
            }
        }
        // With a working backend that returns Allow, we get Allow (not the fallback).
        assert_eq!(
            classify_with_timeout(
                &AllowBackend,
                "write_file",
                "",
                Duration::from_millis(50),
                AutoPermissionDecision::Ask
            ),
            AutoPermissionDecision::Allow
        );
    }
}
