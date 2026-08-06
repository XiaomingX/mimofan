//! Provider-level configuration structs, the provider fallback chain,
//! and the per-provider config get/set helpers.
//!
//! Extracted from `lib.rs` during the config crate split
//! (CODE_STRUCTURE_ANALYSIS.md §3.3).
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfigToml {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub auth_mode: Option<String>,
    pub insecure_skip_tls_verify: Option<bool>,
    #[serde(default)]
    pub http_headers: BTreeMap<String, String>,
    pub path_suffix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<ProviderAuthSourceToml>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersToml {
    /// OpenAI-compatible `/v1/chat/completions` endpoint config.
    #[serde(default)]
    pub openai_compatible: ProviderConfigToml,
    /// Anthropic Messages API compatible endpoint (`/v1/messages`).
    #[serde(default)]
    pub anthropic_compatible: ProviderConfigToml,
    /// Google Gemini compatible endpoint config.
    #[serde(default)]
    pub gemini_compatible: ProviderConfigToml,
}

impl ProvidersToml {
    #[must_use]
    pub fn for_provider(&self, provider: ProviderKind) -> &ProviderConfigToml {
        match provider {
            ProviderKind::OpenAiCompatible => &self.openai_compatible,
            ProviderKind::AnthropicCompatible => &self.anthropic_compatible,
            ProviderKind::GeminiCompatible => &self.gemini_compatible,
        }
    }

    pub fn for_provider_mut(&mut self, provider: ProviderKind) -> &mut ProviderConfigToml {
        match provider {
            ProviderKind::OpenAiCompatible => &mut self.openai_compatible,
            ProviderKind::AnthropicCompatible => &mut self.anthropic_compatible,
            ProviderKind::GeminiCompatible => &mut self.gemini_compatible,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderConfigField {
    ApiKey,
    BaseUrl,
    Model,
    Mode,
    AuthMode,
    InsecureSkipTlsVerify,
    HttpHeaders,
    PathSuffix,
}

impl ProviderConfigField {
    fn parse(key: &str) -> Option<Self> {
        Some(match key {
            "api_key" => Self::ApiKey,
            "base_url" => Self::BaseUrl,
            "model" => Self::Model,
            "mode" => Self::Mode,
            "auth_mode" => Self::AuthMode,
            "insecure_skip_tls_verify" => Self::InsecureSkipTlsVerify,
            "http_headers" => Self::HttpHeaders,
            "path_suffix" => Self::PathSuffix,
            _ => return None,
        })
    }

    fn key(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::BaseUrl => "base_url",
            Self::Model => "model",
            Self::Mode => "mode",
            Self::AuthMode => "auth_mode",
            Self::InsecureSkipTlsVerify => "insecure_skip_tls_verify",
            Self::HttpHeaders => "http_headers",
            Self::PathSuffix => "path_suffix",
        }
    }
}

pub(crate) fn parse_provider_config_key(key: &str) -> Option<(ProviderKind, ProviderConfigField)> {
    let suffix = key.strip_prefix("providers.")?;
    let (provider_key, field_key) = suffix.split_once('.')?;
    let field = ProviderConfigField::parse(field_key)?;
    let provider = ProviderKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.provider().provider_config_key() == provider_key)?;
    Some((provider, field))
}

fn provider_config_key(provider: ProviderKind, field: ProviderConfigField) -> String {
    format!(
        "providers.{}.{}",
        provider.provider().provider_config_key(),
        field.key()
    )
}

pub(crate) fn get_provider_config_value(
    config: &ProviderConfigToml,
    field: ProviderConfigField,
) -> Option<String> {
    match field {
        ProviderConfigField::ApiKey => config.api_key.clone(),
        ProviderConfigField::BaseUrl => config.base_url.clone(),
        ProviderConfigField::Model => config.model.clone(),
        ProviderConfigField::Mode => config.mode.clone(),
        ProviderConfigField::AuthMode => config.auth_mode.clone(),
        ProviderConfigField::InsecureSkipTlsVerify => config
            .insecure_skip_tls_verify
            .map(|value| value.to_string()),
        ProviderConfigField::HttpHeaders => serialize_http_headers(&config.http_headers),
        ProviderConfigField::PathSuffix => config.path_suffix.clone(),
    }
}

pub(crate) fn get_provider_config_display_value(
    config: &ProviderConfigToml,
    field: ProviderConfigField,
) -> Option<String> {
    match field {
        ProviderConfigField::ApiKey => config.api_key.as_deref().map(redact_secret),
        ProviderConfigField::HttpHeaders => {
            serialize_http_headers_for_display(&config.http_headers)
        }
        _ => get_provider_config_value(config, field),
    }
}

pub(crate) fn set_provider_config_value(
    config: &mut ConfigToml,
    provider: ProviderKind,
    field: ProviderConfigField,
    value: &str,
) -> Result<()> {
    match field {
        ProviderConfigField::ApiKey => {
            let value = value.to_string();
            config.providers.for_provider_mut(provider).api_key = Some(value.clone());
            if provider == ProviderKind::OpenAiCompatible {
                config.api_key = Some(value);
            }
        }
        ProviderConfigField::BaseUrl => {
            let value = value.to_string();
            config.providers.for_provider_mut(provider).base_url = Some(value.clone());
            if provider == ProviderKind::OpenAiCompatible {
                config.base_url = Some(value);
            }
        }
        ProviderConfigField::Model => {
            let value = value.to_string();
            config.providers.for_provider_mut(provider).model = Some(value.clone());
            if provider == ProviderKind::OpenAiCompatible {
                config.default_text_model = Some(value);
            }
        }
        ProviderConfigField::Mode => {
            config.providers.for_provider_mut(provider).mode = Some(value.to_string());
        }
        ProviderConfigField::AuthMode => {
            config.providers.for_provider_mut(provider).auth_mode = Some(value.to_string());
        }
        ProviderConfigField::InsecureSkipTlsVerify => {
            config
                .providers
                .for_provider_mut(provider)
                .insecure_skip_tls_verify = Some(parse_bool(value)?);
        }
        ProviderConfigField::HttpHeaders => {
            let headers = parse_http_headers(value)?;
            config.providers.for_provider_mut(provider).http_headers = headers.clone();
            if provider == ProviderKind::OpenAiCompatible {
                config.http_headers = headers;
            }
        }
        ProviderConfigField::PathSuffix => {
            config.providers.for_provider_mut(provider).path_suffix = Some(value.to_string());
        }
    }
    Ok(())
}

pub(crate) fn unset_provider_config_value(
    config: &mut ConfigToml,
    provider: ProviderKind,
    field: ProviderConfigField,
) {
    match field {
        ProviderConfigField::ApiKey => {
            config.providers.for_provider_mut(provider).api_key = None;
            if provider == ProviderKind::OpenAiCompatible {
                config.api_key = None;
            }
        }
        ProviderConfigField::BaseUrl => {
            config.providers.for_provider_mut(provider).base_url = None;
            if provider == ProviderKind::OpenAiCompatible {
                config.base_url = None;
            }
        }
        ProviderConfigField::Model => {
            config.providers.for_provider_mut(provider).model = None;
            if provider == ProviderKind::OpenAiCompatible {
                config.default_text_model = None;
            }
        }
        ProviderConfigField::Mode => {
            config.providers.for_provider_mut(provider).mode = None;
        }
        ProviderConfigField::AuthMode => {
            config.providers.for_provider_mut(provider).auth_mode = None;
        }
        ProviderConfigField::InsecureSkipTlsVerify => {
            config
                .providers
                .for_provider_mut(provider)
                .insecure_skip_tls_verify = None;
        }
        ProviderConfigField::HttpHeaders => {
            config
                .providers
                .for_provider_mut(provider)
                .http_headers
                .clear();
            if provider == ProviderKind::OpenAiCompatible {
                config.http_headers.clear();
            }
        }
        ProviderConfigField::PathSuffix => {
            config.providers.for_provider_mut(provider).path_suffix = None;
        }
    }
}

pub(crate) fn insert_provider_config_values(
    out: &mut BTreeMap<String, String>,
    provider: ProviderKind,
    config: &ProviderConfigToml,
) {
    if let Some(v) = config.api_key.as_ref() {
        out.insert(
            provider_config_key(provider, ProviderConfigField::ApiKey),
            redact_secret(v),
        );
    }
    if let Some(v) = config.base_url.as_ref() {
        out.insert(
            provider_config_key(provider, ProviderConfigField::BaseUrl),
            v.clone(),
        );
    }
    if let Some(v) = config.model.as_ref() {
        out.insert(
            provider_config_key(provider, ProviderConfigField::Model),
            v.clone(),
        );
    }
    if let Some(v) = config.mode.as_ref() {
        out.insert(
            provider_config_key(provider, ProviderConfigField::Mode),
            v.clone(),
        );
    }
    if let Some(v) = config.auth_mode.as_ref() {
        out.insert(
            provider_config_key(provider, ProviderConfigField::AuthMode),
            v.clone(),
        );
    }
    if let Some(v) = config.insecure_skip_tls_verify {
        out.insert(
            provider_config_key(provider, ProviderConfigField::InsecureSkipTlsVerify),
            v.to_string(),
        );
    }
    if let Some(v) = serialize_http_headers_for_display(&config.http_headers) {
        out.insert(
            provider_config_key(provider, ProviderConfigField::HttpHeaders),
            v,
        );
    }
    if let Some(v) = config.path_suffix.as_ref() {
        out.insert(
            provider_config_key(provider, ProviderConfigField::PathSuffix),
            v.clone(),
        );
    }
}

impl ConfigToml {
    /// Resolve the first configured harness profile for a provider/model route.
    ///
    /// Callers may display or test the resolved profile, but runtime
    /// provider/model routing and prompt shaping remain unchanged.
    #[must_use]
    pub fn resolve_harness_profile(
        &self,
        provider_route: &str,
        model: &str,
    ) -> Option<&HarnessProfile> {
        self.harness_profiles
            .iter()
            .chain(built_in_harness_profiles().iter())
            .find(|profile| profile.matches_route(provider_route, model))
    }

    /// Resolve durable hotbar config into normalized 1-8 slot bindings.
    ///
    /// `known_action_ids` is supplied by the TUI action registry.
    /// Unknown actions are preserved so the UI can render a disabled
    /// `?` cell instead of silently deleting user config.
    #[must_use]
    pub fn resolve_hotbar_bindings(&self, known_action_ids: &[&str]) -> HotbarConfigResolution {
        resolve_hotbar_bindings(self.hotbar.as_deref(), known_action_ids)
    }
}

/// Ordered primary-plus-fallback provider list for provider routing.
///
/// Constructing or parsing a chain does not change
/// [`ConfigToml::resolve_runtime_options`].
#[derive(Debug, Clone, PartialEq, Eq)]

pub struct ProviderChain {
    providers: Vec<ProviderKind>,
    position: usize,
}

impl ProviderChain {
    #[must_use]
    pub fn new(active: ProviderKind, fallbacks: &[ProviderKind]) -> Self {
        let mut providers = vec![active];
        for fallback in fallbacks {
            if *fallback != active && !providers.contains(fallback) {
                providers.push(*fallback);
            }
        }
        Self {
            providers,
            position: 0,
        }
    }

    #[must_use]
    pub fn providers(&self) -> &[ProviderKind] {
        &self.providers
    }

    #[must_use]
    pub fn position(&self) -> usize {
        self.position
    }

    #[must_use]
    pub fn current(&self) -> ProviderKind {
        self.providers
            .get(self.position)
            .copied()
            .unwrap_or(self.providers[0])
    }

    #[must_use]
    pub fn has_next(&self) -> bool {
        self.position + 1 < self.providers.len()
    }

    pub fn advance(&mut self) -> Option<ProviderKind> {
        if !self.has_next() {
            return None;
        }
        self.position += 1;
        Some(self.current())
    }

    pub fn reset(&mut self) {
        self.position = 0;
    }

    #[must_use]
    pub fn is_fallback_active(&self) -> bool {
        self.position > 0
    }

    /// Count the current provider plus untried chain entries.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.providers.len() - self.position
    }
}
