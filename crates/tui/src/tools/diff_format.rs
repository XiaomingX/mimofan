//! Build unified-diff strings for tool results.
//!
//! `edit_file` and `write_file` capture the file contents before and after
//! the mutation and emit a unified diff at the head of their `ToolResult`
//! output. The TUI's `output_looks_like_diff` detector then routes the
//! payload through `diff_render::render_diff`, which renders it with line
//! numbers and coloured `+`/`-` gutters (#505).
//!
//! The diff is also a strict UX upgrade for the model — it sees exactly
//! which lines changed instead of a one-line summary.

use similar::TextDiff;

/// Build a unified diff between `old` and `new` keyed at `path`.
///
/// Returns an empty string when the inputs are byte-identical so callers
/// can skip the "no changes" header. The output uses git-style `--- a/...`
/// / `+++ b/...` headers and three lines of context — matching the format
/// the TUI's `diff_render::render_diff` already understands.
#[must_use]
pub fn make_unified_diff(path: &str, old: &str, new: &str) -> String {
    if old == new {
        return String::new();
    }
    let a = format!("a/{path}");
    let b = format!("b/{path}");
    let diff = TextDiff::from_lines(old, new);
    diff.unified_diff()
        .context_radius(3)
        .header(&a, &b)
        .to_string()
}

#[cfg(test)]
mod tests {}
