//! `/restore` slash command — roll back the workspace to a prior snapshot.
//!
//! `/restore` (no arg) lists the 20 most recent snapshots so the user can
//! see what's available. `/restore list [N]` lists more snapshots, capped
//! at 100. `/restore <N>` restores the *N*th-most-recent snapshot, where
//! `N=1` is the newest. In non-YOLO mode we refuse to mutate files unless
//! the user has explicitly trusted the workspace (`/trust on` or YOLO) —
//! the user can always view the list, just not one-shot revert without a
//! safety net.

use super::CommandResult;
use crate::snapshot::{NameStatus, Snapshot, SnapshotRepo};
use crate::tui::app::App;
use crate::tui::history::history_cells_from_message;
use chrono::TimeZone;
use std::path::PathBuf;

const DEFAULT_LIST_LIMIT: usize = 20;
const MAX_LIST_LIMIT: usize = 100;
const MAX_RESTORE_INDEX: usize = 1000;

/// Entry point for `/restore [N|list [N]]`.
pub fn restore(app: &mut App, arg: Option<&str>) -> CommandResult {
    let workspace = app.workspace.clone();
    let repo = match SnapshotRepo::open_or_init(&workspace) {
        Ok(r) => r,
        Err(e) => {
            return CommandResult::error(format!(
                "Snapshot repo unavailable for {}: {e}",
                workspace.display(),
            ));
        }
    };

    let Some(arg) = arg.map(str::trim).filter(|s| !s.is_empty()) else {
        let snapshots = match repo.list(DEFAULT_LIST_LIMIT) {
            Ok(s) => s,
            Err(e) => return CommandResult::error(format!("Failed to list snapshots: {e}")),
        };
        if snapshots.is_empty() {
            return no_snapshots_message();
        }
        return CommandResult::message(format_listing(&snapshots));
    };

    if let Some(limit) = match parse_list_arg(arg) {
        Ok(limit) => limit,
        Err(message) => return CommandResult::error(message),
    } {
        let snapshots = match repo.list(limit) {
            Ok(s) => s,
            Err(e) => return CommandResult::error(format!("Failed to list snapshots: {e}")),
        };
        if snapshots.is_empty() {
            return no_snapshots_message();
        }
        return CommandResult::message(format_listing(&snapshots));
    }

    let n: usize = match arg.parse() {
        Ok(n) if (1..=MAX_RESTORE_INDEX).contains(&n) => n,
        Ok(n) if n > MAX_RESTORE_INDEX => {
            return CommandResult::error(format!(
                "Restore index must be <= {MAX_RESTORE_INDEX}; got {n}. Use /restore list [N] to inspect snapshots first.",
            ));
        }
        _ => {
            return CommandResult::error(format!(
                "Usage: /restore <N> or /restore list [N]  (N is 1-based; got '{arg}')",
            ));
        }
    };
    let snapshots = match repo.list(n.max(DEFAULT_LIST_LIMIT)) {
        Ok(s) => s,
        Err(e) => return CommandResult::error(format!("Failed to list snapshots: {e}")),
    };
    if snapshots.is_empty() {
        return no_snapshots_message();
    }

    if n > snapshots.len() {
        return CommandResult::error(format!(
            "Only {} snapshot(s) available; asked for #{n}.",
            snapshots.len(),
        ));
    }

    // Non-YOLO sessions get a confirmation gate. We don't have a true
    // modal-confirmation path inside slash commands today, so the gate
    // is "require trust mode" — `/trust on` or YOLO. Users in plain
    // Agent mode get a clear message explaining how to proceed.
    if !(app.yolo || app.trust_mode) {
        return CommandResult::message(format!(
            "Refusing to restore snapshot #{n} ('{}') outside trusted mode.\n\
             Run `/trust on` or `/mode yolo` first, then re-run `/restore {n}`.",
            snapshots[n - 1].label,
        ));
    }

    let target = &snapshots[n - 1];

    // Pre-rollback diff preview (matches CodeBuddy's "see what will change"
    // step) so the user isn't blindsided by a silent file revert.
    let preview = match repo.diff_name_status(&target.id) {
        Ok(changes) if !changes.is_empty() => format_preview(&changes),
        Ok(_) => String::new(),
        Err(e) => format!("(could not compute diff preview: {e})\n"),
    };

    if let Err(e) = repo.restore(&target.id) {
        return CommandResult::error(format!("Restore failed: {e}"));
    }

    // Roll back the conversation too when the snapshot recorded a conversation
    // length (combined code+chat revert, matching CodeBuddy's third mode).
    let conversation_reverted =
        if target.conversation_len > 0 && target.conversation_len <= app.api_messages.len() {
            revert_conversation_to(app, target.conversation_len);
            true
        } else {
            false
        };

    let mut msg = String::new();
    if !preview.is_empty() {
        msg.push_str(&preview);
        msg.push('\n');
    }
    msg.push_str(&format!(
        "Restored snapshot #{n} ('{}', {}).",
        target.label,
        short_sha(target.id.as_str()),
    ));
    if conversation_reverted {
        msg.push_str(" Workspace files and conversation history reverted.");
    } else {
        msg.push_str(" Workspace files have been reverted; conversation history is unchanged.");
    }

    CommandResult::message(msg)
}

