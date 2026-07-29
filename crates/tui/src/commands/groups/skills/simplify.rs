//! `/simplify` command.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction};
use crate::tui::history::HistoryCell;

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "simplify",
    aliases: &[],
    usage: "/simplify <target>",
    description_id: MessageId::CmdSimplifyDescription,
};

pub(in crate::commands) struct SimplifyCmd;

impl RegisterCommand for SimplifyCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        simplify(app, arg)
    }
}

pub fn simplify(app: &mut App, args: Option<&str>) -> CommandResult {
    let target = args.unwrap_or("").trim();
    if target.is_empty() {
        return CommandResult::error("Usage: /simplify <target>");
    }

    let instruction = include_str!("../../../prompts/simplify.md").to_string();

    app.add_message(HistoryCell::System {
        content: "Activated code simplification mode (/simplify)...".to_string(),
    });
    app.active_skill = Some(instruction);

    CommandResult::action(AppAction::SendMessage(target.to_string()))
}
