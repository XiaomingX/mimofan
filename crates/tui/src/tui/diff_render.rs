//! Diff rendering helpers for TUI previews.
//!
//! The default layout matches the CodeBuddy-style review UX:
//! - **side-by-side** (`render_diff_side_by_side`): left = old file, right = new
//!   file, added blocks on the right and deleted blocks on the left. This is
//!   the default for any terminal wide enough to fit two gutters.
//! - **unified** (`render_diff`): inline git-style diff with dual line-number
//!   gutters and coloured `+`/`-` markers. Used only as a fallback on
//!   absurdly-narrow terminals (< [`SIDE_BY_SIDE_MIN_WIDTH`]).
//!
//! Both layouts apply per-language syntax highlighting via `syntect` when the
//! originating file path is known (`render_diff_auto` / `render_diff_side_by_side`).

use std::sync::OnceLock;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::palette;

const LINE_NUMBER_WIDTH: usize = 4;
/// Absurdly-narrow terminals (cannot fit two line-number gutters + content)
/// fall back to the inline unified layout. Otherwise we always render
/// side-by-side, matching the CodeBuddy review UX (left = old, right = new).
const SIDE_BY_SIDE_MIN_WIDTH: u16 = 40;
/// Half-width separator column between the two panes in side-by-side mode.
const SEPARATOR_WIDTH: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFileSummary {
    pub path: String,
    pub added: usize,
    pub deleted: usize,
    pub hunks: usize,
}

/// Unified diff line kinds used by the side-by-side layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Context,
    Added,
    Deleted,
}

#[derive(Debug, Clone)]
struct DiffRow {
    kind: LineKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    content: String,
}

/// Public entry point: always renders side-by-side (CodeBuddy-style), except
/// on terminals too narrow to fit two gutters, where it falls back to the
/// inline unified layout.
///
/// When `path` is `None`, the path is inferred from the diff's `+++ b/...`
/// header so syntax highlighting still works for tool-result diffs.
#[must_use]
pub fn render_diff_auto(diff: &str, width: u16, path: Option<&str>) -> Vec<Line<'static>> {
    let resolved = path.or_else(|| path_from_diff(diff));
    if width >= SIDE_BY_SIDE_MIN_WIDTH {
        render_diff_side_by_side(diff, width, resolved)
    } else {
        render_diff_with_path(diff, width, resolved)
    }
}

/// Extract the first `+++ b/<path>` header from a unified diff.
fn path_from_diff(diff: &str) -> Option<&str> {
    for raw in diff.lines() {
        if let Some(rest) = raw.strip_prefix("+++ ") {
            let p = rest.trim_start_matches("b/");
            if p != "/dev/null" && !p.is_empty() {
                return Some(p);
            }
        }
    }
    None
}

/// Backwards-compatible entry: inline unified diff (no syntax highlighting path).
#[must_use]
pub fn render_diff(diff: &str, width: u16) -> Vec<Line<'static>> {
    render_diff_with_path(diff, width, None)
}

