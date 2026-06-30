#![allow(dead_code)]

//! Resource-usage telemetry for long-running mimofan tasks.
//!
//! This module is a pure, side-effect-free foundation for surfacing how many
//! tokens and how much wall-clock time a task has consumed, optionally relative
//! to a budget. It performs no I/O and no rendering; consumers (status lines,
//! the cost panel, the goal/budget tooling) are wired up separately so the
//! formatting and pressure logic can be unit-tested in isolation.
//!
//! The shape intentionally mirrors the budget vocabulary already used by the
//! goal tooling (`token_budget: Option<_>`) so a consumer can adapt between the
//! two without inventing new concepts. We keep a local type rather than reusing
//! `tools::goal` here to avoid coupling a presentation-layer helper to the tool
//! domain model (whose budgets are `u32` and carry unrelated bookkeeping).

use std::{
    fmt::{self, Write as _},
    time::Duration,
};

/// A coarse, three-level read on how close a task is to exhausting its budget.
///
/// The level is derived from the *highest* pressure across all bounded
/// dimensions (tokens and time), so a task that is comfortable on tokens but
/// nearly out of time still reports [`PressureLevel::High`]. When nothing is
/// bounded, pressure is [`PressureLevel::Low`] by definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PressureLevel {
    /// Plenty of headroom (under ~75% of every bounded budget).
    Low,
    /// Getting close (at/over ~75% but under 100% of some budget).
    Medium,
    /// At or over budget on some bounded dimension.
    High,
}

impl PressureLevel {
    /// Fraction at/above which a dimension is considered medium pressure.
    const MEDIUM_THRESHOLD: f64 = 0.75;
    /// Fraction at/above which a dimension is considered high pressure.
    const HIGH_THRESHOLD: f64 = 1.0;

    /// Classify a single budget fraction (e.g. `0.41` for 41% used).
    ///
    /// Negative or non-finite input is treated as [`PressureLevel::Low`]; the
    /// telemetry helpers never produce such values, but classifying defensively
    /// keeps this usable for arbitrary callers.
    fn from_fraction(fraction: f64) -> Self {
        if !fraction.is_finite() || fraction < Self::MEDIUM_THRESHOLD {
            PressureLevel::Low
        } else if fraction < Self::HIGH_THRESHOLD {
            PressureLevel::Medium
        } else {
            PressureLevel::High
        }
    }

    /// A short lowercase label suitable for compact status output.
    pub fn label(self) -> &'static str {
        match self {
            PressureLevel::Low => "low",
            PressureLevel::Medium => "medium",
            PressureLevel::High => "high",
        }
    }
}

/// A snapshot of token and time usage for a single task, with optional budgets.
///
/// All fields are plain counters; this type owns no clock and reads no
/// environment. Construct it from whatever the caller is already tracking and
/// use the helpers below to render or classify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceTelemetry {
    /// Total tokens consumed so far.
    pub tokens_used: u64,
    /// Total wall-clock seconds elapsed so far.
    pub time_used_seconds: u64,
    /// Optional token ceiling for the task; `None` means unbounded.
    pub token_budget: Option<u64>,
    /// Optional time ceiling in seconds; `None` means unbounded.
    pub time_budget_seconds: Option<u64>,
}

impl ResourceTelemetry {
    /// Create a telemetry snapshot with no budgets (fully unbounded).
    pub fn new(tokens_used: u64, time_used_seconds: u64) -> Self {
        Self {
            tokens_used,
            time_used_seconds,
            token_budget: None,
            time_budget_seconds: None,
        }
    }

    /// Set the token budget, returning the updated snapshot (builder style).
    pub fn with_token_budget(mut self, budget: u64) -> Self {
        self.token_budget = Some(budget);
        self
    }

    /// Set the time budget in seconds, returning the updated snapshot.
    pub fn with_time_budget_seconds(mut self, seconds: u64) -> Self {
        self.time_budget_seconds = Some(seconds);
        self
    }

    /// Fraction of the token budget consumed, or `None` when unbounded.
    ///
    /// A zero budget yields `None` (a percentage of nothing is meaningless)
    /// rather than infinity, keeping every downstream consumer safe.
    pub fn token_fraction(&self) -> Option<f64> {
        fraction(self.tokens_used, self.token_budget)
    }

    /// Fraction of the time budget consumed, or `None` when unbounded.
    pub fn time_fraction(&self) -> Option<f64> {
        fraction(self.time_used_seconds, self.time_budget_seconds)
    }

    /// The largest bounded budget fraction across tokens and time.
    ///
    /// Returns `None` only when *neither* dimension is bounded. When at least
    /// one budget is present, the most-pressured bounded dimension wins.
    pub fn budget_fraction(&self) -> Option<f64> {
        match (self.token_fraction(), self.time_fraction()) {
            (Some(t), Some(s)) => Some(t.max(s)),
            (Some(t), None) => Some(t),
            (None, Some(s)) => Some(s),
            (None, None) => None,
        }
    }

    /// Budget fraction expressed as a whole-number percent (rounded), or `None`
    /// when unbounded. This is the value surfaced in the human summary.
    pub fn budget_percent(&self) -> Option<u64> {
        self.budget_fraction().map(|f| (f * 100.0).round() as u64)
    }

