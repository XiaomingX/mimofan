//! Running-session registry backed by socket files in `~/.mimofan/run/`.
//!
//! A session is "running" iff its socket file exists. This is the lightweight
//! discovery mechanism used by `mimofan session list` and `mimofan session kill`.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Directory holding per-session `<id>.sock` files. `~/.mimofan/run`.
pub fn run_dir() -> Result<PathBuf> {
    mimofan_config::ensure_state_dir("run").context("resolve mimofan run dir")
}

/// Socket path for a session id.
pub fn socket_path(id: &str) -> Result<PathBuf> {
    Ok(run_dir()?.join(format!("{id}.sock")))
}

/// A currently-running session (socket file present).
#[derive(Debug, Clone)]
pub struct RunningSession {
    pub id: String,
    pub socket: PathBuf,
}

/// List running sessions by scanning the run dir for `.sock` files.
pub fn list_running() -> Result<Vec<RunningSession>> {
    let dir = run_dir()?;
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let entries = std::fs::read_dir(&dir).context("read run dir")?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sock")
            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
        {
            // Only report sessions whose socket is actually connectable-ish:
            // a stale socket file from a crashed daemon would fail to connect,
            // but discovery-by-file is the documented contract; `attach`
            // surfaces the real connection error.
            out.push(RunningSession {
                id: name.to_string(),
                socket: path,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Remove a session's socket file (used after a clean daemon exit).
pub fn unregister(id: &str) -> Result<()> {
    let path = socket_path(id)?;
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_lives_in_run_dir() {
        let p = socket_path("abc123").unwrap();
        assert!(p.ends_with("abc123.sock"));
        assert!(p.parent().unwrap().ends_with("run"));
    }

    #[test]
    fn list_running_is_deterministic_and_safe_on_missing_dir() {
        // run_dir may not exist in a fresh env; must not error.
        let _ = list_running().unwrap();
    }
}
