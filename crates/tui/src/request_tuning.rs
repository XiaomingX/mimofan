#![allow(dead_code)]

//! Request-tuning capability foundation for CodeWhale providers (#3024).
//!
//! Request "tuning" here means the optional knobs a caller can attach to an
//! outbound model request that shape *how* the model responds without changing
//! *what* it is asked: the reasoning-effort tier and the maximum number of
//! output tokens. Different providers honor these knobs to wildly different
//! degrees, and today several of them silently drop the request — the user
//! turns a dial, the wire request omits it, and nothing changes. Issue #3024
//! tracks exactly these silent no-ops (reasoning-effort and token-limit being
//! ignored for some providers).
//!
//! This module is a **pure, declarative foundation**. It does not build or
//! mutate request bodies; that lives in `client.rs`
//! ([`apply_reasoning_effort`]) and `client/chat.rs`
//! ([`apply_provider_token_limit`]). Instead it *documents and encodes* which
//! providers honor which tuning parameters, so the silent gaps can later be
//! surfaced (e.g. a warning when a user sets a dial a provider ignores) and
//! fixed deliberately rather than by accident. Consumers are intentionally not
//! wired yet — matching the pattern in `features.rs`, this is a metadata
//! foundation only.
//!
//! ## Reasoning-effort enum reuse
//!
//! [`RequestTuning::reasoning_effort`] reuses the canonical
//! [`crate::tui::app::ReasoningEffort`] enum rather than defining a local
//! `Off/Low/Medium/High` copy. That enum is the single source of truth for the
//! effort tiers across the DeepSeek and Codex effort pickers, it is already
//! imported by sibling top-level modules (`auto_reasoning`, `model_routing`),
//! and it carries the provider-normalization logic (`normalize_for_provider`,
//! `api_value_for_provider`) that a future request-tuning consumer will need.
//! Defining a parallel local enum here would duplicate that surface and risk
//! drift, so we import the existing type.
//!
//! ## What "honors" means
//!
//! A provider *honors* a tuning parameter when changing it produces a
//! correspondingly different wire request that the provider acts on:
//!
//! * `honors_reasoning_effort` — the chosen effort *tier* reaches the provider
//!   in a form it distinguishes. A provider that only toggles thinking on/off
//!   (collapsing every non-`Off` tier into a single "enabled" state) or that
//!   drops the field entirely does **not** honor it, because moving between
//!   Low / Medium / High / Max changes nothing observable.
//! * `honors_max_output_tokens` — the requested output-token ceiling reaches
//!   the provider in the field it actually reads, capping generation length.

use crate::tui::app::ReasoningEffort;

/// Optional request-tuning knobs a caller may attach to a model request.
///
/// Both fields are `Option`: `None` means "do not tune; use the provider
/// default". This is metadata describing intent — applying it to a wire
/// request is the responsibility of the client layer, not this module.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestTuning {
    /// Desired reasoning-effort tier, or `None` for the provider default.
    ///
    /// Reuses the canonical [`ReasoningEffort`] enum (see module docs).
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Desired maximum number of output tokens, or `None` for the provider
    /// default.
    pub max_output_tokens: Option<u32>,
}

impl RequestTuning {
    /// A tuning request that asks for no changes (both knobs left at the
    /// provider default).
    #[must_use]
    pub const fn untuned() -> Self {
        Self {
            reasoning_effort: None,
            max_output_tokens: None,
        }
    }

    /// Whether this request asks to tune anything at all.
    #[must_use]
    pub const fn is_tuned(&self) -> bool {
        self.reasoning_effort.is_some() || self.max_output_tokens.is_some()
    }
}

/// Which request-tuning parameters a given provider honors.
///
/// This is the per-provider capability row. A `false` here means a caller can
/// set the corresponding [`RequestTuning`] field but the provider will not act
/// on it — the silent no-op #3024 is about. Encoding it lets future code warn
/// instead of dropping the value invisibly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuningSupport {
    /// `true` when the provider distinguishes reasoning-effort *tiers* on the
    /// wire (not merely an on/off thinking toggle).
    pub honors_reasoning_effort: bool,
    /// `true` when the provider reads the requested output-token ceiling.
    pub honors_max_output_tokens: bool,
}

impl TuningSupport {
    /// The conservative default for unknown / unlisted providers: assume a
    /// plain OpenAI-compatible endpoint that ignores both tuning knobs. This
    /// matches the #3024 failure mode (silent no-op) and is the safe baseline
    /// to warn against until a provider is explicitly characterized.
    pub const UNKNOWN: Self = Self {
        honors_reasoning_effort: false,
        honors_max_output_tokens: false,
    };
}

