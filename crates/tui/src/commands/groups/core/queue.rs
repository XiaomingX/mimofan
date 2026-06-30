//! Queue commands: queue list/edit/drop/clear

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::{Locale, MessageId, tr};
use crate::tui::app::App;

use super::CommandResult;

const PREVIEW_LIMIT: usize = 120;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "queue",
    aliases: &["queued"],
    usage: "/queue [list|send <n>|edit <n>|drop <n>|clear]",
    description_id: MessageId::CmdQueueDescription,
};

pub(in crate::commands) struct QueueCmd;

impl RegisterCommand for QueueCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        queue(app, arg)
    }
}

pub fn queue(app: &mut App, args: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;
    let arg = args.unwrap_or("").trim();
    if arg.is_empty() || arg.eq_ignore_ascii_case("list") {
        return list_queue(app);
    }

    let mut parts = arg.split_whitespace();
    let action = parts.next().unwrap_or("").to_lowercase();

    match action.as_str() {
        "edit" => edit_queue(app, parts.next()),
        "drop" | "remove" | "rm" => drop_queue(app, parts.next()),
        "clear" => clear_queue(app),
        _ => CommandResult::error(tr(locale, MessageId::CmdQueueUsage)),
    }
}

fn list_queue(app: &mut App) -> CommandResult {
    let locale = app.ui_locale;
    let mut lines = Vec::new();
    let queued = app.queued_message_count();

    if let Some(draft) = app.queued_draft.as_ref() {
        lines.push("Editing queued message:".to_string());
        lines.push(format!("- {}", truncate_preview(&draft.display)));
    }

    if queued == 0 {
        if lines.is_empty() {
            return CommandResult::message(tr(locale, MessageId::CmdQueueNoMessages));
        }
        return CommandResult::message(lines.join("\n"));
    }

    lines.push(tr(locale, MessageId::CmdQueueListHeader).replace("{count}", &queued.to_string()));
    for (idx, message) in app.queued_messages.iter().enumerate() {
        lines.push(format!(
            "{}. {}",
            idx + 1,
            truncate_preview(&message.display)
        ));
    }

    lines.push(tr(locale, MessageId::CmdQueueTip).to_string());

    CommandResult::message(lines.join("\n"))
}

fn edit_queue(app: &mut App, index: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;
    if app.queued_draft.is_some() {
        return CommandResult::error(tr(locale, MessageId::CmdQueueAlreadyEditing));
    }
    let index = match parse_index(index, locale) {
        Ok(index) => index,
        Err(err) => return CommandResult::error(err),
    };

    let Some(message) = app.remove_queued_message(index) else {
        return CommandResult::error(tr(locale, MessageId::CmdQueueNotFound));
    };

    app.input = message.display.clone();
    app.cursor_position = app.input.len();
    app.queued_draft = Some(message);
    let status =
        tr(locale, MessageId::CmdQueueEditingStatus).replace("{index}", &(index + 1).to_string());
    app.status_message = Some(status);

    CommandResult::message(
        tr(locale, MessageId::CmdQueueEditingMessage).replace("{index}", &(index + 1).to_string()),
    )
}

fn drop_queue(app: &mut App, index: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;
    let index = match parse_index(index, locale) {
        Ok(index) => index,
        Err(err) => return CommandResult::error(err),
    };

    if app.remove_queued_message(index).is_none() {
        return CommandResult::error(tr(locale, MessageId::CmdQueueNotFound));
    }

    CommandResult::message(
        tr(locale, MessageId::CmdQueueDropped).replace("{index}", &(index + 1).to_string()),
    )
}

fn clear_queue(app: &mut App) -> CommandResult {
    let locale = app.ui_locale;
    let queued = app.queued_message_count();
    let had_draft = app.queued_draft.take().is_some();
    app.queued_messages.clear();
    if queued == 0 && !had_draft {
        return CommandResult::message(tr(locale, MessageId::CmdQueueAlreadyEmpty));
    }

    CommandResult::message(tr(locale, MessageId::CmdQueueCleared))
}

fn parse_index(input: Option<&str>, locale: Locale) -> Result<usize, String> {
    let Some(input) = input else {
        return Err(tr(locale, MessageId::CmdQueueMissingIndex).to_string());
    };
    let raw = input
        .parse::<usize>()
        .map_err(|_| tr(locale, MessageId::CmdQueueIndexPositive).to_string())?;
    if raw == 0 {
        return Err(tr(locale, MessageId::CmdQueueIndexMin).to_string());
    }
    Ok(raw - 1)
}

fn truncate_preview(text: &str) -> String {
    if text.chars().count() <= PREVIEW_LIMIT {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(PREVIEW_LIMIT.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {}
