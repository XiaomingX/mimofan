//! Cached transcript rendering for the TUI.
//!
//! ## Per-cell revision caching
//!
//! Naive caching invalidates the whole transcript whenever ANY cell mutates.
//! During streaming the assistant content cell mutates on every delta — that
//! would force a re-wrap of every cell on every chunk. Codex avoids this by
//! tracking a per-cell revision counter; we mirror that pattern here.
//!
//! Each cell index has a paired `revision: u64`. The cache stores
//! `Vec<CachedCell>` with `(cell_index, revision, lines, line_meta)`. On
//! `ensure`, walk the cells; if a cell's current `revision` matches the cached
//! one (and width/options haven't changed), reuse the rendered lines.
//! Otherwise re-render that cell only and reassemble.
//!
//! Width or render-option changes still bust the entire cache (correct: wrap
//! layout depends on width and which cells are visible at all).

use std::collections::HashSet;
use std::sync::Arc;

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::tui::app::TranscriptSpacing;
use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::scrolling::TranscriptLineMeta;
use crate::tui::ui_text::CopyLineSeparator;

/// Per-cell cached render output. Reused across `ensure` calls when the
/// upstream cell's revision counter hasn't changed.
///
/// Lines are stored behind an `Arc` so that cloning a `CachedCell` during
/// cache-ensure (which touches every cell every frame) is O(1) rather than
/// O(rendered_line_count). Without this, scrolling on a long transcript
/// pays the cost of deep-cloning every cell's `Vec<Line>` per frame, which
/// is the surface-level symptom of issue #78. The flatten step uses
/// `Arc::make_mut` to produce an owned `Vec` for the final `lines`
/// assembly, so the only deep-clone occurs on the flattened output — once
/// per frame instead of once per cell.
#[derive(Debug, Clone)]
struct CachedCell {
    /// Revision the cell was at when the lines/meta were rendered.
    revision: u64,
    /// Rendered lines for this cell (without trailing inter-cell spacers),
    /// shared via `Arc` so cache enumeration is O(N) not O(N*lines).
    lines: Arc<Vec<Line<'static>>>,
    /// Copy separators aligned with `lines`. These preserve source hard
    /// newlines while allowing copy to remove visual soft-wrap breaks.
    copy_separators: Arc<Vec<CopyLineSeparator>>,
    /// Display-column widths of visual prefixes that should be omitted from
    /// clipboard text, aligned with `lines`.
    copy_prefix_widths: Arc<Vec<usize>>,
    /// Whether this cell's rendered output was empty (e.g. Thinking hidden).
    /// Cached so we can skip empty cells without re-rendering.
    is_empty: bool,
    /// Whether this cell is a stream continuation. Determines spacer rules.
    /// Cached because `is_stream_continuation` is cheap but reading via the
    /// cache lets us decide spacers without touching the cell.
    is_stream_continuation: bool,
    /// Whether this cell is conversational (User/Assistant/Thinking). Used
    /// for spacer calculations.
    is_conversational: bool,
    /// Whether this cell is a System or Tool cell (affects spacer rules).
    is_system_or_tool: bool,
    /// Whether this cell participates in the compact tool-card rail group.
    is_tool_groupable: bool,
}

/// Cache of rendered transcript lines for the current viewport.
#[derive(Debug)]
pub struct TranscriptViewCache {
    width: u16,
    options: TranscriptRenderOptions,
    /// Snapshot of folded_thinking indices from the last `ensure` call.
    /// When this changes, all cells must be re-rendered because the fold
    /// state affects the rendered output but not the cell revision.
    folded_cells: HashSet<usize>,
    /// Per-cell rendered output, indexed by current cell position.
    /// Length always equals the cell count seen on the last `ensure` call.
    per_cell: Vec<CachedCell>,
    /// Flattened lines reassembled from `per_cell` plus spacers.
    lines: Vec<Line<'static>>,
    /// Per-line metadata aligned with `lines`.
    line_meta: Vec<TranscriptLineMeta>,
    /// Per-line rail-prefix display-column count (`0` or `2`), aligned with
    /// `lines`. Populated during flatten so that selection-to-text can shift
    /// columns past visual-only decoration glyphs without guessing which
    /// spans are decorative (#1163).
    rail_prefix_widths: Vec<usize>,
}

