//! Tool-output spillover writer (#422).
//!
//! When a tool produces output that's too large to land in the model's
//! context budget, we want two things at once:
//!
//! 1. The transcript / tool-cell renders a bounded preview so the UI
//!    stays scannable.
//! 2. The full original output is preserved on disk so the model can
//!    `read_file` it back if it later needs the elided tail, and so
//!    the user can open it in `$EDITOR`.
//!
//! This module owns the disk side. Files land in
//! `~/.mimofan/tool_outputs/<sanitised-id>.txt`. The id is the tool
//! call id the engine assigns; we sanitise it conservatively (ASCII
//! alphanumeric + `-`/`_`) so a hostile id can't escape the directory
//! via `..` or absolute-path tricks.
//!
//! Boot prune drops files whose mtime is older than [`SPILLOVER_MAX_AGE`]
//! (7 days). Prune failures are logged and never fatal — the user
//! shouldn't see startup wedge because of a stale tool-output file.
//!
//! ## Live callers
//!
//! * [`apply_spillover`] — invoked from the engine's tool-execution
//!   path (`turn_loop.rs`) so any successful tool result over
//!   [`SPILLOVER_THRESHOLD_BYTES`] spills to disk and the model
//!   receives a [`SPILLOVER_HEAD_BYTES`] head plus a pointer footer.
//! * Boot prune in `main.rs` deletes files older than
//!   [`SPILLOVER_MAX_AGE`].
//!
//! UI-side rendering of the inline `full output: <path>` annotation
//! is owned by `tui/history.rs::render_spillover_annotation`. The
//! tool-details pager opens the spillover file when the user
//! presses the tool-details shortcut on a spilled tool cell.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::tools::spec::ToolResult;

// `Path` is only referenced from helpers gated to test builds.

/// Name of the spillover directory under the mimofan home.
pub const SPILLOVER_DIR_NAME: &str = "tool_outputs";

/// Default threshold above which a tool result is a candidate for
/// spillover. Mirrors the `MAX_MEMORY_SIZE` ceiling we use elsewhere
/// for "too large to inline" so the rules feel consistent. Wired
/// callers can pass a different value if a tool family has different
/// economics.
pub const SPILLOVER_THRESHOLD_BYTES: usize = 100 * 1024; // 100 KiB

/// Default boot-prune age. Older spillover files are deleted on
/// startup to keep `~/.mimofan/tool_outputs/` from growing without
/// bound. Mirrors the workspace-snapshot 7-day default.
pub const SPILLOVER_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Resolve `~/.mimofan/tool_outputs/`. Returns `None` if the home
/// directory can't be determined (CI containers occasionally hit
/// this). Callers should treat `None` as "spillover unavailable" and
/// degrade gracefully rather than fail the tool call.
#[cfg(test)]
pub(crate) static TEST_SPILLOVER_ROOT: std::sync::Mutex<Option<PathBuf>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) static TEST_SPILLOVER_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[must_use]
pub fn spillover_root() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(root) = TEST_SPILLOVER_ROOT
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
    {
        return Some(root);
    }

    let home = dirs::home_dir()?;
    let primary = home.join(".mimofan").join(SPILLOVER_DIR_NAME);
    let legacy = home.join(".deepseek").join(SPILLOVER_DIR_NAME);
    if primary.exists() || !legacy.exists() {
        return Some(primary);
    }
    Some(legacy)
}

/// Override the spillover root for tests without mutating `$HOME`.

/// Resolve the spillover-file path for a tool call id. Sanitises the
/// id so that a hostile value can't escape the storage directory.
/// Returns `None` for empty / fully-invalid ids; the caller should
/// treat that as "spillover unavailable" and skip the write.
#[must_use]
pub fn spillover_path(id: &str) -> Option<PathBuf> {
    let sanitised = sanitise_id(id)?;
    Some(spillover_root()?.join(format!("{sanitised}.txt")))
}

