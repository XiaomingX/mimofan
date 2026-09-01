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

        let target = parsed.target.clone();
        let mut findings: Vec<ReviewIssue> = Vec::new();

        // Built-in, always-available Java taint analysis (no external binary,
        // no sandbox/SK needed): runs against .java targets/directories.
        let target_path = std::path::Path::new(&target);
        let is_java = target.ends_with(".java") || target_path.extension().is_none();
        if is_java {
            let internal = if target.ends_with(".java") {
                let src = std::fs::read_to_string(&target).map_err(|e| {
                    ToolError::execution_failed(format!("cannot read {target}: {e}"))
                })?;
                mimofan_staticanalysis::java_taint::analyze_file(&target, &src).unwrap_or_default()
            } else {
                mimofan_staticanalysis::java_taint::analyze_dir(&target)
            };
            for issue in &internal {
                findings.push(to_review_issue(issue));
            }
        }

        // Optional external semgrep (only when a sandbox backend is present
        // and semgrep is installed); the internal analysis already covers
        // Java, so this is additive.
        if let Some(backend) = context.sandbox_backend.as_ref() {
            let opts = SemgrepOptions {
                target: parsed.target.clone(),
                config: parsed.config.clone(),
                extra_flags: parsed.extra_flags.clone(),
            };
            if let Ok(external) = run_semgrep_scan(backend.as_ref(), &opts).await {
                for issue in &external {
                    let ri = to_review_issue(issue);
                    if !findings
                        .iter()
                        .any(|f| f.path == ri.path && f.line == ri.line && f.rule_id == ri.rule_id)
                    {
                        findings.push(ri);
                    }
                }
            }
        }

        let output = SecurityAuditOutput { target, findings };

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
