//! Editor correctness logic, decoupled from the filesystem and tool layer.
//!
//! This crate holds the pure, side-effect-free parts of `edit_file` so they
//! can be unit-tested without a real `ToolContext`:
//!
//! * anchor (content-hash) line lookup,
//! * byte-range → 1-based line-span mapping,
//! * fuzzy matching (leading-whitespace and typographic-punctuation),
//! * the read-before-write guarantee, expressed as an injectable
//!   [`ReadState`] trait so the model's "read first, then edit" guard survives
//!   being pulled out of `ToolContext`.

use std::hash::{Hash, Hasher};
use std::path::Path;

// ───────────────────────────── byte / line helpers ─────────────────────────

/// Map a byte range within `contents` to the 1-based inclusive line numbers
/// it spans, so read-coverage can be checked against what `read_file` showed.
///
/// Mirrors the historical implementation in `crates/tui/src/tools/file.rs`.
pub fn line_span_for_byte_range(contents: &str, start: usize, end: usize) -> (usize, usize) {
    let start = start.min(contents.len());
    let end = end.clamp(start, contents.len());
    let first = contents[..start].matches('\n').count() + 1;
    // A range ending exactly at a newline covers only the lines before it.
    let inner = contents[start..end].trim_end_matches('\n');
    let last = first + inner.matches('\n').count();
    (first, last)
}

/// Byte ranges of every non-overlapping occurrence of `needle` in `haystack`.
pub fn match_byte_ranges(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    haystack
        .match_indices(needle)
        .map(|(idx, m)| (idx, idx + m.len()))
        .collect()
}

/// Compute a short content hash for a line (6 hex chars).
/// Based on trimmed line content (without leading whitespace) for stability
/// across indentation changes.
pub fn line_content_hash(line: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.trim_start().hash(&mut hasher);
    format!("{:06x}", hasher.finish() & 0xFFFFFF)
}

/// Find a line by its content anchor hash.
/// Returns `(line_start_byte, line_end_byte)` including the newline.
/// The search is performed on trimmed content (without leading whitespace).
pub fn find_line_by_anchor(contents: &str, anchor: &str) -> Option<(usize, usize)> {
    let mut byte_pos = 0;
    for line in contents.lines() {
        let line_len = line.len();
        let line_end = byte_pos + line_len;
        let hash = line_content_hash(line);
        if hash == anchor {
            // Include the newline in the range if present
            let end = if line_end < contents.len() && contents.as_bytes()[line_end] == b'\n' {
                line_end + 1
            } else {
                line_end
            };
            return Some((byte_pos, end));
        }
        // Skip the newline
        byte_pos = if line_end < contents.len() {
            line_end + 1
        } else {
            line_end
        };
    }
    None
}

/// Find all lines matching a content anchor hash.
pub fn find_all_lines_by_anchor(contents: &str, anchor: &str) -> Vec<(usize, usize)> {
    let mut results = Vec::new();
    let mut byte_pos = 0;
    for line in contents.lines() {
        let line_len = line.len();
        let line_end = byte_pos + line_len;
        let hash = line_content_hash(line);
        if hash == anchor {
            let end = if line_end < contents.len() && contents.as_bytes()[line_end] == b'\n' {
                line_end + 1
            } else {
                line_end
            };
            results.push((byte_pos, end));
        }
        byte_pos = if line_end < contents.len() {
            line_end + 1
        } else {
            line_end
        };
    }
    results
}

// ───────────────────────────── fuzzy matching ──────────────────────────────

/// Strip leading per-line whitespace from `input`, returning the normalized
/// string plus a byte-map sized to `normalized.len()` whose i-th entry is the
/// original byte offset of the character that produced normalized byte i.
fn strip_line_leading_whitespace_with_map(input: &str) -> (String, Vec<usize>) {
    let mut normalized = String::with_capacity(input.len());
    let mut byte_map = Vec::with_capacity(input.len());
    let mut at_line_start = true;
    for (idx, ch) in input.char_indices() {
        if at_line_start && matches!(ch, ' ' | '\t') {
            continue;
        }
        normalized.push(ch);
        for _ in 0..ch.len_utf8() {
            byte_map.push(idx);
        }
        at_line_start = ch == '\n';
    }
    (normalized, byte_map)
}

fn line_start_before(input: &str, idx: usize) -> usize {
    input[..idx]
        .rfind('\n')
        .map_or(0, |newline| newline.saturating_add(1))
}

