//! Provider switching, model selection, and reasoning effort management.

use crate::client::ApiClient;
use crate::config::{ApiProvider, Config};
use crate::core::engine::{EngineHandle, spawn_engine};
use crate::core::ops::Op;
use crate::route_runtime::{resolve_route_candidate, resolve_runtime_route};

use super::super::app::{App, PendingProviderSwitch, ReasoningEffort};
use super::super::history::HistoryCell;
use super::engine_config_prompt::build_engine_config;

pub(crate) async fn sync_mode_update(engine_handle: &EngineHandle, mode: crate::tui::app::AppMode) {
    let _ = engine_handle.send(Op::ChangeMode { mode }).await;
}

pub(crate) async fn apply_mode_update(
    app: &mut App,
    engine_handle: &EngineHandle,
    mode: crate::tui::app::AppMode,
) -> bool {
    if app.set_mode(mode) {
        sync_mode_update(engine_handle, mode).await;
        true
    } else {
        false
    }
}

pub(crate) async fn apply_model_and_compaction_update(
    engine_handle: &EngineHandle,
    compaction: crate::compaction::CompactionConfig,
    mode: crate::tui::app::AppMode,
    route_limits: Option<mimofan_config::route::RouteLimits>,
) {
    let _ = engine_handle
        .send(Op::SetModel {
            model: compaction.model.clone(),
            mode,
            route_limits,
        })
        .await;
    let _ = engine_handle
        .send(Op::SetCompaction { config: compaction })
        .await;
}

/// Apply the choice made in the `/model` picker (#39): mutate App state so
/// the next turn uses the new model/effort, persist the selection to
/// `~/.mimofan/settings.json` so it survives a restart, push the change to
/// the running engine via `Op::SetModel`/`Op::SetCompaction`, and surface
/// a one-line status describing what changed.
// The model/effort transition needs both the previous and next model+effort
// plus the engine, app, and config handles; bundling them into a struct here
// would only obscure a straightforward orchestration step.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_model_picker_choice(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    config: &mut Config,
    model: String,
    target_provider: Option<ApiProvider>,
    mut effort: ReasoningEffort,
    previous_model: String,
    previous_effort: ReasoningEffort,
) {
    let model_is_auto = model.trim().eq_ignore_ascii_case("auto");
    if model_is_auto {
        effort = ReasoningEffort::Auto;
    } else {
        effort = effort.normalize_for_provider(target_provider.unwrap_or(app.api_provider));
    }
    if let Some(target_provider) = target_provider
        && target_provider != app.api_provider
        && !model_is_auto
    {
        switch_provider(
            app,
            engine_handle,
            config,
            target_provider,
            Some(model.clone()),
        )
        .await;
        if app.api_provider != target_provider {
            return;
        }
        apply_picker_effort_choice(app, engine_handle, effort, previous_effort).await;
        return;
    }

    let model_changed = model != previous_model || app.auto_model != model_is_auto;
    let effort_changed = effort != previous_effort;
    if !model_changed && !effort_changed {
        app.status_message = Some(format!(
            "Model unchanged: {model} · thinking {}",
            effort.display_label_for_provider(app.api_provider)
        ));
        return;
    }

    let mut resolved_model = model.clone();
    if model_changed && !model_is_auto {
        let saved_provider_model = config
            .provider_config_for(app.api_provider)
            .and_then(|provider| provider.model.as_deref());
        match resolve_route_candidate(
            app.api_provider,
            Some(&model),
            saved_provider_model,
            Some(config.api_base_url()),
        ) {
            Ok(candidate) => {
                resolved_model = candidate.wire_model_id.as_str().to_string();
                app.set_active_route_limits(candidate.limits);
            }
            Err(reason) => {
                app.status_message = Some(reason);
                return;
            }
        }
    } else if model_changed && model_is_auto {
        app.active_route_limits = None;
    }

    if model_changed {
        app.set_model_selection(resolved_model.clone());
        app.provider_models.insert(
            app.api_provider.as_str().to_string(),
            resolved_model.clone(),
        );
        app.clear_model_scoped_telemetry();
    }
    if effort_changed {
        app.reasoning_effort = effort;
        app.last_effective_reasoning_effort = None;
    }
    if model_changed || effort_changed {
        app.update_model_compaction_budget();
    }

    // Best-effort persist; surface a status warning if the settings file
    // can't be written rather than aborting the in-memory change.
    let mut persist_warning: Option<String> = None;
    let persist_result = (|| -> anyhow::Result<()> {
        let mut settings = crate::settings::Settings::load()?;
        if model_changed {
            if matches!(app.api_provider, ApiProvider::XiaomiMimo) {
                settings.set("default_model", &resolved_model)?;
            }
            settings.set_model_for_provider(app.api_provider.as_str(), &resolved_model);
        }
        if effort_changed {
            settings.set(
                "reasoning_effort",
                effort.as_setting_for_provider(app.api_provider),
            )?;
        }
        settings.save()
    })();
    if let Err(err) = persist_result {
        persist_warning = Some(format!("(not persisted: {err})"));
    }

    if model_changed {
        apply_model_and_compaction_update(
            engine_handle,
            app.compaction_config(),
            app.mode,
            app.active_route_limits,
        )
        .await;
    }

    let model_summary = if model_is_auto {
        "auto (per-turn model)".to_string()
    } else {
        resolved_model.clone()
    };
    let previous_effort_summary = previous_effort.display_label_for_provider(app.api_provider);
    let effort_summary = if effort == ReasoningEffort::Auto {
        "auto (per-turn thinking)".to_string()
    } else {
        effort
            .display_label_for_provider(app.api_provider)
            .to_string()
    };

    let mut summary = match (model_changed, effort_changed) {
        (true, true) => format!(
            "Model: {previous_model} → {model_summary} · thinking: {previous_effort_summary} → {effort_summary}"
        ),
        (true, false) => {
            format!("Model: {previous_model} → {model_summary} · thinking {effort_summary}")
        }
        (false, true) => format!(
            "Thinking: {previous_effort_summary} → {effort_summary} · model {model_summary}"
        ),
        (false, false) => unreachable!(),
    };
    if let Some(warning) = persist_warning {
        summary.push(' ');
        summary.push_str(&warning);
    }
    app.status_message = Some(summary);
}

