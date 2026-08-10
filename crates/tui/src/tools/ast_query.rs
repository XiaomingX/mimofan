//! Structured AST retrieval tool: `ast_query`.
//!
//! Wraps `mimofan-staticanalysis` so the model can ask tree-sitter questions
//! about source files without shelling out to grep/sed. This is the user-facing
//! entry point for SAST-style retrieval (issue #587): it resolves a file to a
//! grammar, runs a named vulnerability preset or a raw S-expression query, and
//! returns captured nodes with file/line/column locations.
//!
//! The tool is read-only: it never writes, executes, or makes network calls.

use async_trait::async_trait;
use mimofan_staticanalysis::{AstError, AstHit, Language, named_query, query_source};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool name for the model-facing API.
pub const AST_QUERY_TOOL_NAME: &str = "ast_query";

/// Run a tree-sitter query against a source file.
pub struct AstQueryTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AstQueryInput {
    /// Path to the file to query (resolved relative to the workspace). Either
    /// `path` or `source` must be supplied; when both are present `source`
    /// takes precedence and `path` is only used for language inference + the
    /// reported `file` field.
    #[serde(default)]
    path: Option<String>,
    /// Optional inline source. When omitted the tool reads `path` from disk.
    #[serde(default)]
    source: Option<String>,
    /// Language override: `rust`, `java`, `tsx`, `javascript`, `auto` (default).
    /// When omitted, the language is inferred from the file extension.
    #[serde(default)]
    language: Option<String>,
    /// A named query preset key, e.g. `rust.sink.process_exec`. Mutually
    /// exclusive with `query`.
    #[serde(default)]
    named_query: Option<String>,
    /// A raw tree-sitter S-expression query. Mutually exclusive with
    /// `named_query`.
    #[serde(default)]
    query: Option<String>,
    /// Cap the number of returned hits (default 200) to keep model context small.
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AstQueryOutput {
    file: String,
    language: String,
    query: String,
    hit_count: usize,
    hits: Vec<AstHitView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AstHitView {
    line: usize,
    column: usize,
    capture: String,
    text: String,
}

impl From<AstHit> for AstHitView {
    fn from(h: AstHit) -> Self {
        AstHitView {
            line: h.line,
            column: h.column,
            capture: h.capture,
            text: h.text,
        }
    }
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
        _ => Language::Auto,
    }
}

#[async_trait]
impl ToolSpec for AstQueryTool {
    fn name(&self) -> &'static str {
        AST_QUERY_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Query a source file's AST with tree-sitter. Supports named presets \
         (rust.sink.process_exec, rust.unsound.unsafe_block, java.sink.runtime_exec, \
         java.sink.sql_concat) or a raw S-expression. Returns matched nodes with \
         line/column locations. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Workspace-relative path to the file to query."
                },
                "source": {
                    "type": "string",
                    "description": "Inline source to query instead of reading a file."
                },
                "language": {
                    "type": "string",
                    "description": "Language override: rust | java | tsx | javascript | auto (default)."
                },
                "named_query": {
                    "type": "string",
                    "description": "Named preset key, e.g. rust.sink.process_exec."
                },
                "query": {
                    "type": "string",
                    "description": "Raw tree-sitter S-expression query."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max hits to return (default 200)."
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
        let parsed: AstQueryInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::invalid_input(format!("invalid ast_query input: {e}")))?;

