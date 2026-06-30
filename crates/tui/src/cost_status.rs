//! Process-wide cost-accrual side-channel (#526).
//!
//! Background LLM calls outside the main turn-complete path
//! (compaction summaries, seam recompaction) used
//! to drop their token usage on the floor — the dashboard's
//! session-cost only saw the parent turn's tokens, so a long
//! session that triggered compaction under-reported
//! cost by however many tokens those background calls consumed.
//!
//! Mirrors the [`crate::retry_status`] pattern: background callers
//! call [`report`] after each `client.create_message`, the TUI
//! render loop calls [`drain`] every frame, and any drained amount
//! gets folded into `App::accrue_subagent_cost_estimate`.
//!
//! Why a side-channel and not a plumbed callback: the leaky callers
//! (`compaction.rs`, `seam_manager.rs`) are
//! engine-internal machinery without a direct handle to `App` or
//! the engine's event channel. A side-channel keeps the change
//! surface tiny — one new `report` line per call site — and any
//! future background caller (summarizers, retrieval helpers) gets
//! accrued for free without further plumbing.

use std::sync::{Mutex, OnceLock};

use crate::models::Usage;
use crate::pricing::CostEstimate;

static PENDING: OnceLock<Mutex<CostEstimate>> = OnceLock::new();

fn cell() -> &'static Mutex<CostEstimate> {
    PENDING.get_or_init(|| Mutex::new(CostEstimate::default()))
}

/// Background callers report their LLM usage here. Computes the
/// cost via [`crate::pricing::calculate_turn_cost_estimate_from_usage`] and
/// adds it to the pending pool. Cheap; takes a short-lived lock
/// and returns. No-op on models the pricing table doesn't know.
pub fn report(model: &str, usage: &Usage) {
    let Some(cost) = crate::pricing::calculate_turn_cost_estimate_from_usage(model, usage) else {
        return;
    };
    if !cost.is_positive() {
        return;
    }
    // Recover from poisoned lock — a previous holder panicked but the
    // accumulated data is still valid.
    let mut pending = cell().lock().unwrap_or_else(|e| e.into_inner());
    pending.usd += cost.usd;
    pending.cny += cost.cny;
}

/// Drain the pending cost. Returns the accumulated amount and resets
/// the pool to zero. Called by the TUI render / event loop on each
/// frame; any non-zero result gets folded into `accrue_subagent_cost_estimate`.
pub fn drain() -> CostEstimate {
    // Recover from poisoned lock — a previous holder panicked but the
    // accumulated data is still valid.
    let mut pending = cell().lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *pending)
}

/// Reset the pool to zero without consuming. Test-only helper for
/// suites that share the static and need to start from a known
/// state. Production code should always use [`drain`].

#[cfg(test)]
mod tests {}
