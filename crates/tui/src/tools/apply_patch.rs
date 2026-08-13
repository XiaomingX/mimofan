//! Patch tools: `apply_patch` for unified diff patching
//!
//! This tool provides precise file modifications using unified diff format,
//! supporting multi-hunk patches and fuzzy matching.

use std::collections::HashSet;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    lsp_diagnostics_for_paths, optional_bool, optional_str, optional_u64, required_str,
};

/// Maximum lines of context for fuzzy matching (increased for better tolerance)
const MAX_FUZZ: usize = 50;
/// Limit how much context we print in error messages.
const HUNK_PREVIEW_LINES: usize = 4;
const SNIPPET_RADIUS: usize = 2;
const FILE_LIST_LIMIT: usize = 6;

// === Types ===

/// Result of applying a patch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchResult {
    pub success: bool,
    pub files_applied: usize,
    pub files_total: usize,
    pub hunks_applied: usize,
    pub hunks_total: usize,
    pub fuzz_used: usize,
    #[serde(default)]
    pub hunks_with_fuzz: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_summaries: Vec<FileSummary>,
    pub message: String,
}

/// Per-file summary for patch application output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSummary {
    pub path: String,
    pub hunks: usize,
    pub hunks_applied: usize,
    pub fuzz_used: usize,
    pub hunks_with_fuzz: usize,
    pub created: bool,
    pub deleted: bool,
}

/// No-mutation summary of what an `apply_patch` input intends to touch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyPatchPreflight {
    pub touched_files: Vec<String>,
    pub files_total: usize,
    pub hunks_total: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creates: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deletes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_path_mismatch: Option<String>,
}

/// A single hunk in a unified diff
#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<HunkLine>,
}

/// A line in a hunk
#[derive(Debug, Clone)]
pub enum HunkLine {
    Context(String),
    Add(String),
    Remove(String),
}

/// Tool for applying unified diff patches to files
pub struct ApplyPatchTool;

#[derive(Debug, Clone)]
struct FilePatch {
    path: String,
    hunks: Vec<Hunk>,
    delete_after: bool,
    create_if_missing: bool,
}

#[derive(Debug, Clone)]
struct PendingWrite {
    path: PathBuf,
    content: Option<String>,
    original: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct PatchStats {
    files_applied: usize,
    files_total: usize,
    hunks_applied: usize,
    hunks_total: usize,
    fuzz_used: usize,
    hunks_with_fuzz: usize,
}

#[derive(Debug, Default, Clone)]
struct PatchStatsExt {
    stats: PatchStats,
    touched_files: Vec<String>,
    file_summaries: Vec<FileSummary>,
    header_path_mismatch: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct PatchShape {
    has_hunks: bool,
    header_files: Vec<String>,
}

impl PatchShape {
    fn file_count(&self) -> usize {
        self.header_files.len()
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct HunkApplyStats {
    hunks_applied: usize,
    fuzz_used: usize,
    hunks_with_fuzz: usize,
}

#[derive(Debug, Clone)]
enum ApplyPatchPreflightKind {
    Changes,
    PathOverride { path: String, hunks: Vec<Hunk> },
    FilePatches(Vec<FilePatch>),
}

#[derive(Debug, Clone)]
struct ApplyPatchPreflightPlan {
    summary: ApplyPatchPreflight,
    kind: ApplyPatchPreflightKind,
}

// === Errors ===

#[derive(Debug, Error)]
enum ApplyHunkError {
    #[error(
        "Failed to find matching location for hunk (expected at line {expected_line}, adjusted to {adjusted_line} with offset {offset:+})"
    )]
    NoMatch {
        expected_line: usize,
        adjusted_line: usize,
        offset: isize,
    },
}

#[async_trait]
impl ToolSpec for ApplyPatchTool {
    fn name(&self) -> &'static str {
        "apply_patch"
    }

