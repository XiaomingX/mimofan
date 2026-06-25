//! Project command area: workspace bootstrap, LSP wiring, sharing, and goals.

mod goal;
mod init;
pub mod share;

use crate::commands::CommandResult;
use crate::commands::traits::{Command, CommandGroup, CommandInfo, FunctionCommand};
use crate::localization::MessageId;
use crate::tui::app::App;

pub struct ProjectCommands;

impl CommandGroup for ProjectCommands {
    fn commands(&self) -> Vec<Box<dyn Command>> {
        vec![
            Box::new(FunctionCommand::new(&INIT_INFO, run_init)),
            Box::new(FunctionCommand::new(&LSP_INFO, run_lsp)),
            Box::new(FunctionCommand::new(&SHARE_INFO, run_share)),
            Box::new(FunctionCommand::new(&GOAL_INFO, run_goal)),
        ]
    }
}

static INIT_INFO: CommandInfo = CommandInfo {
    name: "init",
    aliases: &[],
    usage: "/init",
    description_id: MessageId::CmdInitDescription,
};
static LSP_INFO: CommandInfo = CommandInfo {
    name: "lsp",
    aliases: &[],
    usage: "/lsp [on|off|status]",
    description_id: MessageId::CmdLspDescription,
};
static SHARE_INFO: CommandInfo = CommandInfo {
    name: "share",
    aliases: &[],
    usage: "/share",
    description_id: MessageId::CmdShareDescription,
};
static GOAL_INFO: CommandInfo = CommandInfo {
    name: "goal",
    aliases: &["hunt", "mubiao", "狩猎"],
    usage: "/goal [objective|clear|pause|resume|complete|blocked] [budget: N]",
    description_id: MessageId::CmdGoalDescription,
};

fn run_registered(app: &mut App, name: &str, arg: Option<&str>) -> CommandResult {
    dispatch(app, name, arg).expect("registered project command should dispatch")
}

fn run_init(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "init", arg)
}
fn run_lsp(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "lsp", arg)
}
fn run_share(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "share", arg)
}
fn run_goal(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "goal", arg)
}

pub(in crate::commands) fn dispatch(
    app: &mut App,
    command: &str,
    arg: Option<&str>,
) -> Option<CommandResult> {
    let result = match command {
        "init" => init::init(app),
        "lsp" => super::config::config::lsp_command(app, arg),
        "share" => share::share(app, arg),
        "goal" | "hunt" | "mubiao" | "狩猎" => goal::hunt(app, arg),
        _ => return None,
    };
    Some(result)
}