/// Inline unified diff with optional syntax highlighting.
#[must_use]
pub fn render_diff_with_path(diff: &str, width: u16, path: Option<&str>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut old_line: Option<usize> = None;
    let mut new_line: Option<usize> = None;
    let summaries = summarize_diff(diff);

    if !summaries.is_empty() {
        lines.extend(render_diff_summary(&summaries, width));
    }

    for raw in diff.lines() {
        if raw.starts_with("diff --git") || raw.starts_with("index ") {
            lines.extend(render_header_line(raw, width));
            continue;
        }

        if raw.starts_with("--- ") || raw.starts_with("+++ ") {
            lines.extend(render_header_line(raw, width));
            continue;
        }

        if raw.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_header(raw) {
                old_line = Some(old_start);
                new_line = Some(new_start);
            }
            lines.extend(render_hunk_header(raw, width));
            continue;
        }

        if raw.starts_with('+') && !raw.starts_with("+++") {
            let content = raw.trim_start_matches('+');
            let styled = highlight_line(content, path);
            lines.extend(render_diff_line(
                &styled,
                width,
                old_line,
                new_line,
                '+',
                Style::default()
                    .fg(palette::DIFF_ADDED)
                    .bg(palette::DIFF_ADDED_BG),
            ));
            if let Some(line) = new_line.as_mut() {
                *line = line.saturating_add(1);
            }
            continue;
        }

        if raw.starts_with('-') && !raw.starts_with("---") {
            let content = raw.trim_start_matches('-');
            let styled = highlight_line(content, path);
            lines.extend(render_diff_line(
                &styled,
                width,
                old_line,
                new_line,
                '-',
                Style::default()
                    .fg(palette::STATUS_ERROR)
                    .bg(palette::DIFF_DELETED_BG),
            ));
            if let Some(line) = old_line.as_mut() {
                *line = line.saturating_add(1);
            }
            continue;
        }

        if raw.starts_with(' ') {
            let content = raw.trim_start_matches(' ');
            let styled = highlight_line(content, path);
            lines.extend(render_diff_line(
                &styled,
                width,
                old_line,
                new_line,
                ' ',
                Style::default().fg(palette::TEXT_PRIMARY),
            ));
            if let Some(line) = old_line.as_mut() {
                *line = line.saturating_add(1);
            }
            if let Some(line) = new_line.as_mut() {
                *line = line.saturating_add(1);
            }
            continue;
        }

        lines.extend(render_header_line(raw, width));
    }

    lines
}

/// Side-by-side diff: left pane = old file, right pane = new file.
#[must_use]
pub fn render_diff_side_by_side(diff: &str, width: u16, path: Option<&str>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let summaries = summarize_diff(diff);
    if !summaries.is_empty() {
        lines.extend(render_diff_summary(&summaries, width));
    }

    let half = side_by_side_half_width(width);
    if half < LINE_NUMBER_WIDTH as u16 + 4 {
        // Not enough room for two panes — fall back to unified.
        return render_diff_with_path(diff, width, path);
    }

    // Parse into per-hunk row lists, then render each hunk as aligned columns.
    let hunks = parse_hunks(diff);
    for (hunk_header, rows) in hunks {
        lines.extend(render_hunk_header(&hunk_header, width));
        lines.extend(render_side_by_side_rows(&rows, half, path));
    }

    lines
}

/// Split a unified diff into `(hunk header, rows)` pairs.
fn parse_hunks(diff: &str) -> Vec<(String, Vec<DiffRow>)> {
    let mut hunks: Vec<(String, Vec<DiffRow>)> = Vec::new();
    let mut old_line: Option<usize> = None;
    let mut new_line: Option<usize> = None;

    for raw in diff.lines() {
        if raw.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_header(raw) {
                old_line = Some(old_start);
                new_line = Some(new_start);
            }
            hunks.push((raw.to_string(), Vec::new()));
            continue;
        }
        if raw.starts_with("diff --git")
            || raw.starts_with("index ")
            || raw.starts_with("--- ")
            || raw.starts_with("+++ ")
        {
            continue;
        }

        let (kind, content, advance_old, advance_new) =
            if raw.starts_with('+') && !raw.starts_with("+++") {
                (LineKind::Added, raw.trim_start_matches('+'), false, true)
            } else if raw.starts_with('-') && !raw.starts_with("---") {
                (LineKind::Deleted, raw.trim_start_matches('-'), true, false)
            } else if raw.starts_with(' ') {
                (LineKind::Context, raw.trim_start_matches(' '), true, true)
            } else {
                // Unknown line (e.g. blank) — treat as context-less filler, skip.
                continue;
            };

        let row = DiffRow {
            kind,
            old_line,
            new_line,
            content: content.to_string(),
        };
        if let Some((_, rows)) = hunks.last_mut() {
            rows.push(row);
        } else {
            // Lines before the first @@ — keep a single synthetic hunk.
            hunks.push(("@@ -0,0 +0,0 @@".to_string(), vec![row]));
        }

        if advance_old {
            old_line = old_line.map(|l| l.saturating_add(1));
        }
        if advance_new {
            new_line = new_line.map(|l| l.saturating_add(1));
        }
    }

    hunks
}

