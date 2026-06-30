//! OSC 8 hyperlink emission and stripping.
//!
//! Modern terminals (iTerm2, Terminal.app 13+, Ghostty, Kitty, WezTerm,
//! Alacritty, recent gnome-terminal/konsole) make a substring clickable when
//! it is wrapped in:
//!
//! ```text
//! \x1b]8;;TARGET\x1b\\LABEL\x1b]8;;\x1b\\
//! ```
//!
//! Terminals that don't understand the sequence simply render the visible
//! `LABEL` and ignore the escape. So emitting OSC 8 is a strict UX upgrade for
//! supporting terminals and a no-op for the rest.
//!
//! # Architecture (#3029)
//!
//! The markdown renderer embeds link payloads *in-band* inside `Span::content`
//! via [`wrap_link`]. ratatui's buffer pipeline drops the leading `ESC` byte
//! but paints the rest of the payload one-byte-per-cell, which would corrupt
//! columns. So each render seam calls [`extract_buffer_link_regions`] after
//! `Paragraph::render`: it recovers each link's target + label display
//! columns, blanks the payload cells (no cell ever holds `\x1b` or `]8;;`),
//! and publishes [`LinkRegion`]s to a thread-local. `ColorCompatBackend::draw`
//! then consumes those regions and emits the OSC 8 escapes *out-of-band* —
//! interleaved with the cell stream through the backend's `Write` impl, never
//! inside a buffer cell. The in-band path is the source of link info; the
//! out-of-band path is what reaches the terminal.
//!
//! The clipboard/selection extraction path still strips any residual codes via
//! [`strip_into`] / [`strip_ansi_into`] as a defense-in-depth.

use std::sync::atomic::{AtomicBool, Ordering};

const OSC8_PREFIX: &str = "\x1b]8;;";
const OSC8_TERMINATOR: &str = "\x1b\\";
const OSC8_CLOSE: &str = "\x1b]8;;\x1b\\";

/// A contiguous run of cells on one terminal row that share a hyperlink target.
#[derive(Debug, Clone)]
pub struct LinkRegion {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub target: String,
}

/// Write an OSC 8 hyperlink open sequence for `target` to `w`.
pub fn write_osc8_open(w: &mut impl std::io::Write, target: &str) -> std::io::Result<()> {
    w.write_all(OSC8_PREFIX.as_bytes())?;
    w.write_all(target.as_bytes())?;
    w.write_all(OSC8_TERMINATOR.as_bytes())
}

/// Write an OSC 8 hyperlink close sequence to `w`.
pub fn write_osc8_close(w: &mut impl std::io::Write) -> std::io::Result<()> {
    w.write_all(OSC8_CLOSE.as_bytes())
}

/// Process-wide enable flag. Set once at app init from `[tui] osc8_links`
/// (when present); otherwise defaults to on for macOS/Linux and off for
/// Windows legacy consoles (see `ui.rs`'s `osc8_default_on`). Read by the
/// renderer to gate out-of-band OSC 8 emission.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Set the process-wide OSC 8 enable flag. Intended to be called once at
/// startup; subsequent calls take effect immediately.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether OSC 8 hyperlink emission is currently enabled.
#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

// --- Thread-local link region accumulator (#3029) ---

use std::cell::RefCell;

thread_local! {
    /// Link regions collected during the current render frame.
    /// Populated by the render closure after scanning the ratatui buffer;
    /// consumed and cleared by `ColorCompatBackend::draw()`.
    pub static FRAME_LINKS: RefCell<Vec<LinkRegion>> = const { RefCell::new(Vec::new()) };
}

/// Replace the thread-local frame link buffer with `links`.
pub fn set_frame_links(links: Vec<LinkRegion>) {
    FRAME_LINKS.with(|cell| {
        *cell.borrow_mut() = links;
    });
}

