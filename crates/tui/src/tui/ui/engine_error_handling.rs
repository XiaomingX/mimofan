//! engine error handling 子系统（从 ui 上帝文件切片）
use super::*;

/// Translate an `EngineEvent::Error` into UI state updates.
///
/// The engine's `recoverable` flag (mirrored on `ErrorEnvelope`) decides
/// whether the session flips into offline mode: stream stalls, chunk
/// timeouts, transient network errors, and rate-limit/server hiccups arrive
/// recoverable and must NOT flip into offline. Hard failures (auth, billing,
/// invalid request) arrive non-recoverable; those flip offline so subsequent
/// messages get queued instead of silently lost mid-flight.
///
/// `severity` drives transcript color: red for `Error`/`Critical`, amber for
/// `Warning`, dim for `Info`.
pub(crate) fn apply_engine_error_to_app(
    app: &mut App,
    envelope: crate::error_taxonomy::ErrorEnvelope,
) {
    let recoverable = envelope.recoverable;
    let message = envelope.message.clone();
    let severity = envelope.severity;
    let turn_was_in_progress =
        app.is_loading || matches!(app.runtime_turn_status.as_deref(), Some("in_progress"));
    streaming_thinking::finalize_current(app);
    if turn_was_in_progress {
        app.finalize_streaming_assistant_as_interrupted();
        app.finalize_active_cell_as_interrupted();
        app.runtime_turn_status = Some("failed".to_string());
    }
    app.streaming_state.reset();
    app.streaming_message_index = None;
    app.streaming_thinking_active_entry = None;

    // #455 (observer-only): fire `on_error` hooks so operators can
    // page on auth / billing / invalid-request failures without
    // tailing the audit log. Read-only — the hook can react but not
    // suppress the error from reaching the transcript. Fast-path
    // skip when no hooks configured.
    if app
        .hooks
        .has_hooks_for_event(crate::hooks::HookEvent::OnError)
    {
        let context = app.base_hook_context().with_error(&message);
        let _ = app.execute_hooks(crate::hooks::HookEvent::OnError, &context);
    }

    app.add_message(HistoryCell::Error {
        message: message.clone(),
        severity,
    });
    app.is_loading = false;
    app.dispatch_started_at = None;
    app.turn_error_posted = true;
    if matches!(
        envelope.category,
        crate::error_taxonomy::ErrorCategory::Authentication
    ) && app.api_key_env_only
    {
        app.offline_mode = true;
        app.onboarding_needs_api_key = true;
        app.onboarding = OnboardingState::ApiKey;
        app.status_message = Some(
            "The API key from MIMOFAN_API_KEY was rejected. Paste a valid key to save it to ~/.mimofan/config.toml, or update the environment variable.".to_string(),
        );
        return;
    }
    if recoverable
        && matches!(
            envelope.category,
            crate::error_taxonomy::ErrorCategory::Network
                | crate::error_taxonomy::ErrorCategory::RateLimit
                | crate::error_taxonomy::ErrorCategory::Timeout
        )
        && app.advance_fallback(message.clone()).is_some()
    {
        let position = app.fallback_chain_position().unwrap_or(0);
        let total = app.fallback_chain_len();
        app.status_message = Some(format!(
            "Switched to {} (fallback {position}/{}) after recoverable provider error.",
            app.api_provider.as_str(),
            total.saturating_sub(1)
        ));
        return;
    }
    if !recoverable {
        app.offline_mode = true;
    }
    // Error is already in the transcript as HistoryCell::Error above;
    // don't emit a redundant status_message that would become a sticky
    // toast in the footer — that duplicates the transcript entry.
}

pub(crate) fn rollback_provider_after_auth_failure(app: &mut App, config: &mut Config) -> Option<String> {
    let pending = app.pending_provider_switch.take()?;
    let PendingProviderSwitch {
        previous_provider,
        previous_model,
        previous_model_ids_passthrough,
        previous_route_limits,
        previous_config,
        previous_onboarding,
        previous_onboarding_needs_api_key,
        previous_api_key_env_only,
    } = pending;

    *config = previous_config;
    app.api_provider = previous_provider;
    app.set_model_selection(previous_model.clone());
    app.provider_models
        .insert(previous_provider.as_str().to_string(), previous_model);
    app.model_ids_passthrough = previous_model_ids_passthrough;
    app.active_route_limits = previous_route_limits;
    app.update_model_compaction_budget();
    app.clear_model_scoped_telemetry();
    app.offline_mode = false;
    app.onboarding = previous_onboarding;
    app.onboarding_needs_api_key = previous_onboarding_needs_api_key;
    app.api_key_env_only = previous_api_key_env_only;

    let persistence_error = (|| -> anyhow::Result<()> {
        crate::config_persistence::persist_root_string_key(
            app.config_path.as_deref(),
            "provider",
            previous_provider.as_str(),
        )?;
        let mut settings = crate::settings::Settings::load()?;
        settings.default_provider = Some(previous_provider.as_str().to_string());
        settings.set_model_for_provider(
            previous_provider.as_str(),
            &app.model_selection_for_persistence(),
        );
        if matches!(previous_provider, ApiProvider::XiaomiMimo) {
            settings.set("default_model", &app.model_selection_for_persistence())?;
        }
        settings.save()?;
        Ok(())
    })()
    .err()
    .map(|err| format!("provider rollback not fully persisted: {err}"));

    Some(match persistence_error {
        Some(warning) => format!(
            "Provider switch failed and has been rolled back to {}. {}",
            previous_provider.as_str(),
            warning
        ),
        None => format!(
            "Provider switch failed and has been rolled back to {}.",
            previous_provider.as_str()
        ),
    })
}

pub(crate) fn persist_offline_queue_state(app: &App) {
    if app.queued_messages.is_empty() && app.queued_draft.is_none() {
        persistence_actor::persist(PersistRequest::ClearOfflineQueue);
        return;
    }
    let state = OfflineQueueState {
        messages: app
            .queued_messages
            .iter()
            .map(queued_ui_to_session)
            .collect(),
        draft: app.queued_draft.as_ref().map(queued_ui_to_session),
        ..OfflineQueueState::default()
    };
    persistence_actor::persist(PersistRequest::OfflineQueue {
        state,
        session_id: app.current_session_id.clone(),
    });
}