/// Render one hunk's rows as aligned left/right panes.
fn render_side_by_side_rows(rows: &[DiffRow], half: u16, path: Option<&str>) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < rows.len() {
        let row = &rows[idx];
        match row.kind {
            LineKind::Context => {
                // Context rows align directly; both panes show the same line.
                out.extend(render_side_by_side_pair(
                    Some(row),
                    Some(row),
                    LineKind::Context,
                    half,
                    path,
                ));
                idx += 1;
            }
            LineKind::Added => {
                // Gather a contiguous run of added rows (right pane only).
                let mut added = Vec::new();
                while idx < rows.len() && rows[idx].kind == LineKind::Added {
                    added.push(rows[idx].clone());
                    idx += 1;
                }
                // Pair with preceding/following context for alignment if available.
                out.extend(render_added_only(&added, half, path));
            }
            LineKind::Deleted => {
                let mut deleted = Vec::new();
                while idx < rows.len() && rows[idx].kind == LineKind::Deleted {
                    deleted.push(rows[idx].clone());
                    idx += 1;
                }
                out.extend(render_deleted_only(&deleted, half, path));
            }
        }
    }
    out
}

fn render_added_only(added: &[DiffRow], half: u16, path: Option<&str>) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for row in added {
        // Left pane empty placeholder, right pane shows the added line.
        out.extend(render_side_by_side_pair(
            None,
            Some(row),
            LineKind::Added,
            half,
            path,
        ));
    }
    out
}

fn render_deleted_only(deleted: &[DiffRow], half: u16, path: Option<&str>) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for row in deleted {
        out.extend(render_side_by_side_pair(
            Some(row),
            None,
            LineKind::Deleted,
            half,
            path,
        ));
    }
    out
}

/// Render a single paired row. `left` is the old-file side, `right` is the
/// new-file side. Either side may be `None` (placeholder, greyed out).
fn render_side_by_side_pair(
    left: Option<&DiffRow>,
    right: Option<&DiffRow>,
    kind: LineKind,
    half: u16,
    path: Option<&str>,
) -> Vec<Line<'static>> {
    let left_content = left.map(|r| r.content.as_str()).unwrap_or("");
    let right_content = right.map(|r| r.content.as_str()).unwrap_or("");
    let left_old = left.and_then(|r| r.old_line);
    let right_new = right.and_then(|r| r.new_line);

    let left_style = match kind {
        LineKind::Deleted => Style::default()
            .fg(palette::STATUS_ERROR)
            .bg(palette::DIFF_DELETED_BG),
        LineKind::Context => Style::default().fg(palette::TEXT_MUTED),
        LineKind::Added => Style::default().fg(palette::TEXT_MUTED),
    };
    let right_style = match kind {
        LineKind::Added => Style::default()
            .fg(palette::DIFF_ADDED)
            .bg(palette::DIFF_ADDED_BG),
        LineKind::Context => Style::default().fg(palette::TEXT_MUTED),
        LineKind::Deleted => Style::default().fg(palette::TEXT_MUTED),
    };

    let left_prefix = format_line_numbers(left_old, None, left_marker(kind));
    let right_prefix = format_line_numbers(None, right_new, right_marker(kind));

    let left_avail = half.saturating_sub(left_prefix.width() as u16).max(1) as usize;
    let right_avail = half.saturating_sub(right_prefix.width() as u16).max(1) as usize;

    let left_wrapped = wrap_text(left_content, left_avail);
    let right_wrapped = wrap_text(right_content, right_avail);

    let left_spans = highlight_line(left_content, path);
    let right_spans = highlight_line(right_content, path);

    let max_lines = left_wrapped.len().max(right_wrapped.len()).max(1);
    let mut out = Vec::new();
    for i in 0..max_lines {
        let l_text = left_wrapped.get(i).map(String::as_str).unwrap_or("");
        let r_text = right_wrapped.get(i).map(String::as_str).unwrap_or("");

        let l_spans = if i == 0 {
            apply_style_to_spans(&left_spans, left_style)
        } else {
            vec![Span::styled(l_text.to_string(), left_style)]
        };
        let r_spans = if i == 0 {
            apply_style_to_spans(&right_spans, right_style)
        } else {
            vec![Span::styled(r_text.to_string(), right_style)]
        };

        let prefix_left = if i == 0 {
            Span::styled(
                left_prefix.clone(),
                Style::default().fg(palette::TEXT_MUTED),
            )
        } else {
            Span::raw(" ".repeat(left_prefix.width()))
        };
        let prefix_right = if i == 0 {
            Span::styled(
                right_prefix.clone(),
                Style::default().fg(palette::TEXT_MUTED),
            )
        } else {
            Span::raw(" ".repeat(right_prefix.width()))
        };

        let sep = Span::styled(
            " ".repeat(SEPARATOR_WIDTH),
            Style::default().fg(palette::TEXT_MUTED),
        );

        let mut spans = vec![prefix_left];
        spans.extend(l_spans);
        // Pad left pane to `half` width so the separator stays aligned.
        let used_left = left_prefix.width() + l_text.width();
        let pad_left = half.saturating_sub(used_left as u16) as usize;
        spans.push(Span::raw(" ".repeat(pad_left)));
        spans.push(sep);
        spans.push(prefix_right);
        spans.extend(r_spans);
        out.push(Line::from(spans));
    }
    out
}

