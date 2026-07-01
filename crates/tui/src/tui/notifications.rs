//! Desktop notifications for turn completion.
//!
//! Supports five delivery mechanisms:
//! - **OSC 9** — terminal escape sequence (`\x1b]9;…\x07`) for iTerm2,
//!   Ghostty, WezTerm, and tmux (with DCS passthrough).
//! - **Kitty** — OSC 99 protocol with ST terminator (no audible beep).
//! - **Ghostty** — OSC 777 notification protocol.
//! - **BEL** — audible bell (`\x07`) as a last-resort fallback.
//!
//! When `method = "auto"`, the resolver picks the best method for the
//! current terminal; Windows falls back to `Bel`, which is routed through
//! `MessageBeep(MB_OK)` for an audible default notification sound.

#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU8;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;

/// Notification delivery method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Method {
    /// Automatically pick the best protocol for the current terminal.
    /// See [`resolve_method`] for the canonical resolution table.
    #[default]
    Auto,
    /// OSC 9 escape: `\x1b]9;<msg>\x07`
    Osc9,
    /// Plain BEL character: `\x07`
    Bel,
    /// osascript
    MacOS,
    /// Kitty notification protocol (OSC 99) with ST terminator.
    /// Uses `ESC ] 99 ; params ST` — no audible beep, unlike BEL.
    Kitty,
    /// Ghostty notification protocol (OSC 777).
    /// Uses `ESC ] 777 ; notify ; title ; message BEL`.
    Ghostty,
    /// Suppress all notifications.
    Off,
}

/// Emit a Windows system beep via `MessageBeep(MB_OK)`.
///
/// Writing BEL (`\\x07`) to the terminal is silent on most Windows
/// terminals (Windows Terminal, Conhost, etc.), so we call the Win32
/// API directly to produce the standard notification sound.
#[cfg(target_os = "windows")]
fn windows_bell() {
    // MB_OK = 0x00000000 — plays the default system sound. Best-effort: a
    // failed beep is not worth surfacing to the caller, so the Result is
    // discarded.
    unsafe {
        let _ = MessageBeep(MESSAGEBOX_STYLE(0));
    }
}

/// Resolve `Auto` to a concrete method by inspecting `$TERM_PROGRAM`,
/// `$LC_TERMINAL`, and `$TERM`.
///
/// Resolution table:
/// - `iTerm.app`, `WezTerm`, `Cmux` → `Osc9`
/// - `Ghostty` → `Ghostty` (OSC 777)
/// - `kitty` → `Kitty` (OSC 99)
/// - `$LC_TERMINAL` matches OSC-9 capable → `Osc9` (Cmux that sets LC_TERMINAL)
/// - `$TERM` contains `ghostty` → `Osc9` (cmux etc.)
/// - `$TERM` contains `kitty` → `Kitty`
/// - Unix unknown → `Bel`
/// - Windows unknown → `Bel`
#[must_use]
fn resolve_method() -> Method {
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    match term_program.as_str() {
        "iTerm.app" | "WezTerm" | "Cmux" => return Method::Osc9,
        "Ghostty" => return Method::Ghostty,
        "kitty" => return Method::Kitty,
        _ => {}
    }

    // LC_TERMINAL fallback for terminals (e.g. Cmux) that set
    // LC_TERMINAL instead of TERM_PROGRAM.
    let lc_terminal = std::env::var("LC_TERMINAL").unwrap_or_default();
    match lc_terminal.as_str() {
        "iTerm.app" | "Ghostty" | "WezTerm" | "Cmux" => return Method::Osc9,
        _ => {}
    }

    // Windows: use BEL so `windows_bell()` (MessageBeep) fires on turn
    // completion.  Previous behavior returned `Off` to avoid the error chime
    // (#583), but `MessageBeep(MB_OK)` plays the *default system sound* —
    // distinct from the error sound — so BEL is safe and gives Windows users
    // audible feedback when a long turn finishes.
    if cfg!(target_os = "windows") {
        return Method::Bel;
    }

    if cfg!(target_os = "macos") {
        return Method::MacOS;
    }

    // Ghostty-based terminals (cmux, etc.) may not set their own
    // TERM_PROGRAM but do set TERM=xterm-ghostty. Likewise for Kitty.
    let term = std::env::var("TERM").unwrap_or_default();
    if term.contains("ghostty") {
        Method::Osc9
    } else if term.contains("kitty") {
        Method::Kitty
    } else {
        Method::Bel
    }
}

