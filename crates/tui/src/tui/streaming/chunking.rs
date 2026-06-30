//! Adaptive stream chunking policy for two-gear streaming.
//!
//! Ported from `codex-rs/tui/src/streaming/chunking.rs`, adapted for mimofan's
//! text-based streaming pipeline. The policy is queue-pressure driven and
//! source-agnostic.
//!
//! # Mental model
//!
//! Two gears:
//! - [`ChunkingMode::Smooth`]: normal pressure.
//! - [`ChunkingMode::CatchUp`]: elevated pressure.
//!
//! Normal-motion callers drain all currently available chunks so the display
//! follows the upstream SSE delta cadence. Low-motion callers stay in Smooth
//! and drain one chunk per tick to reduce visual churn.
//!
//! # Hysteresis
//!
//! - Enter `CatchUp` when `queued_lines >= ENTER_QUEUE_DEPTH_LINES` OR
//!   the oldest queued chunk is at least [`ENTER_OLDEST_AGE`].
//! - Exit `CatchUp` only after pressure stays below [`EXIT_QUEUE_DEPTH_LINES`]
//!   AND [`EXIT_OLDEST_AGE`] for at least [`EXIT_HOLD`].
//! - After exit, suppress immediate re-entry for [`REENTER_CATCH_UP_HOLD`]
//!   unless backlog is "severe" (queue >= [`SEVERE_QUEUE_DEPTH_LINES`] or
//!   oldest >= [`SEVERE_OLDEST_AGE`]).

use std::time::Duration;
use std::time::Instant;

/// Queue-depth threshold that allows entering catch-up mode.
pub(crate) const ENTER_QUEUE_DEPTH_LINES: usize = 160;

/// Oldest-chunk age threshold that allows entering catch-up mode.
pub(crate) const ENTER_OLDEST_AGE: Duration = Duration::from_millis(1_200);

/// Queue-depth threshold used when evaluating catch-up exit hysteresis.
pub(crate) const EXIT_QUEUE_DEPTH_LINES: usize = 32;

/// Oldest-chunk age threshold used when evaluating catch-up exit hysteresis.
pub(crate) const EXIT_OLDEST_AGE: Duration = Duration::from_millis(300);

/// Minimum duration queue pressure must stay below exit thresholds to leave catch-up mode.
pub(crate) const EXIT_HOLD: Duration = Duration::from_millis(250);

/// Cooldown window after a catch-up exit that suppresses immediate re-entry.
pub(crate) const REENTER_CATCH_UP_HOLD: Duration = Duration::from_millis(250);

/// Queue-depth cutoff that marks backlog as severe (bypasses re-entry hold).
pub(crate) const SEVERE_QUEUE_DEPTH_LINES: usize = 640;

/// Oldest-line age cutoff that marks backlog as severe.
pub(crate) const SEVERE_OLDEST_AGE: Duration = Duration::from_millis(4_000);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChunkingMode {
    /// Drain one display chunk per baseline commit tick.
    #[default]
    Smooth,
    /// Drain the queued backlog according to queue pressure.
    CatchUp,
}

/// Captures queue pressure inputs used by adaptive chunking decisions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueSnapshot {
    /// Number of queued stream chunks waiting to be displayed.
    pub queued_lines: usize,
    /// Age of the oldest queued chunk at decision time.
    pub oldest_age: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrainPlan {
    /// Emit all queued chunks available at this tick.
    Available,
    /// Emit exactly one queued line.
    Single,
}

/// Represents one policy decision for a specific queue snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkingDecision {
    /// Mode after applying hysteresis transitions for this decision.
    pub mode: ChunkingMode,
    /// Whether this decision transitioned from `Smooth` into `CatchUp`.
    pub entered_catch_up: bool,
    /// Drain plan to execute for the current commit tick.
    pub drain_plan: DrainPlan,
}

/// Maintains adaptive chunking mode and hysteresis state across ticks.
#[derive(Debug, Default, Clone)]
pub struct AdaptiveChunkingPolicy {
    mode: ChunkingMode,
    below_exit_threshold_since: Option<Instant>,
    last_catch_up_exit_at: Option<Instant>,
    /// When true, the policy never enters `CatchUp` — it stays in `Smooth`
    /// regardless of queue pressure, keeping the display calm for users who
    /// prefer reduced visual churn.
    low_motion: bool,
}

