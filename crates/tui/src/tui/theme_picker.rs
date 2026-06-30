//! `/theme` picker with live preview.
//!
//! Modeled after `feedback_picker`. Differences:
//! - The option list comes from `palette::SELECTABLE_THEMES`.
//! - Up/Down emit a `ConfigUpdated{persist:false}` so the host swaps
//!   `app.ui_theme` immediately and the whole TUI re-paints under the
//!   modal — the user sees the candidate theme before committing.
//! - Enter persists (`persist:true`); Esc emits one more
//!   `ConfigUpdated{persist:false}` to restore the original theme name
//!   that was active when the picker opened.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
};

use crate::palette::{SELECTABLE_THEMES, ThemeId, UiTheme};
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};

pub struct ThemePickerView {
    selected: usize,
    /// Settings name of the theme that was active when the picker opened.
    /// Used to revert on Esc.
    original_name: String,
    /// Cached UiTheme for `ThemeId::System`, captured once at construction
    /// so the per-frame render doesn't re-invoke `UiTheme::detect()` (which
    /// reads `COLORFGBG`) on every keystroke.
    system_ui_theme: UiTheme,
}

impl ThemePickerView {
    #[must_use]
    pub fn new(original_name: String) -> Self {
        // If the persisted name matches one of the entries, start there;
        // otherwise fall back to "System" so the cursor lands on a valid row.
        let selected = SELECTABLE_THEMES
            .iter()
            .position(|id| id.name() == original_name.trim().to_ascii_lowercase())
            .unwrap_or(0);
        Self {
            selected,
            original_name,
            system_ui_theme: UiTheme::detect(),
        }
    }

    fn current(&self) -> ThemeId {
        SELECTABLE_THEMES
            .get(self.selected)
            .copied()
            .unwrap_or(ThemeId::System)
    }

    /// Resolve a theme to a `UiTheme`, returning the cached `System`
    /// resolution to avoid repeated env-var reads inside `render`.
    fn ui_theme_for(&self, id: ThemeId) -> UiTheme {
        if matches!(id, ThemeId::System) {
            self.system_ui_theme
        } else {
            id.ui_theme()
        }
    }

    fn preview_event(&self) -> ViewAction {
        ViewAction::Emit(ViewEvent::ConfigUpdated {
            key: "theme".to_string(),
            value: self.current().name().to_string(),
            persist: false,
        })
    }

    fn commit_event(&self) -> ViewAction {
        ViewAction::EmitAndClose(ViewEvent::ConfigUpdated {
            key: "theme".to_string(),
            value: self.current().name().to_string(),
            persist: true,
        })
    }

    fn revert_event(&self) -> ViewAction {
        ViewAction::EmitAndClose(ViewEvent::ConfigUpdated {
            key: "theme".to_string(),
            value: self.original_name.clone(),
            persist: false,
        })
    }

    fn move_up(&mut self) {
        self.selected = (self.selected + SELECTABLE_THEMES.len() - 1) % SELECTABLE_THEMES.len();
    }

    fn move_down(&mut self) {
        self.selected = (self.selected + 1) % SELECTABLE_THEMES.len();
    }
}