/// Resolve the spillover-file path for a SHA256 content hash. Separate
/// namespace (`sha_<hex>.txt`) from the tool-call-id files so the two
/// reference systems (engine-side spillover + wire-side dedup) can
/// co-exist in one directory without collisions. `sha` must be the
/// raw 64-char lowercase hex digest — case-insensitive matching is
/// done by the caller.
#[must_use]
pub fn sha_spillover_path(sha: &str) -> Option<PathBuf> {
    let sha = sha.trim().to_ascii_lowercase();
    if !is_valid_sha256(&sha) {
        return None;
    }
    Some(spillover_root()?.join(format!("sha_{sha}.txt")))
}

/// True when `s` is a 64-character lowercase ASCII hex string. Used
/// to detect bare SHA refs the model might pass to retrieval and to
/// validate input to [`sha_spillover_path`].
#[must_use]
pub fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Write content to the SHA-addressed spillover file. Idempotent —
/// the same hash always maps to the same path, and the file's bytes
/// are a function of the hash. Skips the write if the file already
/// exists (which is the common case for the wire dedup, since the
/// second sighting writes the same content that the first did).
pub fn write_sha_spillover(sha: &str, content: &str) -> io::Result<PathBuf> {
    let path = sha_spillover_path(sha).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "sha must be a 64-char lowercase hex digest",
        )
    })?;
    if path.exists() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::utils::write_atomic(&path, content.as_bytes())?;
    Ok(path)
}

/// Write `content` to the spillover file for `id`. Creates the
/// parent directory if needed. Returns the resolved path on success.
///
/// Atomic via `write` + filesystem rename guarantees from the
/// underlying OS — the file is created at a temp name first and
/// then renamed into place. Failures bubble up as `io::Error` so the
/// caller can decide whether to surface them.
pub fn write_spillover(id: &str, content: &str) -> io::Result<PathBuf> {
    let path = spillover_path(id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "could not resolve spillover path (empty/invalid id or missing home directory)",
        )
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::utils::write_atomic(&path, content.as_bytes())?;
    Ok(path)
}

/// Drop spillover files older than `max_age`. Returns the number of
/// files removed. Non-fatal: directory-missing returns 0; per-file
/// errors are logged and skipped. Mirrors
/// [`crate::session_manager::prune_workspace_snapshots`].
pub fn prune_older_than(max_age: Duration) -> io::Result<usize> {
    let Some(root) = spillover_root() else {
        return Ok(0);
    };
    if !root.exists() {
        return Ok(0);
    }
    let cutoff = SystemTime::now()
        .checked_sub(max_age)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut pruned = 0usize;
    for entry in fs::read_dir(&root)? {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(target: "spillover", ?err, "skipping unreadable dir entry");
                continue;
            }
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let modified = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(err) => {
                tracing::warn!(target: "spillover", ?err, ?path, "skipping unreadable mtime");
                continue;
            }
        };
        if modified < cutoff {
            if let Err(err) = fs::remove_file(&path) {
                tracing::warn!(target: "spillover", ?err, ?path, "spillover prune skipped a file");
                continue;
            }
            pruned += 1;
        }
    }
    Ok(pruned)
}

/// Convenience for the common "too long? spill it." pattern. If
/// `content` is at or below `threshold` bytes, returns `None` and the
/// caller keeps the inline content. Above the threshold, writes the
/// full content to the spillover file and returns
/// `Some((head, path))` where `head` is the leading slice the caller
/// can show inline. The trailing tail isn't returned — `path` is the
/// canonical reference.
///
/// `head_bytes` controls how much inline content the caller wants to
/// keep. Pass `threshold` for "preserve as much as fits inline" or
/// a smaller value (e.g. `4 * 1024`) for "show a peek".
pub fn maybe_spillover(
    id: &str,
    content: &str,
    threshold: usize,
    head_bytes: usize,
) -> io::Result<Option<(String, PathBuf)>> {
    if content.len() <= threshold {
        return Ok(None);
    }
    let path = write_spillover(id, content)?;
    // Don't slice mid-utf8: walk back to a char boundary if needed.
    let cut = head_bytes.min(content.len());
    let cut = (0..=cut)
        .rev()
        .find(|&i| content.is_char_boundary(i))
        .unwrap_or(0);
    Ok(Some((content[..cut].to_string(), path)))
}

/// Inline head retained when [`apply_spillover`] truncates a tool
/// result. 32 KiB is large enough for the model to keep meaningful
/// context (a long stack trace, a `git diff` head, a directory
/// listing of typical depth) without consuming the lion's share of
/// the per-turn context budget. The full output is preserved on
/// disk; the model can `read_file` it back if it needs the tail.
pub const SPILLOVER_HEAD_BYTES: usize = 32 * 1024;

