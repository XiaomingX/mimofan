//! provider picker api key 子系统（从 ui 上帝文件切片）
use super::*;

/// Persist the typed API key to `~/.mimofan/config.toml`, refresh the
/// in-memory config so the engine can see it, then switch to the provider.
pub(crate) async fn apply_provider_picker_api_key(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    config: &mut Config,
    provider: ApiProvider,
    api_key: String,
) {
    use crate::config::save_api_key_for;

    match save_api_key_for(provider, &api_key) {
        Ok(path) => {
            app.status_message = Some(format!(
                "Saved {} API key to {}",
                provider.as_str(),
                path.display()
            ));
            app.api_key_env_only = false;
        }
        Err(err) => {
            app.add_message(HistoryCell::System {
                content: format!(
                    "Failed to save {} API key: {err}\nProvider unchanged.",
                    provider.as_str()
                ),
            });
            return;
        }
    }

    // Mirror the saved key into the in-memory config so the engine sees it
    // immediately without a reload — `save_api_key_for` only touches disk.
    if matches!(provider, ApiProvider::XiaomiMimo) {
        config.api_key = Some(api_key);
    } else {
        // Capture the custom entry key before borrowing `providers` (#1519).
        let custom_key = (provider == ApiProvider::Custom).then(|| {
            config
                .provider
                .clone()
                .unwrap_or_else(|| "__custom__".to_string())
        });
        let providers = config
            .providers
            .get_or_insert_with(ProvidersConfig::default);
        let entry: &mut ProviderConfig = match provider {
            ApiProvider::XiaomiMimo => &mut providers.xiaomi_mimo,
            ApiProvider::Anthropic => &mut providers.anthropic,
            ApiProvider::Custom => providers
                .custom
                .entry(custom_key.expect("custom key captured for custom provider"))
                .or_default(),
        };
        entry.api_key = Some(api_key);
    }

    switch_provider(app, engine_handle, config, provider, None).await;
}

pub(crate) async fn apply_provider_picker_auth_mode(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    config: &mut Config,
    provider: ApiProvider,
    auth_mode: &str,
    status_prefix: &str,
) {
    match save_provider_auth_mode_for(provider, auth_mode) {
        Ok(path) => {
            set_provider_auth_mode_in_memory(config, provider, auth_mode.to_string());
            app.status_message = Some(format!("{status_prefix}; saved to {}", path.display()));
            app.api_key_env_only = false;
        }
        Err(err) => {
            app.add_message(HistoryCell::System {
                content: format!(
                    "Failed to save {} auth mode: {err}\nProvider unchanged.",
                    provider.as_str()
                ),
            });
            return;
        }
    }

    switch_provider(app, engine_handle, config, provider, None).await;
}

fn set_provider_auth_mode_in_memory(config: &mut Config, provider: ApiProvider, auth_mode: String) {
    // Capture the custom entry key (the selected provider name) before the
    // mutable borrow of `providers` below (#1519).
    let custom_key = (provider == ApiProvider::Custom).then(|| {
        config
            .provider
            .clone()
            .unwrap_or_else(|| "__custom__".to_string())
    });
    let providers = config
        .providers
        .get_or_insert_with(ProvidersConfig::default);
    let entry: &mut ProviderConfig = match provider {
        ApiProvider::XiaomiMimo | ApiProvider::Anthropic => return,
        ApiProvider::Custom => providers
            .custom
            .entry(custom_key.expect("custom key captured for custom provider"))
            .or_default(),
    };
    entry.auth_mode = Some(auth_mode);
}
