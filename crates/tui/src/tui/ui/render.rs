//! TUI rendering logic.
//!
//! Contains the main render function and frame drawing utilities.

use anyhow::Result;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::{Frame, Terminal, layout::Direction};
use std::io::Write;

use super::super::app::{App, OnboardingState};
use super::context_usage::context_usage_snapshot;
use super::sidebar_geometry::sidebar_width_for_chat_area;
use crate::tui::slash_menu::visible_slash_menu_entries;
use crate::tui::widgets::{ChatWidget, ComposerWidget, HeaderData, HeaderWidget, Renderable};

// UI constants
const SLASH_MENU_LIMIT: usize = 128;
const MIN_CHAT_HEIGHT: u16 = 3;
const MIN_COMPOSER_HEIGHT: u16 = 2;
const SIDEBAR_VISIBLE_MIN_WIDTH: u16 = 64;

// Terminal sync constants for synchronized-update mode
const BEGIN_SYNC_UPDATE: &[u8] = b"\x1b[?2026h";
const END_SYNC_UPDATE: &[u8] = b"\x1b[?2026l";
const TERMINAL_ORIGIN_RESET: &[u8] = b"\x1b[H";

use super::toast::render_toast_stack_overlay;
use crate::tui::color_compat::ColorCompatBackend;
use crate::tui::footer_ui::render_footer;
use crate::tui::ui_text::text_display_width;
use crate::tui::views::ModalKind;

use crate::tui::onboarding;

type AppTerminal = Terminal<ColorCompatBackend<std::io::Stdout>>;