impl AdaptiveChunkingPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the policy mode used by the most recent decision.
    pub fn mode(&self) -> ChunkingMode {
        self.mode
    }

    /// Resets state to baseline smooth mode.
    pub fn reset(&mut self) {
        self.mode = ChunkingMode::Smooth;
        self.below_exit_threshold_since = None;
        self.last_catch_up_exit_at = None;
    }

    /// When true, the policy never enters `CatchUp` — it stays in `Smooth`
    /// regardless of queue pressure.
    pub fn set_low_motion(&mut self, low_motion: bool) {
        self.low_motion = low_motion;
        if low_motion {
            self.mode = ChunkingMode::Smooth;
            self.below_exit_threshold_since = None;
            self.last_catch_up_exit_at = None;
        }
    }

    /// Computes a drain decision from the current queue snapshot.
    pub fn decide(&mut self, snapshot: QueueSnapshot, now: Instant) -> ChunkingDecision {
        // In low-motion mode, always use Smooth pacing regardless of queue
        // pressure — the user asked for a calm, steady display.
        if self.low_motion {
            self.mode = ChunkingMode::Smooth;
            self.below_exit_threshold_since = None;
            return ChunkingDecision {
                mode: self.mode,
                entered_catch_up: false,
                drain_plan: DrainPlan::Single,
            };
        }

        if snapshot.queued_lines == 0 {
            self.note_catch_up_exit(now);
            self.mode = ChunkingMode::Smooth;
            self.below_exit_threshold_since = None;
            return ChunkingDecision {
                mode: self.mode,
                entered_catch_up: false,
                drain_plan: DrainPlan::Available,
            };
        }

        let entered_catch_up = match self.mode {
            ChunkingMode::Smooth => self.maybe_enter_catch_up(snapshot, now),
            ChunkingMode::CatchUp => {
                self.maybe_exit_catch_up(snapshot, now);
                false
            }
        };

        ChunkingDecision {
            mode: self.mode,
            entered_catch_up,
            drain_plan: DrainPlan::Available,
        }
    }

    fn maybe_enter_catch_up(&mut self, snapshot: QueueSnapshot, now: Instant) -> bool {
        if !should_enter_catch_up(snapshot) {
            return false;
        }
        if self.reentry_hold_active(now) && !is_severe_backlog(snapshot) {
            return false;
        }
        self.mode = ChunkingMode::CatchUp;
        self.below_exit_threshold_since = None;
        self.last_catch_up_exit_at = None;
        true
    }

    fn maybe_exit_catch_up(&mut self, snapshot: QueueSnapshot, now: Instant) {
        if !should_exit_catch_up(snapshot) {
            self.below_exit_threshold_since = None;
            return;
        }

        match self.below_exit_threshold_since {
            Some(since) if now.saturating_duration_since(since) >= EXIT_HOLD => {
                self.mode = ChunkingMode::Smooth;
                self.below_exit_threshold_since = None;
                self.last_catch_up_exit_at = Some(now);
            }
            Some(_) => {}
            None => {
                self.below_exit_threshold_since = Some(now);
            }
        }
    }

    fn note_catch_up_exit(&mut self, now: Instant) {
        if self.mode == ChunkingMode::CatchUp {
            self.last_catch_up_exit_at = Some(now);
        }
    }

    fn reentry_hold_active(&self, now: Instant) -> bool {
        self.last_catch_up_exit_at
            .is_some_and(|exit| now.saturating_duration_since(exit) < REENTER_CATCH_UP_HOLD)
    }
}

/// Returns whether current queue pressure warrants entering catch-up mode.
fn should_enter_catch_up(snapshot: QueueSnapshot) -> bool {
    snapshot.queued_lines >= ENTER_QUEUE_DEPTH_LINES
        || snapshot
            .oldest_age
            .is_some_and(|oldest| oldest >= ENTER_OLDEST_AGE)
}

/// Returns whether queue pressure is low enough to begin exit hysteresis.
fn should_exit_catch_up(snapshot: QueueSnapshot) -> bool {
    snapshot.queued_lines <= EXIT_QUEUE_DEPTH_LINES
        && snapshot
            .oldest_age
            .is_some_and(|oldest| oldest <= EXIT_OLDEST_AGE)
}

/// Returns whether backlog is severe enough to bypass the re-entry hold.
fn is_severe_backlog(snapshot: QueueSnapshot) -> bool {
    snapshot.queued_lines >= SEVERE_QUEUE_DEPTH_LINES
        || snapshot
            .oldest_age
            .is_some_and(|oldest| oldest >= SEVERE_OLDEST_AGE)
}

#[cfg(test)]
mod tests {}