/// Find `search` inside `contents` after stripping leading per-line whitespace
/// in both. Tolerates indentation drift between the model's remembered copy
/// and the file on disk.
pub fn leading_whitespace_fuzzy_matches(contents: &str, search: &str) -> Vec<(usize, usize)> {
    let (normalized_contents, byte_map) = strip_line_leading_whitespace_with_map(contents);
    let (normalized_search, _) = strip_line_leading_whitespace_with_map(search);
    if normalized_search.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut cursor = 0;
    while let Some(rel_idx) = normalized_contents[cursor..].find(&normalized_search) {
        let norm_start = cursor + rel_idx;
        let norm_end = norm_start + normalized_search.len();
        let Some(&mapped_start) = byte_map.get(norm_start) else {
            break;
        };
        // Use the actual match start position, expanding to line start only
        // when the match begins at a line boundary in the normalized text.
        // This prevents destroying preceding text on the same line when
        // the match starts mid-line after whitespace stripping.
        let original_start =
            if norm_start == 0 || normalized_contents.as_bytes()[norm_start - 1] == b'\n' {
                // Match starts at a line boundary — use line start for full-line replacement.
                line_start_before(contents, mapped_start)
            } else {
                // Match starts mid-line — use the exact mapped position.
                mapped_start
            };
        let original_end = byte_map.get(norm_end).copied().unwrap_or(contents.len());
        matches.push((original_start, original_end));
        cursor = norm_start.saturating_add(1);
    }
    matches
}

/// Normalize typographic punctuation to its ASCII counterpart:
///
/// * `"` `"` / U+201C U+201D → `"`
/// * `'` `'` / U+2018 U+2019 → `'`
/// * `–` `—` / U+2013 U+2014 → `-`
/// * U+00A0 (non-breaking space) → ASCII space
///
/// Returns the normalized string plus a byte-map sized to
/// `normalized.len()` whose i-th entry is the original byte offset of
/// the character that produced normalized byte i. Used to recover the
/// original-byte range after finding a match in normalized space.
fn punctuation_normalized_with_map(input: &str) -> (String, Vec<usize>) {
    let mut normalized = String::with_capacity(input.len());
    let mut byte_map = Vec::with_capacity(input.len());
    for (idx, ch) in input.char_indices() {
        let replacement: Option<char> = match ch {
            '\u{201C}' | '\u{201D}' => Some('"'),
            '\u{2018}' | '\u{2019}' => Some('\''),
            '\u{2013}' | '\u{2014}' => Some('-'),
            '\u{00A0}' => Some(' '),
            _ => None,
        };
        let written = replacement.unwrap_or(ch);
        normalized.push(written);
        for _ in 0..written.len_utf8() {
            byte_map.push(idx);
        }
    }
    (normalized, byte_map)
}

/// Try to find `search` inside `contents` after normalizing typographic
/// punctuation in both. Catches the copy-paste failure mode where a
/// browser, word processor, or chat client silently converted ASCII
/// quotes/dashes to their Unicode "pretty" forms.
pub fn punctuation_normalized_matches(contents: &str, search: &str) -> Vec<(usize, usize)> {
    let (norm_contents, byte_map) = punctuation_normalized_with_map(contents);
    let (norm_search, _) = punctuation_normalized_with_map(search);
    if norm_search.is_empty() {
        return Vec::new();
    }
    // If normalization didn't change anything, the exact-match pass
    // already considered this case — skip to avoid double-reporting.
    if norm_contents == contents && norm_search == search {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut cursor = 0;
    while let Some(rel_idx) = norm_contents[cursor..].find(&norm_search) {
        let norm_start = cursor + rel_idx;
        let norm_end = norm_start + norm_search.len();
        let Some(&original_start) = byte_map.get(norm_start) else {
            break;
        };
        let original_end = byte_map.get(norm_end).copied().unwrap_or(contents.len());
        matches.push((original_start, original_end));
        cursor = norm_start.saturating_add(1);
    }
    matches
}

// ─────────────────────── read-before-write (injected) ──────────────────────

/// How a read-before-write check resolved. The tool layer turns these into the
/// appropriate [`ToolError`](crate::tools::spec::ToolError) while the pure
/// decision lives here, free of any filesystem or `ToolContext` dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCheck {
    /// The edit target has been read and is still fresh — proceed.
    Ok,
    /// The file was never read in this session.
    NeverRead,
    /// The file changed on disk since the last read.
    Stale,
    /// The file's current state could not be inspected to compare.
    Unverifiable,
    /// The specific lines being edited were never observed.
    UnreadLines,
}

impl ReadCheck {
    /// True only for the pass case; every other variant is a rejection.
    pub fn is_ok(self) -> bool {
        matches!(self, ReadCheck::Ok)
    }
}

/// Identity of a file at a point in time, used to detect staleness.
///
/// The bytes hash is authoritative; length and mtime only break ties when the
/// hash is unavailable. Kept free of `SystemTime`/`sha2` so the crate has zero
/// external dependencies — callers fingerprint files however they like and
/// pass the result in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileIdentity {
    /// Length in bytes.
    pub len: u64,
    /// Last-modified time (epoch seconds), or `None` when unknown.
    pub modified: Option<u64>,
    /// SHA-256 of the file bytes (hex), or `None` when the file could not be
    /// hashed.
    pub content_hash: Option<String>,
}

