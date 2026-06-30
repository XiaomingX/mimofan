//! Sidebar rendering — Pinned / Tasks / Agents / Context panels.
//!
//! Extracted from `tui/ui.rs` (P1.2). The sidebar appears to the right of
//! the chat transcript when the available width allows it. Each section
//! reads from `App` snapshots; mutation lives in the main app loop.

use std::fmt::Write;
use std::time::{Duration, Instant};

use crate::tui::app::HuntVerdict;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Widget,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use crate::deepseek_theme::Theme;
use crate::palette;
use crate::tools::plan::StepStatus;
use crate::tools::subagent::{AgentWorkerStatus, SubAgentStatus, agent_worker_status_name};
use crate::tools::todo::TodoStatus;

use super::app::{
    App, SidebarFocus, SidebarHoverRow, SidebarHoverSection, SidebarHoverState, TaskPanelEntry,
    TaskPanelEntryKind,
};
use super::history::{GenericToolCell, HistoryCell, ToolCell, ToolStatus, summarize_tool_output};
use super::subagent_routing::active_fanout_counts;
use super::ui_text::{concise_shell_command_label, truncate_line_to_width};

/// Tolerance for floating-point cost comparison in the sidebar breakdown.
/// Must be large enough that accumulated f64 error across hundreds of turns
/// does not prematurely hide the session+agents breakdown.
const COST_EQ_TOLERANCE: f64 = 1e-6;
const RECENT_TOOL_SCAN_LIMIT: usize = 24;
const ACTIVE_TOOL_COMPLETED_ROW_TTL: Duration = Duration::from_secs(8);
const ACTIVE_TOOL_STALE_RUNNING_ROW_TTL: Duration = Duration::from_secs(600);
const TASK_STOP_TARGET_LABEL: &str = "[x]";
const TASK_STOP_TARGET_SUFFIX: &str = " [x]";

pub fn render_sidebar(f: &mut Frame, area: Rect, app: &mut App) {
    // Clear hover state at the start of each render
    app.sidebar_hover = SidebarHoverState::default();
    if area.width < 24 || area.height < 8 {
        // Paint a styled block over the area so stale cells from a previous
        // (wider) frame don't persist as bleed-through artifacts (#400).
        Block::default()
            .style(Style::default().bg(app.ui_theme.surface_bg))
            .render(area, f.buffer_mut());
        return;
    }

    match app.sidebar_focus {
        SidebarFocus::Auto => render_sidebar_auto(f, area, app),
        SidebarFocus::Pinned => render_sidebar_pinned(f, area, app),
        SidebarFocus::Tasks => render_sidebar_tasks(f, area, app),
        SidebarFocus::Agents => render_sidebar_subagents(f, area, app),
        SidebarFocus::Context => render_context_panel(f, area, app),
        SidebarFocus::Hidden => Block::default()
            .style(Style::default().bg(app.ui_theme.surface_bg))
            .render(area, f.buffer_mut()),
    }
}

/// Build the Auto-mode panel stack. Empty panels collapse to zero height so
/// non-empty ones get the full sidebar real estate. To-do appears when it has
/// useful content, or as the one quiet empty state when nothing else is active.
fn render_sidebar_auto(f: &mut Frame, area: Rect, app: &mut App) {
    let visible = auto_sidebar_panels(auto_sidebar_state(app));
    render_sidebar_panel_stack(f, area, app, &visible);
}

/// Build the pinned panel stack. This uses the same content-sensitive panels
/// as Auto, but it never participates in idle auto-collapse.
fn render_sidebar_pinned(f: &mut Frame, area: Rect, app: &mut App) {
    let visible = auto_sidebar_panels(auto_sidebar_state(app));
    render_sidebar_panel_stack(f, area, app, &visible);
}

fn render_sidebar_panel_stack(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    visible: &[AutoSidebarPanel],
) {
    let constraints: Vec<Constraint> = match visible.len() {
        1 => vec![Constraint::Min(0)],
        2 => vec![Constraint::Percentage(50), Constraint::Min(0)],
        3 => vec![
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Min(0),
        ],
        4 => vec![
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Min(6),
        ],
        _ => vec![
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Min(6),
        ],
    };

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (panel, rect) in visible.iter().zip(sections.iter()) {
        match panel {
            AutoSidebarPanel::Work => render_sidebar_work(f, *rect, app),
            AutoSidebarPanel::Tasks => render_sidebar_tasks(f, *rect, app),
            AutoSidebarPanel::Agents => render_sidebar_subagents(f, *rect, app),
            AutoSidebarPanel::Context => render_context_panel(f, *rect, app),
        }
    }
}

/// Compute the Auto-mode panel signals. Shared by `render_sidebar_auto` (which
/// panel boxes to show) and `sidebar_auto_idle` (whether to collapse the whole
/// sidebar to a full-width transcript). Content-gated: the jobs/tasks panel
/// appears only when there are real durable tasks or background shell jobs,
/// never merely because a turn is in flight.
fn auto_sidebar_state(app: &mut App) -> AutoSidebarState {
    AutoSidebarState {
        work_has_content: sidebar_work_summary(app).has_useful_content(),
        // The jobs/tasks panel appears in Auto mode only for live background
        // work — running or queued shell jobs, RLM, or durable Fleet tasks.
        // Completed jobs, per-turn tools, and model reasoning do not reopen
        // the panel; they remain visible only when Tasks is explicitly focused.
        tasks_empty: !app.task_panel.iter().any(background_task_is_live),
        agents_empty: app.subagent_cache.is_empty()
            && app.agent_progress.is_empty()
            && active_fanout_counts(app).is_none()
            && !foreground_rlm_running(app),
        context_enabled: app.context_panel,
    }
}

