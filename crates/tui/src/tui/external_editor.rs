//! External editor support for the composer.
//!
//! Spawns `$VISUAL`/`$EDITOR` (fallback `vi`) on a temp file pre-populated with
//! the composer's current contents. The TUI is suspended for the duration of
//! the edit and re-entered on return. The temp file is cleaned up in all paths
//! (success, editor failure, IO error) via [`tempfile::NamedTempFile`].
//!
//! Reference: codex-rs's `tui/src/external_editor.rs` — the design here mirrors
//! that approach but is synchronous (called inline from the TUI event loop) and
//! handles its own raw-mode toggling rather than relying on the caller.

use std::env;
use std::fs;
use std::io::{self, Stdout, Write};
use std::process::Command;

use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use tempfile::Builder;

use super::color_compat::ColorCompatBackend;

/// Outcome of a single external-editor invocation.
#[derive(Debug, PartialEq, Eq)]
pub enum EditorOutcome {
    /// Editor exited cleanly and the file contents differ from the seed.
    Edited(String),
    /// Editor exited cleanly but the contents are unchanged (or empty after
    /// trimming). The composer should be left as-is.
    Unchanged,
    /// Editor exited non-zero or could not be spawned. The composer should be
    /// left as-is and a status toast shown.
    Cancelled,
}

/// Resolve the editor command, preferring `$VISUAL` over `$EDITOR`, falling
/// back to `vi`. Returns the raw string for the test path; `spawn_editor`
/// splits it via `shlex` (Unix) so users can set `EDITOR="code --wait"`.
fn resolve_editor() -> String {
    env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| env::var("EDITOR").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "vi".to_string())
}

#[cfg(unix)]
fn split_command(raw: &str) -> Option<Vec<String>> {
    shlex::split(raw)
}

#[cfg(not(unix))]
fn split_command(raw: &str) -> Option<Vec<String>> {
    // On Windows we do not support shell-quoted editor commands; treat the
    // full string as the program name.
    if raw.trim().is_empty() {
        None
    } else {
        Some(vec![raw.to_string()])
    }
}

/// Run the external editor without touching terminal state. Exposed for tests.
///
/// Returns:
/// - `Ok(EditorOutcome::Edited(new))` if the editor exited cleanly and the
///   contents differ from `seed`.
/// - `Ok(EditorOutcome::Unchanged)` if the editor exited cleanly but the
///   contents match `seed`.
/// - `Ok(EditorOutcome::Cancelled)` if the editor exited non-zero or could not
///   be spawned.
///
/// The temp file is removed on every path because [`tempfile::NamedTempFile`]
/// is dropped at the end of the function.
pub fn run_editor_raw(seed: &str) -> io::Result<EditorOutcome> {
    let mut tmp = Builder::new()
        .prefix("deepseek-edit-")
        .suffix(".md")
        .tempfile()?;
    tmp.write_all(seed.as_bytes())?;
    tmp.flush()?;
    let path = tmp.path().to_path_buf();

    let raw = resolve_editor();
    let parts = match split_command(&raw) {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(EditorOutcome::Cancelled),
    };

    let mut cmd = Command::new(&parts[0]);
    if parts.len() > 1 {
        cmd.args(&parts[1..]);
    }
    cmd.arg(&path);

    let status = match cmd.status() {
        Ok(s) => s,
        Err(_) => return Ok(EditorOutcome::Cancelled),
    };
    if !status.success() {
        return Ok(EditorOutcome::Cancelled);
    }

    let new = fs::read_to_string(&path)?;
    // tmp goes out of scope here — file is unlinked.
    if new == seed {
        Ok(EditorOutcome::Unchanged)
    } else {
        Ok(EditorOutcome::Edited(new))
    }
}

/// Suspend the TUI, run the external editor on `current`, then re-enter the
/// TUI. Returns the new composer text iff the user saved changes.
///
/// On any error (raw-mode toggle, IO, editor spawn failure), the function
/// still attempts to fully restore the terminal before returning.
pub(crate) fn spawn_editor_for_input(
    terminal: &mut Terminal<ColorCompatBackend<Stdout>>,
    use_alt_screen: bool,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
    current: &str,
) -> io::Result<EditorOutcome> {
    // 1. Suspend.
    // #443: pop keyboard enhancement flags first so the editor
    // process doesn't inherit a half-configured input mode. Best-
    // effort — matches the shutdown / panic paths in main.rs.
    // Use the Windows-aware helper: the raw crossterm execute!() is a
    // no-op on Windows and would leave the editor process in Kitty mode.
    super::ui::pop_keyboard_enhancement_flags(terminal.backend_mut());
    let _ = disable_raw_mode();
    if use_bracketed_paste {
        let _ = execute!(terminal.backend_mut(), DisableBracketedPaste);
    }
    if use_mouse_capture {
        let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
    }
    if use_alt_screen {
        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    }

    // 2. Run the editor (synchronous; inherits stdio).
    let result = run_editor_raw(current);

    // 3. Resume — best-effort restoration regardless of `result`.
    if use_alt_screen {
        let _ = execute!(terminal.backend_mut(), EnterAlternateScreen);
    }
    if use_mouse_capture {
        let _ = execute!(terminal.backend_mut(), EnableMouseCapture);
    }
    if use_bracketed_paste {
        let _ = execute!(terminal.backend_mut(), EnableBracketedPaste);
    }
    let _ = enable_raw_mode();
    // Force a full repaint so a SIGWINCH during the edit doesn't leave the
    // viewport stale.
    let _ = terminal.clear();

    result
}

#[cfg(test)]
mod tests {}
