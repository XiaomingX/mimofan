//! Access-control static-analysis tool: `access_control`.
//!
//! Wraps [`mimofan_staticanalysis::access_control`] so the model can ask "is this
//! entry point missing an authorization gate?" — a same-file call-graph
//! reachability check that flags entry-point functions which do not reach any
//! configured gate method (AVDH "Access Control agent").
//!
//! The tool is read-only: it only reads files (or accepts inline source), never
//! writes, executes, or makes network calls. `build_from_dir` / `build` are
//! synchronous CPU work, so they run inside `spawn_blocking`.
//!
//! Input modes (exactly one of `target_dir` / `source` / `path` must be
//! provided; the rest are validated at runtime since the schema `required` list
//! is empty):
//! - `target_dir`  → Java cross-file analysis over a directory (merged graph).
//! - `source`       → inline source, labeled by `path` (or `<inline>`).
//! - `path`         → a workspace-relative file read from disk.

use async_trait::async_trait;
use mimofan_staticanalysis::access_control::{
    analyze_dir, analyze_file, DEFAULT_ENTRIES, DEFAULT_GATES, EntrySpec, GateSpec,
};
use mimofan_staticanalysis::{AstError, Language};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const ACCESS_CONTROL_TOOL_NAME: &str = "access_control";

/// Flag entry points that lack a reachable authorization gate.
pub struct AccessControlTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccessControlInput {
    /// Path to the file to analyze (workspace-relative). Used as the source
    /// label when `source` is present, or read from disk when `source` is
    /// absent.
    #[serde(default)]
    path: Option<String>,
    /// Optional inline source to analyze instead of reading a file.
    #[serde(default)]
    source: Option<String>,
    /// Language override: `rust` | `java` | `tsx` | `javascript` | `auto`.
    /// When omitted the language is inferred from the file extension.
    #[serde(default)]
    language: Option<String>,
    /// Java cross-file mode: a directory to analyze with a merged call graph.
    #[serde(default)]
    target_dir: Option<String>,
    /// Override entry-point name suffixes. When present these replace the
    /// defaults.
    #[serde(default)]
    entry_patterns: Option<Vec<String>>,
    /// Override authorization-gate name suffixes. When present these replace
    /// the defaults.
    #[serde(default)]
    gate_patterns: Option<Vec<String>>,
}

/// A single access-control finding, projected for the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityIssueView {
    rule_id: String,
    severity: String,
    category: String,
    title: String,
    description: String,
    path: Option<String>,
    line: Option<u32>,
    cwe: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccessControlOutput {
    file: String,
    issues: Vec<SecurityIssueView>,
}

fn parse_language(raw: &str) -> Language {
    match raw.to_ascii_lowercase().as_str() {
        "rust" => Language::Rust,
        "java" => Language::Java,
        "tsx" | "typescript" => Language::TypeScript,
        "javascript" | "js" => Language::JavaScript,
        "auto" => Language::Auto,
        _ => Language::Auto,
    }
}

/// Build the active gate/entry specs, honoring user-supplied pattern overrides.
fn resolve_specs(
    gate_patterns: &Option<Vec<String>>,
    entry_patterns: &Option<Vec<String>>,
) -> (Vec<GateSpec>, Vec<EntrySpec>) {
    let gates: Vec<GateSpec> = match gate_patterns {
        Some(patterns) if !patterns.is_empty() => patterns
            .iter()
            .map(|sym| GateSpec {
                symbol: sym.clone(),
                severity: "warning".into(),
                category: "missing-authorization".into(),
                cwe: vec!["CWE-862".into()],
            })
            .collect(),
        _ => DEFAULT_GATES().to_vec(),
    };
    let entries: Vec<EntrySpec> = match entry_patterns {
        Some(patterns) if !patterns.is_empty() => patterns
            .iter()
            .map(|sym| EntrySpec {
                symbol: sym.clone(),
                severity: "high".into(),
                category: "unauthenticated-entry-point".into(),
                cwe: vec!["CWE-306".into()],
            })
            .collect(),
        _ => DEFAULT_ENTRIES().to_vec(),
    };
    (gates, entries)
}

fn to_view(issue: mimofan_staticanalysis::sarif::SecurityIssue) -> SecurityIssueView {
    SecurityIssueView {
        rule_id: issue.rule_id,
        severity: issue.severity,
        category: issue.category,
        title: issue.title,
        description: issue.description,
        path: issue.path,
        line: issue.line,
        cwe: issue.cwe,
        evidence: issue.evidence,
    }
}

fn map_ast_error(err: AstError, what: &str) -> ToolError {
    match err {
        AstError::Unsupported(_) => ToolError::not_available(format!(
            "language for '{what}' has no compiled grammar in this build"
        )),
        AstError::Parse(msg) => ToolError::execution_failed(format!(
            "failed to parse source for '{what}': {msg}"
        )),
        AstError::Query(msg) => ToolError::execution_failed(format!(
            "access-control query failed for '{what}': {msg}"
        )),
    }
}

