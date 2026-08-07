//! session loading 子系统（从 ui 上帝文件切片）
use super::*;
use crate::session_manager::SessionManager;

pub(crate) fn apply_loaded_session(app: &mut App, config: &Config, session: &SavedSession) -> bool {
    let (messages, recovered_draft) = recover_interrupted_user_tail(&session.messages);
    app.api_messages = messages;
    app.clear_history();
    app.tool_cells.clear();
    app.tool_details_by_cell.clear();
    app.active_cell = None;
    app.active_tool_details.clear();
    app.active_tool_entry_completed_at.clear();
    app.active_cell_revision = app.active_cell_revision.wrapping_add(1);
    app.exploring_cell = None;
    app.exploring_entries.clear();
    app.ignored_tool_calls.clear();
    app.pending_tool_uses.clear();
    app.last_exec_wait_command = None;

    let messages = app.api_messages.clone();
    let mut message_to_cell = std::collections::HashMap::new();
    for (message_index, msg) in messages.iter().enumerate() {
        let mut cells = history_cells_from_message(msg);
        if msg.role == "user"
            && session
                .context_references
                .iter()
                .any(|record| record.message_index == message_index)
        {
            for cell in &mut cells {
                if let HistoryCell::User { content } = cell {
                    *content = compact_user_context_display(content);
                }
            }
        }
        let base = app.history.len();
        if msg.role == "user"
            && let Some(offset) = cells
                .iter()
                .position(|cell| matches!(cell, HistoryCell::User { .. }))
        {
            message_to_cell.insert(message_index, base + offset);
        }
        app.extend_history(cells);
    }
    app.sync_context_references_from_session(&session.context_references, &message_to_cell);
    app.mark_history_updated();
    app.viewport.transcript_selection.clear();
    app.set_model_selection(session.metadata.model.clone());
    app.update_model_compaction_budget();
    apply_workspace_runtime_state(app, config, session.metadata.workspace.clone());
    app.session.total_tokens = u32::try_from(session.metadata.total_tokens).unwrap_or(u32::MAX);
    app.session.total_conversation_tokens = app.session.total_tokens;
    app.session.session_cost = session.metadata.cost.session_cost_usd;
    app.session.session_cost_cny = session.metadata.cost.session_cost_cny;
    app.session.subagent_cost = session.metadata.cost.subagent_cost_usd;
    app.session.subagent_cost_cny = session.metadata.cost.subagent_cost_cny;
    app.session.subagent_cost_event_seqs.clear();
    // Restore the high-water marks from persisted metadata so the
    // monotonic cost guarantee (#244) survives session restarts.
    // Take the max with the current totals — old sessions without
    // persisted high-water fields deserialise to 0.0 and fall back to
    // the restored total with no regression.
    let total_restored_usd = session.metadata.cost.total_usd();
    let total_restored_cny = session.metadata.cost.total_cny();
    app.session.displayed_cost_high_water = session
        .metadata
        .cost
        .displayed_cost_high_water_usd
        .max(total_restored_usd);
    app.session.displayed_cost_high_water_cny = session
        .metadata
        .cost
        .displayed_cost_high_water_cny
        .max(total_restored_cny);
    app.session.last_prompt_tokens = None;
    app.session.last_completion_tokens = None;
    app.session.last_output_throughput = None;
    app.session.last_prompt_cache_hit_tokens = None;
    app.session.last_prompt_cache_miss_tokens = None;
    app.session.last_reasoning_replay_tokens = None;
    // Accumulated token breakdown is per-runtime-session; reset on load.
    app.session.reset_token_breakdown();
    app.session.turn_cache_history.clear();
    // Restore cumulative turn duration so the footer "worked" chip
    // persists across session restarts (#2038).
    app.cumulative_turn_duration =
        std::time::Duration::from_secs(session.metadata.cumulative_turn_secs);
    app.current_session_id = Some(session.metadata.id.clone());
    app.session_artifacts = session.artifacts.clone();
    app.session_title = Some(session.metadata.title.clone());
    app.workspace_context = None;
    app.workspace_context_refreshed_at = None;
    if let Some(sp) = session.system_prompt.as_ref() {
        app.system_prompt = Some(SystemPrompt::Text(sp.clone()));
    } else {
        app.system_prompt = None;
    }
    let recovered = if let Some(draft) = recovered_draft {
        restore_recovered_retry_draft(app, draft);
        true
    } else {
        false
    };
    restore_plan_and_todo_state(app, &session.metadata.id);
    app.scroll_to_bottom();
    recovered
}

