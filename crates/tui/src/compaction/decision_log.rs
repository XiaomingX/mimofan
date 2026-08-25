//! Decision-event stream with an **independent** compaction path.
//!
//! # Motivation
//!
//! The main conversation compaction in [`super::mod`] rewrites the free-form
//! dialogue (user/assistant text, tool calls, results) into a condensed
//! summary. That is the right treatment for narrative history, but it is the
//! wrong place for *structured* decision events — the model's key choices,
//! tool selections, branch taken at a fork, goal revisions, etc.
//!
//! If decision events are folded into the dialogue summary they get:
//!
//! 1. **Diluted / lost** — a terse `ToolChosen(edit_file)` line can vanish
//!    inside a prose recap of the surrounding chatter.
//! 2. **Polluted** — the main summary accumulates musing and intermediate
//!    reasoning that obscure the actual decision trajectory.
//! 3. **Unretrievable** — long-horizon tasks need a clean decision trail to
//!    answer "why did we pick path X?" long after the dialogue was compacted.
//!
//! [`DecisionLog`] keeps these events in a separate stream and compacts them
//! on its **own** trigger (an event-count threshold, not token budget). A
//! compacted decision log is a structured, high-signal summary (the surviving
//! key decisions plus aggregate statistics) that can be injected into the
//! system prompt without touching the main conversation summary or the
//! objective anchor maintained by [`super::objective`].
//!
//! # Compression threshold strategy
//!
//! Compaction is driven purely by accumulated event count (`compact_threshold`,
//! default 100). Independent event-count triggering was chosen over token
//! budget for two reasons: decision events are individually tiny, so a token
//! threshold would rarely fire and we'd lose the bounded-memory guarantee; and
//! the decision log's value is the *recency + diversity* of choices, which is
//! best bounded by a fixed horizon of the most recent N decisions rather than
//! by bytes. When `drain_compact` runs it keeps the `keep_recent` highest-value
//! events verbatim and replaces the rest with aggregate counts per [`Kind`],
//! guaranteeing the post-compaction length is a function of the kind taxonomy
//! (bounded, small) plus `keep_recent`, never the raw event count.

use std::collections::HashMap;
use std::fmt::Write as _;

/// The turn/step index these events were produced at.
pub type TurnId = u64;

/// Classification of a structured decision event.
///
/// Kept as a closed enum so the compacted summary can aggregate by kind and so
/// future instrumentation (e.g. a UI timeline) can switch on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A specific tool was selected to satisfy a need.
    ToolChosen,
    /// One branch of an explicit fork/choice was taken.
    BranchTaken,
    /// The task objective or plan was revised.
    GoalRevised,
    /// A hypothesis was accepted or rejected.
    Hypothesis,
    /// Any other structured decision worth keeping.
    Other,
}

impl Kind {
    /// Stable, human-readable label used in summaries.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::ToolChosen => "tool_chosen",
            Kind::BranchTaken => "branch_taken",
            Kind::GoalRevised => "goal_revised",
            Kind::Hypothesis => "hypothesis",
            Kind::Other => "other",
        }
    }
}

/// A single structured decision emitted by the model (or the orchestration
/// layer on its behalf).
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionEvent {
    /// Monotonic turn/step counter (1-based) at which the event occurred.
    pub turn: TurnId,
    /// What kind of decision this was.
    pub kind: Kind,
    /// Short human-readable summary of the decision, e.g.
    /// "chose edit_file over write_file to patch src/main.rs".
    pub summary: String,
    /// Optional model confidence in `[0.0, 1.0]`. `None` when the event was
    /// emitted by a deterministic layer (e.g. a tool router) rather than the
    /// model's own judgment.
    pub confidence: Option<f32>,
}

impl DecisionEvent {
    /// Convenience constructor with no confidence value.
    pub fn new(turn: TurnId, kind: Kind, summary: impl Into<String>) -> Self {
        Self {
            turn,
            kind,
            summary: summary.into(),
            confidence: None,
        }
    }

    /// Convenience constructor carrying a confidence score.
    pub fn with_confidence(
        turn: TurnId,
        kind: Kind,
        summary: impl Into<String>,
        confidence: f32,
    ) -> Self {
        Self {
            turn,
            kind,
            summary: summary.into(),
            confidence: Some(confidence.clamp(0.0, 1.0)),
        }
    }
}

