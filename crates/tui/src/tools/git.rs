//! Git power tools: `git_status` and `git_diff`.
//!
//! These tools are read-only wrappers around common git inspection commands,
//! scoped to the workspace and optionally to a sub-path within it.

use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::dependencies::ExternalTool;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_str, optional_u64,
};

const MAX_OUTPUT_CHARS: usize = 40_000;
const DEFAULT_UNIFIED: u64 = 3;
const MAX_UNIFIED: u64 = 50;

// === GitStatusTool ===

/// Tool for reading the concise git status of the workspace.
pub struct GitStatusTool;

#[async_trait]
impl ToolSpec for GitStatusTool {
    fn name(&self) -> &'static str {
        "git_status"
    }

    fn description(&self) -> &'static str {
        "Run `git status --porcelain=v1 -b` in the workspace (optionally scoped to a path)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory or file to scope the status to (must be within the workspace)."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let git_ctx = resolve_git_context(context, optional_str(&input, "path"))?;

        let mut args = vec![
            "-c".to_string(),
            "core.quotepath=false".to_string(),
            "status".to_string(),
            "--porcelain=v1".to_string(),
            "-b".to_string(),
        ];
        if let Some(pathspec) = &git_ctx.pathspec {
            args.push("--".to_string());
            args.push(pathspec.display().to_string());
        }

        let command_str = format_command(&git_ctx.working_dir, &args);
        let output = run_git_command(&git_ctx.working_dir, &args)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = format!("git status failed: {}", stderr.trim());
            return Ok(ToolResult::error(message).with_metadata(json!({
                "command": command_str,
                "exit_code": output.status.code(),
                "stderr": stderr.trim(),
            })));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let (content, truncated, omitted_chars) = truncate_with_note(&stdout, MAX_OUTPUT_CHARS);

        Ok(ToolResult::success(content).with_metadata(json!({
            "command": command_str,
            "working_dir": git_ctx.working_dir,
            "pathspec": git_ctx.pathspec,
            "truncated": truncated,
            "omitted_chars": omitted_chars,
        })))
    }
}

// === GitDiffTool ===

/// Tool for reading git diffs in the workspace.
pub struct GitDiffTool;

#[async_trait]
impl ToolSpec for GitDiffTool {
    fn name(&self) -> &'static str {
        "git_diff"
    }

    fn description(&self) -> &'static str {
        "Run `git diff` in the workspace with sensible defaults and safe truncation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory or file to scope the diff to (must be within the workspace)."
                },
                "cached": {
                    "type": "boolean",
                    "description": "When true, diff staged changes (`--cached`)."
                },
                "unified": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_UNIFIED,
                    "default": DEFAULT_UNIFIED,
                    "description": "Number of context lines to include around changes."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let git_ctx = resolve_git_context(context, optional_str(&input, "path"))?;
        let cached = optional_bool(&input, "cached", false);
        let unified = optional_u64(&input, "unified", DEFAULT_UNIFIED).min(MAX_UNIFIED);

        let mut args = vec![
            "-c".to_string(),
            "core.quotepath=false".to_string(),
            "diff".to_string(),
            "--no-color".to_string(),
            "--no-ext-diff".to_string(),
            format!("--unified={unified}"),
        ];
        if cached {
            args.push("--cached".to_string());
        }
        if let Some(pathspec) = &git_ctx.pathspec {
            args.push("--".to_string());
            args.push(pathspec.display().to_string());
        }

        let command_str = format_command(&git_ctx.working_dir, &args);
        let output = run_git_command(&git_ctx.working_dir, &args)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = format!("git diff failed: {}", stderr.trim());
            return Ok(ToolResult::error(message).with_metadata(json!({
                "command": command_str,
                "exit_code": output.status.code(),
                "stderr": stderr.trim(),
            })));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let (content, truncated, omitted_chars) = truncate_with_note(&stdout, MAX_OUTPUT_CHARS);

        Ok(ToolResult::success(content).with_metadata(json!({
            "command": command_str,
            "working_dir": git_ctx.working_dir,
            "pathspec": git_ctx.pathspec,
            "cached": cached,
            "unified": unified,
            "truncated": truncated,
            "omitted_chars": omitted_chars,
        })))
    }
}

