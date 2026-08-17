//! Read-only viewer pane for a single sub-agent's transcript snapshot.
//!
//! The pane is intentionally *read-only*: it is constructed from a
//! one-shot snapshot of the sub-agent's transcript cells (`Vec<HistoryCell>`)
//! taken at construction time. It never holds a `Mailbox` sender or any other
//! write handle, so opening the viewer cannot influence the running agent —
//! the live write channel is left untouched and downstream rendering reads
//! only from the captured snapshot.
//!
//! Rendering reuses the existing per-cell revision cache in
//! [`crate::tui::transcript::TranscriptViewCache`] via its `ensure` /
//! `lines` surface, exactly as the main transcript path does.

use std::cell::RefCell;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
};

use crate::palette;
use crate::tui::app::App;
use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::transcript::TranscriptViewCache;
use crate::tui::views::{ModalKind, ModalView, ViewAction};

/// Footer hint shown at the bottom of the read-only viewer.
const FOOTER_HINT: &str =
    " j/k scroll  Space/C-b page  g/G top/bottom  End=bottom  q/Esc close (read-only) ";

/// Read-only snapshot pane showing one sub-agent's transcript.
///
/// `cells` is captured once at [`SubAgentViewer::new`] from the live app
/// transcript. Because the pane owns its own copy and only renders from it,
/// it cannot mutate the source agent's state or stream. Scrolling is local
/// and never re-reads the source.
pub struct SubAgentViewer {
    agent_id: String,
    agent_label: String,
    /// One-shot snapshot of the agent's transcript cells. Read-only by
    /// construction — never re-fetched, never mutated.
    cells: Vec<HistoryCell>,
    /// Render options sampled at construction time.
    options: TranscriptRenderOptions,
    /// Per-cell revision vector mirrored from the snapshot so the cache can
    /// do its normal reuse pass without touching the source app.
    revisions: Vec<u64>,
    /// Cached wrapped lines for the snapshot.
    cache: RefCell<TranscriptViewCache>,
    scroll: RefCell<usize>,
    last_visible_height: RefCell<usize>,
    last_total_lines: RefCell<usize>,
}

impl SubAgentViewer {
    /// Build a read-only viewer from a snapshot of the given agent's transcript
    /// cells. `cells` must already be the agent's transcript extracted from a
    /// source (e.g. [`subagent_viewer_cells_for_agent`]); the viewer takes
    /// ownership and never re-reads the source.
    #[must_use]
    pub fn new(
        agent_id: impl Into<String>,
        agent_label: impl Into<String>,
        cells: Vec<HistoryCell>,
        options: TranscriptRenderOptions,
    ) -> Self {
        // Revisions are all 0: the snapshot is static, so the cache only ever
        // needs to render once and can then reuse across frames. We do not
        // carry the source app's revision counters on purpose — that would
        // re-link this pane to the live transcript.
        let revisions = vec![0u64; cells.len()];
        Self {
            agent_id: agent_id.into(),
            agent_label: agent_label.into(),
            cells,
            options,
            revisions,
            cache: RefCell::new(TranscriptViewCache::new()),
            scroll: RefCell::new(0),
            last_visible_height: RefCell::new(0),
            last_total_lines: RefCell::new(0),
        }
    }

    /// The agent id this viewer is showing.
    #[must_use]
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Number of transcript cells in the snapshot (test helper).
    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Render the snapshot into the wrapped-line cache at the given width.
    /// Safe to call every frame; with a static snapshot the cache reuses its
    /// prior render after the first `ensure`.
    fn refresh(&self, width: u16) {
        let mut cache = self.cache.borrow_mut();
        cache.ensure(&self.cells, &self.revisions, width, self.options.clone());
    }

    fn clamp_scroll(&self) {
        let total = self.last_total_lines.borrow().max(0);
        let visible = self.last_visible_height.borrow().max(1);
        let max_scroll = total.saturating_sub(visible);
        let mut scroll = self.scroll.borrow_mut();
        if *scroll > max_scroll {
            *scroll = max_scroll;
        }
    }
}

/// Extract a read-only snapshot of one sub-agent's transcript cells from the
/// app's history, by `agent_id`.
///
/// This is the only consumer-side accessor used by the viewer. It borrows
/// `app` immutably, copies the matching `HistoryCell`s once, and returns the
/// clone — it never mutates the app, the sub-agent card, or any mailbox. The
/// copy is what severs the live write channel: the returned vector is owned
/// by the caller and the running agent keeps writing to its own cells.
///
/// Returns `None` if the agent has no in-transcript card yet (the viewer's
/// caller should fall back to the agent's summary from `SubAgentResult`).
#[must_use]
pub fn subagent_viewer_cells_for_agent(app: &App, agent_id: &str) -> Option<Vec<HistoryCell>> {
    let idx = *app.subagent_card_index.get(agent_id)?;
    let cell = app.history.get(idx)?;
    // Delegate and Fanout cards are the two in-transcript representations of a
    // sub-agent. We snapshot the single card cell that carries the agent's
    // rendered transcript; that is exactly what the main transcript path
    // would flatten for this agent.
    match cell {
        HistoryCell::SubAgent(_) => Some(vec![cell.clone()]),
        _ => None,
    }
}