/// Auto-reveal: in Auto focus mode the sidebar collapses to nothing when there
/// is no active content (no To-do, no live/queued fleet, no background jobs, no
/// pinned context), so an idle session gets a full-width transcript. Any active
/// content brings it back; completed agents linger in the cache as a natural
/// grace before it retracts. Explicit panel focus and Hidden bypass this (the
/// former should always show, the latter is handled by the width helper).
pub(crate) fn sidebar_auto_idle(app: &mut App) -> bool {
    if app.sidebar_focus != SidebarFocus::Auto {
        return false;
    }
    let state = auto_sidebar_state(app);
    !state.work_has_content && state.tasks_empty && state.agents_empty && !state.context_enabled
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoSidebarPanel {
    Work,
    Tasks,
    Agents,
    Context,
}

#[derive(Debug, Clone, Copy)]
struct AutoSidebarState {
    work_has_content: bool,
    tasks_empty: bool,
    agents_empty: bool,
    context_enabled: bool,
}

fn auto_sidebar_panels(state: AutoSidebarState) -> Vec<AutoSidebarPanel> {
    let nothing_else_active = state.tasks_empty && state.agents_empty && !state.context_enabled;
    let mut visible = Vec::with_capacity(4);

    if state.work_has_content || nothing_else_active {
        visible.push(AutoSidebarPanel::Work);
    }
    if !state.tasks_empty {
        visible.push(AutoSidebarPanel::Tasks);
    }
    if !state.agents_empty {
        visible.push(AutoSidebarPanel::Agents);
    }
    if state.context_enabled {
        visible.push(AutoSidebarPanel::Context);
    }

    visible
}

#[derive(Debug, Clone)]
struct SidebarWorkChecklistItem {
    id: u32,
    content: String,
    status: TodoStatus,
}

#[derive(Debug, Clone)]
struct SidebarWorkStrategyStep {
    text: String,
    status: StepStatus,
    elapsed: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SidebarWorkSummary {
    goal_objective: Option<String>,
    goal_token_budget: Option<u32>,
    goal_completed: bool,
    goal_started_at: Option<Instant>,
    tokens_used: u32,
    checklist_completion_pct: u8,
    checklist_items: Vec<SidebarWorkChecklistItem>,
    strategy_explanation: Option<String>,
    strategy_steps: Vec<SidebarWorkStrategyStep>,
    state_updating: bool,
    pause_indicator: Option<String>,
    workflow_paused: bool,
}

impl SidebarWorkSummary {
    fn checklist_is_primary(&self) -> bool {
        !self.checklist_items.is_empty()
    }

    fn checklist_is_complete(&self) -> bool {
        self.checklist_is_primary()
            && self
                .checklist_items
                .iter()
                .all(|item| item.status == TodoStatus::Completed)
    }

    fn has_strategy(&self) -> bool {
        self.strategy_explanation
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || !self.strategy_steps.is_empty()
    }

    fn has_useful_content(&self) -> bool {
        self.goal_objective
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || !self.checklist_items.is_empty()
            || self.has_strategy()
            || self.state_updating
    }

    fn strategy_counts(&self) -> (usize, usize, usize) {
        let mut pending = 0;
        let mut in_progress = 0;
        let mut completed = 0;
        for step in &self.strategy_steps {
            match step.status {
                StepStatus::Pending => pending += 1,
                StepStatus::InProgress => in_progress += 1,
                StepStatus::Completed => completed += 1,
            }
        }
        (pending, in_progress, completed)
    }

    fn strategy_progress_percent(&self) -> u8 {
        if self.strategy_steps.is_empty() {
            return 0;
        }
        let completed = self
            .strategy_steps
            .iter()
            .filter(|step| step.status == StepStatus::Completed)
            .count();
        let percent = completed.saturating_mul(100) / self.strategy_steps.len();
        u8::try_from(percent).unwrap_or(u8::MAX)
    }
}

fn should_render_strategy_step(
    summary: &SidebarWorkSummary,
    step: &SidebarWorkStrategyStep,
) -> bool {
    !summary.checklist_is_complete() || step.status == StepStatus::Completed
}

fn renderable_strategy_steps(summary: &SidebarWorkSummary) -> Vec<&SidebarWorkStrategyStep> {
    summary
        .strategy_steps
        .iter()
        .filter(|step| should_render_strategy_step(summary, step))
        .collect()
}

fn has_renderable_strategy(summary: &SidebarWorkSummary) -> bool {
    summary
        .strategy_explanation
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
        || summary
            .strategy_steps
            .iter()
            .any(|step| should_render_strategy_step(summary, step))
}

fn sidebar_work_summary(app: &mut App) -> SidebarWorkSummary {
    fn live_goal_objective(app: &App) -> Option<String> {
        if app.paused || app.paused_quarry.is_some() {
            app.hunt
                .quarry
                .clone()
                .or_else(|| app.paused_quarry.clone())
        } else {
            app.hunt.quarry.clone()
        }
    }

    fn live_pause_indicator(app: &App) -> Option<String> {
        if app.paused && app.is_loading {
            Some("(Pausing)".to_string())
        } else if app.paused || app.paused_quarry.is_some() {
            Some("(Paused)".to_string())
        } else {
            None
        }
    }

    fn apply_live_goal_state(summary: &mut SidebarWorkSummary, app: &App) {
        summary.goal_objective = live_goal_objective(app);
        summary.goal_token_budget = app.hunt.token_budget;
        summary.goal_completed = app.hunt.verdict == HuntVerdict::Hunted;
        summary.goal_started_at = app.hunt.started_at;
        summary.tokens_used = app.session.total_conversation_tokens;
        summary.pause_indicator = live_pause_indicator(app);
        summary.workflow_paused = app.paused || app.paused_quarry.is_some();
    }

    let fresh = (|| {
        let todos = app.todos.try_lock().ok()?;
        let plan = app.plan_state.try_lock().ok()?;

        let snapshot = todos.snapshot();
        let checklist_completion_pct = snapshot.completion_pct;
        let checklist_items = snapshot
            .items
            .into_iter()
            .map(|item| SidebarWorkChecklistItem {
                id: item.id,
                content: item.content,
                status: item.status,
            })
            .collect();

        let (strategy_explanation, strategy_steps) = if plan.is_empty() {
            (None, Vec::new())
        } else {
            (
                plan.explanation().map(str::to_string),
                plan.steps()
                    .iter()
                    .map(|step| SidebarWorkStrategyStep {
                        text: step.text.clone(),
                        status: step.status.clone(),
                        elapsed: step.elapsed_str(),
                    })
                    .collect(),
            )
        };

        let mut summary = SidebarWorkSummary {
            goal_objective: live_goal_objective(app),
            goal_token_budget: app.hunt.token_budget,
            goal_completed: app.hunt.verdict == HuntVerdict::Hunted,
            goal_started_at: app.hunt.started_at,
            tokens_used: app.session.total_conversation_tokens,
            checklist_completion_pct,
            checklist_items,
            strategy_explanation,
            strategy_steps,
            state_updating: false,
            pause_indicator: live_pause_indicator(app),
            workflow_paused: app.paused || app.paused_quarry.is_some(),
        };
        apply_live_goal_state(&mut summary, app);
        Some(summary)
    })();

    if let Some(summary) = fresh {
        app.cached_work_summary = Some(summary.clone());
        return summary;
    }

    if let Some(cached) = app.cached_work_summary.as_ref() {
        let mut summary = cached.clone();
        apply_live_goal_state(&mut summary, app);
        return summary;
    }

    let mut summary = SidebarWorkSummary {
        state_updating: true,
        ..SidebarWorkSummary::default()
    };
    apply_live_goal_state(&mut summary, app);
    summary
}

fn work_panel_lines(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
    palette_mode: palette::PaletteMode,
    ui_theme: &palette::UiTheme,
) -> Vec<Line<'static>> {
    let theme = Theme::for_palette_mode(palette_mode);
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));

    push_work_goal_lines(summary, content_width, max_rows, &mut lines, ui_theme);

    if summary.state_updating && lines.len() < max_rows {
        lines.push(Line::from(Span::styled(
            "Work state updating...",
            Style::default().fg(ui_theme.text_muted),
        )));
    }

    push_work_checklist_lines(summary, content_width, max_rows, &mut lines, ui_theme);
    push_work_strategy_lines(summary, content_width, max_rows, &mut lines, &theme);

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            work_panel_empty_hint(content_width),
            Style::default().fg(ui_theme.text_muted).italic(),
        )));
    }

    lines
}