/// Wrap an escape sequence for terminal multiplexer passthrough.
///
/// tmux intercepts escape sequences; DCS passthrough tunnels them to
/// the outer terminal unmodified. Every ESC inside the payload is
/// doubled so tmux does not interpret it as DCS end.
fn wrap_for_multiplexer(seq: &str, in_tmux: bool) -> String {
    if in_tmux {
        let escaped = seq.replace('\x1b', "\x1b\x1b");
        format!("\x1bPtmux;{escaped}\x1b\\")
    } else {
        seq.to_string()
    }
}

/// Build the raw escape bytes for the given method and message.
///
/// When `in_tmux` is `true`, OSC sequences are wrapped in DCS passthrough
/// so tmux forwards them to the outer terminal.
#[must_use]
fn build_escape(method: Method, in_tmux: bool, msg: &str) -> Vec<u8> {
    match method {
        Method::Bel => vec![b'\x07'],
        Method::Osc9 => {
            let inner = format!("\x1b]9;{msg}\x07");
            if in_tmux {
                let escaped_inner = inner.replace('\x1b', "\x1b\x1b");
                format!("\x1bPtmux;{escaped_inner}\x1b\\").into_bytes()
            } else {
                inner.into_bytes()
            }
        }
        Method::Kitty => {
            // Kitty notification: OSC 99 ; params ST
            // ST terminator (ESC \) instead of BEL to avoid audible beep.
            let title_seq = "\x1b]99;d=0:p=title\x1b\\";
            let body_seq = format!("\x1b]99;p=body;{msg}\x1b\\");
            let focus_seq = "\x1b]99;d=1:a=focus\x1b\\";
            let combined = format!("{title_seq}{body_seq}{focus_seq}");
            wrap_for_multiplexer(&combined, in_tmux).into_bytes()
        }
        Method::Ghostty => {
            // Ghostty notification: OSC 777 ; notify ; title ; message BEL
            let seq = format!("\x1b]777;notify;mimofan;{msg}\x07");
            wrap_for_multiplexer(&seq, in_tmux).into_bytes()
        }
        // Auto and Off and MacOS should not reach build_escape.
        Method::Auto | Method::Off | Method::MacOS => vec![],
    }
}

/// Emit a turn-complete notification to `sink` if the elapsed time meets or
/// exceeds `threshold`, and `method` is not `Off`.
///
/// This variant takes a `W: Write` sink for testability.
pub fn notify_done_to<W: Write>(
    method: Method,
    in_tmux: bool,
    msg: &str,
    threshold: Duration,
    elapsed: Duration,
    sink: &mut W,
) {
    if elapsed < threshold {
        return;
    }
    let effective = match method {
        Method::Off => return,
        Method::Auto => resolve_method(),
        other => other,
    };

    // macOS Notification Center: handled via osascript, not terminal escapes.
    #[cfg(target_os = "macos")]
    if Method::MacOS == effective {
        macos_display_notification(msg);
        return;
    }

    let bytes = build_escape(effective, in_tmux, msg);
    if bytes.is_empty() {
        return;
    }
    // Best-effort: ignore write errors (e.g. stdout closed).
    let _ = sink.write_all(&bytes);
    let _ = sink.flush();

    // On Windows, writing BEL (`\x07`) to the terminal is silent in most
    // terminals (Windows Terminal, Conhost, etc.). Call MessageBeep to
    // produce an actual notification sound via the system audio scheme.
    #[cfg(target_os = "windows")]
    if effective == Method::Bel {
        windows_bell();
    }
}