/// Render the complete application UI.
pub(crate) fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Clear entire area with the configured app background.
    let background = Block::default().style(Style::default().bg(app.ui_theme.surface_bg));
    f.render_widget(background, size);

    // Show onboarding screen if needed
    if app.onboarding != OnboardingState::None {
        onboarding::render(f, size, app);
        return;
    }

    let header_height = 1;
    let footer_height = 1;
    let slash_menu_entries = visible_slash_menu_entries(app, SLASH_MENU_LIMIT);
    let mention_menu_limit = app.mention_limit;
    let mention_menu_entries =
        crate::tui::file_mention::visible_mention_menu_entries(app, mention_menu_limit);
    if !mention_menu_entries.is_empty() && app.mention_menu_selected >= mention_menu_entries.len() {
        app.mention_menu_selected = mention_menu_entries.len().saturating_sub(1);
    }
    let context_usage = context_usage_snapshot(app);

    // Defensive two-pass layout: pin the header to the absolute top row,
    // then split the remaining body area for chat / preview / composer /
    // footer. This guarantees the header is never vertically centered
    // regardless of ratatui Flex defaults or terminal size.
    // Fixes #1834 — macOS terminal title centering.
    let (header_area, body_area) = {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .flex(ratatui::layout::Flex::Start)
            .constraints([Constraint::Length(header_height), Constraint::Min(1)])
            .split(size);
        (split[0], split[1])
    };

    let body_height = body_area.height;
    let composer_max_height = body_height
        .saturating_sub(MIN_CHAT_HEIGHT + footer_height)
        .max(MIN_COMPOSER_HEIGHT);
    let composer_height = {
        let composer_widget = ComposerWidget::new(
            app,
            composer_max_height,
            &slash_menu_entries,
            &mention_menu_entries,
        );
        composer_widget.desired_height(size.width)
    };

    // Pending-input preview (queued / steered messages). Empty when nothing's
    // queued, so zero height when idle. Phase 2 of #85 — solves the
    // "messages typed during a running turn vanish" complaint by giving the
    // user immediate visible feedback above the composer.
    let pending_preview = super::build_pending_input_preview(app);
    let preview_height = pending_preview.desired_height(size.width);

    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .flex(ratatui::layout::Flex::Start)
        .constraints([
            Constraint::Min(1),                  // Chat area
            Constraint::Length(preview_height),  // Pending input preview (0 if empty)
            Constraint::Length(composer_height), // Composer
            Constraint::Length(footer_height),   // Footer
        ])
        .split(body_area);

    // Render header
    {
        let sanitized_context_window = context_usage
            .as_ref()
            .map(|(_, max, _)| *max)
            .or_else(|| crate::models::context_window_for_model(&app.model));
        let sanitized_prompt_tokens = context_usage
            .as_ref()
            .and_then(|(used, _, _)| u32::try_from(*used).ok());
        let workspace_name = app
            .workspace
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("workspace");
        let model_label = app.model_display_label();
        let effort_label = app.reasoning_effort_display_label();
        let provider_label = match app.api_provider {
            crate::config::ApiProvider::OpenAiCompatible => Some("OpenAI Compatible"),
            crate::config::ApiProvider::AnthropicCompatible => Some("Anthropic Compatible"),
            crate::config::ApiProvider::GeminiCompatible => Some("Gemini Compatible"),
        };
        let status_indicator_started_at = if app.low_motion {
            None
        } else {
            app.turn_started_at
        };
        let header_data = HeaderData::new(
            app.mode,
            &model_label,
            workspace_name,
            app.is_loading,
            app.ui_theme.header_bg,
        )
        .with_usage(
            app.session.total_conversation_tokens,
            sanitized_context_window,
            app.session.session_cost,
            sanitized_prompt_tokens,
        )
        .with_reasoning_effort(Some(&effort_label))
        .with_provider(provider_label)
        .with_status_indicator(crate::tui::widgets::header_status_indicator_frame(
            status_indicator_started_at,
            &app.status_indicator,
        ));
        let header_widget = HeaderWidget::new(header_data);
        let buf = f.buffer_mut();
        header_widget.render(header_area, buf);
    }

    // Render chat + sidebar + optional file-tree pane
    {
        // Defensive backstop (#400): fill the entire body area with ink
        // background before any sub-widgets render, so cells that end up
        // uncovered by layout splits (e.g. after file-tree toggle or
        // resize) don't retain stale content from a previous frame.
        Block::default()
            .style(Style::default().bg(app.ui_theme.surface_bg))
            .render(body_chunks[0], f.buffer_mut());

        let mut sidebar_area = None;

        // When the file-tree pane is visible and the terminal is wide
        // enough, reserve the left ~25% for the file tree.
        let mut chat_area = if app.file_tree.is_some()
            && body_chunks[0].width >= SIDEBAR_VISIBLE_MIN_WIDTH
        {
            app.file_tree_visible = true;
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
                .split(body_chunks[0]);
            let tree_area = split[0];
            let remaining = split[1];

            // Render the file-tree pane.
            if let Some(ref mut state) = app.file_tree {
                super::super::file_tree::render_file_tree(f, tree_area, state, app.ui_theme.mode);
            }

            remaining
        } else {
            app.file_tree_visible = false;
            body_chunks[0]
        };

        // Auto-reveal: in Auto focus mode, collapse the sidebar to a
        // full-width transcript when nothing is active; bring it back the
        // moment there is a To-do, a live fleet, or background jobs.
        app.last_sidebar_host_width = Some(chat_area.width);
        let sidebar_auto_collapsed = crate::tui::sidebar::sidebar_auto_idle(app);
        if !sidebar_auto_collapsed
            && let Some(sidebar_width) = sidebar_width_for_chat_area(app, chat_area.width)
        {
            // Record total width for drag-to-resize percentage calculation.
            app.sidebar_resize_total_width = chat_area.width;
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(1), Constraint::Length(sidebar_width)])
                .split(chat_area);
            chat_area = split[0];
            sidebar_area = Some(split[1]);
        }

        // Record the sidebar rect (or its absence) every frame so mouse
        // hit-testing can route scroll events correctly.
        app.viewport.last_sidebar_area = sidebar_area;

        // When the sidebar is hidden or doesn't fit, drop its stale mouse
        // hit areas and any in-flight resize so clicks on those columns
        // don't keep routing to an invisible handle (#3063).
        if sidebar_area.is_none() {
            app.last_sidebar_area = None;
            app.last_sidebar_handle_area = None;
            app.sidebar_resizing = false;
        }

        let chat_widget = ChatWidget::new(app, chat_area);
        let buf = f.buffer_mut();
        chat_widget.render(chat_area, buf);

        if let Some(sidebar_area) = sidebar_area {
            // Store sidebar area for mouse hit-testing (resize handle).
            app.last_sidebar_area = Some(sidebar_area);

            // Render sidebar
            super::super::sidebar::render_sidebar(f, sidebar_area, app);

            // Paint resize handle (1-col draggable bar) on the left edge of
            // the sidebar, over the sidebar content. Mouse drag on this strip
            // adjusts sidebar_width_percent in real time.
            let handle_rect = Rect {
                x: sidebar_area.x,
                y: sidebar_area.y,
                width: 1,
                height: sidebar_area.height,
            };

            // Store for mouse event handler.
            app.last_sidebar_handle_area = Some(handle_rect);

            let mouse_over = app.last_mouse_pos.is_some_and(|(col, row)| {
                row >= handle_rect.y
                    && row < handle_rect.y.saturating_add(handle_rect.height)
                    && col == handle_rect.x
            });

            let handle_style = if app.sidebar_resizing {
                Style::default()
                    .bg(crate::palette::MIMOFAN_ACCENT_PRIMARY)
                    .fg(crate::palette::TEXT_PRIMARY)
            } else if mouse_over {
                Style::default()
                    .bg(crate::palette::STATUS_WARNING)
                    .fg(crate::palette::TEXT_MUTED)
            } else {
                Style::default()
                    .bg(crate::palette::MIMOFAN_SLATE)
                    .fg(crate::palette::TEXT_MUTED)
            };

            let buf = f.buffer_mut();
            for row in handle_rect.y..handle_rect.y.saturating_add(handle_rect.height) {
                if row < buf.area().height {
                    buf[(handle_rect.x, row)]
                        .set_char('│')
                        .set_style(handle_style);
                }
            }

            // Render sidebar hover popover if active.
            if let Some(ref tooltip_text) = app.sidebar_hover_tooltip
                && let Some((mouse_col, mouse_row)) = app.last_mouse_pos
            {
                let max_popup_width = 72u16.min(size.width.saturating_sub(4));
                if max_popup_width >= 10 && size.height >= 3 {
                    let popup_width = tooltip_text
                        .lines()
                        .map(text_display_width)
                        .max()
                        .unwrap_or(0)
                        .saturating_add(2)
                        .clamp(12, max_popup_width as usize)
                        as u16;
                    let inner_width = popup_width.saturating_sub(2).max(1) as usize;
                    let wrapped_rows = tooltip_text.lines().fold(0u16, |rows, line| {
                        let width = text_display_width(line);
                        rows.saturating_add(((width.max(1) - 1) / inner_width + 1) as u16)
                    });
                    let popup_content_height = wrapped_rows.clamp(1, 10);
                    let popup_height = popup_content_height.saturating_add(2);
                    let x = mouse_col
                        .saturating_add(2)
                        .min(size.width.saturating_sub(popup_width));
                    // Sit one row BELOW the cursor so the tooltip never paints over
                    // the row above the hovered line (which read as corruption).
                    let y = mouse_row
                        .saturating_add(1)
                        .min(size.height.saturating_sub(popup_height));
                    let tooltip_area = Rect {
                        x,
                        y,
                        width: popup_width,
                        height: popup_height,
                    };
                    // Neutral elevated-surface styling so the popover reads as a
                    // detail surface, not a warning highlight.
                    let tooltip = ratatui::widgets::Paragraph::new(tooltip_text.as_str())
                        .wrap(ratatui::widgets::Wrap { trim: false })
                        .block(
                            Block::default()
                                .borders(ratatui::widgets::Borders::ALL)
                                .border_style(
                                    Style::default().fg(crate::palette::MIMOFAN_ACCENT_PRIMARY),
                                )
                                .style(
                                    Style::default()
                                        .bg(crate::palette::SURFACE_ELEVATED)
                                        .fg(crate::palette::TEXT_PRIMARY),
                                ),
                        );
                    f.render_widget(tooltip, tooltip_area);
                }
            }
        }
    }

    // Render pending-input preview (queued/steered messages, if any).
    if preview_height > 0 {
        let buf = f.buffer_mut();
        pending_preview.render(body_chunks[1], buf);
    }

    // Render composer
    let cursor_pos = {
        let composer_widget = ComposerWidget::new(
            app,
            composer_max_height,
            &slash_menu_entries,
            &mention_menu_entries,
        );
        let buf = f.buffer_mut();
        composer_widget.render(body_chunks[2], buf);
        composer_widget.cursor_pos(body_chunks[2])
    };
    app.viewport.last_composer_area = Some(body_chunks[2]);
    {
        let area = body_chunks[2];
        let has_panel = app.composer_border && area.height >= 3 && area.width >= 12;
        let inner = if has_panel {
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .inner(area)
        } else {
            area
        };
        app.viewport.last_composer_content = Some(inner);

        // Compute scroll offset and top padding for mouse coordinate mapping.
        let input_text = app.composer_display_input();
        let input_cursor = app.composer_display_cursor();
        let content_width = usize::from(inner.width.max(1));
        let menu_lines = ComposerWidget::new(
            app,
            composer_max_height,
            &slash_menu_entries,
            &mention_menu_entries,
        )
        .active_menu_reserved_rows();
        let budget = crate::tui::widgets::composer_input_rows_budget(inner.height, menu_lines);
        let (_, _, _, scroll_offset) = crate::tui::widgets::layout_input_with_scroll(
            input_text,
            input_cursor,
            content_width,
            budget,
        );
        let visible_lines = if input_text.is_empty() {
            1
        } else {
            // Count wrapped lines (approximation matching the render path).
            crate::tui::widgets::wrap_input_lines_for_mouse(input_text, content_width).len()
        };
        let top_padding = budget.saturating_sub(visible_lines.clamp(1, budget));
        app.viewport.last_composer_scroll_offset = scroll_offset;
        app.viewport.last_composer_top_padding = top_padding;
    }
    if let Some(cursor_pos) = cursor_pos {
        f.set_cursor_position(cursor_pos);
    }

    // Render footer
    render_footer(f, body_chunks[3], app);
    // Toast stack overlay (#439): when multiple status toasts are queued,
    // surface the older ones as a 1-2 line strip above the footer so a
    // burst of events isn't collapsed to a single visible message.
    render_toast_stack_overlay(f, size, body_chunks[2], body_chunks[3], app);

    // Decision card overlay (v0.8.43 truth-surface). When a decision card is
    // active, render it centered on top of the transcript.
    if let Some(ref card) = app.decision_card {
        let card_width = size.width.clamp(30, 60);
        let card_height = card.desired_height(card_width);
        let card_area = ratatui::layout::Rect {
            x: size
                .x
                .saturating_add(size.width.saturating_sub(card_width) / 2),
            y: size
                .y
                .saturating_add(size.height.saturating_sub(card_height) / 2),
            width: card_width,
            height: card_height.min(size.height),
        };
        let buf = f.buffer_mut();
        card.render(card_area, buf);
    }

    if !app.view_stack.is_empty() {
        // The live transcript overlay snapshots the app's history + active
        // cell on each render so streaming mutations propagate. Other views
        // are static and skip this refresh.
        if app.view_stack.top_kind() == Some(ModalKind::LiveTranscript) {
            super::refresh_live_transcript_overlay(app);
        }
        let buf = f.buffer_mut();
        app.view_stack.render(size, buf);
    }
}

