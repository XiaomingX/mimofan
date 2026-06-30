//! Scroll state tracking for transcript rendering.
//!
//! The transcript view uses a flat line-index scroll model: a single `offset`
//! into the rendered line-meta buffer points at the top visible line, with
//! `usize::MAX` reserved as a sentinel meaning "stuck to the live tail."
//!
//! Why a flat offset, not cell anchors? An earlier design anchored the
//! viewport to a `(cell_index, line_in_cell)` pair on the assumption that
//! the cell list was append-only. It is not — content rewrites (RLM `repl`
//! blocks expanding into `Thinking + Text`, tool result replacements, and
//! compaction) can renumber or remove cells underneath the user. When the
//! anchor cell vanished the viewport teleported to the bottom (issue #56)
//! or "got stuck" because the next keypress would resolve from `max_start`.
//!
//! Codex's pager uses the same line-offset shape; see
//! `codex-rs/tui/src/pager_overlay.rs::PagerView`.

use std::time::{Duration, Instant};

use crate::tui::ui_text::CopyLineSeparator;

const TRACKPAD_EVENT_WINDOW: Duration = Duration::from_millis(35);
const WHEEL_LINES_PER_TICK: i32 = 3;
const TRACKPAD_BASE_LINES_PER_TICK: i32 = 1;
const TRACKPAD_MID_LINES_PER_TICK: i32 = 2;
const TRACKPAD_MAX_LINES_PER_TICK: i32 = 3;

// === Transcript Line Metadata ===

/// Metadata describing how rendered transcript lines map to history cells.
///
/// The scroll state itself does not consult this — it only stores a flat
/// line offset — but other render-time helpers (selection painting,
/// send-flash, jump-to-tool, scrollbar percent) still need the
/// line→cell mapping the cache exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptLineMeta {
    CellLine {
        cell_index: usize,
        line_in_cell: usize,
        copy_prefix_width: usize,
        copy_separator_after: CopyLineSeparator,
    },
    Spacer,
}

impl TranscriptLineMeta {
    /// Return cell/line indices if this entry is a cell line.
    #[must_use]
    pub fn cell_line(&self) -> Option<(usize, usize)> {
        match *self {
            TranscriptLineMeta::CellLine {
                cell_index,
                line_in_cell,
                ..
            } => Some((cell_index, line_in_cell)),
            TranscriptLineMeta::Spacer => None,
        }
    }

    #[must_use]
    pub fn copy_separator_after(&self) -> CopyLineSeparator {
        match *self {
            TranscriptLineMeta::CellLine {
                copy_separator_after,
                ..
            } => copy_separator_after,
            TranscriptLineMeta::Spacer => CopyLineSeparator::Newline,
        }
    }

    #[must_use]
    pub fn copy_prefix_width(&self) -> usize {
        match *self {
            TranscriptLineMeta::CellLine {
                copy_prefix_width, ..
            } => copy_prefix_width,
            TranscriptLineMeta::Spacer => 0,
        }
    }
}

// === Transcript Scroll State ===

/// Sentinel offset meaning "stuck to live tail" — the renderer translates
/// this to `max_start` at draw time, so newly appended lines pull the view
/// down with them.
const TAIL_SENTINEL: usize = usize::MAX;

/// Flat line-offset scroll state for the transcript view.
///
/// Stores the index of the top visible line into the cache's `line_meta`
/// buffer, or [`TAIL_SENTINEL`] (`usize::MAX`) to mean "stuck to bottom."
/// The renderer resolves the sentinel against the current line count and
/// viewport height every frame, so content rewrites simply clamp the
/// user's offset rather than triggering anchor recovery heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptScroll {
    offset: usize,
}

impl Default for TranscriptScroll {
    /// Default state is "stuck to live tail" — matches the historical
    /// `TranscriptScroll::ToBottom` behaviour callers already depend on.
    fn default() -> Self {
        Self::to_bottom()
    }
}

impl TranscriptScroll {
    /// State that follows the live tail (default).
    #[must_use]
    pub const fn to_bottom() -> Self {
        Self {
            offset: TAIL_SENTINEL,
        }
    }

    /// State pinned to a specific line index.
    #[must_use]
    pub const fn at_line(offset: usize) -> Self {
        Self { offset }
    }

    /// Returns true when the view is following the live tail.
    #[must_use]
    pub const fn is_at_tail(self) -> bool {
        self.offset == TAIL_SENTINEL
    }

