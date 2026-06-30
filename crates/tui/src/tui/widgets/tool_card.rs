//! Tool-card visual vocabulary for the v0.6.6 transcript redesign.
//!
//! Tool cards are the boxes that appear when the agent runs `read_file`,
//! `exec_shell`, `apply_patch`, etc. The visual vocabulary is intentionally
//! sparse: a single verb glyph identifies the family, a left rail anchors
//! the card to the timeline, and the spinner cadence reuses the existing
//! tool-status animation.
//!
//! This module owns:
//!
//! - [`ToolFamily`] — the canonical semantic families plus a `Generic`
//!   fallback for anything we don't have a family for yet.
//! - [`tool_family_for_title`] — maps the legacy `render_tool_header` title
//!   string (`"Shell"`, `"Patch"`, `"Workspace"`, etc.) to a family. Lets
//!   the existing call sites drop in family glyphs without re-architecting
//!   each cell.
//! - [`family_glyph`] / [`family_label`] — the verb glyph + label per
//!   family. Glyphs are single graphemes; labels are short verbs.
//! - [`CardRail`] / [`rail_glyph`] — the `╭ │ ╰` rail anchored to the
//!   left margin so the eye can group multi-line cards.
//!
//! The actual line composition still happens inside `history.rs`; this
//! module is the vocabulary, not the layout engine. Keeping it small means
//! a future visual refresh only has to touch the constants here.

use crate::localization::Locale;

/// Tool family — the verb the agent is performing. Used to pick a glyph
/// and label for the card header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFamily {
    /// Reads, listings, exploration. `▷ read`.
    Read,
    /// Edits, patches, writes. `◆ patch`.
    Patch,
    /// Shell, child processes. `▶ run`.
    Run,
    /// Grep, fuzzy file search, web search. `⌕ find`.
    Find,
    /// Single sub-agent dispatch. `◐ delegate`.
    Delegate,
    /// Multi-agent fanout dispatch (rlm). `⋮⋮ fanout`.
    Fanout,
    /// Recursive language model work. `⋮⋮ rlm`.
    Rlm,
    /// Verification gates, tests, and validators. `✓ verify`.
    Verify,
    /// Reasoning / chain-of-thought. `… think`. Reasoning has its own
    /// render path (`render_thinking` in `history.rs`); the family is
    /// declared here for completeness so any future code that reaches for
    /// it has the matching glyph + label vocabulary.
    #[allow(dead_code)]
    Think,
    /// Anything we don't have a family glyph for yet — falls back to a
    /// neutral bullet so the card still renders cleanly.
    Generic,
}

/// Map a legacy tool-header title string (the value passed to
/// `render_tool_header`) to a family. Anything unrecognised falls back to
/// [`ToolFamily::Generic`] so cards still render — they just lose the
/// verb-glyph treatment until the family is added here.
#[must_use]
pub fn tool_family_for_title(title: &str) -> ToolFamily {
    match title {
        "Shell" => ToolFamily::Run,
        "Patch" | "Diff" => ToolFamily::Patch,
        "Workspace" | "Image" => ToolFamily::Read,
        "Search" => ToolFamily::Find,
        "Plan" | "Review" => ToolFamily::Generic,
        _ => ToolFamily::Generic,
    }
}

/// Map an arbitrary tool name (as exposed to the model — e.g. `read_file`,
/// `apply_patch`, `agent`) to a family. Used by `GenericToolCell`
/// where the `tool_family_for_title` shortcut isn't enough because every
/// generic cell shares the title `"Tool"`.
#[must_use]
pub fn tool_family_for_name(name: &str) -> ToolFamily {
    match name {
        "read_file" | "list_dir" | "view_image" => ToolFamily::Read,
        "edit_file" | "apply_patch" | "write_file" => ToolFamily::Patch,
        "exec_shell"
        | "exec_shell_wait"
        | "exec_shell_interact"
        | "exec_shell_cancel"
        | "task_shell_start"
        | "task_shell_wait" => ToolFamily::Run,
        "grep_files" | "file_search" | "web_search" | "fetch_url" => ToolFamily::Find,
        "agent" => ToolFamily::Delegate,
        "rlm_open" | "rlm_eval" | "rlm_configure" | "rlm_close" | "rlm" => ToolFamily::Rlm,
        "run_tests"
        | "run_verifiers"
        | "task_gate_run"
        | "validate_data"
        | "wait_for_dev_server" => ToolFamily::Verify,
        _ => ToolFamily::Generic,
    }
}

/// User-facing label for an arbitrary tool name. Known tools collapse to the
/// semantic verb; unknown tools keep their exact name for debugging.
#[cfg(test)]
#[must_use]
fn tool_display_label_for_name(name: &str) -> String {
    let family = tool_family_for_name(name);
    if matches!(family, ToolFamily::Generic) {
        name.to_string()
    } else {
        family_label(family).to_string()
    }
}