/// Emit a turn-complete notification to **stdout** if `elapsed >= threshold`.
///
/// With `method = Auto`, selects the best protocol for the current terminal
/// (OSC 9, Kitty OSC 99, Ghostty OSC 777, or Bel). The unknown-terminal
/// fallback is platform-aware: `Bel` on every platform, with Windows routing
/// it through `MessageBeep(MB_OK)` for a default system notification sound.
/// See [`resolve_method`] for the canonical resolution table. Pass
/// `in_tmux = true` (i.e. `$TMUX` is non-empty at runtime) to wrap OSC
/// sequences in a DCS passthrough.
pub fn notify_done(
    method: Method,
    in_tmux: bool,
    msg: &str,
    threshold: Duration,
    elapsed: Duration,
) {
    notify_done_to(method, in_tmux, msg, threshold, elapsed, &mut io::stdout());
}

/// Set the terminal taskbar progress state via OSC 9 ; 4.
///
/// Windows Terminal supports this to show progress on the taskbar icon:
/// - `state = 0` — no progress (clear)
/// - `state = 1` — indeterminate (cycling green)
/// - `state = 2` — normal (0-100, requires progress param)
/// - `state = 3` — error (red)
/// - `state = 4` — paused (yellow)
///
/// Other terminals (iTerm2, WezTerm) ignore the sequence silently.
/// Best-effort — write failures are ignored.
pub fn set_taskbar_progress(state: u8, progress: Option<u8>) {
    let seq = if let Some(pct) = progress {
        format!("\x1b]9;4;{state};{pct}\x07")
    } else {
        format!("\x1b]9;4;{state}\x07")
    };
    let mut stdout = io::stdout();
    let _ = stdout.write_all(seq.as_bytes());
    let _ = stdout.flush();
}

/// Set taskbar progress to indeterminate (cycling) — call at turn start.
pub fn set_taskbar_progress_busy() {
    set_taskbar_progress(1, None);
}

/// Clear taskbar progress — call at turn end.
pub fn clear_taskbar_progress() {
    set_taskbar_progress(0, None);
}

/// Animation frame characters for the terminal title.
/// Uses the DeepSeek whale emoji (🐳 spouting, 🐋 resting) to match the
/// existing header status indicator in the TUI.
const TITLE_FRAMES: &[&str] = &["🐳", "🐋", "🐳", "🐋"];
const TITLE_ANIMATION_INTERVAL: Duration = Duration::from_millis(800);

/// Shared flag controlling the title animation loop. Set to `true` by
/// `start_title_animation()`, cleared by `stop_title_animation()`.
static TITLE_ANIMATION_RUNNING: AtomicBool = AtomicBool::new(false);

/// Write OSC 0 (set window title) sequence.
fn set_terminal_title(title: &str) {
    let seq = format!("\x1b]0;{title}\x07");
    let mut stdout = io::stdout();
    let _ = stdout.write_all(seq.as_bytes());
    let _ = stdout.flush();
}

/// Tracks whether the ✅ completion marker was set, so
/// `reset_title_on_interaction()` can skip redundant writes.
static COMPLETION_MARKER_SHOWN: AtomicBool = AtomicBool::new(false);

/// Start an animated terminal title spinner.
///
/// Cycles the terminal title between 🐳→🐋 every 800ms while processing,
/// matching the whale status indicator in the TUI header, so alt-tabbed
/// users can see activity.
///
/// The animation runs in a background tokio task that checks
/// `TITLE_ANIMATION_RUNNING`. Each call restarts the animation with the
/// given `original` base title — safe to call on every turn start.
pub fn start_title_animation(original: &str) {
    // Signal any existing animation loop to exit, then start fresh.
    TITLE_ANIMATION_RUNNING.store(true, Ordering::SeqCst);
    let base = original.to_string();
    tokio::spawn(async move {
        let mut frame = 0usize;
        while TITLE_ANIMATION_RUNNING.load(Ordering::SeqCst) {
            // Yield once per frame so a racing stop_title_animation()
            // can observe the cleared flag and apply the completion
            // marker before the next frame write. Without this yield
            // the background task could overwrite the ✅ marker with
            // the next whale frame.
            tokio::task::yield_now().await;
            if !TITLE_ANIMATION_RUNNING.load(Ordering::SeqCst) {
                break;
            }
            let spinner = TITLE_FRAMES[frame % TITLE_FRAMES.len()];
            set_terminal_title(&format!("{spinner} {base}"));
            frame += 1;
            tokio::time::sleep(TITLE_ANIMATION_INTERVAL).await;
        }
        // Don't restore title here — stop_title_animation() handles
        // what to show on completion (e.g. ✅ marker).
    });
}

