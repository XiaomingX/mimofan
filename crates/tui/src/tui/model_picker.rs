//! `/model` picker modal: pick a model and thinking-effort tier (#39, #2026).
//!
//! The picker intentionally presents model and thinking as independent choices
//! instead of collapsing them into preset route names. The "auto" option is
//! always available; custom (unrecognized) model ids appear as a separate row.
//! Pass-through providers fall back to only "auto" plus the current custom row.
//!
//! On apply we emit a [`ViewEvent::ModelPickerApplied`] with the resolved
//! model id and effort tier.

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::config::{ApiProvider, model_completion_names_for_provider};
use crate::model_registry;
use crate::palette;
use crate::tui::app::{App, ReasoningEffort};
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};

/// Thinking-effort rows shown for DeepSeek-style providers, in the order
/// DeepSeek behaviorally distinguishes them.
const DEFAULT_PICKER_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Auto,
    ReasoningEffort::Off,
    ReasoningEffort::High,
    ReasoningEffort::Max,
];
const CODEX_PICKER_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Max,
];
const AUTO_MODEL_PICKER_EFFORTS: &[ReasoningEffort] = &[ReasoningEffort::Auto];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Model,
    Effort,
}

pub struct ModelPickerView {
    initial_model: String,
    initial_provider: ApiProvider,
    initial_effort: ReasoningEffort,
    active_accepts_custom_model_ids: bool,
    query: String,
    /// Working selection (separate from the initial values so we can offer a
    /// clean Esc-to-cancel without mutating App state).
    selected_model_idx: usize,
    selected_effort_idx: usize,
    focus: Pane,
    /// True when the active model is one we don't list — we still show it
    /// so the picker doesn't quietly forget the user's chosen IDs.
    show_custom_model_row: bool,
    model_rows: Vec<ModelPickerRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelPickerRow {
    id: String,
    provider: Option<ApiProvider>,
    hint: String,
}

impl ModelPickerView {
    #[must_use]
    pub fn new(app: &App) -> Self {
        let initial_model = if app.auto_model {
            "auto".to_string()
        } else {
            app.model.clone()
        };
        let model_rows = picker_model_rows_for_app(app);
        let mut selected_model_idx = model_rows.iter().position(|row| {
            row.id == initial_model
                && (row.provider.is_none() || row.provider == Some(app.api_provider))
        });
        let show_custom_model_row = selected_model_idx.is_none();
        if show_custom_model_row {
            selected_model_idx = Some(active_provider_model_row_count(
                &model_rows,
                app.api_provider,
            ));
        }
        let selected_model_idx = selected_model_idx.unwrap_or(0);

        let initial_effort = app.reasoning_effort;
        let effort_rows = picker_efforts_for_provider(app.api_provider, app.auto_model);
        let normalized = normalize_picker_effort(initial_effort, app.api_provider, app.auto_model);
        let selected_effort_idx = effort_rows
            .iter()
            .position(|e| *e == normalized)
            .unwrap_or_else(|| default_picker_effort_idx(app.api_provider, app.auto_model));

        Self {
            initial_model,
            initial_provider: app.api_provider,
            initial_effort,
            active_accepts_custom_model_ids: app.accepts_custom_model_ids(),
            query: String::new(),
            selected_model_idx,
            selected_effort_idx,
            focus: Pane::Model,
            show_custom_model_row,
            model_rows,
        }
    }

    fn visible_model_rows(&self) -> Vec<&ModelPickerRow> {
        let query = self.query.trim();
        self.model_rows
            .iter()
            .filter(|row| {
                if query.is_empty() {
                    row.provider.is_none() || row.provider == Some(self.initial_provider)
                } else {
                    model_row_matches_query(row, query, self.initial_provider)
                }
            })
            .collect()
    }

    fn model_row_count(&self) -> usize {
        let rows = self.visible_model_rows();
        rows.len() + usize::from(self.custom_model_row_for_visible(&rows).is_some())
    }

