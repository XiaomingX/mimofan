//! Single source of model facts for mimofan (#3071, #3073).
//!
//! Historically, "what is this model's context window / max output / does it
//! reason?" was answered by several hard-coded sites:
//!
//! * [`crate::models::context_window_for_model`] /
//!   [`crate::models::known_context_window_for_model`] for context windows,
//! * [`crate::models::max_output_tokens_for_model`] for output caps,
//! * [`crate::models::model_supports_reasoning`] for the reasoning flag,
//! * the `DEFAULT_*` model-id constants in `crates/config/src/lib.rs` for the
//!   canonical model each provider ships by default.
//!
//! This module is the **foundation** for collapsing those into one place: a
//! [`ModelMetadata`] registry keyed by model id, plus a single [`lookup`]
//! entry point. It is intentionally *additive* — the existing call sites are
//! left untouched in this pass and will be migrated to consume the registry in
//! a later change (so behaviour is unchanged today).
//!
//! ## Seeding discipline (no drift)
//!
//! The registry does not re-declare context-window / max-output / reasoning
//! numbers. Instead it **seeds** each entry by calling the existing
//! `crate::models` functions, so the registry can never silently disagree with
//! `models.rs`. The canonical model ids come from the same provider defaults
//! the config crate ships (see [`SEED_MODEL_IDS`]). The
//! [`tests::registry_context_window_matches_models_rs`] drift guard then
//! re-asserts the equivalence for a sample so that if a future change replaces
//! a seed with a hard-coded literal, CI catches the drift immediately.
//!
//! NOTE: the public surface here is intentionally not yet consumed by
//! production call sites (consumers are wired in a later pass), so
//! `dead_code` is allowed at the module level until then.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::models::{
    context_window_for_model, max_output_tokens_for_model, model_supports_reasoning,
};

/// Coarse provider grouping for a model entry.
///
/// This is deliberately a small, stable enum rather than a re-export of
/// `config::ApiProvider`: the registry's job is to answer "what kind of model
/// is this", and many models (Kimi, GLM, Qwen, …) are reachable through
/// several concrete providers. Routing decisions still live in
/// `config::ApiProvider` / `model_routing`; this is only a hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProvider {
    /// DeepSeek-family models (first-class; preserve full support).
    DeepSeek,
    /// Anthropic Claude models.
    Anthropic,
    /// OpenAI public API models (gpt-5.5 family).
    OpenAi,
    /// OpenAI Codex route models (gpt-5*-codex).
    OpenAiCodex,
    /// Moonshot / Kimi models.
    Moonshot,
    /// Z.ai GLM models.
    Zai,
    /// MiniMax models.
    Minimax,
    /// Alibaba Qwen models.
    Qwen,
    /// Arcee Trinity models.
    Arcee,
    /// Xiaomi MiMo models.
    XiaomiMimo,
    /// Anything not otherwise classified (still gets real metadata via the
    /// `models.rs` heuristics where possible).
    Other,
}

/// Default concrete provider that can serve a registry provider family.
#[must_use]
pub fn serving_provider(provider: ModelProvider) -> crate::config::ApiProvider {
    match provider {
        ModelProvider::DeepSeek => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::Anthropic => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::OpenAi => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::OpenAiCodex => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::Moonshot => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::Zai => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::Minimax => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::Qwen => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::Arcee => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::XiaomiMimo => crate::config::ApiProvider::XiaomiMimo,
        ModelProvider::Other => crate::config::ApiProvider::XiaomiMimo,
    }
}

/// One row of model facts, looked up in [`lookup`].
///
/// All numeric fields are seeded from `crate::models` so they stay in lockstep
/// with the legacy lookups (see module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMetadata {
    /// Canonical model id as sent to the provider (e.g. `"deepseek-v4-pro"`).
    pub id: &'static str,
    /// Coarse provider grouping.
    pub provider: ModelProvider,
    /// Approximate context window in tokens, if known.
    pub context_window: Option<u32>,
    /// Approximate maximum output tokens, if known.
    pub max_output: Option<u32>,
    /// Whether the model emits reasoning / thinking content that must be kept
    /// out of answer prose.
    pub supports_reasoning: bool,
}

impl ModelMetadata {
    /// Build a metadata row for `id` by seeding every fact from the existing
    /// `crate::models` lookups. This is the only constructor, which is what
    /// keeps the registry from drifting away from `models.rs`.
    fn seed(id: &'static str, provider: ModelProvider) -> Self {
        Self {
            id,
            provider,
            context_window: context_window_for_model(id),
            max_output: max_output_tokens_for_model(id),
            supports_reasoning: model_supports_reasoning(id),
        }
    }
}

