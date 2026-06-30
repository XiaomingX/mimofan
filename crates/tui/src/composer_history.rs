//! Cross-session composer input history (#366).
//!
//! Persists user-typed prompts to `~/.mimofan/composer_history.txt`
//! (falling back to a legacy `~/.deepseek/composer_history.txt` only when
//! one already exists, #3240) so pressing Up-arrow at the composer recalls
//! submissions from previous sessions, not just the current one. One entry
//! per line, oldest first,
//! capped at [`MAX_HISTORY_ENTRIES`] entries (older entries are pruned
//! at append time).
//!
//! Entries that begin with `/` (slash commands) are NOT stored — they
//! pollute the recall stream and the fuzzy slash-menu already covers
//! them. Empty / whitespace-only inputs are also skipped.
//!
//! ## Off-thread writes (#1927)
//!
//! [`append_history`] used to block the caller for a read-then-atomic-
//! rewrite of the full file. That ran on the UI thread inside
//! `submit_input`, contributing a perceptible stall after Enter. The
//! public entry point now hands work to a dedicated writer thread via
//! [`writer_sender`] and returns immediately. Submissions stay serialised
//! in arrival order, so the on-disk file keeps its "oldest first"
//! invariant.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

/// Hard cap on persisted history. Keeps the file small (typical entries
/// are < 200 chars, so 1000 entries ≈ 200 KB) and bounds startup load
/// time.
pub const MAX_HISTORY_ENTRIES: usize = 1000;

const HISTORY_FILE_NAME: &str = "composer_history.txt";

fn default_history_path() -> Option<PathBuf> {
    history_path_with_home(dirs::home_dir())
}

/// Resolve the composer-history file under `home`, preferring the mimofan
/// root and only falling back to the legacy `.deepseek` root when a legacy
/// file already exists.
///
/// On a fresh install (neither file present) this returns the `.mimofan`
/// path, so the writer never recreates `~/.deepseek/` at runtime (#3240),
/// while users who haven't migrated keep reading and appending to their
/// existing legacy history. Mirrors the primary/legacy resolution used by
/// `snapshot::paths` and `artifacts`.
fn history_path_with_home(home: Option<PathBuf>) -> Option<PathBuf> {
    let home = home?;
    let primary = home.join(".mimo").join(HISTORY_FILE_NAME);
    if primary.exists() {
        return Some(primary);
    }
    let previous = home.join(".mimofan").join(HISTORY_FILE_NAME);
    if previous.exists() {
        return Some(previous);
    }
    let legacy = home.join(".deepseek").join(HISTORY_FILE_NAME);
    if legacy.exists() {
        return Some(legacy);
    }
    Some(primary)
}

/// Read the persisted history into memory. Returns an empty vec if the
/// file doesn't exist or can't be parsed — this is best-effort.
#[must_use]
pub fn load_history() -> Vec<String> {
    let Some(path) = default_history_path() else {
        return Vec::new();
    };
    load_history_from(&path)
}

fn load_history_from(path: &Path) -> Vec<String> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .collect()
}

/// Append an entry to the persisted history, pruning old entries to
/// stay within [`MAX_HISTORY_ENTRIES`]. Slash-commands and empty input
/// are skipped — those don't help recall.
///
/// Best-effort and non-blocking — work is forwarded to a dedicated writer
/// thread so the caller (typically the UI submit handler) returns
/// immediately. See module docs for the rationale (#1927). Failures on
/// the writer thread are logged via `tracing` but not propagated.
pub fn append_history(entry: &str) {
    let Some(path) = default_history_path() else {
        return;
    };
    append_history_dispatched(&path, entry);
}

/// Path-injectable variant of [`append_history`] used by tests. Forwards
/// the work to the dedicated writer thread (or falls back to a synchronous
/// write if the channel send fails) so callers never block on disk I/O.
fn append_history_dispatched(path: &Path, entry: &str) {
    let entry = entry.to_string();
    if let Err(err) = writer_sender().send(HistoryWrite::Append(path.to_path_buf(), entry)) {
        match err.0 {
            HistoryWrite::Append(path, entry) => append_history_to(&path, &entry),
            #[cfg(test)]
            HistoryWrite::Flush(_) => unreachable!("flush messages are only sent by tests"),
        }
    }
}

enum HistoryWrite {
    Append(PathBuf, String),
    #[cfg(test)]
    Flush(Sender<()>),
}