/// Injectable view of "what has this session observed?" so the read-before-
/// write guarantee can be unit-tested without a real `ToolContext`.
///
/// `ToolContext` in `crates/tui` implements this trait over its shared
/// `FileReadTracker`; tests implement it over a plain `HashMap`.
pub trait ReadState {
    /// Current on-disk identity of `path`, or `None` if the file does not
    /// exist / cannot be inspected. Used to detect staleness.
    fn current_identity(&self, path: &Path) -> Option<FileIdentity>;

    /// Identity captured when the session last read `path`, if any.
    fn prior_identity(&self, path: &Path) -> Option<FileIdentity>;

    /// Whether the 1-based inclusive line range `start..=end` of `path` has
    /// been observed by a prior read. `None` (whole-file read) always yields
    /// `true`. Absence is handled by the freshness check, not here.
    fn covers(&self, path: &Path, start: usize, end: usize) -> bool;
}

/// Require a successful, still-fresh read of `path` before a narrow in-place
/// edit. Pure decision: callers convert the result into an error.
pub fn require_fresh_read(state: &dyn ReadState, path: &Path) -> ReadCheck {
    let Some(prior) = state.prior_identity(path) else {
        return ReadCheck::NeverRead;
    };
    let Some(current) = state.current_identity(path) else {
        return ReadCheck::Unverifiable;
    };
    if current != prior {
        return ReadCheck::Stale;
    }
    ReadCheck::Ok
}

/// Require that the lines `start..=end` (1-based, inclusive) of `path` were
/// actually observed by a prior read. Callers must have already passed
/// [`require_fresh_read`]. Absence of a prior read is reported as `Ok` so it
/// is not double-reported (the freshness check owns "never read").
pub fn require_read_coverage(
    state: &dyn ReadState,
    path: &Path,
    start: usize,
    end: usize,
) -> ReadCheck {
    let Some(_prior) = state.prior_identity(path) else {
        return ReadCheck::Ok;
    };
    if state.covers(path, start, end) {
        ReadCheck::Ok
    } else {
        ReadCheck::UnreadLines
    }
}

