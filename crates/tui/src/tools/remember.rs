//! `remember` tool — model-callable bullet-add into a categorized memory file.
//!
//! Lets the model itself notice a durable preference, convention, or fact
//! worth keeping across sessions and write it to the user's memory
//! directory (`~/.mimofan/memory/` by default), into the chosen category
//! file (`user.md` / `feedback.md` / `project.md` / `reference.md`). The
//! tool is auto-approved and side-effecting only on the user-owned memory
//! directory, so it doesn't get gated behind the same approval flow as
//! shell or arbitrary file writes.
//!
//! Only registered when `[memory] enabled = true` (or
//! `MIMOFAN_MEMORY=on`). When disabled, the tool isn't surfaced to the
//! model at all, so prompts that mention `remember` simply fall through.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};
use crate::memory::{CATEGORIES, DEFAULT_CATEGORY};

/// Tool that appends one bullet to a user memory category file.
pub struct RememberTool;

#[async_trait]
impl ToolSpec for RememberTool {
    fn name(&self) -> &'static str {
        "remember"
    }

    fn description(&self) -> &'static str {
        "Append a durable note to the user's categorized memory so it \
         surfaces in future sessions. Use this when the user states a \
         preference, a convention they want enforced, or a fact about \
         themselves, their project, or an external reference they want you \
         to keep. Pick the category that fits: `user` (who they are), \
         `feedback` (how they want you to work), `project` (project \
         background/decisions), or `reference` (external systems/pointers). \
         Default is `project`. Keep notes terse (one sentence), declarative, \
         not imperative. Don't store secrets, transient tasks, or reasoning \
         scratch — those belong in a checklist or the conversation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": CATEGORIES,
                    "description": "Memory category. One of: user, feedback, project, reference."
                },
                "note": {
                    "type": "string",
                    "description": "The single-sentence durable note to remember."
                }
            },
            "required": ["category", "note"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        // Memory writes are scoped to the user's own memory directory; gating
        // them behind the standard shell/write approval would defeat the
        // point of automatic memory.
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let note = required_str(&input, "note")?;
        let category = match input.get("category").and_then(Value::as_str) {
            Some(c) if !c.trim().is_empty() => c.trim().to_string(),
            _ => DEFAULT_CATEGORY.to_string(),
        };
        let dir = context.memory_dir.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "user memory is disabled — set `[memory] enabled = true` in config.toml or \
                 `MIMOFAN_MEMORY=on` in the environment to enable",
            )
        })?;

        crate::memory::append_entry(dir, &category, note).map_err(|err| {
            ToolError::execution_failed(format!("failed to append to {category}.md: {err}"))
        })?;

        Ok(ToolResult::success(format!(
            "remembered ({category}): {}",
            note.trim_start_matches('#').trim()
        )))
    }
}
