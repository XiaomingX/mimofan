//! Sidebar rendering — Pinned / Tasks / Agents / Context panels.
//!
//! Extracted from `tui/ui.rs` (P1.2). The sidebar appears to the right of
//! the chat transcript when the available width allows it. Each section
//! reads from `App` snapshots; mutation lives in the main app loop.

use std::time::{Duration, Instant};

use crate::tui::subagent_routing::active_fanout_counts;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Widget,
    style::Style,
    widgets::Block,
};

/// Tolerance for floating-point cost comparison in the sidebar breakdown.
/// Must be large enough that accumulated f64 error across hundreds of turns
/// does not prematurely hide the session+agents breakdown.
pub(crate) const COST_EQ_TOLERANCE: f64 = 1e-6;
pub(crate) const RECENT_TOOL_SCAN_LIMIT: usize = 24;
pub(crate) const ACTIVE_TOOL_COMPLETED_ROW_TTL: Duration = Duration::from_secs(8);
pub(crate) const ACTIVE_TOOL_STALE_RUNNING_ROW_TTL: Duration = Duration::from_secs(600);
pub(crate) const TASK_STOP_TARGET_LABEL: &str = "[x]";
pub(crate) const TASK_STOP_TARGET_SUFFIX: &str = " [x]";

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
pub(crate) struct SidebarWorkChecklistItem {
    id: u32,
    content: String,
    status: TodoStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarWorkStrategyStep {
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

// Sub-panel modules extracted from the original 3008-line sidebar.rs (#647).
// Each owns one sidebar surface; cross-module helpers are re-exported below
// so callers (and `render_sidebar`) can reach them through this module.
pub(crate) mod context;
pub(crate) mod subagents;
pub(crate) mod tools;
pub(crate) mod work;

// Re-export every sub-module's `pub(crate)` items into this module's
// namespace so sibling sub-modules can reach cross-panel helpers through
// `use crate::tui::sidebar::*;` (e.g. `render_sidebar_section`,
// `background_task_is_live`, `foreground_rlm_running`, `format_duration_ms`).
pub(crate) use context::*;
pub(crate) use subagents::*;
pub(crate) use tools::*;
pub(crate) use work::*;

// Re-export the external (cross-crate) types the sub-modules rely on, so a
// single glob `use crate::tui::sidebar::*;` inside each sub-module satisfies
// both local and external imports.
pub(crate) use crate::mimofan_theme::Theme;
pub(crate) use crate::palette;
pub(crate) use crate::tools::plan::StepStatus;
pub(crate) use crate::tools::subagent::{
    AgentWorkerStatus, SubAgentStatus, agent_worker_status_name,
};
pub(crate) use crate::tools::todo::TodoStatus;
pub(crate) use crate::tui::app::{App, HuntVerdict, SidebarFocus, SidebarHoverState};
pub(crate) use crate::tui::history::{HistoryCell, ToolCell, ToolStatus, summarize_tool_output};
pub(crate) use crate::tui::ui_text::truncate_line_to_width;
// NOTE: `active_fanout_counts` is `pub(super)` in `subagent_routing` and cannot
// be re-exported as `pub(crate)`; sub-modules import it directly from
// `crate::tui::subagent_routing`.