/// Lazy singleton sender for the dedicated composer-history writer
/// thread. Initialised on first use; the thread runs for the lifetime
/// of the process and drains queued writes in arrival order.
fn writer_sender() -> &'static Sender<HistoryWrite> {
    static SENDER: OnceLock<Sender<HistoryWrite>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = channel::<HistoryWrite>();
        let spawn_result = std::thread::Builder::new()
            .name("composer-history-writer".to_string())
            .spawn(move || {
                // recv() returns Err when all senders have dropped, which
                // only happens at process shutdown because the singleton
                // sender lives in a static for the lifetime of the process.
                while let Ok(message) = rx.recv() {
                    match message {
                        HistoryWrite::Append(path, entry) => {
                            append_history_batch(&rx, (path, entry));
                        }
                        #[cfg(test)]
                        HistoryWrite::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        if let Err(err) = spawn_result {
            tracing::warn!("Failed to spawn composer-history-writer: {err}");
        }
        tx
    })
}

fn append_history_batch(rx: &Receiver<HistoryWrite>, first: (PathBuf, String)) {
    let mut pending = vec![first];
    #[cfg(test)]
    let mut flush = None;

    loop {
        match rx.recv_timeout(Duration::from_millis(2)) {
            Ok(HistoryWrite::Append(path, entry)) => pending.push((path, entry)),
            #[cfg(test)]
            Ok(HistoryWrite::Flush(done)) => {
                flush = Some(done);
                break;
            }
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    for (path, entries) in group_history_writes_by_path(pending) {
        append_history_entries_to(&path, entries.iter().map(String::as_str));
    }

    #[cfg(test)]
    if let Some(done) = flush {
        let _ = done.send(());
    }
}

fn group_history_writes_by_path(writes: Vec<(PathBuf, String)>) -> Vec<(PathBuf, Vec<String>)> {
    let mut grouped: Vec<(PathBuf, Vec<String>)> = Vec::new();

    for (path, entry) in writes {
        if let Some((_, entries)) = grouped
            .iter_mut()
            .find(|(existing_path, _)| existing_path == &path)
        {
            entries.push(entry);
        } else {
            grouped.push((path, vec![entry]));
        }
    }

    grouped
}

fn append_history_to(path: &Path, entry: &str) {
    append_history_entries_to(path, std::iter::once(entry));
}

fn append_history_entries_to<'a>(
    path: &Path,
    entries_to_append: impl IntoIterator<Item = &'a str>,
) {
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        tracing::warn!(
            "Failed to create composer history dir {}: {err}",
            parent.display()
        );
        return;
    }

    // Read existing entries, append the new ones, prune from the front
    // until under the cap, then atomically rewrite.
    let mut entries = load_history_from(path);
    let mut changed = false;
    for entry in entries_to_append {
        let trimmed = entry.trim();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }
        if entries.last().map(String::as_str) == Some(trimmed) {
            // De-dupe consecutive duplicates — repeated submission of the
            // same prompt shouldn't bloat the file.
            continue;
        }
        entries.push(trimmed.to_string());
        changed = true;
    }

    if !changed {
        return;
    }

    if entries.len() > MAX_HISTORY_ENTRIES {
        let excess = entries.len() - MAX_HISTORY_ENTRIES;
        entries.drain(0..excess);
    }

    let payload = entries.join("\n") + "\n";
    if let Err(err) = write_history_atomic(path, payload.as_bytes()) {
        tracing::warn!(
            "Failed to persist composer history at {}: {err}",
            path.display()
        );
    }
}

fn write_history_atomic(path: &Path, payload: &[u8]) -> std::io::Result<()> {
    const RETRY_DELAYS: &[Duration] = &[
        Duration::from_millis(5),
        Duration::from_millis(10),
        Duration::from_millis(25),
        Duration::from_millis(50),
        Duration::from_millis(100),
        Duration::from_millis(200),
        Duration::from_millis(400),
    ];

    for (attempt, delay) in RETRY_DELAYS
        .iter()
        .map(Some)
        .chain(std::iter::once(None))
        .enumerate()
    {
        match crate::utils::write_atomic(path, payload) {
            Ok(()) => return Ok(()),
            Err(err) if delay.is_some() => {
                tracing::debug!(
                    "Retrying composer history write to {} after attempt {} failed: {err}",
                    path.display(),
                    attempt + 1
                );
                std::thread::sleep(*delay.expect("delay checked"));
            }
            Err(err) => return Err(err),
        }
    }

    unreachable!("retry iterator always ends with a final write attempt")
}

#[cfg(test)]
mod tests {}