    fn description(&self) -> &'static str {
        "Apply a unified-diff patch (multi-hunk, multi-file). Use this instead of `git apply`, `patch`, or repeated `edit_file` calls in `exec_shell` — single transactional change with fuzzy matching and a rendered diff."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to patch (relative to workspace)"
                },
                "patch": {
                    "type": "string",
                    "description": "Unified diff patch content"
                },
                "changes": {
                    "type": "array",
                    "description": "Optional full file replacements (path + content).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["path", "content"]
                    }
                },
                "fuzz": {
                    "type": "integer",
                    "description": "Maximum fuzz factor for fuzzy matching (default: 3)"
                },
                "create_if_missing": {
                    "type": "boolean",
                    "description": "Create the file if it doesn't exist (for new file patches)"
                }
            },
            "oneOf": [
                { "required": ["patch"] },
                { "required": ["changes"] }
            ]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::WritesFiles,
            ToolCapability::Sandboxable,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Suggest
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let fuzz = optional_u64(&input, "fuzz", MAX_FUZZ as u64).min(MAX_FUZZ as u64);
        let fuzz = usize::try_from(fuzz).unwrap_or(MAX_FUZZ);
        let create_if_missing = optional_bool(&input, "create_if_missing", false);
        let preflight = preflight_apply_patch_plan(&input)?;

        if let Some(changes_value) = input.get("changes") {
            let (pending, stats) =
                build_pending_writes_from_changes(changes_value, context).await?;
            apply_pending_writes(&pending).await?;
            // Resolve absolute paths for LSP diagnostics query.
            let abs_paths: Vec<PathBuf> = pending.iter().map(|p| p.path.clone()).collect();
            let diag_block = lsp_diagnostics_for_paths(context, &abs_paths).await;
            let result = PatchResult {
                success: true,
                files_applied: stats.stats.files_applied,
                files_total: stats.stats.files_total,
                hunks_applied: stats.stats.hunks_applied,
                hunks_total: stats.stats.hunks_total,
                fuzz_used: stats.stats.fuzz_used,
                hunks_with_fuzz: stats.stats.hunks_with_fuzz,
                touched_files: stats.touched_files.clone(),
                file_summaries: stats.file_summaries.clone(),
                message: build_summary_message(&stats),
            };
            let mut tool_result = ToolResult::json(&result)
                .map_err(|e| ToolError::execution_failed(e.to_string()))?;
            tool_result =
                tool_result.with_metadata(apply_patch_preflight_metadata(&preflight.summary));
            if !diag_block.is_empty() {
                tool_result.content.push('\n');
                tool_result.content.push_str(&diag_block);
            }
            return Ok(tool_result);
        }

        let file_patches = match preflight.kind {
            ApplyPatchPreflightKind::Changes => {
                unreachable!("changes input returned before patch execution")
            }
            ApplyPatchPreflightKind::PathOverride { path, hunks } => vec![FilePatch {
                path,
                hunks,
                delete_after: false,
                create_if_missing,
            }],
            ApplyPatchPreflightKind::FilePatches(file_patches) => file_patches,
        };

        let (pending, mut stats) =
            build_pending_writes_from_patches(file_patches, context, fuzz).await?;
        stats.header_path_mismatch = preflight.summary.header_path_mismatch.clone();
        apply_pending_writes(&pending).await?;
        // Resolve absolute paths for LSP diagnostics query.
        let abs_paths: Vec<PathBuf> = pending
            .iter()
            .filter(|p| p.content.is_some()) // skip deleted files
            .map(|p| p.path.clone())
            .collect();
        let diag_block = lsp_diagnostics_for_paths(context, &abs_paths).await;
        let result = PatchResult {
            success: true,
            files_applied: stats.stats.files_applied,
            files_total: stats.stats.files_total,
            hunks_applied: stats.stats.hunks_applied,
            hunks_total: stats.stats.hunks_total,
            fuzz_used: stats.stats.fuzz_used,
            hunks_with_fuzz: stats.stats.hunks_with_fuzz,
            touched_files: stats.touched_files.clone(),
            file_summaries: stats.file_summaries.clone(),
            message: build_summary_message(&stats),
        };
        let mut tool_result =
            ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        tool_result = tool_result.with_metadata(apply_patch_preflight_metadata(&preflight.summary));
        if !diag_block.is_empty() {
            tool_result.content.push('\n');
            tool_result.content.push_str(&diag_block);
        }
        Ok(tool_result)
    }
}

/// Parse `apply_patch` input into a reusable, no-mutation preflight summary.
///
/// This deliberately stops before workspace resolution or file reads. It is
/// suitable for policy checks, audit logs, diagnostics hooks, and future undo
/// planning that must know the target files before mutation.
pub fn preflight_apply_patch(input: &Value) -> Result<ApplyPatchPreflight, ToolError> {
    Ok(preflight_apply_patch_plan(input)?.summary)
}

fn preflight_apply_patch_plan(input: &Value) -> Result<ApplyPatchPreflightPlan, ToolError> {
    let create_if_missing = optional_bool(input, "create_if_missing", false);

    if let Some(changes_value) = input.get("changes") {
        return Ok(ApplyPatchPreflightPlan {
            summary: preflight_changes(changes_value)?,
            kind: ApplyPatchPreflightKind::Changes,
        });
    }

    let patch_text = required_str(input, "patch")?;
    let path_override = optional_str(input, "path");
    let patch_shape = inspect_patch_shape(patch_text);
    validate_patch_shape(&patch_shape, path_override)?;
    let header_path_mismatch =
        path_override.and_then(|path| diff_header_mismatch(path, &patch_shape));

    if let Some(path) = path_override {
        let hunks = parse_unified_diff(patch_text)?;
        if hunks.is_empty() {
            return Err(ToolError::invalid_input(
                "Patch did not contain any hunks (`@@ ... @@`). Provide a unified diff hunk.",
            ));
        }
        return Ok(ApplyPatchPreflightPlan {
            summary: ApplyPatchPreflight {
                touched_files: vec![path.to_string()],
                files_total: 1,
                hunks_total: hunks.len(),
                creates: if create_if_missing {
                    vec![path.to_string()]
                } else {
                    Vec::new()
                },
                deletes: Vec::new(),
                path_override: Some(path.to_string()),
                header_path_mismatch,
            },
            kind: ApplyPatchPreflightKind::PathOverride {
                path: path.to_string(),
                hunks,
            },
        });
    }

    let file_patches = parse_unified_diff_files(patch_text, create_if_missing)?;
    if file_patches.is_empty() {
        return Err(ToolError::invalid_input(
            "No valid file patches found. Ensure the patch includes `---`/`+++` headers or provide `path`.",
        ));
    }

    let mut touched_files = Vec::new();
    let mut creates = Vec::new();
    let mut deletes = Vec::new();
    let mut hunks_total = 0;
    for file_patch in &file_patches {
        if file_patch.hunks.is_empty() {
            return Err(ToolError::invalid_input(format!(
                "Patch section for `{}` has no hunks (`@@ ... @@`).",
                file_patch.path
            )));
        }
        push_unique(&mut touched_files, file_patch.path.clone());
        hunks_total += file_patch.hunks.len();
        if file_patch.create_if_missing && !file_patch.delete_after {
            push_unique(&mut creates, file_patch.path.clone());
        }
        if file_patch.delete_after {
            push_unique(&mut deletes, file_patch.path.clone());
        }
    }

    Ok(ApplyPatchPreflightPlan {
        summary: ApplyPatchPreflight {
            files_total: file_patches.len(),
            touched_files,
            hunks_total,
            creates,
            deletes,
            path_override: None,
            header_path_mismatch,
        },
        kind: ApplyPatchPreflightKind::FilePatches(file_patches),
    })
}

