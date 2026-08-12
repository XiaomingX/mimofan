//! File system tools: `read_file`, `write_file`, `edit_file`, `list_dir`
//!
//! These tools provide safe file system operations within the workspace,
//! with path validation to prevent escaping the workspace boundary.

use super::diff_format::make_unified_diff;
use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    lsp_diagnostics_for_paths, optional_bool, optional_str, required_str,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::fmt::Display;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Byte-level formatting traits of a file that must survive an edit.
///
/// Editing one line of a CRLF file must not rewrite every other line ending,
/// and a UTF-8 BOM must not silently disappear — either would produce a huge
/// spurious diff that buries the real change in review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct FileFidelity {
    /// File began with a UTF-8 BOM (EF BB BF).
    bom: bool,
    /// CRLF is the dominant line ending, so edits should be written back
    /// with CRLF.
    crlf: bool,
}

/// UTF-8 byte order mark, as it appears once decoded to `str`.
const UTF8_BOM: char = '\u{feff}';

impl FileFidelity {
    /// Split raw file contents into their formatting traits and a normalized
    /// body (no BOM, LF-only) that search/replace logic can operate on.
    fn detect(raw: &str) -> (Self, String) {
        let (bom, without_bom) = match raw.strip_prefix(UTF8_BOM) {
            Some(rest) => (true, rest),
            None => (false, raw),
        };

        let crlf_count = without_bom.matches("\r\n").count();
        // Bare LF = total LF minus those that are part of a CRLF pair.
        let lf_count = without_bom.matches('\n').count() - crlf_count;
        // Ties favor CRLF: a mixed file that is majority-CRLF (or evenly
        // split) is treated as a CRLF file so edits stay consistent with it.
        let crlf = crlf_count > 0 && crlf_count >= lf_count;

        let normalized = if crlf_count > 0 {
            without_bom.replace("\r\n", "\n")
        } else {
            without_bom.to_string()
        };

        (Self { bom, crlf }, normalized)
    }

    /// Re-apply the original formatting to edited, normalized content.
    fn restore(self, normalized: &str) -> String {
        let mut out = if self.crlf {
            // Guard against pre-existing CR in the replacement text so we
            // never emit CRCRLF.
            normalized.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            normalized.to_string()
        };
        if self.bom {
            out.insert(0, UTF8_BOM);
        }
        out
    }
}

/// Map a byte range within `contents` to the 1-based inclusive line numbers
/// it spans, so read-coverage can be checked against what `read_file` showed.
fn line_span_for_byte_range(contents: &str, start: usize, end: usize) -> (usize, usize) {
    let start = start.min(contents.len());
    let end = end.clamp(start, contents.len());
    let first = contents[..start].matches('\n').count() + 1;
    // A range ending exactly at a newline covers only the lines before it.
    let inner = contents[start..end].trim_end_matches('\n');
    let last = first + inner.matches('\n').count();
    (first, last)
}

