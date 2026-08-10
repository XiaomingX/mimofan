//! Unified context-budget math for the TUI.
//!
//! Given a model's context window, the current input token estimate, and a
//! configured output cap, [`ContextBudget`] derives the four numbers the rest
//! of the app needs to reason about a turn:
//!
//!   * **available input budget** — how many input tokens may still be spent
//!     after reserving room for the model's output;
//!   * **output token cap** — the output reservation actually used to compute
//!     that budget (clamped so it never starves the window);
//!   * **compaction trigger** — the input-token level at which compaction
//!     should be suggested (default: ~75% of the window);
//!   * **[`PressureLevel`]** — a coarse Low/Medium/High/Critical signal the UI
//!     can render without re-deriving thresholds.
//!
//! This module is the budget-math *foundation*. It is intentionally pure (no
//! I/O, no clock, no engine/config types) so it can be unit-tested in isolation
//! and later consumed by the engine capacity checkpoints and the TUI pressure
//! indicator. Those consumers are wired in a separate pass; nothing here calls
//! into them.
//!
//! ### Why the output reservation is window-dependent
//!
//! The engine's existing input-budget helper
//! (`core::engine::context::context_input_budget_for_window`) computes
//! `window - reserved_output - headroom` and learned the hard way that
//! reserving a large fixed output (262K for V4-class interleaved thinking) on a
//! *small* self-hosted window (e.g. a 256K vLLM deployment) underflows to a
//! negative budget and silently disables every preflight/recovery path. We
//! mirror that lesson here with saturating arithmetic and an output cap that is
//! always clamped to leave at least [`MIN_INPUT_BUDGET_TOKENS`] of input room,
//! so the budget can never collapse to zero on a legitimately sized window.

// Foundation module: the public surface is exercised by unit tests but is not
// yet referenced by the engine capacity checkpoints or the TUI pressure
// indicator (those consumers are wired in a later pass). Allow dead_code so the
// substrate can land warning-clean ahead of its callers, matching how other
// not-yet-wired primitives in this crate are gated.
//
// Note: the context report now consumes `PressureLevel::from_usage_percent` and
// `label`, but the rest of the substrate (`ContextBudget` and its methods,
// `PressureLevel::suggests_compaction`) is still pending its engine/TUI
// consumers, so the blanket allow stays until those land.

/// Fraction of the window, expressed as a percentage, at or above which
/// compaction should be suggested. Mirrors the "high" pressure boundary the
/// existing context report uses for its diagnostic label, rounded up to the
/// conventional three-quarters-full trigger.
pub const DEFAULT_COMPACTION_TRIGGER_PERCENT: f64 = 75.0;

/// Percentage of the window at or above which pressure is [`PressureLevel::Critical`].
pub const CRITICAL_PRESSURE_PERCENT: f64 = 90.0;

/// Percentage of the window at or above which pressure is [`PressureLevel::High`].
/// This is the compaction trigger by default, so High and "compaction
/// suggested" coincide at the seeded thresholds.
pub const HIGH_PRESSURE_PERCENT: f64 = DEFAULT_COMPACTION_TRIGGER_PERCENT;

/// Percentage of the window at or above which pressure is [`PressureLevel::Medium`].
/// Matches the "moderate" boundary of the existing diagnostic report.
pub const MEDIUM_PRESSURE_PERCENT: f64 = 40.0;

/// Safety headroom (tokens) subtracted from the window in addition to the
/// reserved output, to avoid bumping a provider's hard limit. Matches the
/// engine's `CONTEXT_HEADROOM_TOKENS`.
pub const CONTEXT_HEADROOM_TOKENS: u64 = 1_024;