fn left_marker(kind: LineKind) -> char {
    match kind {
        LineKind::Deleted => '-',
        _ => ' ',
    }
}

fn right_marker(kind: LineKind) -> char {
    match kind {
        LineKind::Added => '+',
        _ => ' ',
    }
}

fn side_by_side_half_width(width: u16) -> u16 {
    // Subtract the separator and split the rest in two.
    let usable = width.saturating_sub(SEPARATOR_WIDTH as u16);
    usable / 2
}

#[must_use]
pub fn summarize_diff(diff: &str) -> Vec<DiffFileSummary> {
    let mut summaries = Vec::new();
    let mut current: Option<DiffFileSummary> = None;

    for raw in diff.lines() {
        if raw.starts_with("diff --git ") {
            if let Some(summary) = current.take()
                && summary.has_changes()
            {
                summaries.push(summary);
            }
            current = Some(DiffFileSummary {
                path: parse_diff_git_path(raw).unwrap_or_else(|| "<file>".to_string()),
                added: 0,
                deleted: 0,
                hunks: 0,
            });
            continue;
        }

        if raw.starts_with("+++ ") {
            let path = raw
                .trim_start_matches("+++ ")
                .trim_start_matches("b/")
                .to_string();
            if path != "/dev/null" {
                current
                    .get_or_insert_with(|| DiffFileSummary {
                        path: path.clone(),
                        added: 0,
                        deleted: 0,
                        hunks: 0,
                    })
                    .path = path.clone();
            }
            continue;
        }

        if raw.starts_with("@@") {
            current
                .get_or_insert_with(|| DiffFileSummary {
                    path: "<file>".to_string(),
                    added: 0,
                    deleted: 0,
                    hunks: 0,
                })
                .hunks += 1;
            continue;
        }

        if raw.starts_with('+') && !raw.starts_with("+++") {
            current
                .get_or_insert_with(|| DiffFileSummary {
                    path: "<file>".to_string(),
                    added: 0,
                    deleted: 0,
                    hunks: 0,
                })
                .added += 1;
        } else if raw.starts_with('-') && !raw.starts_with("---") {
            current
                .get_or_insert_with(|| DiffFileSummary {
                    path: "<file>".to_string(),
                    added: 0,
                    deleted: 0,
                    hunks: 0,
                })
                .deleted += 1;
        }
    }

    if let Some(summary) = current
        && summary.has_changes()
    {
        summaries.push(summary);
    }

    summaries
}

#[must_use]
pub fn diff_summary_label(diff: &str) -> Option<String> {
    let summaries = summarize_diff(diff);
    if summaries.is_empty() {
        return None;
    }
    let files = summaries.len();
    let added: usize = summaries.iter().map(|summary| summary.added).sum();
    let deleted: usize = summaries.iter().map(|summary| summary.deleted).sum();
    Some(format!(
        "{files} file{} +{added} -{deleted}",
        if files == 1 { "" } else { "s" }
    ))
}

