//! Offline model metadata catalog (#3072).
//!
//! This module adds a secret-free metadata layer in front of the legacy model
//! tables. It is intentionally conservative: startup reads a local cache plus a
//! bundled snapshot, never performs a network refresh, and only overrides a
//! legacy fact when the active catalog entry actually carries that field.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

const BUNDLED_CATALOG_JSON: &str = include_str!("../assets/model_catalog.bundled.json");
const OPENROUTER_CACHE_FILE: &str = "openrouter.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MetadataProvenance {
    ProviderApi,
    Bundled,
    UserOverride,
    #[default]
    Unknown,
}

impl MetadataProvenance {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderApi => "provider_api",
            Self::Bundled => "bundled",
            Self::UserOverride => "user_override",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_usd_per_million: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_usd_per_million: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modalities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_parameters: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_model_id: Option<String>,
    #[serde(default)]
    pub provenance: MetadataProvenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogCache {
    pub schema_version: u32,
    pub source: String,
    pub fetched_at: DateTime<Utc>,
    pub ttl_secs: u64,
    #[serde(default)]
    pub entries: BTreeMap<String, CatalogEntry>,
}

impl CatalogCache {
    #[must_use]
    pub fn is_stale(&self, now: DateTime<Utc>) -> bool {
        if now <= self.fetched_at {
            return false;
        }
        let ttl = Duration::seconds(self.ttl_secs.min(i64::MAX as u64) as i64);
        now.signed_duration_since(self.fetched_at) > ttl
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MergedCatalog {
    user_overrides: BTreeMap<String, CatalogEntry>,
    provider_cache: Option<CatalogCache>,
    bundled: CatalogCache,
    now: DateTime<Utc>,
}

impl MergedCatalog {
    pub(crate) fn from_sources(
        user_overrides: BTreeMap<String, CatalogEntry>,
        provider_cache: Option<CatalogCache>,
        bundled: CatalogCache,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            user_overrides,
            provider_cache,
            bundled,
            now,
        }
    }

    #[must_use]
    pub(crate) fn resolve(&self, model: &str) -> Option<&CatalogEntry> {
        if let Some(entry) = entry_for(&self.user_overrides, model) {
            return Some(entry);
        }
        if let Some(provider_cache) = self
            .provider_cache
            .as_ref()
            .filter(|cache| !cache.is_stale(self.now))
            && let Some(entry) = entry_for(&provider_cache.entries, model)
        {
            return Some(entry);
        }
        entry_for(&self.bundled.entries, model)
    }
}

fn entry_for<'a>(
    entries: &'a BTreeMap<String, CatalogEntry>,
    model: &str,
) -> Option<&'a CatalogEntry> {
    entries.get(model).or_else(|| {
        let lower = model.to_lowercase();
        (lower != model).then(|| entries.get(&lower)).flatten()
    })
}

fn active_catalog() -> &'static RwLock<MergedCatalog> {
    static ACTIVE: OnceLock<RwLock<MergedCatalog>> = OnceLock::new();
    ACTIVE.get_or_init(|| {
        RwLock::new(MergedCatalog::from_sources(
            BTreeMap::new(),
            load_cached(),
            bundled_catalog(),
            Utc::now(),
        ))
    })
}

#[must_use]
pub fn resolved_entry(model: &str) -> Option<CatalogEntry> {
    active_catalog()
        .read()
        .ok()
        .and_then(|catalog| catalog.resolve(model).cloned())
}

#[must_use]
pub fn resolved_context_window(model: &str) -> Option<u32> {
    resolved_entry(model).and_then(|entry| entry.context_window)
}

#[must_use]
pub fn resolved_max_output(model: &str) -> Option<u32> {
    resolved_entry(model).and_then(|entry| entry.max_output)
}

#[must_use]
pub fn resolved_supports_reasoning(model: &str) -> Option<bool> {
    resolved_entry(model).and_then(|entry| entry.supports_reasoning)
}

#[must_use]
pub fn resolved_usd_pricing(model: &str) -> Option<(f64, f64)> {
    let entry = resolved_entry(model)?;
    Some((entry.input_usd_per_million?, entry.output_usd_per_million?))
}

#[must_use]
pub fn provenance_for_model(model: &str) -> Option<MetadataProvenance> {
    resolved_entry(model).map(|entry| entry.provenance)
}

pub fn bundled_catalog() -> CatalogCache {
    serde_json::from_str(BUNDLED_CATALOG_JSON).expect("bundled model catalog must parse")
}

fn catalog_cache_read_path() -> Result<PathBuf> {
    Ok(mimofan_config::resolve_state_dir("catalog")?.join(OPENROUTER_CACHE_FILE))
}

fn catalog_cache_write_path() -> Result<PathBuf> {
    Ok(mimofan_config::ensure_state_dir("catalog")?.join(OPENROUTER_CACHE_FILE))
}

pub fn load_cached() -> Option<CatalogCache> {
    let path = catalog_cache_read_path().ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn store_cache(cache: &CatalogCache) -> Result<()> {
    let path = catalog_cache_write_path()?;
    let json = serde_json::to_vec_pretty(cache)?;
    write_cache_file(&path, &json)
        .with_context(|| format!("write model catalog cache {}", path.display()))
}

fn write_cache_file(path: &std::path::Path, json: &[u8]) -> std::io::Result<()> {
    crate::utils::write_atomic(path, json)
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    #[serde(default)]
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    context_length: Option<u32>,
    top_provider: Option<OpenRouterTopProvider>,
    pricing: Option<OpenRouterPricing>,
    architecture: Option<OpenRouterArchitecture>,
    #[serde(default)]
    supported_parameters: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterTopProvider {
    max_completion_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterPricing {
    prompt: Option<String>,
    completion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
}

fn normalize_openrouter_response_for_ids(
    raw: &str,
    curated_ids: &[&str],
) -> Result<Vec<CatalogEntry>> {
    let response: OpenRouterModelsResponse = serde_json::from_str(raw)?;
    let curated: BTreeSet<String> = curated_ids.iter().map(|id| id.to_lowercase()).collect();
    Ok(response
        .data
        .into_iter()
        .filter(|model| curated.contains(&model.id.to_lowercase()))
        .map(|model| {
            let (input_usd_per_million, output_usd_per_million) =
                model.pricing.as_ref().map_or((None, None), |pricing| {
                    (
                        pricing.prompt.as_deref().and_then(per_token_usd_to_million),
                        pricing
                            .completion
                            .as_deref()
                            .and_then(per_token_usd_to_million),
                    )
                });
            let modalities = model.architecture.as_ref().map_or_else(Vec::new, |arch| {
                let mut values = arch.input_modalities.clone();
                values.extend(arch.output_modalities.iter().cloned());
                values.sort();
                values.dedup();
                values
            });
            let supports_reasoning = model
                .supported_parameters
                .iter()
                .any(|param| param.eq_ignore_ascii_case("reasoning"));
            CatalogEntry {
                id: model.id.clone(),
                context_window: model.context_length,
                max_output: model
                    .top_provider
                    .as_ref()
                    .and_then(|provider| provider.max_completion_tokens),
                supports_reasoning: Some(supports_reasoning),
                input_usd_per_million,
                output_usd_per_million,
                modalities,
                supported_parameters: model.supported_parameters,
                provider_model_id: Some(model.id),
                provenance: MetadataProvenance::ProviderApi,
            }
        })
        .collect())
}

fn per_token_usd_to_million(value: &str) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .map(|per_token| per_token * 1_000_000.0)
}

#[cfg(test)]
mod tests {}