/// Append `links` to the thread-local frame link buffer. Used when more than
/// one widget renders link-bearing content into the same frame (e.g. the main
/// transcript and the live-transcript overlay): each seam appends rather than
/// replacing, so all regions reach `ColorCompatBackend::draw`.
pub fn append_frame_links(links: Vec<LinkRegion>) {
    FRAME_LINKS.with(|cell| cell.borrow_mut().extend(links));
}

/// Take the thread-local frame links, leaving an empty vec behind.
pub fn take_frame_links() -> Vec<LinkRegion> {
    FRAME_LINKS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

// --- In-band payload extraction (#3029) ---
//
// The markdown renderer embeds OSC 8 hyperlinks *in-band* inside `Span`
// content via [`wrap_link`]. ratatui's buffer pipeline drops the leading
// `ESC` byte but paints every other byte of the payload into its own cell,
// which drifts columns and corrupts the visible glyph stream. Rather than
// thread structured link metadata through the whole render pipeline, we scan
// the rendered `Buffer` after each `Paragraph::render` and:
//
//   1. recover each link's target + the display-column span of its label, and
//   2. blank the payload cells (the `]8;;`, target, and terminators), leaving
//      only the clean label behind.
//
// The recovered [`LinkRegion`]s are handed to [`set_frame_links`] /
// [`append_frame_links`]; `ColorCompatBackend::draw` consumes them and emits
// the OSC 8 escapes *out-of-band* through the backend's `Write` impl, so no
// payload byte ever reaches a buffer cell. This satisfies the #3029
// acceptance criterion ("no Buffer cell contains `\x1b` or `]8;;`") by
// construction.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// The four cells of the OSC 8 open prefix `ESC ] 8 ; ;` after ratatui strips
/// the leading ESC: `]`, `8`, `;`, `;`.
const OPEN_CELLS: [char; 4] = [']', '8', ';', ';'];

/// Scan `area` of `buf` for in-band OSC 8 link payloads, blank their payload
/// cells, and return one [`LinkRegion`] per recovered link (over the label's
/// display columns, in absolute buffer coordinates).
///
/// A complete payload in the buffer (ESC already stripped by ratatui) looks
/// like `]8;;TARGET\LABEL]8;;\` — four open cells, target bytes, a `\`
/// terminator, the visible label, then the four-cell close `]8;;\`. If the
/// close is missing (e.g. the payload was truncated by wrapping), the whole
/// run is treated as corruption: cells are blanked but no region is emitted,
/// since a half-link is worse than no link.
///
/// `row`/`col_start`/`col_end` are absolute buffer coordinates (they include
/// `area.x`/`area.y`), matching what `ColorCompatBackend::draw` tests against.
#[must_use]
pub fn extract_buffer_link_regions(buf: &mut Buffer, area: Rect) -> Vec<LinkRegion> {
    let mut regions = Vec::new();
    let x_start = area.x;
    let x_end = area.x.saturating_add(area.width);
    let y_start = area.y;
    let y_end = area.y.saturating_add(area.height);

    for y in y_start..y_end {
        let mut x = x_start;
        while x < x_end {
            // Look for the open prefix `]8;;` at the current column.
            if matches_open(buf, x, y, x_end) {
                let payload_start = x;
                // Skip the 4 open cells, then consume the target up to `\`.
                let mut scan = x + OPEN_CELLS.len() as u16;
                let mut target = String::new();
                let mut found_target_term = false;
                while scan < x_end {
                    let ch = cell_char(buf, scan, y);
                    scan += 1;
                    if ch == '\\' {
                        found_target_term = true;
                        break;
                    }
                    target.push(ch);
                }
                if !found_target_term {
                    // Unterminated payload: blank what we can prove is payload
                    // (the open prefix) and bail on this run — the rest may be
                    // legitimate content we must not destroy.
                    blank_cells(buf, payload_start..payload_start + 4, y);
                    x = scan;
                    continue;
                }
                let label_start = scan;
                // Consume label cells until the close prefix `]8;;\`. `scan`
                // walks one cell at a time; when the next four cells spell
                // `]8;;` and the fifth is `\`, the label ends just before them.
                let mut found_close = false;
                while scan + 4 < x_end {
                    if matches_open(buf, scan, y, x_end) && cell_char(buf, scan + 4, y) == '\\' {
                        found_close = true;
                        break;
                    }
                    scan += 1;
                }
                // `scan` is now either at the close prefix (found) or past the
                // row end (not found); in both cases the label occupies
                // `label_start..scan` (exclusive end).
                if !found_close {
                    // No close within the row: blank the open+target+term and
                    // the partial label, emit no region.
                    blank_cells(buf, payload_start..scan, y);
                    x = scan;
                    continue;
                }
                let close_start = scan;
                let close_end = scan + (OPEN_CELLS.len() as u16) + 1; // `]8;;` + `\`
                // Record the region over the label's columns. LinkRegion uses
                // inclusive end coordinates, matching ColorCompatBackend's
                // `x >= col_start && x <= col_end` test. Skip empty labels.
                if scan > label_start {
                    regions.push(LinkRegion {
                        row: y,
                        col_start: label_start,
                        col_end: scan - 1,
                        target,
                    });
                }
                // Blank the payload cells AROUND the label, never the label
                // itself: the open prefix + target + first `\`, then the close
                // `]8;;\`. The label cells in `label_start..scan` are left
                // intact so the visible glyph stream is unchanged.
                blank_cells(buf, payload_start..label_start, y);
                blank_cells(buf, close_start..close_end, y);
                x = close_end;
                continue;
            }
            x += 1;
        }
    }
    regions
}

/// Whether the four cells starting at `(x, y)` spell the OSC 8 open prefix
/// `]8;;` (clamped to `x_end`).
fn matches_open(buf: &Buffer, x: u16, y: u16, x_end: u16) -> bool {
    if x.saturating_add(OPEN_CELLS.len() as u16) > x_end {
        return false;
    }
    OPEN_CELLS
        .iter()
        .enumerate()
        .all(|(i, want)| cell_char(buf, x + i as u16, y) == *want)
}

/// First char of the symbol at `(x, y)` (payload bytes are ASCII, so the cell
/// symbol is a single char). Returns `'\0'` for empty cells so they never
/// falsely match a payload char.
fn cell_char(buf: &Buffer, x: u16, y: u16) -> char {
    let sym = buf[(x, y)].symbol();
    sym.chars().next().unwrap_or('\0')
}

/// Reset the cells in `cols` (relative to absolute `x`) on row `y` to a blank
/// space, clearing any payload bytes.
fn blank_cells(buf: &mut Buffer, cols: std::ops::Range<u16>, y: u16) {
    for x in cols {
        if let Some(cell) = buf.cell_mut(ratatui::layout::Position { x, y }) {
            cell.set_symbol(" ");
        }
    }
}

/// Wrap `label` so it links to `target` in OSC 8-aware terminals. The returned
/// string contains the full `\x1b]8;;TARGET\x1b\LABEL\x1b]8;;\x1b\` payload.
///
/// Does **not** check [`enabled()`]; callers wanting the runtime gate should
/// branch on it before calling this. That keeps the helper test-friendly.
#[must_use]
pub fn wrap_link(target: &str, label: &str) -> String {
    let mut out = String::with_capacity(target.len() + label.len() + 12);
    out.push_str(OSC8_PREFIX);
    out.push_str(target);
    out.push_str(OSC8_TERMINATOR);
    out.push_str(label);
    out.push_str(OSC8_PREFIX);
    out.push_str(OSC8_TERMINATOR);
    out
}

/// Strip every ANSI escape sequence from `s` into `out`, preserving only the
/// visible characters. ratatui's buffer drops the leading `ESC` byte but
/// happily paints every other byte of an escape (`[`, `0`, `;`, `m`, OSC
/// payloads, etc.) into a buffer cell, drifting columns. Tool stdout that
/// includes ANSI (e.g. `gh`/`git` with color forced on, anything run through
/// a PTY) must be sanitized before it enters the transcript.
///
/// Handles CSI (`ESC [ … final`), OSC (`ESC ] … BEL` or `ESC \`), DCS, SOS,
/// PM, APC, and standalone two-byte ESC sequences. OSC 8 hyperlink wrappers
/// (`ESC ] 8 ; … BEL` / `ESC \`) are stripped along with the rest.
pub fn strip_ansi_into(s: &str, out: &mut String) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            match next {
                // CSI: ESC [ ... <final byte 0x40..=0x7E>
                b'[' => {
                    let mut j = i + 2;
                    while j < bytes.len() {
                        let b = bytes[j];
                        if (0x40..=0x7e).contains(&b) {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                    continue;
                }
                // OSC / DCS / SOS / PM / APC: ESC ] | P | X | ^ | _ ... ST(ESC \) or BEL
                b']' | b'P' | b'X' | b'^' | b'_' => {
                    let mut j = i + 2;
                    while j < bytes.len() {
                        if bytes[j] == 0x07 {
                            j += 1;
                            break;
                        }
                        if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                    continue;
                }
                // Standalone two-byte ESC sequence (RIS, charset selection, etc.)
                _ => {
                    i += 2;
                    continue;
                }
            }
        }
        // Strip lone control bytes that ratatui would otherwise drop (and which
        // mean nothing in transcript output) but keep \n, \r, \t as legitimate
        // formatting.
        let b = bytes[i];
        if b < 0x80 {
            if b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t' {
                i += 1;
                continue;
            }
            out.push(b as char);
            i += 1;
        } else {
            // UTF-8 multi-byte sequence: copy the whole code point intact.
            // Pushing `b as char` would mis-decode it as Latin-1 and mangle
            // non-ASCII text (CJK, accented Latin, emoji, …).
            let len = utf8_seq_len(b);
            let end = (i + len).min(bytes.len());
            if let Ok(chunk) = std::str::from_utf8(&bytes[i..end]) {
                out.push_str(chunk);
            }
            i = end;
        }
    }
}

