//! `codebase_search` agent tool.
//!
//! Exposes the offline semantic codebase index (`mimofan_memory::codebase`)
//! to the agent loop. The index is built lazily on first use (or when
//! `reindex: true` is passed) over the workspace's source tree and stored
//! under `<workspace>/.mimofan/codebase_index`. Queries run through SQLite +
//! FTS5 (BM25) with optional Reciprocal-Rank-Fusion hybrid ranking.
//!
//! This closes the gap where `CodebaseIndex` existed as a library
//! (#714 reference impl) but no agent tool let the model reach it — the
//! model could only fall back to `grep_files` / `ast_query`.

use super::spec::{
    ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, optional_str, required_str,
};
use async_trait::async_trait;
use mimofan_memory::codebase::{CodebaseIndex, SearchFilters};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Default maximum number of hits returned to the model.
const DEFAULT_LIMIT: usize = 20;

/// Source extensions indexed (kept small and language-agnostic; the indexer
/// still records `language` per file for `language` filtering downstream).
const INDEXED_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "c", "h", "cpp", "cc", "hpp", "rb", "kt",
    "swift", "php", "cs", "sh", "zig", "lua", "sql", "toml", "yaml", "yml", "json", "md",
];

/// Directories skipped during the lazy index walk (mirrors typical VCS /
/// build ignores so we don't index vendored or generated code).
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    ".mimofan",
];

/// Agent tool that searches the workspace's semantic codebase index.
pub struct CodebaseSearchTool;

#[async_trait]
impl ToolSpec for CodebaseSearchTool {
    fn name(&self) -> &'static str {
        "codebase_search"
    }

    fn description(&self) -> &'static str {
        "Semantic search over the indexed workspace codebase (SQLite + FTS5). \
        Use this for natural-language or symbol-oriented code discovery — e.g. \
        \"where do we parse tool arguments\" or \"functions handling auth tokens\" — \
        instead of brute-force grep when you want ranked, snippeted results. \
        The index is built lazily on first call and cached under .mimofan/codebase_index; \
        pass reindex:true to rebuild it. Supports optional language / path_prefix / \
        symbols filters. Prefer this over grep_files for 'find by meaning' queries."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Natural language or code identifiers; \
                    FTS5 phrase/boolean syntax (\"exact phrase\", AND/OR/NOT, *) is also accepted."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max hits to return (default 20, max 100)."
                },
                "language": {
                    "type": "string",
                    "description": "Restrict to a language id, e.g. \"rust\", \"python\", \"tsx\"."
                },
                "path_prefix": {
                    "type": "string",
                    "description": "Restrict to file paths starting with this prefix (relative to workspace)."
                },
                "symbols": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only return chunks that reference one of these symbols."
                },
                "reindex": {
                    "type": "boolean",
                    "description": "Rebuild the index from scratch before searching (default false)."
                }
            },
            "required": ["query"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;
        if query.trim().is_empty() {
            return Err(ToolError::invalid_input("query must not be empty"));
        }

        let limit = {
            let raw = input
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_LIMIT as u64);
            (raw as usize).clamp(1, 100)
        };
        let language = optional_str(&input, "language").map(|s| s.to_string());
        let path_prefix = optional_str(&input, "path_prefix").map(|s| s.to_string());
        let symbols: Vec<String> = input
            .get("symbols")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(|x| x.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let reindex = input
            .get("reindex")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let workspace = &context.workspace;
        let index_dir = workspace.join(".mimofan").join("codebase_index");
        let repo = workspace.to_string_lossy().to_string();

        // Lazy build / rebuild.
        if reindex {
            let _ = std::fs::remove_dir_all(&index_dir);
        }
        if reindex || !index_dir.join("codebase.db").exists() {
            let built = tokio::task::spawn_blocking({
                let workspace = workspace.clone();
                let index_dir = index_dir.clone();
                let repo = repo.clone();
                move || build_index(&workspace, &index_dir, &repo)
            })
            .await
            .map_err(|e| ToolError::execution_failed(format!("index task panicked: {e}")))??;
            if reindex {
                return ToolResult::json(&json!({
                    "status": "reindexed",
                    "files_indexed": built,
                    "index_dir": index_dir.display().to_string(),
                    "note": "Run codebase_search again (without reindex) to query."
                }))
                .map_err(|e| ToolError::execution_failed(format!("serialize: {e}")));
            }
        }

        let filters = SearchFilters {
            language: language.clone(),
            path_prefix: path_prefix.clone(),
            symbols: symbols.clone(),
        };

        let hits = tokio::task::spawn_blocking({
            let index_dir = index_dir.clone();
            let query = query.to_string();
            move || {
                let idx = CodebaseIndex::open(&index_dir)?;
                idx.hybrid_search(&query, &filters, limit, 60.0)
            }
        })
        .await
        .map_err(|e| ToolError::execution_failed(format!("search task panicked: {e}")))?
        .map_err(|e| ToolError::execution_failed(format!("codebase search failed: {e}")))?;

        let results: Vec<Value> = hits
            .iter()
            .map(|h| {
                json!({
                    "file_path": h.file_path,
                    "start_line": h.start_line,
                    "end_line": h.end_line,
                    "language": h.language,
                    "snippet": h.snippet,
                    "symbols": h.symbols,
                })
            })
            .collect();

        ToolResult::json(&json!({
            "query": query,
            "count": results.len(),
            "hits": results,
        }))
        .map_err(|e| ToolError::execution_failed(format!("serialize: {e}")))
    }
}

