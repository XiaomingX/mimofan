//! Security-audit tool: `security_audit`.
//!
//! Wires the `semgrep` helper library (`security_audit.rs`) into the tool
//! surface so the `security_auditor` personality's instruction to "drive
//! `semgrep`" (security_auditor.md L36) actually resolves to a callable tool.
//! The helper (`build_semgrep_command` / `run_semgrep_scan` /
//! `to_review_issue`) previously had ZERO callers — this is the documentation-lie
//! fix (plan `plans/12-security-detection-wiring.md`, Phase 1).
//!
//! The ToolSpec shape (input type, output type, capabilities, approval, async
//! execute signature) mirrors `call_graph.rs`; the sandbox-backend reuse +
//! fail-closed pattern mirrors `run_poc.rs` (see `context.sandbox_backend`,
//! run_poc.rs:135-159). We never shell out directly with
//! `std::process::Command` — `run_semgrep_scan` already executes through the
//! shared [`SandboxBackend`], so this tool only forwards the backend.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::security_audit::{self, SemgrepOptions};
use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const SECURITY_AUDIT_TOOL_NAME: &str = "security_audit";

/// Input for [`SecurityAuditTool`]. Mirrors [`SemgrepOptions`] field-for-field.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityAuditInput {
    /// Target directory or file to scan.
    target: String,
    /// Optional explicit config (rule pack). Defaults to `auto`.
    #[serde(default)]
    config: Option<String>,
    /// Extra CLI flags (e.g. `--timeout`, `--max-depth`). Appended verbatim.
    #[serde(default)]
    extra_flags: Vec<String>,
}

/// Output for [`SecurityAuditTool`]: normalized [`super::review::ReviewIssue`]s.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityAuditOutput {
    /// Number of findings returned.
    finding_count: usize,
    /// Normalized security findings for the unified `security_issues` channel.
    findings: Vec<super::review::ReviewIssue>,
}

/// Run `semgrep` (through the shared sandbox backend) against `target` and
/// report normalized security findings.
pub struct SecurityAuditTool;

#[async_trait]
impl ToolSpec for SecurityAuditTool {
    fn name(&self) -> &'static str {
        SECURITY_AUDIT_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Run `semgrep` against a target directory or file and report normalized \
         security findings. Provide `target` (the path to scan), optional `config` \
         (rule pack; defaults to semgrep's bundled registry + local rules), and \
         optional `extra_flags` (additional CLI flags appended verbatim, e.g. \
         `--timeout`). Requires a configured sandbox backend; fails closed when \
         none is available. Read-only: it never writes files."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Target directory or file to scan."
                },
                "config": {
                    "type": "string",
                    "description": "Optional explicit config (rule pack). Defaults to `auto` (semgrep's bundled registry + local rules)."
                },
                "extra_flags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Extra CLI flags (e.g. `--timeout`, `--max-depth`), appended verbatim."
                }
            },
            "required": ["target"],
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

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let parsed: SecurityAuditInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::invalid_input(format!("invalid security_audit input: {e}")))?;

        if parsed.target.trim().is_empty() {
            return Err(ToolError::invalid_input(
                "security_audit requires a non-empty 'target'",
            ));
        }

        // Fail-closed: no sandbox backend means we cannot execute `semgrep`.
        // Same pattern as run_poc.rs:135-139 — a single execution surface.
        let Some(backend) = context.sandbox_backend.as_ref() else {
            return Err(ToolError::not_available(
                "no sandbox backend configured; security_audit requires an execution backend",
            ));
        };

        let opts = SemgrepOptions {
            target: parsed.target,
            config: parsed.config,
            extra_flags: parsed.extra_flags,
        };

        // `backend` here is `&Arc<dyn SandboxBackend>`; deref to `&dyn`.
        let issues = security_audit::run_semgrep_scan(backend.as_ref(), &opts)
            .await
            .map_err(|e| ToolError::execution_failed(format!("security_audit failed: {e}")))?;

        let findings: Vec<super::review::ReviewIssue> =
            issues.iter().map(security_audit::to_review_issue).collect();

        let output = SecurityAuditOutput {
            finding_count: findings.len(),
            findings,
        };

        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refuses_without_backend() {
        // Build a minimal ToolContext with no sandbox backend (as unit tests
        // do) and confirm the tool fails closed with a clear message.
        let ctx = ToolContext::new(std::env::temp_dir());
        let tool = SecurityAuditTool;
        let input = json!({ "target": "." });
        let err = tool
            .execute(input, &ctx)
            .await
            .expect_err("security_audit must refuse when no sandbox backend is configured");
        let msg = err.to_string();
        assert!(
            msg.to_ascii_lowercase().contains("sandbox backend"),
            "error must mention sandbox backend, got: {msg}"
        );
    }

    #[tokio::test]
    async fn rejects_empty_target() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let tool = SecurityAuditTool;
        let input = json!({ "target": "  " });
        let err = tool
            .execute(input, &ctx)
            .await
            .expect_err("an empty target must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("non-empty 'target'"),
            "error must name the target field, got: {msg}"
        );
    }
}
