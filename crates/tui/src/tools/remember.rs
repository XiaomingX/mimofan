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
//! Actions are supported via the `action` field, split into two families:
//!
//! Bullet layer (scattered facts in `user.md`/`feedback.md`/`project.md`/
//! `reference.md`):
//! - `add` (default) — append a new bullet (the historic behaviour).
//! - `forget` — delete the first bullet in the category whose text contains
//!   `match` (used to retract a wrong or stale memory).
//! - `update` — replace the text of the first bullet matching `match` with
//!   `note` (used to correct a memory without losing its slot).
//!
//! Decision layer (audited choices in `decisions.md`, with a why-trail):
//! - `decide` — capture a durable decision under a stable `id` with a `note`
//!   (the current understanding) and optional `category`. Use this for
//!   cross-session choices/constraints worth keeping *with their rationale*,
//!   not for scattered preferences (those go in `add`).
//! - `revise` — rewrite the decision identified by `id`, recording `note` as
//!   the new understanding and `match` as *why* it changed (append-only).
//! - `reverse` — overturn the decision identified by `id`, recording `match`
//!   as *why*; the entry is kept (not deleted) so the audit trail survives.
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

/// Which mutation to apply to the chosen memory category or decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RememberAction {
    Add,
    Forget,
    Update,
    Decide,
    Revise,
    Reverse,
}