impl TranscriptViewCache {
    /// Create an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            width: 0,
            options: TranscriptRenderOptions::default(),
            folded_cells: HashSet::new(),
            per_cell: Vec::new(),
            lines: Vec::new(),
            line_meta: Vec::new(),
            rail_prefix_widths: Vec::new(),
        }
    }

    /// Ensure cached lines match the provided cells/widths/per-cell revisions.
    ///
    /// Reuses rendered lines for cells whose `cell_revisions[i]` matches the
    /// previously cached revision (when the cell shape — empty/spacer flags —
    /// also matches). Width or option changes bust the entire cache.
    ///
    /// `cell_revisions.len()` is expected to equal `cells.len()`. If they
    /// disagree (shouldn't happen in normal use) the cache treats every cell
    /// as dirty.
    ///
    /// Retained for tests and external use; the live render path uses the
    /// `ensure_split` variant to avoid concatenating history + active-cell
    /// entries every frame.
    #[allow(dead_code)]
    pub fn ensure(
        &mut self,
        cells: &[HistoryCell],
        cell_revisions: &[u64],
        width: u16,
        options: TranscriptRenderOptions,
    ) {
        self.ensure_split(
            &[cells],
            cell_revisions,
            width,
            options,
            &HashSet::new(),
            None,
        );
    }

    /// Ensure cached lines match the provided cell shards (logically
    /// concatenated) plus per-cell revisions. Avoids the
    /// `concat-into-Vec<HistoryCell>` clone the caller would otherwise pay
    /// every frame on long transcripts.
    ///
    /// `folded_cells` contains original virtual indices of thinking cells
    /// that should render in their folded (summary) form.
    ///
    /// `original_index_map` maps filtered (positional) indices to original
    /// virtual indices. Required when `collapsed_cells` filtering is active
    /// so that `folded_cells` lookups resolve to the correct original index.
    pub fn ensure_split(
        &mut self,
        cell_shards: &[&[HistoryCell]],
        cell_revisions: &[u64],
        width: u16,
        options: TranscriptRenderOptions,
        folded_cells: &HashSet<usize>,
        original_index_map: Option<&[usize]>,
    ) {
        let total_cells: usize = cell_shards.iter().map(|s| s.len()).sum();

        let layout_changed = self.width != width || self.options != options;
        let folded_changed = self.folded_cells != *folded_cells;
        if layout_changed || folded_changed {
            self.per_cell.clear();
        }
        self.width = width;
        self.options = options;
        self.folded_cells = folded_cells.clone();

        // Track whether anything actually changed; if all cells are reused at
        // the same indices, we can skip the reflatten.
        let old_len = self.per_cell.len();
        let mut any_dirty = layout_changed || folded_changed || old_len != total_cells;
        let mut first_dirty: Option<usize> = if old_len != total_cells {
            Some(old_len.min(total_cells))
        } else {
            None
        };

        let mut new_per_cell: Vec<CachedCell> = Vec::with_capacity(total_cells);
        let revisions_match = cell_revisions.len() == total_cells;

        let mut idx: usize = 0;
        for shard in cell_shards {
            for cell in *shard {
                let current_rev = if revisions_match {
                    cell_revisions[idx]
                } else {
                    // No matching revisions — force a re-render this cycle.
                    u64::MAX
                };

                // Reuse cached entry if the revision matches AND it's at the
                // same index (cells can shift on insert/remove, so we only
                // reuse when the index is identical — a stricter invariant
                // codex also uses for its active-cell tail).
                if let Some(prev) = self.per_cell.get(idx)
                    && !layout_changed
                    && prev.revision == current_rev
                    && revisions_match
                {
                    new_per_cell.push(prev.clone());
                    idx += 1;
                    continue;
                }

                any_dirty = true;
                first_dirty = Some(first_dirty.map_or(idx, |current| current.min(idx)));
                let is_tool_groupable = matches!(cell, HistoryCell::Tool(_));
                let render_width = if is_tool_groupable {
                    width.saturating_sub(2).max(1)
                } else {
                    width
                };
                let original_idx = original_index_map
                    .map(|m| *m.get(idx).unwrap_or(&idx))
                    .unwrap_or(idx);
                let folded = folded_cells.contains(&original_idx);
                let rendered = cell.lines_with_copy_metadata_folded(render_width, options, folded);
                let mut lines = Vec::with_capacity(rendered.len());
                let mut copy_separators = Vec::with_capacity(rendered.len());
                let mut copy_prefix_widths = Vec::with_capacity(rendered.len());
                for rendered_line in rendered {
                    lines.push(rendered_line.line);
                    copy_prefix_widths.push(rendered_line.copy_prefix_width);
                    copy_separators.push(rendered_line.copy_separator_after);
                }
                let is_empty = lines.is_empty();
                new_per_cell.push(CachedCell {
                    revision: current_rev,
                    lines: Arc::new(lines),
                    copy_separators: Arc::new(copy_separators),
                    copy_prefix_widths: Arc::new(copy_prefix_widths),
                    is_empty,
                    is_stream_continuation: cell.is_stream_continuation(),
                    is_conversational: cell.is_conversational(),
                    is_system_or_tool: matches!(
                        cell,
                        HistoryCell::System { .. }
                            | HistoryCell::Error { .. }
                            | HistoryCell::Tool(_)
                            | HistoryCell::SubAgent(_)
                            | HistoryCell::ArchivedContext { .. }
                    ),
                    is_tool_groupable,
                });
                idx += 1;
            }
        }

        self.per_cell = new_per_cell;

        if !any_dirty {
            // All cells reused at the same indices: nothing to reflatten.
            // (Width didn't change either, since that bumps `layout_changed`.)
            return;
        }

        let rebuild_from = if layout_changed {
            0
        } else {
            first_dirty.unwrap_or(0).saturating_sub(1)
        };
        self.flatten_from(options.spacing, rebuild_from);
    }

    /// Reassemble flat `lines` / `line_meta` from `per_cell` plus spacers.
    fn flatten(&mut self, spacing: TranscriptSpacing) {
        self.lines.clear();
        self.line_meta.clear();
        self.rail_prefix_widths.clear();
        self.append_flattened_cells(spacing, 0);
    }

    /// Reassemble only the suffix starting at `first_cell`.
    ///
    /// Streaming usually mutates the active tail cell. Rebuilding from the
    /// previous cell preserves spacer correctness while avoiding a full
    /// O(total transcript lines) flatten on every token chunk.
    fn flatten_from(&mut self, spacing: TranscriptSpacing, first_cell: usize) {
        if first_cell == 0 || self.lines.is_empty() || self.line_meta.is_empty() {
            self.flatten(spacing);
            return;
        }

        let truncate_at = self
            .line_meta
            .iter()
            .position(|meta| match meta {
                TranscriptLineMeta::CellLine { cell_index, .. } => *cell_index >= first_cell,
                TranscriptLineMeta::Spacer => false,
            })
            .unwrap_or(self.lines.len());
        self.lines.truncate(truncate_at);
        self.line_meta.truncate(truncate_at);
        self.rail_prefix_widths.truncate(truncate_at);
        self.append_flattened_cells(spacing, first_cell);
    }

    fn append_flattened_cells(&mut self, spacing: TranscriptSpacing, start_cell: usize) {
        for (cell_index, cached) in self.per_cell.iter().enumerate().skip(start_cell) {
            if cached.is_empty {
                continue;
            }
            // Arc::make_mut would deep-clone only on write; since we just
            // rebuilt `lines` from scratch we always need the owned data.
            // Deref is zero-cost and gives us &[Line].
            let rendered_line_count = cached.lines.len();
            for (line_in_cell, line) in cached.lines.iter().enumerate() {
                let final_line = line_with_group_rail(
                    line,
                    tool_group_rail(
                        self.per_cell.as_slice(),
                        cell_index,
                        line_in_cell,
                        rendered_line_count,
                    ),
                    usize::from(self.width),
                );
                self.rail_prefix_widths
                    .push(compute_rail_prefix_width(&final_line));
                self.lines.push(final_line);
                self.line_meta.push(TranscriptLineMeta::CellLine {
                    cell_index,
                    line_in_cell,
                    copy_prefix_width: cached
                        .copy_prefix_widths
                        .get(line_in_cell)
                        .copied()
                        .unwrap_or(0),
                    copy_separator_after: cached
                        .copy_separators
                        .get(line_in_cell)
                        .copied()
                        .unwrap_or(CopyLineSeparator::Newline),
                });
            }

            if let Some(next) = self.per_cell.get(cell_index + 1) {
                let spacer_rows = spacer_rows_between(cached, next, spacing);
                for _ in 0..spacer_rows {
                    self.lines.push(Line::from(""));
                    self.line_meta.push(TranscriptLineMeta::Spacer);
                    self.rail_prefix_widths.push(0);
                }
            }
        }
    }

    /// Return cached lines.
    #[must_use]
    pub fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }

    /// Return cached line metadata.
    #[must_use]
    pub fn line_meta(&self) -> &[TranscriptLineMeta] {
        &self.line_meta
    }

    /// Return total cached lines.
    #[must_use]
    pub fn total_lines(&self) -> usize {
        self.lines.len()
    }

    /// Return the rail-prefix display-column count for the line at
    /// `line_index`. Callers use this to shift selection coordinates past
    /// visual-only decoration glyphs without guessing which spans are
    /// decorative (#1163).
    #[must_use]
    pub fn rail_prefix_width(&self, line_index: usize) -> usize {
        self.rail_prefix_widths
            .get(line_index)
            .copied()
            .unwrap_or(0)
    }
}

