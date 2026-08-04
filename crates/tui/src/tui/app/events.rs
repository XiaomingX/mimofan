//! Event handling and input processing for the TUI application.

use super::helpers::char_count;

/// Strip raw mouse report runs from input text.
///
/// This filters out SGR mouse reports and multi-terminator burst shapes
/// that crossterm sometimes delivers as `Char(c)` keystrokes when its
/// event reader is interrupted mid-sequence during dense streaming output.
pub(crate) fn strip_raw_mouse_report_runs(input: &str, cursor: usize) -> Option<(String, usize)> {
    // First pass: strip the well-defined control-sequence fragment
    // shapes that crossterm sometimes hands us as `Char(c)` keystrokes
    // when its event reader is interrupted mid-sequence during dense
    // streaming output (#1915). This covers OSC 8 hyperlink fragments
    // (`]8;;URL`, including the closing `]8;;`) and Kitty keyboard
    // protocol fragments (`[?…u`, `[>…u`, `[?u`).
    let (after_fragments, after_fragments_cursor, fragments_changed) =
        strip_control_sequence_fragments(input, cursor);

    // Second pass: the existing run-based filter handles SGR mouse
    // reports (`[<35;44;18M`) and the multi-terminator burst shape
    // (`5;46;18M;48;18M`) introduced in e63a4ba4a. It operates on a
    // narrow char set so it can't be confused with user-typed text.
    let chars: Vec<char> = after_fragments.chars().collect();
    let mut output = String::with_capacity(after_fragments.len());
    let mut new_cursor = 0usize;
    let mut changed = fragments_changed;
    let mut index = 0usize;

    while index < chars.len() {
        if is_raw_mouse_report_run_char(chars[index]) {
            let start = index;
            while index < chars.len() && is_raw_mouse_report_run_char(chars[index]) {
                index += 1;
            }
            let run = &chars[start..index];
            if let Some(keep) = raw_mouse_report_keep_mask(run) {
                changed = true;
                for (offset, ch) in run.iter().copied().enumerate() {
                    if !keep[offset] {
                        continue;
                    }
                    if start + offset < cursor {
                        new_cursor += 1;
                    }
                    output.push(ch);
                }
                continue;
            }
            for (offset, ch) in run.iter().copied().enumerate() {
                if start + offset < after_fragments_cursor {
                    new_cursor += 1;
                }
                output.push(ch);
            }
            continue;
        }

        if index < after_fragments_cursor {
            new_cursor += 1;
        }
        output.push(chars[index]);
        index += 1;
    }

    changed.then(|| {
        let cursor = new_cursor.min(char_count(&output));
        (output, cursor)
    })
}

fn is_raw_mouse_report_run_char(ch: char) -> bool {
    matches!(ch, '\x1b' | '[' | '<' | ';' | ':' | 'M' | 'm') || ch.is_ascii_digit()
}

fn looks_like_raw_mouse_report_run(run: &[char]) -> bool {
    if run.len() < 5 {
        return false;
    }
    let has_separator = run.iter().any(|ch| matches!(ch, ';' | ':'));
    let terminators = run.iter().filter(|ch| matches!(ch, 'M' | 'm')).count();
    if !has_separator || terminators == 0 {
        return false;
    }
    has_sgr_mouse_marker(run) || terminators >= 2
}

fn has_sgr_mouse_marker(run: &[char]) -> bool {
    run.windows(2).any(|window| window == ['[', '<'])
}

