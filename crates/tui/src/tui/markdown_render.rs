//! Markdown rendering for TUI transcript lines.
//!
//! ## Width-independent parse vs width-dependent render (CX#6)
//!
//! The previous renderer was a single function `render_markdown(content, width)`
//! that scanned the source, classified each line (heading / list / code-fence /
//! paragraph / link), and word-wrapped to `Line<'static>` in one pass. That meant
//! every terminal resize forced a full re-parse of the source for every visible
//! cell — wasted work on the streaming cell whose content is changing anyway.
//!
//! The codex tui solves this by splitting parse from render. We mirror that:
//!
//! * [`parse`] turns the markdown source into a [`ParsedMarkdown`] AST: a vector
//!   of width-independent [`Block`]s. The block kind already records all the
//!   classification decisions (heading level, list bullet, code block membership)
//!   that don't depend on width.
//! * [`render_parsed`] takes a `ParsedMarkdown` plus a width and a base style and
//!   produces `Vec<Line<'static>>`. It only does word-wrap and span styling.
//!
//! [`render_markdown`] is kept as a thin convenience that does both — useful for
//! callers (Thinking body, message body) that don't want to manage the cache.
//!
//! The transcript cache layer (see `tui/transcript.rs`) caches the parsed AST per
//! cell and re-runs only the render step on width changes. That makes resize a
//! re-flow operation rather than a re-parse + re-flow operation.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::palette;
use crate::tui::osc8;
use crate::tui::ui_text::CopyLineSeparator;

// Thread-local counter incremented every time `parse` runs. Used by tests to
// prove that width-only changes hit the cached-AST path and skip parsing.
// Thread-local (not global atomic) so concurrent tests calling `parse()` can't
// pollute each other's counters.
#[cfg(test)]
thread_local! {
    static PARSE_INVOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[must_use]
pub fn parse_invocation_count() -> u64 {
    PARSE_INVOCATIONS.with(|c| c.get())
}

/// One classified line of markdown source, width-independent.
///
/// All decisions that depend only on the source text (heading level, bullet
/// kind, whether we're inside a fenced code block, paragraph text) are made at
/// parse time. Width-dependent layout (word-wrap, prefix indent) is deferred to
/// the render step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `# heading text`. Includes the heading level (1..6).
    Heading { level: usize, text: String },
    /// A horizontal rule emitted under a level-1 heading.
    HeadingRule,
    /// A standalone `---` / `***` / `___` horizontal rule.
    HorizontalRule,
    /// A bullet (`-`/`*`) or ordered (`1.`) list item with its prefix and body.
    ListItem { bullet: String, text: String },
    /// A line inside a fenced code block. Fences themselves are dropped.
    Code { line: String },
    /// A table row: cells split on `|`.
    TableRow(Vec<String>),
    /// A table separator row (`|---|---|`). Kept so the renderer can draw
    /// horizontal rules at the correct positions.
    TableSeparator,
    /// A non-empty paragraph line that may contain inline links.
    Paragraph { text: String },
    /// An empty source line, preserved so paragraph spacing survives.
    Blank,
}

/// Width-independent parsed-markdown AST for one cell's source.
///
/// Wrapped in `Arc` at the cache layer so the cache can hand the same AST to
/// many render calls without copying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMarkdown {
    blocks: Vec<Block>,
}

/// Width-dependent rendered line plus the source block kind that produced it.
///
/// Most callers only need styled terminal lines, but transcript rendering also
/// needs to avoid adding its conversational continuation rail in front of code
/// blocks. Keeping this metadata here avoids guessing from styled spans.
#[derive(Debug, Clone)]
pub struct RenderedMarkdownLine {
    pub line: Line<'static>,
    pub is_code: bool,
    pub copy_prefix_width: usize,
    pub copy_separator_after: CopyLineSeparator,
}