fn preflight_changes(changes_value: &Value) -> Result<ApplyPatchPreflight, ToolError> {
    let changes = changes_value.as_array().ok_or_else(|| {
        ToolError::invalid_input("`changes` must be an array of objects like {path, content}")
    })?;
    if changes.is_empty() {
        return Err(ToolError::invalid_input("`changes` cannot be empty"));
    }

    let mut touched_files = Vec::new();
    for change in changes {
        let path = change
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::missing_field("changes[].path"))?;
        let _content = change
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::missing_field("changes[].content"))?;
        push_unique(&mut touched_files, path.to_string());
    }

    Ok(ApplyPatchPreflight {
        files_total: changes.len(),
        touched_files,
        hunks_total: 0,
        creates: Vec::new(),
        deletes: Vec::new(),
        path_override: None,
        header_path_mismatch: None,
    })
}

fn apply_patch_preflight_metadata(preflight: &ApplyPatchPreflight) -> Value {
    let mut metadata =
        serde_json::to_value(preflight).expect("ApplyPatchPreflight should serialize");
    if let Some(object) = metadata.as_object_mut() {
        object.insert("event".to_string(), json!("apply_patch.preflight"));
    }
    metadata
}

/// Parse a unified diff into hunks
fn parse_unified_diff(patch: &str) -> Result<Vec<Hunk>, ToolError> {
    let mut hunks = Vec::new();
    let mut lines = patch.lines().peekable();

    // Skip header lines (---, +++ etc)
    while let Some(line) = lines.peek() {
        if line.starts_with("@@") {
            break;
        }
        lines.next();
    }

    // Parse hunks
    while let Some(line) = lines.next() {
        if line.starts_with("@@") {
            let hunk = parse_hunk_header(line, &mut lines)?;
            hunks.push(hunk);
        }
    }

    Ok(hunks)
}

fn parse_unified_diff_files(
    patch: &str,
    create_if_missing: bool,
) -> Result<Vec<FilePatch>, ToolError> {
    let mut files = Vec::new();
    let mut lines = patch.lines().peekable();
    let mut current: Option<FilePatch> = None;
    let mut old_path: Option<String> = None;

    while let Some(line) = lines.next() {
        if line.starts_with("diff --git ") {
            if let Some(file) = current.take() {
                files.push(file);
            }
            old_path = None;
            continue;
        }

        if let Some(stripped) = line.strip_prefix("--- ") {
            old_path = Some(stripped.trim().to_string());
            continue;
        }

        if let Some(stripped) = line.strip_prefix("+++ ") {
            let new_path = Some(stripped.trim().to_string());
            let (path, delete_after, create_flag) =
                resolve_diff_paths(old_path.as_deref(), new_path.as_deref(), create_if_missing)?;
            old_path = None;
            if let Some(file) = current.take() {
                files.push(file);
            }
            current = Some(FilePatch {
                path,
                hunks: Vec::new(),
                delete_after,
                create_if_missing: create_flag,
            });
            continue;
        }

        if line.starts_with("@@") {
            let Some(file) = current.as_mut() else {
                if let Some(path) = old_path.as_deref() {
                    return Err(ToolError::invalid_input(format!(
                        "Patch hunk encountered after `--- {path}` but before a matching `+++` header. Each file section must include both headers."
                    )));
                }
                return Err(ToolError::invalid_input(
                    "Patch hunk encountered before any file header. Add `---`/`+++` headers or provide `path`.",
                ));
            };
            let hunk = parse_hunk_header(line, &mut lines)?;
            file.hunks.push(hunk);
        }
    }

    if let Some(file) = current {
        files.push(file);
    }

    Ok(files)
}

fn resolve_diff_paths(
    old_path: Option<&str>,
    new_path: Option<&str>,
    create_if_missing: bool,
) -> Result<(String, bool, bool), ToolError> {
    let old_norm = old_path.and_then(normalize_diff_path);
    let new_norm = new_path.and_then(normalize_diff_path);
    let delete_after = new_norm.is_none();
    let create_flag = create_if_missing || old_norm.is_none();
    let path = new_norm
        .or(old_norm)
        .ok_or_else(|| ToolError::invalid_input("Patch is missing both old and new file paths"))?;
    Ok((path, delete_after, create_flag))
}

fn normalize_diff_path(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw == "/dev/null" || raw == "dev/null" {
        return None;
    }
    let raw = raw
        .strip_prefix("a/")
        .or_else(|| raw.strip_prefix("b/"))
        .unwrap_or(raw);
    Some(raw.to_string())
}