/// Stop the title animation and show a completion marker.
///
/// Sets the title to `✅ <base>` so alt-tabbed users see at a glance
/// that processing finished. The marker is overwritten on the next turn
/// by [`start_title_animation`].
pub fn stop_title_animation() {
    TITLE_ANIMATION_RUNNING.store(false, Ordering::SeqCst);
    COMPLETION_MARKER_SHOWN.store(false, Ordering::SeqCst);
    // Show ✅ marker only for beep mode. Bell mode already has its own
    // terminal-level visual indicator (flash/icon).
    let mode = COMPLETION_SOUND_MODE.load(Ordering::SeqCst);
    if mode == 1 {
        set_terminal_title("✅ mimofan");
    }
    play_completion_sound();
}

/// Stop the title animation without playing the completion sound.
///
/// Cancellation and failed turns should return the terminal title to rest
/// without presenting them as completed work.
pub fn stop_title_animation_quietly() {
    TITLE_ANIMATION_RUNNING.store(false, Ordering::SeqCst);
    COMPLETION_MARKER_SHOWN.store(false, Ordering::SeqCst);
    set_terminal_title("mimofan");
}

/// Clear the ✅ completion marker from the title when the user interacts.
///
/// Call this on every user input event (key press, mouse click) so the
/// marker doesn't persist once the user is back at the terminal.
pub fn reset_title_on_interaction() {
    if COMPLETION_MARKER_SHOWN.swap(false, Ordering::SeqCst) {
        set_terminal_title("mimofan");
    }
}

/// Completion sound mode (0 = off, 1 = beep, 2 = bell, 3 = file).
static COMPLETION_SOUND_MODE: AtomicU8 = AtomicU8::new(1);
static COMPLETION_SOUND_FILE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
#[cfg(not(target_os = "windows"))]
static COMPLETION_SOUND_FILE_UNSUPPORTED_WARNED: AtomicBool = AtomicBool::new(false);
static COMPLETION_SOUND_FILE_MISSING_WARNED: AtomicBool = AtomicBool::new(false);

fn completion_sound_file_slot() -> &'static Mutex<Option<PathBuf>> {
    COMPLETION_SOUND_FILE.get_or_init(|| Mutex::new(None))
}

fn set_completion_sound(mode: crate::config::CompletionSound, sound_file: Option<PathBuf>) {
    let val = match mode {
        crate::config::CompletionSound::Off => 0u8,
        crate::config::CompletionSound::Beep => 1u8,
        crate::config::CompletionSound::Bell => 2u8,
        crate::config::CompletionSound::File => 3u8,
    };
    COMPLETION_SOUND_MODE.store(val, Ordering::SeqCst);
    if let Ok(mut slot) = completion_sound_file_slot().lock() {
        if sound_file.is_some() {
            COMPLETION_SOUND_FILE_MISSING_WARNED.store(false, Ordering::SeqCst);
        }
        *slot = sound_file;
    }
}

/// Play the configured completion sound (if not `Off`).
pub fn play_completion_sound() {
    match COMPLETION_SOUND_MODE.load(Ordering::SeqCst) {
        0 => {} // Off
        1 => {
            beep_sound();
        }
        2 => {
            bell_sound();
        }
        3 => {
            file_sound();
        }
        _ => {}
    }
}

