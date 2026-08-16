//! Sidebar sub-panel rendering (extracted from sidebar.rs for #647).

use crate::tui::sidebar::*;
use ratatui::{
    Frame,
    layout::Rect,
    prelude::Widget,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use crate::mimofan_theme::Theme;

use crate::tui::app::{App, SidebarHoverRow, SidebarHoverSection};
use crate::tui::ui_text::truncate_line_to_width;

pub(crate) fn render_context_panel(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let theme = &app.ui_theme;
    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(usize::from(area.height).max(4));

    // ── Working set ──────────────────────────────────────────────
    let ws_name = app
        .workspace
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("(root)")
        .to_string();
    lines.push(Line::from(vec![
        Span::styled(
            truncate_line_to_width(&ws_name, content_width.max(1)),
            Style::default().fg(theme.accent_primary).bold(),
        ),
        Span::styled(
            format!("  {}", app.workspace_context.as_deref().unwrap_or("")),
            Style::default().fg(theme.text_dim),
        ),
    ]));

    // ── Token usage ──────────────────────────────────────────────
    let total_tokens = app.session.total_conversation_tokens;
    let window = crate::route_budget::route_context_window_tokens(
        app.api_provider,
        app.effective_model_for_budget(),
        app.active_route_limits,
    );
    let pct = if window > 0 {
        ((total_tokens as f64 / window as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let bar_width = content_width.min(20);
    let filled = ((pct / 100.0) * bar_width as f64) as usize;
    let bar = format!(
        "[{}{}] {:.0}%",
        "█".repeat(filled),
        "░".repeat(bar_width.saturating_sub(filled)),
        pct
    );
    lines.push(Line::from(Span::styled(
        format!(
            "context: {}/{} tokens  {}",
            total_tokens,
            window,
            truncate_line_to_width(&bar, content_width.saturating_sub(32).max(8))
        ),
        Style::default().fg(theme.text_muted),
    )));

    // ── Session cost ─────────────────────────────────────────────
    let cost_line = context_panel_cost_line(app);
    lines.push(Line::from(Span::styled(
        cost_line,
        Style::default().fg(theme.text_muted),
    )));

    // ── MCP servers ──────────────────────────────────────────────
    if app.mcp_configured_count > 0 {
        let restart_hint = if app.mcp_restart_required {
            " (restart needed)"
        } else {
            ""
        };
        lines.push(Line::from(Span::styled(
            format!(
                "mcp: {} server(s){}",
                app.mcp_configured_count, restart_hint
            ),
            Style::default().fg(theme.text_muted),
        )));
    }

    // ── LSP ──────────────────────────────────────────────────────
    let lsp_label = if app.lsp_enabled { "on" } else { "off" };
    lines.push(Line::from(Span::styled(
        format!("lsp: {lsp_label}"),
        Style::default().fg(theme.text_muted),
    )));

    // ── Memory ───────────────────────────────────────────────────
    if app.use_memory {
        let category_count = crate::memory::CATEGORIES
            .iter()
            .filter(|cat| {
                let p = crate::memory::category_path(&app.memory_dir, cat);
                p.exists()
                    && std::fs::read_to_string(&p)
                        .map(|c| !c.trim().is_empty())
                        .unwrap_or(false)
            })
            .count();
        lines.push(Line::from(Span::styled(
            format!(
                "memory: {} ({} categories)",
                app.memory_dir.display(),
                category_count
            ),
            Style::default().fg(theme.text_muted),
        )));
    }

    render_sidebar_section(f, area, "Session", lines, Vec::new(), Vec::new(), app);
}

pub(crate) fn context_panel_cost_line(app: &App) -> String {
    let displayed_total = app.displayed_session_cost_for_currency(app.cost_currency);
    if displayed_total == 0.0 && !crate::pricing::has_pricing_for_model(&app.model) {
        return format!("cost: n/a (no pricing data for {})", app.model);
    }

    let session_cost = app.session_cost_for_currency(app.cost_currency);
    let agent_cost = app.subagent_cost_for_currency(app.cost_currency);
    let real_total = session_cost + agent_cost;
    // Only show the additive breakdown when it matches the displayed
    // total; when the high-water mark is in effect (post-reconciliation),
    // the breakdown would not sum to the displayed value (#244).
    if (displayed_total - real_total).abs() < COST_EQ_TOLERANCE {
        format!(
            "cost: {} (session {} + agents {})",
            app.format_cost_amount(displayed_total),
            app.format_cost_amount(session_cost),
            app.format_cost_amount(agent_cost)
        )
    } else {
        format!("cost: {}", app.format_cost_amount(displayed_total))
    }
}

pub(crate) fn spans_to_text(spans: &[Span<'_>]) -> String {
    let mut s = String::new();
    for span in spans {
        s.push_str(span.content.as_ref());
    }
    s
}

pub(crate) fn render_sidebar_section(
    f: &mut Frame,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    full_texts: Vec<String>,
    row_actions: Vec<Option<String>>,
    app: &mut App,
) {
    if area.width < 4 || area.height < 3 {
        // Clear stale cells before bailing out (#400).
        Block::default()
            .style(Style::default().bg(app.ui_theme.surface_bg))
            .render(area, f.buffer_mut());
        return;
    }

    let theme = Theme::for_palette_mode(app.ui_theme.mode);

    // Record hover metadata for mouse tooltip support.
    let padding = theme.section_padding;
    let content_area = Rect {
        x: area.x + 1 + padding.left,
        y: area.y + 1 + padding.top,
        width: area.width.saturating_sub(2 + padding.left + padding.right),
        height: area.height.saturating_sub(2 + padding.top + padding.bottom),
    };
    let display_texts: Vec<String> = lines
        .iter()
        .map(|line| spans_to_text(&line.spans))
        .collect();
    let hover_texts: Vec<String> = display_texts
        .iter()
        .enumerate()
        .map(|(idx, display)| {
            full_texts
                .get(idx)
                .filter(|text| !text.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| display.clone())
        })
        .collect();
    let rows = sidebar_hover_rows(content_area, &display_texts, &hover_texts, &row_actions);
    app.sidebar_hover.sections.push(SidebarHoverSection {
        content_area,
        lines: hover_texts,
        rows,
    });
    // Truncate the panel title so it always fits within the section width
    // even after a resize. The title occupies up to 4 chars of border chrome
    // (two spaces + one space on each side), so the max title length is
    // area.width.saturating_sub(4) when borders are enabled.
    let max_title_width = area.width.saturating_sub(4).max(1) as usize;
    let display_title = truncate_line_to_width(title, max_title_width);

    // Constrain lines to the visible section area so a Paragraph wrap
    // overflow can't write cells outside the Block bounds (#400). The
    // border + padding consume 2 rows; budget the rest for content.
    let visible_content_rows = area
        .height
        .saturating_sub(2) // top + bottom border
        .saturating_sub(theme.section_padding.top + theme.section_padding.bottom)
        as usize;
    let lines: Vec<Line<'static>> =
        if lines.len() > visible_content_rows && visible_content_rows > 0 {
            lines.into_iter().take(visible_content_rows).collect()
        } else {
            lines
        };

    let section = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(Line::from(vec![Span::styled(
                format!(" {display_title} "),
                Style::default().fg(theme.section_title_color).bold(),
            )]))
            .borders(theme.section_borders)
            .border_type(theme.section_border_type)
            .border_style(Style::default().fg(theme.section_border_color))
            .style(Style::default().bg(theme.section_bg))
            .padding(theme.section_padding),
    );

    f.render_widget(section, area);
}

pub(crate) fn sidebar_hover_rows(
    content_area: Rect,
    display_texts: &[String],
    hover_texts: &[String],
    row_actions: &[Option<String>],
) -> Vec<SidebarHoverRow> {
    display_texts
        .iter()
        .zip(hover_texts.iter())
        .enumerate()
        .map(|(idx, (display_text, full_text))| {
            let row_y = content_area.y.saturating_add(idx as u16);
            let display_width = unicode_width::UnicodeWidthStr::width(display_text.as_str());
            let full_width = unicode_width::UnicodeWidthStr::width(full_text.as_str());
            let click_action = row_actions.get(idx).and_then(|a| a.clone());
            let stop_action = display_text
                .ends_with(TASK_STOP_TARGET_LABEL)
                .then(|| row_actions.get(idx + 1).and_then(|a| a.clone()))
                .flatten()
                .filter(|action| action.contains(" cancel "));
            let stop_target_width = unicode_width::UnicodeWidthStr::width(TASK_STOP_TARGET_LABEL);
            let (stop_zone_start_col, stop_zone_end_col) =
                if stop_action.is_some() && display_width >= stop_target_width {
                    let visible_width = display_width.min(content_area.width as usize);
                    let start = content_area.x.saturating_add(
                        visible_width
                            .saturating_sub(stop_target_width)
                            .min(u16::MAX as usize) as u16,
                    );
                    let end = start.saturating_add(stop_target_width as u16);
                    (Some(start), Some(end))
                } else {
                    (None, None)
                };
            SidebarHoverRow {
                row_y,
                display_text: display_text.clone(),
                full_text: full_text.clone(),
                detail: None,
                is_truncated: display_width > content_area.width as usize
                    || full_width > content_area.width as usize
                    || display_text != full_text,
                click_action,
                stop_action,
                stop_zone_start_col,
                stop_zone_end_col,
            }
        })
        .collect()
}