impl DiffFileSummary {
    fn has_changes(&self) -> bool {
        self.added > 0 || self.deleted > 0 || self.hunks > 0
    }
}

fn parse_diff_git_path(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    let _diff = parts.next()?;
    let _git = parts.next()?;
    let _old = parts.next()?;
    let new = parts.next()?;
    Some(new.trim_start_matches("b/").to_string())
}

fn render_diff_summary(summaries: &[DiffFileSummary], width: u16) -> Vec<Line<'static>> {
    let files = summaries.len();
    let added: usize = summaries.iter().map(|summary| summary.added).sum();
    let deleted: usize = summaries.iter().map(|summary| summary.deleted).sum();
    let hunks: usize = summaries.iter().map(|summary| summary.hunks).sum();

    let mut lines = Vec::new();
    lines.extend(wrap_with_style(
        &format!(
            "summary: {files} file{}, +{added} -{deleted}, {hunks} hunk{}",
            if files == 1 { "" } else { "s" },
            if hunks == 1 { "" } else { "s" },
        ),
        Style::default()
            .fg(palette::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD),
        width,
    ));
    for summary in summaries {
        let row = format!(
            "  {}  +{} -{}  {} hunk{}",
            summary.path,
            summary.added,
            summary.deleted,
            summary.hunks,
            if summary.hunks == 1 { "" } else { "s" },
        );
        lines.extend(wrap_with_style(
            &row,
            Style::default().fg(palette::TEXT_MUTED),
            width,
        ));
    }
    lines
}

fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    let old = parts[1].trim_start_matches('-');
    let new = parts[2].trim_start_matches('+');
    let old_start = old.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new.split(',').next()?.parse::<usize>().ok()?;
    Some((old_start, new_start))
}

fn render_header_line(line: &str, width: u16) -> Vec<Line<'static>> {
    let style = Style::default()
        .fg(palette::MIMOFAN_SKY)
        .add_modifier(Modifier::BOLD);
    wrap_with_style(line, style, width)
}

fn render_hunk_header(line: &str, width: u16) -> Vec<Line<'static>> {
    let style = Style::default().fg(palette::MIMOFAN_ACCENT_PRIMARY);
    wrap_with_style(line, style, width)
}

/// Render a single unified-diff line. `styled` is the syntax-highlighted
/// content (already split into foreground-coloured spans); `style` supplies
/// the base (marker/background) colour for the line.
fn render_diff_line(
    styled: &[(Style, String)],
    width: u16,
    old_line: Option<usize>,
    new_line: Option<usize>,
    marker: char,
    style: Style,
) -> Vec<Line<'static>> {
    let prefix = format_line_numbers(old_line, new_line, marker);
    let prefix_width = prefix.width();
    let available = width.saturating_sub(prefix_width as u16).max(1) as usize;

    // Flatten highlighted spans into one string for wrapping, then re-apply.
    let content: String = styled.iter().map(|(_, s)| s.as_str()).collect();
    let wrapped = wrap_text(&content, available);
    if wrapped.is_empty() {
        return vec![Line::from(vec![Span::styled(
            prefix,
            Style::default().fg(palette::TEXT_MUTED),
        )])];
    }

    let mut out = Vec::new();
    for (idx, chunk) in wrapped.into_iter().enumerate() {
        let spans = if idx == 0 {
            apply_style_to_chunk(&chunk, styled, style)
        } else {
            vec![Span::styled(chunk, style)]
        };
        let prefix_span = if idx == 0 {
            Span::styled(prefix.clone(), Style::default().fg(palette::TEXT_MUTED))
        } else {
            Span::raw(" ".repeat(prefix_width))
        };
        let mut line_spans = vec![prefix_span];
        line_spans.extend(spans);
        out.push(Line::from(line_spans));
    }
    out
}

