//! `/grill-me` command.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction};
use crate::tui::history::HistoryCell;

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "grill-me",
    aliases: &["grill"],
    usage: "/grill-me <task>",
    description_id: MessageId::CmdGrillDescription,
};

pub(in crate::commands) struct GrillCmd;

impl RegisterCommand for GrillCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        grill(app, arg)
    }
}

pub fn grill(app: &mut App, args: Option<&str>) -> CommandResult {
    let target = args.unwrap_or("").trim();
    if target.is_empty() {
        return CommandResult::error("Usage: /grill-me <task>");
    }

    let instruction = include_str!("../../../prompts/grill_me.md").to_string();

    app.add_message(HistoryCell::System {
        content: "Starting interactive requirement clarification (/grill-me)...".to_string(),
    });
    app.active_skill = Some(instruction);

    CommandResult::action(AppAction::SendMessage(target.to_string()))
}
