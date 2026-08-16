//! Sidebar sub-panel rendering (extracted from sidebar.rs for #647).

use std::time::Duration;

use crate::tui::sidebar::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};

use crate::palette;

use crate::tui::app::{App, SidebarFocus, TaskPanelEntry, TaskPanelEntryKind};
use crate::tui::history::{
    GenericToolCell, HistoryCell, ToolCell, ToolStatus, summarize_tool_output,
};
use crate::tui::ui_text::{concise_shell_command_label, truncate_line_to_width};

pub(crate) fn work_panel_empty_hint(content_width: usize) -> String {
    truncate_line_to_width("No active work", content_width)
}

pub(crate) fn render_sidebar_work(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let usable_rows = area.height.saturating_sub(3) as usize;
    let summary = sidebar_work_summary(app);
    let lines = work_panel_lines(
        &summary,
        content_width.max(1),
        usable_rows,
        app.ui_theme.mode,
        &app.ui_theme,
    );

    let full_texts = work_panel_hover_texts(&summary, content_width.max(1), usable_rows);
    render_sidebar_section(f, area, "To-do", lines, full_texts, Vec::new(), app);
}

/// Click actions for one background job row pair (#3028).
///
/// Returns `(show, detail)` where `show` opens the job and `detail` cancels
/// it while it is still running (finished jobs make the detail row a second
/// show target instead — cancel would only error). `shell_*` ids belong to
/// the shell job manager and route through `/jobs`; everything else routes
/// through `/task`.
pub(crate) fn background_task_click_actions(task: &TaskPanelEntry) -> (String, String) {
    let namespace = if task.id.starts_with("shell_") {
        "jobs"
    } else {
        "task"
    };
    let show = format!("/{namespace} show {}", task.id);
    let detail = if matches!(task.status.as_str(), "running" | "queued") {
        format!("/{namespace} cancel {}", task.id)
    } else {
        show.clone()
    };
    (show, detail)
}

pub(crate) fn background_task_has_stop_target(task: &TaskPanelEntry) -> bool {
    matches!(task.status.as_str(), "running" | "queued")
}

pub(crate) fn label_with_stop_target(label: &str, content_width: usize) -> String {
    if content_width == 0 {
        return String::new();
    }
    let suffix_width = unicode_width::UnicodeWidthStr::width(TASK_STOP_TARGET_SUFFIX);
    if content_width <= suffix_width {
        return truncate_line_to_width(TASK_STOP_TARGET_LABEL, content_width);
    }
    let base = truncate_line_to_width(label, content_width.saturating_sub(suffix_width));
    format!("{base}{TASK_STOP_TARGET_SUFFIX}")
}