/// Byte ranges of every non-overlapping occurrence of `needle` in `haystack`.
fn match_byte_ranges(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
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
fn line_content_hash(line: &str) -> String {
    let mut hasher = DefaultHasher::new();
    line.trim_start().hash(&mut hasher);
    format!("{:06x}", hasher.finish() & 0xFFFFFF)
}

/// Find a line by its content anchor hash.
/// Returns (line_start_byte, line_end_byte) including the newline.
/// The search is performed on trimmed content (without leading whitespace).
fn find_line_by_anchor(contents: &str, anchor: &str) -> Option<(usize, usize)> {
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
fn find_all_lines_by_anchor(contents: &str, anchor: &str) -> Vec<(usize, usize)> {
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

// === ReadFileTool ===

/// Tool for reading UTF-8 files from the workspace.
pub struct ReadFileTool;

#[async_trait]
impl ToolSpec for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read a UTF-8 file from the workspace. Use this instead of `cat`, `head`, `tail`, or `sed -n '..p'` in `exec_shell` — it's faster, sandbox-aware, and skips the approval prompt. Plain text is returned as-is and records the file snapshot required before `edit_file` will make a narrow in-place edit. PDFs are auto-extracted via the bundled pure-Rust extractor (no Poppler install required). Image screenshots are OCR-extracted when local OCR is available. Cannot read other non-PDF binaries.\n\nEach line includes a 6-character content hash anchor (e.g. `     1│a1b2c3│ line`). Use this anchor with `edit_file`'s `line_ref` parameter for token-efficient editing without retyping the full line content.\n\nFor large files, use `start_line` and `max_lines` to read in chunks. By default, returns at most 200 lines (~16KB). If `truncated=\"true\"` in the response, use `next_start_line` to continue reading. For PDFs, use `pages` instead — `start_line`/`max_lines` only apply to text files."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to workspace or absolute)"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Starting line (1-based, default 1)"
                },
                "max_lines": {
                    "type": "integer",
                    "description": "Maximum lines to return (default 200, max 500)"
                },
                "pages": {
                    "type": "string",
                    "description": "PDF only: page range to extract, e.g. \"1-5\" or \"10\". Ignored for non-PDF files."
                }
            },
            "required": ["path"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        // Bounded output for large files. The small-file fast path keeps the
        // historical "return contents unchanged" behavior so existing flows
        // (small configs, single source files, etc.) don't suddenly start
        // seeing wrapped output. Once a file is large or the caller asks
        // for an explicit range, we switch to a numbered, line-tagged
        // window with continuation hints so the model can page through
        // without re-loading the entire file on every turn. Harvested
        // from PR #1451 by @Oliver-ZPLiu, closes part of #1450.
        const DEFAULT_READ_LINES: usize = 200;
        const HARD_MAX_READ_LINES: usize = 1500;
        const MAX_VISIBLE_BYTES: usize = 16 * 1024;
        const SMALL_FILE_LINES: usize = 200;
        const SMALL_FILE_BYTES: usize = 16 * 1024;

        let path_str = required_str(&input, "path")?;
        let file_path = context.resolve_path(path_str)?;
        let pages = optional_str(&input, "pages");

        if is_pdf(&file_path)? {
            return read_pdf(&file_path, pages);
        }
        if is_image_for_ocr(&file_path) {
            return read_image_via_ocr(&file_path, path_str);
        }

        let contents = crate::tools::vfs::active_vfs()
            .read_text(&file_path)
            .map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to read {}: {}",
                    file_path.display(),
                    e
                ))
            })?;

        let total_lines = contents.lines().count();
        let total_bytes = contents.len();
        let explicit_range = input
            .get("start_line")
            .or_else(|| input.get("max_lines"))
            .is_some();

        // Small-file fast path. Only applies when the caller didn't pass an
        // explicit range — otherwise an explicit `start_line = 5` on a
        // tiny file would silently ignore the request.
        if !explicit_range && total_lines <= SMALL_FILE_LINES && total_bytes <= SMALL_FILE_BYTES {
            // Whole file was returned verbatim: full coverage.
            context.note_file_read(&file_path);
            return Ok(ToolResult::success(contents));
        }

        let start_line = match input.get("start_line").and_then(Value::as_u64) {
            Some(0) => {
                return Err(ToolError::invalid_input(
                    "start_line must be 1-based and greater than 0".to_string(),
                ));
            }
            Some(v) => usize::try_from(v).map_err(|_| {
                ToolError::invalid_input(
                    "start_line exceeds platform addressable range".to_string(),
                )
            })?,
            None => 1,
        };

        let max_lines = match input.get("max_lines").and_then(Value::as_u64) {
            Some(0) => {
                return Err(ToolError::invalid_input(
                    "max_lines must be greater than 0".to_string(),
                ));
            }
            Some(v) => {
                let converted = usize::try_from(v).map_err(|_| {
                    ToolError::invalid_input(
                        "max_lines exceeds platform addressable range".to_string(),
                    )
                })?;
                std::cmp::min(converted, HARD_MAX_READ_LINES)
            }
            None => DEFAULT_READ_LINES,
        };

        // `start_line > total_lines` is not an error — it lets the model
        // page past the end without raising. Returns an empty-content
        // sentinel so subsequent reads can stop.
        if start_line > total_lines {
            let output = format!(
                "<file path=\"{path_str}\" total_lines=\"{total_lines}\" shown_lines=\"none\" truncated=\"false\">\n\
                 \n\
                 [NO CONTENT] start_line {start_line} is beyond total_lines {total_lines}.\n\
                 </file>"
            );
            return Ok(ToolResult::success(output));
        }

        let lines: Vec<&str> = contents.lines().collect();
        let zero_based_start = start_line - 1;
        let zero_based_end = std::cmp::min(zero_based_start + max_lines, total_lines);
        let shown_first = start_line;
        let shown_last = zero_based_end; // 1-based inclusive line number of the last shown line

        let mut numbered = String::new();
        for (offset, line) in lines[zero_based_start..zero_based_end].iter().enumerate() {
            let line_no = start_line + offset;
            let hash = line_content_hash(line);
            numbered.push_str(&format!("{line_no:>6}│{hash}│ {line}\n"));
        }

        // UTF-8-safe byte truncation of the rendered range.
        let truncated_by_bytes = numbered.len() > MAX_VISIBLE_BYTES;
        let shown_content = if truncated_by_bytes {
            let mut end = MAX_VISIBLE_BYTES;
            while end > 0 && !numbered.is_char_boundary(end) {
                end -= 1;
            }
            &numbered[..end]
        } else {
            &numbered
        };

        let truncated_by_lines = zero_based_end < total_lines;
        let truncated = truncated_by_lines || truncated_by_bytes;
        let next_start = zero_based_end + 1;

        let mut attrs = format!(
            "path=\"{path_str}\" total_lines=\"{total_lines}\" shown_lines=\"{shown_first}-{shown_last}\" truncated=\"{truncated}\""
        );
        if truncated_by_lines {
            attrs.push_str(&format!(" next_start_line=\"{next_start}\""));
        }

        let mut output = format!("<file {attrs}>\n{shown_content}");
        if truncated_by_lines {
            output.push_str(&format!(
                "\n[TRUNCATED] Showing lines {shown_first}-{shown_last} of {total_lines}. To continue, call read_file with path=\"{path_str}\" start_line={next_start} max_lines={max_lines}\n"
            ));
        }
        if truncated_by_bytes {
            output.push_str(
                "\n[TRUNCATED] The selected range exceeded 16KB. Continue with a smaller max_lines value.\n",
            );
        }
        output.push_str("</file>");

        // Record only the lines actually delivered to the caller. When the
        // render was cut off by the byte cap, the trailing partial line was
        // not fully shown, so coverage stops at the last complete line.
        let covered_last = if truncated_by_bytes {
            let complete_lines = shown_content.matches('\n').count();
            shown_first + complete_lines.saturating_sub(1)
        } else {
            shown_last
        };
        if covered_last >= shown_first {
            context.note_file_read_range(&file_path, shown_first, covered_last);
        }

        Ok(ToolResult::success(output))
    }
}

fn read_image_via_ocr(path: &Path, requested_path: &str) -> Result<ToolResult, ToolError> {
    let text = crate::tools::image_ocr::ocr_image_path(path)?;
    Ok(ToolResult::success(format!(
        "<image_ocr path=\"{requested_path}\">\n{text}\n</image_ocr>"
    )))
}

/// Detect a PDF by extension OR by sniffing the `%PDF-` magic bytes.
/// Files without an extension are still recognized as PDFs when the header
/// matches.
fn is_pdf(path: &Path) -> Result<bool, ToolError> {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    {
        return Ok(true);
    }
    // Sniff first 4 bytes. Don't error if the file doesn't exist — let the
    // caller's `read_to_string` produce the canonical not-found error.
    let mut buf = [0u8; 4];
    let result = match fs::File::open(path) {
        Ok(mut f) => {
            use std::io::Read;
            f.read_exact(&mut buf).map(|_| buf)
        }
        Err(_) => return Ok(false),
    };
    Ok(matches!(result, Ok(b) if &b == b"%PDF"))
}

