//! Fuzzy file-picker modal (Ctrl+P).
//!
//! Opens an overlay populated with workspace-relative paths discovered by a
//! single-pass `WalkBuilder` walk (depth from `mention_walk_depth`, default
//! 10, `0` = unlimited; hidden=true, follow_links=false,
//! `.gitignore` honored). Subsequent keystrokes filter the cached candidate
//! list in memory using a small subsequence + first-letter-bonus scorer — no
//! per-keystroke disk traversal.
//!
//! Enter emits a [`ViewEvent::FilePickerSelected`] which the UI handler turns
//! into an `@<path>` insertion at the composer cursor.

use std::collections::HashSet;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ignore::WalkBuilder;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
};

use crate::palette;
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};
use crate::workspace_discovery::{DISCOVERY_ALWAYS_DIRS, path_is_excluded_from_discovery};

/// Maximum number of candidates collected from the initial walk. Keeps memory
/// bounded for very large monorepos; matches the limits codex-rs uses for the
/// equivalent overlay.
const MAX_CANDIDATES: usize = 20_000;

/// Default walk depth used by the picker's own tests. Production callers pass
/// the configured `mention_walk_depth` (default 10, `0` = unlimited) through
/// [`FilePickerView::new_with_relevance_and_depth`], mirroring the `Workspace`
/// fuzzy index default (`DEFAULT_COMPLETIONS_WALK_DEPTH`).

/// Visible candidate rows in the overlay.
const VISIBLE_ROWS: usize = 14;

const MODIFIED_BOOST: i32 = 360;
const MENTIONED_BOOST: i32 = 240;
const TOOL_BOOST: i32 = 160;

/// Working-set hints captured when the picker opens.
///
/// The picker keeps this as plain path strings so filtering stays in-memory and
/// per-keystroke work remains the same shape as the original fuzzy search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilePickerRelevance {
    modified: HashSet<String>,
    mentioned: HashSet<String>,
    tool: HashSet<String>,
}

impl FilePickerRelevance {
    pub fn mark_modified(&mut self, path: impl Into<String>) {
        let path = path.into();
        if !path.is_empty() {
            self.modified.insert(path);
        }
    }

    pub fn mark_mentioned(&mut self, path: impl Into<String>) {
        let path = path.into();
        if !path.is_empty() {
            self.mentioned.insert(path);
        }
    }

    pub fn mark_tool(&mut self, path: impl Into<String>) {
        let path = path.into();
        if !path.is_empty() {
            self.tool.insert(path);
        }
    }

    fn boost_for(&self, path: &str) -> i32 {
        let mut boost = 0;
        if self.modified.contains(path) {
            boost += MODIFIED_BOOST;
        }
        if self.mentioned.contains(path) {
            boost += MENTIONED_BOOST;
        }
        if self.tool.contains(path) {
            boost += TOOL_BOOST;
        }
        boost
    }

    fn markers_for(&self, path: &str) -> String {
        let mut markers = String::with_capacity(3);
        markers.push(if self.modified.contains(path) {
            'M'
        } else {
            ' '
        });
        markers.push(if self.mentioned.contains(path) {
            '@'
        } else {
            ' '
        });
        markers.push(if self.tool.contains(path) { 'T' } else { ' ' });
        markers
    }
}

pub struct FilePickerView {
    /// All workspace-relative candidate paths, captured once at construction.
    candidates: Vec<String>,
    /// Working-set relevance hints, captured once at construction.
    relevance: FilePickerRelevance,
    /// Filtered indices into `candidates`, sorted by descending score.
    filtered: Vec<usize>,
    /// User's typed query (lowercased on each refilter).
    query: String,
    /// Selected row within `filtered`.
    selected: usize,
    /// Top of the visible window within `filtered`.
    scroll: usize,
}

impl FilePickerView {
    /// Build a picker with working-set relevance hints, using the default
    /// walk depth ([`WALK_DEPTH`]). Test-only convenience; production code uses
    /// [`FilePickerView::new_with_relevance_and_depth`] with the configured
    /// `mention_walk_depth`.

    /// Build a picker with working-set relevance hints and an explicit walk
    /// depth. A depth of `0` disables the depth limit so files in deeply
    /// nested workspaces (>= 6 levels) remain discoverable (#2488).
    pub fn new_with_relevance_and_depth(
        workspace_root: &Path,
        relevance: FilePickerRelevance,
        walk_depth: usize,
    ) -> Self {
        let max_depth = if walk_depth == 0 {
            None
        } else {
            Some(walk_depth)
        };
        let candidates = collect_candidates(workspace_root, max_depth);
        let mut view = Self {
            candidates,
            relevance,
            filtered: Vec::new(),
            query: String::new(),
            selected: 0,
            scroll: 0,
        };
        view.refilter();
        view
    }

