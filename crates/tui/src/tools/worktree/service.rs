//! Reusable git worktree service layer.
//!
//! Previously the `git worktree add` logic lived inline inside the sub-agent
//! parser (`tools/subagent/parser.rs`), where only sub-agent spawning could
//! reach it. Issue #697 (and #691) need the same capability from the main
//! session, so the logic is extracted here behind a small, testable surface
//! that both callers share.
//!
//! Behavior is intentionally identical to the original inline implementation:
//! - resolves the git repo root,
//! - derives/validates a branch name,
//! - refuses paths that fall inside the parent checkout,
//! - runs `git worktree add -b <branch> <path> <base_ref>`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::dependencies::{ExternalTool, Git};
use crate::tools::spec::{ToolError, ToolResult};
use crate::tools::subagent::helpers::SUBAGENT_WORKTREE_ROOT_DIR;

/// A requested isolated worktree, independent of the sub-agent spawn request
/// shape so the main session can express the same intent.
#[derive(Debug, Clone, Default)]
pub struct WorktreeSpawnRequest {
    /// Explicit branch name. When `None` a slug-based default is derived from
    /// `branch_seed`.
    pub branch: Option<String>,
    /// Optional explicit path. Relative paths are resolved under the default
    /// worktree root and must stay within it.
    pub path: Option<PathBuf>,
    /// Base ref to branch from. Defaults to `HEAD`.
    pub base_ref: Option<String>,
    /// Seed used to derive the default branch name when `branch` is `None`.
    pub branch_seed: Option<String>,
}

/// Seed used to derive a default branch name (mirrors the sub-agent behavior).
#[must_use]
pub fn default_branch_name(seed: Option<&str>) -> String {
    let seed = seed
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("task");
    format!(
        "mimofan/worktree-{}-{}",
        sanitize_worktree_slug(seed),
        &Uuid::new_v4().to_string()[..8]
    )
}

/// Create an isolated git worktree rooted at `parent_workspace`'s repository.
///
/// Returns the canonical on-disk path of the new worktree. `branch_seed` is
/// only used when `request.branch` is `None`.
pub fn create_isolated_worktree(
    parent_workspace: &Path,
    request: &WorktreeSpawnRequest,
) -> Result<PathBuf, ToolError> {
    let repo_root = git_repo_root(parent_workspace)?;
    let branch = request
        .branch
        .clone()
        .unwrap_or_else(|| default_branch_name(request.branch_seed.as_deref()));
    validate_git_branch_name(&repo_root, &branch)?;

    let base_ref = request
        .base_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("HEAD")
        .to_string();
    let worktree_path = resolve_worktree_path(&repo_root, &branch, request.path.as_ref())?;
    if let Some(parent) = worktree_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ToolError::execution_failed(format!(
                "Failed to create worktree parent '{}': {err}",
                parent.display()
            ))
        })?;
    }

    let path_arg = worktree_path.to_string_lossy().to_string();
    let args = vec![
        "worktree".to_string(),
        "add".to_string(),
        "-b".to_string(),
        branch,
        path_arg,
        base_ref,
    ];
    run_git_checked(&repo_root, &args, "create worktree")?;
    worktree_path.canonicalize().map_err(|err| {
        ToolError::execution_failed(format!(
            "Created worktree path '{}' could not be resolved: {err}",
            worktree_path.display()
        ))
    })
}

/// Remove a worktree that was created via [`create_isolated_worktree`].
///
/// Uses `git worktree remove` so the branch and working tree are cleaned up
/// together. Pass `force` to discard uncommitted changes (mirrors
/// `git worktree remove --force`).
pub fn remove_worktree(worktree_path: &Path, force: bool) -> Result<(), ToolError> {
    let repo_root = git_repo_root(worktree_path)?;
    let mut args = vec!["worktree".to_string(), "remove".to_string()];
    if force {
        args.push("--force".to_string());
    }
    args.push(worktree_path.to_string_lossy().to_string());
    run_git_checked(&repo_root, &args, "remove worktree")?;
    Ok(())
}