/// Apply spillover to a tool result in place. If the result's
/// content exceeds [`SPILLOVER_THRESHOLD_BYTES`], writes the full
/// content to a sibling file under `~/.mimofan/tool_outputs/`,
/// replaces `result.content` with a [`SPILLOVER_HEAD_BYTES`] head
/// plus a footer pointing the model at the spillover file, and
/// stamps `metadata.spillover_path` so the UI can render its
/// "full output: …" annotation.
///
/// Returns the spillover path on success, `None` if no spillover
/// happened (content small enough, error result, write failure).
/// Failures are logged but never bubble up — a tool that produced a
/// result shouldn't be marked failed because the spillover writer
/// couldn't reach disk; we degrade to no-op and the model gets the
/// original (large) content.
///
/// Error results (`success == false`) are skipped: error messages
/// are typically short, and turning them into a "see file" pointer
/// would just hide the error from the model's reasoning.
#[allow(dead_code)]
pub fn apply_spillover(result: &mut ToolResult, tool_id: &str) -> Option<PathBuf> {
    apply_spillover_inner(result, tool_id, None)
}

/// Apply spillover and emit a session-scoped artifact reference.
///
/// The home-level `tool_outputs/<tool-id>.txt` file is still written
/// so `retrieve_tool_result ref=<tool-id>` keeps working during the
/// transition. The canonical artifact content is also written under
/// `~/.mimofan/sessions/<session-id>/artifacts/`, and the inline tool result
/// becomes a fixed-format artifact reference block.
pub fn apply_spillover_with_artifact(
    result: &mut ToolResult,
    tool_id: &str,
    tool_name: &str,
    session_id: &str,
) -> Option<PathBuf> {
    apply_spillover_inner(
        result,
        tool_id,
        Some(ArtifactSpilloverContext {
            tool_name,
            session_id,
        }),
    )
}

struct ArtifactSpilloverContext<'a> {
    tool_name: &'a str,
    session_id: &'a str,
}