fn raw_mouse_report_keep_mask(run: &[char]) -> Option<Vec<bool>> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut index = 0usize;

    while index < run.len() {
        let (start, body_start) = if run[index] == '\x1b'
            && run.get(index + 1) == Some(&'[')
            && run.get(index + 2) == Some(&'<')
        {
            (index, index + 3)
        } else if run[index] == '[' && run.get(index + 1) == Some(&'<') {
            (index, index + 2)
        } else {
            index += 1;
            continue;
        };

        let mut end = body_start;
        let mut has_digit = false;
        let mut has_separator = false;
        let mut matched = false;
        while end < run.len() {
            match run[end] {
                '0'..='9' => {
                    has_digit = true;
                    end += 1;
                }
                ';' | ':' => {
                    has_separator = true;
                    end += 1;
                }
                'M' | 'm' if has_digit && has_separator => {
                    ranges.push((start, end + 1));
                    index = end + 1;
                    matched = true;
                    break;
                }
                _ => break,
            }
        }
        if !matched {
            index = index.saturating_add(1);
        }
    }

    if ranges.is_empty() {
        if looks_like_raw_mouse_report_run(run) {
            return Some(vec![false; run.len()]);
        }
        return None;
    }

    ranges.sort_unstable_by_key(|(start, _)| *start);
    let first_start = ranges[0].0;
    let mut prefix_start = first_start;
    while prefix_start > 0 && is_raw_mouse_report_fragment_char(run[prefix_start - 1]) {
        prefix_start -= 1;
    }
    if prefix_start < first_start
        && looks_like_raw_mouse_report_fragment(&run[prefix_start..first_start])
    {
        ranges.push((prefix_start, first_start));
    }

    let last_end = ranges.iter().map(|(_, end)| *end).max().unwrap_or_default();
    if last_end < run.len() && looks_like_raw_mouse_report_fragment(&run[last_end..]) {
        ranges.push((last_end, run.len()));
    }

    ranges.sort_unstable_by_key(|(start, _)| *start);
    let mut keep = vec![true; run.len()];
    for (start, end) in ranges {
        for slot in keep.iter_mut().take(end.min(run.len())).skip(start) {
            *slot = false;
        }
    }
    Some(keep)
}

fn is_raw_mouse_report_fragment_char(ch: char) -> bool {
    matches!(ch, ';' | ':' | 'M' | 'm') || ch.is_ascii_digit()
}

fn looks_like_raw_mouse_report_fragment(run: &[char]) -> bool {
    if run.len() < 4 {
        return false;
    }
    run.iter().any(|ch| ch.is_ascii_digit())
        && run.iter().any(|ch| matches!(ch, ';' | ':'))
        && run.iter().any(|ch| matches!(ch, 'M' | 'm'))
}

/// Scan `input` for control-sequence fragment shapes (#1915) — OSC 8
/// hyperlinks and Kitty keyboard protocol responses — and excise each
/// match. Returns `(output, new_cursor, changed)`. Cursor positions
/// inside an excised fragment are moved to the fragment's start.
///
/// The match shapes are deliberately narrow so legitimate text like
/// `[is this ok?]` or a typed URL survives untouched:
///
/// - **OSC 8**: `(\x1b?)] 8 ; ...` consuming everything up to the
///   first BEL (`\x07`), `\x1b\\`, lone `\\`, or the next `\x1b]8;`
///   block — terminator characters are optional because crossterm may
///   have already consumed them.
/// - **Kitty CSI**: `(\x1b?) [ (? | > | < | =) ... u` — the
///   private-parameter prefix is what distinguishes a Kitty response
///   from a user-typed `[…u` (which is exceedingly rare and would
///   need an explicit private-parameter byte to be a real CSI).
fn strip_control_sequence_fragments(input: &str, cursor: usize) -> (String, usize, bool) {
    let chars: Vec<char> = input.chars().collect();
    let mut output = String::with_capacity(input.len());
    let mut new_cursor = 0usize;
    let mut changed = false;
    let mut index = 0usize;

    while index < chars.len() {
        if let Some(end) = match_osc8_fragment(&chars, index) {
            // The excised span contributes nothing to `output`, so
            // `new_cursor` simply doesn't tick for any of those
            // characters. A cursor that was inside the span ends up at
            // the fragment's start position in the rewritten input,
            // which matches the existing run-stripper's behavior.
            index = end;
            changed = true;
            continue;
        }

        if let Some(end) = match_kitty_csi_fragment(&chars, index) {
            index = end;
            changed = true;
            continue;
        }

        if index < cursor {
            new_cursor += 1;
        }
        output.push(chars[index]);
        index += 1;
    }

    let cursor = new_cursor.min(char_count(&output));
    (output, cursor, changed)
}

