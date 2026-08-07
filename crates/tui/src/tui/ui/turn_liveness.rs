//! Turn liveness detection and recovery helpers.
//!
//! Monitors active turns for stalls, timeouts, and inconsistent states,
//! providing automatic recovery when the engine or tools stop responding.

use std::time::{Duration, Instant};

use crate::core::events::Event as EngineEvent;
use crate::session_manager::SessionManager;
use crate::tui::persistence_actor::{self, PersistRequest};
use crate::tui::streaming_thinking;

use super::super::app::{App, StatusToastLevel};
use super::super::history::{HistoryCell, ToolCell, ToolStatus};
use super::session_warmup::build_session_snapshot;

// Watchdog timeouts for turn recovery
pub(crate) const DISPATCH_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const TURN_STALL_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const TOOL_HANG_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(900);

/// Reconcile turn liveness by checking for stalled or timed-out turns.
///
/// Returns `true` if the turn state was recovered (caller should repaint).
pub(crate) fn reconcile_turn_liveness(
    app: &mut App,
    now: Instant,
    has_running_agents: bool,
) -> bool {
    // Branch 1: dispatch timeout — the user's prompt was appended to api_messages
    // before dispatch, but the turn never reached `in_progress`.
    if app.is_loading
        && app.runtime_turn_status.is_none()
        && !has_running_agents
        && !app.is_compacting
        && !app.is_purging
        && app.dispatch_started_at.is_some_and(|started| {
            now.saturating_duration_since(started) > DISPATCH_WATCHDOG_TIMEOUT
        })
    {
        // #2739: persist the prompt before clearing turn state so --continue
        // keeps the prompt instead of loading the previous save.
        persist_recovery_snapshot(app);
        app.is_loading = false;
        app.dispatch_started_at = None;
        app.turn_started_at = None;
        app.turn_last_activity_at = None;
        app.push_status_toast(
            "Turn dispatch timed out; the engine may have stopped. Please try again.",
            StatusToastLevel::Error,
            None,
        );
        return true;
    }

    // Branch 2: turn started but status shows completed/interrupted/failed
    // while still marked as loading.
    if app.is_loading
        && matches!(
            app.runtime_turn_status.as_deref(),
            Some("completed" | "interrupted" | "failed")
        )
        && !has_running_agents
        && !app.is_compacting
        && !app.is_purging
    {
        app.is_loading = false;
        app.dispatch_started_at = None;
        app.turn_started_at = None;
        app.turn_last_activity_at = None;
        app.push_status_toast(
            "Recovered from an inconsistent busy state.",
            StatusToastLevel::Warning,
            None,
        );
        return true;
    }

    // Branch 3: turn started but never completed — engine may have
    // panicked, sub-agent may be stuck, or the completion event was lost.
    if app.is_loading
        && matches!(app.runtime_turn_status.as_deref(), Some("in_progress"))
        && !has_running_agents
        && !app.is_compacting
        && !active_turn_has_running_tool(app)
        && app
            .turn_last_activity_at
            .or(app.turn_started_at)
            .is_some_and(|last_activity| {
                now.saturating_duration_since(last_activity) > TURN_STALL_WATCHDOG_TIMEOUT
            })
    {
        recover_stalled_runtime_turn(
            app,
            "Turn stalled — no completion signal received. Please try again.",
            StatusToastLevel::Error,
        );
        return true;
    }

    // Branch 4: tool stalled — turn is in progress with a running tool
    // but no activity for the tool hang timeout.
    if app.is_loading
        && matches!(app.runtime_turn_status.as_deref(), Some("in_progress"))
        && !has_running_agents
        && !app.is_compacting
        && !app.is_purging
        && active_turn_has_running_tool(app)
        && app
            .turn_last_activity_at
            .or(app.turn_started_at)
            .is_some_and(|last_activity| {
                now.saturating_duration_since(last_activity) > TOOL_HANG_WATCHDOG_TIMEOUT
            })
    {
        recover_stalled_runtime_turn(
            app,
            "Tool stalled with no progress for 15m — recovered; the command may still be running in the background. Use exec_shell_cancel or retry.",
            StatusToastLevel::Error,
        );
        return true;
    }

    false
}

/// #2739: persist the current in-memory session state before a recovery or
/// cancellation path clears turn bookkeeping. Without this snapshot, the
/// just-finalised partial turn lives only in `app.api_messages` and is never
/// written to disk, so `--continue` loads the *previous* save — effectively
/// losing the entire in-progress turn.
pub(crate) fn persist_recovery_snapshot(app: &mut App) {
    if let Ok(manager) = SessionManager::default_location() {
        let session = build_session_snapshot(app, &manager);
        if app.current_session_id.is_none() {
            app.current_session_id = Some(session.metadata.id.clone());
        }
        persistence_actor::persist(PersistRequest::SessionSnapshot(session.clone()));
        persistence_actor::persist_plan_state(
            session.metadata.id.clone(),
            app.current_plan_and_todo(),
        );
    }
}