/// Parse a hunk header and its content
fn parse_hunk_header<'a, I>(
    header: &str,
    lines: &mut std::iter::Peekable<I>,
) -> Result<Hunk, ToolError>
where
    I: Iterator<Item = &'a str>,
{
    // Parse @@ -old_start,old_count +new_start,new_count @@
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(ToolError::invalid_input(format!(
            "Invalid hunk header: {header}. Expected `@@ -start,count +start,count @@`."
        )));
    }

    let old_range = parts[1].trim_start_matches('-');
    let new_range = parts[2].trim_start_matches('+');

    let (old_start, old_count) = parse_range(old_range)?;
    let (new_start, new_count) = parse_range(new_range)?;

    // Parse hunk lines
    let mut hunk_lines = Vec::new();
    let expected_lines = old_count.max(new_count) + old_count.min(new_count);

    for _ in 0..expected_lines * 2 {
        // Allow for more lines than expected
        match lines.peek() {
            Some(line) if line.starts_with("@@") => break,
            Some(line) if line.starts_with('-') => {
                hunk_lines.push(HunkLine::Remove(line[1..].to_string()));
                lines.next();
            }
            Some(line) if line.starts_with('+') => {
                hunk_lines.push(HunkLine::Add(line[1..].to_string()));
                lines.next();
            }
            Some(line) if line.starts_with(' ') || line.is_empty() => {
                let content = if line.is_empty() { "" } else { &line[1..] };
                hunk_lines.push(HunkLine::Context(content.to_string()));
                lines.next();
            }
            Some(line)
                if line.starts_with("diff ")
                    || line.starts_with("--- ")
                    || line.starts_with("+++ ") =>
            {
                // Start of a new file patch - don't consume, let outer loop handle it
                break;
            }
            Some(line) if !line.starts_with('\\') => {
                // Treat as context line without leading space
                hunk_lines.push(HunkLine::Context((*line).to_string()));
                lines.next();
            }
            Some(_) => {
                lines.next(); // Skip "\ No newline at end of file" etc
            }
            None => break,
        }
    }

    Ok(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: hunk_lines,
    })
}

/// Parse a range like "10,5" or "10" into (start, count)
fn parse_range(range: &str) -> Result<(usize, usize), ToolError> {
    let parts: Vec<&str> = range.split(',').collect();
    let start = parts[0].parse::<usize>().map_err(|_| {
        ToolError::invalid_input(format!(
            "Invalid line number `{}` in hunk header. Use positive integers like `12` or `12,3`.",
            parts[0]
        ))
    })?;
    let count = if parts.len() > 1 {
        parts[1].parse::<usize>().map_err(|_| {
            ToolError::invalid_input(format!(
                "Invalid line count `{}` in hunk header. Use positive integers like `3`.",
                parts[1]
            ))
        })?
    } else {
        1
    };
    Ok((start, count))
}

fn inspect_patch_shape(patch: &str) -> PatchShape {
    let mut shape = PatchShape::default();
    let mut seen = HashSet::new();
    let mut old_path: Option<String> = None;

    for line in patch.lines() {
        if line.starts_with("@@") {
            shape.has_hunks = true;
        }

        if let Some(stripped) = line.strip_prefix("--- ") {
            old_path = normalize_diff_path(stripped);
            continue;
        }

        if let Some(stripped) = line.strip_prefix("+++ ") {
            let new_path = normalize_diff_path(stripped);
            let resolved = new_path.or(old_path.clone());
            if let Some(path) = resolved
                && seen.insert(path.clone())
            {
                shape.header_files.push(path);
            }
            old_path = None;
        }
    }

    shape
}

fn validate_patch_shape(shape: &PatchShape, path_override: Option<&str>) -> Result<(), ToolError> {
    if !shape.has_hunks {
        return Err(ToolError::invalid_input(
            "Patch must include at least one hunk header (`@@ -start,count +start,count @@`).",
        ));
    }

    match path_override {
        Some(_) if shape.file_count() > 1 => Err(ToolError::invalid_input(format!(
            "Patch references multiple files ({}) but `path` was provided. Remove `path` to apply a multi-file patch, or provide a single-file patch.",
            format_file_list(&shape.header_files),
        ))),
        None if shape.file_count() == 0 => Err(ToolError::invalid_input(
            "Patch contains hunks but no file headers (`---`/`+++`). Provide `path` or add headers.",
        )),
        _ => Ok(()),
    }
}

fn diff_header_mismatch(path_override: &str, shape: &PatchShape) -> Option<String> {
    if shape.file_count() != 1 {
        return None;
    }
    let header_path = &shape.header_files[0];
    let override_norm = normalize_diff_path(path_override).unwrap_or_else(|| path_override.into());
    if &override_norm == header_path {
        None
    } else {
        Some(format!(
            "Note: patch headers reference `{header_path}` but `path` overrides to `{override_norm}`."
        ))
    }
}

fn build_summary_message(stats: &PatchStatsExt) -> String {
    let mut parts = Vec::new();
    if stats.stats.hunks_total > 0 {
        parts.push(format!(
            "Applied {}/{} hunks across {} file(s).",
            stats.stats.hunks_applied, stats.stats.hunks_total, stats.stats.files_applied
        ));
    } else {
        parts.push(format!(
            "Applied {} file change(s).",
            stats.stats.files_applied
        ));
    }

    if !stats.touched_files.is_empty() {
        parts.push(format!(
            "Files: {}.",
            format_file_list(&stats.touched_files)
        ));
    }

    if stats.stats.fuzz_used > 0 {
        parts.push(format!(
            "Fuzz used on {} hunk(s) (total fuzz: {}).",
            stats.stats.hunks_with_fuzz, stats.stats.fuzz_used
        ));
    }

    if let Some(note) = stats.header_path_mismatch.as_deref() {
        parts.push(note.to_string());
    }

    parts.join(" ")
}

