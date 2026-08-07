//! Provider types and model routing functions.

use serde::{Deserialize, Serialize};

/// Supported wire-protocol compat modes.
///
/// 与 `mimofan_config::ProviderKind` 一一对应，仅描述 LLM 网关所说的线协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProvider {
    /// OpenAI-compatible `/v1/chat/completions` endpoint.
    OpenAiCompatible,
    /// Anthropic Messages API compatible endpoint (`/v1/messages`).
    AnthropicCompatible,
    /// Google Gemini compatible endpoint.
    GeminiCompatible,
}

impl ApiProvider {
    #[must_use]
    pub fn names_hint() -> String {
        "openai-compatible, anthropic-compatible, gemini-compatible".to_string()
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        mimofan_config::ProviderKind::parse(value).map(Self::from_kind)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.kind()
            .expect("ApiProvider always maps to a ProviderKind")
            .as_str()
    }

    /// Human-friendly label for picker UIs / status chips.
    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "OpenAI Compatible",
            Self::AnthropicCompatible => "Anthropic Compatible",
            Self::GeminiCompatible => "Gemini Compatible",
        }
    }

    /// Provider metadata from the shared config crate.
    #[must_use]
    pub fn metadata(self) -> Option<&'static dyn mimofan_config::provider::Provider> {
        self.kind().map(|kind| kind.provider())
    }

    /// Environment variable candidates for this provider's API key.
    #[must_use]
    pub fn env_vars(self) -> &'static [&'static str] {
        self.metadata()
            .map_or(&["MIMOFAN_API_KEY"][..], |provider| provider.env_vars())
    }

    /// Environment variable candidates formatted for UI copy.
    #[must_use]
    pub fn env_vars_label(self) -> String {
        self.env_vars().join(" / ")
    }

    /// Providers ordered for picker/browsing surfaces.
    #[must_use]
    pub fn sorted_for_display() -> Vec<Self> {
        mimofan_config::provider::providers_sorted_for_display()
            .iter()
            .map(|provider| Self::from_kind(provider.kind()))
            .collect()
    }

    /// Default base URL for this provider.
    #[must_use]
    pub fn default_base_url(self) -> &'static str {
        self.metadata()
            .expect("ApiProvider variant missing ProviderKind metadata")
            .default_base_url()
    }

    /// Official provider page for creating or locating credentials.
    #[must_use]
    pub fn credential_url(self) -> Option<&'static str> {
        match self {
            Self::OpenAiCompatible => Some("https://platform.openai.com/api-keys"),
            Self::AnthropicCompatible => Some("https://console.anthropic.com/settings/keys"),
            Self::GeminiCompatible => {
                Some("https://aistudio.google.com/app/apikey")
            }
        }
    }

    /// All providers in stable `ProviderKind::ALL` order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &Self::FROM_KIND_LOOKUP
    }

    /// `ProviderKind` discriminant → `ApiProvider` lookup.
    const FROM_KIND_LOOKUP: [Self; 3] = [
        Self::OpenAiCompatible,
        Self::AnthropicCompatible,
        Self::GeminiCompatible,
    ];

    /// Map to the config-level `ProviderKind`.
    #[must_use]
    pub fn kind(self) -> Option<mimofan_config::ProviderKind> {
        match self {
            Self::OpenAiCompatible => Some(mimofan_config::ProviderKind::OpenAiCompatible),
            Self::AnthropicCompatible => Some(mimofan_config::ProviderKind::AnthropicCompatible),
            Self::GeminiCompatible => Some(mimofan_config::ProviderKind::GeminiCompatible),
        }
    }

    /// Construct from a config-level `ProviderKind`.
    #[must_use]
    pub fn from_kind(kind: mimofan_config::ProviderKind) -> Self {
        match kind {
            mimofan_config::ProviderKind::OpenAiCompatible => Self::OpenAiCompatible,
            mimofan_config::ProviderKind::AnthropicCompatible => Self::AnthropicCompatible,
            mimofan_config::ProviderKind::GeminiCompatible => Self::GeminiCompatible,
        }
    }

    /// Whether this provider is a self-hosted / local runtime.
    #[must_use]
    pub fn is_self_hosted(self) -> bool {
        false
    }
}

pub(crate) fn normalize_subagent_provider_key(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| match ch {
            '-' | '_' | '.' | ' ' => '_',
            _ => ch,
        })
        .collect()
}

