//! Pending-input preview widget for the composer area.
//!
//! Port of `codex-rs/tui/src/bottom_pane/pending_input_preview.rs` for
//! issue #85. Renders queued/steered messages above the composer when a
//! turn is in flight, so user input typed during a running turn doesn't
//! disappear silently. The backing state still distinguishes queue/steer
//! origins, but the UI renders one coherent pending-input list.
//!
//! Empty state renders zero rows so the composer doesn't gain wasted height
//! when there's nothing to show.
//!
//! Wired into `ui.rs::render` between the chat area and the composer; the user
//! can see when typed input has been captured for later delivery.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::UnicodeWidthChar;

use crate::palette;
use crate::tui::widgets::Renderable;

/// Per-item line cap before we collapse the rest into a `…` overflow row.
const PREVIEW_LINE_LIMIT: usize = 3;
const PENDING_STEER_PREFIX: &str = "  ↳ Live steer pending: ";
const REJECTED_STEER_PREFIX: &str = "  ↳ Rejected live steer: ";
const EDITING_QUEUED_PREFIX: &str = "  ↳ Editing queued follow-up: ";

/// Description of the keybinding the hint line at the bottom should advertise
/// for the "edit last queued message" action.
#[derive(Debug, Clone)]
pub struct EditBinding {
    pub label: &'static str,
}

impl EditBinding {
    pub const UP: EditBinding = EditBinding { label: "↑" };
}

/// Widget showing pending input while a turn is in progress.
#[derive(Debug, Clone)]
pub struct PendingInputPreview {
    pub context_items: Vec<ContextPreviewItem>,
    pub pending_steers: Vec<String>,
    pub rejected_steers: Vec<String>,
    pub queued_messages: Vec<String>,
    pub editing_queued_message: Option<String>,
    pub edit_binding: EditBinding,
}

/// Compact pre-send context row shown above the composer. `included=false`
/// marks missing/skipped context distinctly from files/media that will be
/// sent or inlined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPreviewItem {
    pub kind: String,
    pub label: String,
    pub detail: Option<String>,
    pub included: bool,
    pub removable: bool,
    pub selected: bool,
}

impl PendingInputPreview {
    pub fn new() -> Self {
        Self {
            context_items: Vec::new(),
            pending_steers: Vec::new(),
            rejected_steers: Vec::new(),
            queued_messages: Vec::new(),
            editing_queued_message: None,
            edit_binding: EditBinding::UP,
        }
    }

    fn has_pending_inputs(&self) -> bool {
        !self.pending_steers.is_empty()
            || !self.rejected_steers.is_empty()
            || !self.queued_messages.is_empty()
            || self.editing_queued_message.is_some()
    }

    /// Build the (possibly empty) ordered line list this widget would render
    /// at `width`. Pulled out so `desired_height` can ask the same renderer
    /// without duplicating wrapping logic.
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        if (self.context_items.is_empty() && !self.has_pending_inputs()) || width < 4 {
            return Vec::new();
        }

        let dim = Style::default()
            .fg(palette::TEXT_DIM)
            .add_modifier(Modifier::DIM);
        let dim_italic = dim.add_modifier(Modifier::ITALIC);

        let mut lines: Vec<Line<'static>> = Vec::new();

        if !self.context_items.is_empty() {
            push_section_header(
                &mut lines,
                Line::from(vec![Span::raw("• "), Span::raw("Context for next send")]),
            );
            for item in &self.context_items {
                push_context_item(&mut lines, item, width);
            }
        }

        if self.has_pending_inputs() {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            push_section_header(
                &mut lines,
                Line::from(vec![Span::raw("• "), Span::raw("Pending inputs")]),
            );
            let pending_steer_indent = continuation_indent(PENDING_STEER_PREFIX);
            for steer in &self.pending_steers {
                push_truncated_item(
                    &mut lines,
                    steer,
                    width,
                    dim,
                    PENDING_STEER_PREFIX,
                    &pending_steer_indent,
                );
            }
            let rejected_steer_indent = continuation_indent(REJECTED_STEER_PREFIX);
            for steer in &self.rejected_steers {
                push_truncated_item(
                    &mut lines,
                    steer,
                    width,
                    dim,
                    REJECTED_STEER_PREFIX,
                    &rejected_steer_indent,
                );
            }
            if let Some(draft) = self.editing_queued_message.as_deref() {
                let editing_indent = continuation_indent(EDITING_QUEUED_PREFIX);
                push_truncated_item(
                    &mut lines,
                    draft,
                    width,
                    dim_italic,
                    EDITING_QUEUED_PREFIX,
                    &editing_indent,
                );
                lines.push(Line::from(vec![Span::styled(
                    "    Esc restores queued follow-up".to_string(),
                    dim,
                )]));
            }
            for (idx, message) in self.queued_messages.iter().enumerate() {
                let row_number = idx + 1;
                let queued_prefix = format!("  ↳ Queued follow-up #{row_number}: ");
                let queued_message_indent = continuation_indent(&queued_prefix);
                push_truncated_item(
                    &mut lines,
                    message,
                    width,
                    dim_italic,
                    &queued_prefix,
                    &queued_message_indent,
                );
                lines.push(Line::from(vec![Span::styled(
                    format!("    /queue send {row_number} · drop {row_number} · clear"),
                    dim,
                )]));
            }
            if !self.queued_messages.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    format!(
                        "    Ctrl+S send now · {} edit last queued",
                        self.edit_binding.label
                    ),
                    dim,
                )]));
            }
        }

        lines
    }
}