    /// Resolve the currently highlighted row to a model id.
    fn resolved_model(&self) -> String {
        let rows = self.visible_model_rows();
        if self.selected_model_idx < rows.len() {
            return rows[self.selected_model_idx].id.clone();
        }
        self.custom_model_row()
            .map(|(model, _)| model)
            .unwrap_or_else(|| self.initial_model.clone())
    }

    fn resolved_provider(&self) -> Option<ApiProvider> {
        let rows = self.visible_model_rows();
        if self.selected_model_idx < rows.len() {
            return rows[self.selected_model_idx].provider;
        }
        self.custom_model_row()
            .map(|(_, provider)| provider)
            .or(Some(self.initial_provider))
    }

    fn resolved_effort(&self) -> ReasoningEffort {
        if self.resolved_model().trim().eq_ignore_ascii_case("auto") {
            return ReasoningEffort::Auto;
        }
        let efforts = self.current_efforts();
        efforts[self
            .selected_effort_idx
            .min(efforts.len().saturating_sub(1))]
    }

    fn current_efforts(&self) -> &'static [ReasoningEffort] {
        picker_efforts_for_provider(
            self.resolved_provider().unwrap_or(self.initial_provider),
            self.resolved_model().trim().eq_ignore_ascii_case("auto"),
        )
    }

    fn custom_model_row(&self) -> Option<(String, ApiProvider)> {
        let rows = self.visible_model_rows();
        self.custom_model_row_for_visible(&rows)
    }

    fn custom_model_row_for_visible(
        &self,
        visible_rows: &[&ModelPickerRow],
    ) -> Option<(String, ApiProvider)> {
        let query = self.query.trim();
        if query.is_empty() {
            return self
                .show_custom_model_row
                .then(|| (self.initial_model.clone(), self.initial_provider));
        }
        if !self.active_accepts_custom_model_ids {
            return None;
        }
        if visible_rows.iter().any(|row| {
            row.provider == Some(self.initial_provider) && row.id.eq_ignore_ascii_case(query)
        }) {
            return None;
        }
        Some((query.to_string(), self.initial_provider))
    }

    fn clamp_model_selection(&mut self) {
        let count = self.model_row_count();
        if count == 0 {
            self.selected_model_idx = 0;
        } else if self.selected_model_idx >= count {
            self.selected_model_idx = count - 1;
        }
    }

    fn update_query(&mut self, next: String) {
        let effort = self.resolved_effort();
        self.query = next;
        self.selected_model_idx = 0;
        self.clamp_model_selection();
        self.select_effort_for_current_model(effort);
    }

    fn select_effort_for_current_model(&mut self, effort: ReasoningEffort) {
        let provider = self.resolved_provider().unwrap_or(self.initial_provider);
        let model_is_auto = self.resolved_model().trim().eq_ignore_ascii_case("auto");
        let normalized = normalize_picker_effort(effort, provider, model_is_auto);
        self.selected_effort_idx = picker_efforts_for_provider(provider, model_is_auto)
            .iter()
            .position(|candidate| *candidate == normalized)
            .unwrap_or_else(|| default_picker_effort_idx(provider, model_is_auto));
    }

    fn move_up(&mut self) -> bool {
        match self.focus {
            Pane::Model => {
                if self.selected_model_idx > 0 {
                    let effort = self.resolved_effort();
                    self.selected_model_idx -= 1;
                    self.select_effort_for_current_model(effort);
                    return true;
                }
            }
            Pane::Effort => {
                if self.selected_effort_idx > 0 {
                    self.selected_effort_idx -= 1;
                    return true;
                }
            }
        }
        false
    }