        let (file_label, source) = match (&parsed.path, &parsed.source) {
            (None, None) => {
                return Err(ToolError::invalid_input(
                    "ast_query requires either 'path' or 'source'",
                ));
            }
            (Some(_), Some(src)) | (None, Some(src)) => {
                // Inline source wins; `path` (if any) is only used for the
                // reported `file` label and language inference.
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

        // Resolve the language: explicit override wins, else extension inference.
        let lang = match &parsed.language {
            Some(l) => parse_language(l),
            None => Language::from_path(&file_label),
        };

        // Pick the query: named preset takes precedence over raw `query`.
        let (query_str, _preset_used) = match (&parsed.named_query, &parsed.query) {
            (Some(name), _) => {
                let q = named_query(name).ok_or_else(|| {
                    ToolError::invalid_input(format!("unknown named_query preset '{name}'"))
                })?;
                (q.to_string(), true)
            }
            (None, Some(q)) => (q.clone(), false),
            (None, None) => {
                return Err(ToolError::invalid_input(
                    "ast_query requires either 'named_query' or 'query'",
                ));
            }
        };

        let hits = match query_source(&file_label, &source, lang, &query_str) {
            Ok(hits) => hits,
            Err(AstError::Unsupported(_)) => {
                return Err(ToolError::not_available(format!(
                    "language '{}' has no compiled grammar in this build",
                    lang.as_str()
                )));
            }
            Err(AstError::Query(msg)) => {
                return Err(ToolError::invalid_input(format!(
                    "invalid tree-sitter query: {msg}"
                )));
            }
            Err(AstError::Parse(msg)) => {
                return Err(ToolError::execution_failed(format!(
                    "failed to parse source: {msg}"
                )));
            }
        };

        let limit = parsed.limit.unwrap_or(200).max(1);
        let total = hits.len();
        let truncated = total > limit;
        let shown: Vec<AstHitView> = hits.into_iter().take(limit).map(Into::into).collect();

        let output = AstQueryOutput {
            file: file_label,
            language: lang.as_str().to_string(),
            query: query_str,
            hit_count: if truncated { shown.len() } else { total },
            hits: shown,
        };

        let mut result = ToolResult::json(&output)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        if truncated {
            result = result.with_metadata(json!({
                "truncated": true,
                "total_hits": total,
                "returned": limit
            }));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx_with_workspace(dir: &std::path::Path) -> ToolContext {
        ToolContext::new(dir.to_path_buf())
    }

    #[tokio::test]
    async fn named_query_against_file_reports_hits() {
        let dir = TempDir::new().expect("tempdir");
        let file_path = dir.path().join("demo.rs");
        std::fs::write(
            &file_path,
            "fn main() {\n    let cmd = std::process::Command::new(\"ls\");\n    run_exec();\n}\n",
        )
        .expect("write demo.rs");

        let tool = AstQueryTool;
        let input = json!({
            "path": "demo.rs",
            "named_query": "rust.sink.process_exec"
        });
        let ctx = ctx_with_workspace(dir.path());
        let result = tool
            .execute(input, &ctx)
            .await
            .expect("execute ok");

        assert!(result.success, "ast_query should succeed");
        let parsed: AstQueryOutput =
            serde_json::from_str(&result.content).expect("valid JSON output");
        assert_eq!(parsed.language, "rust");
        assert!(
            parsed.hits.iter().any(|h| h.text.contains("run_exec")),
            "expected run_exec capture, got {parsed:?}"
        );
        assert!(
            parsed.hits.iter().all(|h| h.line > 0 && h.column > 0),
            "hits must carry 1-based line/column"
        );
    }

    #[tokio::test]
    async fn raw_query_via_inline_source_works() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_with_workspace(dir.path());
        let tool = AstQueryTool;
        let input = json!({
            "source": "fn f() { unsafe { g(); } }",
            "language": "rust",
            "query": "(unsafe_block) @blk"
        });
        let result = tool.execute(input, &ctx).await.expect("execute ok");
        assert!(result.success);
        let parsed: AstQueryOutput =
            serde_json::from_str(&result.content).expect("valid JSON output");
        assert_eq!(parsed.hit_count, 1);
        assert!(parsed.hits[0].text.contains("unsafe"));
    }

    #[tokio::test]
    async fn missing_path_and_source_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_with_workspace(dir.path());
        let tool = AstQueryTool;
        let input = json!({ "named_query": "rust.sink.process_exec" });
        let err = tool.execute(input, &ctx).await;
        assert!(err.is_err(), "must reject input with neither path nor source");
    }

    #[tokio::test]
    async fn unknown_named_query_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_with_workspace(dir.path());
        let tool = AstQueryTool;
        let input = json!({ "source": "fn f(){}", "named_query": "no.such.preset" });
        let err = tool.execute(input, &ctx).await;
        assert!(err.is_err(), "must reject unknown named_query preset");
    }
}