impl Default for PendingInputPreview {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for PendingInputPreview {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let lines = self.lines(area.width);
        if lines.is_empty() {
            return;
        }
        Paragraph::new(lines).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let lines = self.lines(width);
        u16::try_from(lines.len()).unwrap_or(u16::MAX)
    }
}

fn continuation_indent(prefix: &str) -> String {
    " ".repeat(display_width(prefix))
}

fn push_section_header(lines: &mut Vec<Line<'static>>, header: Line<'static>) {
    lines.push(header);
}

fn push_context_item(lines: &mut Vec<Line<'static>>, item: &ContextPreviewItem, width: u16) {
    let status_style = if item.selected {
        Style::default()
            .fg(palette::SELECTION_TEXT)
            .bg(palette::SELECTION_BG)
            .add_modifier(Modifier::BOLD)
    } else if item.included {
        Style::default().fg(palette::TEXT_MUTED)
    } else {
        Style::default().fg(palette::STATUS_WARNING)
    };
    let label_style = if item.selected {
        Style::default()
            .fg(palette::SELECTION_TEXT)
            .bg(palette::SELECTION_BG)
    } else if item.included {
        Style::default().fg(palette::TEXT_PRIMARY)
    } else {
        Style::default().fg(palette::TEXT_MUTED)
    };
    let detail = item
        .detail
        .as_deref()
        .filter(|detail| !detail.trim().is_empty())
        .map(|detail| format!(" · {detail}"))
        .unwrap_or_default();
    let action = if item.selected {
        " · Backspace/Delete removes"
    } else if item.removable {
        " · removable"
    } else {
        ""
    };
    let body = format!("[{}] {}{}{}", item.kind, item.label, detail, action);
    let body_width = width.saturating_sub(4).max(1) as usize;
    for (idx, segment) in wrap_to_width(&body, body_width).into_iter().enumerate() {
        let prefix = if idx == 0 {
            if item.selected { "  ▸ " } else { "  ↳ " }
        } else {
            "    "
        };
        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), status_style),
            Span::styled(segment, label_style),
        ]));
    }
}

/// Render a single bucket item with `↳` prefix, truncating to
/// [`PREVIEW_LINE_LIMIT`] visible rows. Multi-line input wraps at the given
/// column budget and the continuation rows get the `subsequent_indent` so
/// the prefix and the body stay column-aligned.
fn push_truncated_item(
    lines: &mut Vec<Line<'static>>,
    raw: &str,
    width: u16,
    style: Style,
    prefix: &str,
    subsequent_indent: &str,
) {
    let body_width = width.saturating_sub(display_width(prefix) as u16) as usize;
    let body_width = body_width.max(1);

    let mut produced: Vec<String> = Vec::new();
    for (idx, paragraph) in raw.split('\n').enumerate() {
        let wrapped = wrap_to_width(paragraph, body_width);
        for (j, segment) in wrapped.into_iter().enumerate() {
            let row = if idx == 0 && j == 0 {
                format!("{prefix}{segment}")
            } else {
                format!("{subsequent_indent}{segment}")
            };
            produced.push(row);
            if produced.len() > PREVIEW_LINE_LIMIT {
                break;
            }
        }
        if produced.len() > PREVIEW_LINE_LIMIT {
            break;
        }
    }

    let truncated = produced.len() > PREVIEW_LINE_LIMIT;
    for (i, row) in produced.into_iter().enumerate() {
        if i >= PREVIEW_LINE_LIMIT {
            break;
        }
        lines.push(Line::from(Span::styled(row, style)));
    }
    if truncated {
        lines.push(Line::from(Span::styled(
            format!("{subsequent_indent}…"),
            style,
        )));
    }
}

/// Naive word-aware wrap that respects unicode display widths. Matches the
/// behavior expected by snapshot tests in the codex source — long URL-like
/// tokens that exceed `width` are emitted on their own row instead of being
/// hard-broken mid-character.
fn wrap_to_width(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.is_empty() {
        return vec![text.to_string()];
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in text.split_inclusive(' ') {
        let word_width = display_width(word);
        if current_width + word_width > width && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if word_width > width {
            // Token longer than the budget: flush current, emit the word as
            // its own row even though it overflows. Avoids the codex-issue
            // of a long URL fanning out into N junk-ellipsis rows.
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_width = 0;
            }
            out.push(word.trim_end().to_string());
            continue;
        }
        current.push_str(word);
        current_width += word_width;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {}
