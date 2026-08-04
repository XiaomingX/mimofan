//! Environment variable overrides for configuration values.

use anyhow::Result;

use super::subagent_limits::MAX_SUBAGENTS;
use super::{ApiProvider, Config, MemoryConfig, ProvidersConfig, SearchConfig, parse_http_headers};

/// Read the `MIMO_BASE_URL` / `MIMOFAN_BASE_URL` env var that the CLI
/// dispatcher forwards from `--base-url`.  Returns `None` when the var is
/// absent or empty so that provider-specific defaults still apply.
pub(crate) fn env_base_url_override() -> Option<String> {
    env_nonempty("MIMOFAN_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// Read an env var, returning `Err(NotPresent)` if unset or blank.
pub(crate) fn env_nonempty(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(std::env::VarError::NotPresent)
}

pub(crate) fn apply_env_overrides(config: &mut Config) {
    if let Ok(value) = env_nonempty("MIMOFAN_PROVIDER") {
        config.provider = Some(value);
    }
    if let Ok(value) = env_nonempty("MIMOFAN_BASE_URL") {
        match config.api_provider() {
            ApiProvider::XiaomiMimo => {
                config
                    .providers
                    .get_or_insert_with(ProvidersConfig::default)
                    .xiaomi_mimo
                    .base_url = Some(value);
            }
            ApiProvider::Anthropic => {
                config
                    .providers
                    .get_or_insert_with(ProvidersConfig::default)
                    .anthropic
                    .base_url = Some(value);
            }
            ApiProvider::Custom => {
                config.provider_config_for_mut(ApiProvider::Custom).base_url = Some(value);
            }
        }
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("NVIDIA_NIM_BASE_URL")
            .or_else(|_| std::env::var("NIM_BASE_URL"))
            .or_else(|_| std::env::var("NVIDIA_BASE_URL"))
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .nvidia_nim
            .base_url = Some(value);
    }
    // OpenAI-compatible and non-DeepSeek hosted providers are scoped only on
    // their own provider entry -- the legacy root `base_url` keeps DeepSeek-only
    // semantics.
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("OPENAI_BASE_URL")
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .openai
            .base_url = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("OPENROUTER_BASE_URL")
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .openrouter
            .base_url = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("XIAOMI_MIMO_BASE_URL")
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .xiaomi_mimo
            .base_url = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("XIAOMI_MIMO_MODE")
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .xiaomi_mimo
            .mode = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("VOLCENGINE_BASE_URL")
            .or_else(|_| std::env::var("VOLCENGINE_ARK_BASE_URL"))
            .or_else(|_| std::env::var("ARK_BASE_URL"))
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .volcengine
            .base_url = Some(value);
    }
    let active_provider = config.api_provider();
    if matches!(active_provider, ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("SILICONFLOW_BASE_URL")
        && !value.trim().is_empty()
    {
        config.provider_config_for_mut(active_provider).base_url = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) =
            std::env::var("MOONSHOT_BASE_URL").or_else(|_| std::env::var("KIMI_BASE_URL"))
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .moonshot
            .base_url = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_HTTP_HEADERS")
        && let Ok(headers) = parse_http_headers(&value)
        && !headers.is_empty()
    {
        let mut root_headers = config.http_headers.clone().unwrap_or_default();
        root_headers.extend(headers.clone());
        config.http_headers = Some(root_headers);

        let provider = config.api_provider();
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
        let entry = match provider {
            ApiProvider::XiaomiMimo => &mut providers.xiaomi_mimo,
            ApiProvider::Anthropic => &mut providers.anthropic,
            ApiProvider::Custom => providers
                .custom
                .entry(custom_key.expect("custom key captured for custom provider"))
                .or_default(),
        };
        let mut provider_headers = entry.http_headers.clone().unwrap_or_default();
        provider_headers.extend(headers);
        entry.http_headers = Some(provider_headers);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("OPENAI_MODEL")
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .openai
            .model = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("XIAOMI_MIMO_MODEL")
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .xiaomi_mimo
            .model = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("OPENROUTER_MODEL")
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .openrouter
            .model = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) =
            std::env::var("VOLCENGINE_MODEL").or_else(|_| std::env::var("VOLCENGINE_ARK_MODEL"))
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .volcengine
            .model = Some(value);
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("MOONSHOT_MODEL")
            .or_else(|_| std::env::var("KIMI_MODEL_NAME"))
            .or_else(|_| std::env::var("KIMI_MODEL"))
        && !value.trim().is_empty()
    {
        config
            .providers
            .get_or_insert_with(ProvidersConfig::default)
            .moonshot
            .model = Some(value);
    }
    let active_provider = config.api_provider();
    if matches!(active_provider, ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("SILICONFLOW_MODEL")
        && !value.trim().is_empty()
    {
        config.provider_config_for_mut(active_provider).model = Some(value);
    }
    if let Some(value) = env_nonempty("MIMOFAN_MODEL").ok().or_else(|| {
        std::env::var("MIMOFAN_DEFAULT_TEXT_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
    }) {
        // The CLI `--model` handoff always sets MIMOFAN_MODEL, never the
        // provider-specific *_MODEL var. The legacy root `default_text_model`
        // is a DeepSeek-only slot (the validator rejects non-DeepSeek IDs
        // there). For a non-DeepSeek provider the explicit model must land in
        // the provider-scoped slot instead so the verbatim-passthrough path
        // honors it rather than falling back to a DeepSeek/provider default
        // (issue #1714). Mirror the OPENAI_MODEL branch above for every
        // non-DeepSeek provider.
        let provider = config.api_provider();
        // Capture the custom entry key before the mutable borrow below (#1519).
        let custom_key = (provider == ApiProvider::Custom).then(|| {
            config
                .provider
                .clone()
                .unwrap_or_else(|| "__custom__".to_string())
        });
        if matches!(provider, ApiProvider::XiaomiMimo) {
            config.default_text_model = Some(value);
        } else {
            let providers = config
                .providers
                .get_or_insert_with(ProvidersConfig::default);
            let entry = match provider {
                ApiProvider::XiaomiMimo => &mut providers.xiaomi_mimo,
                ApiProvider::Anthropic => &mut providers.anthropic,
                ApiProvider::Custom => providers
                    .custom
                    .entry(custom_key.expect("custom key captured for custom provider"))
                    .or_default(),
            };
            entry.model = Some(value);
        }
    }
    if matches!(config.api_provider(), ApiProvider::XiaomiMimo)
        && let Ok(value) = std::env::var("NVIDIA_NIM_MODEL")
    {
        config.default_text_model = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_SKILLS_DIR") {
        config.skills_dir = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_MCP_CONFIG") {
        config.mcp_config_path = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_NOTES_PATH") {
        config.notes_path = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_MEMORY_PATH") {
        config.memory_path = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_MEMORY") {
        let on = matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "on" | "true" | "yes" | "y" | "enabled"
        );
        config
            .memory
            .get_or_insert_with(MemoryConfig::default)
            .enabled = Some(on);
    }
    if let Ok(value) = std::env::var("MIMOFAN_ALLOW_SHELL") {
        config.allow_shell = Some(value == "1" || value.eq_ignore_ascii_case("true"));
    }
    if let Ok(value) = std::env::var("MIMOFAN_APPROVAL_POLICY") {
        config.approval_policy = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_SANDBOX_MODE") {
        config.sandbox_mode = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_YOLO") {
        config.yolo = Some(value == "1" || value.eq_ignore_ascii_case("true"));
    }
    if let Ok(value) = std::env::var("MIMOFAN_VERBOSITY") {
        config.verbosity = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_SANDBOX_BACKEND") {
        config.sandbox_backend = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_SANDBOX_URL") {
        config.sandbox_url = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_SANDBOX_API_KEY") {
        config.sandbox_api_key = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_MANAGED_CONFIG_PATH") {
        config.managed_config_path = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_SEARCH_API_KEY")
        && !value.trim().is_empty()
    {
        config
            .search
            .get_or_insert_with(SearchConfig::default)
            .api_key = Some(value);
    }
    if let Ok(value) = env_nonempty("MIMOFAN_SEARCH_BASE_URL") {
        config
            .search
            .get_or_insert_with(SearchConfig::default)
            .base_url = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_REQUIREMENTS_PATH") {
        config.requirements_path = Some(value);
    }
    if let Ok(value) = std::env::var("MIMOFAN_MAX_SUBAGENTS")
        && let Ok(parsed) = value.parse::<usize>()
    {
        config.max_subagents = Some(parsed.clamp(1, MAX_SUBAGENTS));
    }
}