pub(crate) fn subagent_provider_key_matches(key: &str, provider: ApiProvider) -> bool {
    // 仅认规范 kebab-case 名（如 openai-compatible / anthropic-compatible /
    // gemini-compatible）。历史产品别名（openai/custom/mimo/anthropic/gemini/
    // google 等）已移除，符合 MECE：模式只有三种，名称唯一。
    if ApiProvider::parse(key).is_some_and(|candidate| candidate == provider) {
        return true;
    }

    normalize_subagent_provider_key(key) == normalize_subagent_provider_key(provider.as_str())
}

// ============================================================================
// Provider Capability Matrix
// ============================================================================

/// Known capabilities for a provider + resolved-model combination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderCapability {
    /// Canonical provider identifier.
    pub provider: ApiProvider,
    /// Resolved model identifier that will be sent in the API payload.
    pub resolved_model: String,
    /// Context window in tokens.
    pub context_window: u32,
    /// Official maximum output tokens for this combo.
    pub max_output: u32,
    /// Whether the provider+model supports thinking/reasoning mode.
    pub thinking_supported: bool,
    /// Whether the provider returns prompt-cache telemetry fields.
    pub cache_telemetry_supported: bool,
    /// Which request-payload dialect the provider uses.
    pub request_payload_mode: RequestPayloadMode,
    /// Deprecation metadata for compatibility aliases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_deprecation: Option<ModelAliasDeprecation>,
}

/// Upstream retirement metadata for a model alias that remains compatible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelAliasDeprecation {
    pub alias: String,
    pub replacement: String,
    pub retirement_date: String,
    pub retirement_utc: String,
    pub notice: String,
}

/// Which request-payload dialect the provider speaks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RequestPayloadMode {
    /// Standard OpenAI-compatible `/v1/chat/completions` payload.
    ChatCompletions,
    /// Native Anthropic Messages API `/v1/messages` payload.
    AnthropicMessages,
    /// Google Gemini `generativelanguage` payload.
    Gemini,
}

/// Resolve the provider capability for a given [`ApiProvider`] and resolved model string.
#[must_use]
pub fn provider_capability(provider: ApiProvider, resolved_model: &str) -> ProviderCapability {
    let request_payload_mode = match provider {
        ApiProvider::OpenAiCompatible => RequestPayloadMode::ChatCompletions,
        ApiProvider::AnthropicCompatible => RequestPayloadMode::AnthropicMessages,
        ApiProvider::GeminiCompatible => RequestPayloadMode::Gemini,
    };
    ProviderCapability {
        provider,
        resolved_model: resolved_model.to_string(),
        context_window: crate::models::context_window_for_model(resolved_model)
            .unwrap_or(crate::models::DEEPSEEK_DEFAULT_CONTEXT_WINDOW),
        max_output: crate::models::max_output_tokens_for_model(resolved_model).unwrap_or(4096),
        thinking_supported: crate::models::model_supports_reasoning(resolved_model),
        cache_telemetry_supported: false,
        request_payload_mode,
        alias_deprecation: None,
    }
}

/// Canonicalize compact DeepSeek model aliases to stable IDs.
#[must_use]
pub fn canonical_model_name(model: &str) -> Option<&'static str> {
    mimofan_config::canonical_model_name(model)
}

/// Normalize a configured/runtime model name.
#[must_use]
pub fn normalize_model_name(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(canonical) = canonical_model_name(trimmed) {
        return Some(canonical.to_string());
    }

    let normalized = trimmed.to_ascii_lowercase();
    if !normalized.starts_with("deepseek") && !normalized.contains("/deepseek") {
        return None;
    }

    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/'))
    {
        return Some(trimmed.to_string());
    }

    None
}

#[must_use]
pub(crate) fn normalize_custom_model_id(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Validate a user-requested model id against the active provider (#3018).
#[must_use]
pub fn requested_model_for_provider(_provider: ApiProvider, model: &str) -> Option<String> {
    normalize_custom_model_id(model)
}

/// Resolve a user-entered model id to the canonical family id a provider understands.
///
/// mimofan 不再绑定产品专属模型别名，模型名原样透传。
#[must_use]
pub fn canonical_model_id_for_provider(_provider: ApiProvider, model: &str) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return None;
    }

    Some(trimmed.to_string())
}

/// Normalize a model selected through the TUI for the active provider.
#[must_use]
pub fn normalize_model_name_for_provider(provider: ApiProvider, model: &str) -> Option<String> {
    canonical_model_id_for_provider(provider, model)
}

#[must_use]
pub fn wire_model_for_provider(provider: ApiProvider, model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    normalize_model_name_for_provider(provider, trimmed).unwrap_or_else(|| trimmed.to_string())
}

#[must_use]
pub fn model_completion_names_for_provider(_provider: ApiProvider) -> Vec<&'static str> {
    Vec::new()
}