pub(crate) fn render_sidebar_tasks(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let usable_rows = area.height.saturating_sub(3) as usize;
    let (lines, row_actions) = task_panel_rows(app, content_width.max(1), usable_rows.max(1));

    let full_texts = task_panel_hover_texts(app, usable_rows.max(1));
    render_sidebar_section(f, area, "Tasks", lines, full_texts, row_actions, app);
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarToolRow {
    name: String,
    status: ToolStatus,
    summary: String,
    duration_ms: Option<u64>,
}

/// Build the Tasks panel lines together with a parallel per-line click-action
/// vector (#3028). Producing both in a single pass keeps the action indices
/// aligned with the rendered lines no matter how the layout evolves.
pub(crate) fn task_panel_rows(
    app: &App,
    content_width: usize,
    max_rows: usize,
) -> (Vec<Line<'static>>, Vec<Option<String>>) {
    let theme = &app.ui_theme;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));
    let mut actions: Vec<Option<String>> = Vec::with_capacity(max_rows.max(4));
    let explicit_tasks_focus = app.sidebar_focus == SidebarFocus::Tasks;

    if explicit_tasks_focus && app.runtime_turn_id.is_some() {
        let status = app
            .runtime_turn_status
            .as_deref()
            .unwrap_or("unknown")
            .to_string();
        // #3030: Use a stable turn number ("Turn 1") instead of the raw
        // UUID prefix.  The full UUID is preserved in the hover text
        // (task_panel_hover_texts) for inspection.
        let turn_label = if app.turn_counter > 0 {
            format!("Turn {} ({status})", app.turn_counter)
        } else {
            format!("Current turn ({status})")
        };
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&turn_label, content_width.max(1)),
            Style::default().fg(theme.accent_primary),
        )));
    }

    let active_rows = active_tool_rows(app);
    if explicit_tasks_focus && !active_rows.is_empty() && lines.len() < max_rows {
        push_sidebar_label_theme(&mut lines, "Live tools", theme);
        push_tool_rows(&mut lines, &active_rows, content_width, max_rows, theme);
    }

    let reasoning_rows = reasoning_task_rows(app);
    if explicit_tasks_focus && !reasoning_rows.is_empty() && lines.len() < max_rows {
        push_sidebar_label_theme(&mut lines, "Model reasoning", theme);
        push_reasoning_rows(&mut lines, &reasoning_rows, content_width, max_rows, theme);
    }

    let background_rows = background_task_rows(
        app,
        if explicit_tasks_focus {
            &active_rows
        } else {
            &[]
        },
    );
    // Lines pushed so far (turn label, Live tools header, live tool rows)
    // are not clickable — backfill their action slots.
    actions.resize(lines.len(), None);
    if !background_rows.is_empty() && lines.len() < max_rows {
        let running = background_rows
            .iter()
            .filter(|task| task.status == "running")
            .count();
        let done = background_rows.len().saturating_sub(running);
        let label = if running == 0 {
            format!("Bash jobs: {done} completed")
        } else if done == 0 {
            format!("Bash jobs: {running} running")
        } else {
            format!("Bash jobs: {running} running, {done} completed")
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(theme.accent_primary).bold(),
        )));
        actions.push(None);

        let max_items = max_rows.saturating_sub(lines.len());
        for task in background_rows.iter().take(max_items) {
            let color = if task.stale && task.status == "running" {
                theme.warning
            } else {
                match task.status.as_str() {
                    "queued" => theme.text_muted,
                    "running" => theme.warning,
                    "completed" => theme.success,
                    "failed" => theme.error_fg,
                    "canceled" => theme.text_dim,
                    _ => theme.text_muted,
                }
            };
            let duration = task
                .duration_ms
                .map(format_duration_ms)
                .unwrap_or_else(|| "-".to_string());
            let (label, detail) = background_task_labels(task, &duration);
            let label = background_task_spinner_prefix(task)
                .map(|prefix| format!("{prefix} {label}"))
                .unwrap_or(label);
            let (show_action, detail_action) = background_task_click_actions(task);
            let label = if background_task_has_stop_target(task) {
                label_with_stop_target(&label, content_width.max(1))
            } else {
                truncate_line_to_width(&label, content_width.max(1))
            };
            lines.push(Line::from(Span::styled(label, Style::default().fg(color))));
            actions.push(Some(show_action));
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    truncate_line_to_width(&detail, content_width.saturating_sub(2).max(1))
                ),
                Style::default().fg(theme.text_dim),
            )));
            actions.push(Some(detail_action));
        }

        if lines.len() < max_rows {
            let stale_running_shells = background_rows
                .iter()
                .filter(|task| {
                    task.id.starts_with("shell_") && task.status == "running" && task.stale
                })
                .collect::<Vec<_>>();
            let any_running_shell = background_rows
                .iter()
                .any(|task| task.id.starts_with("shell_") && task.status == "running");
            let hint_action = if stale_running_shells.len() == 1 {
                Some((
                    "Ctrl+X -> cancel stale job".to_string(),
                    format!("/jobs cancel {}", stale_running_shells[0].id),
                ))
            } else if any_running_shell {
                Some((
                    "Ctrl+X -> /jobs cancel-all".to_string(),
                    "/jobs cancel-all".to_string(),
                ))
            } else {
                None
            };
            if let Some((hint, action)) = hint_action {
                lines.push(Line::from(Span::styled(
                    truncate_line_to_width(&hint, content_width.max(1)),
                    Style::default()
                        .fg(theme.text_muted)
                        .add_modifier(ratatui::style::Modifier::ITALIC),
                )));
                actions.push(Some(action));
            }
        }
    }

    if explicit_tasks_focus && lines.len() < max_rows {
        let recent_rows = recent_tool_rows(app, 4);
        if !recent_rows.is_empty() {
            push_sidebar_label_theme(&mut lines, "Recent tools", theme);
            push_tool_rows(&mut lines, &recent_rows, content_width, max_rows, theme);
        }
    }

    // Yank hint: surface the keyboard path for copying the focused task/turn ID.
    if lines.len() + 1 < max_rows
        && app.runtime_turn_id.is_some()
        && app.sidebar_focus == SidebarFocus::Tasks
    {
        lines.push(Line::from(Span::styled(
            "y → copy turn id  ·  Y → copy full status",
            Style::default()
                .fg(theme.text_dim)
                .add_modifier(ratatui::style::Modifier::ITALIC),
        )));
    }

    if lines.is_empty()
        || (lines.len() == 1
            && app.runtime_turn_id.is_some()
            && active_rows.is_empty()
            && reasoning_rows.is_empty()
            && background_rows.is_empty())
    {
        lines.push(Line::from(Span::styled(
            "No live tools or background jobs",
            Style::default().fg(theme.text_muted),
        )));
    }

    // Backfill action slots for the trailing non-clickable lines (Recent
    // tools, yank hint, empty-state notice).
    actions.resize(lines.len(), None);
    (lines, actions)
}

