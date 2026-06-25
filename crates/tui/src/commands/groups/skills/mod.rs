//! Skills command area: listing and running skills, review, and restore.

mod restore;
mod review;
// This group dir intentionally has a `skills.rs` child module with the same
// name. The module_inception allow is a permanent structure rationale, not
// migration scaffolding; see docs/architecture/command-dispatch.md.
#[allow(clippy::module_inception)]
mod skills;

pub(in crate::commands) use self::skills::run_skill_by_name;

use crate::commands::CommandResult;
use crate::commands::traits::{Command, CommandGroup, CommandInfo, FunctionCommand};
use crate::localization::MessageId;
use crate::tui::app::App;

pub struct SkillsCommands;

impl CommandGroup for SkillsCommands {
    fn commands(&self) -> Vec<Box<dyn Command>> {
        vec![
            Box::new(FunctionCommand::new(&SKILLS_INFO, run_skills)),
            Box::new(FunctionCommand::new(&SKILL_INFO, run_skill)),
            Box::new(FunctionCommand::new(&REVIEW_INFO, run_review)),
            Box::new(FunctionCommand::new(&RESTORE_INFO, run_restore)),
        ]
    }
}

static SKILLS_INFO: CommandInfo = CommandInfo {
    name: "skills",
    aliases: &["jinengliebiao"],
    usage: "/skills [--remote|sync|<prefix>]",
    description_id: MessageId::CmdSkillsDescription,
};
static SKILL_INFO: CommandInfo = CommandInfo {
    name: "skill",
    aliases: &["jineng"],
    usage: "/skill <name|install <spec>|update <name>|uninstall <name>|trust <name>>",
    description_id: MessageId::CmdSkillDescription,
};
static REVIEW_INFO: CommandInfo = CommandInfo {
    name: "review",
    aliases: &["shencha"],
    usage: "/review <target>",
    description_id: MessageId::CmdReviewDescription,
};
static RESTORE_INFO: CommandInfo = CommandInfo {
    name: "restore",
    aliases: &[],
    usage: "/restore [N|list [N]]",
    description_id: MessageId::CmdRestoreDescription,
};

fn run_registered(app: &mut App, name: &str, arg: Option<&str>) -> CommandResult {
    dispatch(app, name, arg).expect("registered skills command should dispatch")
}

fn run_skills(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "skills", arg)
}
fn run_skill(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "skill", arg)
}
fn run_review(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "review", arg)
}
fn run_restore(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "restore", arg)
}

pub(in crate::commands) fn dispatch(
    app: &mut App,
    command: &str,
    arg: Option<&str>,
) -> Option<CommandResult> {
    let result = match command {
        "skills" | "jinengliebiao" => skills::list_skills(app, arg),
        "skill" | "jineng" => skills::run_skill(app, arg),
        "review" | "shencha" => review::review(app, arg),
        "restore" => restore::restore(app, arg),
        _ => return None,
    };
    Some(result)
}