fn family_message_id(family: ToolFamily) -> crate::localization::MessageId {
    match family {
        ToolFamily::Read => crate::localization::MessageId::ToolFamilyRead,
        ToolFamily::Patch => crate::localization::MessageId::ToolFamilyPatch,
        ToolFamily::Run => crate::localization::MessageId::ToolFamilyRun,
        ToolFamily::Find => crate::localization::MessageId::ToolFamilyFind,
        ToolFamily::Delegate => crate::localization::MessageId::ToolFamilyDelegate,
        ToolFamily::Fanout => crate::localization::MessageId::ToolFamilyFanout,
        ToolFamily::Rlm => crate::localization::MessageId::ToolFamilyRlm,
        ToolFamily::Verify => crate::localization::MessageId::ToolFamilyVerify,
        ToolFamily::Think => crate::localization::MessageId::ToolFamilyThink,
        ToolFamily::Generic => crate::localization::MessageId::ToolFamilyGeneric,
    }
}

/// Compact activity/status label for arbitrary tool names. Known built-ins use
/// the semantic verb; unknown tools keep the `tool NAME` form.
#[must_use]
pub fn tool_activity_label_for_name(name: &str, locale: Locale) -> String {
    let family = tool_family_for_name(name);
    let mid = family_message_id(family);
    if matches!(family, ToolFamily::Generic) {
        format!("{} {name}", crate::localization::tr(locale, mid))
    } else {
        crate::localization::tr(locale, mid).to_string()
    }
}

/// Build a compact semantic summary for a tool header from the public tool
/// name and the already-sanitized argument summary.
#[must_use]
pub fn tool_header_summary_for_name(name: &str, input_summary: Option<&str>) -> Option<String> {
    let summary = input_summary?.trim();
    if summary.is_empty() {
        return None;
    }

    let preferred_keys = match tool_family_for_name(name) {
        ToolFamily::Read | ToolFamily::Patch => ["path", "file", "target", "content"].as_slice(),
        ToolFamily::Run => ["command", "cmd", "script"].as_slice(),
        ToolFamily::Find => ["query", "pattern", "path", "scope"].as_slice(),
        ToolFamily::Delegate | ToolFamily::Fanout | ToolFamily::Rlm => {
            ["prompt", "task", "model"].as_slice()
        }
        ToolFamily::Verify => ["profile", "level", "command", "args", "path"].as_slice(),
        ToolFamily::Think | ToolFamily::Generic => {
            ["query", "path", "command", "prompt"].as_slice()
        }
    };

    for key in preferred_keys {
        if let Some(value) = summary_value(summary, key) {
            return Some(value);
        }
    }

    Some(summary.to_string())
}

fn summary_value(summary: &str, key: &str) -> Option<String> {
    for part in summary.split(", ") {
        let Some((part_key, value)) = part.split_once(':') else {
            continue;
        };
        if part_key.trim() == key {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// The verb glyph for a family. Single grapheme so the header layout math
/// in `render_tool_header` stays simple (one cell wide).
#[must_use]
pub fn family_glyph(family: ToolFamily) -> &'static str {
    match family {
        ToolFamily::Read => "\u{25B7}",           // ▷
        ToolFamily::Patch => "\u{25C6}",          // ◆
        ToolFamily::Run => "\u{25B6}",            // ▶
        ToolFamily::Find => "\u{2315}",           // ⌕
        ToolFamily::Delegate => "\u{25D0}",       // ◐
        ToolFamily::Fanout => "\u{22EE}\u{22EE}", // ⋮⋮ (two cells)
        ToolFamily::Rlm => "\u{22EE}\u{22EE}",    // ⋮⋮ (two cells)
        ToolFamily::Verify => "\u{2713}",
        ToolFamily::Think => "\u{2026}",   // …
        ToolFamily::Generic => "\u{2022}", // •
    }
}

/// The short verb label for a family — appears in card headers next to the
/// glyph. Lowercased on purpose; the verb-glyph + label is the new card
/// title vocabulary.
#[must_use]
pub fn family_label(family: ToolFamily) -> &'static str {
    match family {
        ToolFamily::Read => "read",
        ToolFamily::Patch => "patch",
        ToolFamily::Run => "run",
        ToolFamily::Find => "find",
        ToolFamily::Delegate => "delegate",
        ToolFamily::Fanout => "fanout",
        ToolFamily::Rlm => "rlm",
        ToolFamily::Verify => "verify",
        ToolFamily::Think => "think",
        ToolFamily::Generic => "tool",
    }
}

/// Position of a line within a multi-line card — drives the left-rail
/// glyph so the box reads as a contiguous group from top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // wired by future card-refactor follow-ups
pub enum CardRail {
    /// First line of the card — the header. `╭`.
    Top,
    /// Any middle line — body content. `│`.
    Middle,
    /// Last line of the card. `╰`.
    Bottom,
    /// Single-line card — no rail at all.
    Single,
}

/// Map a [`CardRail`] position to its rail glyph. Returned as a `&str`
/// because callers paste it into a span.
#[must_use]
#[allow(dead_code)] // wired by future card-refactor follow-ups
pub fn rail_glyph(rail: CardRail) -> &'static str {
    match rail {
        CardRail::Top => "\u{256D}",    // ╭
        CardRail::Middle => "\u{2502}", // │
        CardRail::Bottom => "\u{2570}", // ╰
        CardRail::Single => "",
    }
}

#[cfg(test)]
mod tests {}