// === GitCommitTool ===

/// Tool for committing staged/unstaged changes in the workspace with a
/// provided commit message.
///
/// This is the only *write* operation in the git tool family. The model is
/// expected to first call `git_diff` (and `git_status`) to gather context,
/// generate a Conventional-Commits-style message, then pass it here. Because
/// committing mutates repository state, the tool declares
/// `ApprovalRequirement::Required` so the harness asks the user to confirm the
/// message **and** the affected file scope before `git commit` ever runs — it
/// is never executed silently.
pub struct GitCommitTool;

#[async_trait]
impl ToolSpec for GitCommitTool {
    fn name(&self) -> &'static str {
        "git_commit"
    }

    fn description(&self) -> &'static str {
        "Commit staged (or optionally `add`-ed) changes with a provided commit message. \
         This is a write operation: the user must approve the message and file scope before it runs. \
         Call `git_diff`/`git_status` first to generate a Conventional-Commits-style message."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The commit message. Should follow Conventional Commits (e.g. 'feat(tools): add git_commit'). Shown to the user for approval."
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of workspace-relative paths to stage before committing. \
                        When omitted and `add_all` is false, only already-staged changes are committed."
                },
                "add_all": {
                    "type": "boolean",
                    "description": "When true, stage all changes (tracked and untracked) via `git add -A` before committing."
                },
                "amend": {
                    "type": "boolean",
                    "description": "When true, amend the previous commit instead of creating a new one."
                },
                "co_authored_by": {
                    "type": "boolean",
                    "description": "When true (default), append a 'Co-Authored-By: mimofan' trailer so GitHub credits mimofan as a co-author (mirrors Claude Code's includeCoAuthoredBy). Set false to omit."
                }
            },
            "required": ["message"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles, ToolCapability::RequiresApproval]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn supports_parallel(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let message = optional_str(&input, "message")
            .ok_or_else(|| ToolError::missing_field("message"))?;
        if message.trim().is_empty() {
            return Err(ToolError::invalid_input("message must not be empty"));
        }
        let amend = optional_bool(&input, "amend", false);
        let add_all = optional_bool(&input, "add_all", false);
        // 默认开启 Co-Authored-By 署名（参考 Claude Code 的 includeCoAuthoredBy）。
        let co_authored_by = optional_bool(&input, "co_authored_by", true);

        let files: Vec<String> = input
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let git_ctx = resolve_git_context(context, None)?;
        let working_dir = &git_ctx.working_dir;

        // 1) Stage the requested files (or everything) before committing.
        if add_all {
            let args = vec!["add".to_string(), "-A".to_string()];
            let out = run_git_command(working_dir, &args)?;
            if !out.status.success() {
                return Err(ToolError::execution_failed(format!(
                    "git add -A failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
        } else if !files.is_empty() {
            let mut args = vec!["add".to_string()];
            for f in &files {
                // Resolve each path through the workspace to guarantee it stays
                // inside the repo (defense-in-depth against "../" escapes).
                let resolved = context.resolve_path(f).map_err(|e| {
                    ToolError::invalid_input(format!("Cannot resolve path '{f}': {e}"))
                })?;
                args.push(resolved.display().to_string());
            }
            let out = run_git_command(working_dir, &args)?;
            if !out.status.success() {
                return Err(ToolError::execution_failed(format!(
                    "git add failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
        }

        // 2) Guard against an empty commit (nothing staged and nothing to commit).
        let nothing_to_commit = {
            let out = run_git_command(
                working_dir,
                &[
                    "-c".to_string(),
                    "core.quotepath=false".to_string(),
                    "diff".to_string(),
                    "--cached".to_string(),
                    "--quiet".to_string(),
                ],
            )?;
            // `git diff --cached --quiet` exits 1 when there is a staged diff.
            out.status.success()
        };
        if nothing_to_commit && !amend {
            return Ok(ToolResult::error(
                "Nothing staged to commit. Call `git_diff`/`git_status` first, or pass `files`/`add_all`.",
            )
            .with_metadata(json!({ "working_dir": working_dir })));
        }

        // 3) Commit. The message is passed via stdin to avoid shell-quoting issues.
        let final_message = append_co_authored_by(message, co_authored_by);
        let mut args = vec![
            "-c".to_string(),
            "core.quotepath=false".to_string(),
            "commit".to_string(),
            "-F".to_string(),
            "-".to_string(),
        ];
        if amend {
            args.push("--amend".to_string());
        }
        let mut cmd = crate::dependencies::Git::command().ok_or_else(|| {
            ToolError::not_available("git is not installed or not in PATH")
        })?;
        cmd.args(&args).current_dir(working_dir);
        cmd.stdin(std::process::Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToolError::not_available("git is not installed or not in PATH")
            } else {
                ToolError::execution_failed(format!("Failed to spawn git commit: {e}"))
            }
        })?;
        use std::io::Write;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(final_message.as_bytes())
                .map_err(|e| ToolError::execution_failed(format!("Failed to write message: {e}")))?;
        }
        let output = child.wait_with_output().map_err(|e| {
            ToolError::execution_failed(format!("Failed to run git commit: {e}"))
        })?;
        if !output.status.success() {
            return Err(ToolError::execution_failed(format!(
                "git commit failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        // 4) Report the new HEAD so the user can verify the committed message.
        let hash_out = run_git_command(working_dir, &["rev-parse".to_string(), "HEAD".to_string()])?;
        let head = if hash_out.status.success() {
            String::from_utf8_lossy(&hash_out.stdout).trim().to_string()
        } else {
            String::new()
        };

        Ok(ToolResult::success(format!("Committed: {head}\n\n{message}")).with_metadata(json!({
            "working_dir": working_dir,
            "head": head,
            "message": message,
            "amend": amend,
            "staged_files": files,
            "add_all": add_all,
        })))
    }
}

// === Helpers ===

struct GitContext {
    working_dir: PathBuf,
    pathspec: Option<PathBuf>,
}

fn resolve_git_context(context: &ToolContext, path: Option<&str>) -> Result<GitContext, ToolError> {
    let workspace = canonical_or_workspace(&context.workspace);
    let mut working_dir = workspace.clone();
    let mut pathspec = None;

    if let Some(raw) = path {
        let resolved = context.resolve_path(raw)?;
        let metadata = fs::metadata(&resolved).map_err(|e| {
            ToolError::invalid_input(format!(
                "Path does not exist or is not accessible: {raw} ({e})"
            ))
        })?;

        if metadata.is_dir() {
            working_dir = resolved;
            pathspec = Some(PathBuf::from("."));
        } else {
            // For file paths, run from the parent and scope to the file name.
            let parent = resolved.parent().ok_or_else(|| {
                ToolError::invalid_input(format!("Path has no parent directory: {raw}"))
            })?;
            working_dir = parent.to_path_buf();
            pathspec = Some(pathspec_from(&working_dir, &resolved));
        }
    }

    if !working_dir.exists() {
        return Err(ToolError::invalid_input(format!(
            "Working directory does not exist: {}",
            working_dir.display()
        )));
    }

    Ok(GitContext {
        working_dir,
        pathspec,
    })
}

fn canonical_or_workspace(workspace: &Path) -> PathBuf {
    workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf())
}

fn pathspec_from(working_dir: &Path, resolved: &Path) -> PathBuf {
    match resolved.strip_prefix(working_dir) {
        Ok(rel) if rel.as_os_str().is_empty() => PathBuf::from("."),
        Ok(rel) => rel.to_path_buf(),
        Err(_) => PathBuf::from("."),
    }
}

fn run_git_command(working_dir: &Path, args: &[String]) -> Result<std::process::Output, ToolError> {
    let Some(mut cmd) = crate::dependencies::Git::command() else {
        return Err(ToolError::not_available(
            "git is not installed or not in PATH",
        ));
    };
    cmd.args(args).current_dir(working_dir);
    cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::not_available("git is not installed or not in PATH")
        } else {
            ToolError::execution_failed(format!("Failed to run git: {e}"))
        }
    })
}

fn format_command(working_dir: &Path, args: &[String]) -> String {
    format!(
        "git -C {} {}",
        working_dir.display(),
        args.iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn truncate_with_note(text: &str, max_chars: usize) -> (String, bool, usize) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false, 0);
    }
    let end = char_boundary_index(text, max_chars);
    let truncated = &text[..end];
    let omitted_chars = text
        .chars()
        .count()
        .saturating_sub(truncated.chars().count());
    let note = format!(
        "\n\n[output truncated to {max_chars} characters; {omitted_chars} characters omitted]"
    );
    (format!("{truncated}{note}"), true, omitted_chars)
}

fn char_boundary_index(text: &str, max_chars: usize) -> usize {
    if max_chars == 0 {
        return 0;
    }
    for (count, (idx, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            return idx;
        }
    }
    text.len()
}

// === Co-Authored-By attribution (mirrors Claude Code's includeCoAuthoredBy) ===

/// mimofan 在 GitHub 上的共同作者署名 trailer。
///
/// 当 AI 通过 `git_commit` 提交代码时，默认在消息末尾追加此 trailer，使 GitHub
/// 把 mimofan 显示为共同作者（co-author），与 Claude Code / CodeBuddy 的行为一致。
/// GitHub 依据 `Co-Authored-By: <name> <email>` 在提交详情与贡献者列表里展示署名，
/// 但不改变真正的 committer（committer 仍是运行 mimofan 的用户的 git 身份）。
const MIMOFAN_CO_AUTHOR_TRAILER: &str =
    "🤖 Generated with [mimofan](https://github.com/XiaomingX/mimofan)\n\n\
     Co-Authored-By: mimofan <noreply@xiaoming.com>";

/// 若 `enabled` 且消息尚未包含 mimofan 的 Co-Authored-By 署名，则追加 trailer。
///
/// 防重复：消息里已出现 `Co-Authored-By: mimofan` 时直接原样返回，避免 amend
/// 同一条提交导致 trailer 叠加。
pub fn append_co_authored_by(message: &str, enabled: bool) -> String {
    if !enabled {
        return message.to_string();
    }
    if message.contains("Co-Authored-By: mimofan") {
        return message.to_string();
    }
    let trimmed = message.trim_end();
    // 规范结尾：消息与 trailer 之间空一行。
    format!("{trimmed}\n\n{MIMOFAN_CO_AUTHOR_TRAILER}")
}

#[cfg(test)]
mod co_author_tests {
    use super::*;

    #[test]
    fn appends_trailer_when_enabled() {
        let out = append_co_authored_by("feat: add thing", true);
        assert!(out.starts_with("feat: add thing\n\n"));
        assert!(out.contains("Co-Authored-By: mimofan <noreply@xiaoming.com>"));
        assert!(out.contains("Generated with [mimofan]"));
    }

    #[test]
    fn omits_trailer_when_disabled() {
        let out = append_co_authored_by("feat: add thing", false);
        assert_eq!(out, "feat: add thing");
    }

    #[test]
    fn does_not_duplicate_existing_trailer() {
        let base = "feat: add thing\n\nCo-Authored-By: mimofan <noreply@xiaoming.com>";
        let out = append_co_authored_by(base, true);
        assert_eq!(out, base, "不应重复追加 trailer");
        assert_eq!(out.matches("Co-Authored-By: mimofan").count(), 1);
    }

    #[test]
    fn preserves_trailing_whitespace_normalization() {
        let out = append_co_authored_by("feat: add thing\n\n\n", true);
        assert!(out.starts_with("feat: add thing\n\n"));
        assert!(!out.contains("\n\n\nCo-Authored-By"));
    }
}