pub(crate) fn task_panel_hover_texts(app: &App, max_rows: usize) -> Vec<String> {
    let mut texts = Vec::with_capacity(max_rows.max(4));
    let explicit_tasks_focus = app.sidebar_focus == SidebarFocus::Tasks;

    if explicit_tasks_focus && let Some(turn_id) = app.runtime_turn_id.as_ref() {
        let status = app.runtime_turn_status.as_deref().unwrap_or("unknown");
        texts.push(format!("turn {turn_id} ({status})"));
    }

    let active_rows = active_tool_rows(app);
    if explicit_tasks_focus && !active_rows.is_empty() && texts.len() < max_rows {
        texts.push("Live tools".to_string());
        push_tool_row_hover_texts(&mut texts, &active_rows, max_rows);
    }

    let reasoning_rows = reasoning_task_rows(app);
    if explicit_tasks_focus && !reasoning_rows.is_empty() && texts.len() < max_rows {
        texts.push("Model reasoning".to_string());
        push_reasoning_row_hover_texts(&mut texts, &reasoning_rows, max_rows);
    }

    let background_rows = background_task_rows(
        app,
        if explicit_tasks_focus {
            &active_rows
        } else {
            &[]
        },
    );
    if !background_rows.is_empty() && texts.len() < max_rows {
        let running = background_rows
            .iter()
            .filter(|task| task.status == "running")
            .count();
        let done = background_rows.len().saturating_sub(running);
        let label = if running == 0 {
            format!("Bash jobs: {done} completed")
        } else if done == 0 {
            format!("Bash jobs: {running} running")
        } else {
            format!("Bash jobs: {running} running, {done} completed")
        };
        texts.push(label);

        let max_items = max_rows.saturating_sub(texts.len());
        for task in background_rows.iter().take(max_items) {
            let duration = task
                .duration_ms
                .map(format_duration_ms)
                .unwrap_or_else(|| "-".to_string());
            let (label, detail) = background_task_labels(task, &duration);
            let label = background_task_spinner_prefix(task)
                .map(|prefix| format!("{prefix} {label}"))
                .unwrap_or(label);
            texts.push(label);
            if texts.len() >= max_rows {
                break;
            }
            texts.push(format!("  {detail}"));
        }

        if texts.len() < max_rows {
            let stale_running_shells = background_rows
                .iter()
                .filter(|task| {
                    task.id.starts_with("shell_") && task.status == "running" && task.stale
                })
                .count();
            let any_running_shell = background_rows
                .iter()
                .any(|task| task.id.starts_with("shell_") && task.status == "running");
            if stale_running_shells == 1 {
                texts.push("Ctrl+X -> cancel stale job".to_string());
            } else if any_running_shell {
                texts.push("Ctrl+X -> /jobs cancel-all".to_string());
            }
        }
    }

    if explicit_tasks_focus && texts.len() < max_rows {
        let recent_rows = recent_tool_rows(app, 4);
        if !recent_rows.is_empty() {
            texts.push("Recent tools".to_string());
            push_tool_row_hover_texts(&mut texts, &recent_rows, max_rows);
        }
    }

    if texts.len() + 1 < max_rows
        && app.runtime_turn_id.is_some()
        && app.sidebar_focus == SidebarFocus::Tasks
    {
        texts.push("y -> copy turn id  ·  Y -> copy full status".to_string());
    }

    if texts.is_empty()
        || (texts.len() == 1
            && app.runtime_turn_id.is_some()
            && active_rows.is_empty()
            && reasoning_rows.is_empty()
            && background_rows.is_empty())
    {
        texts.push("No live tools or background jobs".to_string());
    }

    texts
}

pub(crate) fn push_sidebar_label_theme(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    theme: &palette::UiTheme,
) {
    lines.push(Line::from(Span::styled(
        label.to_string(),
        Style::default().fg(theme.accent_primary).bold(),
    )));
}

pub(crate) fn push_tool_row_hover_texts(
    texts: &mut Vec<String>,
    rows: &[SidebarToolRow],
    max_rows: usize,
) {
    for row in rows {
        if texts.len() >= max_rows {
            break;
        }
        let (marker, _) = tool_status_marker(row.status, &palette::UI_THEME);
        let label = if let Some(duration_ms) = row.duration_ms {
            format!("{marker} {} {}", row.name, format_duration_ms(duration_ms))
        } else {
            format!("{marker} {}", row.name)
        };
        texts.push(label);
        if !row.summary.trim().is_empty() && texts.len() < max_rows {
            texts.push(format!("  {}", row.summary));
        }
    }
}

pub(crate) fn push_reasoning_rows(
    lines: &mut Vec<Line<'static>>,
    rows: &[TaskPanelEntry],
    content_width: usize,
    max_rows: usize,
    theme: &palette::UiTheme,
) {
    for task in rows {
        if lines.len() >= max_rows {
            break;
        }
        let color = match task.status.as_str() {
            "running" => theme.warning,
            "completed" => theme.success,
            "failed" => theme.error_fg,
            _ => theme.text_muted,
        };
        let duration = task
            .duration_ms
            .map(format_duration_ms)
            .unwrap_or_else(|| "-".to_string());
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(
                &format!("thinking {} {duration}", task.status),
                content_width,
            ),
            Style::default().fg(color),
        )));
        if !task.prompt_summary.trim().is_empty() && lines.len() < max_rows {
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    truncate_line_to_width(
                        &task.prompt_summary,
                        content_width.saturating_sub(2).max(1)
                    )
                ),
                Style::default().fg(theme.text_dim),
            )));
        }
    }
}