/// If an OSC 8 hyperlink fragment starts at `chars[start]`, return its
/// end index (exclusive). The leading `ESC` is optional because
/// crossterm's event parser often consumes it before reclassifying the
/// tail as keystrokes.
fn match_osc8_fragment(chars: &[char], start: usize) -> Option<usize> {
    let body_start = if chars.get(start) == Some(&'\x1b')
        && chars.get(start + 1) == Some(&']')
        && chars.get(start + 2) == Some(&'8')
        && chars.get(start + 3) == Some(&';')
    {
        start + 4
    } else if chars.get(start) == Some(&']')
        && chars.get(start + 1) == Some(&'8')
        && chars.get(start + 2) == Some(&';')
    {
        start + 3
    } else {
        return None;
    };

    // After `]8;` we expect the OSC 8 payload: an optional second `;`
    // (params separator), then the URL (or empty for the closing
    // wrapper), then a terminator. We deliberately stop at the first
    // ASCII whitespace so a typed `]8;` followed by real prose can't
    // swallow the user's words — real OSC 8 URLs don't contain spaces.
    let mut end = body_start;
    while end < chars.len() {
        let ch = chars[end];
        // BEL terminator.
        if ch == '\x07' {
            return Some(end + 1);
        }
        // `ESC \\` string terminator (ST).
        if ch == '\x1b' && chars.get(end + 1) == Some(&'\\') {
            return Some(end + 2);
        }
        // Lone `\\` — crossterm sometimes delivers ST with the leading
        // ESC already consumed, leaving just `\\` as a Char keystroke.
        if ch == '\\' {
            return Some(end + 1);
        }
        // Start of the next OSC 8 wrapper (closing `]8;;` glued to the
        // body) — close the current fragment here so the next iteration
        // matches that one separately.
        if ch == '\x1b' && chars.get(end + 1) == Some(&']') {
            return Some(end);
        }
        if ch == ']' && chars.get(end + 1) == Some(&'8') && chars.get(end + 2) == Some(&';') {
            return Some(end);
        }
        if ch.is_whitespace() {
            // We never crossed a terminator, so this isn't a real
            // fragment — give up rather than eat user prose.
            return None;
        }
        end += 1;
    }

    // Reached end of input without a terminator or whitespace. Treat as
    // a fragment in flight (its tail will arrive on a later keystroke
    // and get filtered then).
    Some(end)
}

/// If a private-parameter CSI fragment starts at `chars[start]`, return its
/// end index (exclusive). Shape: `(ESC)? [ (? | > | < | =) [0-9;:]* <final>`
/// where `<final>` is any ASCII letter. This covers the Kitty keyboard
/// protocol (`…u`) *and* the DEC private mode set/reset sequences a terminal
/// emits during a session — bracketed paste (`[?2004h`/`[?2004l`), mouse
/// capture (`[?1000h`), focus reporting (`[?1004h`), and synchronized output
/// (`[?2026h`). Those end in `h`/`l`, not `u`, so the old `u`-only terminator
/// let the leading `[` leak into the composer during dense streaming (#2592,
/// regression of #1915). The private-parameter byte (`?`, `>`, `<`, `=`) is
/// what keeps this distinct from text the user might plausibly type.
fn match_kitty_csi_fragment(chars: &[char], start: usize) -> Option<usize> {
    // Leading ESC is optional (crossterm may have consumed it).
    let seq_start = if chars.get(start) == Some(&'\x1b') {
        start + 1
    } else {
        start
    };

    if chars.get(seq_start) != Some(&'[') {
        return None;
    }

    let private = chars.get(seq_start + 1);
    if !matches!(private, Some('?' | '>' | '<' | '=')) {
        return None;
    }

    // Skip parameter bytes (digits, semicolons, colons).
    let mut pos = seq_start + 2;
    while pos < chars.len() && matches!(chars[pos], '0'..='9' | ';' | ':') {
        pos += 1;
    }

    // Final byte: any ASCII letter completes the sequence.
    if pos < chars.len() && chars[pos].is_ascii_alphabetic() {
        Some(pos + 1)
    } else {
        // No final byte yet — fragment in flight.
        Some(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_raw_mouse_report_runs_noop() {
        let input = "hello world";
        assert_eq!(strip_raw_mouse_report_runs(input, 5), None);
    }

    #[test]
    fn test_strip_raw_mouse_report_runs_with_sgr() {
        let input = "[<35;44;18Mhello";
        let result = strip_raw_mouse_report_runs(input, 0);
        assert!(result.is_some());
        let (output, _) = result.unwrap();
        assert_eq!(output, "hello");
    }

    #[test]
    fn test_match_osc8_fragment() {
        let chars: Vec<char> = "]8;;http://example.com\x07".chars().collect();
        let result = match_osc8_fragment(&chars, 0);
        assert!(result.is_some());
    }

    #[test]
    fn test_match_kitty_csi_fragment() {
        let chars: Vec<char> = "[?2004h".chars().collect();
        let result = match_kitty_csi_fragment(&chars, 0);
        assert!(result.is_some());
    }
}
