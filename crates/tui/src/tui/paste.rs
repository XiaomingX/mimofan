//! Paste-burst handling — turn rapid keystrokes (terminals without bracketed
//! paste) into a single committed buffer instead of N individual chars.
//!
//! Extracted from `tui/ui.rs` (P1.2). The owning state machine lives on
//! `App.paste_burst` (`tui::paste_burst`); these helpers wire it to the key
//! event loop and the composer's text buffer.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, looks_like_slash_command_input};
use super::paste_burst::CharDecision;

/// Process a key in the context of paste-burst detection. Returns `true`
/// when the key was fully handled by the paste machinery (caller skips
/// further input handling); `false` when the key still needs the normal
/// composer path.
pub fn handle_paste_burst_key(app: &mut App, key: &KeyEvent, now: Instant) -> bool {
    if !app.use_paste_burst_detection {
        return false;
    }
    // Once we've observed a real `Event::Paste` in this session, bracketed
    // paste is verified working and the rapid-keystroke heuristic is
    // unnecessary. Skipping it eliminates false positives on fast typing /
    // IME commits / autocomplete on terminals with reliable bracketed
    // paste (the dominant case on iTerm2 / Ghostty / WezTerm / Windows
    // Terminal).
    if app.bracketed_paste_seen {
        return false;
    }

    let has_ctrl_alt_or_super = key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
        || key.modifiers.contains(KeyModifiers::SUPER);

    match key.code {
        KeyCode::Enter => {
            if !in_command_context(app) && app.paste_burst.append_newline_if_active(now) {
                return true;
            }
            if !in_command_context(app)
                && app.paste_burst.newline_should_insert_instead_of_submit(now)
            {
                app.insert_char('\n');
                app.paste_burst.extend_window(now);
                return true;
            }
        }
        KeyCode::Char(c) if !has_ctrl_alt_or_super => {
            if !c.is_ascii() {
                // IME-committed characters (Chinese, Japanese, Korean)
                // arrive as individual KeyCode::Char events, typically with
                // tens-of-milliseconds gaps between each committed character.
                // Paste-burst buffering would lose characters when the IME
                // commits slower than the burst heuristic's timing window.
                //
                // We still call note_plain_char + extend_window so that:
                //   1. The burst timing counter advances for non-IME fast
                //      typing on terminals without bracketed paste support.
                //   2. The Enter-suppression window stays open during a rapid
                //      non-ASCII sequence, preventing premature submission.
                // But the character is inserted directly into the composer
                // rather than placed into the paste-burst buffer.
                if let Some(pending) = app.paste_burst.flush_before_modified_input() {
                    app.insert_str(&pending);
                }
                app.paste_burst.note_plain_char(now);
                app.paste_burst.extend_window(now);
                app.insert_char(c);
                return true;
            }

            let decision = app.paste_burst.on_plain_char(c, now);
            return handle_paste_burst_decision(app, decision, c, now);
        }
        _ => {}
    }

    false
}

/// Apply a paste-burst decision to the composer buffer. Some decisions
/// retroactively grab the last few chars from the input back into the
/// pending paste buffer (when the heuristic decides the recent typing was
/// actually a paste).
pub fn handle_paste_burst_decision(
    app: &mut App,
    decision: CharDecision,
    c: char,
    now: Instant,
) -> bool {
    match decision {
        CharDecision::RetainFirstChar => true,
        CharDecision::BeginBufferFromPending | CharDecision::BufferAppend => {
            app.paste_burst.append_char_to_buffer(c, now);
            true
        }
        CharDecision::BeginBuffer { retro_chars } => {
            if apply_paste_burst_retro_capture(app, retro_chars as usize, c, now) {
                return true;
            }
            app.insert_char(c);
            true
        }
    }
}

fn apply_paste_burst_retro_capture(
    app: &mut App,
    retro_chars: usize,
    c: char,
    now: Instant,
) -> bool {
    let cursor_byte = app.cursor_byte_index();
    let before = &app.composer.input[..cursor_byte];
    let Some(grab) = app
        .composer
        .paste_burst
        .decide_begin_buffer(now, before, retro_chars)
    else {
        return false;
    };
    if !grab.grabbed.is_empty() {
        app.input.replace_range(grab.start_byte..cursor_byte, "");
        let removed = grab.grabbed.chars().count();
        app.cursor_position = app.cursor_position.saturating_sub(removed);
    }
    app.paste_burst.append_char_to_buffer(c, now);
    true
}

fn in_command_context(app: &App) -> bool {
    looks_like_slash_command_input(&app.input)
}

#[cfg(test)]
mod tests {}
