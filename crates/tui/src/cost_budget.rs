//! Cost upper-bound budget guard (#620).
//!
//! mimofan accrues session + sub-agent cost through
//! [`crate::tui::app::App::accrue_session_cost_estimate`] and
//! [`crate::tui::app::App::accrue_subagent_cost_estimate`]. Previously there
//! was no way to be alerted when that accrual crossed a user-configured
//! ceiling, so long tasks could silently blow past a budget.
//!
//! This module evaluates the running cost against two independently
//! configurable ceilings — a per-session high-water limit and a per-calendar-day
//! limit — and emits a [`CostBudgetAlert`] when a threshold is crossed:
//!
//! * a *soft warning* once the cost reaches `warn_percent` of a limit, and
//! * a *hard* alert once the limit itself is reached.
//!
//! Only alerts are produced, never blocking — blocking on exceed has a high
//! blast radius (aborting an in-flight turn) and is intentionally out of scope.
//!
//! The evaluation logic is deliberately pure ([`CostBudget::evaluate`]) so it
//! can be unit-tested without a running TUI. The caller owns any dedupe state
//! by only surfacing an alert the first time a given [`CostBudgetLevel`] is
//! reached for a given limit kind.

use mimofan_config::CostBudgetToml;
use std::fmt::Write as _;

/// Which ceiling an alert refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostBudgetKind {
    /// The per-session high-water cost ceiling.
    Session,
    /// The per-calendar-day accrued cost ceiling.
    Daily,
}

/// Severity of a budget alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CostBudgetLevel {
    /// Soft warning — approaching the limit.
    Warn,
    /// Hard alert — limit reached or exceeded.
    Hard,
}

/// A single budget alert produced when a threshold is crossed.
#[derive(Debug, Clone, PartialEq)]
pub struct CostBudgetAlert {
    /// Which ceiling was crossed.
    pub kind: CostBudgetKind,
    /// Severity of the crossing.
    pub level: CostBudgetLevel,
    /// The limit that was crossed, in USD.
    pub limit_usd: f64,
    /// The cost at the time of crossing, in USD.
    pub current_usd: f64,
}

impl CostBudgetAlert {
    /// Human-readable alert text for the footer / status toast.
    pub fn message(&self) -> String {
        let kind_label = match self.kind {
            CostBudgetKind::Session => "session",
            CostBudgetKind::Daily => "daily",
        };
        let pct = if self.limit_usd > 0.0 {
            (self.current_usd / self.limit_usd * 100.0).min(999.0)
        } else {
            100.0
        };
        let mut msg = String::new();
        match self.level {
            CostBudgetLevel::Warn => {
                let _ = write!(
                    msg,
                    "[cost-budget] {kind_label} cost ${:.2} has reached {:.0}% of the ${:.2} limit",
                    self.current_usd, pct, self.limit_usd
                );
            }
            CostBudgetLevel::Hard => {
                let _ = write!(
                    msg,
                    "[cost-budget] {kind_label} cost ${:.2} exceeded the ${:.2} limit",
                    self.current_usd, self.limit_usd
                );
            }
        }
        msg
    }
}

/// Default fraction of a limit at which the soft warning fires.
const DEFAULT_WARN_PERCENT: f64 = 0.8;

/// Resolved cost budget configuration plus evaluation state.
///
/// Construct via [`CostBudget::from_toml`]; an inactive (or absent) config
/// yields a `disabled` budget whose [`evaluate`](Self::evaluate) always
/// returns `None`.
#[derive(Debug, Clone)]
pub struct CostBudget {
    enabled: bool,
    session_limit_usd: f64,
    daily_limit_usd: f64,
    warn_percent: f64,
}

impl Default for CostBudget {
    fn default() -> Self {
        Self {
            enabled: false,
            session_limit_usd: 0.0,
            daily_limit_usd: 0.0,
            warn_percent: DEFAULT_WARN_PERCENT,
        }
    }
}

impl CostBudget {
    /// Build from the on-disk `[cost_budget]` table. A disabled or
    /// limit-less config produces a no-op budget.
    pub fn from_toml(toml: &CostBudgetToml) -> Self {
        if !toml.has_limit() {
            return Self::default();
        }
        let warn_percent = if toml.warn_percent > 0.0 {
            toml.warn_percent.clamp(0.01, 1.0)
        } else {
            DEFAULT_WARN_PERCENT
        };
        Self {
            enabled: true,
            session_limit_usd: toml.session_limit_usd.max(0.0),
            daily_limit_usd: toml.daily_limit_usd.max(0.0),
            warn_percent,
        }
    }

