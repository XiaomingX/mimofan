//! Call-graph reachability tool: `call_graph`.
//!
//! Wraps [`mimofan_staticanalysis::callgraph::CallGraph`] so the model can ask
//! "which functions are reachable from X?" — the worklist transitive closure
//! built by [`CallGraph::reachable_from_name`]. This is the model-facing entry
//! point for L1 call-graph analysis (issue #598): it resolves a file to a
//! grammar, builds the same-file call graph, and reports every reachable
//! function with its definition site.
//!
//! The tool is read-only: it never writes, executes, or makes network calls.

use async_trait::async_trait;
use mimofan_staticanalysis::callgraph::{CallGraph, FuncId};
use mimofan_staticanalysis::{AstError, Language};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const CALL_GRAPH_TOOL_NAME: &str = "call_graph";

/// Build a call graph for a source file and report functions reachable from a
/// named entry point.
pub struct CallGraphTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallGraphInput {
    /// Path to the file to analyze (resolved relative to the workspace). Either
    /// `path` or `source` must be supplied; when both are present `source`
    /// takes precedence and `path` is only used for language inference + the
    /// reported `file` field.
    #[serde(default)]
    path: Option<String>,
    /// Optional inline source. When omitted the tool reads `path` from disk.
    #[serde(default)]
    source: Option<String>,
    /// Language override: `rust`, `java`, `tsx`, `javascript`, `kotlin`,
    /// `swift`, `objc`, `auto` (default). When omitted, the language is
    /// inferred from the file extension.
    #[serde(default)]
    language: Option<String>,
    /// Entry function name. Reachability is reported starting from this
    /// function. Required.
    #[serde(default)]
    entry: String,
    /// When true, also include unresolved (cross-file) callees by name in the
    /// output so the model sees the external call surface. Default false.
    #[serde(default)]
    include_unresolved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReachableFunction {
    name: String,
    file: String,
    line: usize,
    direct: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallGraphOutput {
    file: String,
    language: String,
    entry: String,
    function_count: usize,
    reachable_count: usize,
    reachable: Vec<ReachableFunction>,
    /// Names of callees that could not be resolved within this file (external
    /// call surface). Only populated when `include_unresolved` is true.
    unresolved_callees: Vec<String>,
}

fn parse_language(raw: &str) -> Language {
    match raw.to_ascii_lowercase().as_str() {
        "rust" => Language::Rust,
        "java" => Language::Java,
        "tsx" | "typescript" => Language::TypeScript,
        "javascript" | "js" => Language::JavaScript,
        "kotlin" => Language::Kotlin,
        "swift" => Language::Swift,
        "objc" | "objectivec" | "objective-c" => Language::ObjectiveC,
        "json" => Language::Json,
        _ => Language::Auto,
    }
}

#[async_trait]
impl ToolSpec for CallGraphTool {
    fn name(&self) -> &'static str {
        CALL_GRAPH_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Build a same-file call graph for a source file and report every function \
         reachable from a named entry point (transitive closure, cycle-safe). \
         Supports rust/java/tsx/javascript/kotlin/swift/objc (per build features). \
         Read-only."
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
                    "description": "Language override: rust | java | tsx | javascript | kotlin | swift | objc | auto (default)."
                },
                "entry": {
                    "type": "string",
                    "description": "Entry function name to compute reachability from."
                },
                "include_unresolved": {
                    "type": "boolean",
                    "description": "Also list cross-file (unresolved) callee names. Default false."
                }
            },
            "required": ["entry"],
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
        let parsed: CallGraphInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::invalid_input(format!("invalid call_graph input: {e}")))?;

        if parsed.entry.is_empty() {
            return Err(ToolError::invalid_input("call_graph requires a non-empty 'entry'"));
        }

        let (file_label, source) = match (&parsed.path, &parsed.source) {
            (None, None) => {
                return Err(ToolError::invalid_input(
                    "call_graph requires either 'path' or 'source'",
                ));
            }
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
        };

        let lang = match &parsed.language {
            Some(l) => parse_language(l),
            None => Language::from_path(&file_label),
        };

        let graph = match CallGraph::build(&file_label, &source, lang) {
            Ok(g) => g,
            Err(AstError::Unsupported(_)) => {
                return Err(ToolError::not_available(format!(
                    "language '{}' has no compiled grammar in this build",
                    lang.as_str()
                )));
            }
            Err(AstError::Parse(msg)) => {
                return Err(ToolError::execution_failed(format!(
                    "failed to parse source: {msg}"
                )));
            }
            Err(AstError::Query(msg)) => {
                return Err(ToolError::execution_failed(format!(
                    "call graph query failed: {msg}"
                )));
            }
        };

        let func_id = match graph.function_id_by_name(&parsed.entry) {
            Some(id) => id,
            None => {
                return Err(ToolError::invalid_input(format!(
                    "entry function '{}' not found in {} ({} functions parsed)",
                    parsed.entry,
                    file_label,
                    graph.len()
                )));
            }
        };

        let reached: std::collections::HashSet<FuncId> = graph.reachable_from(func_id);

        // Direct callees of the entry, to mark `direct: true` in the output.
        let direct_names: std::collections::HashSet<String> = graph
            .calls_of(func_id)
            .iter()
            .map(|e| e.callee_name.clone())
            .collect();

        let mut reachable: Vec<ReachableFunction> = graph
            .functions()
            .iter()
            .filter(|f| reached.contains(&f.id))
            .map(|f| ReachableFunction {
                name: f.name.clone(),
                file: f.file.clone(),
                line: f.line,
                direct: f.id == func_id || direct_names.contains(&f.name),
            })
            .collect();
        // Keep a stable, readable order: entry first, then by line.
        reachable.sort_by_key(|r| (r.name != parsed.entry, r.line));

        let mut unresolved_callees: Vec<String> = Vec::new();
        if parsed.include_unresolved.unwrap_or(false) {
            let mut seen = std::collections::HashSet::new();
            for f in graph.functions() {
                for edge in graph.calls_of(f.id) {
                    if edge.callee.is_none() && seen.insert(edge.callee_name.clone()) {
                        unresolved_callees.push(edge.callee_name.clone());
                    }
                }
            }
            unresolved_callees.sort();
        }

        let output = CallGraphOutput {
            file: file_label,
            language: lang.as_str().to_string(),
            entry: parsed.entry,
            function_count: graph.len(),
            reachable_count: reached.len(),
            reachable,
            unresolved_callees,
        };

        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx_with_workspace(dir: &std::path::Path) -> ToolContext {
        ToolContext::new(dir.to_path_buf())
    }

    const SRC: &str = r#"
fn main() {
    helper();
    leaf();
}

fn helper() {
    leaf();
    recursive();
}

fn recursive() {
    recursive();
}

fn leaf() {}
"#;

    #[tokio::test]
    async fn reachable_from_main_includes_transitive() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_with_workspace(dir.path());
        let tool = CallGraphTool;
        let input = json!({
            "source": SRC,
            "language": "rust",
            "entry": "main"
        });
        let result = tool.execute(input, &ctx).await.expect("execute ok");
        assert!(result.success, "call_graph should succeed");
        let parsed: CallGraphOutput =
            serde_json::from_str(&result.content).expect("valid JSON output");
        let names: Vec<&str> = parsed.reachable.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"helper"));
        assert!(names.contains(&"leaf"));
        assert!(names.contains(&"recursive"));
        assert_eq!(parsed.reachable_count, 4);
    }

    #[tokio::test]
    async fn missing_entry_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_with_workspace(dir.path());
        let tool = CallGraphTool;
        let input = json!({ "source": SRC, "language": "rust", "entry": "nope" });
        let err = tool.execute(input, &ctx).await;
        assert!(err.is_err(), "must reject unknown entry function");
    }
}