/// Play a short completion sound via the system beep.
///
/// On Windows uses `MessageBeep(MB_OK)` which plays the default system
/// notification sound. On other platforms writes `BEL` (`\x07`) to stdout.
#[cfg(target_os = "windows")]
fn beep_sound() {
    windows_bell();
}

/// Non-Windows: write BEL to stdout for the terminal bell.
#[cfg(not(target_os = "windows"))]
fn beep_sound() {
    let _ = io::stdout().write_all(b"\x07");
}

/// Pure terminal BEL character.
fn bell_sound() {
    let _ = io::stdout().write_all(b"\x07");
}

fn configured_sound_file() -> Option<PathBuf> {
    completion_sound_file_slot()
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

#[cfg(target_os = "windows")]
fn play_sound_file(path: &Path) {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Best-effort and async: notification sound failure should not block or
    // fail a completed agent turn.
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(wide.as_ptr()),
            None,
            SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn play_sound_file(_path: &Path) {
    if !COMPLETION_SOUND_FILE_UNSUPPORTED_WARNED.swap(true, Ordering::SeqCst) {
        tracing::warn!("completion_sound = \"file\" is currently supported on Windows only");
    }
}

fn file_sound() {
    if let Some(path) = configured_sound_file() {
        play_sound_file(&path);
    } else if !COMPLETION_SOUND_FILE_MISSING_WARNED.swap(true, Ordering::SeqCst) {
        tracing::warn!("completion_sound = \"file\" requires [notifications].sound_file");
    }
}

/// Show a macOS Notification Center alert via `osascript`.
///
/// Runs on a dedicated background thread so the caller is not blocked.
///
/// The notification includes:
/// - **Title**: "mimofan"
/// - **Subtitle**: First line of `msg` (when the message contains a newline,
///   e.g. the localized completion status from a completed turn)
/// - **Body**: Remaining lines of `msg`, if any
/// - **Sound**: Default macOS notification sound
///
/// The message body is capped at 200 **characters** (not bytes) to keep the
/// bubble readable while correctly handling multi-byte text.
///
/// **Security**: The message is passed to `osascript` as a command-line
/// argument via `ARGV`, never embedded inline in the AppleScript source.
/// AppleScript does not treat backslash as an escape inside double-quoted
/// string literals, so the previous `\"` approach would terminate the
/// string at the `"` and leave any text between unbalanced quotes
/// evaluated as raw AppleScript code — a code-injection vector for
/// AI-generated notification text. Passing via `ARGV` avoids this
/// entirely because the message is never parsed as AppleScript syntax.
///
/// This is best-effort: if `osascript` is not available (e.g. headless SSH
/// session) the error is logged via `tracing::warn!` instead of silently
/// swallowed.
#[cfg(target_os = "macos")]
fn macos_display_notification(msg: &str) {
    let message = msg.to_string();

    // Spawn on a background thread so we don't block the caller.
    // osascript itself is fast (~50 ms), but spawning a subprocess
    // synchronously from an async context steals a tokio thread.
    let _ = std::thread::Builder::new()
        .name("osascript-notif".into())
        .spawn(move || {
            // Build AppleScript that receives the message via ARGV
            // instead of inline string interpolation. AppleScript does
            // not treat backslash as an escape inside double-quoted
            // string literals, so `\"` would terminate the string at
            // the `"` and leave a dangling `\`. Passing the message as
            // a command-line argument avoids any injection risk.
            let (subtitle, body) = macos_notification_parts(&message);
            let args = [
                "-e".to_string(),
                "on run argv".to_string(),
                "-e".to_string(),
                "set theBody to item 1 of argv".to_string(),
                "-e".to_string(),
                "set theSubtitle to item 2 of argv".to_string(),
                "-e".to_string(),
                "display notification theBody with title \"mimofan\" subtitle theSubtitle sound name \"default\"".to_string(),
                "-e".to_string(),
                "end run".to_string(),
                "--".to_string(),
                body,
                subtitle,
            ];

            match std::process::Command::new("osascript")
                .args(&args)
                .output()
            {
                Ok(output) if !output.status.success() => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!(stderr = %stderr, "osascript notification failed");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "osascript notification error");
                }
                _ => {}
            }
        });
}

