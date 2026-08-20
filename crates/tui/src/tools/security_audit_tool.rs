//! Security-audit tool: `security_audit`.
//!
//! Wraps `security_audit::run_semgrep_scan` so the model can drive semgrep SAST
//! through the shared [`SandboxBackend`]. This closes the gap where the
//! `security_auditor` personality claimed it could "drive semgrep" but no
//! model-callable tool existed. The tool is read-only: it never executes code
//! directly (semgrep runs inside the sandbox) and does not write files, so it
//! is registered as `Auto` approval and supports parallel invocation.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::review::ReviewIssue;
use super::security_audit::{SemgrepOptions, run_semgrep_scan, to_review_issue};
use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const SECURITY_AUDIT_TOOL_NAME: &str = "security_audit";

/// Run a semgrep SAST scan over a target and return normalized findings.
pub struct SecurityAuditTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityAuditInput {
    /// Directory or file to scan with semgrep. Required.
    #[serde(default)]
    target: String,
    /// Optional explicit semgrep config/rule pack. Defaults to `auto`.
    #[serde(default)]
    config: Option<String>,
    /// Extra semgrep CLI flags, e.g. `--timeout 60`. Appended verbatim.
    #[serde(default)]
    extra_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityAuditOutput {
    target: String,
    findings: Vec<ReviewIssue>,
}

#[async_trait]
impl ToolSpec for SecurityAuditTool {
    fn name(&self) -> &'static str {
        SECURITY_AUDIT_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Run a semgrep SAST scan over a directory or file (through the shared sandbox \
         backend) and return the normalized security findings. Read-only: semgrep runs \
         inside the sandbox and no files are written by this tool."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Directory or file to scan with semgrep."
                },
                "config": {
                    "type": "string",
                    "description": "Optional explicit semgrep config/rule pack. Defaults to auto."
                },
                "extra_flags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Extra semgrep CLI flags, e.g. --timeout 60."
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

        let issues = run_semgrep_scan(backend.as_ref(), &opts)
            .await
            .map_err(|e| {
                ToolError::execution_failed(format!("security_audit failed: {e}"))
            })?;

        let findings: Vec<ReviewIssue> = issues.iter().map(to_review_issue).collect();

        let output = SecurityAuditOutput {
            target: opts.target,
            findings,
        };

        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_security_audit() {
        let tool = SecurityAuditTool;
        assert_eq!(tool.name(), "security_audit");
    }

    #[test]
    fn input_schema_requires_target() {
        let tool = SecurityAuditTool;
        let schema = tool.input_schema();
        let required = schema["required"].as_array().expect("required array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("target")),
            "input schema must require 'target'"
        );
        assert!(
            schema["properties"]["target"]["type"].as_str() == Some("string"),
            "target must be a string property"
        );
    }
}