// ─────────────────────────────────── tests ─────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Minimal in-memory [`ReadState`]: a single recorded identity and an
    /// optional set of covered line ranges (`None` = whole file).
    #[derive(Default)]
    struct MemReadState {
        prior: HashMap<String, Option<FileIdentity>>,
        covered: HashMap<String, Option<Vec<(usize, usize)>>>,
        // Identity the "current" on-disk check will return.
        current: HashMap<String, Option<FileIdentity>>,
    }

    impl MemReadState {
        fn set_read(&mut self, path: &str, identity: FileIdentity, ranges: Option<Vec<(usize, usize)>>) {
            self.prior.insert(path.to_string(), Some(identity.clone()));
            self.current.insert(path.to_string(), Some(identity));
            self.covered.insert(path.to_string(), ranges);
        }
    }

    impl ReadState for MemReadState {
        fn current_identity(&self, path: &Path) -> Option<FileIdentity> {
            self.current.get(&path.to_string_lossy().into_owned()).cloned().flatten()
        }
        fn prior_identity(&self, path: &Path) -> Option<FileIdentity> {
            self.prior.get(&path.to_string_lossy().into_owned()).cloned().flatten()
        }
        fn covers(&self, path: &Path, start: usize, end: usize) -> bool {
            match self.covered.get(&path.to_string_lossy().into_owned()).cloned().flatten() {
                None => true, // whole-file read
                Some(ranges) => {
                    let mut cursor = start;
                    for (rs, re) in &ranges {
                        if *rs > cursor {
                            return false;
                        }
                        if *re >= cursor {
                            cursor = re + 1;
                        }
                        if cursor > end {
                            return true;
                        }
                    }
                    cursor > end
                }
            }
        }
    }

    fn id(len: u64) -> FileIdentity {
        FileIdentity {
            len,
            modified: Some(1000),
            content_hash: Some(format!("h{len}")),
        }
    }

    // ── anchor lookup ──

    #[test]
    fn find_line_by_anchor_hit() {
        // Anchor hash is computed from trimmed line content.
        let line = "    let x = 1;";
        let anchor = line_content_hash(line);
        let contents = format!("a\n{line}\nb\n");
        let found = find_line_by_anchor(&contents, &anchor);
        assert!(found.is_some());
        let (s, e) = found.unwrap();
        assert_eq!(&contents[s..e], "    let x = 1;\n");
    }

    #[test]
    fn find_line_by_anchor_miss() {
        let contents = "a\nb\nc\n";
        assert!(find_line_by_anchor(&contents, "ffffff").is_none());
    }

    #[test]
    fn find_all_lines_by_anchor_multiple() {
        let anchor = line_content_hash("same line");
        let contents = "same line\nother\nsame line\n";
        let found = find_all_lines_by_anchor(&contents, &anchor);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn find_all_lines_by_anchor_none() {
        let contents = "a\nb\n";
        assert!(find_all_lines_by_anchor(&contents, "000000").is_empty());
    }

    // ── byte range → line span ──

    #[test]
    fn line_span_single_line() {
        // "b\n" is bytes 2..4; line 2 spans a single line.
        let contents = "a\nb\nc\n";
        assert_eq!(line_span_for_byte_range(&contents, 2, 4), (2, 2));
    }

    #[test]
    fn line_span_range_ending_at_newline() {
        // Range 0..2 ("a\n") covers only line 1, not line 2.
        let contents = "a\nb\nc\n";
        assert_eq!(line_span_for_byte_range(&contents, 0, 2), (1, 1));
    }

    #[test]
    fn line_span_multi_line() {
        // Bytes covering "b\nc" → lines 2..3.
        let contents = "a\nb\nc\n";
        assert_eq!(line_span_for_byte_range(&contents, 2, 5), (2, 3));
    }

    #[test]
    fn line_span_clamps_oob() {
        let contents = "a\nb\nc\n";
        assert_eq!(line_span_for_byte_range(&contents, 100, 200), (4, 4));
    }

    // ── fuzzy matching ──

    #[test]
    fn fuzzy_indentation_match() {
        let contents = "    foo()\n    bar()\n";
        let search = "foo()";
        let m = leading_whitespace_fuzzy_matches(&contents, search);
        assert_eq!(m.len(), 1);
        let (s, e) = m[0];
        assert_eq!(&contents[s..e], "    foo()");
    }

    #[test]
    fn fuzzy_punctuation_match() {
        let contents = "say “hi” to me\n";
        let search = "say \"hi\" to me";
        let m = punctuation_normalized_matches(&contents, search);
        assert_eq!(m.len(), 1);
        let (s, e) = m[0];
        assert_eq!(&contents[s..e], "say “hi” to me");
    }

    #[test]
    fn fuzzy_punctuation_skips_when_unchanged() {
        let contents = "say \"hi\" to me\n";
        let search = "say \"hi\" to me";
        // Exact match already handled this case; must not double-report.
        assert!(punctuation_normalized_matches(&contents, search).is_empty());
    }

    // ── read-before-write coverage ──

    #[test]
    fn fresh_read_rejects_unread() {
        let state = MemReadState::default();
        let path = Path::new("main.rs");
        assert_eq!(require_fresh_read(&state, path), ReadCheck::NeverRead);
    }

    #[test]
    fn fresh_read_rejects_stale() {
        let mut state = MemReadState::default();
        let path = Path::new("main.rs");
        state.set_read("main.rs", id(10), None);
        // Disk content changed.
        state.current.insert("main.rs".to_string(), Some(id(20)));
        assert_eq!(require_fresh_read(&state, path), ReadCheck::Stale);
    }

    #[test]
    fn fresh_read_passes_when_fresh() {
        let mut state = MemReadState::default();
        let path = Path::new("main.rs");
        state.set_read("main.rs", id(10), None);
        assert_eq!(require_fresh_read(&state, path), ReadCheck::Ok);
    }

    #[test]
    fn coverage_rejects_unread_lines() {
        let mut state = MemReadState::default();
        let path = Path::new("main.rs");
        // Whole-file read grants full coverage via None... use a range here.
        state.set_read("main.rs", id(10), Some(vec![(1, 5)]));
        // Editing line 8 which was never read.
        assert_eq!(require_read_coverage(&state, path, 8, 8), ReadCheck::UnreadLines);
    }

    #[test]
    fn coverage_passes_within_range() {
        let mut state = MemReadState::default();
        let path = Path::new("main.rs");
        state.set_read("main.rs", id(10), Some(vec![(1, 5)]));
        assert_eq!(require_read_coverage(&state, path, 2, 4), ReadCheck::Ok);
    }

    #[test]
    fn coverage_passes_whole_file_read() {
        let mut state = MemReadState::default();
        let path = Path::new("main.rs");
        state.set_read("main.rs", id(10), None);
        // Even unread line numbers pass with a whole-file snapshot.
        assert_eq!(require_read_coverage(&state, path, 800, 800), ReadCheck::Ok);
    }

    #[test]
    fn coverage_ok_when_never_read() {
        // Absence is owned by the freshness check; do not double-report.
        let state = MemReadState::default();
        let path = Path::new("main.rs");
        assert_eq!(require_read_coverage(&state, path, 1, 1), ReadCheck::Ok);
    }
}