fn work_panel_hover_texts(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
) -> Vec<String> {
    let mut texts = Vec::with_capacity(max_rows.max(4));

    if let Some(objective) = summary.goal_objective.as_deref()
        && !objective.trim().is_empty()
        && texts.len() < max_rows
    {
        let icon = if summary.goal_completed {
            "✓"
        } else if summary.workflow_paused {
            "⏸"
        } else {
            "◆"
        };
        texts.push(format!("{icon} {objective}"));

        if let Some(started) = summary.goal_started_at
            && texts.len() < max_rows
        {
            let elapsed = crate::tui::notifications::humanize_duration(started.elapsed());
            let elapsed_str = if summary.goal_completed {
                format!("completed in {elapsed}")
            } else {
                format!("elapsed: {elapsed}")
            };
            texts.push(elapsed_str);
        }

        if let Some(budget) = summary.goal_token_budget
            && texts.len() < max_rows
        {
            let pct = if budget > 0 {
                ((summary.tokens_used as f64 / budget as f64) * 100.0).min(100.0)
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
            texts.push(format!(
                "tokens: {}/{} {}",
                summary.tokens_used, budget, bar
            ));
        }
    }

    if summary.state_updating && texts.len() < max_rows {
        texts.push("Work state updating...".to_string());
    }

    if !summary.checklist_items.is_empty() && texts.len() < max_rows {
        let total = summary.checklist_items.len();
        let completed = summary
            .checklist_items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        texts.push(format!(
            "{}% complete ({completed}/{total})",
            summary.checklist_completion_pct
        ));

        let reserve_for_strategy = if has_renderable_strategy(summary) {
            2
        } else {
            0
        };
        let available_item_rows = max_rows
            .saturating_sub(texts.len())
            .saturating_sub(reserve_for_strategy)
            .min(summary.checklist_items.len());
        let max_items =
            if summary.checklist_items.len() > available_item_rows && available_item_rows > 1 {
                available_item_rows - 1
            } else {
                available_item_rows
            };
        let start = checklist_window_start(&summary.checklist_items, max_items);
        let end = start
            .saturating_add(max_items)
            .min(summary.checklist_items.len());
        for item in summary.checklist_items[start..end].iter() {
            let prefix = match item.status {
                TodoStatus::Pending => "[ ]",
                TodoStatus::InProgress => "[~]",
                TodoStatus::Completed => "[✓]",
            };
            texts.push(format!("{prefix} #{} {}", item.id, item.content));
        }

        let earlier = start;
        let later = summary.checklist_items.len().saturating_sub(end);
        let remaining = earlier.saturating_add(later);
        if remaining > 0 && texts.len() < max_rows {
            let mut label = match (earlier, later) {
                (0, later) => format!("+{later} more checklist items"),
                (earlier, 0) => format!("+{earlier} earlier checklist items"),
                (earlier, later) => format!("+{earlier} earlier, +{later} later"),
            };
            // Hovering the overflow row reveals the omitted items, since
            // the compact panel gives no other way to inspect them (#3063).
            let omitted = summary.checklist_items[..start]
                .iter()
                .chain(summary.checklist_items[end..].iter());
            for item in omitted {
                let prefix = match item.status {
                    TodoStatus::Pending => "[ ]",
                    TodoStatus::InProgress => "[~]",
                    TodoStatus::Completed => "[✓]",
                };
                let _ = write!(label, "\n{prefix} #{} {}", item.id, item.content);
            }
            texts.push(label);
        }
    }

    if has_renderable_strategy(summary) && texts.len() < max_rows {
        let strategy_steps = renderable_strategy_steps(summary);

        if !summary.checklist_is_primary() && !summary.strategy_steps.is_empty() {
            let (pending, in_progress, completed) = summary.strategy_counts();
            let total = pending + in_progress + completed;
            texts.push(format!(
                "Strategy metadata {}% complete ({completed}/{total})",
                summary.strategy_progress_percent()
            ));
        } else {
            texts.push(work_strategy_context_label(summary).to_string());
        }

        if let Some(explanation) = summary.strategy_explanation.as_deref()
            && texts.len() < max_rows
        {
            texts.push(explanation.to_string());
        }

        let max_steps = max_rows
            .saturating_sub(texts.len())
            .min(strategy_steps.len());
        let remaining = strategy_steps.len().saturating_sub(max_steps);
        for step in strategy_steps.into_iter().take(max_steps) {
            let prefix = match step.status {
                StepStatus::Pending => "[ ]",
                StepStatus::InProgress => "[~]",
                StepStatus::Completed => "[✓]",
            };
            let mut text = if summary.checklist_is_primary() {
                format!(
                    "{} {}",
                    strategy_context_step_prefix(&step.status),
                    step.text
                )
            } else {
                format!("{prefix} {}", step.text)
            };
            if !step.elapsed.is_empty() {
                let _ = write!(text, " ({})", step.elapsed);
            }
            texts.push(text);
        }

        if remaining > 0 && texts.len() < max_rows {
            texts.push(format!("+{remaining} more strategy steps"));
        }
    }

    if texts.is_empty() {
        texts.push("No active work".to_string());
    }

    texts
}

fn push_work_goal_lines(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
    lines: &mut Vec<Line<'static>>,
    theme: &palette::UiTheme,
) {
    let Some(objective) = summary.goal_objective.as_deref() else {
        return;
    };
    if objective.trim().is_empty() || lines.len() >= max_rows {
        return;
    }

    let icon = if summary.goal_completed {
        "✓"
    } else if summary.workflow_paused {
        "⏸"
    } else {
        "◆"
    };
    let status_style = if summary.goal_completed {
        Style::default()
            .fg(theme.success)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.warning)
            .add_modifier(ratatui::style::Modifier::BOLD)
    };
    let label = if let Some(indicator) = summary.pause_indicator.as_deref() {
        format!("{objective} {indicator}")
    } else {
        objective.to_string()
    };

    lines.push(Line::from(Span::styled(
        format!(
            "{} {}",
            icon,
            truncate_line_to_width(&label, content_width.saturating_sub(2).max(1))
        ),
        status_style,
    )));

    // Elapsed time
    if let Some(started) = summary.goal_started_at
        && lines.len() < max_rows
    {
        let elapsed = crate::tui::notifications::humanize_duration(started.elapsed());
        let elapsed_str = if summary.goal_completed {
            format!("completed in {elapsed}")
        } else {
            format!("elapsed: {elapsed}")
        };
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&elapsed_str, content_width),
            Style::default().fg(theme.text_muted),
        )));
    }

    if let Some(budget) = summary.goal_token_budget
        && lines.len() < max_rows
    {
        let pct = if budget > 0 {
            ((summary.tokens_used as f64 / budget as f64) * 100.0).min(100.0)
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
            truncate_line_to_width(
                &format!("tokens: {}/{} {}", summary.tokens_used, budget, bar),
                content_width,
            ),
            Style::default().fg(theme.text_muted),
        )));
    }
}