fn is_image_for_ocr(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp"
            )
        })
}

fn parse_pages_arg(spec: &str) -> Option<(u32, u32)> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((a, b)) = trimmed.split_once('-') {
        let start: u32 = a.trim().parse().ok()?;
        let end: u32 = b.trim().parse().ok()?;
        if start == 0 || end < start {
            return None;
        }
        Some((start, end))
    } else {
        let n: u32 = trimmed.parse().ok()?;
        if n == 0 {
            return None;
        }
        Some((n, n))
    }
}

/// Clean PDF-extracted text for TUI display: collapse consecutive blank
/// lines (more than 1 becomes 1), replace NUL bytes with U+FFFD, replace
/// non-breaking spaces with regular spaces, and trim trailing whitespace
/// on each line. Produces output that won't clutter the transcript with
/// vertical gaps or invisible control characters.
fn clean_pdf_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut blank_run = 0usize;
    let mut any_content = false;
    for line in raw.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_run = blank_run.saturating_add(1);
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            any_content = true;
            // Push cleaned characters directly — avoids a per-line
            // temporary String allocation.
            for c in trimmed.chars() {
                match c {
                    '\0' => out.push('\u{FFFD}'),
                    '\u{A0}' => out.push(' '),
                    other => out.push(other),
                }
            }
            out.push('\n');
        }
    }
    // Trim leading blank lines only — don't use str::trim() which
    // would also strip intentional indentation (e.g. centred titles).
    if any_content {
        let start = out.find(|c: char| c != '\n').unwrap_or(0);
        // Walk back from end to find the last non-newline character.
        let end = out.rfind(|c: char| c != '\n').map_or(out.len(), |i| {
            i + out[i..].chars().next().map_or(1, |c| c.len_utf8())
        });
        out[start..end].to_string()
    } else {
        String::new()
    }
}

fn read_pdf(path: &Path, pages: Option<&str>) -> Result<ToolResult, ToolError> {
    // Validate the `pages` spec once, up front, so both extractor paths
    // surface the same error shape on bad input.
    let page_range = match pages {
        Some(spec) => match parse_pages_arg(spec) {
            Some((start, end)) => Some((start, end)),
            None => {
                return Err(ToolError::invalid_input(format!(
                    "invalid `pages` value `{spec}` (expected `N` or `N-M`, e.g. `1-5`)"
                )));
            }
        },
        None => None,
    };

    // Default to the bundled pure-Rust `pdf-extract` reader: it removes
    // the install-poppler prerequisite that bit every new user, and the
    // crate is already a workspace dep (used by `web_run`'s URL fetch
    // path). Users with column-heavy / complex-table PDFs (academic
    // papers, financial filings) can opt into the historical
    // `pdftotext -layout` route by setting
    // `prefer_external_pdftotext = true` in `~/.mimofan/settings.json`.
    let prefer_external = crate::settings::Settings::load()
        .map(|s| s.prefer_external_pdftotext)
        .unwrap_or(false);

    if prefer_external {
        read_pdf_via_pdftotext(path, page_range)
    } else {
        read_pdf_via_pdf_extract(path, page_range)
    }
}

fn read_pdf_via_pdf_extract(
    path: &Path,
    page_range: Option<(u32, u32)>,
) -> Result<ToolResult, ToolError> {
    let text = if let Some((start, end)) = page_range {
        // Page-by-page extraction so we can slice the requested window
        // without dragging every page through the caller's context.
        // pdf-extract returns pages in document order; `start`/`end` are
        // 1-indexed inclusive (validated above), so we convert to a
        // 0-indexed half-open slice with bounds clamping.
        let pages = guard_pdf_extract(|| pdf_extract::extract_text_by_pages(path)).map_err(|e| {
            ToolError::execution_failed(format!(
                "pdf-extract failed on {}: {e} (set `prefer_external_pdftotext = true` in settings.json to retry via pdftotext)",
                path.display()
            ))
        })?;
        let total = pages.len();
        if total == 0 {
            String::new()
        } else {
            let start_idx = (start as usize).saturating_sub(1).min(total);
            let end_idx = (end as usize).min(total);
            if start_idx >= end_idx {
                String::new()
            } else {
                pages[start_idx..end_idx].join("\n")
            }
        }
    } else {
        // Call extract_text_by_pages even when the caller wants every page:
        // extract_text uses an internal codepath that can hang on certain PDF
        // cross-reference tables or font encodings (#2641). The per-page path
        // avoids that hang and produces identical output when joined.
        guard_pdf_extract(|| pdf_extract::extract_text_by_pages(path))
            .map(|pages| pages.join("\n"))
            .map_err(|e| {
                ToolError::execution_failed(format!(
                    "pdf-extract failed on {}: {e} (set `prefer_external_pdftotext = true` in settings.json to retry via pdftotext)",
                    path.display()
                ))
            })?
    };
    Ok(ToolResult::success(clean_pdf_text(&text)))
}

fn guard_pdf_extract<T, E, F>(extract: F) -> Result<T, String>
where
    E: Display,
    F: FnOnce() -> Result<T, E>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(extract)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(err.to_string()),
        Err(payload) => Err(format!(
            "extractor panicked: {}",
            panic_payload_message(payload.as_ref())
        )),
    }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn read_pdf_via_pdftotext(
    path: &Path,
    page_range: Option<(u32, u32)>,
) -> Result<ToolResult, ToolError> {
    let mut cmd = Command::new("pdftotext");
    cmd.arg("-layout");

    if let Some((start, end)) = page_range {
        cmd.arg("-f").arg(start.to_string());
        cmd.arg("-l").arg(end.to_string());
    }

    cmd.arg(path).arg("-"); // output to stdout
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Structured "binary unavailable" — only reachable when the
            // user explicitly opted into the external path. Hints back at
            // both the install command and the in-tree default.
            return ToolResult::json(&json!({
                "type": "binary_unavailable",
                "path": path.display().to_string(),
                "kind": "pdf",
                "reason": "pdftotext not installed (prefer_external_pdftotext = true in settings)",
                "hint": "install poppler (macOS: `brew install poppler`; Debian/Ubuntu: `apt install poppler-utils`) — or unset `prefer_external_pdftotext` to use the bundled pure-Rust extractor"
            }))
            .map_err(|e| {
                ToolError::execution_failed(format!("failed to serialize response: {e}"))
            });
        }
        Err(e) => {
            return Err(ToolError::execution_failed(format!(
                "failed to launch pdftotext: {e}"
            )));
        }
    };

    let output = child
        .wait_with_output()
        .map_err(|e| ToolError::execution_failed(format!("pdftotext failed to complete: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ToolError::execution_failed(format!(
            "pdftotext failed (exit {:?}): {stderr}",
            output.status.code()
        )));
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(ToolResult::success(clean_pdf_text(&text)))
}