    fn refilter(&mut self) {
        let query = self.query.trim().to_lowercase();
        let mut scored: Vec<(usize, i32, i32, i32)> = if query.is_empty() {
            self.candidates
                .iter()
                .enumerate()
                .map(|(idx, path)| {
                    let boost = self.relevance.boost_for(path);
                    (idx, boost, 0, boost)
                })
                .collect()
        } else {
            self.candidates
                .iter()
                .enumerate()
                .filter_map(|(idx, path)| {
                    score(&query, path).map(|fuzzy| {
                        let boost = self.relevance.boost_for(path);
                        (idx, fuzzy + boost, fuzzy, boost)
                    })
                })
                .collect()
        };

        // Higher scores first; tie-break by ascending path length, then lex order
        // so shorter / more central matches surface above deep nested ones.
        scored.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| b.3.cmp(&a.3))
                .then_with(|| self.candidates[a.0].len().cmp(&self.candidates[b.0].len()))
                .then_with(|| self.candidates[a.0].cmp(&self.candidates[b.0]))
        });

        self.filtered = scored.into_iter().map(|(idx, _, _, _)| idx).collect();
        if self.filtered.is_empty() {
            self.selected = 0;
            self.scroll = 0;
        } else if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len() - 1;
        }
        self.adjust_scroll();
    }

    fn adjust_scroll(&mut self) {
        if self.filtered.is_empty() {
            self.scroll = 0;
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + VISIBLE_ROWS {
            self.scroll = self.selected + 1 - VISIBLE_ROWS;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let max = self.filtered.len() - 1;
        let next = if delta.is_negative() {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            (self.selected + delta as usize).min(max)
        };
        self.selected = next;
        self.adjust_scroll();
    }

    fn selected_path(&self) -> Option<&str> {
        let idx = *self.filtered.get(self.selected)?;
        self.candidates.get(idx).map(String::as_str)
    }
}

impl ModalView for FilePickerView {
    fn kind(&self) -> ModalKind {
        ModalKind::FilePicker
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => ViewAction::Close,
            KeyCode::Enter => {
                if let Some(path) = self.selected_path() {
                    let path = path.to_string();
                    return ViewAction::EmitAndClose(ViewEvent::FilePickerSelected { path });
                }
                ViewAction::Close
            }
            KeyCode::Up => {
                self.move_selection(-1);
                ViewAction::None
            }
            KeyCode::Down => {
                self.move_selection(1);
                ViewAction::None
            }
            KeyCode::PageUp => {
                self.move_selection(-(VISIBLE_ROWS as isize));
                ViewAction::None
            }
            KeyCode::PageDown => {
                self.move_selection(VISIBLE_ROWS as isize);
                ViewAction::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
                self.scroll = 0;
                self.refilter();
                ViewAction::None
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.clear();
                self.selected = 0;
                self.scroll = 0;
                self.refilter();
                ViewAction::None
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && !ch.is_control() =>
            {
                self.query.push(ch);
                self.selected = 0;
                self.scroll = 0;
                self.refilter();
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = 80.min(area.width.saturating_sub(4));
        let popup_height = ((VISIBLE_ROWS as u16) + 6).min(area.height.saturating_sub(4));

        let popup_area = Rect {
            x: area.x + (area.width.saturating_sub(popup_width)) / 2,
            y: area.y + (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let title = Line::from(vec![Span::styled(
            " File Picker ",
            Style::default()
                .fg(palette::WHALE_ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )]);
        let footer_text = format!(
            " {} match{}  ↑/↓ select  Enter insert @path  Esc close ",
            self.filtered.len(),
            if self.filtered.len() == 1 { "" } else { "es" },
        );
        let block = Block::default()
            .title(title)
            .title_bottom(Line::from(Span::styled(
                footer_text,
                Style::default().fg(palette::TEXT_MUTED),
            )))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::DEEPSEEK_INK))
            .padding(Padding::uniform(1));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        let mut lines: Vec<Line<'static>> = Vec::new();
        // Query line.
        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::raw(self.query.clone()),
            Span::styled(
                " ",
                Style::default()
                    .fg(palette::DEEPSEEK_INK)
                    .bg(palette::DEEPSEEK_SKY),
            ),
        ]));
        lines.push(Line::from(""));

        let visible = VISIBLE_ROWS.min(inner.height.saturating_sub(2) as usize);
        let end = (self.scroll + visible).min(self.filtered.len());
        if self.filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No matches",
                Style::default().fg(palette::TEXT_MUTED),
            )));
        } else {
            for idx in self.scroll..end {
                let path = &self.candidates[self.filtered[idx]];
                let selected = idx == self.selected;
                let style = if selected {
                    Style::default()
                        .fg(palette::SELECTION_TEXT)
                        .bg(palette::SELECTION_BG)
                } else {
                    Style::default().fg(palette::TEXT_PRIMARY)
                };
                let prefix = if selected { "▶ " } else { "  " };
                let marker_field = if inner.width >= 18 {
                    format!("{} ", self.relevance.markers_for(path))
                } else {
                    String::new()
                };
                let reserved = prefix.chars().count() + marker_field.chars().count();
                let display = truncate_path(path, (inner.width as usize).saturating_sub(reserved));
                let mut line = Line::from(format!("{prefix}{marker_field}{display}"));
                line.style = style;
                lines.push(line);
            }
        }

        Paragraph::new(lines)
            .style(Style::default().fg(palette::TEXT_PRIMARY))
            .render(inner, buf);
    }
}

