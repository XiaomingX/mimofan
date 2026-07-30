//! `/fast` and `/normal` commands — one-toggle speed mode.
//!
//! `/fast` switches to the cheap-tier model and low reasoning effort for
//! faster responses on simple tasks. `/normal` restores the previous state.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::model_routing::RouterCandidates;
use crate::tui::app::{App, AppAction, ReasoningEffort};

use super::CommandResult;

pub(in crate::commands) const FAST_INFO: CommandInfo = CommandInfo {
    name: "fast",
    aliases: &[],
    usage: "/fast",
    description_id: crate::localization::MessageId::CmdFastModeDescription,
};

pub(in crate::commands) struct FastCmd;

impl RegisterCommand for FastCmd {
    fn info() -> &'static CommandInfo {
        &FAST_INFO
    }

    fn execute(app: &mut App, _arg: Option<&str>) -> CommandResult {
        if app.fast_mode_active {
            return CommandResult::message("Already in /fast mode. Use /normal to exit.");
        }

        // Save current state for /normal restore.
        app.fast_saved_model = Some(app.model.clone());
        app.fast_saved_effort = Some(app.reasoning_effort);

        // Switch to cheap-tier model for the current provider.
        let candidates = match app.api_provider.as_str() {
            "deepseek" | "deepseek-chat" | "deepseek-api" => RouterCandidates::deepseek(),
            _ => RouterCandidates {
                big: app.model.clone(),
                cheap: None,
            },
        };

        let cheap_model = candidates.cheap_or_big().to_string();
        let model_changed = app.model != cheap_model;
        app.model = cheap_model.clone();
        app.auto_model = false;
        app.last_effective_model = None;

        // Set low reasoning effort for speed.
        app.reasoning_effort = ReasoningEffort::Low;
        app.last_effective_reasoning_effort = None;

        app.fast_mode_active = true;
        app.active_route_limits = None;
        app.update_model_compaction_budget();

        if model_changed {
            app.clear_model_scoped_telemetry();
        }

        let old_label = app.fast_saved_model.as_deref().unwrap_or("unknown");
        CommandResult::with_message_and_action(
            format!(
                "⚡ Fast mode ON: {old_label} → {cheap_model}, reasoning: low. Use /normal to restore."
            ),
            AppAction::UpdateCompaction(app.compaction_config()),
        )
    }
}

pub(in crate::commands) const NORMAL_INFO: CommandInfo = CommandInfo {
    name: "normal",
    aliases: &[],
    usage: "/normal",
    description_id: crate::localization::MessageId::CmdNormalModeDescription,
};

pub(in crate::commands) struct NormalCmd;

impl RegisterCommand for NormalCmd {
    fn info() -> &'static CommandInfo {
        &NORMAL_INFO
    }

    fn execute(app: &mut App, _arg: Option<&str>) -> CommandResult {
        if !app.fast_mode_active {
            return CommandResult::message("Not in /fast mode. Use /fast to enable speed mode.");
        }

        // Restore saved state.
        let restored_model = app
            .fast_saved_model
            .take()
            .unwrap_or_else(|| "auto".to_string());
        let restored_effort = app
            .fast_saved_effort
            .take()
            .unwrap_or(ReasoningEffort::High);

        let model_changed = app.model != restored_model;
        app.model = restored_model.clone();
        app.auto_model = restored_model == "auto";
        app.last_effective_model = None;

        app.reasoning_effort = restored_effort;
        app.last_effective_reasoning_effort = None;

        app.fast_mode_active = false;
        app.active_route_limits = None;
        app.update_model_compaction_budget();

        if model_changed {
            app.clear_model_scoped_telemetry();
        }

        CommandResult::with_message_and_action(
            format!("✅ Normal mode restored: model={restored_model}."),
            AppAction::UpdateCompaction(app.compaction_config()),
        )
    }
}