/// UI-facing context-pressure thresholds (window-usage percentage).
///
/// These describe *when to surface a visual warning to the user*, which is a
/// deliberately more conservative band than the budget-level compaction
/// trigger (`HIGH_PRESSURE_PERCENT` / `DEFAULT_COMPACTION_TRIGGER_PERCENT`).
/// The two are intentionally distinct: the engine may compact at 75% of the
/// window to protect the V4 prefix cache, but the UI shouldn't start flashing
/// "high" until 85% so users aren't alarmed by normal long-session growth.
///
/// These constants are the single source of truth for the previously
/// copy-pasted `CONTEXT_WARNING_THRESHOLD_PERCENT` / `CONTEXT_CRITICAL_THRESHOLD_PERCENT`
/// literals in `tui/ui/mod.rs`, `tui/widgets/header.rs`, and
/// `tui/context_inspector.rs`. Reuse them so the warning band can be tuned in
/// one place.
pub const UI_WARNING_PERCENT: f64 = 85.0;
/// Window-usage percentage at or above which the UI shows a critical warning.
pub const UI_CRITICAL_PERCENT: f64 = 95.0;
/// Window-usage percentage at or above which the UI suggests compacting.
pub const UI_SUGGEST_COMPACT_PERCENT: f64 = 60.0;

/// Smallest input budget (tokens) [`ContextBudget`] will report for any window
/// large enough to hold it. The output cap is clamped down as needed to
/// preserve this much input room, so a generous configured output cap can never
/// drive the available input budget to zero on a usable window.
pub const MIN_INPUT_BUDGET_TOKENS: u64 = 1_024;

// ── Token estimation ──────────────────────────────────────────────────────────
//
// Token *counts* are no longer derived here. The whole app routes through the
// shared tiktoken BPE estimator (see the tokeniser module), so this module owns
// only budget thresholds and pressure-level classification — it never estimates
// tokens itself. The estimates that used to live here (a chars/4 and a
// chars/3 heuristic with their `CHARS_PER_TOKEN_*` constants) had zero
// production callers and were removed; keeping a char-count heuristic alive
// next to the real tokenizer would have re-introduced the very divergence that
// the single-estimator cleanup fixed.

/// Coarse, UI-facing description of how full the context window is.
///
/// Ordered from least to most pressure so the variants can be compared
/// (`level >= PressureLevel::High`) and so the derived `Ord` matches intuition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PressureLevel {
    /// Plenty of room; nothing to surface.
    Low,
    /// Noticeably filling up; informational.
    Medium,
    /// At or past the compaction trigger; suggest compaction.
    High,
    /// Near the window limit; compaction/clear is urgent.
    Critical,
}

impl PressureLevel {
    /// Classify a window-usage percentage (0.0..=100.0) into a level.
    ///
    /// Inputs outside the range are clamped, so callers may pass a raw
    /// percentage without pre-validating it.
    #[must_use]
    pub fn from_usage_percent(percent: f64) -> Self {
        let percent = percent.clamp(0.0, 100.0);
        if percent >= CRITICAL_PRESSURE_PERCENT {
            PressureLevel::Critical
        } else if percent >= HIGH_PRESSURE_PERCENT {
            PressureLevel::High
        } else if percent >= MEDIUM_PRESSURE_PERCENT {
            PressureLevel::Medium
        } else {
            PressureLevel::Low
        }
    }

    /// Lowercase, stable label suitable for status lines and logs.
    ///
    /// Kept aligned with the existing context-report vocabulary
    /// (`low`/`moderate`/`high`/`critical`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            PressureLevel::Low => "low",
            PressureLevel::Medium => "moderate",
            PressureLevel::High => "high",
            PressureLevel::Critical => "critical",
        }
    }

    /// Whether this level is at or past the point where compaction should be
    /// suggested to the user.
    #[must_use]
    pub const fn suggests_compaction(self) -> bool {
        matches!(self, PressureLevel::High | PressureLevel::Critical)
    }
}

/// A computed snapshot of how a turn's input sits against a model's context
/// window, plus the derived output cap, compaction trigger, and pressure level.
///
/// Construct via [`ContextBudget::new`]. All fields are token counts unless the
/// name says otherwise. The struct is `Copy` and holds no borrowed data so it
/// can be cached on UI state cheaply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    /// Total context window for the active route (input + output), in tokens.
    pub window_tokens: u64,
    /// Current estimated input tokens already committed to the turn.
    pub input_tokens: u64,
    /// Output tokens reserved (and thus the effective output cap) for the turn.
    /// Derived from the configured cap, clamped to fit the window while leaving
    /// at least [`MIN_INPUT_BUDGET_TOKENS`] of input room.
    pub output_cap_tokens: u64,
    /// Input tokens still available before hitting the reserved boundary
    /// (`window - output_cap - headroom - input`, saturating at 0).
    pub available_input_tokens: u64,
    /// Input-token level at or above which compaction should be suggested
    /// (`DEFAULT_COMPACTION_TRIGGER_PERCENT` of the window).
    pub compaction_trigger_tokens: u64,
    /// Coarse pressure level derived from window usage.
    pub pressure: PressureLevel,
}