/// Report which request-tuning parameters `provider_name` honors.
///
/// `provider_name` is matched (case-insensitively) against the canonical
/// provider id strings used elsewhere in the codebase — e.g.
/// [`crate::config::ApiProvider::as_str`] / `ProviderKind::as_str`, which yield
/// `"deepseek"`, `"openai"`, `"moonshot"`, `"ollama"`, `"atlascloud"`, etc.
/// Unknown names fall back to [`TuningSupport::UNKNOWN`].
///
/// ## Documented rows (grounded in current client behavior, #3024)
///
/// | provider     | reasoning effort | max output tokens | notes |
/// |--------------|------------------|-------------------|-------|
/// | `deepseek`   | yes              | yes               | Emits `reasoning_effort` + `thinking`; sends `max_tokens`. The reference "honors both" provider. |
/// | `openai`     | no               | no                | `apply_reasoning_effort` emits nothing for OpenAI; the plain `max_tokens` field is not the ceiling its reasoning models read (they expect `max_completion_tokens`, which is not sent for this provider). Both knobs are silent no-ops. |
/// | `moonshot`   | no               | no                | Kimi only toggles `thinking` enabled/disabled — every non-`Off` tier collapses to "enabled", so the effort *tier* is not honored. Token ceiling flagged by #3024. |
/// | `ollama`     | no               | no                | Only sets a `think: true/false` boolean — the effort tier is dropped. Token ceiling flagged by #3024. |
/// | `atlascloud` | no               | no                | Speaks the DeepSeek dialect but collapses Low/Medium → `high` (lossy: the chosen tier is not preserved) and the token ceiling is flagged by #3024. |
///
/// The `false` cells are the deliberate gaps #3024 exists to fix; this table
/// is the record of intent, not a behavior change.
#[must_use]
pub fn provider_tuning_support(provider_name: &str) -> TuningSupport {
    match provider_name.trim().to_ascii_lowercase().as_str() {
        // DeepSeek is the reference implementation: both knobs reach the wire
        // and are acted on.
        "deepseek" | "deepseek-cn" | "deepseekcn" => TuningSupport {
            honors_reasoning_effort: true,
            honors_max_output_tokens: true,
        },

        // #3024 gaps — each ignores at least one tuning knob today.
        "openai" => TuningSupport {
            honors_reasoning_effort: false,
            honors_max_output_tokens: false,
        },
        "moonshot" => TuningSupport {
            honors_reasoning_effort: false,
            honors_max_output_tokens: false,
        },
        "ollama" => TuningSupport {
            honors_reasoning_effort: false,
            honors_max_output_tokens: false,
        },
        "atlascloud" => TuningSupport {
            honors_reasoning_effort: false,
            honors_max_output_tokens: false,
        },

        _ => TuningSupport::UNKNOWN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_tuning_default_is_untuned() {
        let tuning = RequestTuning::default();
        assert_eq!(tuning, RequestTuning::untuned());
        assert_eq!(tuning.reasoning_effort, None);
        assert_eq!(tuning.max_output_tokens, None);
        assert!(!tuning.is_tuned());
    }

    #[test]
    fn request_tuning_reuses_reasoning_effort_enum() {
        let tuning = RequestTuning {
            reasoning_effort: Some(ReasoningEffort::High),
            max_output_tokens: Some(4096),
        };
        assert_eq!(tuning.reasoning_effort, Some(ReasoningEffort::High));
        assert_eq!(tuning.max_output_tokens, Some(4096));
        assert!(tuning.is_tuned());
    }

    #[test]
    fn deepseek_honors_both_tuning_params() {
        let support = provider_tuning_support("deepseek");
        assert!(support.honors_reasoning_effort);
        assert!(support.honors_max_output_tokens);
    }

    #[test]
    fn deepseek_cn_alias_matches_deepseek() {
        assert_eq!(
            provider_tuning_support("deepseek-cn"),
            provider_tuning_support("deepseek")
        );
        assert_eq!(
            provider_tuning_support("deepseekcn"),
            provider_tuning_support("deepseek")
        );
    }

    #[test]
    fn openai_honors_neither_tuning_param() {
        // #3024: reasoning-effort emits nothing and the plain max_tokens field
        // is not the ceiling OpenAI's reasoning models read.
        let support = provider_tuning_support("openai");
        assert!(!support.honors_reasoning_effort);
        assert!(!support.honors_max_output_tokens);
    }

    #[test]
    fn moonshot_does_not_honor_reasoning_effort_tier() {
        // Kimi only toggles thinking on/off; the effort tier is collapsed.
        let support = provider_tuning_support("moonshot");
        assert!(!support.honors_reasoning_effort);
        assert!(!support.honors_max_output_tokens);
    }

    #[test]
    fn ollama_does_not_honor_reasoning_effort_tier() {
        // Ollama only sets a think boolean; the effort tier is dropped.
        let support = provider_tuning_support("ollama");
        assert!(!support.honors_reasoning_effort);
        assert!(!support.honors_max_output_tokens);
    }

    #[test]
    fn atlascloud_does_not_honor_tuning_params() {
        // Speaks the DeepSeek dialect but collapses Low/Medium -> high (lossy)
        // and the token ceiling is flagged by #3024.
        let support = provider_tuning_support("atlascloud");
        assert!(!support.honors_reasoning_effort);
        assert!(!support.honors_max_output_tokens);
    }

    #[test]
    fn unknown_provider_falls_back_to_conservative_default() {
        let support = provider_tuning_support("totally-unknown-provider");
        assert_eq!(support, TuningSupport::UNKNOWN);
        assert!(!support.honors_reasoning_effort);
        assert!(!support.honors_max_output_tokens);
    }

    #[test]
    fn provider_name_match_is_case_and_whitespace_insensitive() {
        assert_eq!(
            provider_tuning_support("  DeepSeek  "),
            provider_tuning_support("deepseek")
        );
        assert_eq!(
            provider_tuning_support("OpenAI"),
            provider_tuning_support("openai")
        );
    }
}