// === WriteFileTool ===

/// Tool for writing UTF-8 files to the workspace.
pub struct WriteFileTool;

#[async_trait]
impl ToolSpec for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Write content to a UTF-8 file in the workspace. Use this instead of heredocs (`cat <<EOF > file`) or `echo > file` in `exec_shell` — diffs render inline and approval is handled cleanly. Creates or overwrites; parent directories are auto-created."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::WritesFiles,
            ToolCapability::Sandboxable,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Suggest
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = required_str(&input, "path")?;
        let file_content = required_str(&input, "content")?;

        let file_path = context.resolve_path(path_str)?;

        // Snapshot the existing contents (if any) before we overwrite — used
        // to render an inline diff in the tool result.
        let existed_before = file_path.exists();

        // Overwriting an existing file destroys content wholesale, so it is
        // gated by the same read-before-write rule as `edit_file`. Creating a
        // new file is exempt: there is nothing to have read beforehand.
        if existed_before {
            context.require_fresh_file_read_for("write_file", &file_path, path_str)?;
        }

        let vfs = crate::tools::vfs::active_vfs();
        let prior_contents = if existed_before {
            vfs.read_text(&file_path).unwrap_or_default()
        } else {
            String::new()
        };

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            vfs.create_dir_all(parent).map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        vfs.write_text(&file_path, file_content).map_err(|e| {
            ToolError::execution_failed(format!("Failed to write {}: {}", file_path.display(), e))
        })?;
        context.note_file_read(&file_path);

        let display = file_path.display().to_string();
        let diff = make_unified_diff(&display, &prior_contents, file_content);
        let summary = if existed_before {
            format!("Wrote {} bytes to {}", file_content.len(), display)
        } else {
            format!("Created {} ({} bytes)", display, file_content.len())
        };
        let body = if diff.is_empty() {
            format!("{summary}\n(no changes)")
        } else {
            format!("{diff}\n{summary}")
        };

        // Append LSP diagnostics for the written file when enabled (#428).
        let diag_block = lsp_diagnostics_for_paths(context, &[file_path]).await;
        let full_body = if diag_block.is_empty() {
            body
        } else {
            format!("{body}\n{diag_block}")
        };

        Ok(ToolResult::success(full_body))
    }
}

// === EditFileTool ===

/// Tool for search/replace editing of files.
pub struct EditFileTool;