impl ModalView for SubAgentViewer {
    fn kind(&self) -> ModalKind {
        ModalKind::SubAgentViewer
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> ViewAction {
        use crossterm::event::{KeyCode, KeyModifiers};

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => ViewAction::Close,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ViewAction::Close
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let mut scroll = self.scroll.borrow_mut();
                *scroll = scroll.saturating_sub(1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let mut scroll = self.scroll.borrow_mut();
                *scroll = scroll.saturating_add(1);
                ViewAction::None
            }
            KeyCode::Char('g') => {
                *self.scroll.borrow_mut() = 0;
                ViewAction::None
            }
            KeyCode::Char('G') => {
                let total = *self.last_total_lines.borrow();
                let visible = self.last_visible_height.borrow().max(1);
                *self.scroll.borrow_mut() = total.saturating_sub(visible);
                ViewAction::None
            }
            KeyCode::Char(' ') | KeyCode::PageDown | KeyCode::Char('f')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let step = self.last_visible_height.borrow().max(1);
                let mut scroll = self.scroll.borrow_mut();
                *scroll = scroll.saturating_add(step);
                ViewAction::None
            }
            KeyCode::PageUp | KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let step = self.last_visible_height.borrow().max(1);
                let mut scroll = self.scroll.borrow_mut();
                *scroll = scroll.saturating_sub(step);
                ViewAction::None
            }
            KeyCode::End => {
                let total = *self.last_total_lines.borrow();
                let visible = self.last_visible_height.borrow().max(1);
                *self.scroll.borrow_mut() = total.saturating_sub(visible);
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = 100.min(area.width.saturating_sub(4));
        let popup_height = 30.min(area.height.saturating_sub(4));

        let popup_area = Rect {
            x: (area.width.saturating_sub(popup_width)) / 2,
            y: (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        self.refresh(popup_area.width);

        let cache = self.cache.borrow();
        let all_lines: &[Line<'static>] = cache.lines();
        let total_lines = all_lines.len();
        drop(cache);

        let visible_height = (popup_area.height as usize).saturating_sub(2).max(1);
        *self.last_total_lines.borrow_mut() = total_lines;
        *self.last_visible_height.borrow_mut() = visible_height;
        self.clamp_scroll();

        let scroll = *self.scroll.borrow();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let start = scroll.min(max_scroll);
        let end = (start + visible_height).min(total_lines);

        let body_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::MIMOFAN_INK))
            .padding(Padding::horizontal(1))
            .title(Line::from(vec![ratatui::text::Span::styled(
                format!(" SubAgent transcript · {} ", self.agent_label),
                Style::default()
                    .fg(palette::MIMOFAN_ACCENT_PRIMARY)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )]))
            .title_bottom(Line::from(vec![ratatui::text::Span::styled(
                format!(" {FOOTER_HINT} [{}] ", self.agent_id),
                Style::default().fg(palette::TEXT_MUTED),
            )]));

        let inner = body_block.inner(popup_area);
        body_block.render(popup_area, buf);

        if all_lines.is_empty() {
            let empty = vec![Line::from(ratatui::text::Span::styled(
                "No transcript captured for this agent yet (read-only snapshot).",
                Style::default().fg(palette::TEXT_MUTED),
            ))];
            Paragraph::new(empty)
                .style(Style::default().fg(palette::TEXT_PRIMARY))
                .render(inner, buf);
            return;
        }

        let visible: Vec<Line<'static>> = all_lines[start..end].to_vec();
        Paragraph::new(visible)
            .style(Style::default().fg(palette::TEXT_PRIMARY))
            .render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::history::{HistoryCell, SubAgentCell, TranscriptRenderOptions};
    use crate::tui::widgets::agent_card::{AgentLifecycle, DelegateCard};

    fn sample_cells() -> Vec<HistoryCell> {
        // Build a minimal DelegateCard-based cell. The exact card contents do
        // not matter for the read-only assertion; we only need something the
        // TranscriptViewCache can render without panicking.
        let mut card = DelegateCard::new("agent-1", "general");
        card.status = AgentLifecycle::Running;
        card.summary = Some("working on the task".to_string());
        card.push_action("step 1: read source");
        card.push_action("step 2: edit file");
        card.push_action("step 3: run tests");
        vec![HistoryCell::SubAgent(SubAgentCell::Delegate(card))]
    }

    #[test]
    fn viewer_renders_snapshot_without_panic() {
        let cells = sample_cells();
        let viewer = SubAgentViewer::new(
            "agent-1",
            "Agent 1",
            cells,
            TranscriptRenderOptions::default(),
        );

        // Simulate a render into a tiny buffer. The viewer must not panic and
        // must produce at least its chrome (the wrapped lines may be empty for
        // a collapsed card, but the cache round-trip must succeed).
        let mut buf = Buffer::empty(Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        });
        viewer.render(
            Rect {
                x: 0,
                y: 0,
                width: 120,
                height: 40,
            },
            &mut buf,
        );
        // The cache must have cached the snapshot's lines (0 or more).
        let cache = viewer.cache.borrow();
        let _ = cache.lines();
    }

    #[test]
    fn viewer_is_read_only_on_source() {
        // The read-only contract: after handing `cells` to the viewer, the
        // source vector is untouched. We model the source as the owner and
        // assert equality of a captured copy after constructing + rendering.
        let cells = sample_cells();
        let mut source = cells.clone();

        let viewer = SubAgentViewer::new(
            "agent-1",
            "Agent 1",
            cells,
            TranscriptRenderOptions::default(),
        );

        // Render a couple of frames; this exercises the cache path.
        let mut buf = Buffer::empty(Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        });
        for _ in 0..3 {
            viewer.render(
                Rect {
                    x: 0,
                    y: 0,
                    width: 120,
                    height: 40,
                },
                &mut buf,
            );
        }

        // Scroll interactions must not leak into the source.
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let _ = viewer.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let _ = viewer.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));

        // The source transcript is exactly as it was when the snapshot was
        // taken. The viewer only ever reads its own owned `cells`.
        assert_eq!(
            source, viewer.cells,
            "viewer must not mutate the source transcript snapshot"
        );
    }
}