pub(crate) fn push_reasoning_row_hover_texts(
    texts: &mut Vec<String>,
    rows: &[TaskPanelEntry],
    max_rows: usize,
) {
    for task in rows {
        if texts.len() >= max_rows {
            break;
        }
        let duration = task
            .duration_ms
            .map(format_duration_ms)
            .unwrap_or_else(|| "-".to_string());
        texts.push(format!("thinking {} {duration}", task.status));
        if !task.prompt_summary.trim().is_empty() && texts.len() < max_rows {
            texts.push(format!("  {}", task.prompt_summary));
        }
    }
}

pub(crate) fn background_task_labels(task: &TaskPanelEntry, duration: &str) -> (String, String) {
    let stale_label = stale_no_output_label(task);
    let owner_label = task
        .owner_agent_name
        .as_deref()
        .or(task.owner_agent_id.as_deref())
        .filter(|owner| !owner.trim().is_empty())
        .map(|owner| format!("by {owner}"))
        .unwrap_or_default();
    let status = stale_label
        .as_ref()
        .map(|label| format!("{} ({label})", task.status))
        .unwrap_or_else(|| task.status.clone());

    if let Some(command) = task.prompt_summary.strip_prefix("shell: ") {
        let command = concise_shell_command_label(command, 96);
        return (
            format!("Bash {status} {command} {duration}"),
            compact_join([
                format!("{} \u{00B7} Bash", task.id),
                owner_label,
                stale_label.unwrap_or_default(),
            ]),
        );
    }

    (
        format!(
            "{} {} {}",
            truncate_line_to_width(&task.id, 10),
            status,
            duration
        ),
        compact_join([
            task.prompt_summary.clone(),
            owner_label,
            stale_label.unwrap_or_default(),
        ]),
    )
}

pub(crate) fn background_task_is_live(task: &TaskPanelEntry) -> bool {
    task.kind == TaskPanelEntryKind::Background
        && matches!(task.status.as_str(), "queued" | "running")
}

const BRAILLE_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const BRAILLE_SPINNER_FRAME_MS: u64 = 100;

pub(crate) fn background_task_spinner_prefix(task: &TaskPanelEntry) -> Option<&'static str> {
    if task.status != "running" {
        return None;
    }
    let frame = task.duration_ms.unwrap_or_default() / BRAILLE_SPINNER_FRAME_MS;
    Some(BRAILLE_SPINNER_FRAMES[frame as usize % BRAILLE_SPINNER_FRAMES.len()])
}

pub(crate) fn stale_no_output_label(task: &TaskPanelEntry) -> Option<String> {
    if !(task.stale && task.status == "running") {
        return None;
    }
    task.elapsed_since_output_ms
        .map(format_duration_ms)
        .map(|duration| format!("stale, no output {duration}"))
        .or_else(|| Some("stale, no output".to_string()))
}

pub(crate) fn active_tool_rows(app: &App) -> Vec<SidebarToolRow> {
    let Some(active) = app.active_cell.as_ref() else {
        return Vec::new();
    };
    let mut rows: Vec<SidebarToolRow> = Vec::new();
    let mut stale_running: Vec<SidebarToolRow> = Vec::new();
    for (entry_idx, cell) in active.entries().iter().enumerate() {
        let Some(row) = sidebar_tool_row_from_cell(cell) else {
            continue;
        };
        match active_tool_row_visibility(app, entry_idx, &row) {
            ActiveToolRowVisibility::Visible => rows.push(row),
            ActiveToolRowVisibility::StaleRunning => stale_running.push(row),
            ActiveToolRowVisibility::Hidden => {}
        }
    }
    if !stale_running.is_empty() {
        rows.push(collapsed_stale_running_row(stale_running));
    }
    editorial_tool_rows(rows, usize::MAX, ToolRowOrder::OldestFirst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveToolRowVisibility {
    Visible,
    StaleRunning,
    Hidden,
}

pub(crate) fn active_tool_row_visibility(
    app: &App,
    entry_idx: usize,
    row: &SidebarToolRow,
) -> ActiveToolRowVisibility {
    if row.status == ToolStatus::Running {
        return if row
            .duration_ms
            .is_some_and(|ms| ms >= duration_ms(ACTIVE_TOOL_STALE_RUNNING_ROW_TTL))
        {
            ActiveToolRowVisibility::StaleRunning
        } else {
            ActiveToolRowVisibility::Visible
        };
    }

    let Some(completed_at) = app.active_tool_entry_completed_at.get(&entry_idx) else {
        return ActiveToolRowVisibility::Hidden;
    };
    if completed_at.elapsed() <= ACTIVE_TOOL_COMPLETED_ROW_TTL {
        ActiveToolRowVisibility::Visible
    } else {
        ActiveToolRowVisibility::Hidden
    }
}

pub(crate) fn collapsed_stale_running_row(rows: Vec<SidebarToolRow>) -> SidebarToolRow {
    let count = rows.len();
    let oldest_ms = rows
        .iter()
        .filter_map(|row| row.duration_ms)
        .max()
        .unwrap_or_default();
    let first_summary = rows
        .iter()
        .find_map(|row| (!row.summary.trim().is_empty()).then(|| row.summary.clone()))
        .unwrap_or_else(|| "open Activity Detail".to_string());
    SidebarToolRow {
        name: if count == 1 {
            "run".to_string()
        } else {
            format!("run x{count}")
        },
        status: ToolStatus::Running,
        summary: format!("long-running · {first_summary}"),
        duration_ms: (oldest_ms > 0).then_some(oldest_ms),
    }
}

pub(crate) fn recent_tool_rows(app: &App, limit: usize) -> Vec<SidebarToolRow> {
    let rows: Vec<SidebarToolRow> = app
        .history
        .iter()
        .rev()
        .filter_map(sidebar_tool_row_from_cell)
        .take(RECENT_TOOL_SCAN_LIMIT)
        .collect();
    editorial_tool_rows(rows, limit, ToolRowOrder::NewestFirst)
}

pub(crate) fn push_tool_rows(
    lines: &mut Vec<Line<'static>>,
    rows: &[SidebarToolRow],
    content_width: usize,
    max_rows: usize,
    theme: &palette::UiTheme,
) {
    for row in rows {
        if lines.len() >= max_rows {
            break;
        }
        let (marker, color) = tool_status_marker(row.status, theme);
        let label = if let Some(duration_ms) = row.duration_ms {
            format!("{marker} {} {}", row.name, format_duration_ms(duration_ms))
        } else {
            format!("{marker} {}", row.name)
        };
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&label, content_width),
            Style::default().fg(color),
        )));
        if !row.summary.trim().is_empty() && lines.len() < max_rows {
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    truncate_line_to_width(&row.summary, content_width.saturating_sub(2).max(1))
                ),
                Style::default().fg(theme.text_dim),
            )));
        }
    }
}