    fn move_down(&mut self) -> bool {
        match self.focus {
            Pane::Model => {
                let max = self.model_row_count().saturating_sub(1);
                if self.selected_model_idx < max {
                    let effort = self.resolved_effort();
                    self.selected_model_idx += 1;
                    self.select_effort_for_current_model(effort);
                    return true;
                }
            }
            Pane::Effort => {
                let max = self.current_efforts().len().saturating_sub(1);
                if self.selected_effort_idx < max {
                    self.selected_effort_idx += 1;
                    return true;
                }
            }
        }
        false
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Pane::Model => Pane::Effort,
            Pane::Effort => Pane::Model,
        };
    }

    fn build_event(&self) -> ViewEvent {
        let provider = self
            .resolved_provider()
            .filter(|provider| *provider != self.initial_provider);
        ViewEvent::ModelPickerApplied {
            model: self.resolved_model(),
            provider,
            effort: self.resolved_effort(),
            previous_model: self.initial_model.clone(),
            previous_effort: self.initial_effort,
        }
    }

    fn render_pane(
        &self,
        area: Rect,
        buf: &mut Buffer,
        title: &str,
        rows: Vec<(String, String)>,
        selected: usize,
        focused: bool,
    ) {
        let border_style = if focused {
            Style::default().fg(palette::DEEPSEEK_SKY)
        } else {
            Style::default().fg(palette::BORDER_COLOR)
        };
        let visible_height = usize::from(area.height.saturating_sub(2));
        let (start, end) = visible_row_window(selected, rows.len(), visible_height);
        let title = if rows.len() > visible_height && visible_height > 0 {
            format!(" {title} {}-{}/{} ", start + 1, end, rows.len())
        } else {
            format!(" {title} ")
        };
        let block = Block::default()
            .title(Line::from(Span::styled(
                title,
                Style::default().fg(palette::TEXT_PRIMARY).bold(),
            )))
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default());
        let inner = block.inner(area);
        block.render(area, buf);

        let mut lines = Vec::with_capacity(end.saturating_sub(start));
        for (idx, (label, hint)) in rows.iter().enumerate().skip(start).take(end - start) {
            let is_selected = idx == selected;
            let marker = if is_selected { "▸" } else { " " };
            let label_style = if is_selected {
                Style::default()
                    .fg(palette::SELECTION_TEXT)
                    .bg(palette::SELECTION_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette::TEXT_PRIMARY)
            };
            let hint_style = if is_selected {
                Style::default()
                    .fg(palette::SELECTION_TEXT)
                    .bg(palette::SELECTION_BG)
            } else {
                Style::default().fg(palette::TEXT_MUTED)
            };
            let spans = picker_row_spans(
                label,
                hint,
                marker,
                usize::from(inner.width),
                label_style,
                hint_style,
            );
            lines.push(Line::from(spans));
        }
        Paragraph::new(lines).render(inner, buf);
    }
}

fn visible_row_window(selected: usize, total: usize, viewport_height: usize) -> (usize, usize) {
    if total == 0 || viewport_height == 0 {
        return (0, 0);
    }

    let visible = viewport_height.min(total);
    let mut start = selected.saturating_sub(visible / 2);
    if start + visible > total {
        start = total.saturating_sub(visible);
    }
    (start, start + visible)
}

fn picker_row_spans<'a>(
    label: &'a str,
    hint: &'a str,
    marker: &'static str,
    width: usize,
    label_style: Style,
    hint_style: Style,
) -> Vec<Span<'a>> {
    let prefix_width = 3;
    let label_width = width.saturating_sub(prefix_width);
    let label = fit_text(label, label_width);
    let mut spans = vec![
        Span::styled(" ", label_style),
        Span::styled(marker, label_style),
        Span::styled(" ", label_style),
        Span::styled(label, label_style),
    ];

    if !hint.is_empty() {
        let hint_text = format!("  ({hint})");
        let used = prefix_width
            + unicode_width::UnicodeWidthStr::width(
                spans
                    .last()
                    .map(|span| span.content.as_ref())
                    .unwrap_or_default(),
            );
        if used + unicode_width::UnicodeWidthStr::width(hint_text.as_str()) <= width {
            spans.push(Span::styled(hint_text, hint_style));
        }
    }

    spans
}

fn fit_text(text: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width <= 3 {
        return ".".repeat(width);
    }

    let mut out = String::new();
    let target = width - 3;
    let mut used = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > target {
            break;
        }
        used += ch_width;
        out.push(ch);
    }
    out.push_str("...");
    out
}

