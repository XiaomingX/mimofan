//! `/fleet` command.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction};

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "fleet",
    aliases: &["loadout", "party"],
    usage: "/fleet",
    description_id: MessageId::CmdFleetDescription,
};

pub(in crate::commands) struct FleetCmd;

impl RegisterCommand for FleetCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        match arg.map(str::trim).filter(|arg| !arg.is_empty()) {
            None
            | Some("setup" | "roles" | "role" | "profiles" | "profile" | "party" | "loadout") => {
                CommandResult::action(AppAction::OpenFleetSetup)
            }
            Some("status" | "workers" | "worker" | "agents" | "subagents" | "list") => {
                super::core::subagents(app)
            }
            Some("help" | "?") => CommandResult::message(
                "Usage: /fleet [setup|status]\n\n/fleet opens the setup flow. /fleet status shows Fleet worker status; /subagents is a compatibility shortcut for the same status view.",
            ),
            Some(other) => CommandResult::error(format!(
                "Unknown /fleet target '{other}'. Use `/fleet setup` or `/fleet status`."
            )),
        }
    }
}

#[cfg(test)]
mod tests {}