impl ContextBudget {
    /// Build a budget snapshot for a route.
    ///
    /// * `window_tokens` — the route-effective context window (input + output).
    /// * `input_tokens` — current estimated input tokens for the turn.
    /// * `configured_output_cap` — the output reservation the caller would like
    ///   (e.g. the engine's `TURN_MAX_OUTPUT_TOKENS`). It is clamped down so it
    ///   never consumes the headroom or the minimum input budget; on a window
    ///   too small to hold even the minimum input budget plus headroom, the cap
    ///   collapses to whatever is left (possibly zero).
    ///
    /// Never panics and never underflows: all arithmetic saturates.
    #[must_use]
    pub fn new(window_tokens: u64, input_tokens: u64, configured_output_cap: u64) -> Self {
        Self::with_trigger(window_tokens, input_tokens, configured_output_cap, None)
    }

    /// Like [`ContextBudget::new`], but overrides the compaction trigger with an
    /// explicit token level instead of deriving it from
    /// [`DEFAULT_COMPACTION_TRIGGER_PERCENT`].
    ///
    /// This is what lets the *configured* compaction threshold
    /// (`compaction.token_threshold`, itself derived from the user's
    /// `compact_threshold` percentage of the route window) become the single
    /// decision input, rather than each call site re-deriving its own rule.
    /// `None` keeps the default percent-of-window trigger; `Some(0)` is clamped
    /// up to 1 so a misconfigured zero cannot make every turn "need" compaction.
    #[must_use]
    pub fn with_trigger(
        window_tokens: u64,
        input_tokens: u64,
        configured_output_cap: u64,
        trigger_tokens: Option<u64>,
    ) -> Self {
        let output_cap_tokens = clamp_output_cap(window_tokens, configured_output_cap);

        // Reserve output + safety headroom; whatever remains is spendable input.
        let reserved = output_cap_tokens.saturating_add(CONTEXT_HEADROOM_TOKENS);
        let input_budget_ceiling = window_tokens.saturating_sub(reserved);
        let available_input_tokens = input_budget_ceiling.saturating_sub(input_tokens);

        let compaction_trigger_tokens = match trigger_tokens {
            Some(explicit) => explicit.max(1),
            None => percent_of(window_tokens, DEFAULT_COMPACTION_TRIGGER_PERCENT),
        };

        let pressure =
            PressureLevel::from_usage_percent(usage_percent(window_tokens, input_tokens));

        ContextBudget {
            window_tokens,
            input_tokens,
            output_cap_tokens,
            available_input_tokens,
            compaction_trigger_tokens,
            pressure,
        }
    }

    /// Fraction of the window currently consumed by input, as a percentage in
    /// `0.0..=100.0`. A zero window reports `0.0` rather than dividing by zero.
    #[must_use]
    pub fn usage_percent(&self) -> f64 {
        usage_percent(self.window_tokens, self.input_tokens)
    }

    /// Whether current input has reached the compaction trigger and compaction
    /// should be suggested.
    #[must_use]
    pub fn should_compact(&self) -> bool {
        self.window_tokens > 0 && self.input_tokens >= self.compaction_trigger_tokens
    }

    /// Whether another `additional_input_tokens` of input would fit within the
    /// available budget (i.e. not exceed the reserved boundary).
    #[must_use]
    pub fn fits_additional(&self, additional_input_tokens: u64) -> bool {
        additional_input_tokens <= self.available_input_tokens
    }

    /// Whether the input has exhausted the spendable budget, i.e. the next
    /// request would cross the reserved output + headroom boundary.
    ///
    /// This is the "must compact or the provider will reject us" signal that
    /// the emergency-recovery path cares about, as distinct from
    /// [`ContextBudget::should_compact`], which is the softer "we've crossed
    /// the configured trigger" signal.
    #[must_use]
    pub fn is_over_budget(&self) -> bool {
        self.window_tokens > 0 && self.available_input_tokens == 0
    }
}

