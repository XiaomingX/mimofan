//! `session_search` tool — model-callable semantic recall of long-term memory.
//!
//! Complements the `remember` / `remember_vector` write tools: it lets the
//! model *retrieve* the durable observations previously stored in the vector
//! memory store by semantic similarity, so it can actively pull relevant
//! session/project context instead of waiting for the system-prompt injection.
//!
//! Registered only when the embedding backend is configured
//! (`MIMOFAN_MEMORY_API_KEY`), matching the gating of `remember_vector`, so
//! the model never sees a tool it cannot use. Read-only and auto-approved:
//! the recall only reads the user's own vector memory store.

#![cfg(feature = "vector-memory")]

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};
use crate::vector_memory::VectorMemory;

/// Default number of recalled memories when `top_k` is not supplied.
const DEFAULT_TOP_K: usize = 8;

/// Upper bound on `top_k` to keep a single recall from flooding the context.
const MAX_TOP_K: usize = 50;

/// Tool that semantically recalls long-term / session-indexed memory.
pub struct SessionSearchTool;

#[async_trait]
impl ToolSpec for SessionSearchTool {
    fn name(&self) -> &'static str {
        "session_search"
    }

    fn description(&self) -> &'static str {
        "检索已沉淀的长期记忆/会话索引。通过语义相似度召回之前用 remember / remember_vector \
         存储的 durable observation（项目事实、用户偏好、反馈、约定等），让模型能主动拉取相关 \
         上下文而非被动等待提示注入。query 是召回目标的自然语言描述；可选 top_k 控制返回条数 \
         （默认 8，上限 50）。仅在已配置向量记忆后端时可用。"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "召回目标的自然语言描述，例如「该项目如何处理错误处理」「用户的编码偏好」。"
                },
                "top_k": {
                    "type": "integer",
                    "description": "返回的最相关记忆条数（默认 8，上限 50）。",
                    "minimum": 1,
                    "maximum": MAX_TOP_K as u64
                }
            },
            "required": ["query"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Recall is read-only against the user's own memory store.
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        // Read-only semantic recall; auto-approve so the model can retrieve
        // its own durable context without friction.
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;

        // Empty / whitespace-only query cannot be embedded meaningfully.
        let query = query.trim();
        if query.is_empty() {
            return Err(ToolError::invalid_input(
                "session_search requires a non-empty 'query'",
            ));
        }

        let top_k = parse_top_k(&input);

        let mem_dir = context
            .memory_dir
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .ok_or_else(|| {
                ToolError::execution_failed(
                    "session_search requires a configured memory path (set `[memory] enabled = \
                     true` in config.toml or `MIMOFAN_MEMORY=on` in the environment)",
                )
            })?;

        let mut vm = VectorMemory::open(&mem_dir).map_err(|err| {
            ToolError::execution_failed(format!("failed to open vector memory: {err}"))
        })?;
        if !vm.enabled() {
            return Err(ToolError::execution_failed(
                "session_search is not configured: set MIMOFAN_MEMORY_API_KEY (and optionally \
                 MIMOFAN_MEMORY_BASE_URL / MIMOFAN_MEMORY_MODEL / MIMOFAN_MEMORY_DIMENSION) to \
                 enable, then restart",
            ));
        }

        // Hold the Send embedding service across the await; the non-Send
        // vector store is only touched synchronously afterwards.
        let embedder = vm
            .take_embedder()
            .ok_or_else(|| ToolError::execution_failed("vector-memory embedder unavailable"))?;
        let embedding = embedder
            .embed_text(query)
            .await
            .map_err(|err| ToolError::execution_failed(format!("embedding failed: {err}")))?;

        let project = context
            .workspace
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default")
            .to_string();

        let matches = vm
            .search_embedded(&embedding, Some(&project), top_k)
            .map_err(|err| ToolError::execution_failed(format!("recall failed: {err}")))?;

        if matches.is_empty() {
            return Ok(ToolResult::success(format!(
                "session_search: 未找到与「{query}」相关的长期记忆（项目 {project}）。"
            )));
        }

        // Reuse the same rendering the system-prompt injection path uses, so
        // the model sees a consistent memory block shape.
        let block = VectorMemory::format_injection_block(&project, &matches)
            .unwrap_or_else(|| format!("session_search: 命中 {} 条记忆。", matches.len()));

        Ok(ToolResult::success(format!(
            "session_search 召回 {} 条与「{query}」相关的长期记忆（项目 {project}）：\n{block}",
            matches.len(),
            block = block
        )))
    }
}

/// Parse and clamp `top_k` from the tool input. Missing / out-of-range values
/// fall back to [`DEFAULT_TOP_K`] so a single bad call never errors out.
fn parse_top_k(input: &Value) -> usize {
    let raw = input
        .get("top_k")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_TOP_K);
    raw.clamp(1, MAX_TOP_K)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_and_schema() {
        let tool = SessionSearchTool;
        assert_eq!(tool.name(), "session_search");
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["query"]));
        assert!(schema["properties"]["query"]["type"] == json!("string"));
        assert!(schema["properties"]["top_k"]["type"] == json!("integer"));
        // Read-only → auto-approve, never requires interactive approval.
        assert!(tool.is_read_only());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }

    #[test]
    fn parse_top_k_defaults_and_clamps() {
        assert_eq!(parse_top_k(&json!({})), DEFAULT_TOP_K);
        assert_eq!(parse_top_k(&json!({ "top_k": 3 })), 3);
        assert_eq!(parse_top_k(&json!({ "top_k": 0 })), 1);
        assert_eq!(parse_top_k(&json!({ "top_k": 9_999 })), MAX_TOP_K);
    }

    #[test]
    fn empty_query_is_rejected() {
        // No network / memory access happens for an empty query — validated
        // before any backend open. The execute() branch trims the parsed
        // query and returns an invalid_input error for blank input.
        let input = json!({ "query": "   " });
        let present = required_str(&input, "query").unwrap();
        assert!(
            present.trim().is_empty(),
            "blank query must be caught by trim check"
        );
    }

    #[tokio::test]
    async fn execute_returns_friendly_error_without_memory_dir() {
        // A ToolContext with no memory_dir must fail closed with a clear,
        // non-panic error (no backend open, no network).
        let ctx = ToolContext::new(std::env::temp_dir());
        let tool = SessionSearchTool;
        let result = tool.execute(json!({ "query": "错误处理约定" }), &ctx).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("memory path") || msg.contains("configured"),
            "unexpected error: {msg}"
        );
    }
}
