//! Core commands: help, clear, exit, model

use std::fmt::Write;
use std::path::PathBuf;

use crate::config::{
    ApiProvider, COMMON_DEEPSEEK_MODELS, normalize_custom_model_id,
    normalize_model_name_for_provider,
};
use crate::localization::{MessageId, tr};
use crate::route_runtime::resolve_route_candidate;
use crate::tui::app::{App, AppAction, AppMode, ReasoningEffort};
use crate::tui::views::{HelpView, ModalKind, SubAgentsView, subagent_view_agents};

use super::CommandResult;

/// Show help information
pub fn help(app: &mut App, topic: Option<&str>) -> CommandResult {
    if let Some(topic) = topic {
        // Show help for specific command
        if let Some(cmd) = crate::commands::get_command_info(topic) {
            let mut help = format!(
                "{}\n\n  {}\n\n  {} {}",
                cmd.name,
                cmd.description_for(app.ui_locale),
                tr(app.ui_locale, MessageId::HelpUsageLabel),
                cmd.usage
            );
            if !cmd.aliases.is_empty() {
                let _ = write!(
                    help,
                    "\n  {} {}",
                    tr(app.ui_locale, MessageId::HelpAliasesLabel),
                    cmd.aliases.join(", ")
                );
            }
            return CommandResult::message(help);
        }
        return CommandResult::error(
            tr(app.ui_locale, MessageId::HelpUnknownCommand).replace("{topic}", topic),
        );
    }

    // Show help overlay
    if app.view_stack.top_kind() != Some(ModalKind::Help) {
        app.view_stack.push(HelpView::new_for_locale(app.ui_locale));
    }
    CommandResult::ok()
}

/// Clear conversation history
pub fn clear(app: &mut App) -> CommandResult {
    let todos_cleared = reset_conversation_state(app);
    app.current_session_id = None;
    let locale = app.ui_locale;
    let message = if todos_cleared {
        tr(locale, MessageId::ClearConversation).to_string()
    } else {
        tr(locale, MessageId::ClearConversationBusy).to_string()
    };
    CommandResult::with_message_and_action(
        message,
        AppAction::SyncSession {
            session_id: None,
            messages: Vec::new(),
            system_prompt: None,
            model: app.model.clone(),
            workspace: app.workspace.clone(),
        },
    )
}

/// Reset the active conversation without choosing the next session id.
pub(crate) fn reset_conversation_state(app: &mut App) -> bool {
    app.clear_history();
    app.mark_history_updated();
    app.api_messages.clear();
    app.system_prompt = None;
    app.viewport.transcript_selection.clear();
    app.queued_messages.clear();
    app.queued_draft = None;
    app.session.total_tokens = 0;
    app.session.total_conversation_tokens = 0;
    app.session.reset_token_breakdown();
    app.session.session_cost = 0.0;
    app.session.session_cost_cny = 0.0;
    app.session.subagent_cost = 0.0;
    app.session.subagent_cost_cny = 0.0;
    app.session.subagent_cost_event_seqs.clear();
    app.session.displayed_cost_high_water = 0.0;
    app.session.displayed_cost_high_water_cny = 0.0;
    let todos_cleared = app.clear_todos();
    app.tool_log.clear();
    app.tool_cells.clear();
    app.tool_details_by_cell.clear();
    app.exploring_entries.clear();
    app.ignored_tool_calls.clear();
    app.pending_tool_uses.clear();
    app.last_exec_wait_command = None;
    app.session.last_prompt_tokens = None;
    app.session.last_completion_tokens = None;
    app.session.last_output_throughput = None;
    app.session.last_prompt_cache_hit_tokens = None;
    app.session.last_prompt_cache_miss_tokens = None;
    app.session.last_reasoning_replay_tokens = None;
    app.session.turn_cache_history.clear();
    app.session.last_cache_inspection = None;
    app.session.last_warmup_key = None;
    app.session.last_tool_catalog = None;
    app.session.last_base_url = None;
    todos_cleared
}

/// Exit the application
pub fn exit() -> CommandResult {
    CommandResult::action(AppAction::Quit)
}