#[cfg(target_os = "macos")]
fn macos_notification_parts(msg: &str) -> (String, String) {
    const SUBTITLE_MAX_CHARS: usize = 80;
    const BODY_MAX_CHARS: usize = 200;

    let sanitized = super::ui::sanitize_stream_chunk(msg);
    let lines: Vec<&str> = sanitized
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        return ("mimofan".to_string(), String::new());
    }

    let subtitle = truncate_notification_text(lines[0], SUBTITLE_MAX_CHARS);
    let body = truncate_notification_text(&lines[1..].join("\n"), BODY_MAX_CHARS);
    (subtitle, body)
}

#[cfg(target_os = "macos")]
fn truncate_notification_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let take = max_chars.saturating_sub(3);
    let mut out = text.chars().take(take).collect::<String>();
    out.push_str("...");
    out
}

/// Return a human-readable duration string, capped at two units so
/// it stays compact in headers and notifications.
///
/// Examples:
/// * `"45s"`, `"1m"`, `"1m 12s"`
/// * `"1h"`, `"3h 12m"` (#447 — was previously `"192m"` form)
/// * `"1d"`, `"2d 5h"` (#447 — multi-day sessions)
/// * `"1w"`, `"3w 2d"` (#447 — long-running automations)
///
/// The output drops the secondary unit when it's zero, so `"1h"`
/// rather than `"1h 0m"`. Sub-minute precision is dropped at the
/// hour mark and above; the goal is "is this a couple of hours or
/// a couple of days," not stopwatch accuracy.
#[must_use]
pub fn humanize_duration(d: Duration) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;

    let total = d.as_secs();
    if total == 0 {
        return "0s".to_string();
    }
    if total >= WEEK {
        let w = total / WEEK;
        let days = (total % WEEK) / DAY;
        return if days == 0 {
            format!("{w}w")
        } else {
            format!("{w}w {days}d")
        };
    }
    if total >= DAY {
        let days = total / DAY;
        let h = (total % DAY) / HOUR;
        return if h == 0 {
            format!("{days}d")
        } else {
            format!("{days}d {h}h")
        };
    }
    if total >= HOUR {
        let h = total / HOUR;
        let m = (total % HOUR) / MINUTE;
        return if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {m}m")
        };
    }
    if total >= MINUTE {
        let m = total / MINUTE;
        let s = total % MINUTE;
        return if s == 0 {
            format!("{m}m")
        } else {
            format!("{m}m {s}s")
        };
    }
    format!("{total}s")
}

// ── Per-turn notification composition ────────────────────────────────
//
// The helpers below decide *whether* to notify on a completed turn and
// *what message* to put in the body. The low-level dispatcher is
// `notify_done`; everything in this block sits in front of it.

use crate::localization::Locale;
use crate::models::{ContentBlock, Message};
use crate::tui::app::App;

/// Resolve the effective notification method/threshold/include-summary tuple
/// for a completed turn, taking the high-level
/// `[tui].notification_condition` override into account on top of the
/// lower-level `[notifications]` block.
///
/// Returns `None` to mean "do not notify" (either because the user set
/// `notification_condition = "never"` or because the resolved method is
/// `Off`).
pub fn settings(config: &crate::config::Config) -> Option<(Method, Duration, bool)> {
    let notif = config.notifications_config();
    // Initialize completion sound mode from config.
    set_completion_sound(notif.completion_sound, notif.sound_file);
    let method = match notif.method {
        crate::config::NotificationMethod::Auto => Method::Auto,
        crate::config::NotificationMethod::Osc9 => Method::Osc9,
        crate::config::NotificationMethod::Bel => Method::Bel,
        crate::config::NotificationMethod::Kitty => Method::Kitty,
        crate::config::NotificationMethod::Ghostty => Method::Ghostty,
        crate::config::NotificationMethod::Off => Method::Off,
    };

    if let Some(condition) = config
        .tui
        .as_ref()
        .and_then(|tui| tui.notification_condition)
    {
        match condition {
            crate::config::NotificationCondition::Always => {
                return Some((method, Duration::ZERO, notif.include_summary));
            }
            crate::config::NotificationCondition::Never => return None,
        }
    }

    Some((
        method,
        Duration::from_secs(notif.threshold_secs),
        notif.include_summary,
    ))
}