#[async_trait]
impl ToolSpec for AccessControlTool {
    fn name(&self) -> &'static str {
        ACCESS_CONTROL_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Detect entry points (functions whose name ends in an entry suffix such as \
         '_handler', '_endpoint', '_route') that have no reachable authorization gate \
         (e.g. require_role / require_auth / check_permission). Uses same-file call-graph \
         reachability. Supports rust/java/tsx/javascript. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Workspace-relative path to the file to analyze."
                },
                "source": {
                    "type": "string",
                    "description": "Inline source to analyze instead of reading a file."
                },
                "language": {
                    "type": "string",
                    "description": "Language override: rust | java | tsx | javascript | auto (default)."
                },
                "target_dir": {
                    "type": "string",
                    "description": "Java cross-file mode: a directory to analyze with a merged call graph."
                },
                "entry_patterns": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Override entry-point name suffixes (replaces defaults)."
                },
                "gate_patterns": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Override authorization-gate name suffixes (replaces defaults)."
                }
            },
            "required": [],
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
        let parsed: AccessControlInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::invalid_input(format!("invalid access_control input: {e}")))?;

        if parsed.target_dir.is_none() && parsed.path.is_none() && parsed.source.is_none() {
            return Err(ToolError::invalid_input(
                "access_control requires at least one of 'target_dir', 'path', or 'source'",
            ));
        }

        let (gates, entries) = resolve_specs(&parsed.gate_patterns, &parsed.entry_patterns);

        // Java cross-file mode: analyze a directory with a merged call graph.
        if let Some(target_dir) = &parsed.target_dir {
            let target_dir_inner = target_dir.clone();
            let gates = gates.clone();
            let entries = entries.clone();
            let issues = tokio::task::spawn_blocking(move || {
                analyze_dir(&target_dir_inner, &entries, &gates)
            })
            .await
            .map_err(|e| ToolError::execution_failed(format!("access_control join error: {e}")))?;
            let views: Vec<SecurityIssueView> = issues.into_iter().map(to_view).collect();
            let output = AccessControlOutput {
                file: target_dir.clone(),
                issues: views,
            };
            return ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()));
        }

        // Single-file mode: resolve (file_label, source) from path/source.
        let (file_label, source) = match (&parsed.path, &parsed.source) {
            (Some(_), Some(src)) | (None, Some(src)) => {
                let label = parsed
                    .path
                    .clone()
                    .unwrap_or_else(|| "<inline>".to_string());
                (label, src.clone())
            }
            (Some(p), None) => {
                let resolved = context.resolve_path(p)?;
                let src = std::fs::read_to_string(&resolved).map_err(|e| {
                    ToolError::execution_failed(format!(
                        "failed to read {}: {e}",
                        resolved.display()
                    ))
                })?;
                (resolved.display().to_string(), src)
            }
            (None, None) => unreachable!("guarded above"),
        };

        let lang = match &parsed.language {
            Some(l) => parse_language(l),
            None => Language::from_path(&file_label),
        };

        let file_label2 = file_label.clone();
        let source2 = source.clone();
        let gates2 = gates.clone();
        let entries2 = entries.clone();
        let issues = tokio::task::spawn_blocking(move || {
            analyze_file(&file_label2, &source2, lang, &entries2, &gates2)
        })
        .await
        .map_err(|e| ToolError::execution_failed(format!("access_control join error: {e}")))?
        .map_err(|e| map_ast_error(e, &file_label))?;

        let views: Vec<SecurityIssueView> = issues.into_iter().map(to_view).collect();
        let output = AccessControlOutput {
            file: file_label,
            issues: views,
        };

        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_access_control() {
        let tool = AccessControlTool;
        assert_eq!(tool.name(), ACCESS_CONTROL_TOOL_NAME);
        assert_eq!(tool.name(), "access_control");
    }

    #[test]
    fn input_schema_has_target_dir() {
        let tool = AccessControlTool;
        let schema = tool.input_schema();
        assert!(
            schema["properties"]["target_dir"]["type"].as_str() == Some("string"),
            "target_dir must be a string property"
        );
        // required list stays empty; presence is validated at runtime.
        let required = schema["required"].as_array().expect("required array");
        assert!(required.is_empty(), "runtime three-way-or validation");
    }

    #[test]
    fn exposed_rust_handler_flags_issue() {
        let source = r#"
fn create_user_handler(req: &str) -> String {
    "ok".to_string()
}
"#;
        let issues = analyze_file(
            "test.rs",
            source,
            Language::Rust,
            DEFAULT_ENTRIES(),
            DEFAULT_GATES(),
        )
        .expect("analyze_file succeeds");
        assert_eq!(issues.len(), 1, "exposed handler should be flagged");
        assert_eq!(issues[0].rule_id, "access-control.missing-gate");
    }
}
