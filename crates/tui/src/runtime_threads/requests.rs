//! Request/response types for the runtime thread API.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use mimofan_protocol::runtime::{DynamicToolSpec, TurnEnvironmentParams};

use super::types::{ThreadRecord, TurnItemRecord, TurnRecord};
use crate::config::MAX_ACTIVE_THREADS_DEFAULT;

#[derive(Debug, Clone)]
pub struct RuntimeThreadManagerConfig {
    pub data_dir: PathBuf,
    pub task_data_dir: PathBuf,
    pub max_active_threads: usize,
}

impl RuntimeThreadManagerConfig {
    #[must_use]
    pub fn from_task_data_dir(task_data_dir: PathBuf) -> Self {
        let data_dir = if let Ok(override_dir) = std::env::var("MIMOFAN_RUNTIME_DIR") {
            if override_dir.trim().is_empty() {
                task_data_dir.join("runtime")
            } else {
                PathBuf::from(override_dir)
            }
        } else {
            task_data_dir.join("runtime")
        };
        Self {
            data_dir,
            task_data_dir,
            max_active_threads: MAX_ACTIVE_THREADS_DEFAULT,
        }
    }
}

/// Visibility filter for `list_threads`. Default is `ActiveOnly`. The runtime
/// API exposes this as the combination of `include_archived` and
/// `archived_only` query params (see `runtime_api.rs`); whalescale#260 / #563.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThreadListFilter {
    /// Only `archived = false` threads. The original default.
    #[default]
    ActiveOnly,
    /// Active and archived threads, sorted as the store returns them.
    IncludeArchived,
    /// Only `archived = true` threads.
    ArchivedOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateThreadRequest {
    pub model: Option<String>,
    pub workspace: Option<PathBuf>,
    pub mode: Option<String>,
    pub allow_shell: Option<bool>,
    pub trust_mode: Option<bool>,
    pub auto_approve: Option<bool>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub dynamic_tools: Vec<DynamicToolSpec>,
    #[serde(default)]
    pub environments: Vec<TurnEnvironmentParams>,
}

/// Mutable fields accepted by `PATCH /v1/threads/{id}`.
///
/// Each field is optional — missing means "no change". Extended in v0.8.10
/// (#562, whalescale#256) so the UI can flip persistent thread state without
/// having to recreate a thread or pass per-turn overrides on every send.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateThreadRequest {
    pub archived: Option<bool>,
    pub allow_shell: Option<bool>,
    pub trust_mode: Option<bool>,
    pub auto_approve: Option<bool>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub title: Option<String>,
    pub system_prompt: Option<String>,
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StartTurnRequest {
    pub prompt: String,
    #[serde(default)]
    pub input_summary: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub allow_shell: Option<bool>,
    pub trust_mode: Option<bool>,
    pub auto_approve: Option<bool>,
    #[serde(default)]
    pub dynamic_tools: Vec<DynamicToolSpec>,
    #[serde(default)]
    pub environment_id: Option<String>,
    /// OpenAI-compatible `response_format` (e.g.
    /// `{"type":"json_object"}` for JSON mode). The Anthropic Messages
    /// dialect ignores this field by design; the engine routes that
    /// provider through `build_anthropic_body` instead.
    #[serde(default)]
    pub response_format: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerTurnRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompactThreadRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadDetail {
    pub thread: ThreadRecord,
    pub turns: Vec<TurnRecord>,
    pub items: Vec<TurnItemRecord>,
    pub latest_seq: u64,
}

/// Aggregation key for `aggregate_usage`. Mimofanscale#261 / #564.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageGroupBy {
    Day,
    Model,
    Provider,
    Thread,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: f64,
    pub turns: u64,
    /// Prefix-cache hit rate in [0,1]: `cached_tokens / (input_tokens + cached_tokens)`.
    /// Derived for #646 — `cached_tokens` was collected but never turned into a ratio.
    pub prefix_cache_hit_rate: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UsageBucket {
    pub key: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: f64,
    pub turns: u64,
    /// Prefix-cache hit rate in [0,1]. See `UsageTotals::prefix_cache_hit_rate`.
    pub prefix_cache_hit_rate: f64,
}

/// Derive the prefix-cache hit rate from prompt-input vs. cache-hit token counts.
///
/// Returns `0.0` when there are no prompt tokens at all (neither fresh input nor
/// cache hits), so callers never divide by zero. See #646.
pub fn prefix_cache_hit_rate(input_tokens: u64, cached_tokens: u64) -> f64 {
    let denom = input_tokens.saturating_add(cached_tokens);
    if denom == 0 {
        0.0
    } else {
        cached_tokens as f64 / denom as f64
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageAggregation {
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub group_by: String,
    pub totals: UsageTotals,
    pub buckets: Vec<UsageBucket>,
}

/// Best-effort provider classification from a model name. Used as a grouping
/// key for `/v1/usage?group_by=provider`. Cost-tracking already runs the
/// model→pricing→cost path; this only labels the bucket.
pub fn provider_label_for_model(model: &str) -> &'static str {
    if model.starts_with("deepseek-ai/") {
        "nvidia-nim"
    } else if model.starts_with("deepseek-") {
        "deepseek"
    } else if model.starts_with("openai/") || model.starts_with("anthropic/") {
        "openrouter"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_cache_hit_rate_basic() {
        // 100 fresh input + 300 cache hits => 75% hit rate.
        assert!((prefix_cache_hit_rate(100, 300) - 0.75).abs() < 1e-9);
    }

    #[test]
    fn prefix_cache_hit_rate_all_cached() {
        assert!((prefix_cache_hit_rate(0, 500) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn prefix_cache_hit_rate_none_cached() {
        assert!((prefix_cache_hit_rate(500, 0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn prefix_cache_hit_rate_zero_denominator_is_safe() {
        // No tokens at all: must not divide by zero, returns 0.0.
        assert_eq!(prefix_cache_hit_rate(0, 0), 0.0);
    }

    #[test]
    fn usage_totals_default_hit_rate_is_zero() {
        let t = UsageTotals::default();
        assert_eq!(t.prefix_cache_hit_rate, 0.0);
    }

    #[test]
    fn usage_bucket_default_hit_rate_is_zero() {
        let b = UsageBucket::default();
        assert_eq!(b.prefix_cache_hit_rate, 0.0);
    }
}
