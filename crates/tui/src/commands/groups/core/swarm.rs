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
/// WhaleFlow/Fleet workers (#3218).
pub fn swarm(_app: &mut App, arg: Option<&str>) -> CommandResult {
    let (_max_depth, task) = match super::util::parse_depth_prefixed_arg(arg, 1) {
        Ok(parsed) => parsed,
        Err(message) => return CommandResult::error(message),
    };
    if !matches!(task.map(str::trim), Some(task) if !task.is_empty()) {
        return CommandResult::error(
            "Usage: /swarm [N] <task>\n\n\
             /swarm is currently gated. Use /goal for a persistent objective \
             or /agent for a single sub-agent while durable Fleet-backed \
             swarm workers are still landing.",
        );
    }
    CommandResult::error(
        "/swarm is gated in v0.8.61: prompt-only agent fanout is disabled until the durable Train-3 worker/goal re-dispatch substrate lands. Use /goal for the persistent objective or /agent [N] <task> for one bounded sub-agent.",
    )
}

#[cfg(test)]
mod tests {}
