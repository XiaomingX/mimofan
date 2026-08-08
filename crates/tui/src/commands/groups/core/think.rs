//! `/think` command — explicit reasoning-effort control.
//!
//! `/fast` and `/normal` move model *and* effort together as a coarse speed
//! preset. `/think` is the fine-grained counterpart: it sets the reasoning
//! effort tier on its own, leaving the active model untouched. This mirrors the
//! explicit thinking-budget control users expect from comparable assistants
//! (Claude Code's `think`/`ultrathink`, CodeBuddy's reasoning-effort setting).

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::tui::app::{App, AppAction, ReasoningEffort};

use super::CommandResult;

pub(in crate::commands) const THINK_INFO: CommandInfo = CommandInfo {
    name: "think",
    aliases: &[],
    usage: "/think [off|low|medium|high|max|auto]",
    description_id: crate::localization::MessageId::CmdThinkDescription,
};

/// Tiers offered by `/think`, in ascending order of effort.
///
/// `ReasoningEffort::from_setting` intentionally falls back to `Max` for any
/// unrecognised string, which is the right behavior for a config file but the
/// wrong one for an interactive command: a typo like `/think hgih` would
/// silently select the most expensive tier. So `/think` validates against this
/// list first and reports the accepted values instead of guessing.
const THINK_TIERS: &[&str] = &["off", "low", "medium", "high", "max", "auto"];

/// Aliases accepted in addition to the canonical tier names, so the common
/// spellings from other assistants keep working.
fn resolve_tier(raw: &str) -> Option<ReasoningEffort> {
    let normalized = raw.trim().to_ascii_lowercase();
    let canonical = match normalized.as_str() {
        "off" | "disabled" | "none" | "false" => "off",
        "low" | "minimal" => "low",
        "medium" | "mid" => "medium",
        "high" => "high",
        // `ultrathink` is Claude Code's spelling for the top tier; `xhigh` is
        // the provider-side wire name already understood elsewhere.
        "max" | "maximum" | "xhigh" | "ultrathink" | "ultracode" => "max",
        "auto" | "automatic" => "auto",
        _ => return None,
    };
    Some(ReasoningEffort::from_setting(canonical))
}

pub(in crate::commands) struct ThinkCmd;

impl RegisterCommand for ThinkCmd {
    fn info() -> &'static CommandInfo {
        &THINK_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        let Some(raw) = arg.map(str::trim).filter(|value| !value.is_empty()) else {
            return CommandResult::message(format!(
                "Reasoning effort: {}. Usage: /think <{}>",
                app.reasoning_effort.as_setting(),
                THINK_TIERS.join("|")
            ));
        };

        let Some(effort) = resolve_tier(raw) else {
            return CommandResult::message(format!(
                "Unknown reasoning tier '{raw}'. Accepted values: {}.",
                THINK_TIERS.join(", ")
            ));
        };

        let previous = app.reasoning_effort;
        if previous == effort {
            return CommandResult::message(format!(
                "Reasoning effort already {}.",
                effort.as_setting()
            ));
        }

        app.reasoning_effort = effort;
        app.last_effective_reasoning_effort = None;

        // `/think` deliberately does not touch the model, but the effective
        // effort still feeds the compaction budget, so refresh it the same way
        // `/fast` does.
        app.active_route_limits = None;
        app.update_model_compaction_budget();

        // Leaving fast mode's effort behind means the `/normal` restore point is
        // stale; drop the flag so the UI does not claim fast mode is still on.
        let mut suffix = String::new();
        if app.fast_mode_active {
            app.fast_mode_active = false;
            app.fast_saved_model = None;
            app.fast_saved_effort = None;
            suffix.push_str(" Fast mode cleared (effort set manually).");
        }

        CommandResult::with_message_and_action(
            format!(
                "🧠 Reasoning effort: {} → {}.{suffix}",
                previous.as_setting(),
                effort.as_setting()
            ),
            AppAction::UpdateCompaction(app.compaction_config()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_canonical_tiers() {
        assert_eq!(resolve_tier("off"), Some(ReasoningEffort::Off));
        assert_eq!(resolve_tier("low"), Some(ReasoningEffort::Low));
        assert_eq!(resolve_tier("medium"), Some(ReasoningEffort::Medium));
        assert_eq!(resolve_tier("high"), Some(ReasoningEffort::High));
        assert_eq!(resolve_tier("max"), Some(ReasoningEffort::Max));
        assert_eq!(resolve_tier("auto"), Some(ReasoningEffort::Auto));
    }

    #[test]
    fn resolves_aliases_and_is_case_insensitive() {
        assert_eq!(resolve_tier("  MINIMAL "), Some(ReasoningEffort::Low));
        assert_eq!(resolve_tier("UltraThink"), Some(ReasoningEffort::Max));
        assert_eq!(resolve_tier("xhigh"), Some(ReasoningEffort::Max));
        assert_eq!(resolve_tier("none"), Some(ReasoningEffort::Off));
    }

    #[test]
    fn rejects_unknown_tier_instead_of_defaulting_to_max() {
        // The bug this guards: `ReasoningEffort::from_setting` maps anything
        // unrecognised to `Max`, so a typo must not silently pick the most
        // expensive tier.
        assert_eq!(resolve_tier("hgih"), None);
        assert_eq!(resolve_tier(""), None);
    }
}