/// Switch or view current model. With no argument, open the two-pane
/// picker (Pro/Flash + thinking effort) per #39 — gives users a discoverable
/// way to flip both knobs without memorising the docs.
pub fn model(app: &mut App, model_name: Option<&str>) -> CommandResult {
    if let Some(name) = model_name {
        if name.trim().eq_ignore_ascii_case("auto") {
            let old_model = app.model_display_label();
            let model_changed = !app.auto_model || app.model != "auto";
            app.auto_model = true;
            app.model = "auto".to_string();
            app.last_effective_model = None;
            app.reasoning_effort = ReasoningEffort::Auto;
            app.last_effective_reasoning_effort = None;
            app.active_route_limits = None;
            app.update_model_compaction_budget();
            if model_changed {
                app.clear_model_scoped_telemetry();
            } else {
                app.session.last_prompt_tokens = None;
                app.session.last_completion_tokens = None;
                app.session.last_output_throughput = None;
            }
            app.provider_models
                .insert(app.api_provider.as_str().to_string(), "auto".to_string());
            let persist_warning =
                provider_model_selection_persist_warning(app.api_provider, "auto");
            let mut message = tr(app.ui_locale, MessageId::ModelChanged)
                .replace("{old}", &old_model)
                .replace("{new}", "auto");
            if let Some(warning) = persist_warning {
                message.push_str(&warning);
            }
            return CommandResult::with_message_and_action(
                message,
                AppAction::UpdateCompaction(app.compaction_config()),
            );
        }
        let model_id = if app.accepts_custom_model_ids() {
            let Some(model_id) = normalize_custom_model_id(name) else {
                return CommandResult::error(format!(
                    "Invalid model '{name}'. Expected a non-empty model ID."
                ));
            };
            model_id
        } else {
            let Some(model_id) = normalize_model_name_for_provider(app.api_provider, name) else {
                return CommandResult::error(format!(
                    "Invalid model '{name}'. Expected auto or a model for the active provider. Common DeepSeek models: {}",
                    COMMON_DEEPSEEK_MODELS.join(", ")
                ));
            };
            model_id
        };
        let strict_direct_custom_endpoint = app.accepts_custom_model_ids()
            && matches!(
                app.api_provider,
                ApiProvider::XiaomiMimo
            );
        let route_limits = if strict_direct_custom_endpoint {
            None
        } else {
            match resolve_route_candidate(app.api_provider, Some(&model_id), None, None) {
                Ok(candidate) => Some(candidate.limits),
                Err(reason) => return CommandResult::error(reason),
            }
        };
        let old_model = app.model_display_label();
        let model_changed = app.auto_model || app.model != model_id;
        app.set_model_selection(model_id.clone());
        if let Some(limits) = route_limits {
            app.set_active_route_limits(limits);
        } else {
            app.active_route_limits = None;
        }
        app.update_model_compaction_budget();
        if model_changed {
            app.clear_model_scoped_telemetry();
        } else {
            app.session.last_prompt_tokens = None;
            app.session.last_completion_tokens = None;
            app.session.last_output_throughput = None;
        }
        app.provider_models
            .insert(app.api_provider.as_str().to_string(), model_id.clone());
        let persist_warning = provider_model_selection_persist_warning(app.api_provider, &model_id);
        let mut message = tr(app.ui_locale, MessageId::ModelChanged)
            .replace("{old}", &old_model)
            .replace("{new}", &model_id);
        if let Some(warning) = persist_warning {
            message.push_str(&warning);
        }
        CommandResult::with_message_and_action(
            message,
            AppAction::UpdateCompaction(app.compaction_config()),
        )
    } else {
        CommandResult::action(AppAction::OpenModelPicker)
    }
}

fn provider_model_selection_persist_warning(provider: ApiProvider, model: &str) -> Option<String> {
    crate::settings::Settings::persist_provider_model_selection(provider, model)
        .err()
        .map(|err| format!(" (not persisted: {err})"))
}

/// Fetch and list available models from the configured API endpoint.
pub fn models(_app: &mut App) -> CommandResult {
    CommandResult::action(AppAction::FetchModels)
}

/// List Fleet worker status from the engine.
pub fn subagents(app: &mut App) -> CommandResult {
    if app.view_stack.top_kind() != Some(ModalKind::SubAgents) {
        let agents = subagent_view_agents(app, &app.subagent_cache);
        app.view_stack.push(SubAgentsView::new(agents));
    }
    app.status_message = Some(tr(app.ui_locale, MessageId::SubagentsFetching).to_string());
    CommandResult::action(AppAction::ListSubAgents)
}