/// List worktrees known to the repository (excluding the main checkout).
#[must_use]
pub fn list_worktrees(workspace: &Path) -> Vec<PathBuf> {
    let Ok(repo_root) = git_repo_root(workspace) else {
        return Vec::new();
    };
    let Ok(out) = run_git_checked(
        &repo_root,
        &["worktree".to_string(), "list".to_string()],
        "list worktrees",
    ) else {
        return Vec::new();
    };
    out.lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(PathBuf::from)
        .collect()
}

fn git_repo_root(workspace: &Path) -> Result<PathBuf, ToolError> {
    let output = run_git_checked(
        workspace,
        &["rev-parse".to_string(), "--show-toplevel".to_string()],
        "resolve git repository root",
    )?;
    let root = output.trim();
    if root.is_empty() {
        return Err(ToolError::invalid_input(
            "worktree operations require a git repository workspace".to_string(),
        ));
    }
    Ok(PathBuf::from(root))
}

fn validate_git_branch_name(repo_root: &Path, branch: &str) -> Result<(), ToolError> {
    let branch = branch.trim();
    if branch.is_empty() {
        return Err(ToolError::invalid_input(
            "worktree branch cannot be blank".to_string(),
        ));
    }
    run_git_checked(
        repo_root,
        &[
            "check-ref-format".to_string(),
            "--branch".to_string(),
            branch.to_string(),
        ],
        "validate worktree branch",
    )
    .map(|_| ())
    .map_err(|err| ToolError::invalid_input(format!("Invalid worktree branch '{branch}': {err}")))
}

fn resolve_worktree_path(
    repo_root: &Path,
    branch: &str,
    requested_path: Option<&PathBuf>,
) -> Result<PathBuf, ToolError> {
    let default_root = default_worktree_root(repo_root);
    let path = match requested_path {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => {
            let resolved = normalize_path_lexically(&default_root.join(path));
            if !resolved.starts_with(&default_root) {
                return Err(ToolError::invalid_input(format!(
                    "relative worktree_path '{}' must stay under {}",
                    path.display(),
                    default_root.display()
                )));
            }
            resolved
        }
        None => default_root.join(sanitize_worktree_slug(branch)),
    };
    let normalized = normalize_path_lexically(&path);
    let repo_canonical = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    if normalized.starts_with(&repo_canonical) {
        return Err(ToolError::invalid_input(format!(
            "worktree_path must not be inside the parent checkout: {} is under {}",
            normalized.display(),
            repo_canonical.display()
        )));
    }
    Ok(normalized)
}

fn default_worktree_root(repo_root: &Path) -> PathBuf {
    let repo_name = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_worktree_slug)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "repo".to_string());
    let parent = repo_root.parent().unwrap_or(repo_root);
    normalize_path_lexically(&parent.join(SUBAGENT_WORKTREE_ROOT_DIR).join(repo_name))
}

pub(crate) fn sanitize_worktree_slug(input: &str) -> String {
    let mut slug = String::new();
    for ch in input.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else if matches!(ch, '-' | '_' | '.') {
            ch
        } else {
            '-'
        };
        if normalized == '-' && slug.ends_with('-') {
            continue;
        }
        slug.push(normalized);
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches(['-', '.', '_']).to_string();
    if slug.is_empty() {
        "task".to_string()
    } else {
        slug
    }
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn run_git_checked(workspace: &Path, args: &[String], action: &str) -> Result<String, ToolError> {
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = Git::output(&arg_refs, workspace).map_err(|err| {
        ToolError::execution_failed(format!("Failed to {action}: could not run git: {err}"))
    })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("git exited with status {}", output.status)
    };
    Err(ToolError::execution_failed(format!(
        "Failed to {action}: {detail}"
    )))
}

/// Build a short human-readable summary of a worktree creation result for the
/// model-facing tool output.
#[must_use]
pub fn worktree_created_summary(path: &Path, branch: &str) -> ToolResult {
    ToolResult::success(json!({
        "status": "created",
        "path": path.display().to_string(),
        "branch": branch,
    }).to_string())
}
