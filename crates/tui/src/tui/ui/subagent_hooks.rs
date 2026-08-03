//! subagent hooks 子系统（从 ui 上帝文件切片）
use super::*;

pub(crate) fn execute_subagent_observer_hook(
    app: &App,
    event: HookEvent,
    agent_id: &str,
    text_field: &str,
    text: &str,
) {
    if !app.hooks.has_hooks_for_event(event) {
        return;
    }

    let (preview, truncated) = bounded_subagent_hook_preview(text);
    let context = app.base_hook_context().with_message(&preview);
    let mut payload = serde_json::json!({
        "event": event.as_str(),
        "agent_id": agent_id,
        "session_id": context.session_id.as_deref(),
        "workspace": context.workspace.as_ref().map(|path| path.display().to_string()),
        "mode": context.mode.as_deref(),
        "model": context.model.as_deref(),
        "total_tokens": context.total_tokens,
    });
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            format!("{text_field}_preview"),
            serde_json::Value::String(preview),
        );
        object.insert(
            format!("{text_field}_truncated"),
            serde_json::Value::Bool(truncated),
        );
    }

    if event == HookEvent::SubagentComplete {
        payload["status"] = serde_json::Value::String(
            subagent_completion_status(text).unwrap_or_else(|| "unknown".to_string()),
        );
    }

    let hooks = app.hooks.clone();
    let _ = std::thread::Builder::new()
        .name(format!("{}-observer-hook", event.as_str()))
        .spawn(move || {
            let _ = hooks.execute_json_observer(event, &context, &payload);
        });
}

pub(crate) fn execute_turn_end_observer_hook(
    app: &App,
    usage: &Usage,
    duration: Duration,
    error: Option<&str>,
) {
    if !app.hooks.has_hooks_for_event(HookEvent::TurnEnd) {
        return;
    }

    let context = app.base_hook_context();
    let payload = crate::hooks::turn_end_payload(TurnEndPayloadInput {
        context: &context,
        turn_id: app.runtime_turn_id.as_deref(),
        status: app.runtime_turn_status.as_deref().unwrap_or("unknown"),
        error,
        duration,
        usage,
        totals: TurnEndTotals {
            session_tokens: app.session.total_tokens,
            conversation_tokens: app.session.total_conversation_tokens,
            input_tokens: app.session.total_input_tokens,
            output_tokens: app.session.total_output_tokens,
        },
        tool_count: app.tool_evidence.len(),
        queued_message_count: app.queued_message_count(),
    });
    let hooks = app.hooks.clone();
    let _ = std::thread::Builder::new()
        .name("turn_end-observer-hook".to_string())
        .spawn(move || {
            let _ = hooks.execute_json_observer(HookEvent::TurnEnd, &context, &payload);
        });
}

fn bounded_subagent_hook_preview(text: &str) -> (String, bool) {
    if text.len() <= SUBAGENT_HOOK_PREVIEW_LIMIT {
        return (text.to_string(), false);
    }
    let safe_end = text
        .char_indices()
        .take_while(|(idx, ch)| idx + ch.len_utf8() <= SUBAGENT_HOOK_PREVIEW_LIMIT)
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    (format!("{}...[truncated]", &text[..safe_end]), true)
}

fn subagent_completion_status(result: &str) -> Option<String> {
    const START: &str = "<mimo:subagent.done>";
    const END: &str = "</mimo:subagent.done>";

    if let Some(start) = result.find(START).map(|idx| idx + START.len())
        && let Some(end) = result[start..].find(END).map(|idx| idx + start)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&result[start..end])
        && let Some(status) = value.get("status").and_then(serde_json::Value::as_str)
    {
        return Some(status.to_string());
    }

    let summary = result.lines().find_map(|line| {
        let trimmed = line.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })?;
    let summary = summary.to_ascii_lowercase();
    if matches!(summary.as_str(), "cancelled" | "canceled")
        || summary.starts_with("cancelled:")
        || summary.starts_with("canceled:")
    {
        Some("cancelled".to_string())
    } else if summary == "failed" || summary.starts_with("failed:") {
        Some("failed".to_string())
    } else if summary == "interrupted" || summary.starts_with("interrupted:") {
        Some("interrupted".to_string())
    } else {
        None
    }
}