/// Split `chunk` against the highlighted `styled` ranges and emit spans that
/// keep both the syntax foreground colour and the line's base (bg/marker) style.
fn apply_style_to_chunk(
    chunk: &str,
    styled: &[(Style, String)],
    base: Style,
) -> Vec<Span<'static>> {
    // Reconstruct the full original string to locate `chunk` offsets.
    let full: String = styled.iter().map(|(_, s)| s.as_str()).collect();
    let start = full.find(chunk).unwrap_or(0);
    let end = start + chunk.len();

    let mut spans = Vec::new();
    let mut cursor = 0usize;
    for (style, text) in styled {
        let seg_start = cursor;
        let seg_end = cursor + text.len();
        cursor = seg_end;
        if seg_end <= start || seg_start >= end {
            continue;
        }
        let slice_start = seg_start.max(start) - start;
        let slice_end = seg_end.min(end) - start;
        let slice = &chunk[slice_start..slice_end];
        if slice.is_empty() {
            continue;
        }
        let mut merged = base;
        merged.fg = style.fg;
        spans.push(Span::styled(slice.to_string(), merged));
    }
    if spans.is_empty() {
        spans.push(Span::styled(chunk.to_string(), base));
    }
    spans
}

/// Apply `base` (background/marker colour) over the highlighted spans, keeping
/// each token's syntax foreground colour.
fn apply_style_to_spans(styled: &[(Style, String)], base: Style) -> Vec<Span<'static>> {
    styled
        .iter()
        .map(|(style, text)| {
            let mut merged = base;
            merged.fg = style.fg;
            Span::styled(text.clone(), merged)
        })
        .collect()
}

fn format_line_numbers(old_line: Option<usize>, new_line: Option<usize>, marker: char) -> String {
    let old = old_line
        .map(|value| format!("{value:>LINE_NUMBER_WIDTH$}"))
        .unwrap_or_else(|| " ".repeat(LINE_NUMBER_WIDTH));
    let new = new_line
        .map(|value| format!("{value:>LINE_NUMBER_WIDTH$}"))
        .unwrap_or_else(|| " ".repeat(LINE_NUMBER_WIDTH));
    format!("{old} {new} {marker} ")
}

fn wrap_with_style(text: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for part in wrap_text(text, width.max(1) as usize) {
        out.push(Line::from(Span::styled(part, style)));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled("", style)));
    }
    out
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let lead = text
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let lead_width = lead.width();
    let mut current = lead.clone();
    let mut current_width = lead_width;
    let mut has_word = false;

    for word in trimmed.split_whitespace() {
        let word_width = word.width();
        if word_width > width {
            if has_word {
                lines.push(std::mem::take(&mut current));
                current = lead.clone();
                current_width = lead_width;
            }
            push_word_breaking_chars(word, width, &mut current, &mut current_width, &mut lines);
            has_word = current_width > lead_width;
            continue;
        }
        let additional = if has_word { word_width + 1 } else { word_width };
        if current_width + additional > width && has_word {
            lines.push(current);
            current = lead.clone();
            current_width = lead_width;
            has_word = false;
        }
        if has_word {
            current.push(' ');
            current_width += 1;
        }
        if current_width + word_width > width && !has_word && lead_width > 0 {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if current_width == 0 && lead_width > 0 && word_width + lead_width <= width {
            current = lead.clone();
            current_width = lead_width;
        }
        current.push_str(word);
        current_width += word_width;
        has_word = true;
    }

    if has_word || !current.is_empty() {
        lines.push(current);
    } else {
        lines.push(String::new());
    }

    lines
}

fn push_word_breaking_chars(
    word: &str,
    width: usize,
    current: &mut String,
    current_width: &mut usize,
    lines: &mut Vec<String>,
) {
    for ch in word.chars() {
        let char_width = ch.width().unwrap_or(1);
        if *current_width + char_width > width && *current_width > 0 {
            lines.push(std::mem::take(current));
            *current_width = 0;
        }
        current.push(ch);
        *current_width += char_width;
    }
}

// ----------------------------------------------------------------------------
// Syntax highlighting (syntect)
// ----------------------------------------------------------------------------

struct Highlighter {
    syntax_set: SyntaxSet,
    theme: syntect::highlighting::Theme,
}

static HIGHLIGHTER: OnceLock<Highlighter> = OnceLock::new();