fn apply_spillover_inner(
    result: &mut ToolResult,
    tool_id: &str,
    artifact_context: Option<ArtifactSpilloverContext<'_>>,
) -> Option<PathBuf> {
    if !result.success {
        return None;
    }
    if result.content.len() <= SPILLOVER_THRESHOLD_BYTES {
        return None;
    }
    let original_content = result.content.clone();
    let total = original_content.len();
    let outcome = match maybe_spillover(
        tool_id,
        &original_content,
        SPILLOVER_THRESHOLD_BYTES,
        SPILLOVER_HEAD_BYTES,
    ) {
        Ok(Some(pair)) => pair,
        Ok(None) => return None,
        Err(err) => {
            tracing::warn!(
                target: "spillover",
                ?err,
                tool_id,
                "spillover write failed; passing original content through"
            );
            return None;
        }
    };
    let (head, path) = outcome;
    let path_str = path.display().to_string();

    let mut artifact_path = None;
    if let Some(context) = artifact_context {
        let artifact_id = crate::artifacts::artifact_id_for_tool_call(tool_id);
        match crate::artifacts::write_session_artifact(
            context.session_id,
            &artifact_id,
            &original_content,
        ) {
            Ok((absolute_path, relative_path)) => {
                let record = crate::artifacts::record_tool_output_artifact(
                    context.session_id,
                    tool_id,
                    context.tool_name,
                    relative_path.clone(),
                    &original_content,
                );
                let transcript_ref = crate::artifacts::TranscriptArtifactRef::from(&record);
                result.content = crate::artifacts::render_transcript_artifact_ref(&transcript_ref);
                artifact_path = Some((absolute_path, relative_path, record));
            }
            Err(err) => {
                tracing::warn!(
                    target: "spillover",
                    ?err,
                    tool_id,
                    "session artifact write failed; falling back to legacy spillover footer"
                );
            }
        }
    }

    if artifact_path.is_none() {
        let footer = format!(
            "\n\n[Output truncated: {head_kib} KiB of {total_kib} KiB shown. \
             Full output saved to {path_str}. Use \
             `retrieve_tool_result ref={tool_id} mode=tail` or \
             `retrieve_tool_result ref={tool_id} mode=query query=<text>` \
             if you need the elided output.]",
            head_kib = head.len() / 1024,
            total_kib = total / 1024,
        );
        result.content = format!("{head}{footer}");
    }

    let metadata = result.metadata.get_or_insert_with(|| serde_json::json!({}));
    if let Some(obj) = metadata.as_object_mut() {
        if let Some((absolute_path, relative_path, record)) = artifact_path.as_ref() {
            obj.insert(
                "spillover_path".into(),
                serde_json::Value::String(absolute_path.display().to_string()),
            );
            obj.insert(
                "legacy_spillover_path".into(),
                serde_json::Value::String(path_str),
            );
            obj.insert(
                "artifact_id".into(),
                serde_json::Value::String(record.id.clone()),
            );
            obj.insert(
                "artifact_session_id".into(),
                serde_json::Value::String(record.session_id.clone()),
            );
            obj.insert(
                "artifact_relative_path".into(),
                serde_json::Value::String(crate::artifacts::format_artifact_relative_path(
                    relative_path,
                )),
            );
            obj.insert(
                "artifact_path".into(),
                serde_json::Value::String(absolute_path.display().to_string()),
            );
            obj.insert(
                "artifact_byte_size".into(),
                serde_json::Value::Number(serde_json::Number::from(record.byte_size)),
            );
            obj.insert(
                "artifact_preview".into(),
                serde_json::Value::String(record.preview.clone()),
            );
        } else {
            obj.insert("spillover_path".into(), serde_json::Value::String(path_str));
        }
    } else {
        // Pre-existing metadata that wasn't a JSON object (rare,
        // possibly an array). Replace with an object so we can
        // attach our key without losing prior data — wrap it under
        // a `_prior` field so callers that introspect can recover.
        let prior = std::mem::replace(metadata, serde_json::json!({}));
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert("_prior".into(), prior);
            if let Some((absolute_path, relative_path, record)) = artifact_path.as_ref() {
                obj.insert(
                    "spillover_path".into(),
                    serde_json::Value::String(absolute_path.display().to_string()),
                );
                obj.insert(
                    "legacy_spillover_path".into(),
                    serde_json::Value::String(path.display().to_string()),
                );
                obj.insert(
                    "artifact_id".into(),
                    serde_json::Value::String(record.id.clone()),
                );
                obj.insert(
                    "artifact_session_id".into(),
                    serde_json::Value::String(record.session_id.clone()),
                );
                obj.insert(
                    "artifact_relative_path".into(),
                    serde_json::Value::String(crate::artifacts::format_artifact_relative_path(
                        relative_path,
                    )),
                );
                obj.insert(
                    "artifact_path".into(),
                    serde_json::Value::String(absolute_path.display().to_string()),
                );
                obj.insert(
                    "artifact_byte_size".into(),
                    serde_json::Value::Number(serde_json::Number::from(record.byte_size)),
                );
                obj.insert(
                    "artifact_preview".into(),
                    serde_json::Value::String(record.preview.clone()),
                );
            } else {
                obj.insert(
                    "spillover_path".into(),
                    serde_json::Value::String(path.display().to_string()),
                );
            }
        }
    }
    artifact_path
        .map(|(absolute_path, _, _)| absolute_path)
        .or(Some(path))
}

/// Sanitise a tool call id for use as a filename. Keeps ASCII
/// alphanumerics, `-`, and `_`; rejects `.` to keep `..` traversal
/// out, rejects empty results. Returns `None` if the input contains
/// no acceptable characters.
fn sanitise_id(id: &str) -> Option<String> {
    let cleaned: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Override the storage roots for tests so they don't pollute the
/// user's real `~/.mimofan/` directory. This uses explicit test hooks instead
/// of `$HOME` because Windows home-dir resolution can ignore environment
/// overrides and return the runner profile directory.
#[cfg(test)]
pub(crate) fn set_test_spillover_root(root: Option<PathBuf>) -> Option<PathBuf> {
    let mut guard = TEST_SPILLOVER_ROOT
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    std::mem::replace(&mut *guard, root)
}