/// Switch to a configured profile.
pub fn profile_switch(_app: &mut App, arg: Option<&str>) -> CommandResult {
    let profile_name = match arg {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => {
            return CommandResult::error(
                "Usage: /profile <name>\n\nSwitch to a named config profile. Profiles are defined in ~/.mimofan/config.toml under [profiles] sections.",
            );
        }
    };
    CommandResult::with_message_and_action(
        format!("Switching to profile '{profile_name}'..."),
        AppAction::SwitchProfile {
            profile: profile_name,
        },
    )
}

pub fn workspace_switch(app: &mut App, arg: Option<&str>) -> CommandResult {
    let Some(raw_path) = arg.map(str::trim).filter(|path| !path.is_empty()) else {
        return CommandResult::message(format!("Current workspace: {}", app.workspace.display()));
    };

    let expanded = match expand_workspace_path(raw_path) {
        Ok(path) => path,
        Err(message) => return CommandResult::error(message),
    };
    let candidate = if expanded.is_absolute() {
        expanded
    } else {
        app.workspace.join(expanded)
    };

    if !candidate.exists() {
        return CommandResult::error(format!("Workspace does not exist: {}", candidate.display()));
    }
    if !candidate.is_dir() {
        return CommandResult::error(format!(
            "Workspace is not a directory: {}",
            candidate.display()
        ));
    }

    let workspace = candidate.canonicalize().unwrap_or(candidate);
    CommandResult::with_message_and_action(
        format!("Switching workspace to {}...", workspace.display()),
        AppAction::SwitchWorkspace { workspace },
    )
}