pub(crate) fn provider_scoped_model_completion_ids(app: &App) -> Vec<String> {
    // Slash completions inline the current custom model so `/model <current>`
    // stays visible even when it is outside the provider catalog.
    provider_scoped_model_ids_for_app(app, true)
}

fn picker_model_rows_for_app(app: &App) -> Vec<ModelPickerRow> {
    let mut rows = Vec::new();
    push_provider_model_rows(
        &mut rows,
        app.api_provider,
        provider_scoped_model_ids_for_app(app, false),
        app.api_provider,
    );

    for provider in ApiProvider::sorted_for_display() {
        if provider == app.api_provider {
            continue;
        }
        let mut model_ids = provider_catalog_model_ids(provider);
        if let Some(model) = app
            .provider_models
            .get(provider.as_str())
            .map(|model| model.trim())
            .filter(|model| !model.is_empty())
        {
            push_model_id(&mut model_ids, model);
        }
        push_provider_model_rows(&mut rows, provider, model_ids, app.api_provider);
    }

    rows
}

fn push_provider_model_rows(
    rows: &mut Vec<ModelPickerRow>,
    provider: ApiProvider,
    model_ids: Vec<String>,
    active_provider: ApiProvider,
) {
    for id in model_ids {
        if id == "auto" {
            push_model_row(rows, id, None, picker_model_hint("auto"));
        } else {
            let mut hint = picker_model_hint(&id);
            if provider != active_provider {
                hint = format!("switch route · {hint}");
            }
            push_model_row(rows, id.clone(), Some(provider), hint);
        }
    }
}

fn provider_catalog_model_ids(provider: ApiProvider) -> Vec<String> {
    let mut models = Vec::new();
    for id in model_completion_names_for_provider(provider) {
        if id != "auto" {
            push_model_id(&mut models, id);
        }
    }
    models
}

fn provider_scoped_model_ids_for_app(app: &App, include_current_model: bool) -> Vec<String> {
    // `include_current_model` is for completion surfaces that do not have a
    // separate custom/current-model row.
    let mut models = Vec::new();
    push_model_id(&mut models, "auto");
    for id in model_completion_names_for_provider(app.api_provider) {
        push_model_id(&mut models, id);
    }

    if let Some(model) = app
        .provider_models
        .get(app.api_provider.as_str())
        .map(|model| model.trim())
        .filter(|model| !model.is_empty())
    {
        push_model_id(&mut models, model);
    }

    if include_current_model && !app.auto_model {
        push_model_id(&mut models, app.model.trim());
    }

    models
}

fn push_model_id(models: &mut Vec<String>, model: &str) {
    let model = model.trim();
    if model.is_empty() {
        return;
    }
    if !models
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(model))
    {
        models.push(model.to_string());
    }
}

fn push_model_row(
    rows: &mut Vec<ModelPickerRow>,
    id: String,
    provider: Option<ApiProvider>,
    hint: String,
) {
    if rows
        .iter()
        .any(|row| row.id == id && row.provider == provider)
    {
        return;
    }
    rows.push(ModelPickerRow { id, provider, hint });
}

fn model_row_matches_query(
    row: &ModelPickerRow,
    query: &str,
    initial_provider: ApiProvider,
) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    let provider_matches = row.provider.is_some_and(|provider| {
        provider.as_str().contains(&query)
            || provider
                .display_name()
                .to_ascii_lowercase()
                .contains(&query)
    });
    provider_matches
        || row.id.to_ascii_lowercase().contains(&query)
        || ((row.provider.is_none() || row.provider == Some(initial_provider))
            && row.hint.to_ascii_lowercase().contains(&query))
}

fn model_row_label(row: &ModelPickerRow, initial_provider: ApiProvider) -> String {
    match row.provider {
        Some(provider) if provider != initial_provider => {
            format!("{} · {}", provider.display_name(), row.id)
        }
        _ => row.id.clone(),
    }
}

