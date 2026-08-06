//! Built-in provider metadata, keyed by wire-protocol compat mode.
//!
//! mimofan 只区分 LLM 网关「说哪种线协议」，不绑定任何具体产品。
//! 每种 [`ProviderKind`] 对应一个 [`WireFormat`]，元数据（默认
//! base_url / model / env var）仅作占位与兜底；真实端点全部来自配置。

use super::ProviderKind;

/// Wire protocol spoken by an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireFormat {
    /// OpenAI-compatible `/v1/chat/completions` payloads.
    OpenAiCompatible,
    /// Native Anthropic Messages API (`/v1/messages`).
    AnthropicCompatible,
    /// Google Gemini `generativelanguage` protocol
    /// (`/v1beta/models/<model>:generateContent`).
    GeminiCompatible,
}

/// Static metadata for a built-in compat mode.
pub trait Provider: Send + Sync {
    /// Provider enum variant represented by this entry.
    fn kind(&self) -> ProviderKind;

    /// Canonical provider identifier.
    fn id(&self) -> &'static str {
        self.kind().as_str()
    }

    /// Human-readable label for UIs and diagnostics.
    fn display_name(&self) -> &'static str;

    /// Default base URL used when no config/env/CLI override is present.
    fn default_base_url(&self) -> &'static str;

    /// Default model used when no config/env/CLI override is present.
    fn default_model(&self) -> &'static str;

    /// Environment variable candidates used for this mode's API key.
    fn env_vars(&self) -> &'static [&'static str];

    /// TOML table key under `[providers.<key>]`.
    fn provider_config_key(&self) -> &'static str;

    /// Alternate names accepted during provider resolution.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Wire protocol used by the endpoint.
    fn wire(&self) -> WireFormat {
        WireFormat::OpenAiCompatible
    }
}

macro_rules! provider {
    (
        $struct_name:ident,
        $kind:ident,
        $id:literal,
        $display_name:literal,
        $base_url:literal,
        $model:literal,
        [$($env_var:literal),* $(,)?],
        $config_key:literal,
        $wire:expr,
        aliases: [$($alias:literal),* $(,)?]
    ) => {
        /// Zero-sized metadata entry for this compat mode.
        pub struct $struct_name;

        impl Provider for $struct_name {
            fn id(&self) -> &'static str {
                $id
            }

            fn kind(&self) -> ProviderKind {
                ProviderKind::$kind
            }

            fn display_name(&self) -> &'static str {
                $display_name
            }

            fn default_base_url(&self) -> &'static str {
                $base_url
            }

            fn default_model(&self) -> &'static str {
                $model
            }

            fn env_vars(&self) -> &'static [&'static str] {
                &[$($env_var),*]
            }

            fn provider_config_key(&self) -> &'static str {
                $config_key
            }

            fn aliases(&self) -> &'static [&'static str] {
                &[$($alias),*]
            }

            fn wire(&self) -> WireFormat {
                $wire
            }
        }
    };
}

// OpenAI-compatible `/v1/chat/completions` endpoint.
//
// 涵盖所有兼容 OpenAI Chat Completions 协议的自建/第三方网关。
// 默认占位为 loopback，强制用户在配置中显式给出真实 base_url，
// 避免误打到公共主机。
provider!(
    OpenAiCompatibleProvider,
    OpenAiCompatible,
    "openai-compatible",
    "OpenAI Compatible",
    "http://localhost/v1",
    "gpt-4o",
    ["OPENAI_API_KEY", "MIMOFAN_API_KEY"],
    "openai_compatible",
    WireFormat::OpenAiCompatible,
    aliases: ["openai", "openai-compatible", "custom", "xiaomi-mimo", "mimo"]
);

// Anthropic Messages API compatible endpoint (`/v1/messages`).
provider!(
    AnthropicCompatibleProvider,
    AnthropicCompatible,
    "anthropic-compatible",
    "Anthropic Compatible",
    "https://api.anthropic.com/v1",
    "claude-sonnet-4-0",
    ["ANTHROPIC_API_KEY", "MIMOFAN_API_KEY"],
    "anthropic_compatible",
    WireFormat::AnthropicCompatible,
    aliases: ["anthropic", "anthropic-compatible"]
);

// Google Gemini compatible endpoint (`generativelanguage` protocol).
provider!(
    GeminiCompatibleProvider,
    GeminiCompatible,
    "gemini-compatible",
    "Gemini Compatible",
    "https://generativelanguage.googleapis.com/v1beta",
    "gemini-2.0-flash",
    ["GEMINI_API_KEY", "GOOGLE_API_KEY", "MIMOFAN_API_KEY"],
    "gemini_compatible",
    WireFormat::GeminiCompatible,
    aliases: ["gemini", "gemini-compatible", "google"]
);

static OPENAI_PROVIDER: OpenAiCompatibleProvider = OpenAiCompatibleProvider;
static ANTHROPIC_PROVIDER: AnthropicCompatibleProvider = AnthropicCompatibleProvider;
static GEMINI_PROVIDER: GeminiCompatibleProvider = GeminiCompatibleProvider;

static PROVIDER_REGISTRY: [&dyn Provider; 3] =
    [&OPENAI_PROVIDER, &ANTHROPIC_PROVIDER, &GEMINI_PROVIDER];

/// Return all built-in provider metadata entries in `ProviderKind::ALL` order.
///
/// This insertion order is the stable order used for internal parsing and
/// default selection. It is intentionally NOT the order user-facing UI should
/// render; for browsing/picker surfaces use [`providers_sorted_for_display`].
#[must_use]
pub fn all_providers() -> &'static [&'static dyn Provider] {
    &PROVIDER_REGISTRY
}

/// Return all built-in providers ordered for user-facing display.
///
/// Providers are sorted alphabetically (case-insensitively) by
/// [`Provider::display_name`] so model/provider browsing surfaces present a
/// neutral, predictable list rather than leading with whichever provider
/// happens to sit first in [`ProviderKind::ALL`]. The ordering policy
/// intentionally differs from internal parsing/default order:
///
/// - [`all_providers`] / [`ProviderKind::ALL`] — stable order for internal
///   matching, parsing, and default selection. Do not reorder.
/// - [`providers_sorted_for_display`] — neutral alphabetical order for UI
///   browsing.
///
/// Returns an owned `Vec` because the sorted order is computed, not static.
#[must_use]
pub fn providers_sorted_for_display() -> Vec<&'static dyn Provider> {
    let mut providers = all_providers().to_vec();
    providers.sort_by(|a, b| {
        a.display_name()
            .to_ascii_lowercase()
            .cmp(&b.display_name().to_ascii_lowercase())
    });
    providers
}

/// Find a provider by canonical id only.
#[must_use]
pub fn lookup_provider(id: &str) -> Option<&'static dyn Provider> {
    let id = id.trim();
    all_providers()
        .iter()
        .copied()
        .find(|provider| provider.id() == id)
}

/// Resolve a provider by canonical id or supported legacy alias.
#[must_use]
pub fn resolve_provider(id_or_alias: &str) -> Option<&'static dyn Provider> {
    ProviderKind::parse(id_or_alias).map(provider_for_kind)
}

/// Return metadata for a known provider kind.
#[must_use]
pub fn provider_for_kind(kind: ProviderKind) -> &'static dyn Provider {
    PROVIDER_REGISTRY
        .iter()
        .find(|p| p.kind() == kind)
        .copied()
        .expect("ProviderKind variant missing from PROVIDER_REGISTRY")
}