/// Parse markdown source into a width-independent block AST.
///
/// This is a small line-oriented parser tuned for the patterns we render:
/// fenced code blocks, ATX headings, dash/star/numbered list items, and plain
/// paragraphs with optional links. It does not attempt to handle every CommonMark
/// edge case — that's intentional. The renderer will treat anything we don't
/// classify as `Block::Paragraph`.
#[must_use]
pub fn parse(content: &str) -> ParsedMarkdown {
    #[cfg(test)]
    PARSE_INVOCATIONS.with(|c| c.set(c.get() + 1));

    let mut blocks = Vec::new();
    let mut in_code_block = false;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            blocks.push(Block::Code {
                line: raw_line.to_string(),
            });
            continue;
        }

        if let Some((level, text)) = parse_heading(trimmed) {
            blocks.push(Block::Heading {
                level,
                text: text.to_string(),
            });
            if level == 1 {
                blocks.push(Block::HeadingRule);
            }
            continue;
        }

        if let Some((bullet, text)) = parse_list_item(trimmed) {
            blocks.push(Block::ListItem {
                bullet,
                text: text.to_string(),
            });
            continue;
        }

        if is_horizontal_rule(trimmed) {
            blocks.push(Block::HorizontalRule);
            continue;
        }

        match parse_table_row(trimmed) {
            Some(cells) => {
                blocks.push(Block::TableRow(cells));
                continue;
            }
            None if trimmed.starts_with('|') => {
                blocks.push(Block::TableSeparator);
                continue;
            }
            None => {}
        }

        if trimmed.is_empty() {
            // Whitespace-only lines are blank paragraphs.
            blocks.push(Block::Blank);
            continue;
        }

        blocks.push(Block::Paragraph {
            text: raw_line.to_string(),
        });
    }

    ParsedMarkdown { blocks }
}

/// Render a parsed-markdown AST at the given terminal width.
///
/// This is the width-dependent half: word-wrapping, link styling, code-block
/// formatting. The AST is owned by the caller (typically the transcript cache),
/// so width-only changes can call `render_parsed` again with the same AST and
/// skip the parse step entirely.
#[must_use]
pub fn render_parsed(parsed: &ParsedMarkdown, width: u16, base_style: Style) -> Vec<Line<'static>> {
    render_parsed_tagged(parsed, width, base_style)
        .into_iter()
        .map(|line| line.line)
        .collect()
}

/// Render a parsed-markdown AST and preserve per-line source metadata.
#[must_use]
pub fn render_parsed_tagged(
    parsed: &ParsedMarkdown,
    width: u16,
    base_style: Style,
) -> Vec<RenderedMarkdownLine> {
    let width = width.max(1) as usize;
    let mut out: Vec<RenderedMarkdownLine> = Vec::with_capacity(parsed.blocks.len());

    let mut i = 0;
    while i < parsed.blocks.len() {
        if matches!(
            &parsed.blocks[i],
            Block::TableRow(_) | Block::TableSeparator
        ) {
            let start = i;
            while i < parsed.blocks.len()
                && matches!(
                    &parsed.blocks[i],
                    Block::TableRow(_) | Block::TableSeparator
                )
            {
                i += 1;
            }
            out.extend(
                render_table_group(&parsed.blocks[start..i], width, base_style)
                    .into_iter()
                    .map(|line| RenderedMarkdownLine {
                        line,
                        is_code: false,
                        copy_prefix_width: 0,
                        copy_separator_after: CopyLineSeparator::Newline,
                    }),
            );
            continue;
        }

        match &parsed.blocks[i] {
            Block::Heading { text, .. } => {
                let style = Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::BOLD);
                out.extend(render_wrapped_line_tagged(text, width, style, false, false));
            }
            Block::HeadingRule => {
                out.push(RenderedMarkdownLine {
                    line: Line::from(Span::styled(
                        "─".repeat(width.min(40)),
                        Style::default().fg(palette::TEXT_DIM),
                    )),
                    is_code: false,
                    copy_prefix_width: 0,
                    copy_separator_after: CopyLineSeparator::Newline,
                });
            }
            Block::HorizontalRule => {
                out.push(RenderedMarkdownLine {
                    line: Line::from(Span::styled(
                        "─".repeat(width.min(60)),
                        Style::default().fg(palette::TEXT_DIM),
                    )),
                    is_code: false,
                    copy_prefix_width: 0,
                    copy_separator_after: CopyLineSeparator::Newline,
                });
            }
            Block::ListItem { bullet, text } => {
                let bullet_style = Style::default().fg(palette::DEEPSEEK_SKY);
                out.extend(render_list_line_tagged(
                    bullet,
                    text,
                    width,
                    bullet_style,
                    base_style,
                ));
            }
            Block::Code { line } => {
                let code_style = Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::ITALIC);
                out.extend(render_wrapped_line_tagged(
                    line, width, code_style, true, true,
                ));
            }
            Block::Paragraph { text } => {
                let link_style = Style::default()
                    .fg(palette::WHALE_ACCENT_PRIMARY)
                    .add_modifier(Modifier::UNDERLINED);
                out.extend(render_line_with_links_tagged(
                    text, width, base_style, link_style,
                ));
            }
            Block::Blank => {
                out.push(RenderedMarkdownLine {
                    line: Line::from(""),
                    is_code: false,
                    copy_prefix_width: 0,
                    copy_separator_after: CopyLineSeparator::Newline,
                });
            }
            Block::TableRow(_) | Block::TableSeparator => unreachable!(),
        }
        i += 1;
    }

    if out.is_empty() {
        out.push(RenderedMarkdownLine {
            line: Line::from(""),
            is_code: false,
            copy_prefix_width: 0,
            copy_separator_after: CopyLineSeparator::Newline,
        });
    }

    out
}