pub(crate) fn sidebar_tool_row_from_cell(cell: &HistoryCell) -> Option<SidebarToolRow> {
    let HistoryCell::Tool(tool) = cell else {
        return None;
    };
    match tool {
        ToolCell::Exec(exec) => Some(SidebarToolRow {
            name: concise_shell_command_label(&exec.command, 48),
            status: shell_status_for_sidebar(
                &exec.command,
                exec.status,
                exec.output_summary.as_deref(),
                exec.output.as_deref(),
            ),
            summary: shell_summary_for_sidebar(
                &exec.command,
                exec.status,
                exec.output_summary.as_deref(),
                exec.output.as_deref(),
            ),
            duration_ms: exec.duration_ms.or_else(|| {
                (exec.status == ToolStatus::Running).then(|| {
                    u64::try_from(
                        exec.started_at
                            .map(|started| started.elapsed().as_millis())
                            .unwrap_or_default(),
                    )
                    .unwrap_or(u64::MAX)
                })
            }),
        }),
        ToolCell::Exploring(explore) => {
            let running = explore
                .entries
                .iter()
                .filter(|entry| entry.status == ToolStatus::Running)
                .count();
            let status = if running > 0 {
                ToolStatus::Running
            } else if explore
                .entries
                .iter()
                .any(|entry| entry.status == ToolStatus::Failed)
            {
                ToolStatus::Failed
            } else {
                ToolStatus::Success
            };
            let first = explore.entries.first().map(|entry| entry.label.as_str());
            Some(SidebarToolRow {
                name: "workspace".to_string(),
                status,
                summary: compact_join([
                    format!("{} item(s), {running} running", explore.entries.len()),
                    first.unwrap_or_default().to_string(),
                ]),
                duration_ms: None,
            })
        }
        ToolCell::PlanUpdate(plan) => Some(SidebarToolRow {
            name: "update_plan".to_string(),
            status: plan.status,
            summary: plan
                .snapshot
                .objective
                .as_deref()
                .or(plan.snapshot.title.as_deref())
                .or(plan.snapshot.explanation.as_deref())
                .or(plan.snapshot.recommended_approach.as_deref())
                .or_else(|| plan.snapshot.items.first().map(|step| step.step.as_str()))
                .unwrap_or("")
                .to_string(),
            duration_ms: None,
        }),
        ToolCell::PatchSummary(patch) => Some(SidebarToolRow {
            name: "patch".to_string(),
            status: patch.status,
            summary: compact_join([patch.path.clone(), patch.summary.clone()]),
            duration_ms: None,
        }),
        ToolCell::Review(review) => Some(SidebarToolRow {
            name: "review".to_string(),
            status: review.status,
            summary: review.target.clone(),
            duration_ms: None,
        }),
        ToolCell::DiffPreview(diff) => Some(SidebarToolRow {
            name: "diff".to_string(),
            status: ToolStatus::Success,
            summary: diff.title.clone(),
            duration_ms: None,
        }),
        ToolCell::Mcp(mcp) => Some(SidebarToolRow {
            name: mcp.tool.clone(),
            status: mcp.status,
            summary: mcp
                .content
                .as_deref()
                .map(summarize_tool_output)
                .unwrap_or_default(),
            duration_ms: None,
        }),
        ToolCell::ViewImage(image) => Some(SidebarToolRow {
            name: "image".to_string(),
            status: ToolStatus::Success,
            summary: image.path.display().to_string(),
            duration_ms: None,
        }),
        ToolCell::WebSearch(search) => Some(SidebarToolRow {
            name: "web_search".to_string(),
            status: search.status,
            summary: compact_join([
                search.query.clone(),
                search.summary.clone().unwrap_or_default(),
            ]),
            duration_ms: None,
        }),
        ToolCell::Generic(generic) => Some(SidebarToolRow {
            name: friendly_generic_tool_name(&generic.name).to_string(),
            status: generic.status,
            summary: generic_tool_sidebar_summary(generic),
            duration_ms: None,
        }),
    }
}