fn expand_workspace_path(path: &str) -> Result<PathBuf, String> {
    if path == "~" {
        return dirs::home_dir().ok_or_else(|| "Could not resolve home directory".to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        let home =
            dirs::home_dir().ok_or_else(|| "Could not resolve home directory".to_string())?;
        return Ok(home.join(rest));
    }
    Ok(PathBuf::from(path))
}

struct ProviderLinkInfo {
    key_url: Option<&'static str>,
    docs_url: &'static str,
    note: &'static str,
}

fn provider_link_info(provider_id: &str) -> ProviderLinkInfo {
    match provider_id {
        "deepseek" => ProviderLinkInfo {
            key_url: Some("https://platform.deepseek.com/api_keys"),
            docs_url: "https://api-docs.deepseek.com/",
            note: "Create an API key in the DeepSeek platform console.",
        },
        "nvidia-nim" => ProviderLinkInfo {
            key_url: Some("https://build.nvidia.com/settings/api-keys"),
            docs_url: "https://build.nvidia.com/explore/discover",
            note: "NVIDIA NIM keys are managed from the NVIDIA build console.",
        },
        "openai" => ProviderLinkInfo {
            key_url: Some("https://platform.openai.com/api-keys"),
            docs_url: "https://platform.openai.com/docs/api-reference",
            note: "Use this for OpenAI or compatible endpoints that share OpenAI-style auth.",
        },
        "atlascloud" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://atlascloud.ai/docs/en/api-keys",
            note: "Atlas Cloud documents API key creation in its API Keys guide.",
        },
        "wanjie-ark" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://platform.lingyiwanwu.com/docs",
            note: "Use the Wanjie/01.AI platform console for provider credentials.",
        },
        "volcengine" => ProviderLinkInfo {
            key_url: Some("https://console.volcengine.com/ark/apiKey"),
            docs_url: "https://www.volcengine.com/docs/82379/1541594",
            note: "Volcengine Ark API keys are managed in the Ark console.",
        },
        "openrouter" => ProviderLinkInfo {
            key_url: Some("https://openrouter.ai/settings/keys"),
            docs_url: "https://openrouter.ai/docs/api/reference/authentication",
            note: "OpenRouter keys can include app credit limits and model routing controls.",
        },
        "xiaomi-mimo" => ProviderLinkInfo {
            key_url: Some("https://platform.xiaomimimo.com/token-plan"),
            docs_url: "https://mimo.mi.com/docs/en-US/tokenplan/Token%20Plan/subscription",
            note: "Token Plan keys use the base URL shown on the Xiaomi MiMo Token Plan page.",
        },
        "novita" => ProviderLinkInfo {
            key_url: Some("https://novita.ai/en/settings/key-management"),
            docs_url: "https://novita.ai/docs/guides/quickstart",
            note: "Novita keys are managed from Key Management in account settings.",
        },
        "fireworks" => ProviderLinkInfo {
            key_url: Some("https://fireworks.ai/api-keys"),
            docs_url: "https://docs.fireworks.ai/getting-started/quickstart",
            note: "Create a Fireworks API key before exporting FIREWORKS_API_KEY.",
        },
        "siliconflow" => ProviderLinkInfo {
            key_url: Some("https://cloud.siliconflow.com/account/ak"),
            docs_url: "https://docs.siliconflow.com/en/userguide/quickstart",
            note: "Use the global SiliconFlow console unless your route is the China endpoint.",
        },
        "siliconflow-CN" => ProviderLinkInfo {
            key_url: Some("https://cloud.siliconflow.cn/account/ak"),
            docs_url: "https://docs.siliconflow.cn/en/userguide/quickstart",
            note: "Use the China SiliconFlow console for the China endpoint.",
        },
        "arcee" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.arcee.ai/other/create-your-first-api-key",
            note: "Arcee documents key creation from the platform API Keys page.",
        },
        "moonshot" => ProviderLinkInfo {
            key_url: Some("https://platform.kimi.ai/console/api-keys"),
            docs_url: "https://platform.kimi.ai/docs/api/overview",
            note: "Moonshot/Kimi keys are managed in the Kimi Open Platform console.",
        },
        "sglang" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.sglang.ai/",
            note: "Self-hosted SGLang usually needs a local base URL, not a hosted token.",
        },
        "vllm" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.vllm.ai/en/stable/serving/openai_compatible_server/",
            note: "Self-hosted vLLM usually needs a local base URL, not a hosted token.",
        },
        "ollama" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.ollama.com/api",
            note: "Local Ollama does not require an API key by default.",
        },
        "huggingface" => ProviderLinkInfo {
            key_url: Some("https://huggingface.co/settings/tokens"),
            docs_url: "https://huggingface.co/docs/hub/en/security-tokens",
            note: "Use a scoped Hugging Face access token.",
        },
        "together" => ProviderLinkInfo {
            key_url: Some("https://api.together.ai/settings/api-keys"),
            docs_url: "https://docs.together.ai/docs/api-keys-authentication",
            note: "Together API keys are project-scoped.",
        },
        "openai-codex" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://developers.openai.com/codex/",
            note: "This route uses Codex/ChatGPT auth instead of a normal provider API key.",
        },
        "anthropic" => ProviderLinkInfo {
            key_url: Some("https://console.anthropic.com/settings/keys"),
            docs_url: "https://docs.anthropic.com/en/api/overview",
            note: "Create Claude API keys from the Anthropic Console.",
        },
        "zai" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.z.ai/api-reference/introduction",
            note: "Create or manage Z.ai API keys from the API Keys page linked in the docs.",
        },
        "stepfun" => ProviderLinkInfo {
            key_url: Some("https://platform.stepfun.ai/"),
            docs_url: "https://platform.stepfun.ai/docs/en/quickstart/overview",
            note: "Open Account Management > Interface Keys in the StepFun console.",
        },
        "minimax" => ProviderLinkInfo {
            key_url: Some(
                "https://platform.minimax.io/user-center/basic-information/interface-key",
            ),
            docs_url: "https://platform.minimax.io/docs/api-reference/api-overview",
            note: "MiniMax has separate pay-as-you-go API keys and Token Plan subscription keys.",
        },
        "deepinfra" => ProviderLinkInfo {
            key_url: Some("https://deepinfra.com/dash/api_keys"),
            docs_url: "https://docs.deepinfra.com/quickstart",
            note: "Create DeepInfra API keys from the dashboard.",
        },
        _ => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://mimofan.dev/docs/providers",
            note: "Use the provider console for credentials, then configure the matching env var.",
        },
    }
}