/// Convenience wrapper: parse + render in one call.
///
/// Equivalent to `render_parsed(&parse(content), width, base_style)`. Callers
/// that don't manage their own cache (the Thinking body, the immediate message
/// body) use this.
#[must_use]
pub fn render_markdown(content: &str, width: u16, base_style: Style) -> Vec<Line<'static>> {
    let parsed = parse(content);
    render_parsed(&parsed, width, base_style)
}

/// Convenience wrapper: parse + render while keeping per-line source metadata.
#[must_use]
pub fn render_markdown_tagged(
    content: &str,
    width: u16,
    base_style: Style,
) -> Vec<RenderedMarkdownLine> {
    let parsed = parse(content);
    render_parsed_tagged(&parsed, width, base_style)
}

/// Render plain text: split on newlines, word-wrap each line independently,
/// preserve leading whitespace and blank lines. No markdown interpretation.
#[must_use]
pub fn render_plain_text(content: &str, width: u16, base_style: Style) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut lines = Vec::new();
    for raw_line in content.split('\n') {
        if raw_line.is_empty() {
            lines.push(Line::from(""));
        } else {
            lines.extend(wrap_plain_line(raw_line, width, base_style));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines
}

/// Word-wrap a single line at `width`, preserving leading whitespace.
/// Handles over-long words by char-breaking (same strategy as the markdown
/// line renderer).
fn wrap_plain_line(line: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    if width == 0 || line.is_empty() {
        return vec![Line::from("")];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    let mut last_break_pos = None;

    for ch in line.chars() {
        loop {
            let ch_width = char_display_width(ch, current_width);
            if current_width + ch_width <= width || current.is_empty() {
                break;
            }

            if let Some(pos) = last_break_pos {
                if pos == current.len() {
                    chunks.push(std::mem::take(&mut current));
                    current_width = 0;
                    last_break_pos = None;
                    break;
                }

                if current[..pos].chars().any(|c| !c.is_whitespace()) {
                    let tail = current.split_off(pos);
                    chunks.push(std::mem::take(&mut current));
                    current = tail;
                    current_width = plain_display_width(&current);
                    last_break_pos = last_plain_break_pos(&current);
                    continue;
                }
            }

            chunks.push(std::mem::take(&mut current));
            current_width = 0;
            last_break_pos = None;
            break;
        }

        let ch_width = char_display_width(ch, current_width);
        current.push(ch);
        current_width += ch_width;
        if ch.is_whitespace() {
            last_break_pos = Some(current.len());
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    if chunks.is_empty() {
        return vec![Line::from("")];
    }

    chunks
        .into_iter()
        .map(|chunk| Line::from(vec![Span::styled(chunk, style)]))
        .collect()
}

fn plain_display_width(text: &str) -> usize {
    let mut width = 0usize;
    for ch in text.chars() {
        width += char_display_width(ch, width);
    }
    width
}

fn last_plain_break_pos(text: &str) -> Option<usize> {
    text.char_indices()
        .rev()
        .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx + ch.len_utf8()))
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 {
        return None;
    }
    let text = trimmed[hashes..].trim();
    if text.is_empty() {
        None
    } else {
        Some((hashes, text))
    }
}

fn parse_list_item(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return Some(("-".to_string(), trimmed[2..].trim()));
    }
    let bytes = trimmed.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == 0 || idx >= bytes.len() || bytes[idx] != b'.' {
        return None;
    }
    let rest = &trimmed[idx + 1..];
    if !rest.starts_with(' ') {
        return None;
    }
    Some((format!("{}.", &trimmed[..idx]), rest.trim_start()))
}

fn render_wrapped_line_tagged(
    line: &str,
    width: usize,
    style: Style,
    indent_code: bool,
    is_code: bool,
) -> Vec<RenderedMarkdownLine> {
    let prefix = if indent_code { "  " } else { "" };
    let prefix_width = prefix.width();
    let available = width.saturating_sub(prefix_width).max(1);
    // Code blocks must preserve leading whitespace (indentation is semantic).
    // Use hard character-width wrapping instead of word-wrap.
    let wrapped = if indent_code {
        wrap_code_line(line, available)
    } else {
        wrap_text(line, available)
    };
    let mut out = Vec::new();

    let last_index = wrapped.len().saturating_sub(1);
    for (idx, chunk) in wrapped.into_iter().enumerate() {
        let line = if idx == 0 {
            Line::from(vec![Span::raw(prefix), Span::styled(chunk, style)])
        } else {
            Line::from(vec![
                Span::raw(" ".repeat(prefix_width)),
                Span::styled(chunk, style),
            ])
        };
        let copy_separator_after = if idx == last_index {
            CopyLineSeparator::Newline
        } else if is_code {
            CopyLineSeparator::None
        } else {
            CopyLineSeparator::Space
        };
        out.push(RenderedMarkdownLine {
            line,
            is_code,
            copy_prefix_width: if indent_code { prefix_width } else { 0 },
            copy_separator_after,
        });
    }

    out
}

fn render_list_line_tagged(
    bullet: &str,
    text: &str,
    width: usize,
    bullet_style: Style,
    text_style: Style,
) -> Vec<RenderedMarkdownLine> {
    let bullet_prefix = format!("{bullet} ");
    let bullet_width = bullet_prefix.width();
    let available = width.saturating_sub(bullet_width).max(1);
    let wrapped = render_line_with_links_tagged(text, available, text_style, link_style());

    let mut out = Vec::new();
    for (idx, rendered) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            let mut spans = vec![Span::styled(bullet_prefix.clone(), bullet_style)];
            spans.extend(rendered.line.spans);
            out.push(RenderedMarkdownLine {
                line: Line::from(spans),
                is_code: false,
                copy_prefix_width: 0,
                copy_separator_after: rendered.copy_separator_after,
            });
        } else {
            let mut spans = vec![Span::raw(" ".repeat(bullet_width))];
            spans.extend(rendered.line.spans);
            out.push(RenderedMarkdownLine {
                line: Line::from(spans),
                is_code: false,
                copy_prefix_width: bullet_width,
                copy_separator_after: rendered.copy_separator_after,
            });
        }
    }
    out
}

