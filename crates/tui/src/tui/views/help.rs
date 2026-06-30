//! Searchable help overlay for `?`, `F1`, and `Ctrl+/`.
//!
//! Renders two stacked sections — *Slash commands* and *Keybindings* — with
//! a live substring filter applied as the user types in the search box. The
//! command list is sourced from [`crate::commands::command_infos()`] and the
//! keybinding list from [`crate::tui::keybindings::KEYBINDINGS`] so neither
//! can drift from the wired-up handlers.
//!
//! Keys: any printable character extends the filter, `Backspace` (or `Ctrl+H`)
//! shrinks it,
//! `↑`/`↓` (or `Ctrl+P`/`Ctrl+N`) move the selection, `PgUp`/`PgDn` jump by
//! ten rows, `Home`/`End` jump to ends, and `Esc` closes. Pressing `?` again
//! at the call-site (`tui::ui`) also toggles the overlay closed.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::commands;
use crate::localization::{Locale, MessageId, tr};
use crate::palette;
use crate::tui::keybindings::KEYBINDINGS;
use crate::tui::views::{ModalKind, ModalView, ViewAction};

/// Two top-level sections rendered in the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpSection {
    Command,
    Keybinding,
}

impl HelpSection {
    fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::Command => tr(locale, MessageId::HelpSlashCommands),
            Self::Keybinding => tr(locale, MessageId::HelpKeybindings),
        }
    }

    /// Sort key — commands before keybindings keeps the most-used surface up
    /// top so an unfiltered overlay opens with the user's likely target in
    /// view without scrolling.
    fn rank(self) -> u8 {
        match self {
            Self::Command => 0,
            Self::Keybinding => 1,
        }
    }
}

#[derive(Debug, Clone)]
struct HelpEntry {
    section: HelpSection,
    /// Sort-within-section key — keybinding entries reuse their declared
    /// section's rank so the help overlay groups Navigation, Editing, … in
    /// the same order as `tui::keybindings`.
    sub_rank: u8,
    label: String,
    description: String,
    /// Lowercased haystack used for substring matching; pre-built so each
    /// keystroke does not re-allocate per entry.
    haystack: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpRenderRow {
    Section(HelpSection),
    Entry { slot: usize, entry_idx: usize },
}

pub struct HelpView {
    locale: Locale,
    entries: Vec<HelpEntry>,
    /// Indices into `entries`, in display order, after filtering.
    filtered: Vec<usize>,
    query: String,
    selected: usize,
}

impl Default for HelpView {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpView {
    pub fn new() -> Self {
        Self::new_for_locale(Locale::ZhHans)
    }

    pub fn new_for_locale(locale: Locale) -> Self {
        let entries = build_entries(locale);
        let mut view = Self {
            locale,
            entries,
            filtered: Vec::new(),
            query: String::new(),
            selected: 0,
        };
        view.refilter();
        view
    }

    fn tr(&self, id: MessageId) -> &'static str {
        tr(self.locale, id)
    }

    fn refilter(&mut self) {
        // Substring matching is intentional — fuzzy matchers can hide the
        // exact-prefix hit a user is typing toward, which is the wrong
        // failure mode for a *help* surface. We split on whitespace so
        // multi-term queries (`apply mode`) act as an AND.
        let query = self.query.trim().to_ascii_lowercase();
        let terms: Vec<&str> = query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .collect();

        let mut filtered: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| terms.iter().all(|term| entry.haystack.contains(term)))
            .map(|(idx, _)| idx)
            .collect();

        filtered.sort_by_key(|idx| {
            let entry = &self.entries[*idx];
            (entry.section.rank(), entry.sub_rank, entry.label.clone())
        });
        self.filtered = filtered;
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.filtered.len() as isize;
        let next = (self.selected as isize + delta).clamp(0, len - 1) as usize;
        self.selected = next;
    }

    fn move_selection_wrapping(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.filtered.len() as isize;
        let next = (self.selected as isize + delta).rem_euclid(len) as usize;
        self.selected = next;
    }

    fn render_rows(&self) -> Vec<HelpRenderRow> {
        let mut rows = Vec::new();
        let mut active_section: Option<HelpSection> = None;

        for (slot, entry_idx) in self.filtered.iter().copied().enumerate() {
            let entry = &self.entries[entry_idx];
            if active_section != Some(entry.section) {
                rows.push(HelpRenderRow::Section(entry.section));
                active_section = Some(entry.section);
            }
            rows.push(HelpRenderRow::Entry { slot, entry_idx });
        }

        rows
    }

