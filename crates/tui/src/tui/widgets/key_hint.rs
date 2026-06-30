//! Terminal-aware keybinding rendering.
//!
//! `KeyBinding` is a typed representation of a chord (a [`KeyCode`] plus a
//! [`KeyModifiers`] set) that knows how to render itself in a way that matches
//! the host platform's conventions. On macOS the Option key renders as `⌥`
//! (matching how every other Mac app — including Terminal, iTerm2, and the
//! system menu bar — labels Option chords). On Linux and Windows we keep the
//! plain-text `alt + X` notation that users coming from other CLIs already
//! recognise.
//!
//! See `codex-rs/tui/src/key_hint.rs` for the original design; this is a
//! ratatui-compatible port that exposes a [`std::fmt::Display`] impl plus a
//! `KeyBinding -> Span` conversion so call sites can use it equally well in
//! plain `format!` calls and inside ratatui [`ratatui::text::Line`] /
//! [`ratatui::text::Span`] builders.
//!
//! Windows AltGr disambiguation: many European keyboard layouts produce
//! `Ctrl+Alt` events when AltGr is pressed alone (to type `@`, `\`, etc.).
//! [`is_altgr`] returns `true` for that combination on Windows so callers can
//! suppress alt-bound shortcut matching when the user is genuinely just
//! reaching for a glyph. On non-Windows targets the function always returns
//! `false`. See [`has_ctrl_or_alt`] for the convenience predicate that
//! shortcut handlers should prefer over a raw `mods.contains(...)` check.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{style::Style, text::Span};

// Compile-time platform detection. The `#[cfg(test)]` arm forces the macOS
// rendering during `cargo test` so unit tests are deterministic regardless of
// the host they run on (CI hits Ubuntu, macOS, and Windows).
#[cfg(test)]
const ALT_PREFIX: &str = "⌥+";

#[cfg(all(not(test), target_os = "macos"))]
const ALT_PREFIX: &str = "⌥+";
#[cfg(all(not(test), not(target_os = "macos")))]
const ALT_PREFIX: &str = "alt+";

const CTRL_PREFIX: &str = "ctrl+";
const SHIFT_PREFIX: &str = "shift+";

/// A typed representation of a single chord (key + modifiers).
///
/// Construct via [`plain`], [`alt`], [`shift`], [`ctrl`], or [`ctrl_alt`] for
/// the common cases, or [`KeyBinding::new`] for arbitrary modifier sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyBinding {
    key: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Build a binding from a key code and modifier set.
    pub const fn new(key: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { key, modifiers }
    }

    /// `true` if the supplied [`KeyEvent`] matches this binding (key + mods),
    /// considering only `Press` / `Repeat` events (release events are ignored
    /// — crossterm only emits them when key-release reporting is on, and we
    /// never want to fire a shortcut on key-up regardless).
    pub fn is_press(&self, event: KeyEvent) -> bool {
        self.key == event.code
            && self.modifiers == event.modifiers
            && (event.kind == KeyEventKind::Press || event.kind == KeyEventKind::Repeat)
    }
}

/// A binding with no modifiers.
pub const fn plain(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::NONE)
}

/// `Alt`-modified binding (renders as `⌥` on macOS, `alt+` elsewhere).
pub const fn alt(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::ALT)
}

/// `Shift`-modified binding.
pub const fn shift(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::SHIFT)
}

/// `Ctrl`-modified binding.
pub const fn ctrl(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::CONTROL)
}

/// `Ctrl+Alt`-modified binding.
pub const fn ctrl_alt(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::CONTROL.union(KeyModifiers::ALT))
}

fn modifiers_to_string(modifiers: KeyModifiers) -> String {
    let mut result = String::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        result.push_str(CTRL_PREFIX);
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        result.push_str(SHIFT_PREFIX);
    }
    if modifiers.contains(KeyModifiers::ALT) {
        result.push_str(ALT_PREFIX);
    }
    result
}

fn keycode_to_string(key: &KeyCode) -> String {
    match key {
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "shift+tab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "del".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_string().to_ascii_lowercase(),
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".to_string(),
        KeyCode::Left => "←".to_string(),
        KeyCode::Right => "→".to_string(),
        KeyCode::PageUp => "pgup".to_string(),
        KeyCode::PageDown => "pgdn".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        _ => format!("{key}").to_ascii_lowercase(),
    }
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            modifiers_to_string(self.modifiers),
            keycode_to_string(&self.key)
        )
    }
}

impl From<KeyBinding> for Span<'static> {
    fn from(binding: KeyBinding) -> Self {
        (&binding).into()
    }
}

impl From<&KeyBinding> for Span<'static> {
    fn from(binding: &KeyBinding) -> Self {
        Span::styled(binding.to_string(), key_hint_style())
    }
}

fn key_hint_style() -> Style {
    Style::default().dim()
}

/// `true` if `mods` carries Ctrl or Alt — but not the AltGr Ctrl+Alt
/// combination on Windows. Shortcut handlers should prefer this predicate
/// over `mods.contains(CONTROL) || mods.contains(ALT)` so they don't fire on
/// AltGr keypresses (which on European keyboard layouts are how users type
/// `@`, `\`, `|`, etc.).
pub fn has_ctrl_or_alt(mods: KeyModifiers) -> bool {
    (mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::ALT)) && !is_altgr(mods)
}

/// On Windows, AltGr is delivered as `Ctrl+Alt`. There's no terminal-portable
/// way to tell a real `Ctrl+Alt` chord apart from a layout-emitted AltGr glyph
/// — crossterm doesn't expose left-vs-right modifier distinction across all
/// backends — so we treat any `Ctrl+Alt` (with no other modifiers) as AltGr.
/// This trades the (rare) ability to bind `Ctrl+Alt+<char>` for not
/// swallowing accented characters European users type. On non-Windows
/// platforms this always returns `false`.
#[cfg(windows)]
#[inline]
pub fn is_altgr(mods: KeyModifiers) -> bool {
    mods.contains(KeyModifiers::ALT) && mods.contains(KeyModifiers::CONTROL)
}

#[cfg(not(windows))]
#[inline]
pub fn is_altgr(_mods: KeyModifiers) -> bool {
    false
}
