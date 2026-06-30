//! Provider switching: flip between DeepSeek, hosted providers, and self-hosted
//! OpenAI-compatible DeepSeek V4 servers at runtime.
//!
//! `/provider` with no args opens the picker modal (#52). `/provider <name>`
//! keeps the v0.6.6 CLI form for muscle-memory + scripted use.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::config::{ApiProvider, canonical_model_id_for_provider, provider_passes_model_through};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction};

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "provider",
    aliases: &[],
    usage: "/provider [name] [model]",
    description_id: MessageId::CmdProviderDescription,
};

pub(in crate::commands) struct ProviderCmd;

impl RegisterCommand for ProviderCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        provider(app, arg)
    }
}

/// Switch or view the current LLM backend.
///
/// With no args, opens the picker modal. With `<provider> [model]`, performs
/// the switch directly (e.g. `/provider nim flash` lands on
/// `deepseek-ai/deepseek-v4-flash`). The optional model accepts shorthand
/// (`flash`, `pro`, `v4-flash`, `v4-pro`) or any normal provider model ID.
pub fn provider(app: &mut App, args: Option<&str>) -> CommandResult {
    let trimmed = args.map(str::trim).filter(|s| !s.is_empty());
    let Some(args) = trimmed else {
        return CommandResult::action(AppAction::OpenProviderPicker);
    };

    let mut parts = args.split_whitespace();
    let name = parts.next().unwrap_or("");
    let model_arg = parts.next();

    if name.eq_ignore_ascii_case("fallback") {
        return provider_fallback(app, model_arg);
    }

    let Some(target) = ApiProvider::parse(name) else {
        return CommandResult::error(format!(
            "Unknown provider '{name}'. Expected: {}.",
            ApiProvider::names_hint()
        ));
    };

    let model = match model_arg {
        None => None,
        Some(raw) => {
            // Expand provider shorthands (flash/pro, Xiaomi MiMo tts/omni, …)
            // uniformly, then either keep the id verbatim for providers that take
            // opaque/custom model tags, or resolve it to the canonical family id.
            // Families are treated equally: each resolves through its own
            // canonical map (DeepSeek, GLM via Z.ai/Zhipu, Kimi, MiniMax, …) and
            // an id matching none passes through unchanged — the upstream API is
            // the authority. Wire-id translation is deferred to the route
            // resolver at request time, so `/provider` stores canonical names.
            let expanded = expand_model_alias_for_provider(target, raw);
            if provider_passes_model_through(target) {
                Some(expanded)
            } else {
                match canonical_model_id_for_provider(target, &expanded) {
                    Some(canonical) => Some(canonical),
                    None => {
                        return CommandResult::error(format!(
                            "Invalid model '{raw}'. Provide a non-empty model id."
                        ));
                    }
                }
            }
        }
    };

    if target == app.api_provider && model.is_none() {
        return CommandResult::message(format!("Already on provider: {}", target.as_str()));
    }

    CommandResult::action(AppAction::SwitchProvider {
        provider: target,
        model,
    })
}

fn provider_fallback(app: &mut App, subcommand: Option<&str>) -> CommandResult {
    match subcommand {
        Some("reset") => {
            let Some((_, primary, _)) = app.fallback_chain_entries().first().copied() else {
                return CommandResult::message(
                    "No fallback providers configured. Add `fallback_providers` to your config.",
                );
            };
            CommandResult::with_message_and_action(
                format!(
                    "Fallback chain reset to primary provider: {}.",
                    primary.as_str()
                ),
                AppAction::SwitchProvider {
                    provider: primary,
                    model: None,
                },
            )
        }
        Some(other) => CommandResult::error(format!(
            "Unknown fallback command '{other}'. Usage: /provider fallback [reset]"
        )),
        None => {
            let entries = app.fallback_chain_entries();
            if entries.is_empty() {
                return CommandResult::message(
                    "No fallback providers configured. Add `fallback_providers` to your config.",
                );
            }

            let mut lines = vec![
                format!("Current provider: {}", app.api_provider.as_str()),
                "Fallback chain:".to_string(),
            ];
            for (index, provider, is_current) in entries {
                let role = if index == 0 { "primary" } else { "fallback" };
                let marker = if is_current { " <- current" } else { "" };
                lines.push(format!(
                    "  [{index}] {} ({role}){marker}",
                    provider.as_str()
                ));
            }
            if let Some(reason) = app.last_fallback_reason.as_deref() {
                lines.push(format!("Last fallback: {reason}"));
            }
            lines.push("Use `/provider fallback reset` to return to the primary provider.".into());
            CommandResult::message(lines.join("\n"))
        }
    }
}

fn expand_model_alias_for_provider(provider: ApiProvider, name: &str) -> String {
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    if matches!(provider, ApiProvider::XiaomiMimo) {
        return match lower.as_str() {
            "pro" | "mimo" => "mimo-v2.5-pro".to_string(),
            "ultraspeed" | "pro-ultraspeed" => "mimo-v2.5-pro-ultraspeed".to_string(),
            "text" | "omni" | "v2.5-omni" => "mimo-v2.5".to_string(),
            "tts" | "speech" | "mimo-tts" => "mimo-v2.5-tts".to_string(),
            "voicedesign" | "voice-design" | "mimo-voice-design" => {
                "mimo-v2.5-tts-voicedesign".to_string()
            }
            "voiceclone" | "voice-clone" | "mimo-voice-clone" => {
                "mimo-v2.5-tts-voiceclone".to_string()
            }
            // Not a shorthand: keep the id as typed (case preserved for custom
            // token-plan model ids).
            _ => trimmed.to_string(),
        };
    }

    match lower.as_str() {
        "pro" | "v4-pro" => "deepseek-v4-pro".to_string(),
        "flash" | "v4-flash" => "deepseek-v4-flash".to_string(),
        // Not a shorthand: keep the id as typed (case preserved for opaque
        // model tags on passthrough providers like HuggingFace).
        _ => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {}
