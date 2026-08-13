//! `remember_vector` tool — model-callable semantic memory write.
//!
//! Complements the `remember` tool (which appends a bullet to the user's
//! `memory.md` file). `remember_vector` stores a typed observation into the
//! vector memory store so it can be semantically recalled across sessions
//! via `/vmemory query` and the system-prompt injection path.
//!
//! Registered only when the embedding backend is configured
//! (`MIMOFAN_MEMORY_API_KEY`), so the model never sees a tool it can't use.
//! Auto-approved: the write is scoped to the user's own vector memory store
//! under the memory directory, never arbitrary files or the shell.

#![cfg(feature = "vector-memory")]

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};
use crate::vector_memory::{VectorMemory, parse_memory_category};

/// Tool that stores one typed observation into the user's vector memory store.
pub struct RememberVectorTool;

#[async_trait]
impl ToolSpec for RememberVectorTool {
    fn name(&self) -> &'static str {
        "remember_vector"
    }

    fn description(&self) -> &'static str {
        "Store a typed observation into the semantic vector memory store so it can be \
         recalled across sessions by semantic similarity (complementing the `remember` \
         file-based tool). Use this when the user states a durable decision, a code \
         change, a project fact, or a convention worth retrieving later by meaning — \
         not just by keyword. Keep the content terse (one sentence). Don't store \
         secrets, transient tasks, or reasoning scratch."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "description": "Memory category: one of user, feedback, project, reference (shared with the file-based memory system)."
                },
                "content": {
                    "type": "string",
                    "description": "The single-sentence observation to remember."
                }
            },
            "required": ["kind", "content"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        // Scoped to the user's own vector memory store; auto-approve so the
        // model can build durable cross-session context without friction.
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let kind = required_str(&input, "kind")?;
        let content = required_str(&input, "content")?;

        let mem_dir = context
            .memory_dir
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .ok_or_else(|| {
                ToolError::execution_failed(
                    "vector-memory requires a configured memory path (set `[memory] enabled = \
                     true` in config.toml or `MIMOFAN_MEMORY=on` in the environment)",
                )
            })?;

        let mut vm = VectorMemory::open(&mem_dir).map_err(|err| {
            ToolError::execution_failed(format!("failed to open vector memory: {err}"))
        })?;
        if !vm.enabled() {
            return Err(ToolError::execution_failed(
                "vector-memory is not configured: set MIMOFAN_MEMORY_API_KEY (and optionally \
                 MIMOFAN_MEMORY_BASE_URL / MIMOFAN_MEMORY_MODEL / MIMOFAN_MEMORY_DIMENSION) to \
                 enable, then restart",
            ));
        }

        let obs_kind = parse_memory_category(kind)
            .map_err(|err| ToolError::execution_failed(err.to_string()))?;
        let kind_str = obs_kind.as_str();

        // #718: refuse to persist secrets into the semantic memory store.
        // The vector store is recalled by similarity and could otherwise
        // surface a pasted credential back to the model or to other sessions.
        if mimofan_secrets::is_sensitive_content(content) {
            return Err(ToolError::execution_failed(
                "refusing to store: content looks like it contains a secret \
                 (API key, token, private key, etc.). Secrets must not be \
                 written into semantic memory.",
            ));
        }

        let project = context
            .workspace
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default")
            .to_string();

        // Take the Send embedding service and hold it across the await; the
        // non-Send store is only touched synchronously afterwards.
        let embedder = vm
            .take_embedder()
            .ok_or_else(|| ToolError::execution_failed("vector-memory embedder unavailable"))?;
        let embedding = embedder
            .embed_text(content)
            .await
            .map_err(|err| ToolError::execution_failed(format!("embedding failed: {err}")))?;
        let id = vm
            .store_observation(&project, kind_str, content, &context.session_id, &embedding)
            .map_err(|err| ToolError::execution_failed(format!("failed to store: {err}")))?;

        Ok(ToolResult::success(format!(
            "remembered (vector id {id}): [{kind_str}] {content}"
        )))
    }
}