fn push_work_checklist_lines(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
    lines: &mut Vec<Line<'static>>,
    theme: &palette::UiTheme,
) {
    if summary.checklist_items.is_empty() || lines.len() >= max_rows {
        return;
    }

    let total = summary.checklist_items.len();
    let completed = summary
        .checklist_items
        .iter()
        .filter(|item| item.status == TodoStatus::Completed)
        .count();
    lines.push(Line::from(vec![
        Span::styled(
            format!("{}%", summary.checklist_completion_pct),
            Style::default().fg(theme.success).bold(),
        ),
        Span::styled(
            format!(" complete ({completed}/{total})"),
            Style::default().fg(theme.text_muted),
        ),
    ]));

    let reserve_for_strategy = if has_renderable_strategy(summary) {
        2
    } else {
        0
    };
    let available_item_rows = max_rows
        .saturating_sub(lines.len())
        .saturating_sub(reserve_for_strategy)
        .min(summary.checklist_items.len());
    let max_items =
        if summary.checklist_items.len() > available_item_rows && available_item_rows > 1 {
            available_item_rows - 1
        } else {
            available_item_rows
        };
    let start = checklist_window_start(&summary.checklist_items, max_items);
    let end = start
        .saturating_add(max_items)
        .min(summary.checklist_items.len());
    for item in summary.checklist_items[start..end].iter() {
        let (prefix, color) = match item.status {
            TodoStatus::Pending => ("[ ]", theme.text_muted),
            TodoStatus::InProgress => ("[~]", theme.warning),
            TodoStatus::Completed => ("[✓]", theme.success),
        };
        let text = format!("{prefix} #{} {}", item.id, item.content);
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&text, content_width),
            Style::default().fg(color),
        )));
    }

    let earlier = start;
    let later = summary.checklist_items.len().saturating_sub(end);
    let remaining = earlier.saturating_add(later);
    if remaining > 0 && lines.len() < max_rows {
        let label = match (earlier, later) {
            (0, later) => format!("+{later} more checklist items"),
            (earlier, 0) => format!("+{earlier} earlier checklist items"),
            (earlier, later) => format!("+{earlier} earlier, +{later} later"),
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(theme.text_muted),
        )));
    }
}

fn checklist_window_start(items: &[SidebarWorkChecklistItem], max_items: usize) -> usize {
    if max_items >= items.len() {
        return 0;
    }
    let Some(active_idx) = items
        .iter()
        .position(|item| item.status == TodoStatus::InProgress)
    else {
        return 0;
    };
    active_idx
        .saturating_sub(max_items / 2)
        .min(items.len().saturating_sub(max_items))
}

fn push_work_strategy_lines(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
) {
    if !has_renderable_strategy(summary) || lines.len() >= max_rows {
        return;
    }

    let checklist_is_primary = summary.checklist_is_primary();
    let strategy_steps = renderable_strategy_steps(summary);
    if !checklist_is_primary && !summary.strategy_steps.is_empty() {
        let (pending, in_progress, completed) = summary.strategy_counts();
        let total = pending + in_progress + completed;
        lines.push(Line::from(vec![
            Span::styled(
                "Strategy metadata ",
                Style::default().fg(theme.plan_summary_color).bold(),
            ),
            Span::styled(
                format!("{}%", summary.strategy_progress_percent()),
                Style::default().fg(theme.plan_progress_color).bold(),
            ),
            Span::styled(
                format!(" complete ({completed}/{total})"),
                Style::default().fg(theme.plan_summary_color),
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            work_strategy_context_label(summary),
            Style::default().fg(theme.plan_summary_color).bold(),
        )));
    }

    if let Some(explanation) = summary.strategy_explanation.as_deref()
        && lines.len() < max_rows
    {
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(explanation, content_width),
            Style::default().fg(theme.plan_explanation_color),
        )));
    }

    let max_steps = max_rows
        .saturating_sub(lines.len())
        .min(strategy_steps.len());
    let remaining = strategy_steps.len().saturating_sub(max_steps);
    for step in strategy_steps.into_iter().take(max_steps) {
        let (prefix, color) = match step.status {
            StepStatus::Pending => ("[ ]", theme.plan_pending_color),
            StepStatus::InProgress => ("[~]", theme.plan_in_progress_color),
            StepStatus::Completed => ("[✓]", theme.plan_completed_color),
        };
        let (text_prefix, color) = if checklist_is_primary {
            (
                strategy_context_step_prefix(&step.status),
                strategy_context_step_color(&step.status, theme),
            )
        } else {
            (prefix, color)
        };
        let mut text = format!("{text_prefix} {}", step.text);
        if !step.elapsed.is_empty() {
            let _ = write!(text, " ({})", step.elapsed);
        }
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&text, content_width),
            Style::default().fg(color),
        )));
    }

    if remaining > 0 && lines.len() < max_rows {
        lines.push(Line::from(Span::styled(
            format!("+{remaining} more strategy steps"),
            Style::default().fg(theme.plan_summary_color),
        )));
    }
}

fn work_strategy_context_label(summary: &SidebarWorkSummary) -> &'static str {
    if summary.checklist_is_primary() {
        "Strategy context"
    } else {
        "Strategy metadata"
    }
}

fn strategy_context_step_prefix(status: &StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "phase next:",
        StepStatus::InProgress => "phase now:",
        StepStatus::Completed => "phase done:",
    }
}