/// The reason a compaction decision came back positive.
///
/// Returned by [`compaction_decision`] so callers can distinguish a routine
/// threshold crossing from a hard budget exhaustion (which warrants the
/// emergency path) without re-deriving either condition themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionTrigger {
    /// Input crossed the configured/derived compaction trigger.
    ThresholdReached,
    /// Input exhausted the spendable budget; compaction is mandatory.
    BudgetExhausted,
}

/// Single decision point for "should we compact now?".
///
/// `enabled` short-circuits to `None` so the caller does not need a separate
/// config check that could drift from this rule. Budget exhaustion outranks a
/// plain threshold crossing because it changes how aggressively the caller
/// should compact.
#[must_use]
pub fn compaction_decision(budget: &ContextBudget, enabled: bool) -> Option<CompactionTrigger> {
    if !enabled {
        return None;
    }
    if budget.is_over_budget() {
        return Some(CompactionTrigger::BudgetExhausted);
    }
    if budget.should_compact() {
        return Some(CompactionTrigger::ThresholdReached);
    }
    None
}

/// Clamp a desired output cap so it fits the window while preserving at least
/// [`MIN_INPUT_BUDGET_TOKENS`] of input room plus [`CONTEXT_HEADROOM_TOKENS`].
///
/// On a window too small to hold even that floor, returns whatever room is left
/// after the headroom (possibly zero) rather than underflowing.
fn clamp_output_cap(window_tokens: u64, configured_output_cap: u64) -> u64 {
    // The most output we can reserve and still keep the input floor + headroom.
    let reserved_floor = MIN_INPUT_BUDGET_TOKENS.saturating_add(CONTEXT_HEADROOM_TOKENS);
    let max_output = window_tokens.saturating_sub(reserved_floor);
    configured_output_cap.min(max_output)
}

/// Window usage as a percentage in `0.0..=100.0`. Zero window -> `0.0`.
fn usage_percent(window_tokens: u64, input_tokens: u64) -> f64 {
    if window_tokens == 0 {
        return 0.0;
    }
    ((input_tokens as f64 / window_tokens as f64) * 100.0).clamp(0.0, 100.0)
}