fn format_file_list(files: &[String]) -> String {
    if files.is_empty() {
        return "<none>".to_string();
    }
    let mut shown: Vec<String> = files.iter().take(FILE_LIST_LIMIT).cloned().collect();
    let remaining = files.len().saturating_sub(shown.len());
    if remaining > 0 {
        shown.push(format!("... (+{remaining} more)"));
    }
    shown.join(", ")
}

fn push_unique(target: &mut Vec<String>, value: String) {
    if !target.iter().any(|existing| existing == &value) {
        target.push(value);
    }
}

async fn build_pending_writes_from_changes(
    changes_value: &Value,
    context: &ToolContext,
) -> Result<(Vec<PendingWrite>, PatchStatsExt), ToolError> {
    let changes = changes_value.as_array().ok_or_else(|| {
        ToolError::invalid_input("`changes` must be an array of objects like {path, content}")
    })?;
    if changes.is_empty() {
        return Err(ToolError::invalid_input("`changes` cannot be empty"));
    }

    let mut pending = Vec::new();
    let mut stats = PatchStatsExt::default();
    for change in changes {
        let path = change
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::missing_field("changes[].path"))?;
        let content = change
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::missing_field("changes[].content"))?;

        let resolved = context.resolve_path(path)?;
        // Rewriting an existing file requires a fresh prior read; creating a
        // new one does not. Checked here, during planning, so a violation
        // aborts before `apply_pending_writes` touches the disk.
        if resolved.exists() {
            context.require_fresh_file_read_for("apply_patch", &resolved, path)?;
        }
        let original = if resolved.exists() {
            Some(read_file_content(&resolved).await?)
        } else {
            None
        };
        let created = original.is_none();

        pending.push(PendingWrite {
            path: resolved,
            content: Some(content.to_string()),
            original,
        });

        stats.stats.files_total += 1;
        stats.stats.files_applied += 1;
        push_unique(&mut stats.touched_files, path.to_string());
        stats.file_summaries.push(FileSummary {
            path: path.to_string(),
            hunks: 0,
            hunks_applied: 0,
            fuzz_used: 0,
            hunks_with_fuzz: 0,
            created,
            deleted: false,
        });
    }

    Ok((pending, stats))
}

async fn build_pending_writes_from_patches(
    file_patches: Vec<FilePatch>,
    context: &ToolContext,
    fuzz: usize,
) -> Result<(Vec<PendingWrite>, PatchStatsExt), ToolError> {
    let mut pending = Vec::new();
    let mut stats = PatchStatsExt::default();
    stats.stats.files_total = file_patches.len();

    for file_patch in file_patches {
        if file_patch.hunks.is_empty() {
            return Err(ToolError::invalid_input(format!(
                "Patch section for `{}` has no hunks (`@@ ... @@`).",
                file_patch.path
            )));
        }

        let resolved = context.resolve_path(&file_patch.path)?;
        // Patching or deleting an existing file requires a fresh prior read;
        // creating a new one does not. Every touched file is checked, and the
        // whole batch aborts before any write lands.
        if resolved.exists() {
            context.require_fresh_file_read_for("apply_patch", &resolved, &file_patch.path)?;
        }
        let original = if resolved.exists() {
            Some(read_file_content(&resolved).await?)
        } else {
            None
        };

        if original.is_none() && !file_patch.create_if_missing {
            return Err(ToolError::execution_failed(format!(
                "File `{}` does not exist at `{}`. Set create_if_missing=true for new files or include headers for file creation.",
                file_patch.path,
                resolved.display(),
            )));
        }

        if file_patch.delete_after && original.is_none() {
            return Err(ToolError::execution_failed(format!(
                "File `{}` does not exist at `{}` to delete.",
                file_patch.path,
                resolved.display(),
            )));
        }

        let base_content = original.clone().unwrap_or_default();
        let mut lines: Vec<String> = if base_content.is_empty() {
            Vec::new()
        } else {
            base_content.lines().map(String::from).collect()
        };

        let apply_stats =
            apply_hunks_to_lines(&mut lines, &file_patch.hunks, fuzz, &file_patch.path)?;
        stats.stats.hunks_applied += apply_stats.hunks_applied;
        stats.stats.hunks_total += file_patch.hunks.len();
        stats.stats.fuzz_used += apply_stats.fuzz_used;
        stats.stats.hunks_with_fuzz += apply_stats.hunks_with_fuzz;
        stats.stats.files_applied += 1;
        push_unique(&mut stats.touched_files, file_patch.path.clone());
        stats.file_summaries.push(FileSummary {
            path: file_patch.path.clone(),
            hunks: file_patch.hunks.len(),
            hunks_applied: apply_stats.hunks_applied,
            fuzz_used: apply_stats.fuzz_used,
            hunks_with_fuzz: apply_stats.hunks_with_fuzz,
            created: original.is_none() && !file_patch.delete_after,
            deleted: file_patch.delete_after,
        });

        if file_patch.delete_after {
            pending.push(PendingWrite {
                path: resolved,
                content: None,
                original,
            });
        } else {
            let new_content = lines.join("\n");
            pending.push(PendingWrite {
                path: resolved,
                content: Some(new_content),
                original,
            });
        }
    }

    Ok((pending, stats))
}