/// Configuration for [`DecisionLog`] behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionLogConfig {
    /// Number of accumulated events that triggers an independent compaction
    /// pass on the next `drain_compact` call. See module docs for why this is
    /// an event count rather than a token budget.
    pub compact_threshold: usize,
    /// How many of the most recent (highest-turn) events survive a compaction
    /// verbatim; the rest collapse into per-kind aggregate counts.
    pub keep_recent: usize,
}

impl Default for DecisionLogConfig {
    fn default() -> Self {
        Self {
            compact_threshold: 100,
            keep_recent: 12,
        }
    }
}

/// A collecting buffer of structured decision events with an independent
/// compaction path.
///
/// Events are appended with [`DecisionLog::record`]. When the accumulated count
/// crosses [`DecisionLogConfig::compact_threshold`], the next
/// [`DecisionLog::drain_compact`] compacts the buffered events into a compact
/// summary (the most recent `keep_recent` verbatim plus aggregate per-kind
/// counts) and resets the buffer; [`DecisionLog::drain_compact`] otherwise
/// returns `None` so callers can avoid rewriting the system prompt when nothing
/// changed.
#[derive(Debug, Clone, Default)]
pub struct DecisionLog {
    config: DecisionLogConfig,
    events: Vec<DecisionEvent>,
    /// Running totals across *all* events ever recorded, including ones already
    /// compacted away. Lets `summary` report lifetime statistics even after
    /// the raw buffer was drained.
    lifetime_counts: HashMap<Kind, usize>,
    /// Total events ever recorded (for the lifetime stat).
    lifetime_total: usize,
}

impl DecisionLog {
    /// Create a log with the default threshold policy.
    pub fn new() -> Self {
        Self::with_config(DecisionLogConfig::default())
    }

    /// Create a log with an explicit threshold policy (used by tests).
    pub fn with_config(config: DecisionLogConfig) -> Self {
        Self {
            config,
            events: Vec::new(),
            lifetime_counts: HashMap::new(),
            lifetime_total: 0,
        }
    }

