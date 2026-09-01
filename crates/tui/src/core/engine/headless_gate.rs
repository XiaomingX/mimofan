//! #863 — Headless unattended gate (total gatekeeper / umbrella).
//!
//! This is the umbrella gate that makes `--unattended` actually safe
//! end-to-end. It validates at startup that unattended mode is *coherent*:
//!
//! - **(a)** the engine must NOT block on human input — enforced by the
//!   `#853` `UnattendedPolicy` tool filter (applied at engine construction),
//! - **(b)** on any unrecoverable error the engine writes a structured
//!   failure event through `crate::core::engine::trace::SessionEventSink`
//!   (the single trajectory writer, `SessionEventKind::Error`) and exits
//!   non-zero rather than hanging,
//! - **(c)** the `TaskBudget` (#848) halts the run at exhaustion,
//! - **(d)** the resume path (#857) lets a crash be restarted.
//!
//! `HeadlessGate::validate` returns `Err` when unattended mode is enabled but
//! the configuration cannot guarantee a safe, terminating run. `write_failure`
//! appends a structured failure record to the configured failure log so a
//! headless supervisor can detect and act on crashes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::core::engine::trace::{SessionEvent, SessionEventKind, SessionEventSink};

/// Validation/coherence errors surfaced by [`HeadlessGate::validate`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HeadlessGateError {
    /// Attended tool use *and* unattended mode were both requested.
    #[error("unattended mode conflicts with interactive approval settings")]
    ConflictingApprovalMode,
    /// No bound could keep a headless run from running forever.
    #[error(
        "unattended mode requires a termination bound: set a task budget \
         (--task-budget-tokens / task_budget_tokens) or a max-turn cap \
         (--max-turns / max_steps)"
    )]
    MissingTerminationBound,
    /// No durable failure log path was configured.
    #[error(
        "unattended mode requires a failure log path (failure_log_path); \
         refusing to run headless without a crash record"
    )]
    MissingFailureLog,
}

/// The subset of engine configuration the gate inspects. Kept narrow so the
/// gate can be unit-tested without constructing a full [`EngineConfig`].
#[derive(Debug, Clone, Default)]
pub struct HeadlessGateConfig {
    /// Whether `--unattended` was requested.
    pub unattended: bool,
    /// Aggregate token budget for the goal, if any (#848).
    pub task_budget_tokens: Option<usize>,
    /// Maximum number of model steps before the run ends. `0` means unbounded.
    pub max_steps: u32,
    /// Path to which structured failure events are written (#863).
    pub failure_log_path: Option<PathBuf>,
}

/// The umbrella gate for unattended/headless runs.
#[derive(Debug, Clone, Default)]
pub struct HeadlessGate {
    config: HeadlessGateConfig,
    /// Resolved, absolute failure-log path (after defaulting + canonicalisation).
    resolved_failure_log: Option<PathBuf>,
}

impl HeadlessGate {
    /// Build a gate from the inspected configuration.
    #[must_use]
    pub fn new(config: HeadlessGateConfig) -> Self {
        Self {
            config,
            resolved_failure_log: None,
        }
    }