async fn apply_pending_writes(pending: &[PendingWrite]) -> Result<(), ToolError> {
    let mut applied = Vec::new();

    for entry in pending {
        let result = if let Some(content) = entry.content.as_ref() {
            if let Some(parent) = entry.path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::execution_failed(format!(
                        "Failed to create directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
            tokio::fs::write(&entry.path, content).await.map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to write {}: {}",
                    entry.path.display(),
                    e
                ))
            })
        } else if entry.path.exists() {
            tokio::fs::remove_file(&entry.path).await.map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to delete {}: {}",
                    entry.path.display(),
                    e
                ))
            })
        } else {
            Ok(())
        };

        if let Err(err) = result {
            rollback_pending_writes(&applied).await;
            return Err(err);
        }

        applied.push(entry.clone());
    }

    Ok(())
}

async fn rollback_pending_writes(applied: &[PendingWrite]) {
    for entry in applied.iter().rev() {
        match entry.original.as_ref() {
            Some(content) => {
                let _ = tokio::fs::write(&entry.path, content).await;
            }
            None => {
                let _ = tokio::fs::remove_file(&entry.path).await;
            }
        }
    }
}

async fn read_file_content(path: &PathBuf) -> Result<String, ToolError> {
    tokio::fs::read_to_string(path).await.map_err(|e| {
        ToolError::execution_failed(format!("Failed to read {}: {}", path.display(), e))
    })
}

fn preview_expected_lines(hunk: &Hunk, limit: usize) -> Vec<String> {
    let mut preview = Vec::new();
    for line in hunk.lines.iter().filter_map(|line| match line {
        HunkLine::Context(s) => Some((" ", s)),
        HunkLine::Remove(s) => Some(("-", s)),
        HunkLine::Add(_) => None,
    }) {
        if preview.len() >= limit {
            break;
        }
        preview.push(format!("  {}{}", line.0, line.1));
    }
    if preview.is_empty() {
        preview.push("  <no context lines in hunk>".to_string());
    }
    preview
}

fn snippet_around(lines: &[String], line_1_based: usize, radius: usize) -> Vec<String> {
    if lines.is_empty() {
        return vec!["  <empty file>".to_string()];
    }

    let center = line_1_based
        .saturating_sub(1)
        .min(lines.len().saturating_sub(1));
    let start = center.saturating_sub(radius);
    let end = (center + radius).min(lines.len().saturating_sub(1));

    lines[start..=end]
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let line_no = start + idx + 1;
            format!("  {line_no:>4}: {line}")
        })
        .collect()
}

fn format_hunk_no_match_error(
    lines: &[String],
    hunk: &Hunk,
    err: &ApplyHunkError,
    max_fuzz: usize,
) -> String {
    match err {
        ApplyHunkError::NoMatch {
            expected_line,
            adjusted_line,
            offset,
        } => {
            let expected_preview = preview_expected_lines(hunk, HUNK_PREVIEW_LINES).join("\n");
            let file_preview = snippet_around(lines, *adjusted_line, SNIPPET_RADIUS).join("\n");
            format!(
                "could not find matching context near line {expected_line} (searched around line {adjusted_line} with offset {offset:+} and fuzz up to {max_fuzz}). Expected context preview:\n{expected_preview}\nFile snippet near line {adjusted_line}:\n{file_preview}\nHints: ensure the patch matches the current file contents, increase `fuzz`, or regenerate the patch."
            )
        }
    }
}

fn apply_hunks_to_lines(
    lines: &mut Vec<String>,
    hunks: &[Hunk],
    fuzz: usize,
    file_label: &str,
) -> Result<HunkApplyStats, ToolError> {
    let mut stats = HunkApplyStats::default();
    let mut cumulative_offset: isize = 0;

    for (idx, hunk) in hunks.iter().enumerate() {
        match apply_hunk(lines, hunk, fuzz, &mut cumulative_offset) {
            Ok(fuzz_used) => {
                stats.fuzz_used += fuzz_used;
                stats.hunks_applied += 1;
                if fuzz_used > 0 {
                    stats.hunks_with_fuzz += 1;
                }
            }
            Err(e) => {
                let detail = format_hunk_no_match_error(lines, hunk, &e, fuzz);
                return Err(ToolError::execution_failed(format!(
                    "Failed to apply hunk {}/{} for `{}`: {}",
                    idx + 1,
                    hunks.len(),
                    file_label,
                    detail
                )));
            }
        }
    }

    Ok(stats)
}

/// Apply a hunk to the file content with fuzzy matching
fn apply_hunk(
    lines: &mut Vec<String>,
    hunk: &Hunk,
    max_fuzz: usize,
    cumulative_offset: &mut isize,
) -> Result<usize, ApplyHunkError> {
    // Build expected old lines from hunk
    let old_lines: Vec<&str> = hunk
        .lines
        .iter()
        .filter_map(|line| match line {
            HunkLine::Context(s) | HunkLine::Remove(s) => Some(s.as_str()),
            HunkLine::Add(_) => None,
        })
        .collect();

    // Build new lines from hunk
    let new_lines: Vec<String> = hunk
        .lines
        .iter()
        .filter_map(|line| match line {
            HunkLine::Context(s) | HunkLine::Add(s) => Some(s.clone()),
            HunkLine::Remove(_) => None,
        })
        .collect();

    // Try to find the location with fuzzy matching
    // Apply cumulative offset from previous hunks, clamping to valid range.
    let base_idx = if hunk.old_start > 0 {
        hunk.old_start - 1
    } else {
        0
    };
    // Use checked_add_signed to safely handle negative offsets without
    // risking isize overflow on adversarial input.
    let start_idx = base_idx
        .checked_add_signed(*cumulative_offset)
        .unwrap_or(0)
        .min(lines.len());

    for fuzz in 0..=max_fuzz {
        // Try at exact position first, then nearby
        let search_range = if fuzz == 0 {
            vec![start_idx]
        } else {
            let min = start_idx.saturating_sub(fuzz);
            let max = (start_idx + fuzz).min(lines.len());
            (min..=max).collect()
        };

        for pos in search_range {
            if matches_at_position(lines, &old_lines, pos) {
                // Apply the hunk
                let end_pos = pos + old_lines.len();
                lines.splice(pos..end_pos, new_lines.clone());

                // Update cumulative offset: new lines added minus old lines removed
                let delta = new_lines.len() as isize - old_lines.len() as isize;
                *cumulative_offset += delta;

                return Ok(fuzz);
            }
        }
    }

    // Special case: adding to empty file or new hunk at end
    if old_lines.is_empty() && (lines.is_empty() || start_idx >= lines.len()) {
        let delta = new_lines.len() as isize;
        lines.extend(new_lines);
        *cumulative_offset += delta;
        return Ok(0);
    }

    Err(ApplyHunkError::NoMatch {
        expected_line: hunk.old_start,
        adjusted_line: start_idx + 1, // Convert back to 1-indexed
        offset: *cumulative_offset,
    })
}