/// Walk `workspace`, index every recognized source file, and return the
/// number of files indexed. Runs on a blocking thread.
fn build_index(workspace: &Path, index_dir: &Path, repo: &str) -> Result<usize, ToolError> {
    let idx = CodebaseIndex::open(index_dir)
        .map_err(|e| ToolError::execution_failed(format!("open index: {e}")))?;
    let exts: HashSet<&str> = INDEXED_EXTENSIONS.iter().copied().collect();

    let mut count = 0usize;
    for path in walkdir(workspace) {
        let ext = match path.extension().and_then(|s| s.to_str()) {
            Some(e) => e.to_ascii_lowercase(),
            None => continue,
        };
        if !exts.contains(ext.as_str()) {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue, // skip unreadable / binary
        };
        let rel = path
            .strip_prefix(workspace)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        if let Err(e) = idx.index_file(repo, &rel, &ext, &content) {
            tracing::warn!(target: "tool.codebase_search", file = %rel, "index_file failed: {e}");
        } else {
            count += 1;
        }
    }
    Ok(count)
}

/// Minimal recursive dir walker that skips `SKIP_DIRS` without pulling in a
/// walkdir dependency (keeps the tool self-contained). Per-entry read errors
/// are skipped (logged) rather than aborting the whole walk.
fn walkdir(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(target: "tool.codebase_search", dir = %dir.display(), "read_dir failed: {e}");
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && SKIP_DIRS.contains(&name)
            {
                continue;
            }
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() => out.push(path),
                _ => continue,
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_file(dir: &Path, rel: &str, content: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }

    fn tool_context(workspace: &Path) -> ToolContext {
        ToolContext::new(workspace.to_path_buf())
    }

    #[test]
    fn name_and_schema_are_well_formed() {
        let tool = CodebaseSearchTool;
        assert_eq!(tool.name(), "codebase_search");
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"].is_object());
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("query"))
        );
        // Read-only capability -> auto-approved, no destructive side effects.
        assert!(tool.capabilities().contains(&ToolCapability::ReadOnly));
    }

    #[tokio::test]
    async fn indexes_and_finds_a_symbol_across_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        write_file(
            ws,
            "src/auth/token.rs",
            "pub fn validate_auth_token(t: &str) -> bool { t.len() > 8 }\n",
        );
        write_file(
            ws,
            "src/net/fetch.rs",
            "pub async fn fetch_url(u: &str) { println!(\"{u}\"); }\n",
        );

        let tool = CodebaseSearchTool;
        let ctx = tool_context(ws);

        // Lazy index + query in one call.
        let out = tool
            .execute(json!({ "query": "validate auth token", "limit": 10 }), &ctx)
            .await
            .expect("execute");
        assert!(out.success, "tool should succeed: {}", out.content);

        let parsed: Value = serde_json::from_str(&out.content).unwrap();
        assert!(parsed["count"].as_u64().unwrap() >= 1, "expected >=1 hit");
        let hit = &parsed["hits"][0];
        assert!(hit["file_path"].as_str().unwrap().contains("token.rs"));
        assert!(!hit["snippet"].as_str().unwrap().is_empty());

        // Second call should hit the cached index (no rebuild) and still find it.
        let out2 = tool
            .execute(json!({ "query": "fetch url" }), &ctx)
            .await
            .expect("execute2");
        let parsed2: Value = serde_json::from_str(&out2.content).unwrap();
        assert!(
            parsed2["hits"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["file_path"].as_str().unwrap().contains("fetch.rs"))
        );
    }

    #[tokio::test]
    async fn reindex_rebuilds_from_scratch() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        write_file(ws, "a.rs", "fn alpha() {}\n");
        let tool = CodebaseSearchTool;
        let ctx = tool_context(ws);

        // First call builds the index.
        let _ = tool
            .execute(json!({ "query": "alpha" }), &ctx)
            .await
            .unwrap();
        // Reindex returns a status, not hits.
        let re = tool
            .execute(json!({ "query": "alpha", "reindex": true }), &ctx)
            .await
            .expect("reindex");
        let rp: Value = serde_json::from_str(&re.content).unwrap();
        assert_eq!(rp["status"], "reindexed");
        assert!(rp["files_indexed"].as_u64().unwrap() >= 1);
    }

    #[tokio::test]
    async fn empty_query_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = CodebaseSearchTool;
        let ctx = tool_context(tmp.path());
        let err = tool
            .execute(json!({ "query": "   " }), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("empty"));
    }
}