/// Show provider dashboard, token, and docs links.
pub fn deepseek_links(app: &mut App) -> CommandResult {
    let locale = app.ui_locale;
    let active_provider = app.api_provider.as_str();
    let mut message = format!(
        "{}\n─────────────────────────────\n",
        tr(locale, MessageId::LinksTitle)
    );

    for provider in mimofan_config::provider::providers_sorted_for_display() {
        let links = provider_link_info(provider.id());
        let active_marker = if provider.id() == active_provider {
            " <- current"
        } else {
            ""
        };
        let _ = writeln!(
            message,
            "\n{} ({}){}",
            provider.display_name(),
            provider.id(),
            active_marker
        );
        if let Some(key_url) = links.key_url {
            let _ = writeln!(
                message,
                "{} {}",
                tr(locale, MessageId::LinksDashboard),
                key_url
            );
        } else {
            let _ = writeln!(
                message,
                "{} {}",
                tr(locale, MessageId::LinksDashboard),
                links.note
            );
        }
        let _ = writeln!(
            message,
            "{}      {}",
            tr(locale, MessageId::LinksDocs),
            links.docs_url
        );
        let env_vars = provider.env_vars();
        if env_vars.is_empty() {
            let _ = writeln!(message, "Env: none");
        } else {
            let _ = writeln!(message, "Env: {}", env_vars.join(", "));
        }
    }

    let _ = writeln!(message, "\n{}", tr(locale, MessageId::LinksTip));
    CommandResult::message(message)
}

/// Show home dashboard with stats and quick actions
pub fn home_dashboard(app: &mut App) -> CommandResult {
    let locale = app.ui_locale;
    let mut stats = String::new();

    // Basic info
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeDashboardTitle));
    let _ = writeln!(stats, "============================================");

    // Model & mode
    let _ = writeln!(
        stats,
        "{}      {}",
        tr(locale, MessageId::HomeModel),
        app.model
    );
    let _ = writeln!(
        stats,
        "{}       {}",
        tr(locale, MessageId::HomeMode),
        app.mode.label()
    );
    let _ = writeln!(
        stats,
        "{}  {}",
        tr(locale, MessageId::HomeWorkspace),
        app.workspace.display()
    );

    // Session stats
    let history_count = app.history.len();
    let total_tokens = app.session.total_conversation_tokens;
    let queued_messages = app.queued_messages.len();
    let _ = writeln!(
        stats,
        "{}    {} messages",
        tr(locale, MessageId::HomeHistory),
        history_count
    );
    let _ = writeln!(
        stats,
        "{}     {} (session)",
        tr(locale, MessageId::HomeTokens),
        total_tokens
    );
    if queued_messages > 0 {
        let _ = writeln!(
            stats,
            "{}     {} messages",
            tr(locale, MessageId::HomeQueued),
            queued_messages
        );
    }

    // Fleet role workers
    let subagent_count = app.subagent_cache.len();
    if subagent_count > 0 {
        let _ = writeln!(
            stats,
            "{} {} active",
            tr(locale, MessageId::HomeSubagents),
            subagent_count
        );
    }

    // Active skill
    if let Some(skill) = &app.active_skill {
        let _ = writeln!(
            stats,
            "{}      {} (active)",
            tr(locale, MessageId::HomeSkill),
            skill
        );
    }

    // Quick actions section
    let _ = writeln!(stats, "\n{}", tr(locale, MessageId::HomeQuickActions));
    let _ = writeln!(stats, "--------------------------------------------");
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeQuickLinks));
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeQuickSkills));
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeQuickConfig));
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeQuickSettings));
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeQuickModel));
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeQuickSubagents));
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeQuickTaskList));
    let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeQuickHelp));

    // Mode-specific tips
    let _ = writeln!(stats, "\n{}", tr(locale, MessageId::HomeModeTips));
    let _ = writeln!(stats, "--------------------------------------------");
    match app.mode {
        AppMode::Agent => {
            let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeAgentModeTip));
            let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeAgentModeReviewTip));
            let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeAgentModeYoloTip));
        }
        AppMode::Yolo => {
            let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeYoloModeTip));
            let _ = writeln!(stats, "{}", tr(locale, MessageId::HomeYoloModeCaution));
        }
        AppMode::Plan => {
            let _ = writeln!(stats, "{}", tr(locale, MessageId::HomePlanModeTip));
            let _ = writeln!(stats, "{}", tr(locale, MessageId::HomePlanModeChecklistTip));
        }
    }

    CommandResult::message(stats)
}

/// Toggle output translation to the current system language on/off.
///
/// When enabled, the model is instructed to respond in the current locale and an
/// interception layer translates any remaining English output before it
/// reaches the user.
pub fn translate(app: &mut App) -> CommandResult {
    app.translation_enabled = !app.translation_enabled;
    let locale = app.ui_locale;
    if app.translation_enabled {
        CommandResult::message(tr(locale, MessageId::CmdTranslateOn))
    } else {
        CommandResult::message(tr(locale, MessageId::CmdTranslateOff))
    }
}

#[cfg(test)]
mod tests {}