#[async_trait]
impl ToolSpec for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Replace text in a single file via exact search/replace after the file has been read with `read_file` in this session. Use this instead of `sed -i` in `exec_shell` for one unambiguous in-place edit. Returns a compact unified diff, not the full file. For structural, multi-block, or cross-file changes, use `apply_patch` or `write_file` instead.\n\nTwo editing modes:\n1. **Anchor mode** (token-efficient): Use `line_ref` from `read_file` output + `replace` with the new line content. No need to retype the full old line.\n2. **Search mode** (legacy): Use `search` + `replace` for exact text matching. Supports automatic fuzzy matching for indentation and punctuation differences. Set `replace_all=true` to rewrite every occurrence in one call (rename-style edits); otherwise the search must match exactly once.\n\nThe file's original BOM and CRLF/LF line endings are preserved."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "line_ref": {
                    "type": "string",
                    "description": "Line anchor from read_file output (e.g. 'a1b2c3'). Replaces the anchored line with `replace` content."
                },
                "search": {
                    "type": "string",
                    "description": "Exact text to search for, including whitespace, indentation, and newlines. Used when line_ref is not provided."
                },
                "replace": {
                    "type": "string",
                    "description": "Text to replace with (required)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence of `search` instead of requiring a unique match (default false). Use for rename-style edits where one symbol changes in many places. Ignored in anchor mode."
                },
                "fuzz": {
                    "type": "boolean",
                    "description": "Deprecated: fuzzy fallback is now automatic. Accepted for backward compatibility but ignored."
                }
            },
            "required": ["path", "replace"],
            "anyOf": [
                { "required": ["line_ref"] },
                { "required": ["search"] }
            ]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::WritesFiles,
            ToolCapability::Sandboxable,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Suggest
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = required_str(&input, "path")?;
        let replace = required_str(&input, "replace")?;
        let line_ref = optional_str(&input, "line_ref");
        let search = optional_str(&input, "search");
        let replace_all = optional_bool(&input, "replace_all", false);
        let _fuzz = optional_bool(&input, "fuzz", false);

        // Validate: must have either line_ref or search
        let is_anchor_mode = match (&line_ref, &search) {
            (Some(lr), _) if !lr.trim().is_empty() => true,
            (None, Some(s)) if !s.trim().is_empty() => false,
            (None, None) | (Some(_), None) => {
                return Err(ToolError::invalid_input(
                    "Either line_ref (from read_file anchor) or search text is required"
                        .to_string(),
                ));
            }
            (Some(lr), Some(s)) if lr.trim().is_empty() && s.trim().is_empty() => {
                return Err(ToolError::invalid_input(
                    "Either line_ref or search must be non-empty".to_string(),
                ));
            }
            (Some(_), Some(_)) => {
                // Both provided - prefer anchor mode
                true
            }
            _ => false,
        };

        let file_path = context.resolve_path(path_str)?;
        context.require_fresh_file_read(&file_path, path_str)?;

        let raw_contents = crate::tools::vfs::active_vfs()
            .read_text(&file_path)
            .map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to read {}: {}",
                    file_path.display(),
                    e
                ))
            })?;
        // Match against a normalized (BOM-stripped, LF-only) body so search
        // strings written with plain \n still match CRLF files; the original
        // byte formatting is restored before writing.
        let (fidelity, contents) = FileFidelity::detect(&raw_contents);

        let (updated, count, fuzz_kind) = if is_anchor_mode {
            // Anchor mode: find line by content hash
            let anchor = line_ref.unwrap();
            let matches = find_all_lines_by_anchor(&contents, anchor);
            match matches.as_slice() {
                [] => {
                    return Err(ToolError::execution_failed(format!(
                        "Anchor '{}' not found in {}. The line may have been deleted or modified. Recovery: call read_file to get fresh anchors.",
                        anchor,
                        file_path.display(),
                    )));
                }
                [(start, end)] => {
                    let (first, last) = line_span_for_byte_range(&contents, *start, *end);
                    context.require_read_coverage(&file_path, path_str, first, last)?;
                    let mut updated = contents.clone();
                    // Replace the entire line. User provides line content without
                    // trailing newline; we add it back if the original had one.
                    let original_had_newline = end > start && contents.as_bytes()[end - 1] == b'\n';
                    let new_content = if original_had_newline && !replace.ends_with('\n') {
                        format!("{replace}\n")
                    } else if !original_had_newline && replace.ends_with('\n') {
                        replace.trim_end_matches('\n').to_string()
                    } else {
                        replace.to_string()
                    };
                    updated.replace_range(*start..*end, &new_content);
                    (updated, 1, Some("anchor"))
                }
                _ => {
                    return Err(ToolError::execution_failed(format!(
                        "Anchor '{}' is non-unique: matched {} locations in {}. Recovery: use a different anchor or provide surrounding context with search mode.",
                        anchor,
                        matches.len(),
                        file_path.display(),
                    )));
                }
            }
        } else {
            // Search mode: existing str_replace logic
            let search = search.unwrap();
            if search == replace {
                return Err(ToolError::invalid_input(
                    "search and replace are identical, no change intended",
                ));
            }

            let count = contents.matches(&search).count();
            if count == 0 {
                // First fallback: tolerate indentation differences.
                let indent_matches = leading_whitespace_fuzzy_matches(&contents, search);
                match indent_matches.as_slice() {
                    [(start, end)] => {
                        let (first, last) = line_span_for_byte_range(&contents, *start, *end);
                        context.require_read_coverage(&file_path, path_str, first, last)?;
                        let mut updated = contents.clone();
                        updated.replace_range(*start..*end, replace);
                        (updated, 1, Some("indentation"))
                    }
                    [] => {
                        // Second fallback: tolerate typographic-punctuation
                        // drift (smart quotes, em-dashes, NBSP). Picks up the
                        // copy-paste failure mode where a browser/chat client
                        // silently substituted Unicode punctuation in for the
                        // ASCII the file actually contains.
                        let punct_matches = punctuation_normalized_matches(&contents, search);
                        match punct_matches.as_slice() {
                            [] => {
                                return Err(ToolError::execution_failed(format!(
                                    "Search string not found in {}. Recovery: call read_file with path=\"{path_str}\" to inspect the current contents, then retry with a search string copied from the file.",
                                    file_path.display(),
                                )));
                            }
                            [(start, end)] => {
                                let (first, last) =
                                    line_span_for_byte_range(&contents, *start, *end);
                                context.require_read_coverage(&file_path, path_str, first, last)?;
                                let mut updated = contents.clone();
                                updated.replace_range(*start..*end, replace);
                                (updated, 1, Some("punctuation"))
                            }
                            _ => {
                                return Err(ToolError::execution_failed(format!(
                                    "edit_file search is non-unique after punctuation normalization: matched {} locations in {}. Recovery: call read_file with path=\"{path_str}\" and retry with surrounding lines that make the search unique.",
                                    punct_matches.len(),
                                    file_path.display()
                                )));
                            }
                        }
                    }
                    _ => {
                        return Err(ToolError::execution_failed(format!(
                            "edit_file search is non-unique after indentation normalization: matched {} locations in {}. Recovery: call read_file with path=\"{path_str}\" and retry with surrounding lines that make the search unique.",
                            indent_matches.len(),
                            file_path.display()
                        )));
                    }
                }
            } else if count > 1 && !replace_all {
                return Err(ToolError::execution_failed(format!(
                    "edit_file search is non-unique: matched {count} locations in {}. \
                     Recovery: either retry with surrounding lines that make the search match exactly once, \
                     or pass replace_all=true to replace all {count} occurrences in a single call.",
                    file_path.display()
                )));
            } else {
                // Every match site must have been read, otherwise a
                // replace_all could rewrite regions the model never saw.
                for (start, end) in match_byte_ranges(&contents, search) {
                    let (first, last) = line_span_for_byte_range(&contents, start, end);
                    context.require_read_coverage(&file_path, path_str, first, last)?;
                }
                (contents.replace(search, replace), count, None)
            }
        };

        // Restore the original BOM and line-ending style so untouched lines
        // keep their exact original bytes.
        let to_write = fidelity.restore(&updated);
        crate::tools::vfs::active_vfs()
            .write_text(&file_path, &to_write)
            .map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to write {}: {}",
                    file_path.display(),
                    e
                ))
            })?;
        context.note_file_read(&file_path);

        let display = file_path.display().to_string();
        let diff = make_unified_diff(&display, &contents, &updated);
        let fuzz_note = match fuzz_kind {
            Some("anchor") => " (anchor match)",
            Some("indentation") => " (fuzzy indentation match)",
            Some("punctuation") => {
                " (fuzzy punctuation match — typographic quotes/dashes normalized)"
            }
            Some(other) => other,
            None => "",
        };
        let plural = if count == 1 { "" } else { "s" };
        let summary = format!("Replaced {count} occurrence{plural} in {display}{fuzz_note}");
        let body = if diff.is_empty() {
            format!("{summary}\n(no textual changes)")
        } else {
            format!("{diff}\n{summary}")
        };

        // Append LSP diagnostics for the edited file when enabled (#428).
        let diag_block = lsp_diagnostics_for_paths(context, &[file_path]).await;
        let full_body = if diag_block.is_empty() {
            body
        } else {
            format!("{body}\n{diag_block}")
        };

        Ok(ToolResult::success(full_body))
    }
}

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

