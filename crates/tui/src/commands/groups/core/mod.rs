//! Core command area: model/provider selection, help, navigation, and the
//! persistent RLM / sub-agent entry points.

mod agent;
mod anchor;
mod auto;
mod clear;
// This group dir intentionally has a `core.rs` child module with the same
// name. The module_inception allow is a permanent structure rationale, not
// migration scaffolding; see docs/architecture/command-dispatch.md.
#[allow(clippy::module_inception)]
mod core;
mod exit;
mod fast;
mod feedback;
mod fleet;
mod freeze;
mod grill;
mod help;
mod hf;
mod home;
mod hooks;
mod issue_monitor;
mod links;
mod model;
mod models;
mod plan;
mod profile;
mod provider;
pub mod queue;
mod rlm;
pub mod schedule;
mod stash;
mod subagents;
mod swarm;
mod translate;
pub mod util;
pub mod voice;
mod workspace;
mod yolo;

pub(in crate::commands) use self::core::reset_conversation_state;

use crate::commands::CommandResult;
use crate::commands::traits::{Command, CommandGroup, FunctionCommand, RegisterCommand};

pub struct CoreCommands;

impl CommandGroup for CoreCommands {
    fn commands(&self) -> Vec<Box<dyn Command>> {
        vec![
            Box::new(FunctionCommand::new(
                anchor::AnchorCmd::info(),
                anchor::AnchorCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                help::HelpCmd::info(),
                help::HelpCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                clear::ClearCmd::info(),
                clear::ClearCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                exit::ExitCmd::info(),
                exit::ExitCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                fast::FastCmd::info(),
                fast::FastCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                fast::NormalCmd::info(),
                fast::NormalCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                model::ModelCmd::info(),
                model::ModelCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                models::ModelsCmd::info(),
                models::ModelsCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                provider::ProviderCmd::info(),
                provider::ProviderCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                queue::QueueCmd::info(),
                queue::QueueCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                stash::StashCmd::info(),
                stash::StashCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                hooks::HooksCmd::info(),
                hooks::HooksCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                issue_monitor::MonitorCmd::info(),
                issue_monitor::MonitorCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                subagents::SubagentsCmd::info(),
                subagents::SubagentsCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                fleet::FleetCmd::info(),
                fleet::FleetCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                freeze::FreezeCmd::info(),
                freeze::FreezeCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                freeze::UnfreezeCmd::info(),
                freeze::UnfreezeCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                grill::GrillCmd::info(),
                grill::GrillCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                agent::AgentCmd::info(),
                agent::AgentCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                auto::AutoCmd::info(),
                auto::AutoCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                plan::PlanCmd::info(),
                plan::PlanCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                yolo::YoloCmd::info(),
                yolo::YoloCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                swarm::SwarmCmd::info(),
                swarm::SwarmCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                links::LinksCmd::info(),
                links::LinksCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                feedback::FeedbackCmd::info(),
                feedback::FeedbackCmd::execute,
            )),
            Box::new(FunctionCommand::new(hf::HfCmd::info(), hf::HfCmd::execute)),
            Box::new(FunctionCommand::new(
                home::HomeCmd::info(),
                home::HomeCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                workspace::WorkspaceCmd::info(),
                workspace::WorkspaceCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                profile::ProfileCmd::info(),
                profile::ProfileCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                rlm::RlmCmd::info(),
                rlm::RlmCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                translate::TranslateCmd::info(),
                translate::TranslateCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                voice::VoiceCmd::info(),
                voice::VoiceCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                voice::VoiceSendCmd::info(),
                voice::VoiceSendCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                voice::VoiceControlCmd::info(),
                voice::VoiceControlCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                schedule::NightCmd::info(),
                schedule::NightCmd::execute,
            )),
            Box::new(FunctionCommand::new(
                schedule::TimeCmd::info(),
                schedule::TimeCmd::execute,
            )),
        ]
    }
}