/// Draw a complete application frame, optionally with a full viewport reset.
///
/// When `full_repaint` is true, the terminal scroll margins and origin mode
/// are reset, the screen is cleared, ratatui's buffer is emptied, and then
/// the full UI is drawn — all within a single DEC 2026 synchronized-update
/// batch so GPU-accelerated terminals (Ghostty, VS Code, Kitty) render one
/// complete frame instead of a blank intermediate frame followed by the UI.
///
/// When `full_repaint` is false, only the diff from the previous draw is
/// written (normal incremental update path).
pub(crate) fn draw_app_frame_inner(
    terminal: &mut AppTerminal,
    app: &mut App,
    full_repaint: bool,
) -> Result<()> {
    terminal.backend_mut().set_palette_mode(app.ui_theme.mode);
    terminal.backend_mut().set_theme(app.theme_id, app.ui_theme);
    // DEC 2026 wrapping is on by default but can be turned off for
    // terminals that mishandle it (Ptyxis 50.x + VTE 0.84.x flashes the
    // whole viewport on every wrapped frame instead of deferring as the
    // standard requires). Settings::synchronized_output_enabled resolves
    // the user's setting against the Ptyxis env auto-detect.
    let wrap_in_sync_update = app.synchronized_output_enabled;
    if wrap_in_sync_update {
        let _ = terminal.backend_mut().write_all(BEGIN_SYNC_UPDATE);
    }

    // Run fallible draw operations in a closure so END_SYNC_UPDATE is
    // always sent even if an intermediate step fails. Without this, a
    // failing `?` would return early and leave the terminal stuck in
    // synchronized-update mode (screen frozen).
    let result = (|| -> Result<()> {
        if full_repaint {
            terminal.backend_mut().write_all(TERMINAL_ORIGIN_RESET)?;
            terminal.clear()?;
        }
        terminal.draw(|f| render(f, app))?;
        Ok(())
    })();

    // Always end the synchronized update, regardless of success or failure.
    if wrap_in_sync_update {
        let _ = terminal.backend_mut().write_all(END_SYNC_UPDATE);
    }
    let _ = std::io::Write::flush(&mut terminal.backend_mut());
    result
}