/// Length in bytes of the UTF-8 sequence that starts with `lead`. Falls back
/// to `1` for continuation bytes / invalid leads so callers always make
/// forward progress.
fn utf8_seq_len(lead: u8) -> usize {
    if lead < 0xc0 {
        1
    } else if lead < 0xe0 {
        2
    } else if lead < 0xf0 {
        3
    } else {
        4
    }
}

/// Strip OSC 8 escape sequences from `s` into `out`, preserving the visible
/// label text. Other escapes (color, style) pass through untouched. The
/// implementation handles both the standard `ESC \` and the lone `BEL`
/// terminators that some emitters use.
pub fn strip_into(s: &str, out: &mut String) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Look for the OSC 8 prefix `ESC ] 8 ;`
        if i + 4 <= bytes.len()
            && bytes[i] == 0x1b
            && bytes[i + 1] == b']'
            && bytes[i + 2] == b'8'
            && bytes[i + 3] == b';'
        {
            // Skip until the string terminator (ESC \) or BEL.
            let mut j = i + 4;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    j += 1;
                    break;
                }
                if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                    j += 2;
                    break;
                }
                j += 1;
            }
            i = j;
            continue;
        }
        let b = bytes[i];
        if b < 0x80 {
            out.push(b as char);
            i += 1;
        } else {
            let len = utf8_seq_len(b);
            let end = (i + len).min(bytes.len());
            if let Ok(chunk) = std::str::from_utf8(&bytes[i..end]) {
                out.push_str(chunk);
            }
            i = end;
        }
    }
}

#[cfg(test)]
mod tests {}
