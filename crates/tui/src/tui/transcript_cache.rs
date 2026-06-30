//! Wrapped-line cache for the live transcript overlay (#94).
//!
//! Each cell's rendered output is cached under a `(CellId, width, revision)`
//! key. The revision portion comes from `App.history_revisions` (or the
//! synthetic active-cell revision); the cache invalidates entries the moment
//! a cell mutates because the upstream tag changes. Width changes invalidate
//! everything for that cell because wrap layout depends on width.
//!
//! Live cells (the streaming assistant body, in-flight tool entries) bump
//! their revision on every mutation, so the cache always reflects the latest
//! frame of their output without ever paying for a re-wrap of unrelated
//! cells. Resize-driven re-wrap is bounded to the cells whose width key just
//! changed; nothing else is invalidated.
//!
//! The cache is bounded to keep memory predictable on long sessions.
//! Eviction is a simple insertion-order scheme — a strict LRU would be
//! overkill for the access pattern (full sweep on every render frame).

use std::collections::HashMap;
use std::collections::VecDeque;

use ratatui::text::Line;

/// Soft cap on the number of cached entries before insertion-order eviction
/// kicks in. Sized for the worst-case "5,000-line transcript at 200 cells,
/// resize twice" pattern; well under a megabyte even with 10 KB cells.
const DEFAULT_CAPACITY: usize = 512;

/// Identifier for a transcript cell within a live render. `History(idx)`
/// addresses a finalized history cell at the given index;
/// `Active(entry_idx)` addresses the synthetic active-cell entry while a
/// turn is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellId {
    History(usize),
    Active(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    cell: CellId,
    width: u16,
    revision: u64,
}

/// Bounded cache of wrapped lines. Keyed by `(cell_id, width, revision)` —
/// any change to a cell's revision (mutation), the terminal width (resize),
/// or the cell's identity (insert/delete shifting indices) misses the cache.
#[derive(Debug)]
pub struct TranscriptCache {
    capacity: usize,
    entries: HashMap<Key, Vec<Line<'static>>>,
    /// Insertion order so we can evict the oldest entry when full. Two-step
    /// (HashMap + VecDeque) so insertion is O(1) and lookup stays O(1).
    insertion_order: VecDeque<Key>,
}

impl Default for TranscriptCache {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl TranscriptCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::with_capacity(capacity.max(1)),
            insertion_order: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    /// Look up wrapped lines previously rendered at this exact key. Returns
    /// `None` if the cell never wrapped at this width/revision before.
    #[must_use]
    pub fn get(&self, cell: CellId, width: u16, revision: u64) -> Option<&[Line<'static>]> {
        let key = Key {
            cell,
            width,
            revision,
        };
        self.entries.get(&key).map(Vec::as_slice)
    }

    /// Cache a fresh wrap result. If the cache is at capacity the oldest
    /// inserted entry is evicted first.
    pub fn insert(&mut self, cell: CellId, width: u16, revision: u64, lines: Vec<Line<'static>>) {
        let key = Key {
            cell,
            width,
            revision,
        };
        // Replace an existing key in place — keep its position in the
        // insertion-order queue so we don't trigger spurious eviction.
        if self.entries.insert(key, lines).is_some() {
            return;
        }
        if self.entries.len() > self.capacity
            && let Some(oldest) = self.insertion_order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        self.insertion_order.push_back(key);
    }

    /// Drop every cached entry. Used when the underlying transcript shape
    /// changes drastically (e.g. session reset).
    #[allow(dead_code)] // Reserved for /clear and session-reset call sites.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.insertion_order.clear();
    }
}

#[cfg(test)]
mod tests {}