    /// Number of events currently buffered (not yet compacted).
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// `true` when no events are buffered and nothing has been compacted.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.lifetime_total == 0
    }

    /// Total number of events ever recorded, across all compaction cycles.
    pub fn lifetime_event_count(&self) -> usize {
        self.lifetime_total
    }

    /// Append a decision event to the buffer.
    ///
    /// Updates both the per-drain buffer and the lifetime statistics. Does not
    /// itself perform compaction — callers decide when to drain (typically once
    /// per turn loop, keeping the decision stream independent of the main
    /// dialogue compaction cadence).
    pub fn record(&mut self, event: DecisionEvent) {
        *self.lifetime_counts.entry(event.kind).or_insert(0) += 1;
        self.lifetime_total = self.lifetime_total.saturating_add(1);
        self.events.push(event);
    }

    /// Whether the buffered event count has crossed the compaction threshold.
    ///
    /// Used as a cheap gate so callers can skip `drain_compact` work entirely
    /// until it is worthwhile.
    pub fn should_compact(&self) -> bool {
        self.events.len() >= self.config.compact_threshold
    }

    /// Drain and (conditionally) compact the buffered decision events.
    ///
    /// Returns `Some(summary)` when the buffer crossed
    /// [`DecisionLogConfig::compact_threshold`] (or when `force` is set), in
    /// which case the buffer is **cleared** and the returned summary is the
    /// compacted view. Returns `None` when there is nothing to compact yet,
    /// leaving the buffer intact so events are not lost.
    ///
    /// The compacted summary keeps the `keep_recent` highest-turn events
    /// verbatim and collapses the older events into per-kind aggregate counts,
    /// so its length is bounded by `keep_recent + number_of_kinds` regardless
    /// of how many events were drained.
    pub fn drain_compact(&mut self) -> Option<String> {
        if !self.should_compact() {
            return None;
        }
        let summary = self.build_compact_summary();
        self.events.clear();
        Some(summary)
    }

    /// Force a compact/drain even if the threshold was not reached (e.g. at
    /// session end so no decision is silently dropped). Same contract as
    /// [`DecisionLog::drain_compact`] but bypasses the threshold gate.
    pub fn drain_compact_forced(&mut self) -> Option<String> {
        if self.events.is_empty() {
            return None;
        }
        let summary = self.build_compact_summary();
        self.events.clear();
        Some(summary)
    }

    /// Build the compact summary from the current buffer without clearing it.
    /// Pure / private: shared by both drain paths.
    fn build_compact_summary(&self) -> String {
        let keep = self.config.keep_recent.min(self.events.len());
        let split_at = self.events.len() - keep;
        let (old, recent) = self.events.split_at(split_at);

        // Aggregate the older, dropped events by kind.
        let mut collapsed: HashMap<Kind, usize> = HashMap::new();
        for ev in old {
            *collapsed.entry(ev.kind).or_insert(0) += 1;
        }

        let mut out = String::new();
        let _ = writeln!(out, "## 🧭 Decision Log (compacted)");
        let _ = writeln!(
            out,
            "Recent {} decision event(s) retained; {} earlier event(s) collapsed by kind.",
            recent.len(),
            old.len()
        );

        if !collapsed.is_empty() {
            let _ = writeln!(out, "\nAggregated (older) decisions:");
            // Stable order by kind label for deterministic summaries.
            let mut kinds: Vec<Kind> = collapsed.keys().copied().collect();
            kinds.sort_by_key(|k| k.as_str());
            for k in kinds {
                let n = collapsed[&k];
                let _ = writeln!(out, "- {}: {}", k.as_str(), n);
            }
        }

        if !recent.is_empty() {
            let _ = writeln!(out, "\nRetained key decisions (most recent first):");
            // Show most-recent first: iterate in reverse.
            for ev in recent.iter().rev() {
                match ev.confidence {
                    Some(c) => {
                        let _ = writeln!(
                            out,
                            "- [turn {}] {} (conf={:.2}): {}",
                            ev.turn,
                            ev.kind.as_str(),
                            c,
                            ev.summary
                        );
                    }
                    None => {
                        let _ = writeln!(
                            out,
                            "- [turn {}] {}: {}",
                            ev.turn,
                            ev.kind.as_str(),
                            ev.summary
                        );
                    }
                }
            }
        }

        out
    }

    /// A compact, always-available summary for injection into the system
    /// prompt. Unlike [`DecisionLog::drain_compact`], this never mutates the
    /// buffer or triggers compaction — it renders the current buffered events
    /// (capped to `keep_recent` most recent) plus lifetime statistics. Use this
    /// during the main compaction step to surface the decision trail without
    /// disturbing the independent drain cadence.
    ///
    /// Returns `None` when the log has never recorded anything.
    pub fn summary(&self) -> Option<String> {
        if self.lifetime_total == 0 && self.events.is_empty() {
            return None;
        }

        let keep = self.config.keep_recent.min(self.events.len());
        let recent: Vec<&DecisionEvent> = self.events.iter().rev().take(keep).collect();

        let mut out = String::new();
        let _ = writeln!(out, "## 🧭 Decision Trail");
        let _ = writeln!(
            out,
            "Lifetime decisions: {} (tool_chosen={}, branch_taken={}, goal_revised={}, hypothesis={}, other={})",
            self.lifetime_total,
            self.lifetime_counts
                .get(&Kind::ToolChosen)
                .copied()
                .unwrap_or(0),
            self.lifetime_counts
                .get(&Kind::BranchTaken)
                .copied()
                .unwrap_or(0),
            self.lifetime_counts
                .get(&Kind::GoalRevised)
                .copied()
                .unwrap_or(0),
            self.lifetime_counts
                .get(&Kind::Hypothesis)
                .copied()
                .unwrap_or(0),
            self.lifetime_counts.get(&Kind::Other).copied().unwrap_or(0),
        );

        if !recent.is_empty() {
            let _ = writeln!(out, "\nMost recent decisions:");
            for ev in recent.iter().rev() {
                let _ = writeln!(
                    out,
                    "- [turn {}] {}: {}",
                    ev.turn,
                    ev.kind.as_str(),
                    ev.summary
                );
            }
        }

        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(threshold: usize, keep: usize) -> DecisionLogConfig {
        DecisionLogConfig {
            compact_threshold: threshold,
            keep_recent: keep,
        }
    }

    #[test]
    fn empty_log_reports_empty() {
        let mut log = DecisionLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert_eq!(log.lifetime_event_count(), 0);
        assert!(log.summary().is_none());
        assert!(log.drain_compact().is_none());
    }

    #[test]
    fn record_accumulates_and_tracks_lifetime() {
        let mut log = DecisionLog::with_config(cfg(100, 12));
        assert!(!log.should_compact());
        for i in 1..=10u64 {
            log.record(DecisionEvent::new(
                i,
                if i % 2 == 0 {
                    Kind::ToolChosen
                } else {
                    Kind::BranchTaken
                },
                format!("decision {i}"),
            ));
        }
        assert_eq!(log.len(), 10);
        assert_eq!(log.lifetime_event_count(), 10);
        // Below threshold: drain is a no-op and preserves the buffer.
        assert!(log.drain_compact().is_none());
        assert_eq!(log.len(), 10, "buffer must survive a no-op drain");
    }

    #[test]
    fn crossing_threshold_triggers_compaction() {
        let mut log = DecisionLog::with_config(cfg(5, 12));
        for i in 1..=5u64 {
            log.record(DecisionEvent::new(i, Kind::ToolChosen, format!("tool {i}")));
        }
        assert!(log.should_compact());
        let summary = log.drain_compact().expect("threshold crossed → summary");
        assert!(summary.contains("Decision Log (compacted)"));
        assert_eq!(log.len(), 0, "buffer cleared after drain");
        assert_eq!(log.lifetime_event_count(), 5, "lifetime stat preserved");
    }

    #[test]
    fn compacted_summary_keeps_key_events_and_shrinks() {
        // Threshold 20, keep_recent 3: 20 events → summary keeps 3 verbatim,
        // collapses 17 into aggregates. Summary length must be far below the
        // combined length of all 20 raw summaries.
        let keep = 3usize;
        let total = 20usize;
        let mut log = DecisionLog::with_config(cfg(total, keep));
        let mut raw_len = 0usize;
        for i in 1..=total as u64 {
            let s = format!(
                "decision event number {i} with a fairly long descriptive summary text that would cost many tokens if kept verbatim for every single one of these events"
            );
            raw_len += s.chars().count();
            let kind = match i % 3 {
                0 => Kind::ToolChosen,
                1 => Kind::BranchTaken,
                _ => Kind::GoalRevised,
            };
            log.record(DecisionEvent::with_confidence(i, kind, s, 0.9));
        }

        let summary = log.drain_compact().expect("must compact at threshold");
        assert!(summary.contains("Retained key decisions"));
        assert!(summary.contains("Aggregated (older) decisions"));
        // The three retained events are the most recent (turns 18,19,20).
        assert!(summary.contains("turn 20"));
        assert!(summary.contains("turn 18"));
        // Event #1's full text is unique and must be collapsed, not retained
        // verbatim. (Use the full phrasing, not a bare "turn 1" substring, which
        // would also match "turn 18"/"turn 19".)
        assert!(
            !summary.contains("decision event number 1 with a fairly long"),
            "oldest event must be collapsed, not verbatim"
        );

        let summary_len = summary.chars().count();
        assert!(
            summary_len < raw_len,
            "compacted summary ({} chars) must be shorter than raw events ({} chars)",
            summary_len,
            raw_len
        );
    }

    #[test]
    fn drain_clears_buffer_and_summary_still_works() {
        let mut log = DecisionLog::with_config(cfg(3, 2));
        for i in 1..=3u64 {
            log.record(DecisionEvent::new(i, Kind::Other, format!("e{i}")));
        }
        assert!(log.drain_compact().is_some());
        assert_eq!(log.len(), 0);
        // Even after draining, `summary` can render lifetime stats.
        let s = log.summary().expect("lifetime stats available post-drain");
        assert!(s.contains("Lifetime decisions: 3"));
    }

    #[test]
    fn forced_drain_works_below_threshold() {
        let mut log = DecisionLog::with_config(cfg(100, 1));
        log.record(DecisionEvent::new(1, Kind::GoalRevised, "revised goal"));
        // Normal drain would no-op (below threshold).
        assert!(log.drain_compact().is_none());
        // Forced drain compacts and clears regardless.
        let s = log.drain_compact_forced().expect("forced drain");
        assert!(s.contains("revised goal"));
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn confidence_is_rendered_in_compact_summary() {
        let mut log = DecisionLog::with_config(cfg(2, 2));
        log.record(DecisionEvent::with_confidence(
            1,
            Kind::Hypothesis,
            "accepted hypothesis A",
            0.87,
        ));
        log.record(DecisionEvent::new(
            2,
            Kind::Hypothesis,
            "rejected hypothesis B",
        ));
        let s = log.drain_compact().expect("compact");
        assert!(s.contains("conf=0.87"), "confidence must render");
        assert!(s.contains("rejected hypothesis B"));
    }
}
