//! Parked-draft stash for the composer (#440).
//!
//! A stash is a side-channel from history: it holds drafts the user
//! parked deliberately (Ctrl+S) instead of submissions made in the
//! past (which live in `composer_history.rs`). Pop semantics make it
//! a LIFO — the most recent stash comes back first.
//!
//! ## On-disk format
//!
//! `~/.mimofanfan/composer_stash.jsonl` — one JSON object per line:
//!
//! ```jsonl
//! {"ts":"2026-05-04T01:23:45Z","text":"draft here"}
//! ```
//!
//! Self-healing parser: malformed lines are skipped silently so a
//! single bad write doesn't corrupt the rest of the stash. The
//! parser doesn't require any specific field order; only `text` is
//! mandatory.
//!
//! ## Why JSONL and not a plain text file?
//!
//! Drafts can contain newlines (they're prompts, not single-line
//! commands), so a `\n`-delimited plain file would mangle multi-line
//! drafts. JSONL escapes newlines inside JSON strings without
//! ambiguity and the timestamp / future fields land cleanly.

use std::fs;
use std::io;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const STASH_FILE_NAME: &str = "composer_stash.jsonl";

/// Hard cap so a runaway script can't fill the user's home with
/// parked drafts. Older entries are pruned at push time when the
/// stash exceeds this count.
pub const MAX_STASH_ENTRIES: usize = 200;

/// One parked draft. Fields are `#[serde(default)]` so legacy /
/// truncated records still parse instead of poisoning the stash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StashedDraft {
    /// RFC 3339 timestamp; omitted on legacy records.
    #[serde(default)]
    pub ts: String,
    /// The parked text. Required — entries with no `text` are
    /// dropped during load (treated as malformed).
    pub text: String,
}

fn default_stash_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        let primary = home.join(".mimofan").join(STASH_FILE_NAME);
        let previous = home.join(".mimofan").join(STASH_FILE_NAME);
        if primary.exists() || !previous.exists() {
            return primary;
        }
        previous
    })
}

/// Load every stashed draft from disk in the order they were
/// written (oldest first). Self-healing: malformed lines are
/// dropped silently. Returns an empty vec when the file doesn't
/// exist.
#[must_use]
pub fn load_stash() -> Vec<StashedDraft> {
    let Some(path) = default_stash_path() else {
        return Vec::new();
    };
    load_stash_from(&path)
}

fn load_stash_from(path: &Path) -> Vec<StashedDraft> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<StashedDraft>(&line).ok())
        .filter(|draft| !draft.text.is_empty())
        .collect()
}

/// Push a new draft onto the stash. Empty / whitespace-only text
/// is silently dropped so a stray Ctrl+S on an empty composer
/// doesn't pollute the file. Failures are logged but never
/// propagated — stash is a UX nicety, not a correctness concern.
pub fn push_stash(text: &str) {
    let Some(path) = default_stash_path() else {
        return;
    };
    push_stash_to(&path, text);
}

fn push_stash_to(path: &Path, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        tracing::warn!(
            "Failed to create composer stash dir {}: {err}",
            parent.display()
        );
        return;
    }

    let mut entries = load_stash_from(path);
    entries.push(StashedDraft {
        ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        text: text.to_string(),
    });
    if entries.len() > MAX_STASH_ENTRIES {
        let excess = entries.len() - MAX_STASH_ENTRIES;
        entries.drain(0..excess);
    }
    write_stash_to(path, &entries);
}

/// Remove and return the most recently pushed draft, if any.
/// Rewrites the on-disk file with the remaining entries.
#[must_use]
pub fn pop_stash() -> Option<StashedDraft> {
    let path = default_stash_path()?;
    pop_stash_from(&path)
}

/// Wipe the stash file entirely. Returns the number of entries
/// that were dropped (so the caller can report it). Returns 0
/// when the file doesn't exist or had no entries.
pub fn clear_stash() -> io::Result<usize> {
    let Some(path) = default_stash_path() else {
        return Ok(0);
    };
    clear_stash_at(&path)
}

fn clear_stash_at(path: &Path) -> io::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let entries = load_stash_from(path);
    let count = entries.len();
    if count == 0 {
        return Ok(0);
    }
    crate::utils::write_atomic(path, b"")?;
    Ok(count)
}

fn pop_stash_from(path: &Path) -> Option<StashedDraft> {
    let mut entries = load_stash_from(path);
    let popped = entries.pop()?;
    write_stash_to(path, &entries);
    Some(popped)
}

fn write_stash_to(path: &Path, entries: &[StashedDraft]) {
    let mut payload = String::new();
    for entry in entries {
        match serde_json::to_string(entry) {
            Ok(line) => {
                payload.push_str(&line);
                payload.push('\n');
            }
            Err(err) => {
                // A draft that round-trips through serde shouldn't
                // fail to serialize, but belt-and-suspenders so a
                // weird codepoint in `text` doesn't blow the file
                // away mid-write.
                tracing::warn!("Skipping stash entry due to serialize failure: {err}");
            }
        }
    }
    if let Err(err) = crate::utils::write_atomic(path, payload.as_bytes()) {
        tracing::warn!(
            "Failed to persist composer stash at {}: {err}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {}
