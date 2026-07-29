//! Project command area: workspace bootstrap, LSP wiring, sharing, and goals.

mod do_cmd;
mod goal;
mod init;
mod make_plan;
pub mod share;

use crate::commands::CommandResult;
use crate::commands::traits::{
    Command, CommandGroup, CommandInfo, FunctionCommand, RegisterCommand,
};
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
            Box::new(FunctionCommand::new(&MAKE_PLAN_INFO, run_make_plan)),
            Box::new(FunctionCommand::new(&DO_INFO, run_do)),
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
static MAKE_PLAN_INFO: CommandInfo = CommandInfo {
    name: "make-plan",
    aliases: &["makeplan", "mp"],
    usage: "/make-plan <task_description>",
    description_id: MessageId::CmdMakePlanDescription,
};
static DO_INFO: CommandInfo = CommandInfo {
    name: "do",
    aliases: &[],
    usage: "/do [step_id|all|next]",
    description_id: MessageId::CmdDoDescription,
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
fn run_make_plan(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "make-plan", arg)
}
fn run_do(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "do", arg)
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
        "make-plan" | "makeplan" | "mp" => make_plan::MakePlanCmd::execute(app, arg),
        "do" => do_cmd::DoCmd::execute(app, arg),
        _ => return None,
    };
    Some(result)
}