fn active_provider_model_row_count(rows: &[ModelPickerRow], provider: ApiProvider) -> usize {
    rows.iter()
        .filter(|row| row.provider.is_none() || row.provider == Some(provider))
        .count()
}

fn picker_model_hint(id: &str) -> String {
    if id == "auto" {
        return "select per turn".to_string();
    }
    let Some(metadata) = model_registry::lookup(id) else {
        return "provider model".to_string();
    };

    let mut parts = Vec::new();
    if let Some(context_window) = metadata.context_window {
        parts.push(format!(
            "{} ctx",
            format_picker_context_window(context_window)
        ));
    }
    if metadata.supports_reasoning {
        parts.push("reasoning".to_string());
    }
    parts.push(if crate::pricing::has_pricing_for_model(id) {
        "priced".to_string()
    } else {
        "price unknown".to_string()
    });
    parts.join(" · ")
}

fn format_picker_context_window(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        if tokens.is_multiple_of(1_000_000) {
            format!("{}M", tokens / 1_000_000)
        } else {
            format!("{:.2}M", tokens as f64 / 1_000_000.0)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
    } else if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

impl ModalView for ModelPickerView {
    fn kind(&self) -> ModalKind {
        ModalKind::ModelPicker
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => ViewAction::Close,
            KeyCode::Enter if self.model_row_count() == 0 => ViewAction::None,
            KeyCode::Enter => ViewAction::EmitAndClose(self.build_event()),
            KeyCode::Char(ch)
                if self.focus == Pane::Model
                    && !key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                let mut query = self.query.clone();
                query.push(ch);
                self.update_query(query);
                ViewAction::None
            }
            KeyCode::Backspace if self.focus == Pane::Model && !self.query.is_empty() => {
                let mut query = self.query.clone();
                query.pop();
                self.update_query(query);
                ViewAction::None
            }
            KeyCode::Up => {
                self.move_up();
                ViewAction::None
            }
            KeyCode::Down => {
                self.move_down();
                ViewAction::None
            }
            KeyCode::PageUp => {
                for _ in 0..5 {
                    self.move_up();
                }
                ViewAction::None
            }
            KeyCode::PageDown => {
                for _ in 0..5 {
                    self.move_down();
                }
                ViewAction::None
            }
            KeyCode::Home => {
                match self.focus {
                    Pane::Model => {
                        let effort = self.resolved_effort();
                        self.selected_model_idx = 0;
                        self.select_effort_for_current_model(effort);
                    }
                    Pane::Effort => self.selected_effort_idx = 0,
                }
                ViewAction::None
            }
            KeyCode::End => {
                match self.focus {
                    Pane::Model => {
                        let effort = self.resolved_effort();
                        self.selected_model_idx = self.model_row_count().saturating_sub(1);
                        self.select_effort_for_current_model(effort);
                    }
                    Pane::Effort => {
                        self.selected_effort_idx = self.current_efforts().len().saturating_sub(1);
                    }
                }
                ViewAction::None
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Left | KeyCode::BackTab => {
                self.toggle_focus();
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> ViewAction {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.move_up();
                ViewAction::None
            }
            MouseEventKind::ScrollDown => {
                self.move_down();
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_classic(area, buf);
    }
}

impl ModelPickerView {
    fn render_classic(&self, area: Rect, buf: &mut Buffer) {
        let available_width = area.width.saturating_sub(4);
        let popup_width = if available_width >= 60 {
            available_width.min(96)
        } else {
            area.width.saturating_sub(2).max(1)
        };
        let desired_height = (self.model_row_count().max(self.current_efforts().len()) as u16)
            .saturating_add(4)
            .clamp(10, 22);
        let available_height = area.height.saturating_sub(4);
        let popup_height = if available_height >= 10 {
            desired_height.min(available_height)
        } else {
            area.height.saturating_sub(2).max(1)
        };
        let popup_area = Rect {
            x: area.x + (area.width.saturating_sub(popup_width)) / 2,
            y: area.y + (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        // Outer chrome with title + footer hint.
        let outer = Block::default()
            .title(Line::from(Span::styled(
                " Model & thinking ",
                Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(vec![
                Span::styled(" ↑↓ ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("move "),
                Span::styled(" Tab ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("switch "),
                Span::styled(" Type ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("filter "),
                Span::styled(" Enter ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("apply "),
                Span::styled(" Esc ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("cancel "),
            ]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default());
        let inner = outer.inner(popup_area);
        outer.render(popup_area, buf);

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
            .split(inner);

        let mut model_rows: Vec<(String, String)> = self
            .visible_model_rows()
            .iter()
            .map(|row| {
                (
                    model_row_label(row, self.initial_provider),
                    row.hint.clone(),
                )
            })
            .collect();
        if let Some((model, provider)) = self.custom_model_row() {
            let label = if self.query.trim().is_empty() {
                model
            } else {
                format!("{} · {}", provider.display_name(), model)
            };
            let hint = if self.query.trim().is_empty() {
                "current (custom)".to_string()
            } else {
                "custom route".to_string()
            };
            model_rows.push((label, hint));
        }
        let model_title = if self.query.trim().is_empty() {
            "Model".to_string()
        } else {
            format!("Model: {}", self.query.trim())
        };
        self.render_pane(
            columns[0],
            buf,
            &model_title,
            model_rows,
            self.selected_model_idx,
            self.focus == Pane::Model,
        );

        let effort_provider = self.resolved_provider().unwrap_or(self.initial_provider);
        let current_efforts = self.current_efforts();
        let selected_effort_idx = self
            .selected_effort_idx
            .min(current_efforts.len().saturating_sub(1));
        let effort_rows: Vec<(String, String)> = current_efforts
            .iter()
            .map(|effort| {
                let label = effort
                    .display_label_for_provider(effort_provider)
                    .to_string();
                let hint = match effort {
                    ReasoningEffort::Auto => "choose per turn".to_string(),
                    ReasoningEffort::Off => "no extra reasoning".to_string(),
                    ReasoningEffort::Low => "lighter reasoning".to_string(),
                    ReasoningEffort::Medium => "balanced reasoning".to_string(),
                    ReasoningEffort::High => "deeper reasoning".to_string(),
                    ReasoningEffort::Max => {
                        if effort_provider == ApiProvider::XiaomiMimo {
                            "extra-high reasoning".to_string()
                        } else {
                            "maximum reasoning".to_string()
                        }
                    }
                };
                (label, hint)
            })
            .collect();
        self.render_pane(
            columns[1],
            buf,
            "Thinking",
            effort_rows,
            selected_effort_idx,
            self.focus == Pane::Effort,
        );
    }
}

fn picker_efforts_for_provider(
    provider: ApiProvider,
    model_is_auto: bool,
) -> &'static [ReasoningEffort] {
    if model_is_auto {
        return AUTO_MODEL_PICKER_EFFORTS;
    }
    match provider {
        ApiProvider::XiaomiMimo => CODEX_PICKER_EFFORTS,
        _ => DEFAULT_PICKER_EFFORTS,
    }
}

fn normalize_picker_effort(
    effort: ReasoningEffort,
    provider: ApiProvider,
    model_is_auto: bool,
) -> ReasoningEffort {
    if model_is_auto {
        return ReasoningEffort::Auto;
    }
    if provider == ApiProvider::XiaomiMimo {
        return effort.normalize_for_provider(provider);
    }
    match effort {
        ReasoningEffort::Low | ReasoningEffort::Medium => ReasoningEffort::High,
        other => other,
    }
}

fn default_picker_effort_idx(provider: ApiProvider, model_is_auto: bool) -> usize {
    let default_effort = if model_is_auto {
        ReasoningEffort::Auto
    } else if provider == ApiProvider::XiaomiMimo {
        ReasoningEffort::Medium
    } else {
        ReasoningEffort::High
    };
    picker_efforts_for_provider(provider, model_is_auto)
        .iter()
        .position(|effort| *effort == default_effort)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {}
