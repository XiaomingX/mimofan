//! paused command 子系统（从 ui 上帝文件切片）
use super::*;

fn paused_quarry_title(quarry: &str) -> &str {
    quarry
        .split(['\n', '\r'])
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("the paused command")
}

fn is_resume_message(message: &str) -> bool {
    let words: Vec<String> = message
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        return false;
    }
    let text = words.join(" ");
    let has_resume_verb = words
        .iter()
        .any(|word| matches!(word.as_str(), "continue" | "resume"));
    if !has_resume_verb {
        return false;
    }

    let blockers = [
        "do not continue",
        "do not resume",
        "don t continue",
        "don t resume",
        "dont continue",
        "dont resume",
        "not continue",
        "not resume",
        "continue yet",
        "resume yet",
        "will continue",
        "will resume",
        "continue tomorrow",
        "resume tomorrow",
        "continue later",
        "resume later",
    ];
    if blockers.iter().any(|blocker| text.contains(blocker)) {
        return false;
    }
    if matches!(
        words.first().map(String::as_str),
        Some("how" | "what" | "when" | "where" | "why")
    ) {
        return false;
    }

    if words.len() == 1 {
        return true;
    }

    let context_words = [
        "please", "now", "paused", "pause", "command", "task", "work", "request", "goal",
        "previous", "last", "same", "it", "that", "this", "go", "ahead",
    ];
    if words
        .iter()
        .any(|word| context_words.contains(&word.as_str()))
    {
        return true;
    }

    text.starts_with("can you continue")
        || text.starts_with("can you resume")
        || text.starts_with("could you continue")
        || text.starts_with("could you resume")
}

fn paused_command_note(title: &str, resume: bool) -> String {
    let instruction = if resume {
        "The user is resuming that paused command. Continue the paused command."
    } else {
        "The user is not resuming that paused command. Answer only the new message and do not continue the paused command."
    };
    format!(
        "\n\nmimofan paused custom slash command context:\n\
Paused custom slash command: {title}\n\
Paused command: {title}\n\
{instruction}"
    )
}

pub(crate) fn prepare_paused_command_message(
    app: &mut App,
    engine_handle: &EngineHandle,
    user_message: &str,
) -> Option<String> {
    if !app.paused && app.paused_quarry.is_none() {
        engine_handle.set_paused(false);
        return None;
    }

    engine_handle.set_paused(false);
    app.paused = false;

    let Some(quarry) = app
        .paused_quarry
        .clone()
        .or_else(|| app.hunt.quarry.clone())
    else {
        app.pausable = false;
        return None;
    };
    let title = paused_quarry_title(&quarry).to_string();
    if is_resume_message(user_message) {
        app.hunt.quarry = Some(app.paused_quarry.take().unwrap_or(quarry));
        app.pausable = true;
        Some(paused_command_note(&title, true))
    } else {
        app.hunt.quarry = None;
        app.hunt.tokens_used = 0;
        app.hunt.time_used_seconds = 0;
        app.hunt.continuation_count = 0;
        Some(paused_command_note(&title, false))
    }
}

pub(crate) fn pause_pausable_command(app: &mut App, engine_handle: &EngineHandle) {
    app.paused_quarry = app
        .paused_quarry
        .clone()
        .or_else(|| app.hunt.quarry.clone());
    app.hunt.quarry = None;
    app.hunt.tokens_used = 0;
    app.hunt.time_used_seconds = 0;
    app.hunt.continuation_count = 0;
    app.paused = true;
    app.pausable = true;
    engine_handle.set_paused(true);
    app.status_message = Some(
        "Request paused. Send `continue` or `resume` to continue, or Esc to cancel.".to_string(),
    );
}

pub(crate) fn clear_paused_command_state(app: &mut App, engine_handle: &EngineHandle) {
    app.pausable = false;
    app.paused = false;
    app.paused_quarry = None;
    engine_handle.set_paused(false);
}

