//! context usage 子系统（从 ui 上帝文件切片）
use super::*;

fn estimated_context_tokens(app: &App) -> Option<i64> {
    i64::try_from(estimate_input_tokens_conservative(
        &app.api_messages,
        app.system_prompt.as_ref(),
    ))
    .ok()
}

pub(crate) fn context_usage_snapshot(app: &App) -> Option<(i64, u32, f64)> {
    let max = crate::route_budget::route_context_window_tokens(
        app.api_provider,
        app.effective_model_for_budget(),
        app.active_route_limits,
    );
    let max_i64 = i64::from(max);
    let reported = app
        .session
        .last_prompt_tokens
        .map(i64::from)
        .map(|tokens| tokens.max(0));
    let estimated = estimated_context_tokens(app).map(|tokens| tokens.max(0));

    // Always prefer the estimated current-context size (computed from
    // `app.api_messages`) when we have it. Reported `last_prompt_tokens`
    // comes from `Event::TurnComplete.usage`, which the engine builds with
    // `turn.add_usage` — that SUMS input_tokens across every round in the
    // turn, so a multi-round tool-call turn reports a value much larger
    // than the actual context window state, then the next single-round
    // turn drops back to a single round's input_tokens. User-visible %
    // was bouncing 31% → 9% (#115) because of this. The estimate is
    // monotonic wrt conversation growth, which is what a "context filling
    // up" indicator should show. We still consult `reported` only as a
    // fallback when no estimate is available (e.g., immediately after a
    // session restore before the api_messages are populated).
    let used = match (estimated, reported) {
        (Some(estimated), _) => estimated.min(max_i64),
        (None, Some(reported)) => reported.min(max_i64),
        (None, None) => return None,
    };

    let max_f64 = f64::from(max);
    let used_f64 = used as f64;
    let percent = ((used_f64 / max_f64) * 100.0).clamp(0.0, 100.0);
    Some((used, max, percent))
}

pub(crate) fn maybe_warn_context_pressure(app: &mut App) {
    let Some((used, max, percent)) = context_usage_snapshot(app) else {
        return;
    };

    let configured_threshold = app.compact_threshold_percent.clamp(10.0, 100.0);
    let warning_threshold = CONTEXT_SUGGEST_COMPACT_THRESHOLD_PERCENT.min(configured_threshold);
    if percent < warning_threshold {
        return;
    }

    let recommendation = if !app.auto_compact {
        "Consider enabling auto_compact or use /compact."
    } else if percent >= configured_threshold {
        "Auto-compaction will run before the next send."
    } else {
        "Auto-compaction is enabled."
    };

    if percent >= CONTEXT_CRITICAL_THRESHOLD_PERCENT {
        app.status_message = Some(format!(
            "Context critical: {percent:.0}% ({used}/{max} tokens). {recommendation}"
        ));
        return;
    }

    if app.status_message.is_none() {
        let status_prefix = if percent >= CONTEXT_WARNING_THRESHOLD_PERCENT {
            "Context high"
        } else {
            "Context building"
        };
        app.status_message = Some(format!(
            "{status_prefix}: {percent:.0}% ({used}/{max} tokens). {recommendation}"
        ));
    }
}

pub(crate) fn should_auto_compact_before_send(app: &App) -> bool {
    if !app.auto_compact {
        return false;
    }
    context_usage_snapshot(app)
        .map(|(_, _, pct)| pct >= app.compact_threshold_percent.clamp(10.0, 100.0))
        .unwrap_or(false)
}

pub(crate) fn status_animation_interval_ms(app: &App) -> u64 {
    if app.low_motion {
        2_400
    } else {
        UI_STATUS_ANIMATION_MS
    }
}

pub(crate) fn active_poll_ms(app: &App) -> u64 {
    if app.low_motion {
        96
    } else {
        UI_ACTIVE_POLL_MS
    }
}

pub(crate) fn idle_poll_ms(app: &App) -> u64 {
    if app.low_motion { 120 } else { UI_IDLE_POLL_MS }
}

pub(crate) fn clamp_event_poll_timeout(timeout: Duration) -> Duration {
    const MIN_EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(1);
    timeout.max(MIN_EVENT_POLL_TIMEOUT)
}

pub(crate) fn history_has_live_motion(history: &[HistoryCell]) -> bool {
    use crate::tui::history::SubAgentCell;
    use crate::tui::widgets::agent_card::AgentLifecycle;
    history.iter().any(|cell| match cell {
        HistoryCell::Thinking { streaming, .. } => *streaming,
        HistoryCell::Tool(tool) => match tool {
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
        },
        HistoryCell::SubAgent(SubAgentCell::Delegate(card)) => matches!(
            card.status,
            AgentLifecycle::Pending | AgentLifecycle::Running
        ),
        HistoryCell::SubAgent(SubAgentCell::Fanout(card)) => card
            .workers
            .iter()
            .any(|w| matches!(w.status, AgentLifecycle::Pending | AgentLifecycle::Running)),
        _ => false,
    })
}