    /// Resolve the scroll state to a concrete top line index.
    ///
    /// `max_start` is `total_lines.saturating_sub(visible_lines)`. The
    /// returned `Self` is the canonicalized state — if the resolved top
    /// reached the tail (or the transcript fits in one screen) we collapse
    /// to [`TranscriptScroll::to_bottom`], so the caller can treat the
    /// returned state as authoritative.
    ///
    /// `line_meta` is accepted for API compatibility with the previous
    /// cell-anchored implementation. It is unused here because the flat
    /// offset model needs no cell-index lookup; we just clamp.
    #[must_use]
    pub fn resolve_top(self, line_meta: &[TranscriptLineMeta], max_start: usize) -> (Self, usize) {
        let _ = line_meta;
        if self.offset == TAIL_SENTINEL {
            return (Self::to_bottom(), max_start);
        }
        let top = self.offset.min(max_start);
        if top >= max_start {
            (Self::to_bottom(), max_start)
        } else {
            (Self::at_line(top), top)
        }
    }

    /// Apply a scroll delta and return the updated state.
    ///
    /// `delta_lines` is signed: negative scrolls up (toward the start),
    /// positive scrolls down (toward the tail). When the resolved offset
    /// hits `max_start` we snap to [`TranscriptScroll::to_bottom`] so
    /// subsequent appended content pulls the view along.
    ///
    /// `line_meta` is accepted for API compatibility; only its length is
    /// consulted. `visible_lines` controls the page size for clamping.
    #[must_use]
    pub fn scrolled_by(
        self,
        delta_lines: i32,
        line_meta: &[TranscriptLineMeta],
        visible_lines: usize,
    ) -> Self {
        if delta_lines == 0 {
            return self;
        }

        let total_lines = line_meta.len();
        if total_lines <= visible_lines {
            // Whole transcript fits; only "tail" is meaningful.
            return Self::to_bottom();
        }

        let max_start = total_lines.saturating_sub(visible_lines);
        let current_top = if self.offset == TAIL_SENTINEL {
            max_start
        } else {
            self.offset.min(max_start)
        };

        let new_top = if delta_lines < 0 {
            current_top.saturating_sub(delta_lines.unsigned_abs() as usize)
        } else {
            let delta = usize::try_from(delta_lines).unwrap_or(usize::MAX);
            current_top.saturating_add(delta).min(max_start)
        };

        if new_top >= max_start {
            Self::to_bottom()
        } else {
            Self::at_line(new_top)
        }
    }

    /// Pin the scroll state to a specific line index in the rendered
    /// transcript (saturating to the meta buffer length).
    ///
    /// Returns `None` if `line_meta` is empty (caller should default to
    /// [`TranscriptScroll::to_bottom`] in that case).
    #[must_use]
    pub fn anchor_for(line_meta: &[TranscriptLineMeta], start: usize) -> Option<Self> {
        if line_meta.is_empty() {
            return None;
        }
        let clamped = start.min(line_meta.len().saturating_sub(1));
        Some(Self::at_line(clamped))
    }
}

/// Direction for mouse scroll input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

impl ScrollDirection {
    fn sign(self) -> i32 {
        match self {
            ScrollDirection::Up => -1,
            ScrollDirection::Down => 1,
        }
    }
}

/// Stateful tracker for mouse scroll accumulation.
#[derive(Debug, Default)]
pub struct MouseScrollState {
    last_event_at: Option<Instant>,
    last_direction: Option<ScrollDirection>,
    rapid_same_direction_ticks: u8,
}

/// A computed scroll delta from user input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollUpdate {
    pub delta_lines: i32,
}

impl MouseScrollState {
    /// Create a new scroll state tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a scroll event and return the resulting delta.
    pub fn on_scroll(&mut self, direction: ScrollDirection) -> ScrollUpdate {
        let now = Instant::now();
        self.on_scroll_at(direction, now)
    }

    fn on_scroll_at(&mut self, direction: ScrollDirection, now: Instant) -> ScrollUpdate {
        let is_trackpad = self
            .last_event_at
            .is_some_and(|last| now.saturating_duration_since(last) < TRACKPAD_EVENT_WINDOW);
        let same_direction = self.last_direction == Some(direction);

        self.last_event_at = Some(now);
        self.last_direction = Some(direction);

        let lines_per_tick = if is_trackpad {
            if same_direction {
                self.rapid_same_direction_ticks = self.rapid_same_direction_ticks.saturating_add(1);
            } else {
                self.rapid_same_direction_ticks = 1;
            }
            match self.rapid_same_direction_ticks {
                0..=2 => TRACKPAD_BASE_LINES_PER_TICK,
                3..=5 => TRACKPAD_MID_LINES_PER_TICK,
                _ => TRACKPAD_MAX_LINES_PER_TICK,
            }
        } else {
            self.rapid_same_direction_ticks = 0;
            WHEEL_LINES_PER_TICK
        };

        ScrollUpdate {
            delta_lines: direction.sign() * lines_per_tick,
        }
    }
}

#[cfg(test)]
mod tests {}