    /// Coarse pressure level derived from [`Self::budget_fraction`].
    ///
    /// Unbounded tasks are always [`PressureLevel::Low`].
    pub fn pressure(&self) -> PressureLevel {
        match self.budget_fraction() {
            Some(fraction) => PressureLevel::from_fraction(fraction),
            None => PressureLevel::Low,
        }
    }

    /// A compact, human-readable one-liner, e.g. `12.3k tok · 4m12s · 41% budget`.
    ///
    /// Tokens are abbreviated with `k`/`M` suffixes, time is rendered as
    /// `Hh Mm Ss` (dropping leading zero units), and the budget segment is
    /// omitted entirely when the task is unbounded.
    pub fn human_summary(&self) -> String {
        let mut out = String::new();
        // `write!` into a String is infallible; ignore the Result.
        let _ = write!(
            out,
            "{} tok · {}",
            format_tokens(self.tokens_used),
            format_duration(self.time_used_seconds),
        );
        if let Some(percent) = self.budget_percent() {
            let _ = write!(out, " · {percent}% budget");
        }
        out
    }
}

impl fmt::Display for ResourceTelemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.human_summary())
    }
}

/// Output-token throughput for a live or completed turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenThroughput {
    pub output_tokens: u64,
    pub elapsed_seconds: f64,
}

impl TokenThroughput {
    pub fn new(output_tokens: u64, elapsed: Duration) -> Option<Self> {
        let elapsed_seconds = elapsed.as_secs_f64();
        if output_tokens == 0 || !elapsed_seconds.is_finite() || elapsed_seconds <= 0.0 {
            return None;
        }
        Some(Self {
            output_tokens,
            elapsed_seconds,
        })
    }

    pub fn from_estimated_text(text: &str, elapsed: Duration) -> Option<Self> {
        Self::new(estimate_output_tokens_from_text(text), elapsed)
    }

    pub fn tokens_per_second(self) -> f64 {
        self.output_tokens as f64 / self.elapsed_seconds
    }

    pub fn compact_rate(self) -> String {
        let rate = self.tokens_per_second();
        if rate < 10.0 {
            format!("{rate:.1}")
        } else {
            format!("{rate:.0}")
        }
    }
}

/// Estimate output tokens from streamed text before provider usage arrives.
///
/// Provider-reported usage remains canonical at turn completion. During a live
/// stream, this gives the footer a stable approximation without inspecting
/// provider-specific tokenizer internals.
pub fn estimate_output_tokens_from_text(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    if chars == 0 {
        0
    } else {
        chars.saturating_add(3) / 4
    }
}

/// Divide `used` by an optional budget, guarding against an absent or zero
/// budget. Returns `None` when the budget is `None` or `0`.
fn fraction(used: u64, budget: Option<u64>) -> Option<f64> {
    match budget {
        Some(budget) if budget > 0 => Some(used as f64 / budget as f64),
        _ => None,
    }
}

/// Format a token count with a `k`/`M` suffix once it crosses each threshold.
///
/// Values under 1_000 are printed verbatim. Thousands use one decimal place
/// (`12.3k`), trimming a trailing `.0` so round values read cleanly (`5k`).
/// Millions follow the same rule (`1.5M`, `2M`).
fn format_tokens(tokens: u64) -> String {
    const K: u64 = 1_000;
    const M: u64 = 1_000_000;
    if tokens >= M {
        format_scaled(tokens, M, 'M')
    } else if tokens >= K {
        format_scaled(tokens, K, 'k')
    } else {
        tokens.to_string()
    }
}

/// Render `value / divisor` to one decimal place with `suffix`, dropping a
/// trailing `.0`. The divisor is always one of the constants above (non-zero).
fn format_scaled(value: u64, divisor: u64, suffix: char) -> String {
    let scaled = value as f64 / divisor as f64;
    // Round to one decimal before deciding whether the fraction is ".0", so a
    // value like 1_999_999 reads as "2M" rather than "1.9...M".
    let rounded = (scaled * 10.0).round() / 10.0;
    if (rounded.fract()).abs() < f64::EPSILON {
        format!("{}{}", rounded as u64, suffix)
    } else {
        format!("{rounded:.1}{suffix}")
    }
}

/// Format a duration in seconds as a compact `Hh Mm Ss` string.
///
/// Leading zero units are dropped, so 252s renders as `4m12s` and 90s as
/// `1m30s`. Sub-minute durations render as bare seconds (`0s`, `45s`). Minutes
/// and seconds are zero-padded only when a larger unit precedes them, matching
/// conventional clock-style readouts (`1h05m`, `2h00m03s`).
fn format_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;

    let mut out = String::new();
    if hours > 0 {
        let _ = write!(out, "{hours}h");
    }
    if hours > 0 || minutes > 0 {
        if hours > 0 {
            let _ = write!(out, "{minutes:02}m");
        } else {
            let _ = write!(out, "{minutes}m");
        }
    }
    // Always include seconds unless we have hours+minutes and seconds is zero
    // would still be informative; we keep seconds for precision, padding when a
    // minute or hour precedes it.
    if hours > 0 || minutes > 0 {
        let _ = write!(out, "{seconds:02}s");
    } else {
        let _ = write!(out, "{seconds}s");
    }
    out
}

#[cfg(test)]
mod tests {}