fn render_line_with_links_tagged(
    line: &str,
    width: usize,
    base_style: Style,
    link_style: Style,
) -> Vec<RenderedMarkdownLine> {
    if line.trim().is_empty() {
        return vec![RenderedMarkdownLine {
            line: Line::from(""),
            is_code: false,
            copy_prefix_width: 0,
            copy_separator_after: CopyLineSeparator::Newline,
        }];
    }

    // Flatten inline tokens into (word, style) pairs preserving inter-token spaces.
    let tokens = parse_inline_spans(line, base_style, link_style);
    let mut words: Vec<InlineToken> = Vec::new();
    for token in tokens {
        let mut first = true;
        for part in token.text.split(' ') {
            if !first {
                // The space consumed by split — attach as a plain space word
                // so the wrap loop can decide whether to keep or break it.
                words.push(InlineToken::new(" ".to_string(), token.style, None));
            }
            if !part.is_empty() {
                words.push(InlineToken::new(
                    part.to_string(),
                    token.style,
                    token.link_url.clone(),
                ));
            }
            first = false;
        }
    }

    let mut lines: Vec<RenderedMarkdownLine> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for word in words {
        let ww = word.text.width();
        if word.text == " " {
            // Space: emit only if we're mid-line and it fits; otherwise drop
            // (it's a potential wrap point, not content).
            if !current_spans.is_empty() && current_width < width {
                current_spans.push(Span::raw(" "));
                current_width += 1;
            }
            continue;
        }
        // If the word itself is wider than an entire line, hard-break it at
        // character boundaries so wrapping always makes progress (#1344,
        // #1351). Without this, long URLs / paths / hashes were placed on
        // their own line whole and silently overflowed the right edge of
        // the transcript.
        if ww > width && width > 0 {
            // Flush the in-progress line first.
            if !current_spans.is_empty() {
                push_inline_line(&mut lines, &mut current_spans, CopyLineSeparator::Space);
                current_width = 0;
            }
            // Char-break the word into width-sized chunks. Each full chunk
            // becomes its own line; the final partial chunk continues the
            // current line so the next word can pack onto it.
            let mut chunk = String::new();
            let mut chunk_w = 0usize;
            for ch in word.text.chars() {
                let cw = ch.width().unwrap_or(1);
                if chunk_w + cw > width && chunk_w > 0 {
                    lines.push(RenderedMarkdownLine {
                        line: Line::from(vec![word.span_for(std::mem::take(&mut chunk))]),
                        is_code: false,
                        copy_prefix_width: 0,
                        copy_separator_after: CopyLineSeparator::None,
                    });
                    chunk_w = 0;
                }
                chunk.push(ch);
                chunk_w += cw;
            }
            if !chunk.is_empty() {
                current_spans.push(word.span_for(chunk));
                current_width = chunk_w;
            }
            continue;
        }
        // Wrap before this word if it doesn't fit.
        if current_width > 0 && current_width + ww > width {
            // Trim trailing space span before breaking.
            push_inline_line(&mut lines, &mut current_spans, CopyLineSeparator::Space);
            current_width = 0;
        }
        current_spans.push(word.into_span());
        current_width += ww;
    }

    if !current_spans.is_empty() {
        push_inline_line(&mut lines, &mut current_spans, CopyLineSeparator::Newline);
    } else if let Some(last) = lines.last_mut() {
        last.copy_separator_after = CopyLineSeparator::Newline;
    }
    if lines.is_empty() {
        lines.push(RenderedMarkdownLine {
            line: Line::from(""),
            is_code: false,
            copy_prefix_width: 0,
            copy_separator_after: CopyLineSeparator::Newline,
        });
    }
    lines
}

