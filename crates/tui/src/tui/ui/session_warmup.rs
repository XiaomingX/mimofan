//! session warmup 子系统（从 ui 上帝文件切片）
use super::*;
use std::sync::{Arc, Mutex as StdMutex};

use mimofan_config::catalog::{
    CatalogCompiler, ProviderCatalogCache, base_url_fingerprint, now_unix,
};

pub(crate) async fn fetch_available_models(
    config: &Config,
    catalog_cache: Arc<StdMutex<ProviderCatalogCache>>,
) -> Result<Vec<String>> {
    use crate::client::ApiClient;

    let client = ApiClient::new(config, catalog_cache.clone())?;
    // Best-effort live refresh: non-fatal, bounded by a timeout so the UI
    // never hangs on a slow/unreachable provider endpoint.
    let _ = tokio::time::timeout(
        Duration::from_secs(20),
        client.refresh_catalog(crate::client::CATALOG_TTL_SECS),
    )
    .await;
    crate::config_persistence::save_catalog_cache(&catalog_cache);

    // Merge bundled offerings (network-free) with any fresh live rows from the
    // shared cache for this provider + base URL. Live rows win on identity via
    // the compiler's layered merge (#3385).
    let provider = client.catalog_provider_id();
    let fp = base_url_fingerprint(client.base_url());
    let cache = catalog_cache.lock().expect("catalog cache poisoned");
    let live = cache.fresh_offerings(&provider, &fp, now_unix());
    let snapshot = CatalogCompiler::new()
        .with_bundled(mimofan_config::catalog::bundled_catalog_offerings())
        .with_live(live)
        .compile();
    drop(cache);

    let mut ids: Vec<String> = snapshot
        .offerings
        .into_iter()
        .map(|offering| offering.wire_model_id)
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

pub(crate) async fn run_cache_warmup(
    app: &App,
    config: &Config,
) -> Result<(Usage, String, PromptInspection)> {
    let client = ApiClient::new_detached(config)?;
    let base_url = client.base_url().to_string();
    let reasoning_effort = if app.reasoning_effort == ReasoningEffort::Auto {
        app.last_effective_reasoning_effort
            .and_then(|effort| effort.api_value_for_provider(app.api_provider))
            .map(str::to_string)
    } else {
        app.reasoning_effort
            .api_value_for_provider(app.api_provider)
            .map(str::to_string)
    };
    let request = MessageRequest {
        model: app.model.clone(),
        messages: app.api_messages.clone(),
        max_tokens: 1024,
        system: app.system_prompt.clone(),
        tools: app.session.last_tool_catalog.clone(),
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort,
        stream: None,
        temperature: None,
        top_p: None,
        response_format: None,
    };
    let warmup = build_cache_warmup_request(&request);
    let inspection = inspect_prompt_for_request(&warmup);
    let response =
        tokio::time::timeout(Duration::from_secs(45), client.create_message(warmup)).await??;
    Ok((response.usage, base_url, inspection))
}

// `format_*` chip/message builders moved to `tui/format_helpers.rs`.

pub(crate) fn build_session_snapshot(app: &App, manager: &SessionManager) -> SavedSession {
    let model = app.model_selection_for_persistence();
    if let Some(ref existing_id) = app.current_session_id
        && let Ok(existing) = manager.load_session(existing_id)
    {
        let mut updated = update_session(
            existing,
            &app.api_messages,
            u64::from(app.session.total_tokens),
            app.system_prompt.as_ref(),
        );
        updated.metadata.model = model;
        updated.metadata.mode = Some(app.mode.as_setting().to_string());
        app.sync_cost_to_metadata(&mut updated.metadata);
        updated.context_references = app.session_context_references.clone();
        updated.artifacts = app.session_artifacts.clone();
        updated
    } else {
        let mut session = if let Some(existing_id) = app.current_session_id.as_ref() {
            create_saved_session_with_id_and_mode(
                existing_id.clone(),
                &app.api_messages,
                &model,
                &app.workspace,
                u64::from(app.session.total_tokens),
                app.system_prompt.as_ref(),
                Some(app.mode.as_setting()),
            )
        } else {
            create_saved_session_with_mode(
                &app.api_messages,
                &model,
                &app.workspace,
                u64::from(app.session.total_tokens),
                app.system_prompt.as_ref(),
                Some(app.mode.as_setting()),
            )
        };
        app.sync_cost_to_metadata(&mut session.metadata);
        session.context_references = app.session_context_references.clone();
        session.artifacts = app.session_artifacts.clone();
        session
    }
}

pub(crate) fn queued_ui_to_session(msg: &QueuedMessage) -> QueuedSessionMessage {
    QueuedSessionMessage {
        display: msg.display.clone(),
        skill_instruction: msg.skill_instruction.clone(),
    }
}

pub(crate) fn queued_session_to_ui(msg: QueuedSessionMessage) -> QueuedMessage {
    QueuedMessage {
        display: msg.display,
        skill_instruction: msg.skill_instruction,
    }
}
