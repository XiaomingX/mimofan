//! Keyboard-shortcut predicates and platform-specific labels.
//!
//! These helpers normalise the cross-platform variations between
//! `Ctrl+…` (Linux/Windows) and `Cmd+…` (macOS), legacy `Ctrl+H`-as-
//! backspace handling, and the macOS Option-Latin-character escapes.
//! Centralising them
//! keeps the composer / transcript event loops in `ui.rs` short and
//! lets us add a new platform without touching the call sites.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn has_control_like_modifier(modifiers: KeyModifiers) -> bool {
    has_control_like_modifier_for_platform(modifiers, cfg!(target_os = "macos"))
}

pub(super) fn has_control_like_modifier_for_platform(
    modifiers: KeyModifiers,
    is_macos: bool,
) -> bool {
    modifiers.contains(KeyModifiers::CONTROL)
        || (is_macos && modifiers.contains(KeyModifiers::SUPER))
}

/// Copy-to-clipboard: `Cmd+C` on macOS or `Ctrl+Shift+C` elsewhere.
pub(super) fn is_copy_shortcut(key: &KeyEvent) -> bool {
    let is_c = matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'));
    if !is_c {
        return false;
    }

    if key.modifiers.contains(KeyModifiers::SUPER) {
        return true;
    }

    key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT)
}

/// Toggle the file-tree pane: `Ctrl+Shift+E` on Linux/Windows or
/// `Cmd+Shift+E` on macOS.
pub(super) fn is_file_tree_toggle_shortcut(key: &KeyEvent) -> bool {
    let is_shifted_e = matches!(key.code, KeyCode::Char('E'))
        || (matches!(key.code, KeyCode::Char('e')) && key.modifiers.contains(KeyModifiers::SHIFT));
    if !is_shifted_e {
        return false;
    }

    let has_forbidden_modifier =
        key.modifiers.contains(KeyModifiers::ALT) || key.modifiers.contains(KeyModifiers::SUPER);
    let ctrl_shift_e = key.modifiers.contains(KeyModifiers::CONTROL) && !has_forbidden_modifier;

    let cmd_shift_e = key.modifiers.contains(KeyModifiers::SUPER)
        && key.modifiers.contains(KeyModifiers::SHIFT)
        && !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT);

    ctrl_shift_e || cmd_shift_e
}

pub(super) fn tool_details_shortcut_label() -> &'static str {
    "v"
}

pub(super) fn tool_details_shortcut_action_hint(noun: &str) -> String {
    format!("{} opens {noun}", tool_details_shortcut_label())
}

pub(super) fn activity_shortcut_label() -> &'static str {
    "Ctrl+O"
}

/// Modifier predicate for the v0.8.30 family of `Alt+<key>` transcript-
/// nav shortcuts (`Alt+G` / `Alt+[` / `Alt+]` / `Alt+?` / `Alt+L`). Requires
/// `Alt` and disallows `Ctrl` / `Super` so the
/// bindings don't collide with platform clipboard / window-management
/// shortcuts. `Shift` is permitted so the capital-letter forms work on
/// any keyboard layout that produces them as `Alt+Shift+key`.
///
/// Plain `Char` events (no modifier, or modifier=`Shift` alone for the
/// uppercase form) fall through to text insertion, which is the whole
/// point — typing "good morning" no longer eats the first `g`.
pub(super) fn alt_nav_modifiers(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::ALT)
        && !modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::SUPER)
}

pub(super) fn is_macos_option_v_legacy_key(key: &KeyEvent) -> bool {
    is_macos_option_v_legacy_key_for_platform(key, cfg!(target_os = "macos"))
}

pub(super) fn is_macos_option_v_legacy_key_for_platform(key: &KeyEvent, is_macos: bool) -> bool {
    is_macos && key.modifiers.is_empty() && matches!(key.code, KeyCode::Char('\u{221A}'))
}

/// Paste-from-clipboard: `Cmd+V` (macOS), `Ctrl+V` (Linux/Windows), or
/// the legacy raw `\u{16}` ETX byte some terminals emit.
pub(super) fn is_paste_shortcut(key: &KeyEvent) -> bool {
    let is_v = matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'));
    let is_legacy_ctrl_v = matches!(key.code, KeyCode::Char('\u{16}'));
    if !is_v && !is_legacy_ctrl_v {
        return false;
    }

    if is_legacy_ctrl_v {
        return true;
    }

    // Cmd+V on macOS
    if key.modifiers.contains(KeyModifiers::SUPER) {
        return true;
    }

    // Ctrl+V on Linux/Windows
    key.modifiers.contains(KeyModifiers::CONTROL)
}

/// Whether the key event represents a user typing a printable
/// character into the composer (no modifier that would turn it into
/// a shortcut).
pub(super) fn is_text_input_key(key: &KeyEvent) -> bool {
    if matches!(key.code, KeyCode::Char(c) if c.is_control()) {
        return false;
    }

    !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
        && !key.modifiers.contains(KeyModifiers::SUPER)
}

/// `Ctrl+H` is the legacy ASCII backspace many terminals still emit
/// when the user presses Backspace. Disallows Alt/Super so it doesn't
/// shadow window-management combos.
pub(super) fn is_ctrl_h_backspace(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('h'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
        && !key.modifiers.contains(KeyModifiers::SUPER)
}
