//! Xiaomi MiMo specific base URL and API key resolution logic.

use super::{DEFAULT_XIAOMI_MIMO_BASE_URL, XIAOMI_MIMO_TOKEN_PLAN_CN_BASE_URL, normalize_base_url};

pub(crate) fn xiaomi_mimo_base_url_for_mode(mode: &str) -> Option<&'static str> {
    mimofan_config::xiaomi_mimo_base_url_for_mode(mode)
}

pub(crate) fn xiaomi_mimo_mode_uses_standard_endpoint(normalized_mode: &str) -> bool {
    mimofan_config::xiaomi_mimo_mode_uses_standard_endpoint(normalized_mode)
}

pub(crate) fn xiaomi_mimo_base_url_uses_token_plan(base_url: &str) -> bool {
    let normalized = normalize_base_url(base_url).to_ascii_lowercase();
    normalized == XIAOMI_MIMO_TOKEN_PLAN_CN_BASE_URL || normalized == DEFAULT_XIAOMI_MIMO_BASE_URL
}

pub(crate) fn xiaomi_mimo_env_var(candidates: &[&str]) -> Option<String> {
    candidates.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

pub(crate) fn xiaomi_mimo_env_api_key_for_runtime(
    mode: Option<&str>,
    base_url: Option<&str>,
) -> Option<String> {
    use mimofan_config::{XIAOMI_MIMO_STANDARD_ENV_VARS, XIAOMI_MIMO_TOKEN_PLAN_ENV_VARS};

    let normalized_mode =
        mode.map(|value| value.trim().to_ascii_lowercase().replace(['_', ' '], "-"));
    let standard_selected = normalized_mode
        .as_deref()
        .is_some_and(xiaomi_mimo_mode_uses_standard_endpoint)
        || base_url.is_some_and(xiaomi_mimo_base_url_is_pay_as_you_go);
    if standard_selected {
        return xiaomi_mimo_env_var(XIAOMI_MIMO_STANDARD_ENV_VARS);
    }

    let token_plan_selected = normalized_mode
        .as_deref()
        .and_then(xiaomi_mimo_base_url_for_mode)
        .is_some()
        || base_url.is_some_and(xiaomi_mimo_base_url_uses_token_plan);
    if token_plan_selected {
        return xiaomi_mimo_env_var(XIAOMI_MIMO_TOKEN_PLAN_ENV_VARS);
    }

    xiaomi_mimo_env_var(XIAOMI_MIMO_TOKEN_PLAN_ENV_VARS)
        .or_else(|| xiaomi_mimo_env_var(XIAOMI_MIMO_STANDARD_ENV_VARS))
}

pub(crate) fn resolve_xiaomi_mimo_base_url(
    configured: Option<String>,
    api_key: Option<&str>,
    mode: Option<&str>,
) -> String {
    let normalized_mode =
        mode.map(|value| value.trim().to_ascii_lowercase().replace(['_', ' '], "-"));
    let uses_standard_mode = normalized_mode
        .as_deref()
        .is_some_and(xiaomi_mimo_mode_uses_standard_endpoint);
    let mode_base_url = normalized_mode
        .as_deref()
        .and_then(xiaomi_mimo_base_url_for_mode);
    let uses_token_plan = xiaomi_mimo_api_key_uses_token_plan(api_key);
    match configured {
        Some(base_url) if uses_standard_mode => base_url,
        Some(base_url) if uses_token_plan && xiaomi_mimo_base_url_is_pay_as_you_go(&base_url) => {
            mode_base_url
                .unwrap_or(DEFAULT_XIAOMI_MIMO_BASE_URL)
                .to_string()
        }
        Some(base_url) => base_url,
        None => {
            if let Some(base_url) = mode_base_url {
                base_url.to_string()
            } else if uses_standard_mode {
                mimofan_config::XIAOMI_MIMO_PAY_AS_YOU_GO_BASE_URL.to_string()
            } else if uses_token_plan || api_key.is_none() {
                DEFAULT_XIAOMI_MIMO_BASE_URL.to_string()
            } else {
                mimofan_config::XIAOMI_MIMO_PAY_AS_YOU_GO_BASE_URL.to_string()
            }
        }
    }
}

pub(crate) fn xiaomi_mimo_api_key_uses_token_plan(api_key: Option<&str>) -> bool {
    api_key.is_some_and(|key| key.trim_start().starts_with("tp-"))
}

pub(crate) fn xiaomi_mimo_base_url_is_pay_as_you_go(base_url: &str) -> bool {
    matches!(
        normalize_base_url(base_url).to_ascii_lowercase().as_str(),
        "https://api.xiaomimimo.com" | "https://api.xiaomimimo.com/v1"
    )
}