pub(crate) async fn apply_picker_effort_choice(
    app: &mut App,
    engine_handle: &EngineHandle,
    mut effort: ReasoningEffort,
    previous_effort: ReasoningEffort,
) {
    effort = effort.normalize_for_provider(app.api_provider);
    if effort == previous_effort {
        return;
    }

    app.reasoning_effort = effort;
    app.last_effective_reasoning_effort = None;
    app.update_model_compaction_budget();

    let persist_warning = (|| -> anyhow::Result<()> {
        let mut settings = crate::settings::Settings::load()?;
        settings.set(
            "reasoning_effort",
            effort.as_setting_for_provider(app.api_provider),
        )?;
        settings.save()
    })()
    .err()
    .map(|err| format!(" (not persisted: {err})"));

    apply_model_and_compaction_update(
        engine_handle,
        app.compaction_config(),
        app.mode,
        app.active_route_limits,
    )
    .await;

    let mut summary = format!(
        "Thinking: {} → {} · model {}",
        previous_effort.display_label_for_provider(app.api_provider),
        effort.display_label_for_provider(app.api_provider),
        app.model_display_label()
    );
    if let Some(warning) = persist_warning {
        summary.push_str(&warning);
    }
    app.status_message = Some(summary);
}

/// Apply a `/provider` switch by resolving a complete route candidate before
/// mutating state, then respawning the engine so the API client picks up the
/// new base URL/key. When `model_override` is set, it replaces the active
/// model post-switch after provider-scoped normalization.
pub(crate) async fn switch_provider(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    config: &mut Config,
    target: ApiProvider,
    model_override: Option<String>,
) {
    let previous_provider = app.api_provider;
    let previous_model = app.model.clone();
    let previous_model_ids_passthrough = app.model_ids_passthrough;
    let previous_config = config.clone();
    app.pending_provider_switch = Some(PendingProviderSwitch {
        previous_provider,
        previous_model: previous_model.clone(),
        previous_model_ids_passthrough,
        previous_route_limits: app.active_route_limits,
        previous_config: previous_config.clone(),
        previous_onboarding: app.onboarding,
        previous_onboarding_needs_api_key: app.onboarding_needs_api_key,
        previous_api_key_env_only: app.api_key_env_only,
    });

    let resolved_route = match resolve_runtime_route(config, target, model_override.as_deref()) {
        Ok(route) => route,
        Err(reason) => {
            app.pending_provider_switch = None;
            app.add_message(HistoryCell::System {
                content: format!(
                    "Cannot switch to {}: {reason}\nProvider unchanged ({}).",
                    target.as_str(),
                    previous_provider.as_str()
                ),
            });
            app.status_message = Some(format!(
                "Route rejected before provider switch: {}.",
                target.as_str()
            ));
            return;
        }
    };
    let resolved_endpoint = resolved_route.candidate.endpoint.base_url.clone();
    let next_config = resolved_route.config;
    let new_model = resolved_route.model;

    if let Err(err) = ApiClient::from_candidate(&next_config, &resolved_route.candidate) {
        app.pending_provider_switch = None;
        app.add_message(HistoryCell::System {
            content: format!(
                "Failed to switch provider to {}: {err}\nProvider unchanged ({}).",
                target.as_str(),
                previous_provider.as_str()
            ),
        });
        return;
    }
    *config = next_config;

    let new_base_url = resolved_endpoint;
    let new_endpoint = display_base_url_host(&new_base_url);
    let cache_scope_changed = previous_provider != target || previous_model != new_model;
    app.api_provider = target;
    app.max_subagents = config
        .max_subagents_for_provider(target)
        .clamp(1, crate::config::MAX_SUBAGENTS);
    app.provider_chain = target
        .kind()
        .map(|kind| mimofan_config::ProviderChain::new(kind, &config.fallback_providers))
        .filter(|chain| chain.providers().len() > 1);
    app.last_fallback_reason = None;
    app.model_ids_passthrough = config.model_ids_pass_through();
    app.reasoning_effort = app.reasoning_effort.normalize_for_provider(target);
    app.set_model_selection(new_model.clone());
    app.set_active_route_limits(resolved_route.candidate.limits);
    if model_override.is_some() {
        app.provider_models
            .insert(target.as_str().to_string(), new_model.clone());
    }
    app.update_model_compaction_budget();
    if cache_scope_changed {
        app.clear_model_scoped_telemetry();
    } else {
        app.session.last_prompt_tokens = None;
        app.session.last_completion_tokens = None;
        app.session.last_output_throughput = None;
    }

    let _ = engine_handle.send(Op::Shutdown).await;
    let engine_config = build_engine_config(app, config);
    *engine_handle = spawn_engine(engine_config, config);

    if !app.api_messages.is_empty() {
        let _ = engine_handle
            .send(Op::SyncSession {
                session_id: app.current_session_id.clone(),
                messages: app.api_messages.clone(),
                system_prompt: app.system_prompt.clone(),
                system_prompt_override: false,
                model: app.model.clone(),
                workspace: app.workspace.clone(),
            })
            .await;
    }
    let _ = engine_handle
        .send(Op::SetCompaction {
            config: app.compaction_config(),
        })
        .await;

    let persist_warning = (|| -> anyhow::Result<()> {
        crate::config_persistence::persist_root_string_key(
            app.config_path.as_deref(),
            "provider",
            target.as_str(),
        )?;

        let mut settings = crate::settings::Settings::load()?;
        settings.default_provider = Some(target.as_str().to_string());
        if model_override.is_some() {
            settings.set_model_for_provider(target.as_str(), &new_model);
            if matches!(target, ApiProvider::XiaomiMimo) {
                settings.set("default_model", &new_model)?;
            }
        }
        settings.save()?;
        Ok(())
    })()
    .err()
    .map(|err| format!("Provider selection was not fully persisted: {err}"));

    let mut switch_summary = format!(
        "Provider switched: {} → {}",
        previous_provider.as_str(),
        target.as_str(),
    );
    switch_summary.push(char::from(10));
    switch_summary.push_str(&format!("Model: {previous_model} → {new_model}"));
    switch_summary.push(char::from(10));
    switch_summary.push_str(&format!("Endpoint: {new_endpoint}"));
    if let Some(ref warning) = persist_warning {
        switch_summary.push(char::from(10));
        switch_summary.push_str(warning);
    }
    app.add_message(HistoryCell::System {
        content: switch_summary,
    });

    let mut status_message = format!("Provider: {} via {}", target.as_str(), new_endpoint);
    if persist_warning.is_some() {
        status_message.push_str(" (not fully persisted)");
    }
    app.status_message = Some(status_message);
}