pub(crate) fn shell_status_for_sidebar(
    command: &str,
    status: ToolStatus,
    output_summary: Option<&str>,
    output: Option<&str>,
) -> ToolStatus {
    if status == ToolStatus::Failed && looks_like_pending_ci(command, output_summary, output) {
        ToolStatus::Running
    } else {
        status
    }
}

pub(crate) fn shell_summary_for_sidebar(
    command: &str,
    status: ToolStatus,
    output_summary: Option<&str>,
    output: Option<&str>,
) -> String {
    if status == ToolStatus::Failed && looks_like_pending_ci(command, output_summary, output) {
        return format!(
            "Waiting for CI \u{00B7} {}",
            crate::tui::key_shortcuts::tool_details_shortcut_action_hint("details")
        );
    }

    let summary = compact_join([
        output_summary.unwrap_or_default().to_string(),
        output
            .map(first_nonempty_line)
            .unwrap_or_default()
            .to_string(),
    ]);
    if status == ToolStatus::Failed {
        failure_summary_with_hint(&summary)
    } else {
        summary
    }
}

pub(crate) fn looks_like_pending_ci(
    command: &str,
    output_summary: Option<&str>,
    output: Option<&str>,
) -> bool {
    let command_label = concise_shell_command_label(command, 80).to_ascii_lowercase();
    if !command_label.starts_with("gh pr checks") && !command_label.starts_with("gh run watch") {
        return false;
    }

    let text = compact_join([
        output_summary.unwrap_or_default().to_string(),
        output.unwrap_or_default().to_string(),
    ])
    .to_ascii_lowercase();
    if text.is_empty() {
        return false;
    }
    let pending = ["pending", "queued", "in_progress", "in progress", "waiting"]
        .iter()
        .any(|needle| text.contains(needle));
    let hard_failure = ["failed", "failure", "error", "cancelled", "canceled"]
        .iter()
        .any(|needle| text.contains(needle));
    pending && !hard_failure
}

pub(crate) fn failure_summary_with_hint(summary: &str) -> String {
    let hint = crate::tui::key_shortcuts::tool_details_shortcut_action_hint("details");
    if summary.trim().is_empty() {
        hint
    } else if summary.contains(&hint) {
        summary.to_string()
    } else {
        format!("{summary} \u{00B7} {hint}")
    }
}

pub(crate) fn friendly_generic_tool_name(name: &str) -> &str {
    match name {
        "task_shell_start" => "start Bash",
        "task_shell_wait" => "wait Bash",
        "task_shell_write" => "write Bash",
        _ => name,
    }
}

pub(crate) fn generic_tool_sidebar_summary(generic: &GenericToolCell) -> String {
    match generic.name.as_str() {
        "task_shell_start" => compact_join([
            generic.input_summary.clone().unwrap_or_default(),
            "background Bash".to_string(),
        ]),
        "task_shell_wait" => compact_join([
            generic.input_summary.clone().unwrap_or_default(),
            generic.output_summary.clone().unwrap_or_default(),
        ]),
        _ => compact_join([
            generic.input_summary.clone().unwrap_or_default(),
            generic.output_summary.clone().unwrap_or_default(),
            generic
                .output
                .as_deref()
                .map(summarize_tool_output)
                .unwrap_or_default(),
        ]),
    }
}

pub(crate) fn background_task_rows(
    app: &App,
    active_rows: &[SidebarToolRow],
) -> Vec<TaskPanelEntry> {
    let mut rows: Vec<TaskPanelEntry> = app
        .task_panel
        .iter()
        .filter(|task| task.kind == TaskPanelEntryKind::Background)
        .filter(|task| !background_task_duplicates_live_tool(task, active_rows))
        .cloned()
        .collect();
    rows.sort_by_key(|task| (task_status_rank(task.status.as_str()), task.id.clone()));
    rows
}

pub(crate) fn reasoning_task_rows(app: &App) -> Vec<TaskPanelEntry> {
    let mut rows: Vec<TaskPanelEntry> = app
        .task_panel
        .iter()
        .filter(|task| task.kind == TaskPanelEntryKind::ModelReasoning)
        .cloned()
        .collect();
    rows.sort_by_key(|task| (task_status_rank(task.status.as_str()), task.id.clone()));
    rows
}