fn push_inline_line(
    lines: &mut Vec<RenderedMarkdownLine>,
    spans: &mut Vec<Span<'static>>,
    copy_separator_after: CopyLineSeparator,
) {
    if let Some(last) = spans.last()
        && last.content.as_ref() == " "
    {
        spans.pop();
    }
    lines.push(RenderedMarkdownLine {
        line: Line::from(std::mem::take(spans)),
        is_code: false,
        copy_prefix_width: 0,
        copy_separator_after,
    });
}

#[derive(Clone)]
struct InlineToken {
    text: String,
    style: Style,
    link_url: Option<String>,
}

impl InlineToken {
    fn new(text: String, style: Style, link_url: Option<String>) -> Self {
        Self {
            text,
            style,
            link_url,
        }
    }

    fn span_for(&self, text: String) -> Span<'static> {
        let content = match &self.link_url {
            Some(url) if osc8::enabled() => osc8::wrap_link(url, &text),
            _ => text,
        };
        Span::styled(content, self.style)
    }

    fn into_span(self) -> Span<'static> {
        let Self {
            text,
            style,
            link_url,
        } = self;
        let content = match link_url {
            Some(url) if osc8::enabled() => osc8::wrap_link(&url, &text),
            _ => text,
        };
        Span::styled(content, style)
    }
}