fn spacer_rows_between(
    current: &CachedCell,
    next: &CachedCell,
    spacing: TranscriptSpacing,
) -> usize {
    if current.is_stream_continuation {
        return 0;
    }

    if current.is_tool_groupable && next.is_tool_groupable {
        return 0;
    }

    let conversational_gap = match spacing {
        TranscriptSpacing::Compact => 0,
        TranscriptSpacing::Comfortable => 1,
        TranscriptSpacing::Spacious => 2,
    };
    let secondary_gap = match spacing {
        TranscriptSpacing::Compact => 0,
        TranscriptSpacing::Comfortable | TranscriptSpacing::Spacious => 1,
    };

    if current.is_conversational && next.is_conversational {
        conversational_gap
    } else if current.is_system_or_tool || next.is_system_or_tool {
        secondary_gap
    } else {
        0
    }
}

fn tool_group_rail(
    cells: &[CachedCell],
    cell_index: usize,
    line_in_cell: usize,
    rendered_line_count: usize,
) -> Option<crate::tui::widgets::tool_card::CardRail> {
    let cached = cells.get(cell_index)?;
    if !cached.is_tool_groupable || rendered_line_count == 0 {
        return None;
    }

    let previous_is_tool = cell_index
        .checked_sub(1)
        .and_then(|idx| cells.get(idx))
        .is_some_and(|cell| cell.is_tool_groupable && !cell.is_empty);
    let next_is_tool = cells
        .get(cell_index + 1)
        .is_some_and(|cell| cell.is_tool_groupable && !cell.is_empty);
    let first_line_in_group = !previous_is_tool && line_in_cell == 0;
    let last_line_in_group = !next_is_tool && line_in_cell + 1 == rendered_line_count;

    let rail = match (first_line_in_group, last_line_in_group) {
        (true, true) if rendered_line_count == 1 => {
            crate::tui::widgets::tool_card::CardRail::Single
        }
        (true, _) => crate::tui::widgets::tool_card::CardRail::Top,
        (_, true) => crate::tui::widgets::tool_card::CardRail::Bottom,
        _ => crate::tui::widgets::tool_card::CardRail::Middle,
    };
    Some(rail)
}