fn leading_whitespace_fuzzy_matches(contents: &str, search: &str) -> Vec<(usize, usize)> {
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
fn punctuation_normalized_matches(contents: &str, search: &str) -> Vec<(usize, usize)> {
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

// === ListDirTool ===

/// Tool for listing directory contents.
pub struct ListDirTool;

const LIST_DIR_TIMEOUT: Duration = Duration::from_secs(30);

#[async_trait]
impl ToolSpec for ListDirTool {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List entries in a directory relative to the workspace. Use this instead of `ls`, `ls -la`, or `find . -maxdepth 1` in `exec_shell` for directory listings."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path (default: .)"
                }
            },
            "required": []
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = optional_str(&input, "path").unwrap_or(".");
        let dir_path = context.resolve_path(path_str)?;

        let entries =
            list_dir_entries_async(dir_path, context.cancel_token.clone(), LIST_DIR_TIMEOUT)
                .await?;

        ToolResult::json(&entries).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

async fn list_dir_entries_async(
    dir_path: PathBuf,
    cancel_token: Option<CancellationToken>,
    timeout: Duration,
) -> Result<Vec<Value>, ToolError> {
    let worker_cancel_token = cancel_token.clone();
    run_blocking_list_dir(timeout, cancel_token, move || {
        list_dir_entries(&dir_path, worker_cancel_token.as_ref())
    })
    .await
}

async fn run_blocking_list_dir<F>(
    timeout: Duration,
    cancel_token: Option<CancellationToken>,
    list_dir: F,
) -> Result<Vec<Value>, ToolError>
where
    F: FnOnce() -> Result<Vec<Value>, ToolError> + Send + 'static,
{
    if cancel_token
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        return Err(list_dir_cancelled());
    }

    let task = tokio::task::spawn_blocking(list_dir);
    let result = match cancel_token {
        Some(token) => {
            tokio::select! {
                biased;
                () = token.cancelled() => return Err(list_dir_cancelled()),
                result = tokio::time::timeout(timeout, task) => result,
            }
        }
        None => tokio::time::timeout(timeout, task).await,
    };

    let joined = result.map_err(|_| list_dir_timeout(timeout))?;
    joined.map_err(|err| {
        ToolError::execution_failed(format!("list_dir worker failed before completion: {err}"))
    })?
}

fn list_dir_entries(
    dir_path: &Path,
    cancel_token: Option<&CancellationToken>,
) -> Result<Vec<Value>, ToolError> {
    check_list_dir_cancelled(cancel_token)?;

    let mut entries = Vec::new();

    let dir_entries = crate::tools::vfs::active_vfs()
        .list_dir(dir_path)
        .map_err(|e| {
            ToolError::execution_failed(format!(
                "Failed to read directory {}: {}",
                dir_path.display(),
                e
            ))
        })?;
    for entry in dir_entries {
        check_list_dir_cancelled(cancel_token)?;

        let is_dir = entry
            .metadata()
            .map(|m| m.is_dir())
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        entries.push(json!({
            "name": entry.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "is_dir": is_dir,
        }));
    }

    Ok(entries)
}

fn check_list_dir_cancelled(cancel_token: Option<&CancellationToken>) -> Result<(), ToolError> {
    if cancel_token.is_some_and(CancellationToken::is_cancelled) {
        return Err(list_dir_cancelled());
    }
    Ok(())
}

fn list_dir_cancelled() -> ToolError {
    ToolError::execution_failed("list_dir cancelled before completion")
}