fn strategy_context_step_color(status: &StepStatus, theme: &Theme) -> ratatui::style::Color {
    match status {
        StepStatus::Pending => theme.plan_pending_color,
        StepStatus::InProgress => theme.plan_in_progress_color,
        StepStatus::Completed => theme.plan_summary_color,
    }
}

#[must_use]
fn work_panel_empty_hint(content_width: usize) -> String {
    truncate_line_to_width("No active work", content_width)
}

fn render_sidebar_work(f: &mut Frame, area: Rect, app: &mut App) {
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
fn background_task_click_actions(task: &TaskPanelEntry) -> (String, String) {
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

fn background_task_has_stop_target(task: &TaskPanelEntry) -> bool {
    matches!(task.status.as_str(), "running" | "queued")
}

fn label_with_stop_target(label: &str, content_width: usize) -> String {
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

fn render_sidebar_tasks(f: &mut Frame, area: Rect, app: &mut App) {
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
struct SidebarToolRow {
    name: String,
    status: ToolStatus,
    summary: String,
    duration_ms: Option<u64>,
}

/// Build the Tasks panel lines together with a parallel per-line click-action
/// vector (#3028). Producing both in a single pass keeps the action indices
/// aligned with the rendered lines no matter how the layout evolves.
fn task_panel_rows(
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

fn task_panel_hover_texts(app: &App, max_rows: usize) -> Vec<String> {
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

fn push_sidebar_label_theme(lines: &mut Vec<Line<'static>>, label: &str, theme: &palette::UiTheme) {
    lines.push(Line::from(Span::styled(
        label.to_string(),
        Style::default().fg(theme.accent_primary).bold(),
    )));
}

fn push_tool_row_hover_texts(texts: &mut Vec<String>, rows: &[SidebarToolRow], max_rows: usize) {
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

fn push_reasoning_rows(
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

fn push_reasoning_row_hover_texts(
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

fn background_task_labels(task: &TaskPanelEntry, duration: &str) -> (String, String) {
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

fn background_task_is_live(task: &TaskPanelEntry) -> bool {
    task.kind == TaskPanelEntryKind::Background
        && matches!(task.status.as_str(), "queued" | "running")
}

const BRAILLE_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const BRAILLE_SPINNER_FRAME_MS: u64 = 100;

fn background_task_spinner_prefix(task: &TaskPanelEntry) -> Option<&'static str> {
    if task.status != "running" {
        return None;
    }
    let frame = task.duration_ms.unwrap_or_default() / BRAILLE_SPINNER_FRAME_MS;
    Some(BRAILLE_SPINNER_FRAMES[frame as usize % BRAILLE_SPINNER_FRAMES.len()])
}

fn stale_no_output_label(task: &TaskPanelEntry) -> Option<String> {
    if !(task.stale && task.status == "running") {
        return None;
    }
    task.elapsed_since_output_ms
        .map(format_duration_ms)
        .map(|duration| format!("stale, no output {duration}"))
        .or_else(|| Some("stale, no output".to_string()))
}

fn active_tool_rows(app: &App) -> Vec<SidebarToolRow> {
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
enum ActiveToolRowVisibility {
    Visible,
    StaleRunning,
    Hidden,
}

fn active_tool_row_visibility(
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

fn collapsed_stale_running_row(rows: Vec<SidebarToolRow>) -> SidebarToolRow {
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

fn recent_tool_rows(app: &App, limit: usize) -> Vec<SidebarToolRow> {
    let rows: Vec<SidebarToolRow> = app
        .history
        .iter()
        .rev()
        .filter_map(sidebar_tool_row_from_cell)
        .take(RECENT_TOOL_SCAN_LIMIT)
        .collect();
    editorial_tool_rows(rows, limit, ToolRowOrder::NewestFirst)
}

fn push_tool_rows(
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

fn sidebar_tool_row_from_cell(cell: &HistoryCell) -> Option<SidebarToolRow> {
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

fn shell_status_for_sidebar(
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

fn shell_summary_for_sidebar(
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

fn looks_like_pending_ci(
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

fn failure_summary_with_hint(summary: &str) -> String {
    let hint = crate::tui::key_shortcuts::tool_details_shortcut_action_hint("details");
    if summary.trim().is_empty() {
        hint
    } else if summary.contains(&hint) {
        summary.to_string()
    } else {
        format!("{summary} \u{00B7} {hint}")
    }
}

fn friendly_generic_tool_name(name: &str) -> &str {
    match name {
        "task_shell_start" => "start Bash",
        "task_shell_wait" => "wait Bash",
        "task_shell_write" => "write Bash",
        _ => name,
    }
}

fn generic_tool_sidebar_summary(generic: &GenericToolCell) -> String {
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

fn background_task_rows(app: &App, active_rows: &[SidebarToolRow]) -> Vec<TaskPanelEntry> {
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

fn reasoning_task_rows(app: &App) -> Vec<TaskPanelEntry> {
    let mut rows: Vec<TaskPanelEntry> = app
        .task_panel
        .iter()
        .filter(|task| task.kind == TaskPanelEntryKind::ModelReasoning)
        .cloned()
        .collect();
    rows.sort_by_key(|task| (task_status_rank(task.status.as_str()), task.id.clone()));
    rows
}

fn background_task_duplicates_live_tool(
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
enum ToolRowOrder {
    OldestFirst,
    NewestFirst,
}

fn editorial_tool_rows(
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

fn sidebar_row_identity(row: &SidebarToolRow) -> String {
    format!(
        "{}\n{}",
        row.name.trim(),
        normalize_activity_text(row.summary.as_str())
    )
}

fn is_ci_poll_row(row: &SidebarToolRow) -> bool {
    row.name.starts_with("gh pr checks") || row.name.starts_with("gh run watch")
}

fn is_shell_wait_poll_row(row: &SidebarToolRow) -> bool {
    row.status == ToolStatus::Running
        && matches!(row.name.as_str(), "wait Bash" | "exec_shell_wait")
}

fn shell_wait_poll_key(row: &SidebarToolRow) -> String {
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

fn normalize_activity_text(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    crate::tui::osc8::strip_ansi_into(text, &mut cleaned);
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tool_row_rank(row: &SidebarToolRow) -> u8 {
    match row.status {
        ToolStatus::Failed => 0,
        // A schema-hydrated deferred tool is not "run done" — it must be
        // retried — so it ranks with active work, not completed successes.
        ToolStatus::Running | ToolStatus::Hydrated => 1,
        ToolStatus::Success if is_low_value_tool(&row.name) => 3,
        ToolStatus::Success => 2,
    }
}

fn task_status_rank(status: &str) -> u8 {
    match status {
        "running" => 0,
        "failed" => 1,
        "queued" => 2,
        "completed" => 3,
        "canceled" => 4,
        _ => 5,
    }
}

fn is_low_value_tool(name: &str) -> bool {
    let base = name.split_whitespace().next().unwrap_or(name);
    matches!(
        base,
        "read_file" | "grep_files" | "file_search" | "find" | "checklist_update"
    )
}

fn compact_join(parts: impl IntoIterator<Item = String>) -> String {
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let part = part.trim();
        if !part.is_empty() && !out.iter().any(|seen| seen == part) {
            out.push(part.to_string());
        }
    }
    out.join(" · ")
}

fn first_nonempty_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
}

fn tool_status_marker(
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

fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn render_sidebar_subagents(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let usable_rows = area.height.saturating_sub(3) as usize;
    let cached_ids: std::collections::HashSet<&str> = app
        .subagent_cache
        .iter()
        .map(|agent| agent.agent_id.as_str())
        .collect();
    let progress_only_count = app
        .agent_progress
        .keys()
        .filter(|id| !cached_ids.contains(id.as_str()))
        .count();
    let cached_running = app
        .subagent_cache
        .iter()
        .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
        .count();
    let role_counts: std::collections::BTreeMap<String, usize> =
        app.subagent_cache
            .iter()
            .fold(std::collections::BTreeMap::new(), |mut acc, agent| {
                *acc.entry(agent.agent_type.as_str().to_string())
                    .or_insert(0) += 1;
                acc
            });
    let (fanout_running, fanout_total) = active_fanout_counts(app)
        .map(|(running, total)| (running, Some(total)))
        .unwrap_or((0, None));
    let foreground_rlm_running = foreground_rlm_running(app);

    let summary = SidebarSubagentSummary {
        cached_total: app.subagent_cache.len(),
        cached_running,
        progress_only_count,
        fanout_total,
        fanout_running,
        foreground_rlm_running,
        role_counts,
    };
    let rows = sidebar_agent_rows(app);
    let (lines, row_actions) = subagent_panel_rows(
        &summary,
        &rows,
        content_width,
        usable_rows.max(1),
        &app.ui_theme,
    );
    let full_texts = subagent_panel_hover_texts(&summary, &rows, usable_rows.max(1));

    render_sidebar_section(f, area, "Agents", lines, full_texts, row_actions, app);
}

/// Minimal projection of the data the sub-agent sidebar needs. Lifted out
/// of `render_sidebar_subagents` so the rendering can be snapshot-tested
/// without a full `App`.
#[derive(Debug, Clone, Default)]
pub struct SidebarSubagentSummary {
    pub cached_total: usize,
    pub cached_running: usize,
    pub progress_only_count: usize,
    pub fanout_total: Option<usize>,
    pub fanout_running: usize,
    pub foreground_rlm_running: bool,
    pub role_counts: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct SidebarAgentRow {
    pub id: String,
    pub parent_run_id: Option<String>,
    pub spawn_depth: u32,
    pub name: String,
    pub role: String,
    pub status: String,
    pub objective: Option<String>,
    pub git_branch: Option<String>,
    pub progress: Option<String>,
    pub steps_taken: u32,
    pub duration_ms: Option<u64>,
}

fn foreground_rlm_running(app: &App) -> bool {
    app.active_cell.as_ref().is_some_and(|active| {
        active.entries().iter().any(|entry| {
            matches!(
                entry,
                HistoryCell::Tool(ToolCell::Generic(generic))
                    if matches!(
                        generic.name.as_str(),
                        "rlm_open" | "rlm_eval" | "rlm_configure" | "rlm_close" | "rlm"
                    ) && generic.status == ToolStatus::Running
            )
        })
    })
}

fn sidebar_agent_rows(app: &App) -> Vec<SidebarAgentRow> {
    let mut rows: Vec<SidebarAgentRow> = app
        .subagent_cache
        .iter()
        .map(|agent| {
            let progress = app
                .agent_progress
                .get(&agent.agent_id)
                .cloned()
                .or_else(|| {
                    agent
                        .result
                        .as_deref()
                        .map(summarize_tool_output)
                        .filter(|summary| !summary.trim().is_empty())
                });
            // #3030: Prefer the user-assigned nickname > stable label
            // ("Agent 1") > raw name. Every spawned agent gets a label-map
            // entry, so the generated label must not shadow nicknames.
            let display_name = agent
                .nickname
                .clone()
                .or_else(|| app.agent_label_map.get(&agent.agent_id).cloned())
                .unwrap_or_else(|| agent.name.clone());
            SidebarAgentRow {
                id: agent.agent_id.clone(),
                parent_run_id: agent.parent_run_id.clone(),
                spawn_depth: agent.spawn_depth,
                name: display_name,
                role: agent.agent_type.as_str().to_string(),
                status: agent
                    .worker_status
                    .map(sidebar_worker_status_text)
                    .unwrap_or_else(|| subagent_status_text(&agent.status))
                    .to_string(),
                objective: Some(agent.assignment.objective.clone())
                    .filter(|objective| !objective.trim().is_empty()),
                git_branch: agent.git_branch.clone(),
                progress,
                steps_taken: agent.steps_taken,
                duration_ms: Some(agent.duration_ms),
            }
        })
        .collect();

    let cached_ids: std::collections::HashSet<&str> = app
        .subagent_cache
        .iter()
        .map(|agent| agent.agent_id.as_str())
        .collect();
    rows.extend(
        app.agent_progress
            .iter()
            .filter(|(id, _)| !cached_ids.contains(id.as_str()))
            .map(|(id, progress)| {
                // #3030: Prefer stable label for progress-only agents too.
                let display_name = app
                    .agent_label_map
                    .get(id.as_str())
                    .cloned()
                    .unwrap_or_else(|| id.clone());
                let meta = app.agent_progress_meta.get(id.as_str());
                let spawn_depth = meta.map(|meta| meta.spawn_depth).unwrap_or_default();
                SidebarAgentRow {
                    id: id.clone(),
                    parent_run_id: meta.and_then(|meta| meta.parent_run_id.clone()),
                    spawn_depth,
                    name: display_name,
                    role: if spawn_depth > 1 {
                        "child".to_string()
                    } else {
                        "agent".to_string()
                    },
                    status: sidebar_progress_status_text(progress).to_string(),
                    objective: None,
                    git_branch: None,
                    progress: Some(progress.clone()),
                    steps_taken: 0,
                    duration_ms: None,
                }
            }),
    );

    sort_sidebar_agent_rows_as_tree(rows)
}

fn sort_sidebar_agent_rows_as_tree(rows: Vec<SidebarAgentRow>) -> Vec<SidebarAgentRow> {
    let known_ids: std::collections::HashSet<String> =
        rows.iter().map(|row| row.id.clone()).collect();
    let mut children: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    let mut roots = Vec::new();

    for (idx, row) in rows.iter().enumerate() {
        if let Some(parent) = row.parent_run_id.as_deref()
            && known_ids.contains(parent)
        {
            children.entry(parent.to_string()).or_default().push(idx);
            continue;
        }
        roots.push(idx);
    }

    fn push_tree(
        idx: usize,
        rows: &[SidebarAgentRow],
        children: &std::collections::HashMap<String, Vec<usize>>,
        seen: &mut std::collections::HashSet<usize>,
        out: &mut Vec<SidebarAgentRow>,
    ) {
        if !seen.insert(idx) {
            return;
        }
        out.push(rows[idx].clone());
        if let Some(child_indices) = children.get(&rows[idx].id) {
            for child_idx in child_indices {
                push_tree(*child_idx, rows, children, seen, out);
            }
        }
    }

    let mut out = Vec::with_capacity(rows.len());
    let mut seen = std::collections::HashSet::new();
    for idx in roots {
        push_tree(idx, &rows, &children, &mut seen, &mut out);
    }
    for idx in 0..rows.len() {
        push_tree(idx, &rows, &children, &mut seen, &mut out);
    }
    out
}

fn subagent_status_text(status: &SubAgentStatus) -> &'static str {
    match status {
        SubAgentStatus::Running => "running",
        SubAgentStatus::Completed => "done",
        SubAgentStatus::Interrupted(_) => "interrupted",
        SubAgentStatus::Failed(_) => "failed",
        SubAgentStatus::Cancelled => "canceled",
        SubAgentStatus::BudgetExhausted => "budget",
    }
}

fn sidebar_worker_status_text(status: AgentWorkerStatus) -> &'static str {
    match status {
        AgentWorkerStatus::Queued => "queued",
        AgentWorkerStatus::Starting => "starting",
        AgentWorkerStatus::Running => "running",
        AgentWorkerStatus::WaitingForUser => "waiting",
        AgentWorkerStatus::ModelWait => "model wait",
        AgentWorkerStatus::RunningTool => "tool",
        AgentWorkerStatus::Completed => "done",
        AgentWorkerStatus::Failed => "failed",
        AgentWorkerStatus::Cancelled => "canceled",
        AgentWorkerStatus::Interrupted => "interrupted",
    }
}

fn sidebar_progress_status_text(progress: &str) -> &'static str {
    let lower = progress.to_ascii_lowercase();
    if lower.contains("queued") {
        "queued"
    } else if lower.contains("waiting for user") || lower.contains("waiting for follow-up") {
        "waiting"
    } else if lower.contains("waiting for model") || lower.contains("requesting model") {
        "model wait"
    } else if lower.contains("running tool")
        || lower.contains("executing tool")
        || lower.contains("tool:")
    {
        "tool"
    } else if lower.contains("starting") {
        "starting"
    } else {
        agent_worker_status_name(AgentWorkerStatus::Running)
    }
}

/// Build sub-agent sidebar lines from summary + per-agent rows. Public
/// for the snapshot tests in this module.

/// Build the Agents panel lines together with a parallel per-line
/// click-action vector (#3028). Agent label rows open the Fleet worker status
/// view via `/fleet status`; header, role-mix, detail, and RLM lines are not
/// clickable.
fn subagent_panel_rows(
    summary: &SidebarSubagentSummary,
    rows: &[SidebarAgentRow],
    content_width: usize,
    max_rows: usize,
    theme: &palette::UiTheme,
) -> (Vec<Line<'static>>, Vec<Option<String>>) {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));
    let mut actions: Vec<Option<String>> = Vec::with_capacity(max_rows.max(4));

    let fanout_total = summary.fanout_total.unwrap_or(0);
    if summary.cached_total == 0
        && summary.progress_only_count == 0
        && fanout_total == 0
        && !summary.foreground_rlm_running
    {
        lines.push(Line::from(Span::styled(
            "No agents",
            Style::default().fg(theme.text_muted),
        )));
        actions.push(None);
        return (lines, actions);
    }

    let (live_running, total) = if let Some(total) = summary.fanout_total {
        (summary.fanout_running, total)
    } else {
        (
            summary.cached_running + summary.progress_only_count,
            summary.cached_total + summary.progress_only_count,
        )
    };
    let done = total.saturating_sub(live_running);
    let header = if live_running > 0 {
        vec![
            Span::styled(
                format!("{live_running} running"),
                Style::default().fg(theme.accent_primary).bold(),
            ),
            Span::styled(format!(" / {total}"), Style::default().fg(theme.text_muted)),
        ]
    } else {
        vec![Span::styled(
            format!("{done} done"),
            Style::default().fg(theme.success),
        )]
    };
    lines.push(Line::from(header));
    actions.push(None);

    if !summary.role_counts.is_empty() {
        let mix: Vec<String> = summary
            .role_counts
            .iter()
            .map(|(role, count)| format!("{count} {role}"))
            .collect();
        let role_line = mix.join(" \u{00B7} ");
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&role_line, content_width.max(1)),
            Style::default().fg(theme.text_dim),
        )));
        actions.push(None);
    }

    for row in rows {
        if lines.len() >= max_rows {
            break;
        }
        let (marker, color) = agent_status_marker(row.status.as_str(), theme);
        let tree_prefix = agent_tree_prefix(row);
        let label = format!("{tree_prefix}{marker} {} {}", row.role, row.name);
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&label, content_width.max(1)),
            Style::default().fg(color),
        )));
        actions.push(Some("/fleet status".to_string()));

        // Auto-collapse finished sub-agents: hide detail lines for completed
        // agents so the sidebar stays compact when work is done.
        if row.status == "done" {
            continue;
        }

        if lines.len() >= max_rows {
            break;
        }
        // #3030: keep raw agent ids out of the compact detail line — the
        // full id remains available in the hover text.
        let mut detail_parts = Vec::new();
        if row.steps_taken > 0 {
            detail_parts.push(format!("{} step(s)", row.steps_taken));
        }
        if let Some(progress) = row.progress.as_deref()
            && !progress.trim().is_empty()
        {
            detail_parts.push(summarize_tool_output(progress));
        }
        if let Some(branch) = row.git_branch.as_deref() {
            detail_parts.push(format!("branch {branch}"));
        }
        if detail_parts.is_empty() {
            detail_parts.push(row.status.clone());
        }
        lines.push(Line::from(Span::styled(
            format!(
                "  {}",
                truncate_line_to_width(
                    &detail_parts.join(" · "),
                    content_width.saturating_sub(2).max(1)
                )
            ),
            Style::default().fg(theme.text_dim),
        )));
        actions.push(None);
    }

    if summary.foreground_rlm_running {
        lines.push(Line::from(vec![
            Span::styled("RLM", Style::default().fg(theme.accent_primary).bold()),
            Span::styled(
                " foreground work active",
                Style::default().fg(theme.text_dim),
            ),
        ]));
        actions.push(None);
    }

    debug_assert_eq!(lines.len(), actions.len());
    (lines, actions)
}

fn agent_tree_prefix(row: &SidebarAgentRow) -> String {
    if row.parent_run_id.is_none() && row.spawn_depth <= 1 {
        return String::new();
    }
    let depth = row.spawn_depth.max(2).saturating_sub(2).min(6);
    format!("{}└─ ", "  ".repeat(depth as usize))
}

fn subagent_panel_hover_texts(
    summary: &SidebarSubagentSummary,
    rows: &[SidebarAgentRow],
    max_rows: usize,
) -> Vec<String> {
    let mut texts = Vec::with_capacity(max_rows.max(4));

    let fanout_total = summary.fanout_total.unwrap_or(0);
    if summary.cached_total == 0
        && summary.progress_only_count == 0
        && fanout_total == 0
        && !summary.foreground_rlm_running
    {
        texts.push("No agents".to_string());
        return texts;
    }

    let (live_running, total) = if let Some(total) = summary.fanout_total {
        (summary.fanout_running, total)
    } else {
        (
            summary.cached_running + summary.progress_only_count,
            summary.cached_total + summary.progress_only_count,
        )
    };
    let done = total.saturating_sub(live_running);
    if live_running > 0 {
        texts.push(format!("{live_running} running / {total}"));
    } else {
        texts.push(format!("{done} done"));
    }

    if !summary.role_counts.is_empty() && texts.len() < max_rows {
        let mix: Vec<String> = summary
            .role_counts
            .iter()
            .map(|(role, count)| format!("{count} {role}"))
            .collect();
        texts.push(mix.join(" · "));
    }

    for row in rows {
        if texts.len() >= max_rows {
            break;
        }
        // The compact label row truncates aggressively, so its hover text
        // carries the full agent dossier: id, role, status, elapsed,
        // objective, branch, and untruncated progress (#3063).
        texts.push(agent_row_hover_text(row));

        if row.status == "done" {
            continue;
        }

        if texts.len() >= max_rows {
            break;
        }
        let mut detail_parts = Vec::new();
        detail_parts.push(row.id.clone());
        if row.steps_taken > 0 {
            detail_parts.push(format!("{} step(s)", row.steps_taken));
        }
        if let Some(progress) = row.progress.as_deref()
            && !progress.trim().is_empty()
        {
            detail_parts.push(progress.trim().to_string());
        }
        if let Some(branch) = row.git_branch.as_deref() {
            detail_parts.push(format!("branch {branch}"));
        }
        if let Some(duration) = row.duration_ms {
            detail_parts.push(format_duration_ms(duration));
        }
        texts.push(format!("  {}", detail_parts.join(" · ")));
    }

    if summary.foreground_rlm_running && texts.len() < max_rows {
        texts.push("RLM foreground work active".to_string());
    }

    texts
}

/// Full hover dossier for one Agents-panel label row (#3063). The compact
/// row only shows `marker role name`, so hovering reveals everything else
/// without spamming raw ids into the normal view.
fn agent_row_hover_text(row: &SidebarAgentRow) -> String {
    let (marker, _) = agent_status_marker(row.status.as_str(), &palette::UI_THEME);
    let mut text = format!(
        "{}{} {} {}",
        agent_tree_prefix(row),
        marker,
        row.role,
        row.name
    );
    let _ = write!(text, "\nid: {}", row.id);
    if let Some(parent) = row.parent_run_id.as_deref() {
        let _ = write!(text, "\nparent: {parent}");
    }
    if row.spawn_depth > 0 {
        let _ = write!(text, "\ndepth: {}", row.spawn_depth);
    }
    let mut status_line = format!("status: {}", row.status);
    if let Some(duration) = row.duration_ms {
        let _ = write!(status_line, " · elapsed {}", format_duration_ms(duration));
    }
    if row.steps_taken > 0 {
        let _ = write!(status_line, " · {} step(s)", row.steps_taken);
    }
    let _ = write!(text, "\n{status_line}");
    if let Some(objective) = row.objective.as_deref() {
        let _ = write!(text, "\nobjective: {}", objective.trim());
    }
    if let Some(branch) = row.git_branch.as_deref() {
        let _ = write!(text, "\nbranch: {branch}");
    }
    if let Some(progress) = row.progress.as_deref()
        && !progress.trim().is_empty()
    {
        let _ = write!(text, "\nprogress: {}", progress.trim());
    }
    text
}

fn agent_status_marker(
    status: &str,
    theme: &palette::UiTheme,
) -> (&'static str, ratatui::style::Color) {
    match status {
        "running" => ("[~]", theme.warning),
        "done" => ("[✓]", theme.success),
        "failed" => ("[!]", theme.error_fg),
        "canceled" | "interrupted" => ("[-]", theme.text_muted),
        _ => ("[ ]", theme.text_muted),
    }
}

/// Session-context panel (#504) — consolidated session state overview.
///
/// Surfaces at-a-glance: working set, token usage / context %, running
/// cost, MCP server count, LSP toggle state, cycle count, and memory
/// file size + mtime. Each section is a compact one-liner so the panel
/// reads as a dashboard rather than a scrolling list.
fn render_context_panel(f: &mut Frame, area: Rect, app: &mut App) {
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
        let size_hint = std::fs::metadata(&app.memory_path)
            .map(|m| m.len())
            .map(|bytes| {
                if bytes >= 1024 * 1024 {
                    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                } else if bytes >= 1024 {
                    format!("{:.1} KB", bytes as f64 / 1024.0)
                } else {
                    format!("{bytes} B")
                }
            })
            .unwrap_or_else(|_| "—".to_string());
        lines.push(Line::from(Span::styled(
            format!("memory: {} ({})", app.memory_path.display(), size_hint),
            Style::default().fg(theme.text_muted),
        )));
    }

    render_sidebar_section(f, area, "Session", lines, Vec::new(), Vec::new(), app);
}

fn context_panel_cost_line(app: &App) -> String {
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

fn spans_to_text(spans: &[Span<'_>]) -> String {
    let mut s = String::new();
    for span in spans {
        s.push_str(span.content.as_ref());
    }
    s
}

fn render_sidebar_section(
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

fn sidebar_hover_rows(
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

#[cfg(test)]
mod tests {}