fn line_with_group_rail(
    line: &Line<'static>,
    rail: Option<crate::tui::widgets::tool_card::CardRail>,
    max_width: usize,
) -> Line<'static> {
    let Some(rail) = rail else {
        return line.clone();
    };
    let glyph = crate::tui::widgets::tool_card::rail_glyph(rail);
    if glyph.is_empty() {
        let mut rendered = line.clone();
        rendered.spans = truncate_spans_to_width(rendered.spans, max_width);
        return rendered;
    }

    let mut rendered = line.clone();
    let mut spans = Vec::with_capacity(rendered.spans.len() + 1);
    spans.push(Span::styled(
        format!("{glyph} "),
        Style::default().fg(crate::palette::TEXT_DIM),
    ));
    spans.extend(rendered.spans);
    rendered.spans = truncate_spans_to_width(spans, max_width);
    rendered
}

/// Return the display-column count of consecutive visual-only decorative
/// spans at the start of a rendered transcript line. Iterates through
/// leading spans matching either of two patterns:
///
/// * Pattern A — span is `"<glyph>[<glyph>…]<space>"` where every character
///   except the trailing space is a rail-drawing character (e.g. `▏ `,
///   `▶ `, `⋮⋮ `). The entire span width is accumulated.
/// * Pattern B — span is `"<glyph>"` (1 drawing char) followed by a lone
///   space span `" "` (e.g. `●` then ` `, `▎` then ` `).
///
/// Stops at the first non-matching span. Every decorated glyph used by the
/// TUI is a single display-column character, so char-count = display width.
///
/// Returns `0` for lines whose first span is not a decorative prefix.
fn compute_rail_prefix_width(line: &Line<'static>) -> usize {
    let spans = line.spans.as_slice();
    let mut total = 0;
    let mut i = 0;

    while i < spans.len() {
        let content = spans[i].content.as_ref();
        let n_chars = content.chars().count();

        // Pattern A — span "<glyph>[<glyph>…]<space>" (≥ 2 chars, trailing
        // space, all preceding chars are drawing chars).
        if n_chars >= 2
            && content.ends_with(' ')
            && content
                .chars()
                .take(n_chars.saturating_sub(1))
                .all(is_rail_drawing_char)
        {
            total += n_chars;
            i += 1;
            continue;
        }

        // Pattern B — span "<glyph>" (1 drawing char) + next span " ".
        if n_chars == 1
            && content.chars().next().is_some_and(is_rail_drawing_char)
            && spans.get(i + 1).is_some_and(|s| s.content.as_ref() == " ")
        {
            total += 2;
            i += 2;
            continue;
        }

        break;
    }

    total
}