/// Entry point for `/rewind [N|list [N]|chat]`.
pub fn rewind(app: &mut App, arg: Option<&str>) -> CommandResult {
    let arg = arg.map(str::trim).filter(|s| !s.is_empty());

    if let Some("chat") = arg {
        return CommandResult::action(crate::tui::app::AppAction::OpenBacktrackOverlay);
    }

    if arg.is_none() {
        let res = restore(app, None);
        if res.is_error {
            return res;
        }
        let message = res.message.unwrap_or_default();
        let tip = "\n\n💡 Tip: Type `/rewind chat` or press `Esc Esc` in TUI to backtrack conversation history without reverting workspace files.";
        return CommandResult::message(format!("{message}{tip}"));
    }

    restore(app, arg)
}

fn parse_list_arg(arg: &str) -> Result<Option<usize>, String> {
    let mut parts = arg.split_whitespace();
    let action = match parts.next() {
        Some(action) => action,
        None => return Ok(None),
    };
    if action != "list" {
        return Ok(None);
    }
    let Some(value) = parts.next() else {
        return Ok(Some(DEFAULT_LIST_LIMIT));
    };
    if parts.next().is_some() {
        return Err(format!(
            "Usage: /restore list [N]  (got extra arguments in '{arg}')",
        ));
    }
    match value.parse::<usize>() {
        Ok(limit @ 1..=MAX_LIST_LIMIT) => Ok(Some(limit)),
        Ok(limit) if limit > MAX_LIST_LIMIT => Err(format!(
            "Restore list limit must be <= {MAX_LIST_LIMIT}; got {limit}.",
        )),
        _ => Err(format!(
            "Usage: /restore list [N]  (N must be >= 1; got '{value}')",
        )),
    }
}

fn no_snapshots_message() -> CommandResult {
    CommandResult::message(
        "No snapshots yet. Send a message to create the first pre-turn snapshot.",
    )
}

/// Render a concise file-change preview (modified / added / deleted counts
/// plus the path list) for a snapshot rollback.
fn format_preview(changes: &[(NameStatus, PathBuf)]) -> String {
    let mut modified = 0;
    let mut added = 0;
    let mut deleted = 0;
    for (status, _) in changes {
        match status {
            NameStatus::Modified => modified += 1,
            NameStatus::Added => added += 1,
            NameStatus::Deleted => deleted += 1,
        }
    }
    let mut out = format!(
        "Will change {} file(s): {} modified, {} added, {} deleted",
        changes.len(),
        modified,
        added,
        deleted,
    );
    // Keep the inline preview readable; list every path for small changes,
    // cap the listing for large ones.
    let listed = changes.len().min(50);
    for (status, path) in changes.iter().take(listed) {
        out.push_str(&format!("\n  [{}] {}", status.as_str(), path.display()));
    }
    if changes.len() > listed {
        out.push_str(&format!("\n  … and {} more", changes.len() - listed));
    }
    out
}

/// Roll the conversation back to `len` api messages, rebuilding the transcript
/// history from the truncated message list. Must keep `app.api_messages` and
/// the rendered history in lockstep so the next turn sends a consistent
/// context to the engine (same contract `apply_backtrack` relies on).
fn revert_conversation_to(app: &mut App, len: usize) {
    if len >= app.api_messages.len() {
        return;
    }
    app.api_messages.truncate(len);
    app.clear_history();
    let messages: Vec<_> = app.api_messages.clone();
    for msg in &messages {
        app.extend_history(history_cells_from_message(msg));
    }
    app.mark_history_updated();
    app.scroll_to_bottom();
    app.needs_redraw = true;
}

fn format_listing(snapshots: &[Snapshot]) -> String {
    let mut out = String::from(
        "Recent snapshots (newest first; pass /restore <N> to revert; /restore list 50 shows more):\n",
    );
    for (i, s) in snapshots.iter().enumerate() {
        out.push_str(&format!(
            "  #{:<2}  {}  {}  {}\n",
            i + 1,
            format_snapshot_time(s.timestamp),
            short_sha(s.id.as_str()),
            s.label,
        ));
    }
    out
}

fn format_snapshot_time(timestamp: i64) -> String {
    match chrono::Utc.timestamp_opt(timestamp, 0).single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M UTC").to_string(),
        None => "unknown time".to_string(),
    }
}

fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(8)]
}