impl ModalView for ThemePickerView {
    fn kind(&self) -> ModalKind {
        ModalKind::ThemePicker
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> ViewAction {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_up(),
            MouseEventKind::ScrollDown => self.move_down(),
            _ => {}
        }
        ViewAction::None
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => self.revert_event(),
            KeyCode::Enter => self.commit_event(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_up();
                self.preview_event()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_down();
                self.preview_event()
            }
            KeyCode::Home => {
                self.selected = 0;
                self.preview_event()
            }
            KeyCode::End => {
                self.selected = SELECTABLE_THEMES.len().saturating_sub(1);
                self.preview_event()
            }
            // Number shortcuts: '1'..='9' jump to that row (1-indexed).
            // '0' is rejected explicitly — saturating_sub would otherwise
            // collapse it onto row 0, which is unintuitive.
            KeyCode::Char(c)
                if matches!(c, '1'..='9')
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let idx = (c as usize) - ('1' as usize);
                if idx < SELECTABLE_THEMES.len() {
                    self.selected = idx;
                    self.preview_event()
                } else {
                    ViewAction::None
                }
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        // Modal must always fit inside `area`. The old `.max(52) / .max(10)`
        // floors could produce dimensions larger than the available area on
        // very small terminals (or split-pane setups), which then made the
        // centering arithmetic underflow and ratatui assert. Take a
        // soft-preferred size and clamp it strictly to `area`.
        let popup_width = 78u16.min(area.width.saturating_sub(4));
        // 1 title + 1 spacer + N rows + spacer + bottom hint
        let needed_height = (SELECTABLE_THEMES.len() as u16).saturating_add(9);
        let popup_height = needed_height.min(area.height.saturating_sub(4));

        if popup_width == 0 || popup_height == 0 {
            // Nothing sensible to draw — the host's caller has already
            // cleared the area, so we just return.
            return;
        }

        let popup_area = Rect {
            x: area.x + (area.width.saturating_sub(popup_width)) / 2,
            y: area.y + (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        // The live theme has already been swapped under us via ConfigUpdated,
        // so we pull the *current* preview's UiTheme from the cursor row to
        // skin the modal chrome. That way the popup itself shifts color as
        // the cursor moves, matching what the background will look like
        // after Enter.
        let live = self.ui_theme_for(self.current());

        Clear.render(popup_area, buf);

        let block = Block::default()
            .title(Line::from(Span::styled(
                " Theme ",
                Style::default()
                    .fg(live.status_working)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(vec![
                Span::styled(" ↑/↓ ", Style::default().fg(live.text_muted)),
                Span::raw("preview "),
                Span::styled(" Enter ", Style::default().fg(live.text_muted)),
                Span::raw("save "),
                Span::styled(" Esc ", Style::default().fg(live.text_muted)),
                Span::raw("revert "),
            ]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(live.border))
            .style(Style::default().bg(live.surface_bg))
            .padding(Padding::uniform(1));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        let mut lines: Vec<Line> = Vec::with_capacity(SELECTABLE_THEMES.len() + 5);
        lines.push(Line::from(Span::styled(
            "Pick a theme — preview is live; Enter saves to settings.toml.",
            Style::default().fg(live.text_muted),
        )));
        lines.push(Line::from(""));

        for (idx, id) in SELECTABLE_THEMES.iter().enumerate() {
            let id = *id;
            let is_selected = idx == self.selected;
            let row_style = if is_selected {
                Style::default()
                    .fg(live.text_body)
                    .bg(live.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(live.text_body)
            };
            let tagline_style = if is_selected {
                Style::default().fg(live.text_muted).bg(live.selection_bg)
            } else {
                Style::default().fg(live.text_dim)
            };
            let number_style = if is_selected {
                Style::default()
                    .fg(live.status_working)
                    .bg(live.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(live.text_hint)
            };
            let pointer = if is_selected { "▶" } else { " " };

            // 3-cell color swatch per row using the candidate theme's own
            // accent + panel + border colors so the picker doubles as a
            // legend. Use the cached resolver so `System` doesn't repeat
            // `UiTheme::detect()`.
            let row_theme = self.ui_theme_for(id);
            let swatch = vec![
                Span::styled("  ", Style::default().bg(row_theme.surface_bg)),
                Span::styled("  ", Style::default().bg(row_theme.panel_bg)),
                Span::styled("  ", Style::default().bg(row_theme.status_working)),
                Span::styled("  ", Style::default().bg(row_theme.mode_yolo)),
                Span::styled("  ", Style::default().bg(row_theme.mode_plan)),
            ];

            let mut spans: Vec<Span> = Vec::with_capacity(8);
            spans.push(Span::styled(format!(" {pointer} "), row_style));
            spans.push(Span::styled(format!("{}. ", idx + 1), number_style));
            spans.push(Span::styled(
                format!("{:<22}", id.display_name()),
                row_style,
            ));
            spans.extend(swatch);
            spans.push(Span::raw("  "));
            spans.push(Span::styled(id.tagline(), tagline_style));

            lines.push(Line::from(spans));
        }

        Paragraph::new(lines).render(inner, buf);
    }
}

#[cfg(test)]
mod tests {}