    /// Whether the gate is active (unattended mode requested).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.config.unattended
    }

    /// Resolve the failure log path: an explicit `failure_log_path` wins;
    /// otherwise unattended mode defaults it to
    /// `<workspace>/.mimofan/failures.jsonl`. Returns `None` when unattended is
    /// off (no failure log is needed for interactive runs).
    #[must_use]
    pub fn resolve_failure_log_path(&self, workspace: &Path) -> Option<PathBuf> {
        if !self.config.unattended {
            return None;
        }
        if let Some(p) = &self.config.failure_log_path {
            return Some(p.clone());
        }
        Some(workspace.join(".mimofan").join("failures.jsonl"))
    }

    /// Validate that unattended mode is coherent. Returns `Err` describing the
    /// first coherence violation. Call this *before* the engine starts running.
    pub fn validate(&mut self, workspace: &Path) -> Result<()> {
        if !self.config.unattended {
            // Interactive runs are never gated.
            return Ok(());
        }

        // (c)+(a) termination bound: a headless run must have *some* bound so it
        // cannot run forever. A task budget OR a max-turn cap both satisfy this.
        let has_budget = self.config.task_budget_tokens.is_some_and(|b| b > 0);
        let has_max_turns = self.config.max_steps > 0;
        if !has_budget && !has_max_turns {
            bail!(HeadlessGateError::MissingTerminationBound);
        }

        // (b) failure log: a headless crash must be recordable.
        let failure_log = self.resolve_failure_log_path(workspace);
        if failure_log.is_none() {
            bail!(HeadlessGateError::MissingFailureLog);
        }
        self.resolved_failure_log = failure_log;

        Ok(())
    }

    /// The failure-log path resolved during [`validate`]. Callers should invoke
    /// [`validate`] first; this returns `None` if validation has not run or
    /// unattended mode is off.
    #[must_use]
    pub fn failure_log_path(&self) -> Option<&Path> {
        self.resolved_failure_log.as_deref()
    }

    /// Append a structured failure event to the failure log.
    ///
    /// The record goes through [`SessionEventSink`] (loop v1 / T3: the single
    /// trajectory/failure writer; the old `EventLog` was removed) opened at the
    /// failure-log path with redaction disabled so external headless
    /// supervisors can read the failure message. The failure `code` is carried
    /// in the event's `exit_status` field and the message in `text`.
    ///
    /// A missing parent directory is created, and any I/O error is returned
    /// (not silently swallowed) so the caller can still exit non-zero.
    /// Returns 0 on success (sequence tracking lives in the session trajectory).
    pub fn write_failure(
        &self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<u64> {
        let path = self
            .failure_log_path()
            .ok_or_else(|| anyhow!(HeadlessGateError::MissingFailureLog))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating failure log dir {}", parent.display()))?;
        }
        let sink =
            SessionEventSink::open_at(path).context("opening failure-log SessionEventSink")?;
        let event = SessionEvent {
            kind: SessionEventKind::Error,
            turn: 0,
            ts: chrono::Utc::now().to_rfc3339(),
            text: Some(message.into()),
            tool_name: None,
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: Some("system".to_string()),
            tool_result: None,
            tool_call_id: None,
            session_id: None,
            model: None,
            exit_status: Some(code.into()),
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        sink.emit(&event)
            .context("writing failure event to failure log")?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_workspace() -> PathBuf {
        std::env::temp_dir().join("mimofan_headless_gate_test")
    }

    #[test]
    fn interactive_mode_is_never_gated() {
        // Even with no bounds, an attended run is allowed.
        let mut gate = HeadlessGate::new(HeadlessGateConfig {
            unattended: false,
            task_budget_tokens: None,
            max_steps: 0,
            failure_log_path: None,
        });
        assert!(gate.validate(Path::new("/tmp/x")).is_ok());
        assert!(!gate.is_active());
    }

    #[test]
    fn unattended_without_bounds_is_rejected() {
        let mut gate = HeadlessGate::new(HeadlessGateConfig {
            unattended: true,
            task_budget_tokens: None,
            max_steps: 0,
            failure_log_path: Some(PathBuf::from("/tmp/failures.jsonl")),
        });
        let err = gate.validate(Path::new("/tmp/x")).unwrap_err();
        assert_eq!(
            err.downcast_ref::<HeadlessGateError>(),
            Some(&HeadlessGateError::MissingTerminationBound)
        );
    }

    #[test]
    fn unattended_defaults_failure_log_when_unset() {
        // When no explicit failure log is configured, unattended mode defaults
        // it to `<workspace>/.mimofan/failures.jsonl` so the run still has a
        // crash record. Validation must therefore succeed and resolve a path.
        let ws = temp_workspace();
        let mut gate = HeadlessGate::new(HeadlessGateConfig {
            unattended: true,
            task_budget_tokens: Some(1000),
            max_steps: 0,
            failure_log_path: None,
        });
        assert!(gate.validate(&ws).is_ok());
        let path = gate.failure_log_path().expect("failure log resolved");
        assert!(path.ends_with(".mimofan/failures.jsonl"));
    }

    #[test]
    fn unattended_with_budget_is_accepted() {
        let ws = temp_workspace();
        let mut gate = HeadlessGate::new(HeadlessGateConfig {
            unattended: true,
            task_budget_tokens: Some(1000),
            max_steps: 0,
            failure_log_path: Some(ws.join("failures.jsonl")),
        });
        assert!(gate.validate(&ws).is_ok());
        assert!(gate.failure_log_path().is_some());
    }

    #[test]
    fn unattended_with_max_turns_is_accepted() {
        let ws = temp_workspace();
        let mut gate = HeadlessGate::new(HeadlessGateConfig {
            unattended: true,
            task_budget_tokens: None,
            max_steps: 50,
            failure_log_path: Some(ws.join("failures.jsonl")),
        });
        assert!(gate.validate(&ws).is_ok());
    }

    #[test]
    fn unattended_defaults_failure_log_to_workspace() {
        let ws = temp_workspace();
        let gate = HeadlessGate::new(HeadlessGateConfig {
            unattended: true,
            task_budget_tokens: Some(100),
            max_steps: 0,
            failure_log_path: None,
        });
        let path = gate.resolve_failure_log_path(&ws).unwrap();
        assert!(path.ends_with(".mimofan/failures.jsonl"));
    }

    #[test]
    fn write_failure_emits_session_event_through_sink() {
        // T3 normalization: the failure record is emitted via
        // SessionEventSink (single trajectory writer) in SessionEvent schema —
        // no more separate EventLog writer.
        let dir = tempfile::TempDir::new().unwrap();
        let failure_log = dir.path().join("failures.jsonl");
        let mut gate = HeadlessGate::new(HeadlessGateConfig {
            unattended: true,
            task_budget_tokens: Some(100),
            max_steps: 0,
            failure_log_path: Some(failure_log.clone()),
        });
        gate.validate(dir.path()).unwrap();

        gate.write_failure("turn_failed", "boom message").unwrap();

        let events = crate::core::engine::trace::read_session(&failure_log);
        assert_eq!(events.len(), 1, "one failure event written");
        let ev = &events[0];
        assert_eq!(ev.kind, SessionEventKind::Error);
        assert_eq!(ev.exit_status.as_deref(), Some("turn_failed"));
        assert_eq!(ev.text.as_deref(), Some("boom message"));
        assert_eq!(ev.source.as_deref(), Some("system"));
    }
}