/// Build the notification body for a completed turn. Prefers the live
/// streaming text the user just saw; falls back to the latest assistant
/// message in `api_messages` if streaming text is empty (for example, the
/// turn finished entirely through tool output). When `include_summary` is
/// true, an elapsed/cost line is appended.
pub fn completed_turn_message(
    app: &App,
    current_streaming_text: &str,
    include_summary: bool,
    turn_elapsed: Duration,
    turn_cost: Option<crate::pricing::CostEstimate>,
) -> String {
    let mut msg = completion_status(
        notification_turn_complete(app.ui_locale),
        include_summary,
        turn_elapsed,
        turn_cost.map(|cost| crate::pricing::format_cost_estimate(cost, app.cost_currency)),
    );

    if let Some(preview) =
        text_summary(current_streaming_text).or_else(|| latest_assistant_text(&app.api_messages))
    {
        msg.push('\n');
        msg.push_str(&preview);
    }

    msg
}

/// Compose a notification body for a sub-agent completion. Falls back
/// to a generic "sub-agent X complete" if no human-readable line can
/// be teased out of the child's transcript.
pub fn subagent_completion_message(
    locale: Locale,
    id: &str,
    result: &str,
    include_summary: bool,
    elapsed: Duration,
) -> String {
    let result_line = result
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("<mimo:subagent.done>"));
    let mut msg = completion_status(
        notification_subagent_complete(locale),
        include_summary,
        elapsed,
        None,
    );
    let detail = result_line
        .and_then(text_summary)
        .map(|summary| format!("{id}: {summary}"))
        .unwrap_or_else(|| id.to_string());

    msg.push('\n');
    msg.push_str(&detail);

    msg
}

fn completion_status(
    label: &str,
    include_summary: bool,
    elapsed: Duration,
    cost: Option<String>,
) -> String {
    if !include_summary {
        return label.to_string();
    }

    let human = humanize_duration(elapsed);
    match cost {
        Some(cost) => format!("{label} ({human}, {cost})"),
        None => format!("{label} ({human})"),
    }
}

fn notification_turn_complete(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhHans => "本轮已完成",
    }
}

fn notification_subagent_complete(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhHans => "子代理已完成",
    }
}

/// Find the latest assistant message in `messages` and return a
/// notification-ready summary of its `Text` content. Thinking blocks,
/// tool calls, and tool results are skipped — only the user-visible
/// reply contributes to the body.
pub fn latest_assistant_text(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .and_then(|message| {
            let text = message
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text, .. } => Some(text.as_str()),
                    ContentBlock::Thinking { .. }
                    | ContentBlock::ToolUse { .. }
                    | ContentBlock::ToolResult { .. }
                    | ContentBlock::ServerToolUse { .. }
                    | ContentBlock::ToolSearchToolResult { .. }
                    | ContentBlock::CodeExecutionToolResult { .. } => None,
                    ContentBlock::ImageUrl { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            text_summary(&text)
        })
}

/// Sanitize + collapse + truncate streaming text into something fit to
/// hand the OS notification system. Returns `None` when nothing
/// useful remains after sanitization.
pub fn text_summary(text: &str) -> Option<String> {
    const MAX_CHARS: usize = 360;

    let sanitized = super::ui::sanitize_stream_chunk(text);
    let collapsed = sanitized
        .lines()
        .map(str::trim)
        .filter(|line: &&str| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((idx, _)) = trimmed.char_indices().nth(MAX_CHARS) {
        let mut s = String::with_capacity(idx + 3);
        s.push_str(&trimmed[..idx]);
        s.push_str("...");
        Some(s)
    } else {
        Some(trimmed.to_string())
    }
}