impl RememberAction {
    fn from_str(s: &str) -> RememberAction {
        match s.to_ascii_lowercase().as_str() {
            "forget" | "delete" | "remove" => RememberAction::Forget,
            "update" | "edit" | "replace" => RememberAction::Update,
            "decide" | "decision" => RememberAction::Decide,
            "revise" | "amend" => RememberAction::Revise,
            "reverse" | "overturn" => RememberAction::Reverse,
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
        "Manage a durable note or decision in the user's categorized memory \
         so it surfaces in future sessions. Use this when the user states a \
         preference, a convention they want enforced, or a fact about \
         themselves, their project, or an external reference they want you \
         to keep. Pick the category that fits: `user` (who they are), \
         `feedback` (how they want you to work), `project` (project \
         background/decisions), or `reference` (external systems/pointers). \
         Default is `project`. Keep notes terse (one sentence), declarative, \
         not imperative. Don't store secrets, transient tasks, or reasoning \
         scratch — those belong in a checklist or the conversation. \
         Bullet actions: `add` (default, append `note`), `forget` (delete the \
         first bullet in `category` containing `match`), `update` (replace the \
         text of the first bullet in `category` matching `match` with `note`). \
         Decision actions (audited with a why-trail in `decisions.md`): \
         `decide` (capture a durable choice under stable `id`, with `note` as \
         the current understanding and optional `category`), `revise` (rewrite \
         the decision `id`, with `note` as the new understanding and `match` \
         as *why* it changed), `reverse` (overturn decision `id`, with \
         `match` as *why* — the entry is kept, not deleted). Prefer `decide` \
         over `add` for cross-session choices/constraints whose rationale \
         matters; use `add` for scattered preferences."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "forget", "update", "decide", "revise", "reverse"],
                    "description": "What to do. Bullet actions: `add` appends `note` (default); `forget` removes the first bullet containing `match`; `update` replaces the text of the first bullet matching `match` with `note`. Decision actions: `decide` captures a choice under `id` with `note`; `revise` rewrites `id` (`note` = new understanding, `match` = why); `reverse` overturns `id` (`match` = why)."
                },
                "category": {
                    "type": "string",
                    "enum": CATEGORIES,
                    "description": "Memory category for bullet actions. One of: user, feedback, project, reference."
                },
                "id": {
                    "type": "string",
                    "description": "Stable decision id for decision actions (`decide`/`revise`/`reverse`). A short slug, unique within the memory directory."
                },
                "note": {
                    "type": "string",
                    "description": "The single-sentence durable text (required for `add`, `update`, and `decide`; new understanding for `revise`)."
                },
                "match": {
                    "type": "string",
                    "description": "Substring to locate the bullet to forget/update (required for `forget`/`update`); rationale for `revise`/`reverse` (why changed/overturned)."
                }
            },
            "required": ["action"]
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
            RememberAction::Decide => {
                let id = required_str(&input, "id")?;
                let note = required_str(&input, "note")?;
                crate::memory::decision_create(dir, id, "", &category, note)
                    .map_err(|err| {
                        ToolError::execution_failed(format!(
                            "failed to decide `{id}`: {err}"
                        ))
                    })?;
                Ok(ToolResult::success(format!(
                    "decided ({id}): {}",
                    note.trim_start_matches('#').trim()
                )))
            }
            RememberAction::Revise => {
                let id = required_str(&input, "id")?;
                let note = required_str(&input, "note")?;
                let why = required_str(&input, "match")?;
                let ok = crate::memory::decision_revise(dir, id, note, why).map_err(|err| {
                    ToolError::execution_failed(format!("failed to revise `{id}`: {err}"))
                })?;
                if ok {
                    Ok(ToolResult::success(format!(
                        "revised ({id}): now -> {}",
                        note.trim_start_matches('#').trim()
                    )))
                } else {
                    Ok(ToolResult::success(format!(
                        "nothing to revise ({id}): unknown id or already reversed"
                    )))
                }
            }
            RememberAction::Reverse => {
                let id = required_str(&input, "id")?;
                let why = required_str(&input, "match")?;
                let ok = crate::memory::decision_reverse(dir, id, why).map_err(|err| {
                    ToolError::execution_failed(format!("failed to reverse `{id}`: {err}"))
                })?;
                if ok {
                    Ok(ToolResult::success(format!(
                        "reversed ({id}): {why}"
                    )))
                } else {
                    Ok(ToolResult::success(format!(
                        "nothing to reverse ({id}): unknown id or already reversed"
                    )))
                }
            }
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
                let removed =
                    crate::memory::remove_entry(dir, &category, matcher).map_err(|err| {
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
                let replaced = crate::memory::replace_entry(dir, &category, matcher, note)
                    .map_err(|err| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn tmp_memory_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "mimofan-remember-test-{}-{}-{}",
            std::process::id(),
            nanos,
            seq
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn ctx_with_memory(dir: &Path) -> ToolContext {
        let mut ctx = ToolContext::new(dir.to_path_buf());
        ctx.memory_dir = Some(dir.to_path_buf());
        ctx
    }

    fn json_action(action: &str, category: &str, id: &str, note: &str, matcher: &str) -> Value {
        json!({
            "action": action,
            "category": category,
            "id": id,
            "note": note,
            "match": matcher,
        })
    }

    #[tokio::test]
    async fn decide_writes_decisions_md() {
        let dir = tmp_memory_dir();
        let ctx = ctx_with_memory(&dir);
        let _res = RememberTool
            .execute(
                json_action("decide", "architecture", "api-auth", "Use Bearer tokens", ""),
                &ctx,
            )
            .await
            .unwrap();
        let entries = memory::read_decisions(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "api-auth");
        assert!(entries[0].current.contains("Bearer tokens"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn revise_appends_revision() {
        let dir = tmp_memory_dir();
        let ctx = ctx_with_memory(&dir);
        RememberTool
            .execute(
                json_action("decide", "architecture", "d1", "v1", ""),
                &ctx,
            )
            .await
            .unwrap();
        let _res = RememberTool
            .execute(
                json_action("revise", "architecture", "d1", "v2", "switched to mTLS"),
                &ctx,
            )
            .await
            .unwrap();
        let entries = memory::read_decisions(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].current, "v2");
        assert_eq!(entries[0].history.len(), 2);
        assert_eq!(entries[0].history[1].kind, memory::DecisionEventKind::Revision);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn reverse_keeps_entry_with_reversal() {
        let dir = tmp_memory_dir();
        let ctx = ctx_with_memory(&dir);
        RememberTool
            .execute(json_action("decide", "policy", "d1", "v1", ""), &ctx)
            .await
            .unwrap();
        let _res = RememberTool
            .execute(
                json_action("reverse", "policy", "d1", "", "superseded by gateway"),
                &ctx,
            )
            .await
            .unwrap();
        let entries = memory::read_decisions(&dir);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].reversed);
        assert_eq!(entries[0].history.len(), 2);
        assert_eq!(entries[0].history[1].kind, memory::DecisionEventKind::Reversal);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_bullet_still_works() {
        // Regression guard: the original bullet actions must be untouched.
        let dir = tmp_memory_dir();
        let ctx = ctx_with_memory(&dir);
        let _res = RememberTool
            .execute(
                json!({ "action": "add", "category": "user", "note": "prefers Rust" }),
                &ctx,
            )
            .await
            .unwrap();
        let _res = _res;
        let user = memory::read_category(&dir, "user").expect("user file");
        assert!(user.contains("prefers Rust"));
        let _ = fs::remove_dir_all(&dir);
    }
}
