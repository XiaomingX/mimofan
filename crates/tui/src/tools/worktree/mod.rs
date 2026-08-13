//! Git worktree tools for the main session.
//!
//! Exposes `enter_worktree` / `exit_worktree` so the model can spin up an
//! isolated checkout for experiments and tear it down afterwards, reusing the
//! same service layer the sub-agent spawner uses (see `service.rs`). Addresses
//! the main-session gap called out in #697 and the leakage concern in #691.

pub mod service;

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};
use crate::tools::worktree::service::{
    WorktreeSpawnRequest, create_isolated_worktree, list_worktrees, remove_worktree,
};

/// `enter_worktree` — create an isolated git worktree and report its path.
pub struct EnterWorktreeTool;

/// `exit_worktree` — remove a previously created worktree.
pub struct ExitWorktreeTool;

#[async_trait]
impl ToolSpec for EnterWorktreeTool {
    fn name(&self) -> &str {
        "enter_worktree"
    }

    fn description(&self) -> &str {
        "Create an isolated git worktree (separate checkout + branch) for sandboxed \
         experimentation, parallel exploration, or risky refactors. Returns the \
         absolute path to use as the new working directory. The branch is created \
         from HEAD unless `base_ref` is supplied."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "branch": {
                    "type": "string",
                    "description": "Branch name for the new worktree. Auto-derived when omitted."
                },
                "path": {
                    "type": "string",
                    "description": "Optional explicit path. Relative paths resolve under the default worktree root."
                },
                "base_ref": {
                    "type": "string",
                    "description": "Ref to branch from. Defaults to HEAD."
                },
                "label": {
                    "type": "string",
                    "description": "Human label used to derive the default branch name when `branch` is omitted."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: Value,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let branch = required_opt_string(&input, "branch");
        let path = optional_path(&input, "path");
        let base_ref = required_opt_string(&input, "base_ref");
        let label = required_opt_string(&input, "label");
        let seed = branch
            .as_deref()
            .or_else(|| label.as_deref())
            .map(str::to_string);

        let request = WorktreeSpawnRequest {
            branch,
            path,
            base_ref,
            branch_seed: seed,
        };
        let created = create_isolated_worktree(&context.workspace, &request)?;
        Ok(ToolResult::success(json!({
            "status": "entered",
            "path": created.display().to_string(),
            "branch": request.branch.clone().unwrap_or_default(),
        }).to_string()))
    }
}

#[async_trait]
impl ToolSpec for ExitWorktreeTool {
    fn name(&self) -> &str {
        "exit_worktree"
    }

    fn description(&self) -> &str {
        "Remove a git worktree previously created with `enter_worktree`, cleaning up \
         both the working tree and its branch. Use `force` to discard uncommitted \
         changes."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path of the worktree to remove."
                },
                "force": {
                    "type": "boolean",
                    "description": "Discard uncommitted changes (git worktree remove --force)."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path = required_string(&input, "path")?;
        let force = input
            .get("force")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let path_buf = PathBuf::from(&path);
        remove_worktree(&path_buf, force)?;
        Ok(ToolResult::success(json!({
            "status": "exited",
            "path": path,
        }).to_string()))
    }
}

/// List the worktrees currently tracked by the session repository. Exposed as
/// a small helper for callers that already hold a workspace path.
#[must_use]
pub fn current_worktrees(workspace: &std::path::Path) -> Vec<PathBuf> {
    list_worktrees(workspace)
}

fn required_string(input: &Value, key: &str) -> Result<String, ToolError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ToolError::missing_field(key))
}

fn required_opt_string(input: &Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn optional_path(input: &Value, key: &str) -> Option<PathBuf> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}