/// Restore persisted plan + todo (checklist) state for a resumed session.
///
/// Reads `<sessions_dir>/<id>.plan.json` and replays it into `app.plan_state`
/// / `app.todos`. Synchronous (`std::sync::Mutex::lock`) — never call across
/// an `.await` (ARCHITECTURE_STABILITY.md §8.3). Missing or corrupt plan files
/// are silently ignored so session loading is never blocked.
fn restore_plan_and_todo_state(app: &mut App, session_id: &str) {
    let Ok(manager) = SessionManager::default_location() else {
        return;
    };
    let Ok(Some(state)) = manager.load_plan_state(session_id) else {
        return;
    };
    if let Some(plan) = state.plan {
        if let Ok(mut guard) = app.plan_state.try_lock() {
            guard.apply_snapshot(plan);
        }
    }
    if let Some(todos) = state.todos {
        if let Ok(mut guard) = app.todos.try_lock() {
            guard.apply_snapshot(todos);
        }
    }
}

/// Derive a short display title from the API message list.
/// Skips the `<turn_meta>` block prepended by the engine and takes the first
/// real user-text block, truncated to 32 characters.
pub(crate) fn derive_session_title(messages: &[Message]) -> Option<String> {
    messages.iter().find(|m| m.role == "user").and_then(|m| {
        m.content.iter().find_map(|block| match block {
            ContentBlock::Text { text, .. } if !text.starts_with(TURN_META_PREFIX) => {
                let first_line = text.trim().lines().next().unwrap_or("").trim();
                if first_line.is_empty() {
                    return None;
                }
                let char_count = first_line.chars().count();
                let chars: String = first_line.chars().take(SESSION_TITLE_MAX_CHARS).collect();
                if char_count > SESSION_TITLE_MAX_CHARS {
                    Some(format!("{chars}…"))
                } else {
                    Some(chars)
                }
            }
            _ => None,
        })
    })
}

fn recover_interrupted_user_tail(messages: &[Message]) -> (Vec<Message>, Option<QueuedMessage>) {
    let mut recovered = messages.to_vec();
    let Some(last) = recovered.last() else {
        return (recovered, None);
    };
    if last.role != "user" {
        return (recovered, None);
    }
    let Some(display) = retry_display_from_user_message(last) else {
        return (recovered, None);
    };
    if looks_like_slash_command_input(&display) {
        return (recovered, None);
    }
    recovered.pop();
    (recovered, Some(QueuedMessage::new(display, None)))
}

fn retry_display_from_user_message(message: &Message) -> Option<String> {
    let text = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let display = compact_user_context_display(&text).trim().to_string();
    if display.is_empty() {
        None
    } else {
        Some(display)
    }
}

fn restore_recovered_retry_draft(app: &mut App, draft: QueuedMessage) {
    app.input.clone_from(&draft.display);
    app.cursor_position = app.input.chars().count();
    app.queued_draft = Some(draft);
    app.status_message = Some(
        "Recovered interrupted prompt as an editable draft; press Enter to retry.".to_string(),
    );
    app.needs_redraw = true;
}

fn compact_user_context_display(content: &str) -> String {
    content
        .split("\n\n---\n\nLocal context from @mentions:")
        .next()
        .unwrap_or(content)
        .to_string()
}
