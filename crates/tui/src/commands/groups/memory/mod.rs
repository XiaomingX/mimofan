//! Memory command area: persistent memory and quick notes.

// This group dir intentionally has a `memory.rs` child module with the same
// name. The module_inception allow is a permanent structure rationale, not
// migration scaffolding; see docs/architecture/command-dispatch.md.
#[allow(clippy::module_inception)]
mod decision;
mod memory;
mod note;
#[cfg(feature = "vector-memory")]
mod vmemory;

use crate::commands::CommandResult;
use crate::commands::traits::{Command, CommandGroup, CommandInfo, FunctionCommand};
use crate::localization::MessageId;
use crate::tui::app::App;

pub struct MemoryCommands;

impl CommandGroup for MemoryCommands {
    fn commands(&self) -> Vec<Box<dyn Command>> {
        let mut cmds: Vec<Box<dyn Command>> = vec![
            Box::new(FunctionCommand::new(&NOTE_INFO, run_note)),
            Box::new(FunctionCommand::new(&MEMORY_INFO, run_memory)),
            Box::new(FunctionCommand::new(&DECISION_INFO, run_decision)),
        ];
        #[cfg(feature = "vector-memory")]
        cmds.push(Box::new(FunctionCommand::new(&VMEMORY_INFO, run_vmemory)));
        cmds
    }
}

static NOTE_INFO: CommandInfo = CommandInfo {
    name: "note",
    aliases: &[],
    usage: "/note [add|list|show|edit|remove|clear|path]",
    description_id: MessageId::CmdNoteDescription,
};
static MEMORY_INFO: CommandInfo = CommandInfo {
    name: "memory",
    aliases: &[],
    usage: "/memory [show|path|clear|edit|help]",
    description_id: MessageId::CmdMemoryDescription,
};
static DECISION_INFO: CommandInfo = CommandInfo {
    name: "decision",
    aliases: &[],
    usage: "/decision [list|show <id>|create <id> <title> <current>|revise <id> <new>|reverse <id> <why>|help]",
    description_id: MessageId::CmdMemoryDescription,
};

#[cfg(feature = "vector-memory")]
static VMEMORY_INFO: CommandInfo = CommandInfo {
    name: "vmemory",
    aliases: &[],
    usage: "/vmemory [status|remember <kind> <text>|query <text>|list|help]",
    description_id: MessageId::CmdVectorMemoryDescription,
};

fn run_registered(app: &mut App, name: &str, arg: Option<&str>) -> CommandResult {
    dispatch(app, name, arg).expect("registered memory command should dispatch")
}

fn run_note(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "note", arg)
}
fn run_memory(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "memory", arg)
}
fn run_decision(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "decision", arg)
}

#[cfg(feature = "vector-memory")]
fn run_vmemory(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "vmemory", arg)
}

pub(in crate::commands) fn dispatch(
    app: &mut App,
    command: &str,
    arg: Option<&str>,
) -> Option<CommandResult> {
    let result = match command {
        "memory" => memory::memory(app, arg),
        "note" => note::note(app, arg),
        "decision" => decision::decision(app, arg),
        #[cfg(feature = "vector-memory")]
        "vmemory" => vmemory::vmemory(app, arg),
        _ => return None,
    };
    Some(result)
}