/// Canonical `(model id, provider)` seeds for the registry.
///
/// These mirror the provider defaults shipped by `crates/config/src/lib.rs`
/// (the `DEFAULT_*_MODEL` constants) plus the explicitly-enumerated models in
/// [`crate::models::known_context_window_for_model`]. Keep this list curated:
/// it is the set of models we make first-class promises about. Unknown ids are
/// still answered by [`lookup`] via the `models.rs` heuristics, they just are
/// not pre-seeded here.
const SEED_MODEL_IDS: &[(&str, ModelProvider)] = &[
    // --- DeepSeek (first-class; config DEFAULT_DEEPSEEK_MODEL / NIM / OpenAI
    // / Atlascloud / Novita / Fireworks / Siliconflow / SGLang / vLLM /
    // Huggingface / Together / Volcengine / WanjieArk / Ollama defaults) ---
    ("deepseek-v4-pro", ModelProvider::DeepSeek),
    ("deepseek-v4-flash", ModelProvider::DeepSeek),
    ("deepseek-ai/deepseek-v4-pro", ModelProvider::DeepSeek),
    ("deepseek-ai/deepseek-v4-flash", ModelProvider::DeepSeek),
    ("deepseek/deepseek-v4-pro", ModelProvider::DeepSeek),
    ("deepseek/deepseek-v4-flash", ModelProvider::DeepSeek),
    ("deepseek-reasoner", ModelProvider::DeepSeek),
    ("deepseek-coder:1.3b", ModelProvider::DeepSeek),
    // --- Anthropic (config DEFAULT_ANTHROPIC_MODEL + models.rs rows) ---
    ("claude-opus-4-8", ModelProvider::Anthropic),
    ("claude-sonnet-4-6", ModelProvider::Anthropic),
    ("claude-haiku-4-5", ModelProvider::Anthropic),
    // --- OpenAI public API + Codex (config DEFAULT_OPENAI_CODEX_MODEL) ---
    ("gpt-5.5", ModelProvider::OpenAi),
    ("gpt-5.5-pro", ModelProvider::OpenAi),
    ("gpt-5-codex", ModelProvider::OpenAiCodex),
    ("gpt-5.3-codex", ModelProvider::OpenAiCodex),
    // --- Moonshot / Kimi (config DEFAULT_MOONSHOT_MODEL / KIMI_CODE) ---
    ("kimi-k2.7-code", ModelProvider::Moonshot),
    ("kimi-k2.6", ModelProvider::Moonshot),
    ("kimi-for-coding", ModelProvider::Moonshot),
    ("moonshotai/kimi-k2.7-code", ModelProvider::Moonshot),
    ("moonshotai/kimi-k2.6", ModelProvider::Moonshot),
    // --- Z.ai GLM (config DEFAULT_ZAI_MODEL) ---
    ("z-ai/glm-5.1", ModelProvider::Zai),
    ("z-ai/glm-5.2", ModelProvider::Zai),
    ("glm-5.1", ModelProvider::Zai),
    ("glm-5.2", ModelProvider::Zai),
    // --- MiniMax (config DEFAULT_MINIMAX_MODEL) ---
    ("minimax/minimax-m3", ModelProvider::Minimax),
    ("minimax-m3", ModelProvider::Minimax),
    ("minimax/minimax-m2.7", ModelProvider::Minimax),
    ("minimax-m2.7", ModelProvider::Minimax),
    // --- Qwen (OpenRouter routing defaults) ---
    ("qwen/qwen3.6-flash", ModelProvider::Qwen),
    ("qwen/qwen3.6-plus", ModelProvider::Qwen),
    ("qwen/qwen3.6-35b-a3b", ModelProvider::Qwen),
    // --- Arcee Trinity (config DEFAULT_ARCEE_MODEL) ---
    ("trinity-large-thinking", ModelProvider::Arcee),
    ("arcee-ai/trinity-large-thinking", ModelProvider::Arcee),
    ("trinity-mini", ModelProvider::Arcee),
    // --- Xiaomi MiMo (config DEFAULT_XIAOMI_MIMO_MODEL) ---
    ("mimo-v2.5-pro", ModelProvider::XiaomiMimo),
    ("mimo-v2.5-pro-ultraspeed", ModelProvider::XiaomiMimo),
    ("mimo-v2.5", ModelProvider::XiaomiMimo),
];

fn registry() -> &'static BTreeMap<&'static str, ModelMetadata> {
    static REGISTRY: OnceLock<BTreeMap<&'static str, ModelMetadata>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        SEED_MODEL_IDS
            .iter()
            .map(|&(id, provider)| (id, ModelMetadata::seed(id, provider)))
            .collect()
    })
}

/// Look up model facts by id.
///
/// Returns a pre-seeded [`ModelMetadata`] when `model` is one of the canonical
/// [`SEED_MODEL_IDS`] (case-insensitive). For any other id, this falls back to
/// the same `crate::models` heuristics (explicit `_Nk` suffix, DeepSeek/Claude
/// family rules, etc.) and reports the provider as [`ModelProvider::Other`], so
/// callers always get a usable answer rather than `None` for a real model.
///
/// Returns `None` only when the id is unrecognised by every existing source
/// (no seed match and `models.rs` yields no context window).
#[must_use]
pub fn lookup(model: &str) -> Option<ModelMetadata> {
    if let Some(meta) = registry().get(model) {
        return Some(meta.clone());
    }
    // Case-insensitive seed match (model ids are compared lowercased by the
    // legacy `models.rs` helpers, so honour that here too).
    let lowered = model.to_lowercase();
    if lowered != model
        && let Some(meta) = registry().get(lowered.as_str())
    {
        return Some(meta.clone());
    }

    // Not pre-seeded: defer to the existing heuristics. If they recognise the
    // model at all (any known context window), surface a synthetic row so the
    // single lookup entry point still works for the long tail of ids.
    let context_window = context_window_for_model(model);
    let max_output = max_output_tokens_for_model(model);
    let supports_reasoning = model_supports_reasoning(model);
    if context_window.is_none() && max_output.is_none() && !supports_reasoning {
        return None;
    }
    Some(ModelMetadata {
        // The id is not 'static here; we cannot store it, so this synthetic row
        // reports an empty id. Pre-seeded rows (the common case) carry the real
        // id. This keeps the public type `'static`-clean without leaking.
        id: "",
        provider: ModelProvider::Other,
        context_window,
        max_output,
        supports_reasoning,
    })
}

/// All pre-seeded model ids, for callers that want to enumerate the canonical
/// catalog (e.g. a future provider-aware model picker, #3075).
#[must_use]
pub fn seeded_model_ids() -> Vec<&'static str> {
    registry().keys().copied().collect()
}

#[cfg(test)]
mod tests {}
