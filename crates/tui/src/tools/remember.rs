//! `remember` tool — model-callable memory management into a categorized
//! memory file.
//!
//! Lets the model itself notice a durable preference, convention, or fact
//! worth keeping across sessions and write it to the user's memory
//! directory (`~/.mimofan/memory/` by default), into the chosen category
//! file (`user.md` / `feedback.md` / `project.md` / `reference.md`). The
//! tool is auto-approved and side-effecting only on the user-owned memory
//! directory, so it doesn't get gated behind the same approval flow as
//! shell or arbitrary file writes.
//!
//! Three actions are supported via the `action` field:
//! - `add` (default) — append a new bullet (the historic behaviour).
//! - `forget` — delete the first bullet in the category whose text contains
//!   `match` (used to retract a wrong or stale memory).
//! - `update` — replace the text of the first bullet matching `match` with
//!   `note` (used to correct a memory without losing its slot).
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

/// Tool that manages bullets in a user memory category file.
pub struct RememberTool;

/// Which mutation to apply to the chosen memory category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RememberAction {
    Add,
    Forget,
    Update,
}

impl RememberAction {
    fn from_str(s: &str) -> RememberAction {
        match s.to_ascii_lowercase().as_str() {
            "forget" | "delete" | "remove" => RememberAction::Forget,
            "update" | "edit" | "replace" => RememberAction::Update,
            _ => RememberAction::Add,
        }
    }
}

#[async_trait]
impl ToolSpec for RememberTool {
    fn name(&self) -> &'static str {
        "remember"
    }

    fn description(&self) -> &'static str {
        "Manage a durable note in the user's categorized memory so it \
         surfaces in future sessions. Use this when the user states a \
         preference, a convention they want enforced, or a fact about \
         themselves, their project, or an external reference they want you \
         to keep. Pick the category that fits: `user` (who they are), \
         `feedback` (how they want you to work), `project` (project \
         background/decisions), or `reference` (external systems/pointers). \
         Default is `project`. Keep notes terse (one sentence), declarative, \
         not imperative. Don't store secrets, transient tasks, or reasoning \
         scratch — those belong in a checklist or the conversation. \
         Actions: `add` (default, append `note`), `forget` (delete the first \
         bullet in `category` containing `match`), `update` (replace the text \
         of the first bullet in `category` matching `match` with `note`)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "forget", "update"],
                    "description": "What to do. `add` appends `note` (default); `forget` removes the first bullet containing `match`; `update` replaces the text of the first bullet matching `match` with `note`."
                },
                "category": {
                    "type": "string",
                    "enum": CATEGORIES,
                    "description": "Memory category. One of: user, feedback, project, reference."
                },
                "note": {
                    "type": "string",
                    "description": "The single-sentence durable note to remember (required for `add` and `update`)."
                },
                "match": {
                    "type": "string",
                    "description": "Substring used to locate the bullet to forget or update (required for `forget` and `update`)."
                }
            },
            "required": ["category"]
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
        let category = match input.get("category").and_then(Value::as_str) {
            Some(c) if !c.trim().is_empty() => c.trim().to_string(),
            _ => DEFAULT_CATEGORY.to_string(),
        };
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .map(RememberAction::from_str)
            .unwrap_or(RememberAction::Add);
        let dir = context.memory_dir.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "user memory is disabled — set `[memory] enabled = true` in config.toml or \
                 `MIMOFAN_MEMORY=on` in the environment to enable",
            )
        })?;

        match action {
            RememberAction::Add => {
                let note = required_str(&input, "note")?;
                crate::memory::append_entry(dir, &category, note).map_err(|err| {
                    ToolError::execution_failed(format!("failed to append to {category}.md: {err}"))
                })?;
                Ok(ToolResult::success(format!(
                    "remembered ({category}): {}",
                    note.trim_start_matches('#').trim()
                )))
            }
            RememberAction::Forget => {
                let matcher = required_str(&input, "match")?;
                let removed = crate::memory::remove_entry(dir, &category, matcher).map_err(|err| {
                    ToolError::execution_failed(format!(
                        "failed to forget from {category}.md: {err}"
                    ))
                })?;
                if removed {
                    Ok(ToolResult::success(format!(
                        "forgot ({category}): removed bullet matching `{matcher}`"
                    )))
                } else {
                    Ok(ToolResult::success(format!(
                        "nothing to forget ({category}): no bullet matched `{matcher}`"
                    )))
                }
            }
            RememberAction::Update => {
                let note = required_str(&input, "note")?;
                let matcher = required_str(&input, "match")?;
                let replaced =
                    crate::memory::replace_entry(dir, &category, matcher, note).map_err(|err| {
                        ToolError::execution_failed(format!(
                            "failed to update {category}.md: {err}"
                        ))
                    })?;
                if replaced {
                    Ok(ToolResult::success(format!(
                        "updated ({category}): bullet matching `{matcher}` -> {}",
                        note.trim_start_matches('#').trim()
                    )))
                } else {
                    Ok(ToolResult::success(format!(
                        "nothing to update ({category}): no bullet matched `{matcher}`"
                    )))
                }
            }
        }
    }
}
