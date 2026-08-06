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
    // TODO(mimofan-refactor): `ApiProvider::OpenAiCompatible` was removed. Named custom
    // providers are now represented as `ApiProvider::OpenAiCompatible` with a
    // non-built-in `config.provider` name and live in `providers.custom[name]`.
    // Built-in OpenAI-compatible (formerly XiaomiMimo) uses `config.api_key`
    // plus `providers.openai_compatible`. Distinguish by provider name.
    if matches!(provider, ApiProvider::OpenAiCompatible)
        && !config.provider.as_deref().is_some_and(|p| p != "openai-compatible")
    {
        config.api_key = Some(api_key);
    } else {
        let providers = config
            .providers
            .get_or_insert_with(ProvidersConfig::default);
        let entry: &mut ProviderConfig = match provider {
            ApiProvider::OpenAiCompatible => {
                if let Some(key) = config.provider.clone() {
                    providers.custom.entry(key).or_default()
                } else {
                    &mut providers.openai_compatible
                }
            }
            ApiProvider::AnthropicCompatible => &mut providers.anthropic_compatible,
            ApiProvider::GeminiCompatible => &mut providers.gemini_compatible,
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
    // TODO(mimofan-refactor): `ApiProvider::OpenAiCompatible` was removed; named custom
    // providers now flow through `ApiProvider::OpenAiCompatible` with a
    // `config.provider` name and live in `providers.custom[name]`. Built-in
    // OpenAI-compatible / Anthropic-compatible / Gemini-compatible do not
    // persist auth_mode here.
    let providers = config
        .providers
        .get_or_insert_with(ProvidersConfig::default);
    let entry: &mut ProviderConfig = match provider {
        ApiProvider::OpenAiCompatible => {
            if let Some(key) = config.provider.clone() {
                providers.custom.entry(key).or_default()
            } else {
                return;
            }
        }
        ApiProvider::AnthropicCompatible | ApiProvider::GeminiCompatible => return,
    };
    entry.auth_mode = Some(auth_mode);
}
