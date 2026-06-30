//! Boot-time snapshot pruning.
//!
//! Called from `session_manager` once per session start. Failure is
//! never fatal — old snapshots taking disk space is annoying but not
//! correctness-breaking, so we log and move on.

use std::io;
use std::path::Path;
use std::time::Duration;

use super::paths::snapshot_git_dir;
use super::repo::SnapshotRepo;

/// Default snapshot retention window: 7 days.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Prune snapshots older than `max_age` for the given workspace.
///
/// If no snapshot repo exists yet (first run) this is a cheap no-op.
/// Returns the number of snapshots removed.
pub fn prune_older_than(workspace: &Path, max_age: Duration) -> io::Result<usize> {
    let git_dir = snapshot_git_dir(workspace);
    if !git_dir.exists() {
        return Ok(0);
    }
    let repo = SnapshotRepo::open_or_init(workspace)?;
    let removed = repo.prune_older_than(max_age)?;
    repo.prune_unreachable_objects()?;
    Ok(removed)
}

#[cfg(test)]
mod tests {}