fn highlighter() -> &'static Highlighter {
    HIGHLIGHTER.get_or_init(|| {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .unwrap_or_else(|| theme_set.themes.values().next().cloned().unwrap());
        Highlighter { syntax_set, theme }
    })
}

fn syntax_for_path(path: Option<&str>) -> Option<&'static SyntaxReference> {
    let path = path?;
    let hl = highlighter();
    hl.syntax_set
        .find_syntax_by_extension(std::path::Path::new(path).extension()?.to_str()?)
        .or_else(|| hl.syntax_set.find_syntax_by_token(path))
}

/// Highlight a single line into foreground-coloured spans. Returns a single
/// default-coloured span when highlighting is unavailable.
fn highlight_line(line: &str, path: Option<&str>) -> Vec<(Style, String)> {
    let syntax = match syntax_for_path(path) {
        Some(s) => s,
        None => return vec![(Style::default(), line.to_string())],
    };
    let hl = highlighter();
    let mut h = HighlightLines::new(syntax, &hl.theme);
    let mut out = Vec::new();
    for text in LinesWithEndings::from(line) {
        match h.highlight_line(text, &hl.syntax_set) {
            Ok(ranges) => {
                for (s_style, s_text) in ranges {
                    let fg = ratatui::style::Color::Rgb(
                        s_style.foreground.r,
                        s_style.foreground.g,
                        s_style.foreground.b,
                    );
                    let mut style = Style::default().fg(fg);
                    if s_style.font_style.contains(FontStyle::BOLD) {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if s_style.font_style.contains(FontStyle::ITALIC) {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    out.push((style, s_text.trim_end_matches('\n').to_string()));
                }
            }
            Err(_) => out.push((Style::default(), line.to_string())),
        }
    }
    if out.is_empty() {
        out.push((Style::default(), line.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_diff() -> String {
        crate::tools::diff_format::make_unified_diff(
            "src/main.rs",
            "fn main() {\n    println!(\"old\");\n}\n",
            "fn main() {\n    println!(\"new\");\n    let x = 1;\n}\n",
        )
    }

    #[test]
    fn medium_terminal_uses_side_by_side_layout() {
        // CodeBuddy-style: side-by-side is the default even at 80 columns.
        let diff = sample_diff();
        let lines = render_diff_auto(&diff, 80, None);
        assert!(!lines.is_empty());
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(joined.contains("main.rs"));
    }

    #[test]
    fn extremely_narrow_terminal_falls_back_to_unified() {
        // Below the absolute minimum width, side-by-side cannot fit two
        // gutters, so we fall back to the inline unified layout.
        let diff = sample_diff();
        let lines = render_diff_auto(&diff, 30, None);
        assert!(!lines.is_empty());
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(joined.contains("main.rs"));
    }

    #[test]
    fn wide_terminal_uses_side_by_side_layout() {
        let diff = sample_diff();
        let lines = render_diff_auto(&diff, 160, None);
        assert!(!lines.is_empty());
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(joined.contains("main.rs"));
    }

    #[test]
    fn path_derived_from_diff_enables_highlighting_without_panic() {
        // No explicit path; render_diff_auto should infer it from `+++ b/...`.
        let diff = sample_diff();
        let unified = render_diff_auto(&diff, 80, None);
        let side_by_side = render_diff_auto(&diff, 200, None);
        assert!(!unified.is_empty());
        assert!(!side_by_side.is_empty());
    }

    #[test]
    fn unknown_extension_falls_back_to_plain_highlight() {
        let diff = crate::tools::diff_format::make_unified_diff("README", "a\n", "b\n");
        let lines = render_diff_auto(&diff, 160, None);
        assert!(!lines.is_empty());
    }

    #[test]
    fn side_by_side_shows_added_content_on_right_pane() {
        // Build a diff where the new file gained a line; the added text must
        // appear in the rendered output (on the right pane).
        let diff = crate::tools::diff_format::make_unified_diff(
            "src/main.rs",
            "fn main() {\n}\n",
            "fn main() {\n    let x = 42;\n}\n",
        );
        let lines = render_diff_auto(&diff, 200, None);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            joined.contains("let x = 42;"),
            "added line should appear in side-by-side output"
        );
    }
}