/// `percent`% of `window_tokens`, rounded to the nearest token. Saturates at
/// `u64::MAX` and treats out-of-range percentages by clamping to `0.0..=100.0`.
fn percent_of(window_tokens: u64, percent: f64) -> u64 {
    let percent = percent.clamp(0.0, 100.0);
    let value = (window_tokens as f64) * (percent / 100.0);
    // `as u64` saturates on overflow and floors; add 0.5 to round to nearest.
    (value + 0.5) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_level_classifies_percent_bands() {
        assert_eq!(PressureLevel::from_usage_percent(0.0), PressureLevel::Low);
        assert_eq!(PressureLevel::from_usage_percent(39.9), PressureLevel::Low);
        assert_eq!(
            PressureLevel::from_usage_percent(MEDIUM_PRESSURE_PERCENT),
            PressureLevel::Medium
        );
        assert_eq!(
            PressureLevel::from_usage_percent(HIGH_PRESSURE_PERCENT - 0.1),
            PressureLevel::Medium
        );
        assert_eq!(
            PressureLevel::from_usage_percent(HIGH_PRESSURE_PERCENT),
            PressureLevel::High
        );
        assert_eq!(
            PressureLevel::from_usage_percent(CRITICAL_PRESSURE_PERCENT - 0.1),
            PressureLevel::High
        );
        assert_eq!(
            PressureLevel::from_usage_percent(CRITICAL_PRESSURE_PERCENT),
            PressureLevel::Critical
        );
        assert_eq!(PressureLevel::from_usage_percent(100.0), PressureLevel::Critical);
    }

    #[test]
    fn pressure_level_clamps_out_of_range() {
        assert_eq!(PressureLevel::from_usage_percent(-50.0), PressureLevel::Low);
        assert_eq!(PressureLevel::from_usage_percent(250.0), PressureLevel::Critical);
    }

    #[test]
    fn suggests_compaction_only_at_high_and_critical() {
        assert!(!PressureLevel::Low.suggests_compaction());
        assert!(!PressureLevel::Medium.suggests_compaction());
        assert!(PressureLevel::High.suggests_compaction());
        assert!(PressureLevel::Critical.suggests_compaction());
    }

    #[test]
    fn context_budget_computes_trigger_and_pressure() {
        // 200k window, 150k input -> 75% used -> at the High/compaction trigger.
        let budget = ContextBudget::new(200_000, 150_000, 8_192);
        assert_eq!(budget.window_tokens, 200_000);
        assert_eq!(budget.input_tokens, 150_000);
        assert_eq!(budget.compaction_trigger_tokens, 150_000);
        assert!(budget.should_compact());
        assert_eq!(budget.pressure, PressureLevel::High);
        assert_eq!(budget.usage_percent(), 75.0);
    }

    #[test]
    fn context_budget_below_trigger_is_safe() {
        let budget = ContextBudget::new(200_000, 100_000, 8_192);
        assert_eq!(budget.pressure, PressureLevel::Medium);
        assert!(!budget.should_compact());
        assert!(budget.available_input_tokens > 0);
    }

    #[test]
    fn context_budget_never_underflows_on_small_window() {
        let budget = ContextBudget::new(4_096, 1_000, 262_144);
        assert!(budget.output_cap_tokens <= budget.window_tokens);
        assert!(!budget.should_compact() || budget.input_tokens >= budget.compaction_trigger_tokens);
    }

    #[test]
    fn context_budget_zero_window_is_safe() {
        let budget = ContextBudget::new(0, 0, 8_192);
        assert_eq!(budget.usage_percent(), 0.0);
        assert!(!budget.should_compact());
        assert_eq!(budget.pressure, PressureLevel::Low);
    }

    #[test]
    fn explicit_trigger_overrides_percent_of_window() {
        // Configured threshold (60k) is below the 75%-of-window default (150k),
        // so a 100k input should compact under the override but not the default.
        let default_budget = ContextBudget::new(200_000, 100_000, 8_192);
        assert!(!default_budget.should_compact());

        let configured = ContextBudget::with_trigger(200_000, 100_000, 8_192, Some(60_000));
        assert_eq!(configured.compaction_trigger_tokens, 60_000);
        assert!(configured.should_compact());
    }

    #[test]
    fn zero_trigger_is_clamped_so_it_does_not_always_fire_at_zero_input() {
        let budget = ContextBudget::with_trigger(200_000, 0, 8_192, Some(0));
        assert_eq!(budget.compaction_trigger_tokens, 1);
        assert!(!budget.should_compact());
    }

    #[test]
    fn compaction_decision_respects_enabled_flag() {
        let budget = ContextBudget::new(200_000, 150_000, 8_192);
        assert!(budget.should_compact());
        assert_eq!(
            compaction_decision(&budget, true),
            Some(CompactionTrigger::ThresholdReached)
        );
        assert_eq!(compaction_decision(&budget, false), None);
    }

    #[test]
    fn compaction_decision_reports_budget_exhaustion_first() {
        // Input consumes everything left after the output reservation, so the
        // decision must escalate past a plain threshold crossing.
        let budget = ContextBudget::new(200_000, 199_000, 8_192);
        assert_eq!(budget.available_input_tokens, 0);
        assert_eq!(
            compaction_decision(&budget, true),
            Some(CompactionTrigger::BudgetExhausted)
        );
    }

    #[test]
    fn compaction_decision_is_none_when_there_is_room() {
        let budget = ContextBudget::new(200_000, 10_000, 8_192);
        assert_eq!(compaction_decision(&budget, true), None);
    }

    #[test]
    fn ui_threshold_constants_are_distinct_from_budget_band() {
        // UI warning band is intentionally more conservative than the budget
        // compaction trigger, so the two must not silently collapse together.
        assert!(UI_WARNING_PERCENT > HIGH_PRESSURE_PERCENT);
        assert!(UI_CRITICAL_PERCENT > UI_WARNING_PERCENT);
        assert!(UI_SUGGEST_COMPACT_PERCENT < UI_WARNING_PERCENT);
    }
}