/// Parse an entire line into (text, style) segments, handling **bold**,
/// *italic*, `code`, ~~strikethrough~~, `[text](url)` links, and bare URLs.
fn parse_inline_spans(line: &str, base_style: Style, link_style: Style) -> Vec<InlineToken> {
    let bold_style = base_style.add_modifier(Modifier::BOLD);
    let italic_style = base_style.add_modifier(Modifier::ITALIC);
    let code_style = base_style
        .add_modifier(Modifier::ITALIC)
        .bg(palette::SURFACE_ELEVATED);
    let strike_style = base_style.add_modifier(Modifier::CROSSED_OUT);
    let mut out = Vec::new();
    let mut rest = line;

    while !rest.is_empty() {
        // **bold**
        if let Some(end) = rest.strip_prefix("**").and_then(|s| s.find("**")) {
            let inner = &rest[2..2 + end];
            out.push(InlineToken::new(inner.to_string(), bold_style, None));
            rest = &rest[2 + end + 2..];
            continue;
        }
        // __bold__
        if let Some(end) = rest.strip_prefix("__").and_then(|s| s.find("__")) {
            let inner = &rest[2..2 + end];
            out.push(InlineToken::new(inner.to_string(), bold_style, None));
            rest = &rest[2 + end + 2..];
            continue;
        }
        // *italic*
        if rest.starts_with('*')
            && !rest.starts_with("**")
            && let Some(end) = rest[1..].find('*')
        {
            let inner = &rest[1..1 + end];
            let after = &rest[1 + end + 1..];
            // Closing delimiter must not be immediately followed by a
            // letter, digit, or underscore (otherwise it's part of an
            // identifier like `mimofan_tui`, not italic markup).
            if !after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                out.push(InlineToken::new(inner.to_string(), italic_style, None));
                rest = after;
                continue;
            }
        }
        // _italic_
        if rest.starts_with('_')
            && !rest.starts_with("__")
            && let Some(end) = rest[1..].find('_')
        {
            let inner = &rest[1..1 + end];
            let after = &rest[1 + end + 1..];
            // Closing delimiter must not be immediately followed by a
            // letter, digit, or underscore.
            if !after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                out.push(InlineToken::new(inner.to_string(), italic_style, None));
                rest = after;
                continue;
            }
        }
        // `inline code`
        if let Some(end) = rest.strip_prefix('`').and_then(|s| s.find('`')) {
            let inner = &rest[1..1 + end];
            out.push(InlineToken::new(inner.to_string(), code_style, None));
            rest = &rest[1 + end + 1..];
            continue;
        }
        // ~~strikethrough~~
        if let Some(end) = rest.strip_prefix("~~").and_then(|s| s.find("~~")) {
            let inner = &rest[2..2 + end];
            out.push(InlineToken::new(inner.to_string(), strike_style, None));
            rest = &rest[2 + end + 2..];
            continue;
        }
        // [text](url)
        if rest.starts_with('[')
            && let Some(bracket_end) = rest.find(']')
        {
            let text = &rest[1..bracket_end];
            let after_bracket = &rest[bracket_end + 1..];
            if after_bracket.starts_with('(')
                && let Some(paren_end) = after_bracket.find(')')
            {
                let url = &after_bracket[1..paren_end];
                if osc8::enabled() {
                    out.push(InlineToken::new(
                        text.to_string(),
                        link_style,
                        Some(url.to_string()),
                    ));
                } else {
                    out.push(InlineToken::new(
                        format!("{text} ({url})"),
                        link_style,
                        None,
                    ));
                }
                rest = &after_bracket[paren_end + 1..];
                continue;
            }
        }
        // URL: consume until whitespace
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let url = &rest[..end];
            if osc8::enabled() {
                out.push(InlineToken::new(
                    url.to_string(),
                    link_style,
                    Some(url.to_string()),
                ));
            } else {
                out.push(InlineToken::new(url.to_string(), link_style, None));
            }
            rest = &rest[end..];
            continue;
        }
        // Plain text: consume until next marker or URL; always advance at least 1 char.
        let next = find_next_marker(rest).max(rest.chars().next().map_or(1, |c| c.len_utf8()));
        out.push(InlineToken::new(rest[..next].to_string(), base_style, None));
        rest = &rest[next..];
    }
    out
}

/// Find the index of the next inline marker (`**`, `__`, `*`, `_`, `http`)
/// in `s`, or `s.len()` if none found.
fn find_next_marker(s: &str) -> usize {
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        let ch_len = s[i..].chars().next().map_or(1, |c| c.len_utf8());
        let slice = &s[i..];
        if slice.starts_with("**")
            || slice.starts_with("__")
            || slice.starts_with("~~")
            || slice.starts_with('`')
            || slice.starts_with('[')
            || (slice.starts_with('*') && !slice.starts_with("**"))
            || (slice.starts_with('_') && !slice.starts_with("__"))
            || slice.starts_with("http://")
            || slice.starts_with("https://")
        {
            return i;
        }
        i += ch_len;
    }
    s.len()
}

fn is_horizontal_rule(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    (stripped.chars().all(|c| c == '-')
        || stripped.chars().all(|c| c == '*')
        || stripped.chars().all(|c| c == '_'))
        && stripped.len() >= 3
}