fn list_dir_timeout(timeout: Duration) -> ToolError {
    ToolError::Timeout {
        seconds: timeout.as_secs().max(1),
    }
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn ctx(dir: &TempDir) -> ToolContext {
        ToolContext::new(dir.path().to_path_buf())
    }

    async fn read_all(ctx: &ToolContext, name: &str) {
        ReadFileTool
            .execute(json!({ "path": name }), ctx)
            .await
            .expect("read_file should succeed");
    }

    async fn edit(ctx: &ToolContext, input: Value) -> Result<ToolResult, ToolError> {
        EditFileTool.execute(input, ctx).await
    }

    // --- 1. Partial reads must not authorize whole-file edits ---

    #[test]
    fn line_span_maps_byte_ranges_to_line_numbers() {
        let text = "one\ntwo\nthree\nfour\n";
        // "one" occupies line 1.
        assert_eq!(line_span_for_byte_range(text, 0, 3), (1, 1));
        // "three" is on line 3.
        let idx = text.find("three").unwrap();
        assert_eq!(line_span_for_byte_range(text, idx, idx + 5), (3, 3));
        // A range spanning two lines reports both.
        let idx = text.find("two").unwrap();
        assert_eq!(line_span_for_byte_range(text, idx, idx + "two\nthree".len()), (2, 3));
        // A trailing newline does not pull in the following line.
        assert_eq!(line_span_for_byte_range(text, 0, 4), (1, 1));
    }

    #[tokio::test]
    async fn partial_read_rejects_edit_outside_observed_range() {
        let dir = TempDir::new().unwrap();
        // 400 distinct lines so the read is windowed rather than whole-file.
        let body: String = (1..=400).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.path().join("big.txt"), &body).unwrap();
        let ctx = ctx(&dir);

        // Read only the first 200 lines.
        ReadFileTool
            .execute(
                json!({ "path": "big.txt", "start_line": 1, "max_lines": 200 }),
                &ctx,
            )
            .await
            .unwrap();

        // Editing line 300 was never observed and must be refused.
        let err = edit(
            &ctx,
            json!({ "path": "big.txt", "search": "line 300", "replace": "line 300 edited" }),
        )
        .await
        .expect_err("edit outside the read range must fail");
        let msg = err.to_string();
        assert!(msg.contains("never read"), "unexpected error: {msg}");
        // The error must be directly actionable: it names the exact recovery call.
        assert!(msg.contains("start_line=300"), "missing recovery hint: {msg}");
        assert!(msg.contains("1-200"), "should report observed range: {msg}");
        // File must be untouched.
        let after = std::fs::read_to_string(dir.path().join("big.txt")).unwrap();
        assert_eq!(after, body);
    }

    #[tokio::test]
    async fn edit_inside_observed_range_is_allowed() {
        let dir = TempDir::new().unwrap();
        let body: String = (1..=400).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.path().join("big.txt"), &body).unwrap();
        let ctx = ctx(&dir);

        ReadFileTool
            .execute(
                json!({ "path": "big.txt", "start_line": 1, "max_lines": 200 }),
                &ctx,
            )
            .await
            .unwrap();

        edit(
            &ctx,
            json!({ "path": "big.txt", "search": "line 150", "replace": "line 150 edited" }),
        )
        .await
        .expect("edit within the observed range should succeed");

        let after = std::fs::read_to_string(dir.path().join("big.txt")).unwrap();
        assert!(after.contains("line 150 edited"));
    }

    #[tokio::test]
    async fn successive_reads_accumulate_coverage() {
        let dir = TempDir::new().unwrap();
        let body: String = (1..=400).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.path().join("big.txt"), &body).unwrap();
        let ctx = ctx(&dir);

        // Page through the file in two reads covering disjoint windows.
        for start in [1, 201] {
            ReadFileTool
                .execute(
                    json!({ "path": "big.txt", "start_line": start, "max_lines": 200 }),
                    &ctx,
                )
                .await
                .unwrap();
        }

        // Line 300 is now covered by the second read.
        edit(
            &ctx,
            json!({ "path": "big.txt", "search": "line 300", "replace": "line 300 edited" }),
        )
        .await
        .expect("edit should be allowed once the range has been read");

        let after = std::fs::read_to_string(dir.path().join("big.txt")).unwrap();
        assert!(after.contains("line 300 edited"));
    }

    #[tokio::test]
    async fn small_file_read_grants_full_coverage() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("small.txt"), "alpha\nbeta\ngamma\n").unwrap();
        let ctx = ctx(&dir);

        // Whole-file read: any line may be edited.
        read_all(&ctx, "small.txt").await;
        edit(
            &ctx,
            json!({ "path": "small.txt", "search": "gamma", "replace": "delta" }),
        )
        .await
        .expect("whole-file read should authorize any edit");

        let after = std::fs::read_to_string(dir.path().join("small.txt")).unwrap();
        assert_eq!(after, "alpha\nbeta\ndelta\n");
    }

    // --- 2. replace_all semantics ---

    #[tokio::test]
    async fn single_match_replaces_without_replace_all() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "keep\ntarget\nkeep\n").unwrap();
        let ctx = ctx(&dir);
        read_all(&ctx, "a.txt").await;

        let result = edit(
            &ctx,
            json!({ "path": "a.txt", "search": "target", "replace": "changed" }),
        )
        .await
        .expect("unique match should succeed");

        assert!(result.content.contains("Replaced 1 occurrence in"));
        let after = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(after, "keep\nchanged\nkeep\n");
    }

    #[tokio::test]
    async fn multi_match_without_replace_all_reports_count_and_suggests_flag() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old\nmid\nold\nend\nold\n").unwrap();
        let ctx = ctx(&dir);
        read_all(&ctx, "a.txt").await;

        let err = edit(
            &ctx,
            json!({ "path": "a.txt", "search": "old", "replace": "new" }),
        )
        .await
        .expect_err("non-unique match must fail without replace_all");

        let msg = err.to_string();
        assert!(msg.contains("matched 3 locations"), "missing count: {msg}");
        assert!(msg.contains("replace_all=true"), "missing suggestion: {msg}");
        // Nothing should have been written.
        let after = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(after, "old\nmid\nold\nend\nold\n");
    }

    #[tokio::test]
    async fn multi_match_with_replace_all_rewrites_every_occurrence() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old\nmid\nold\nend\nold\n").unwrap();
        let ctx = ctx(&dir);
        read_all(&ctx, "a.txt").await;

        let result = edit(
            &ctx,
            json!({ "path": "a.txt", "search": "old", "replace": "new", "replace_all": true }),
        )
        .await
        .expect("replace_all should succeed on multiple matches");

        assert!(result.content.contains("Replaced 3 occurrences in"));
        let after = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(after, "new\nmid\nnew\nend\nnew\n");
    }

    #[tokio::test]
    async fn replace_all_defaults_to_false_preserving_legacy_behavior() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "dup\ndup\n").unwrap();
        let ctx = ctx(&dir);
        read_all(&ctx, "a.txt").await;

        // Explicit false and omitted must behave identically.
        let omitted = edit(
            &ctx,
            json!({ "path": "a.txt", "search": "dup", "replace": "x" }),
        )
        .await;
        let explicit = edit(
            &ctx,
            json!({ "path": "a.txt", "search": "dup", "replace": "x", "replace_all": false }),
        )
        .await;
        assert!(omitted.is_err() && explicit.is_err());
    }

    // --- 3. BOM / CRLF fidelity ---

    #[test]
    fn detect_splits_bom_and_crlf_from_body() {
        let (f, body) = FileFidelity::detect("\u{feff}a\r\nb\r\n");
        assert!(f.bom && f.crlf);
        assert_eq!(body, "a\nb\n");

        let (f, body) = FileFidelity::detect("a\nb\n");
        assert!(!f.bom && !f.crlf);
        assert_eq!(body, "a\nb\n");
    }

    #[test]
    fn restore_is_inverse_of_detect() {
        for original in ["a\r\nb\r\n", "\u{feff}a\r\nb\r\n", "a\nb\n", "\u{feff}x\ny\n"] {
            let (f, body) = FileFidelity::detect(original);
            assert_eq!(f.restore(&body), original, "roundtrip failed for {original:?}");
        }
    }

    #[test]
    fn restore_does_not_double_convert_crlf_in_replacement() {
        let f = FileFidelity {
            bom: false,
            crlf: true,
        };
        // Replacement text that already contains CRLF must not become CRCRLF.
        assert_eq!(f.restore("a\r\nb"), "a\r\nb");
    }

    #[tokio::test]
    async fn crlf_file_stays_crlf_after_edit() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("crlf.txt");
        std::fs::write(&path, "alpha\r\nbeta\r\ngamma\r\n").unwrap();
        let ctx = ctx(&dir);
        read_all(&ctx, "crlf.txt").await;

        edit(
            &ctx,
            json!({ "path": "crlf.txt", "search": "beta", "replace": "BETA" }),
        )
        .await
        .expect("edit should succeed");

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, "alpha\r\nBETA\r\ngamma\r\n");
        // No stray bare LF was introduced anywhere.
        assert_eq!(after.matches('\n').count(), after.matches("\r\n").count());
    }

    #[tokio::test]
    async fn bom_is_preserved_after_edit() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bom.txt");
        std::fs::write(&path, "\u{feff}alpha\nbeta\n").unwrap();
        let ctx = ctx(&dir);
        read_all(&ctx, "bom.txt").await;

        edit(
            &ctx,
            json!({ "path": "bom.txt", "search": "beta", "replace": "BETA" }),
        )
        .await
        .expect("edit should succeed");

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "BOM must be preserved");
        assert_eq!(String::from_utf8(bytes).unwrap(), "\u{feff}alpha\nBETA\n");
    }

    #[tokio::test]
    async fn crlf_edit_touches_only_the_edited_line() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("many.txt");
        let before: String = (1..=50).map(|i| format!("line {i}\r\n")).collect();
        std::fs::write(&path, &before).unwrap();
        let ctx = ctx(&dir);
        read_all(&ctx, "many.txt").await;

        edit(
            &ctx,
            json!({ "path": "many.txt", "search": "line 25", "replace": "line 25 edited" }),
        )
        .await
        .expect("edit should succeed");

        let after = std::fs::read_to_string(&path).unwrap();
        let expected = before.replace("line 25\r\n", "line 25 edited\r\n");
        // Byte-for-byte identical apart from the single edited line: no
        // whole-file line-ending churn.
        assert_eq!(after, expected);
    }

    // --- write_file read-before-overwrite enforcement (#695) ---

    async fn write(ctx: &ToolContext, input: Value) -> Result<ToolResult, ToolError> {
        WriteFileTool.execute(input, ctx).await
    }

    #[tokio::test]
    async fn write_file_rejects_blind_overwrite_of_existing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("existing.txt");
        std::fs::write(&path, "precious original contents\n").unwrap();
        let ctx = ctx(&dir);

        let err = write(
            &ctx,
            json!({ "path": "existing.txt", "content": "clobbered\n" }),
        )
        .await
        .expect_err("overwriting an unread file must be refused");

        let msg = err.to_string();
        assert!(msg.contains("write_file"), "{msg}");
        assert!(msg.contains("has not been read"), "{msg}");
        assert!(msg.contains("never_read"), "{msg}");
        // The refusal must be total: the original bytes survive untouched.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "precious original contents\n",
        );
    }

    #[tokio::test]
    async fn write_file_allows_creating_a_new_file_without_a_prior_read() {
        let dir = TempDir::new().unwrap();
        let ctx = ctx(&dir);

        // There is nothing to have read: creation must not be gated.
        write(&ctx, json!({ "path": "brand_new.txt", "content": "hello\n" }))
            .await
            .expect("creating a new file must be allowed");

        assert_eq!(
            std::fs::read_to_string(dir.path().join("brand_new.txt")).unwrap(),
            "hello\n",
        );
    }

    #[tokio::test]
    async fn write_file_allows_overwrite_after_reading() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("existing.txt");
        std::fs::write(&path, "original\n").unwrap();
        let ctx = ctx(&dir);

        read_all(&ctx, "existing.txt").await;
        write(
            &ctx,
            json!({ "path": "existing.txt", "content": "replacement\n" }),
        )
        .await
        .expect("overwrite after a fresh read must be allowed");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement\n");
    }

    #[tokio::test]
    async fn write_file_rejects_overwrite_when_file_changed_after_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("racy.txt");
        std::fs::write(&path, "first\n").unwrap();
        let ctx = ctx(&dir);

        read_all(&ctx, "racy.txt").await;
        // A concurrent writer changes the file behind our back.
        std::fs::write(&path, "changed by someone else\n").unwrap();

        let err = write(&ctx, json!({ "path": "racy.txt", "content": "mine\n" }))
            .await
            .expect_err("a stale read must not authorize an overwrite");

        let msg = err.to_string();
        assert!(msg.contains("stale_content"), "{msg}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "changed by someone else\n",
        );
    }

    #[tokio::test]
    async fn write_file_detects_same_length_change_after_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("samelen.txt");
        std::fs::write(&path, "aaaa\n").unwrap();
        let ctx = ctx(&dir);

        read_all(&ctx, "samelen.txt").await;
        // Same byte length as before, so `len` alone cannot distinguish it.
        // Detection relies on the content hash added for #695 gap 2.
        std::fs::write(&path, "bbbb\n").unwrap();

        let err = write(&ctx, json!({ "path": "samelen.txt", "content": "cccc\n" }))
            .await
            .expect_err("same-length external change must still be detected");
        assert!(err.to_string().contains("stale_content"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "bbbb\n");
    }
}