/// Recover from a stalled runtime turn by finalizing streaming state and
/// clearing turn bookkeeping.
pub(crate) fn recover_stalled_runtime_turn(app: &mut App, message: &str, level: StatusToastLevel) {
    // Finalize in-flight thinking / assistant / tool cells so the
    // transcript doesn't show permanent spinners after recovery.
    streaming_thinking::finalize_current(app);
    app.finalize_streaming_assistant_as_interrupted();
    app.finalize_active_cell_as_interrupted();
    app.streaming_state.reset();
    app.streaming_message_index = None;
    app.streaming_thinking_active_entry = None;

    // #2739: persist the partial turn's api_messages before clearing
    // turn state. Without this snapshot the stalled/cancelled turn's
    // messages are held only in memory and --continue sees the
    // *previous* save, losing the entire in-progress turn.
    persist_recovery_snapshot(app);

    app.is_loading = false;
    app.turn_started_at = None;
    app.turn_last_activity_at = None;
    app.runtime_turn_status = None;
    app.runtime_turn_id = None;
    app.dispatch_started_at = None;
    // Per-turn scroll lock — clear so the next turn auto-scrolls.
    app.user_scrolled_during_stream = false;
    app.push_status_toast(message, level, None);
}

/// #3033: gate progress-driven repaints to at most one per 100ms.
///
/// Returns whether the current `AgentProgress` event may request a redraw,
/// updating the last-redraw timestamp when it may. Data updates are never
/// throttled — only the repaint request is.
pub(crate) fn agent_progress_redraw_permitted(
    last_redraw: &mut Option<Instant>,
    now: Instant,
) -> bool {
    match *last_redraw {
        Some(last) if now.duration_since(last) < Duration::from_millis(100) => false,
        _ => {
            *last_redraw = Some(now);
            true
        }
    }
}

pub(crate) fn agent_progress_redraw_permitted_for_drain(
    last_redraw: &mut Option<Instant>,
    seen_agents: &mut std::collections::HashSet<String>,
    agent_id: &str,
    now: Instant,
) -> bool {
    if !seen_agents.insert(agent_id.to_string()) {
        return false;
    }
    agent_progress_redraw_permitted(last_redraw, now)
}

/// Recover from an engine event disconnect by finalizing streaming state
/// and clearing all turn bookkeeping.
pub(crate) fn recover_engine_event_disconnect(app: &mut App) -> bool {
    let had_live_work = app.is_loading
        || app.is_compacting
        || app.is_purging
        || matches!(app.runtime_turn_status.as_deref(), Some("in_progress"))
        || app.streaming_message_index.is_some()
        || app.streaming_thinking_active_entry.is_some()
        || app
            .active_cell
            .as_ref()
            .is_some_and(|cell| !cell.is_empty());

    if !had_live_work {
        return false;
    }

    streaming_thinking::finalize_current(app);
    app.finalize_streaming_assistant_as_interrupted();
    app.finalize_active_cell_as_interrupted();
    app.streaming_state.reset();
    app.streaming_message_index = None;
    app.streaming_thinking_active_entry = None;

    // #2739: persist partial turn before clearing state.
    persist_recovery_snapshot(app);

    app.is_loading = false;
    app.is_compacting = false;
    app.is_purging = false;
    app.turn_started_at = None;
    app.turn_last_activity_at = None;
    app.runtime_turn_status = None;
    app.runtime_turn_id = None;
    app.dispatch_started_at = None;
    app.user_scrolled_during_stream = false;

    for msg in app.drain_pending_steers() {
        app.queue_message(msg);
    }

    app.add_message(HistoryCell::Error {
        message: "Engine stopped before completing the turn. Check ~/.mimofan/crashes and retry."
            .to_string(),
        severity: crate::error_taxonomy::ErrorSeverity::Error,
    });
    app.push_status_toast(
        "Engine stopped before completing the turn.",
        StatusToastLevel::Error,
        None,
    );
    true
}

/// Record turn activity timestamps for stall detection.
pub(crate) fn record_turn_activity(app: &mut App, event: &EngineEvent, now: Instant) {
    if matches!(event, EngineEvent::TurnStarted { .. }) {
        app.turn_last_activity_at = Some(now);
        return;
    }

    if app.is_loading || matches!(app.runtime_turn_status.as_deref(), Some("in_progress")) {
        app.turn_last_activity_at = Some(now);
    }
}

/// Check if the active turn has any running tools.
pub(crate) fn active_turn_has_running_tool(app: &App) -> bool {
    app.active_cell.as_ref().is_some_and(|active| {
        active.entries().iter().any(|cell| match cell {
            HistoryCell::Tool(tool) => tool_cell_is_running(tool),
            _ => false,
        })
    })
}

/// Check if terminal input recovery is relevant given current app state.
pub(crate) fn terminal_input_recovery_relevant(app: &App, has_running_agents: bool) -> bool {
    app.is_loading
        || has_running_agents
        || app.is_compacting
        || app.is_purging
        || matches!(app.runtime_turn_status.as_deref(), Some("in_progress"))
        || active_turn_has_running_tool(app)
}

/// Check if a tool cell is currently running.
pub(crate) fn tool_cell_is_running(tool: &ToolCell) -> bool {
    match tool {
        ToolCell::Exec(cell) => cell.status == ToolStatus::Running,
        ToolCell::Exploring(cell) => cell
            .entries
            .iter()
            .any(|entry| entry.status == ToolStatus::Running),
        ToolCell::PlanUpdate(cell) => cell.status == ToolStatus::Running,
        ToolCell::PatchSummary(cell) => cell.status == ToolStatus::Running,
        ToolCell::Review(cell) => cell.status == ToolStatus::Running,
        ToolCell::DiffPreview(_) => false,
        ToolCell::Mcp(cell) => cell.status == ToolStatus::Running,
        ToolCell::ViewImage(_) => false,
        ToolCell::WebSearch(cell) => cell.status == ToolStatus::Running,
        ToolCell::Generic(cell) => cell.status == ToolStatus::Running,
    }
}