fn truncate_path(path: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if path.chars().count() <= max {
        return path.to_string();
    }
    let take = max.saturating_sub(1);
    let truncated: String = path
        .chars()
        .rev()
        .take(take)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{truncated}")
}

/// Single-pass walk that collects workspace-relative paths. `max_depth` of
/// `None` walks the whole tree (still bounded by `MAX_CANDIDATES` and
/// `.gitignore`); `Some(n)` caps the recursion at `n` levels.
fn collect_candidates(root: &Path, max_depth: Option<usize>) -> Vec<String> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .follow_links(false)
        .max_depth(max_depth)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);

    let mut out: Vec<String> = Vec::new();
    for entry in builder.build().flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path);
        if rel.as_os_str().is_empty() {
            continue;
        }
        let display = path_to_workspace_string(rel);
        if !display.is_empty() {
            out.push(display);
        }
        if out.len() >= MAX_CANDIDATES {
            break;
        }
    }

    // Whitelist AI-tool dot-directories so they're discoverable even when
    // gitignored. Walk each one separately with gitignore disabled.
    for dir in DISCOVERY_ALWAYS_DIRS {
        let dot_dir = root.join(dir);
        if !dot_dir.is_dir() {
            continue;
        }
        let mut dot_builder = WalkBuilder::new(&dot_dir);
        dot_builder
            .hidden(true)
            .follow_links(false)
            .git_ignore(false)
            .ignore(false)
            .max_depth(max_depth.map(|d| d.saturating_sub(1)));
        for entry in dot_builder.build().flatten() {
            // Exclude machine-generated bulk (e.g. .deepseek/snapshots/).
            if path_is_excluded_from_discovery(root, entry.path()) {
                continue;
            }
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(path);
            if rel.as_os_str().is_empty() {
                continue;
            }
            let display = path_to_workspace_string(rel);
            if !display.is_empty() {
                out.push(display);
            }
            if out.len() >= MAX_CANDIDATES {
                break;
            }
        }
    }

    out.sort();
    out
}

fn path_to_workspace_string(path: &Path) -> String {
    // Use forward-slash separators for cross-platform display, matching how
    // @-mentions are spelled in the composer.
    let mut out = String::new();
    for (idx, comp) in path.components().enumerate() {
        if idx > 0 {
            out.push('/');
        }
        out.push_str(&comp.as_os_str().to_string_lossy());
    }
    out
}

/// Subsequence scorer with first-letter and boundary bonuses.
///
/// Returns `None` if `query` is not a subsequence of `path` (case-insensitive),
/// otherwise a positive score where higher is better.
///
/// Heuristics (kept deliberately small and predictable):
/// * +25 for each match that lands at the start of the path or right after a
///   boundary character (`/`, `_`, `-`, `.`, ` `).
/// * +10 if the very first character of the query matches the first character
///   of the path.
/// * +5 per consecutive match (rewards contiguous runs like typing "main" and
///   matching `main.rs`).
/// * Penalty proportional to the gap between consecutive matches keeps tightly
///   matched candidates above scattered ones.
pub fn score(query: &str, path: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let p: Vec<char> = path.chars().flat_map(char::to_lowercase).collect();
    if q.len() > p.len() {
        return None;
    }

    let mut qi = 0usize;
    let mut score: i32 = 0;
    let mut last_match: Option<usize> = None;
    let mut consecutive = 0i32;

    for (i, ch) in p.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if *ch == q[qi] {
            // Boundary / start bonus.
            if i == 0 {
                score += 25;
                if qi == 0 {
                    score += 10;
                }
            } else if matches!(p[i - 1], '/' | '_' | '-' | '.' | ' ') {
                score += 25;
            } else {
                score += 1;
            }

            // Consecutive bonus.
            if last_match == Some(i.saturating_sub(1)) {
                consecutive += 1;
                score += 5 * consecutive;
            } else {
                consecutive = 0;
            }

            // Gap penalty.
            if let Some(prev) = last_match {
                let gap = i - prev - 1;
                score -= gap as i32;
            }

            last_match = Some(i);
            qi += 1;
        }
    }

    if qi == q.len() { Some(score) } else { None }
}

#[cfg(test)]
mod tests {}