/// Parse a markdown table row like `| foo | bar |` into trimmed cell strings.
/// Returns `None` for separator rows (`|---|---|`).
fn parse_table_row(line: &str) -> Option<Vec<String>> {
    if !line.starts_with('|') {
        return None;
    }
    let inner = line.trim_matches('|');
    let cells = split_table_cells(inner);
    // Separator row: every non-empty cell is only dashes/colons/spaces
    if cells
        .iter()
        .all(|c| c.is_empty() || c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '))
    {
        return None;
    }
    Some(cells)
}

fn split_table_cells(inner: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut in_code = false;
    let mut chars = inner.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if matches!(chars.peek(), Some('|')) {
                    current.push('|');
                    let _ = chars.next();
                } else {
                    current.push(ch);
                }
            }
            '`' => {
                in_code = !in_code;
                current.push(ch);
            }
            '|' if !in_code => {
                cells.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    cells.push(current.trim().to_string());
    cells
}

/// Word-wrap a single cell's text into one or more visual lines, each
/// constrained to `col_width` display columns. Whitespace is the preferred
/// break point; words wider than `col_width` are hard-broken at character
/// boundaries so wrapping always makes progress (no infinite loop on URLs
/// or paths). Returns at least one segment.
fn wrap_cell_text(cell: &str, col_width: usize) -> Vec<String> {
    if cell.is_empty() || cell.width() <= col_width {
        return vec![cell.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0usize;

    for word in cell.split_whitespace() {
        let word_w = word.width();
        if current_w == 0 {
            if word_w > col_width {
                push_word_breaking_chars(word, col_width, &mut current, &mut current_w, &mut lines);
            } else {
                current.push_str(word);
                current_w = word_w;
            }
        } else if current_w + 1 + word_w <= col_width {
            current.push(' ');
            current.push_str(word);
            current_w += 1 + word_w;
        } else {
            lines.push(std::mem::take(&mut current));
            current_w = 0;
            if word_w > col_width {
                push_word_breaking_chars(word, col_width, &mut current, &mut current_w, &mut lines);
            } else {
                current.push_str(word);
                current_w = word_w;
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn render_table_row(cells: &[String], width: usize, base_style: Style) -> Vec<Line<'static>> {
    if cells.is_empty() {
        return vec![Line::from("")];
    }
    let col_width = (width.saturating_sub(3 * cells.len() + 1)) / cells.len();
    let col_width = col_width.max(4);
    let sep_style = Style::default().fg(palette::TEXT_DIM);

    // Wrap each cell into one or more visual segments. The row's visual
    // height equals the tallest column. Cells that wrap to fewer segments
    // get blank-padded continuation lines so column separators stay aligned.
    let wrapped: Vec<Vec<String>> = cells.iter().map(|c| wrap_cell_text(c, col_width)).collect();
    let row_height = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(row_height);
    for row in 0..row_height {
        let mut spans: Vec<Span> = vec![Span::styled("│ ".to_string(), sep_style)];
        for (i, cell_segments) in wrapped.iter().enumerate() {
            let segment = cell_segments.get(row).map(String::as_str).unwrap_or("");
            let cell_spans = parse_inline_spans(segment, base_style, link_style());
            let cell_width: usize = cell_spans.iter().map(|token| token.text.width()).sum();
            let pad = col_width.saturating_sub(cell_width);
            for token in cell_spans {
                spans.push(token.into_span());
            }
            spans.push(Span::raw(" ".repeat(pad)));
            if i + 1 < cells.len() {
                spans.push(Span::styled(" │ ".to_string(), sep_style));
            } else {
                spans.push(Span::styled(" │".to_string(), sep_style));
            }
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn table_col_width(num_cols: usize, term_width: usize) -> usize {
    let col_width = (term_width.saturating_sub(3 * num_cols + 1)) / num_cols;
    col_width.max(4)
}

fn render_table_border(
    num_cols: usize,
    col_width: usize,
    sep_style: Style,
    left: &str,
    mid: &str,
    right: &str,
) -> Line<'static> {
    let fill = "\u{2500}".repeat(col_width);
    let mut s = String::new();
    s.push_str(left);
    for i in 0..num_cols {
        s.push_str(&fill);
        if i + 1 < num_cols {
            s.push_str(mid);
        } else {
            s.push_str(right);
        }
    }
    Line::from(Span::styled(s, sep_style))
}

fn render_table_group(blocks: &[Block], width: usize, base_style: Style) -> Vec<Line<'static>> {
    let sep_style = Style::default().fg(palette::TEXT_DIM);

    let num_cols = blocks
        .iter()
        .filter_map(|b| match b {
            Block::TableRow(cells) => Some(cells.len()),
            _ => None,
        })
        .max()
        .unwrap_or(1);

    let col_width = table_col_width(num_cols, width);

    let mut lines = Vec::new();

    // Top border
    lines.push(render_table_border(
        num_cols,
        col_width,
        sep_style,
        "\u{250C}\u{2500}",
        "\u{2500}\u{252C}\u{2500}",
        "\u{2500}\u{2510}",
    ));

    let mid_border = || {
        render_table_border(
            num_cols,
            col_width,
            sep_style,
            "\u{251C}\u{2500}",
            "\u{2500}\u{253C}\u{2500}",
            "\u{2500}\u{2524}",
        )
    };

    for i in 0..blocks.len() {
        match &blocks[i] {
            Block::TableRow(cells) => {
                lines.extend(render_table_row(cells, width, base_style));
                if i + 1 < blocks.len() && matches!(&blocks[i + 1], Block::TableRow(_)) {
                    lines.push(mid_border());
                }
            }
            Block::TableSeparator => {
                lines.push(mid_border());
            }
            _ => {}
        }
    }

    // Bottom border
    lines.push(render_table_border(
        num_cols,
        col_width,
        sep_style,
        "\u{2514}\u{2500}",
        "\u{2500}\u{2534}\u{2500}",
        "\u{2500}\u{2518}",
    ));

    lines
}

fn link_style() -> Style {
    Style::default()
        .fg(palette::WHALE_ACCENT_PRIMARY)
        .add_modifier(Modifier::UNDERLINED)
}

/// Hard-wrap a code line at `width` display columns, preserving all
/// whitespace (including leading indentation). Unlike [`wrap_text`], this
/// does not split on word boundaries — code indentation is semantic.
/// Display-column width of a single character for the purposes of terminal
/// line-wrap calculations.
///
/// `UnicodeWidthChar::width` returns `None` for control characters, which
/// includes `\t`. A tab advances to the next 8-column tab stop, so we model
/// it as 8 columns here (a safe over-estimate that avoids terminal overflow).
/// Other control characters are counted as 1 column.
fn char_display_width(ch: char, col: usize) -> usize {
    match ch {
        '\t' => 8 - (col % 8), // advance to next 8-column tab stop
        _ => ch.width().unwrap_or(1),
    }
}

/// Hard-wrap a code line at `width` display columns, preserving all
/// whitespace (including leading indentation). Unlike [`wrap_text`], this
/// does not split on word boundaries — code indentation is semantic.
fn wrap_code_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 || line.is_empty() {
        return vec![line.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in line.chars() {
        let ch_width = char_display_width(ch, current_width);
        if current_width + ch_width > width && !current.is_empty() {
            chunks.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    chunks.push(current);
    chunks
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = word.width();
        // If this single word is wider than the entire line, hard-break it
        // at character boundaries so wrapping always makes progress
        // (#1344, #1351). Without this, long URLs / paths / hashes overflow
        // the right edge silently.
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            push_word_breaking_chars(word, width, &mut current, &mut current_width, &mut lines);
            continue;
        }
        let additional = if current.is_empty() {
            word_width
        } else {
            word_width + 1
        };
        if current_width + additional > width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
            current_width = word_width;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
    }

    if current.is_empty() {
        lines.push(String::new());
    } else {
        lines.push(current);
    }

    lines
}

/// Push characters from `word` into `current`, flushing to `lines` when the
/// running display width would exceed `width`. Width is computed at the
/// `unicode-width` char level, matching the rest of the rendering pipeline.
/// Used by `wrap_text` and `wrap_cell_text` so a word longer than the
/// allotted width never silently overflows the right edge.
fn push_word_breaking_chars(
    word: &str,
    width: usize,
    current: &mut String,
    current_width: &mut usize,
    lines: &mut Vec<String>,
) {
    for ch in word.chars() {
        let cw = ch.width().unwrap_or(1);
        if *current_width + cw > width && *current_width > 0 {
            lines.push(std::mem::take(current));
            *current_width = 0;
        }
        current.push(ch);
        *current_width += cw;
    }
}

#[cfg(test)]
mod tests {}
