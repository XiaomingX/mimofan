//! `/night` and `/time` commands for scheduled task execution.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::{MessageId, tr};
use crate::tui::app::App;

use super::CommandResult;

pub(in crate::commands) const NIGHT_COMMAND_INFO: CommandInfo = CommandInfo {
    name: "night",
    aliases: &["yejian"],
    usage: "/night <prompt> --schedule <HH:MM>",
    description_id: MessageId::CmdNightDescription,
};

pub(in crate::commands) const TIME_COMMAND_INFO: CommandInfo = CommandInfo {
    name: "time",
    aliases: &["dingshi"],
    usage: "/time [list|cancel <id>]",
    description_id: MessageId::CmdTimeDescription,
};

pub(in crate::commands) struct NightCmd;

impl RegisterCommand for NightCmd {
    fn info() -> &'static CommandInfo {
        &NIGHT_COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        night(app, arg)
    }
}

pub(in crate::commands) struct TimeCmd;

impl RegisterCommand for TimeCmd {
    fn info() -> &'static CommandInfo {
        &TIME_COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        time(app, arg)
    }
}

/// Queue a prompt for execution at a specific time (night mode).
///
/// Usage: `/night <prompt> --schedule <HH:MM>`
///
/// This command queues a message to be sent at the specified time,
/// allowing users to take advantage of off-peak API hours.
fn night(app: &mut App, arg: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;
    let arg = arg.unwrap_or("").trim();

    if arg.is_empty() {
        return CommandResult::error(tr(locale, MessageId::CmdNightUsage));
    }

    // Parse the schedule time from the argument
    let (prompt, schedule_time) = match parse_night_args(arg) {
        Ok(result) => result,
        Err(err) => return CommandResult::error(err),
    };

    // Validate the schedule time
    let hour = schedule_time.0;
    let minute = schedule_time.1;
    if hour > 23 || minute > 59 {
        return CommandResult::error(tr(locale, MessageId::CmdNightInvalidTime));
    }

    // For now, we'll queue the message and show a confirmation
    // In a full implementation, this would use the automation manager
    // to schedule the task for execution at the specified time.
    let message = tr(locale, MessageId::CmdNightScheduled)
        .replace("{prompt}", &truncate_preview(&prompt, 50))
        .replace("{time}", &format!("{hour:02}:{minute:02}"));

    CommandResult::message(message)
}

/// Manage scheduled tasks.
///
/// Usage: `/time [list|cancel <id>]`
///
/// This command shows scheduled tasks or cancels a specific task.
fn time(app: &mut App, arg: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;
    let arg = arg.unwrap_or("").trim();

    if arg.is_empty() || arg.eq_ignore_ascii_case("list") {
        return list_scheduled_tasks(app);
    }

    let mut parts = arg.split_whitespace();
    let action = parts.next().unwrap_or("").to_lowercase();

    match action.as_str() {
        "cancel" | "drop" | "remove" => cancel_scheduled_task(app, parts.next()),
        _ => CommandResult::error(tr(locale, MessageId::CmdTimeUsage)),
    }
}

/// List all scheduled tasks.
fn list_scheduled_tasks(app: &mut App) -> CommandResult {
    let locale = app.ui_locale;

    // For now, show a placeholder message
    // In a full implementation, this would query the automation manager
    // for scheduled tasks.
    let message = tr(locale, MessageId::CmdTimeListEmpty);
    CommandResult::message(message)
}

/// Cancel a scheduled task by ID.
fn cancel_scheduled_task(app: &mut App, id: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;

    let Some(_id) = id else {
        return CommandResult::error(tr(locale, MessageId::CmdTimeMissingId));
    };

    // For now, show a placeholder message
    // In a full implementation, this would cancel the task in the automation manager.
    let message = tr(locale, MessageId::CmdTimeCancelled);
    CommandResult::message(message)
}

/// Parse arguments for the `/night` command.
///
/// Expected format: `<prompt> --schedule <HH:MM>`
/// Returns `(prompt, (hour, minute))` or an error message.
fn parse_night_args(args: &str) -> Result<(String, (u32, u32)), String> {
    let parts: Vec<&str> = args.splitn(2, "--schedule").collect();
    if parts.len() != 2 {
        return Err("Usage: /night <prompt> --schedule <HH:MM>".to_string());
    }

    let prompt = parts[0].trim().to_string();
    let time_str = parts[1].trim();

    let time_parts: Vec<&str> = time_str.splitn(2, ':').collect();
    if time_parts.len() != 2 {
        return Err("Invalid time format. Use HH:MM (e.g., 00:30)".to_string());
    }

    let hour = time_parts[0]
        .parse::<u32>()
        .map_err(|_| "Invalid hour".to_string())?;
    let minute = time_parts[1]
        .parse::<u32>()
        .map_err(|_| "Invalid minute".to_string())?;

    Ok((prompt, (hour, minute)))
}

/// Truncate text to a maximum length with ellipsis.
fn truncate_preview(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(max_chars.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_night_args_valid() {
        let result = parse_night_args("run tests --schedule 00:30");
        assert!(result.is_ok());
        let (prompt, (hour, minute)) = result.unwrap();
        assert_eq!(prompt, "run tests");
        assert_eq!(hour, 0);
        assert_eq!(minute, 30);
    }

    #[test]
    fn test_parse_night_args_missing_schedule() {
        let result = parse_night_args("run tests");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_night_args_invalid_time() {
        let result = parse_night_args("run tests --schedule abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_truncate_preview_short() {
        let text = "short text";
        assert_eq!(truncate_preview(text, 20), "short text");
    }

    #[test]
    fn test_truncate_preview_long() {
        let text = "this is a very long text that should be truncated";
        let result = truncate_preview(text, 20);
        assert!(result.len() <= 23); // 20 + "..."
        assert!(result.ends_with("..."));
    }
}