/// Characters that serve as decoration glyphs in the TUI left-rail and
/// tool-header prefix system. All are single display-column characters.
fn is_rail_drawing_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{2500}'..='\u{257F}'   // Box Drawing (╭ ╮ ╰ ╯ │ ╎ …)
        | '\u{2580}'..='\u{259F}' // Block Elements (▏ ▎ ▍ ▌ …)
        | '\u{25A0}'..='\u{25FF}' // Geometric Shapes (● ▶ ▷ ◆ ◐ …)
        | '\u{2022}'              // • bullet (tool status / generic tool)
        | '\u{2026}'              // … ellipsis (reasoning opener)
        | '\u{00B7}'              // · middle dot (tool running symbol)
        | '\u{2315}'              // ⌕ telephone recorder (find/search tool)
        | '\u{22EE}'              // ⋮ vertical ellipsis (fanout/rlm tool)
    )
}

fn truncate_spans_to_width(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Span<'static>> {
    if max_width == 0 || spans.is_empty() {
        return Vec::new();
    }
    let current_width: usize = spans
        .iter()
        .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    if current_width <= max_width {
        return spans;
    }

    let ellipsis = if max_width > 3 { "..." } else { "" };
    let content_budget = max_width.saturating_sub(ellipsis.len());
    let mut used = 0usize;
    let mut truncated = Vec::with_capacity(spans.len() + usize::from(!ellipsis.is_empty()));
    let mut last_style = Style::default();

    'outer: for span in spans {
        last_style = span.style;
        let mut content = String::new();
        for ch in span.content.chars() {
            let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + width > content_budget {
                break 'outer;
            }
            content.push(ch);
            used += width;
        }
        if !content.is_empty() {
            truncated.push(Span::styled(content, span.style));
        }
    }

    if !ellipsis.is_empty() {
        truncated.push(Span::styled(ellipsis.to_string(), last_style));
    }
    truncated
}

#[cfg(test)]
mod tests {}