pub(crate) fn background_task_duplicates_live_tool(
    task: &TaskPanelEntry,
    active_rows: &[SidebarToolRow],
) -> bool {
    if task.status != "running" {
        return false;
    }

    if task.id.starts_with("rlm-") || task.prompt_summary.starts_with("RLM: ") {
        return active_rows
            .iter()
            .any(|row| row.status == ToolStatus::Running && row.name.starts_with("rlm_"));
    }

    let Some(command) = task.prompt_summary.strip_prefix("shell: ") else {
        return false;
    };
    let command = normalize_activity_text(command);
    !command.is_empty()
        && active_rows.iter().any(|row| {
            row.status == ToolStatus::Running
                && normalize_activity_text(&format!("{} {}", row.name, row.summary))
                    .contains(&command)
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolRowOrder {
    OldestFirst,
    NewestFirst,
}

pub(crate) fn editorial_tool_rows(
    rows: Vec<SidebarToolRow>,
    limit: usize,
    order_mode: ToolRowOrder,
) -> Vec<SidebarToolRow> {
    #[derive(Clone)]
    struct Candidate {
        rank: u8,
        order: usize,
        row: SidebarToolRow,
    }

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut low_value_groups: Vec<(usize, SidebarToolRow, usize)> = Vec::new();
    let mut ci_poll_groups: Vec<(usize, SidebarToolRow, usize)> = Vec::new();
    let mut shell_wait_groups: Vec<(usize, SidebarToolRow, usize, String)> = Vec::new();
    let mut seen_success: Vec<String> = Vec::new();
    let mut seen_success_tool_names: Vec<String> = Vec::new();
    let mut seen_failures: Vec<String> = Vec::new();
    let mut visible_failure_count: usize = 0;
    const MAX_VISIBLE_FAILURES: usize = 2;

    for (order, mut row) in rows.into_iter().enumerate() {
        if row.status == ToolStatus::Failed {
            // Deduplicate failures for the same tool name: keep only the most
            // recent failure per tool. Fixes #1884 — stale failures from
            // tools that have since succeeded no longer crowd the sidebar.
            let fail_key = row.name.trim().to_ascii_lowercase();
            if order_mode == ToolRowOrder::NewestFirst
                && seen_success_tool_names.contains(&fail_key)
            {
                continue;
            }
            if seen_failures.contains(&fail_key) {
                continue;
            }
            seen_failures.push(fail_key);
            row.summary = failure_summary_with_hint(&row.summary);
        }

        if is_ci_poll_row(&row) {
            if let Some((_, grouped, count)) = ci_poll_groups
                .iter_mut()
                .find(|(_, grouped, _)| grouped.name == row.name)
            {
                *count += 1;
                if grouped.duration_ms.is_none() {
                    grouped.duration_ms = row.duration_ms;
                }
            } else {
                ci_poll_groups.push((order, row, 1));
            }
            continue;
        }

        if is_shell_wait_poll_row(&row) {
            let key = shell_wait_poll_key(&row);
            if let Some((_, grouped, count, _)) = shell_wait_groups
                .iter_mut()
                .find(|(_, _, _, existing_key)| existing_key == &key)
            {
                *count += 1;
                if !row.summary.trim().is_empty() {
                    grouped.summary = row.summary;
                }
            } else {
                shell_wait_groups.push((order, row, 1, key));
            }
            continue;
        }

        if is_low_value_tool(&row.name) && row.status == ToolStatus::Success {
            if let Some((_, grouped, count)) = low_value_groups
                .iter_mut()
                .find(|(_, grouped, _)| grouped.name == row.name)
            {
                *count += 1;
                if grouped.summary.trim().is_empty() && !row.summary.trim().is_empty() {
                    grouped.summary = row.summary;
                }
            } else {
                low_value_groups.push((order, row, 1));
            }
            continue;
        }

        let key = sidebar_row_identity(&row);
        if row.status == ToolStatus::Success && seen_success.iter().any(|seen| seen == &key) {
            continue;
        }
        if row.status == ToolStatus::Success {
            seen_success.push(key);
            let normalized = row.name.trim().to_ascii_lowercase();
            if !seen_success_tool_names.contains(&normalized) {
                seen_success_tool_names.push(normalized.clone());
            }

            // Active rows are oldest-first, so a success means any candidate
            // failure for the same tool is stale. Recent history rows are
            // newest-first; in that path the success is older than any
            // already-seen failure and must not remove it.
            if order_mode == ToolRowOrder::OldestFirst {
                let mut removed_visible_failures = 0usize;
                let mut removed_any_failure = false;
                candidates.retain(|c| {
                    let remove = c.row.status == ToolStatus::Failed
                        && c.row.name.trim().eq_ignore_ascii_case(&normalized);
                    if remove {
                        removed_any_failure = true;
                        if c.rank == 0 {
                            removed_visible_failures += 1;
                        }
                    }
                    !remove
                });
                if removed_any_failure {
                    seen_failures.retain(|seen| seen != &normalized);
                    visible_failure_count =
                        visible_failure_count.saturating_sub(removed_visible_failures);
                }
            }
        }

        // Cap visible failures at MAX_VISIBLE_FAILURES. Excess failures
        // get demoted to rank 3 so they don't crowd the top of the
        // sidebar. (#1884)
        let rank = if row.status == ToolStatus::Failed {
            if visible_failure_count >= MAX_VISIBLE_FAILURES {
                3
            } else {
                visible_failure_count += 1;
                0
            }
        } else {
            tool_row_rank(&row)
        };

        candidates.push(Candidate { rank, order, row });
    }

    for (order, mut row, count) in ci_poll_groups {
        if count > 1 {
            let command = row.name.clone();
            row.name = "Waiting for CI".to_string();
            row.summary = format!(
                "{command} \u{00B7} {count} polls collapsed \u{00B7} {}",
                crate::tui::key_shortcuts::tool_details_shortcut_action_hint("details")
            );
            row.status = ToolStatus::Running;
        }
        candidates.push(Candidate {
            rank: tool_row_rank(&row),
            order,
            row,
        });
    }

    for (order, mut row, count, key) in shell_wait_groups {
        if count > 1 {
            row.summary = compact_join([
                format!("{key} \u{00B7} {count} waits collapsed"),
                row.summary.clone(),
            ]);
        }
        candidates.push(Candidate {
            rank: tool_row_rank(&row),
            order,
            row,
        });
    }

    for (order, mut row, count) in low_value_groups {
        if count > 1 {
            row.name = format!("{} x{count}", row.name);
            if !row.summary.trim().is_empty() {
                row.summary = format!("latest: {}", row.summary);
            }
        }
        candidates.push(Candidate {
            rank: tool_row_rank(&row).saturating_add(1),
            order,
            row,
        });
    }

    candidates.sort_by_key(|candidate| (candidate.rank, candidate.order));
    candidates
        .into_iter()
        .take(limit)
        .map(|candidate| candidate.row)
        .collect()
}

pub(crate) fn sidebar_row_identity(row: &SidebarToolRow) -> String {
    format!(
        "{}\n{}",
        row.name.trim(),
        normalize_activity_text(row.summary.as_str())
    )
}

pub(crate) fn is_ci_poll_row(row: &SidebarToolRow) -> bool {
    row.name.starts_with("gh pr checks") || row.name.starts_with("gh run watch")
}

pub(crate) fn is_shell_wait_poll_row(row: &SidebarToolRow) -> bool {
    row.status == ToolStatus::Running
        && matches!(row.name.as_str(), "wait Bash" | "exec_shell_wait")
}

pub(crate) fn shell_wait_poll_key(row: &SidebarToolRow) -> String {
    const MARKER: &str = "task_id:";
    if let Some((_, rest)) = row.summary.split_once(MARKER) {
        let task_id = rest
            .trim_start()
            .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '\u{00B7}')
            .next()
            .unwrap_or_default()
            .trim();
        if !task_id.is_empty() {
            return task_id.to_string();
        }
    }

    normalize_activity_text(&row.name)
}

pub(crate) fn normalize_activity_text(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    crate::tui::osc8::strip_ansi_into(text, &mut cleaned);
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn tool_row_rank(row: &SidebarToolRow) -> u8 {
    match row.status {
        ToolStatus::Failed => 0,
        // A schema-hydrated deferred tool is not "run done" — it must be
        // retried — so it ranks with active work, not completed successes.
        ToolStatus::Running | ToolStatus::Hydrated => 1,
        ToolStatus::Success if is_low_value_tool(&row.name) => 3,
        ToolStatus::Success => 2,
    }
}

pub(crate) fn task_status_rank(status: &str) -> u8 {
    match status {
        "running" => 0,
        "failed" => 1,
        "queued" => 2,
        "completed" => 3,
        "canceled" => 4,
        _ => 5,
    }
}

pub(crate) fn is_low_value_tool(name: &str) -> bool {
    let base = name.split_whitespace().next().unwrap_or(name);
    matches!(
        base,
        "read_file" | "grep_files" | "file_search" | "find" | "checklist_update"
    )
}

pub(crate) fn compact_join(parts: impl IntoIterator<Item = String>) -> String {
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let part = part.trim();
        if !part.is_empty() && !out.iter().any(|seen| seen == part) {
            out.push(part.to_string());
        }
    }
    out.join(" · ")
}

pub(crate) fn first_nonempty_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
}

pub(crate) fn tool_status_marker(
    status: ToolStatus,
    theme: &palette::UiTheme,
) -> (&'static str, ratatui::style::Color) {
    match status {
        ToolStatus::Running => ("[~]", theme.warning),
        ToolStatus::Success => ("[✓]", theme.success),
        ToolStatus::Hydrated => ("[~]", theme.warning),
        ToolStatus::Failed => ("[!]", theme.error_fg),
    }
}

pub(crate) fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

pub(crate) fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
