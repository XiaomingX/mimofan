//! Built-in provider metadata.
//!
//! This module is a metadata foundation for collapsing provider drift over
//! time. It deliberately does not mutate request bodies or choose fallback
//! providers; runtime routing remains in `ConfigToml::resolve_runtime_options`.
//!
//! mimofan 仅内置 XiaomiMiMo provider，以及 Custom 用于用户自定义 OpenAI-compatible endpoint。

use super::{DEFAULT_XIAOMI_MIMO_BASE_URL, DEFAULT_XIAOMI_MIMO_MODEL, ProviderKind};

/// Wire protocol spoken by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireFormat {
    /// OpenAI-compatible `/v1/chat/completions` style payloads.
    ChatCompletions,
    /// OpenAI Responses API (`/responses`).
    Responses,
    /// Native Anthropic Messages API (`/v1/messages`).
    AnthropicMessages,
}

/// Static metadata for a built-in model provider.
pub trait Provider: Send + Sync {
    /// Provider enum variant represented by this entry.
    fn kind(&self) -> ProviderKind;

    /// Canonical provider identifier.
    fn id(&self) -> &'static str {
        self.kind().as_str()
    }

    /// Human-readable provider label for UIs and diagnostics.
    fn display_name(&self) -> &'static str;

    /// Default base URL used when no config/env/CLI override is present.
    fn default_base_url(&self) -> &'static str;

    /// Default model used when no config/env/CLI override is present.
    fn default_model(&self) -> &'static str;

    /// Environment variable candidates used for this provider's API key.
    fn env_vars(&self) -> &'static [&'static str];

    /// TOML table key under `[providers.<key>]`.
    fn provider_config_key(&self) -> &'static str;

    /// Alternate names accepted during provider resolution.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Wire format used by the provider.
    fn wire(&self) -> WireFormat {
        WireFormat::ChatCompletions
    }
}

macro_rules! provider {
    (
        $struct_name:ident,
        $kind:ident,
        $id:literal,
        $display_name:literal,
        $base_url:ident,
        $model:ident,
        [$($env_var:literal),* $(,)?],
        $config_key:literal,
        aliases: [$($alias:literal),* $(,)?]
    ) => {
        /// Zero-sized metadata entry for this built-in provider.
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
        }
    };
}

provider!(
    XiaomiMimo,
    XiaomiMimo,
    "xiaomi-mimo",
    "Xiaomi MiMo",
    DEFAULT_XIAOMI_MIMO_BASE_URL,
    DEFAULT_XIAOMI_MIMO_MODEL,
    [
        "XIAOMI_MIMO_TOKEN_PLAN_API_KEY",
        "MIMO_TOKEN_PLAN_API_KEY",
        "XIAOMI_MIMO_API_KEY",
        "XIAOMI_API_KEY",
        "MIMO_API_KEY",
    ],
    "xiaomi_mimo",
    aliases: ["xiaomi_mimo", "xiaomimimo", "mimo", "xiaomi"]
);

/// User-defined OpenAI-compatible endpoint (#1519).
///
/// A single dynamic provider identity for arbitrary `[providers.<name>]
/// kind="openai-compatible"` config entries. Unlike the built-in providers it
/// carries no real default base URL/model/env var: the concrete endpoint, model
/// id, and auth env var all arrive from the named `[providers.<name>]` config
/// table at route time. The placeholder base URL/model here exist only so the
/// descriptor stays well-formed (non-empty) for conformance; runtime routing
/// always supplies a `base_url_override` and a wire model id, so these
/// placeholders are never used to reach the network.
pub struct Custom;

impl Provider for Custom {
    fn id(&self) -> &'static str {
        "custom"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Custom
    }

    fn display_name(&self) -> &'static str {
        "Custom (OpenAI-compatible)"
    }

    fn default_base_url(&self) -> &'static str {
        // Placeholder only; the real endpoint comes from the named config table
        // via the route's base_url_override. Loopback so a misconfigured custom
        // provider fails closed locally rather than reaching a public host.
        "http://localhost/v1"
    }

    fn default_model(&self) -> &'static str {
        // Placeholder only; the real model id comes from config and is preserved
        // verbatim as the wire model id.
        "custom-model"
    }

    fn env_vars(&self) -> &'static [&'static str] {
        // No built-in env var: the auth env var is named per-entry via
        // `[providers.<name>] api_key_env = "..."`.
        &[]
    }

    fn provider_config_key(&self) -> &'static str {
        "custom"
    }

    fn wire(&self) -> WireFormat {
        WireFormat::ChatCompletions
    }
}

static XIAOMI_MIMO: XiaomiMimo = XiaomiMimo;
static CUSTOM: Custom = Custom;

static PROVIDER_REGISTRY: [&dyn Provider; 2] = [&XIAOMI_MIMO, &CUSTOM];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_order_is_alphabetical_by_display_name() {
        let display = providers_sorted_for_display();
        let names: Vec<String> = display
            .iter()
            .map(|p| p.display_name().to_ascii_lowercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(
            names, sorted,
            "providers_sorted_for_display must be alphabetical (case-insensitive) by display name"
        );
    }

    #[test]
    fn display_order_is_complete_and_unique() {
        let display = providers_sorted_for_display();
        assert_eq!(
            display.len(),
            all_providers().len(),
            "display order must include every built-in provider"
        );
        let mut ids: Vec<&str> = display.iter().map(|p| p.id()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(
            before,
            ids.len(),
            "display order must not contain duplicates"
        );
    }

    #[test]
    fn xiaomi_mimo_and_custom_present() {
        let providers = all_providers();
        assert!(
            providers
                .iter()
                .any(|p| p.kind() == ProviderKind::XiaomiMimo)
        );
        assert!(providers.iter().any(|p| p.kind() == ProviderKind::Custom));
    }
}
