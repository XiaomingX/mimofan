//! Environment variable overrides for configuration values.

use anyhow::Result;

use super::subagent_limits::MAX_SUBAGENTS;
use super::{
    ApiProvider, Config, LimitsConfig, MemoryConfig, ProvidersConfig, SearchConfig,
    parse_http_headers,
};

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
        config
            .provider_config_for_mut(config.api_provider())
            .base_url = Some(value);
    }
    // 各厂商专用 env 变量统一归入对应线协议槽位（不再绑定具体产品）。
    let set_base_url = |config: &mut Config, field: &str, value: String| {
        let providers = config
            .providers
            .get_or_insert_with(ProvidersConfig::default);
        match field {
            "openai_compatible" => providers.openai_compatible.base_url = Some(value),
            "anthropic_compatible" => providers.anthropic_compatible.base_url = Some(value),
            "gemini_compatible" => providers.gemini_compatible.base_url = Some(value),
            name => {
                providers
                    .custom
                    .entry(name.to_string())
                    .or_default()
                    .base_url = Some(value);
            }
        }
    };
    let set_model = |config: &mut Config, field: &str, value: String| {
        let providers = config
            .providers
            .get_or_insert_with(ProvidersConfig::default);
        match field {
            "openai_compatible" => providers.openai_compatible.model = Some(value),
            "anthropic_compatible" => providers.anthropic_compatible.model = Some(value),
            "gemini_compatible" => providers.gemini_compatible.model = Some(value),
            name => {
                providers.custom.entry(name.to_string()).or_default().model = Some(value);
            }
        }
    };
    if let Some(value) = std::env::var("OPENAI_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        set_base_url(config, "openai_compatible", value);
    }
    if let Some(value) = std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        set_base_url(config, "anthropic_compatible", value);
    }
    if let Some(value) = std::env::var("GEMINI_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        set_base_url(config, "gemini_compatible", value);
    }
    // 其余历史厂商 endpoint 变量按 OpenAI 兼容自定义端点收口。
    for (env_var, slot) in [
        ("NVIDIA_NIM_BASE_URL", "nvidia_nim"),
        ("NIM_BASE_URL", "nvidia_nim"),
        ("NVIDIA_BASE_URL", "nvidia_nim"),
        ("OPENROUTER_BASE_URL", "openrouter"),
        ("VOLCENGINE_BASE_URL", "volcengine"),
        ("VOLCENGINE_ARK_BASE_URL", "volcengine"),
        ("ARK_BASE_URL", "volcengine"),
        ("MOONSHOT_BASE_URL", "moonshot"),
        ("KIMI_BASE_URL", "moonshot"),
        ("SILICONFLOW_BASE_URL", "siliconflow"),
        // Deprecated product-specific alias; listed before the generic key so
        // `OPENAI_COMPATIBLE_BASE_URL` wins when both are set.
        ("XIAOMI_MIMO_BASE_URL", "openai_compatible"),
        ("OPENAI_COMPATIBLE_BASE_URL", "openai_compatible"),
    ] {
        if let Some(value) = std::env::var(env_var).ok().filter(|v| !v.trim().is_empty()) {
            set_base_url(config, slot, value);
        }
    }
    if let Ok(value) = std::env::var("MIMOFAN_HTTP_HEADERS")
        && let Ok(headers) = parse_http_headers(&value)
        && !headers.is_empty()
    {
        let mut root_headers = config.http_headers.clone().unwrap_or_default();
        root_headers.extend(headers.clone());
        config.http_headers = Some(root_headers);

        let provider = config.api_provider();
        let providers = config
            .providers
            .get_or_insert_with(ProvidersConfig::default);
        let entry = match provider {
            ApiProvider::OpenAiCompatible => &mut providers.openai_compatible,
            ApiProvider::AnthropicCompatible => &mut providers.anthropic_compatible,
            ApiProvider::GeminiCompatible => &mut providers.gemini_compatible,
        };
        let mut provider_headers = entry.http_headers.clone().unwrap_or_default();
        provider_headers.extend(headers);
        entry.http_headers = Some(provider_headers);
    }
    if let Ok(value) = std::env::var("OPENAI_MODEL") {
        set_model(config, "openai_compatible", value);
    }
    if let Ok(value) = std::env::var("ANTHROPIC_MODEL") {
        set_model(config, "anthropic_compatible", value);
    }
    if let Ok(value) = std::env::var("GEMINI_MODEL") {
        set_model(config, "gemini_compatible", value);
    }
    for (env_var, slot) in [
        ("OPENROUTER_MODEL", "openrouter"),
        ("VOLCENGINE_MODEL", "volcengine"),
        ("VOLCENGINE_ARK_MODEL", "volcengine"),
        ("MOONSHOT_MODEL", "moonshot"),
        ("KIMI_MODEL_NAME", "moonshot"),
        ("KIMI_MODEL", "moonshot"),
        ("SILICONFLOW_MODEL", "siliconflow"),
        // Deprecated product-specific alias; listed before the generic key so
        // `OPENAI_COMPATIBLE_MODEL` wins when both are set.
        ("XIAOMI_MIMO_MODEL", "openai_compatible"),
        ("OPENAI_COMPATIBLE_MODEL", "openai_compatible"),
    ] {
        if let Some(value) = std::env::var(env_var).ok().filter(|v| !v.trim().is_empty()) {
            set_model(config, slot, value);
        }
    }
    if let Some(value) = env_nonempty("MIMOFAN_MODEL").ok().or_else(|| {
        std::env::var("MIMOFAN_DEFAULT_TEXT_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
    }) {
        let provider = config.api_provider();
        match provider {
            ApiProvider::OpenAiCompatible => {
                let providers = config
                    .providers
                    .get_or_insert_with(ProvidersConfig::default);
                if let Some(name) = config.provider.clone()
                    && providers.custom.contains_key(&name)
                {
                    providers.custom.entry(name).or_default().model = Some(value);
                } else {
                    config.default_text_model = Some(value);
                }
            }
            other => {
                config.provider_config_for_mut(other).model = Some(value);
            }
        }
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
    if let Ok(value) = std::env::var("MIMOFAN_MEMORY_DIR") {
        config.memory_dir = Some(value);
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
        config
            .limits
            .get_or_insert_with(LimitsConfig::default)
            .max_subagents = Some(parsed.clamp(1, MAX_SUBAGENTS));
    }
}