/// Check if `old_lines` match at the given position
fn matches_at_position(lines: &[String], old_lines: &[&str], pos: usize) -> bool {
    if pos + old_lines.len() > lines.len() {
        return false;
    }

    for (i, old_line) in old_lines.iter().enumerate() {
        // Normalize whitespace for comparison
        let file_line = lines[pos + i].trim_end();
        let expected = old_line.trim_end();
        if file_line != expected {
            return false;
        }
    }

    true
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx(dir: &TempDir) -> ToolContext {
        ToolContext::new(dir.path().to_path_buf())
    }

    /// A minimal single-hunk diff replacing `one` with `ONE`.
    fn patch_text() -> &'static str {
        "@@ -1,2 +1,2 @@\n-one\n+ONE\n two\n"
    }

    #[tokio::test]
    async fn apply_patch_rejects_patching_an_unread_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("target.txt");
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let ctx = ctx(&dir);

        let err = ApplyPatchTool
            .execute(json!({ "path": "target.txt", "patch": patch_text() }), &ctx)
            .await
            .expect_err("patching an unread file must be refused");

        let msg = err.to_string();
        assert!(msg.contains("apply_patch"), "{msg}");
        assert!(msg.contains("never_read"), "{msg}");
        // Nothing may reach disk when the guard trips.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
    }

    #[tokio::test]
    async fn apply_patch_allows_patching_after_a_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("target.txt");
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let ctx = ctx(&dir);
        // Note the read with the same resolved (canonicalized) path the guard
        // compares against, so the snapshot key matches regardless of /var vs
        // /private/var symlink normalisation on macOS.
        let resolved = ctx.resolve_path("target.txt").unwrap();
        ctx.note_file_read(&resolved);

        ApplyPatchTool
            .execute(json!({ "path": "target.txt", "patch": patch_text() }), &ctx)
            .await
            .expect("patching after a read must be allowed");

        assert!(std::fs::read_to_string(&path).unwrap().starts_with("ONE"));
    }

    #[tokio::test]
    async fn apply_patch_changes_rejects_unread_overwrite_but_allows_creation() {
        let dir = TempDir::new().unwrap();
        let existing = dir.path().join("existing.txt");
        std::fs::write(&existing, "original\n").unwrap();
        let ctx = ctx(&dir);

        // Overwriting an unread file via `changes` is refused...
        let err = ApplyPatchTool
            .execute(
                json!({ "changes": [{ "path": "existing.txt", "content": "clobbered\n" }] }),
                &ctx,
            )
            .await
            .expect_err("unread overwrite must be refused");
        assert!(err.to_string().contains("never_read"), "{err}");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "original\n");

        // ...while creating a brand-new file stays allowed.
        ApplyPatchTool
            .execute(
                json!({ "changes": [{ "path": "fresh.txt", "content": "new\n" }] }),
                &ctx,
            )
            .await
            .expect("creating a new file must be allowed");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("fresh.txt")).unwrap(),
            "new\n",
        );
    }

    #[tokio::test]
    async fn apply_patch_batch_aborts_before_writing_when_one_file_is_unread() {
        let dir = TempDir::new().unwrap();
        let read_file = dir.path().join("a.txt");
        let unread_file = dir.path().join("b.txt");
        std::fs::write(&read_file, "alpha\n").unwrap();
        std::fs::write(&unread_file, "beta\n").unwrap();
        let ctx = ctx(&dir);
        // Only the first file was read. Resolve it the same way the guard
        // does so the snapshot key matches.
        let read_resolved = ctx.resolve_path("a.txt").unwrap();
        ctx.note_file_read(&read_resolved);

        let err = ApplyPatchTool
            .execute(
                json!({ "changes": [
                    { "path": "a.txt", "content": "alpha changed\n" },
                    { "path": "b.txt", "content": "beta changed\n" },
                ] }),
                &ctx,
            )
            .await
            .expect_err("a single unread file must fail the whole batch");
        assert!(err.to_string().contains("never_read"), "{err}");

        // The transaction must not have partially applied: even the file that
        // *was* read stays untouched.
        assert_eq!(std::fs::read_to_string(&read_file).unwrap(), "alpha\n");
        assert_eq!(std::fs::read_to_string(&unread_file).unwrap(), "beta\n");
    }

    /// One first-try application of a single-hunk unified diff.
    ///
    /// Returns the parsed [`PatchResult`] on success, or the tool error string
    /// when the guard refuses the mutation. The caller decides what counts as
    /// a "first try" success: `success && hunks_applied == hunks_total &&
    /// fuzz_used == 0`.
    async fn apply_first_try(
        dir: &TempDir,
        rel_path: &str,
        original: &str,
        patch: &str,
    ) -> Result<PatchResult, String> {
        let path = dir.path().join(rel_path);
        std::fs::write(&path, original).unwrap();
        let ctx = ctx(dir);
        // Resolve the same canonicalized path the guard compares against so the
        // read snapshot key matches regardless of /var vs /private/var symlinks.
        let resolved = ctx.resolve_path(rel_path).unwrap();
        ctx.note_file_read(&resolved);

        let result = ApplyPatchTool
            .execute(json!({ "path": rel_path, "patch": patch }), &ctx)
            .await
            .map_err(|e| e.to_string())?;

        // `ToolResult::json` serialises the `PatchResult` into `content`.
        serde_json::from_str::<PatchResult>(&result.content)
            .map_err(|e| format!("failed to parse PatchResult from content: {e}"))
    }

    /// #689 — Edit/Apply first-try success-rate benchmark.
    ///
    /// Constructs a small polyglot corpus of *correct* single- and multi-hunk
    /// unified diffs (the kind a model would emit on a clean read) and measures
    /// how many apply without fuzzing on the very first attempt. This is the
    /// empirical floor for "one-shot edit success", distinct from fuzzy-recovery
    /// (which only kicks in after a first try has already failed).
    ///
    /// A sample counts as first-try success when the tool reports
    /// `success && hunks_applied == hunks_total && fuzz_used == 0`. The benchmark
    /// asserts a high floor (>= 0.9) so a regression in the patch parser (e.g.
    /// stricter header handling) is caught here rather than in production.
    #[tokio::test]
    async fn edit_apply_first_try_benchmark() {
        // (rel_path, original_content, unified_diff)
        let samples: &[(&str, &str, &str)] = &[
            // Rust: replace a function body line.
            (
                "lib.rs",
                "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
                "@@ -1,3 +1,3 @@\n fn add(a: i32, b: i32) -> i32 {\n-    a + b\n+    a.wrapping_add(b)\n }\n",
            ),
            // Python: change a return value.
            (
                "main.py",
                "def greet(name):\n    return 'hi ' + name\n",
                "@@ -1,2 +1,2 @@\n def greet(name):\n-    return 'hi ' + name\n+    return f'hi {name}'\n",
            ),
            // JavaScript: swap an assignment.
            (
                "app.js",
                "const x = 1;\nconst y = 2;\n",
                "@@ -1,2 +1,2 @@\n const x = 1;\n-const y = 2;\n+const y = 3;\n",
            ),
            // Go: change a constant.
            (
                "svc.go",
                "package main\n\nconst port = 8080\n",
                "@@ -1,3 +1,3 @@\n package main\n\n-const port = 8080\n+const port = 9090\n",
            ),
            // JSON: change a field value.
            (
                "cfg.json",
                "{\n  \"name\": \"x\",\n  \"version\": 1\n}\n",
                "@@ -1,3 +1,3 @@\n {\n   \"name\": \"x\",\n-  \"version\": 1\n+  \"version\": 2\n }\n",
            ),
            // YAML: change a scalar.
            (
                "vals.yaml",
                "database:\n  host: localhost\n  port: 5432\n",
                "@@ -1,3 +1,3 @@\n database:\n   host: localhost\n-  port: 5432\n+  port: 6432\n",
            ),
            // Markdown: replace a heading line.
            (
                "README.md",
                "# Title\n\nSome intro text.\n",
                "@@ -1,2 +1,2 @@\n-# Title\n+# New Title\n\n Some intro text.\n",
            ),
            // Multi-hunk Rust: two non-adjacent edits.
            (
                "multi.rs",
                "fn a() {}\n\nfn b() {}\n\nfn c() {}\n",
                "@@ -1,1 +1,1 @@\n-fn a() {}\n+fn a() -> () {}\n@@ -3,1 +3,1 @@\n-fn b() {}\n+fn b() -> () {}\n",
            ),
        ];

        let mut first_try_ok = 0usize;
        let mut failures: Vec<(String, String)> = Vec::new();

        for (rel_path, original, patch) in samples {
            let dir = dir_for_sample();
            match apply_first_try(&dir, rel_path, original, patch).await {
                Ok(pr) if pr.success && pr.hunks_applied == pr.hunks_total && pr.fuzz_used == 0 => {
                    first_try_ok += 1;
                }
                Ok(pr) => {
                    failures.push((
                        (*rel_path).to_string(),
                        format!(
                            "success={} hunks={}/{} fuzz={} msg={}",
                            pr.success, pr.hunks_applied, pr.hunks_total, pr.fuzz_used, pr.message
                        ),
                    ));
                }
                Err(e) => failures.push(((*rel_path).to_string(), e)),
            }
        }

        let total = samples.len();
        let rate = first_try_ok as f64 / total as f64;
        println!(
            "[#689 benchmark] first-try success {first_try_ok}/{total} = {:.2}",
            rate
        );
        for (p, why) in &failures {
            println!("  miss {p}: {why}");
        }

        assert!(
            rate >= 0.9,
            "edit/apply first-try success rate {rate:.2} below 0.9 floor; misses: {failures:?}"
        );
    }

    /// Private helper that builds a fresh TempDir per sample so each attempt is
    /// fully isolated (no cross-sample filesystem state).
    fn dir_for_sample() -> TempDir {
        TempDir::new().unwrap()
    }
}
