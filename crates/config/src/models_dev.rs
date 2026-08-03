//! Models.dev catalog schema and helpers.
//!
//! Models.dev is the upstream taxonomy mimofan should use for model facts,
//! provider offerings, pricing, limits, and capabilities. This module is
//! intentionally network-free: callers provide JSON from a bundled snapshot,
//! live refresh, or tests. Runtime fetch/cache policy belongs above this layer.
//!
//! The important boundary is the same one Models.dev uses:
//! - `models` are provider-agnostic model facts.
//! - `providers.*.models` are provider-scoped wire offerings.
//!
//! A provider row may inline inherited facts without exposing a canonical
//! `base_model` link. mimofan must preserve that distinction instead of
//! inferring canonical ownership from wire IDs or namespace prefixes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::route::{ModelId, ProviderId, ProviderModelOffering, RouteLimits, WireModelId};

/// Provider catalog endpoint used by Models.dev.
pub const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";
/// Provider-agnostic model metadata endpoint used by Models.dev.
pub const MODELS_DEV_MODELS_URL: &str = "https://models.dev/models.json";
/// Combined `{ models, providers }` endpoint used by Models.dev.
pub const MODELS_DEV_CATALOG_URL: &str = "https://models.dev/catalog.json";

/// Combined Models.dev catalog payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelsDevCatalog {
    /// Provider-agnostic model facts, keyed by canonical model id.
    #[serde(default)]
    pub models: BTreeMap<String, ModelsDevModel>,
    /// Provider-scoped catalogs, keyed by provider id.
    #[serde(default)]
    pub providers: BTreeMap<String, ModelsDevProvider>,
}

impl ModelsDevCatalog {
    /// Parse a Models.dev combined catalog JSON payload.
    ///
    /// # Errors
    /// Returns a serde error when the input is not valid Models.dev JSON.
    pub fn parse_json(raw: &str) -> serde_json::Result<Self> {
        serde_json::from_str(raw)
    }

    /// Look up provider-agnostic model facts by canonical model id.
    #[must_use]
    pub fn model(&self, model_id: &str) -> Option<&ModelsDevModel> {
        self.models.get(model_id.trim())
    }

    /// Look up a provider catalog by provider id.
    #[must_use]
    pub fn provider(&self, provider_id: &str) -> Option<&ModelsDevProvider> {
        self.providers.get(provider_id.trim())
    }

    /// Look up a provider-scoped wire model row.
    #[must_use]
    pub fn provider_model(
        &self,
        provider_id: &str,
        wire_model_id: &str,
    ) -> Option<&ModelsDevProviderModel> {
        self.provider(provider_id)?.models.get(wire_model_id.trim())
    }

    /// Build a route offering from a provider-scoped Models.dev row.
    ///
    /// The canonical model is set only when the row carries an explicit
    /// `base_model` id. Generated Models.dev JSON often inlines inherited facts
    /// without that link, so callers must not guess one from a prefix.
    #[must_use]
    pub fn provider_offering(
        &self,
        provider_id: &str,
        wire_model_id: &str,
    ) -> Option<ProviderModelOffering> {
        let provider_key = provider_id.trim();
        let provider = self.provider(provider_key)?;
        let model = provider.models.get(wire_model_id.trim())?;
        let provider_id = provider.effective_id(provider_key);
        Some(ProviderModelOffering {
            provider: ProviderId::from(provider_id),
            canonical_model: model.base_model.clone().map(ModelId::from),
            wire_model_id: WireModelId::from(model.id.clone()),
            endpoint_key: "chat".to_string(),
            default_for_provider: model.default_for_provider,
            limits: model
                .limit
                .as_ref()
                .map(RouteLimits::from)
                .unwrap_or_default(),
            pricing: crate::pricing::route_pricing_sku_from_cost(model.cost.as_ref()),
        })
    }

    /// Build route offerings for every normal text-chat model served by a
    /// provider.
    ///
    /// Non-chat rows (for example TTS/audio-only offerings) stay in the parsed
    /// catalog but are excluded from route resolution lists.
    #[must_use]
    pub fn provider_offerings(&self, provider_id: &str) -> Option<Vec<ProviderModelOffering>> {
        let provider_key = provider_id.trim();
        let provider = self.provider(provider_key)?;
        let provider_id = provider.effective_id(provider_key);
        Some(
            provider
                .models
                .values()
                .filter(|model| model.supports_text_chat())
                .map(|model| ProviderModelOffering {
                    provider: ProviderId::from(provider_id.clone()),
                    canonical_model: model.base_model.clone().map(ModelId::from),
                    wire_model_id: WireModelId::from(model.id.clone()),
                    endpoint_key: "chat".to_string(),
                    default_for_provider: model.default_for_provider,
                    limits: model
                        .limit
                        .as_ref()
                        .map(RouteLimits::from)
                        .unwrap_or_default(),
                    pricing: crate::pricing::route_pricing_sku_from_cost(model.cost.as_ref()),
                })
                .collect(),
        )
    }
}