    /// Whether this budget is active and has at least one limit.
    pub fn is_active(&self) -> bool {
        self.enabled && (self.session_limit_usd > 0.0 || self.daily_limit_usd > 0.0)
    }

    /// Threshold (in USD) at which the soft warning for `kind` fires.
    fn warn_threshold(&self, kind: CostBudgetKind) -> f64 {
        let limit = match kind {
            CostBudgetKind::Session => self.session_limit_usd,
            CostBudgetKind::Daily => self.daily_limit_usd,
        };
        limit * self.warn_percent
    }

    /// Evaluate `current_usd` against the given ceiling.
    ///
    /// Returns the highest-severity alert whose threshold has been crossed,
    /// or `None` if the cost is still below the soft-warning threshold. A
    /// config with no limit for `kind` never alerts.
    pub fn evaluate(&self, kind: CostBudgetKind, current_usd: f64) -> Option<CostBudgetAlert> {
        if !self.enabled {
            return None;
        }
        let limit = match kind {
            CostBudgetKind::Session => self.session_limit_usd,
            CostBudgetKind::Daily => self.daily_limit_usd,
        };
        if limit <= 0.0 {
            return None;
        }
        let current_usd = current_usd.max(0.0);
        if current_usd >= limit {
            return Some(CostBudgetAlert {
                kind,
                level: CostBudgetLevel::Hard,
                limit_usd: limit,
                current_usd,
            });
        }
        if current_usd >= self.warn_threshold(kind) {
            return Some(CostBudgetAlert {
                kind,
                level: CostBudgetLevel::Warn,
                limit_usd: limit,
                current_usd,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimofan_config::CostBudgetToml;

    fn budget(session: f64, daily: f64, warn: f64) -> CostBudget {
        CostBudget::from_toml(&CostBudgetToml {
            enabled: true,
            session_limit_usd: session,
            daily_limit_usd: daily,
            warn_percent: warn,
        })
    }

    #[test]
    fn disabled_config_is_noop() {
        let b = CostBudget::default();
        assert!(!b.is_active());
        assert_eq!(b.evaluate(CostBudgetKind::Session, 1_000.0), None);
    }

    #[test]
    fn below_warn_threshold_is_silent() {
        let b = budget(100.0, 0.0, 0.8);
        assert_eq!(b.evaluate(CostBudgetKind::Session, 50.0), None);
    }

    #[test]
    fn warn_at_threshold() {
        let b = budget(100.0, 0.0, 0.8);
        let alert = b
            .evaluate(CostBudgetKind::Session, 80.0)
            .expect("should warn");
        assert_eq!(alert.level, CostBudgetLevel::Warn);
        // Still below the hard limit, so a slightly-higher value stays a warn.
        let alert2 = b
            .evaluate(CostBudgetKind::Session, 99.0)
            .expect("should warn");
        assert_eq!(alert2.level, CostBudgetLevel::Warn);
    }

    #[test]
    fn hard_at_and_over_limit() {
        let b = budget(100.0, 0.0, 0.8);
        let alert = b
            .evaluate(CostBudgetKind::Session, 100.0)
            .expect("should hard-alert");
        assert_eq!(alert.level, CostBudgetLevel::Hard);
        let over = b
            .evaluate(CostBudgetKind::Session, 250.0)
            .expect("should hard-alert");
        assert_eq!(over.level, CostBudgetLevel::Hard);
    }

    #[test]
    fn daily_limit_independent() {
        let b = budget(0.0, 50.0, 0.5);
        assert!(b.is_active());
        assert_eq!(b.evaluate(CostBudgetKind::Session, 1_000.0), None);
        let alert = b
            .evaluate(CostBudgetKind::Daily, 30.0)
            .expect("daily warn at 50%");
        assert_eq!(alert.kind, CostBudgetKind::Daily);
        assert_eq!(alert.level, CostBudgetLevel::Warn);
    }

    #[test]
    fn message_renders_both_levels() {
        let b = budget(100.0, 0.0, 0.8);
        let warn = b.evaluate(CostBudgetKind::Session, 85.0).unwrap();
        assert!(warn.message().contains("85%"));
        let hard = b.evaluate(CostBudgetKind::Session, 100.0).unwrap();
        assert!(hard.message().contains("exceeded"));
    }
}
