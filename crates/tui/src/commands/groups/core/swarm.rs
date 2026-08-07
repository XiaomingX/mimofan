//! `/swarm` command - gated until durable Fleet-backed workers are available.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::App;

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "swarm",
    aliases: &["fanout", "qun"],
    usage: "/swarm [N] <task>",
    description_id: MessageId::CmdSwarmDescription,
};

pub(in crate::commands) struct SwarmCmd;

impl RegisterCommand for SwarmCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        swarm(app, arg)
    }
}

/// Gate the old prompt-only swarm fanout until it can route through durable
/// MimofanFlow/Fleet workers (#3218).
pub fn swarm(_app: &mut App, arg: Option<&str>) -> CommandResult {
    let (_max_depth, task) = match super::util::parse_depth_prefixed_arg(arg, 1) {
        Ok(parsed) => parsed,
        Err(message) => return CommandResult::error(message),
    };
    if !matches!(task.map(str::trim), Some(task) if !task.is_empty()) {
        return CommandResult::error(
            "Usage: /swarm [N] <task>\n\n\
             /swarm is currently gated (durable Fleet/Train-3 workers are still landing). \
             In the meantime, fan out your work with the tools that are live:\n\
             - /make-plan -x <task>  plan + autonomously execute the whole task in one shot\n\
             - /do auto              run every pending checklist step end-to-end\n\
             - /agent [N] <task>     spawn one bounded sub-agent for a single task",
        );
    }
    CommandResult::error(
        "/swarm is gated: prompt-only agent fanout is disabled until the durable Train-3 worker/goal \
         re-dispatch substrate lands. Today you can fan out with the live tools instead:\n\
         - /make-plan -x <task>  one-click plan + autonomous execution\n\
         - /do auto              drive an existing checklist to completion\n\
         - /agent [N] <task>     a single bounded sub-agent\n\
         Use /goal for a persistent objective that re-dispatches across turns.",
    )
}