/// Provider-agnostic model facts from `models.json` / `catalog.models`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelsDevModel {
    /// Canonical Models.dev model id, such as `zhipuai/glm-5.2`.
    #[serde(default)]
    pub id: String,
    /// Human-friendly model name.
    #[serde(default)]
    pub name: Option<String>,
    /// Model family, such as `glm`, `gpt`, or `claude`.
    #[serde(default)]
    pub family: Option<String>,
    /// Whether attachments are accepted.
    #[serde(default)]
    pub attachment: Option<bool>,
    /// Whether the model supports reasoning.
    #[serde(default)]
    pub reasoning: Option<bool>,
    /// Whether tool calling is supported.
    #[serde(default)]
    pub tool_call: Option<bool>,
    /// Whether structured output is supported.
    #[serde(default)]
    pub structured_output: Option<bool>,
    /// Whether temperature is supported.
    #[serde(default)]
    pub temperature: Option<bool>,
    /// Whether weights are open.
    #[serde(default)]
    pub open_weights: Option<bool>,
    /// Token limits.
    #[serde(default)]
    pub limit: Option<ModelsDevLimit>,
    /// Input/output modalities.
    #[serde(default)]
    pub modalities: Option<ModelsDevModalities>,
}

impl ModelsDevModel {
    /// True when the model can be used for normal text chat.
    #[must_use]
    pub fn supports_text_chat(&self) -> bool {
        supports_text_chat(self.modalities.as_ref())
    }
}

/// Provider-scoped model row from `api.json` / `catalog.providers.*.models`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelsDevProviderModel {
    /// Provider wire model id.
    #[serde(default)]
    pub id: String,
    /// Optional explicit canonical model link from source TOML.
    #[serde(default)]
    pub base_model: Option<String>,
    /// Human-friendly model name.
    #[serde(default)]
    pub name: Option<String>,
    /// Model family as exposed for this provider row.
    #[serde(default)]
    pub family: Option<String>,
    /// Whether this is the provider's default model in a mimofan snapshot.
    #[serde(default, alias = "default")]
    pub default_for_provider: bool,
    /// Whether attachments are accepted.
    #[serde(default)]
    pub attachment: Option<bool>,
    /// Whether the model supports reasoning.
    #[serde(default)]
    pub reasoning: Option<bool>,
    /// Flexible reasoning-control metadata.
    #[serde(default)]
    pub reasoning_options: Vec<serde_json::Value>,
    /// Whether tool calling is supported.
    #[serde(default)]
    pub tool_call: Option<bool>,
    /// Whether structured output is supported.
    #[serde(default)]
    pub structured_output: Option<bool>,
    /// Whether temperature is supported.
    #[serde(default)]
    pub temperature: Option<bool>,
    /// Whether weights are open through this offering.
    #[serde(default)]
    pub open_weights: Option<bool>,
    /// Token limits for this provider offering.
    #[serde(default)]
    pub limit: Option<ModelsDevLimit>,
    /// Input/output modalities for this provider offering.
    #[serde(default)]
    pub modalities: Option<ModelsDevModalities>,
    /// Provider-scoped pricing.
    #[serde(default)]
    pub cost: Option<ModelsDevCost>,
    /// Interleaved reasoning field hints.
    #[serde(default)]
    pub interleaved: Option<ModelsDevInterleaved>,
}

impl ModelsDevProviderModel {
    /// True when the provider offering can be used for normal text chat.
    #[must_use]
    pub fn supports_text_chat(&self) -> bool {
        supports_text_chat(self.modalities.as_ref())
    }
}

/// Provider row from Models.dev.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelsDevProvider {
    /// Provider id, such as `zai`, `zhipuai`, or `openrouter`.
    #[serde(default)]
    pub id: String,
    /// Human-friendly provider name.
    #[serde(default)]
    pub name: Option<String>,
    /// Default API base URL, if published.
    #[serde(default)]
    pub api: Option<String>,
    /// AI SDK package identifier, useful as a protocol hint.
    #[serde(default)]
    pub npm: Option<String>,
    /// Documentation URL, if published.
    #[serde(default)]
    pub doc: Option<String>,
    /// Environment variable names for credentials.
    #[serde(default)]
    pub env: Vec<String>,
    /// Provider-scoped wire model rows.
    #[serde(default)]
    pub models: BTreeMap<String, ModelsDevProviderModel>,
}

impl ModelsDevProvider {
    /// Resolve the effective provider id for this row.
    ///
    /// Models.dev snapshots usually repeat the catalog key in the `id` field,
    /// but generated JSON can omit it. Fall back to the catalog key so callers
    /// never emit an empty [`ProviderId`].
    #[must_use]
    fn effective_id(&self, provider_key: &str) -> String {
        if self.id.trim().is_empty() {
            provider_key.to_string()
        } else {
            self.id.trim().to_string()
        }
    }
}

/// Token limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelsDevLimit {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
}

impl From<&ModelsDevLimit> for RouteLimits {
    fn from(limit: &ModelsDevLimit) -> Self {
        Self {
            context_tokens: limit.context,
            input_tokens: limit.input,
            output_tokens: limit.output,
        }
    }
}

/// Input/output modalities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelsDevModalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// Provider-scoped cost fields. Values are per million tokens unless a future
/// Models.dev row specifies a richer tiering object in fields mimofan does
/// not yet model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelsDevCost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// Interleaved reasoning field metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelsDevInterleaved {
    #[serde(default)]
    pub field: Option<String>,
}

fn supports_text_chat(modalities: Option<&ModelsDevModalities>) -> bool {
    let Some(modalities) = modalities else {
        return true;
    };
    // Treat an empty modality list the same as absent metadata. An incomplete
    // catalog snapshot can deserialize to `Some({ input: [], output: [] })`,
    // and `Iterator::any` over an empty slice is `false` — without this guard
    // such rows would be silently dropped from chat offerings even though the
    // `None` branch above defaults them to chat-capable. Only an explicitly
    // populated, non-text list excludes the row.
    let input_ok = modalities.input.is_empty()
        || modalities
            .input
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("text"));
    let output_ok = modalities.output.is_empty()
        || modalities
            .output
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("text"));
    input_ok && output_ok
}