pub(crate) async fn apply_provider_fallback_switch(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    config: &mut Config,
    previous_provider: ApiProvider,
) {
    let target = app.api_provider;
    let previous_model = app.model.clone();

    let resolved_route = match resolve_runtime_route(config, target, None) {
        Ok(route) => route,
        Err(reason) => {
            app.api_provider = previous_provider;
            app.last_fallback_reason = Some(format!(
                "Fallback provider {} route was rejected: {reason}",
                target.as_str()
            ));
            app.status_message = Some(format!(
                "Fallback provider {} rejected; provider remains {}.",
                target.as_str(),
                previous_provider.as_str()
            ));
            return;
        }
    };
    let resolved_endpoint = resolved_route.candidate.endpoint.base_url.clone();
    let next_config = resolved_route.config;
    let new_model = resolved_route.model;

    if let Err(err) = ApiClient::from_candidate(&next_config, &resolved_route.candidate) {
        app.api_provider = previous_provider;
        app.last_fallback_reason = Some(format!(
            "Fallback provider {} was unavailable: {err}",
            target.as_str()
        ));
        app.status_message = Some(format!(
            "Fallback provider {} unavailable; provider remains {}.",
            target.as_str(),
            previous_provider.as_str()
        ));
        return;
    }
    *config = next_config;

    let new_base_url = resolved_endpoint;
    let new_endpoint = display_base_url_host(&new_base_url);
    let cache_scope_changed = previous_provider != target || previous_model != new_model;
    app.model_ids_passthrough = config.model_ids_pass_through();
    app.reasoning_effort = app.reasoning_effort.normalize_for_provider(target);
    app.set_model_selection(new_model.clone());
    app.set_active_route_limits(resolved_route.candidate.limits);
    app.update_model_compaction_budget();
    if cache_scope_changed {
        app.clear_model_scoped_telemetry();
    } else {
        app.session.last_prompt_tokens = None;
        app.session.last_completion_tokens = None;
        app.session.last_output_throughput = None;
    }

    let _ = engine_handle.send(Op::Shutdown).await;
    let engine_config = build_engine_config(app, config);
    *engine_handle = spawn_engine(engine_config, config);

    if !app.api_messages.is_empty() {
        let _ = engine_handle
            .send(Op::SyncSession {
                session_id: app.current_session_id.clone(),
                messages: app.api_messages.clone(),
                system_prompt: app.system_prompt.clone(),
                system_prompt_override: false,
                model: app.model.clone(),
                workspace: app.workspace.clone(),
            })
            .await;
    }
    let _ = engine_handle
        .send(Op::SetCompaction {
            config: app.compaction_config(),
        })
        .await;

    app.add_message(HistoryCell::System {
        content: format!(
            "Provider fallback: {} -> {}\nModel: {} -> {}\nEndpoint: {}",
            previous_provider.as_str(),
            target.as_str(),
            previous_model,
            new_model,
            new_endpoint
        ),
    });
    app.status_message = Some(format!(
        "Fallback provider: {} via {}",
        target.as_str(),
        new_endpoint
    ));
}

pub(crate) fn display_base_url_host(base_url: &str) -> String {
    let without_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    without_scheme
        .split('/')
        .next()
        .filter(|host| !host.is_empty())
        .unwrap_or(base_url)
        .to_string()
}

pub(crate) fn sync_config_provider_from_app(config: &mut Config, app: &App) {
    config.provider = Some(app.api_provider.as_str().to_string());
}

pub(crate) fn provider_picker_model_override(app: &App, provider: ApiProvider) -> Option<String> {
    (app.api_provider == provider).then(|| app.model.clone())
}
