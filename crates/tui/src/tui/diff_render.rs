//! Diff rendering helpers for TUI previews.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::palette;

const LINE_NUMBER_WIDTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFileSummary {
    pub path: String,
    pub added: usize,
    pub deleted: usize,
    pub hunks: usize,
}

pub fn render_diff(diff: &str, width: u16) -> Vec<Line<'static>> {
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
            lines.extend(render_diff_line(
                content,
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
            lines.extend(render_diff_line(
                content,
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
            lines.extend(render_diff_line(
                content,
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
        .fg(palette::DEEPSEEK_SKY)
        .add_modifier(Modifier::BOLD);
    wrap_with_style(line, style, width)
}

fn render_hunk_header(line: &str, width: u16) -> Vec<Line<'static>> {
    let style = Style::default().fg(palette::WHALE_ACCENT_PRIMARY);
    wrap_with_style(line, style, width)
}

fn render_diff_line(
    content: &str,
    width: u16,
    old_line: Option<usize>,
    new_line: Option<usize>,
    marker: char,
    style: Style,
) -> Vec<Line<'static>> {
    let prefix = format_line_numbers(old_line, new_line, marker);
    let prefix_width = prefix.width();
    let available = width.saturating_sub(prefix_width as u16).max(1) as usize;
    let wrapped = wrap_text(content, available);

    let mut out = Vec::new();
    for (idx, chunk) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            out.push(Line::from(vec![
                Span::styled(prefix.clone(), Style::default().fg(palette::TEXT_MUTED)),
                Span::styled(chunk, style),
            ]));
        } else {
            out.push(Line::from(vec![
                Span::raw(" ".repeat(prefix_width)),
                Span::styled(chunk, style),
            ]));
        }
    }

    if out.is_empty() {
        out.push(Line::from(vec![Span::styled(
            prefix,
            Style::default().fg(palette::TEXT_MUTED),
        )]));
    }

    out
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

#[cfg(test)]
mod tests {}