    fn selected_render_row(rows: &[HelpRenderRow], selected: usize) -> usize {
        rows.iter()
            .position(|row| matches!(row, HelpRenderRow::Entry { slot, .. } if *slot == selected))
            .unwrap_or(0)
    }

    fn visible_row_start(rows: &[HelpRenderRow], selected: usize, visible_budget: usize) -> usize {
        if rows.len() <= visible_budget {
            return 0;
        }

        let selected_row = Self::selected_render_row(rows, selected);
        let half = visible_budget / 2;
        if selected_row <= half {
            0
        } else if selected_row + half >= rows.len() {
            rows.len().saturating_sub(visible_budget)
        } else {
            selected_row.saturating_sub(half)
        }
    }
}

fn build_entries(locale: Locale) -> Vec<HelpEntry> {
    let mut entries = Vec::new();

    for command in commands::command_infos() {
        let label = format!("/{}", command.name);
        let localized = command.description_for(locale);
        let description = if command.aliases.is_empty() {
            localized.to_string()
        } else {
            format!(
                "{}  (aliases: {})",
                localized,
                command
                    .aliases
                    .iter()
                    .map(|a| format!("/{a}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let haystack = format!(
            "{} {} {}",
            label.to_ascii_lowercase(),
            description.to_ascii_lowercase(),
            command.usage.to_ascii_lowercase()
        );
        entries.push(HelpEntry {
            section: HelpSection::Command,
            // Commands have no inherent ordering — fall back to alphabetical
            // by leaning on `label.clone()` in the final sort_by_key tuple.
            sub_rank: 0,
            label,
            description,
            haystack,
        });
    }

    for binding in KEYBINDINGS {
        let label = binding.chord.to_string();
        let description = format!(
            "[{}] {}",
            binding.section.label(locale),
            tr(locale, binding.description_id)
        );
        let haystack = format!(
            "{} {}",
            label.to_ascii_lowercase(),
            description.to_ascii_lowercase()
        );
        entries.push(HelpEntry {
            section: HelpSection::Keybinding,
            sub_rank: binding.section.rank(),
            label,
            description,
            haystack,
        });
    }

    entries
}

fn modal_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette::BORDER_COLOR))
        .style(Style::default().bg(palette::DEEPSEEK_INK))
        .padding(Padding::uniform(1))
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text.width() <= max_width {
        return text.to_string();
    }
    let mut out = String::new();
    let limit = max_width.saturating_sub(1);
    for ch in text.chars() {
        let next_width = out.width() + ch.to_string().width();
        if next_width > limit {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

impl ModalView for HelpView {
    fn kind(&self) -> ModalKind {
        ModalKind::Help
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> ViewAction {
        // Scroll clamps at the ends (keyboard Up/Down wrap); wheel-wrapping
        // reads as disorienting.
        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_selection(-1),
            MouseEventKind::ScrollDown => self.move_selection(1),
            _ => {}
        }
        ViewAction::None
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => ViewAction::Close,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ViewAction::Close
            }
            KeyCode::Char('q') | KeyCode::Char('Q') if self.query.is_empty() => ViewAction::Close,
            KeyCode::Up => {
                self.move_selection_wrapping(-1);
                ViewAction::None
            }
            KeyCode::Down => {
                self.move_selection_wrapping(1);
                ViewAction::None
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection_wrapping(-1);
                ViewAction::None
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection_wrapping(1);
                ViewAction::None
            }
            KeyCode::PageUp => {
                self.move_selection(-10);
                ViewAction::None
            }
            KeyCode::PageDown => {
                self.move_selection(10);
                ViewAction::None
            }
            KeyCode::Home => {
                self.selected = 0;
                ViewAction::None
            }
            KeyCode::End => {
                if !self.filtered.is_empty() {
                    self.selected = self.filtered.len() - 1;
                }
                ViewAction::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter();
                ViewAction::None
            }
            // Terminals where stty erase == ^H send Ctrl+H instead of
            // Backspace (DEL). Treat it identically so the filter input
            // works across all platforms (#958).
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.pop();
                self.refilter();
                ViewAction::None
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
            {
                self.query.push(c);
                self.refilter();
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = 90.min(area.width.saturating_sub(4));
        let popup_height = 28.min(area.height.saturating_sub(4));
        let popup_area = Rect {
            x: area.width.saturating_sub(popup_width) / 2,
            y: area.height.saturating_sub(popup_height) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let mut lines: Vec<Line<'static>> = Vec::new();

        let query_label = if self.query.is_empty() {
            self.tr(MessageId::HelpFilterPlaceholder).to_string()
        } else {
            format!("{}{}", self.tr(MessageId::HelpFilterPrefix), self.query)
        };
        lines.push(Line::from(Span::styled(
            query_label,
            Style::default()
                .fg(palette::DEEPSEEK_SKY)
                .add_modifier(Modifier::BOLD),
        )));

        let match_count = if self.query.is_empty() {
            format!("{} entries", self.entries.len())
        } else {
            format!("{} / {} matches", self.filtered.len(), self.entries.len())
        };
        lines.push(Line::from(Span::styled(
            match_count,
            Style::default()
                .fg(palette::TEXT_DIM)
                .add_modifier(Modifier::ITALIC),
        )));
        lines.push(Line::from(""));

        if self.filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                self.tr(MessageId::HelpNoMatches),
                Style::default()
                    .fg(palette::TEXT_MUTED)
                    .add_modifier(Modifier::ITALIC),
            )));
        } else {
            // The chord/label column takes up to 28 cols on wide screens;
            // descriptions fill the remainder. Borders and padding eat 4
            // cells from each side (border 1 + padding 1) × 2.
            let inner_width = popup_width.saturating_sub(4) as usize;
            let label_width = 28.min(inner_width.saturating_sub(8));
            let desc_capacity = inner_width.saturating_sub(label_width + 4);

            // The block uses a one-cell border plus one-cell padding, so the
            // real paragraph body is four rows shorter than the outer popup.
            // Budget against that body height so selected rows are not clipped
            // by the bottom border/padding.
            let header_lines = lines.len();
            let visible_budget = (popup_height as usize)
                .saturating_sub(4)
                .saturating_sub(header_lines)
                .max(1);

            let rows = self.render_rows();
            let row_start = Self::visible_row_start(&rows, self.selected, visible_budget);

            for row in rows.iter().skip(row_start).take(visible_budget) {
                match *row {
                    HelpRenderRow::Section(section) => {
                        let count = self
                            .filtered
                            .iter()
                            .filter(|idx| self.entries[**idx].section == section)
                            .count();
                        lines.push(Line::from(Span::styled(
                            format!("  {} ({})", section.label(self.locale), count),
                            Style::default()
                                .fg(palette::WHALE_ACCENT_PRIMARY)
                                .add_modifier(Modifier::BOLD),
                        )));
                    }
                    HelpRenderRow::Entry { slot, entry_idx } => {
                        let entry = &self.entries[entry_idx];
                        let is_selected = slot == self.selected;
                        let style = if is_selected {
                            Style::default()
                                .fg(palette::SELECTION_TEXT)
                                .bg(palette::SELECTION_BG)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(palette::TEXT_PRIMARY)
                        };
                        let cursor = if is_selected { "▶ " } else { "  " };
                        let label = truncate_to_width(&entry.label, label_width);
                        let desc = truncate_to_width(&entry.description, desc_capacity);
                        let line_text = format!("{cursor}{label:<label_width$}  {desc}",);
                        lines.push(Line::from(Span::styled(line_text, style)));
                    }
                }
            }
        }

        let block = modal_block()
            .title(Line::from(vec![Span::styled(
                format!(" {} ", self.tr(MessageId::HelpTitle)),
                Style::default()
                    .fg(palette::WHALE_ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]))
            .title_bottom(Line::from(vec![
                Span::styled(
                    self.tr(MessageId::HelpFooterTypeFilter),
                    Style::default().fg(palette::TEXT_MUTED),
                ),
                Span::styled(
                    self.tr(MessageId::HelpFooterMove),
                    Style::default().fg(palette::TEXT_MUTED),
                ),
                Span::styled(
                    self.tr(MessageId::HelpFooterJump),
                    Style::default().fg(palette::TEXT_MUTED),
                ),
                Span::styled(
                    self.tr(MessageId::HelpFooterClose),
                    Style::default().fg(palette::TEXT_MUTED),
                ),
            ]));

        Paragraph::new(lines).block(block).render(popup_area, buf);
    }
}

#[cfg(test)]
mod tests {}
