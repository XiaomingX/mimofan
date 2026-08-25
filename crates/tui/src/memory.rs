//! User memory: categorized index + on-demand category files.
//!
//! v0.9.0 replaces the single-file `memory.md` with a CodeBuddy-style layout:
//!
//! - `~/.mimofan/memory/` (configurable via `memory_dir` in `config.toml`
//!   and `MIMOFAN_MEMORY_DIR` env) holds:
//!   - `MEMORY.md` — a small **index** that is ALWAYS injected into the
//!     system prompt. It stays tiny, so prompt-prefix caching is unaffected.
//!   - `user.md`, `feedback.md`, `project.md`, `reference.md` — category
//!     files holding durable notes. These are NOT auto-injected; the model
//!     reads them on demand via its Read tool when relevant.
//! - Quick capture `# foo` and the `remember` tool append a timestamped
//!   bullet to the chosen category file and refresh the index.
//! - A legacy `~/.mimofan/memory.md` is migrated into `project.md` once
//!   (see [`migrate_legacy`]).
//!
//! Default behavior is **enabled**; opt out with `[memory] enabled = false`
//! or `MIMOFAN_MEMORY=off`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::Utc;
use regex::Regex;

// `MemoryCategory` is the four-way long-term memory classification
// (user / feedback / project / reference). It is declared locally here as the
// authoritative `crate::memory::MemoryCategory` so the tui crate owns the type
// and keeps call sites stable; `mimofan_memory::category` provides the
// mirrored implementation for the storage layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryCategory {
    /// Preferences, facts, and context about the user.
    User,
    /// Corrections, guidance, and how-to instructions.
    Feedback,
    /// Project-scoped state, decisions, and todos.
    Project,
    /// External references (docs, APIs, pointers).
    Reference,
}

impl MemoryCategory {
    /// All categories, in canonical order.
    pub const ALL: &'static [MemoryCategory] = &[
        MemoryCategory::User,
        MemoryCategory::Feedback,
        MemoryCategory::Project,
        MemoryCategory::Reference,
    ];

    /// Stable string form (lowercase), used for file names and serialization.
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryCategory::User => "user",
            MemoryCategory::Feedback => "feedback",
            MemoryCategory::Project => "project",
            MemoryCategory::Reference => "reference",
        }
    }

    /// Parse a category from a string (case-insensitive).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "user" => Some(MemoryCategory::User),
            "feedback" => Some(MemoryCategory::Feedback),
            "project" => Some(MemoryCategory::Project),
            "reference" => Some(MemoryCategory::Reference),
            _ => None,
        }
    }
}
// #732/#659: re-export the cross-session `UserProfile` API so the engine can
// inject/distill it via `crate::memory::UserProfile` / `inject_user_profile` /
// `distill_session` (consistent with the `MemoryCategory` re-export above).
pub use mimofan_memory::{UserProfile, distill_session, inject_user_profile};

/// 兼容旧调用点的分类名列表（由 [`MemoryCategory`] 派生）。
pub const CATEGORIES: &[&str] = &["user", "feedback", "project", "reference"];

/// 默认分类：当 `# foo` 快速捕获未带显式前缀时使用。
pub const DEFAULT_CATEGORY: &str = "project";

/// Cap on the injected index size. Indexes stay tiny, but we still truncate
/// and mark it like the old single-file memory did, to be safe.
const MAX_INDEX_SIZE: usize = 8 * 1024;

/// Path to the index file inside `dir`.
#[must_use]
pub fn index_path(dir: &Path) -> PathBuf {
    dir.join("MEMORY.md")
}

/// Path to a category file inside `dir`.
#[must_use]
pub fn category_path(dir: &Path, category: &str) -> PathBuf {
    dir.join(format!("{category}.md"))
}

/// Whether `category` is a known memory category.
#[must_use]
pub fn is_category(category: &str) -> bool {
    MemoryCategory::from_str(category).is_some()
}

/// Create the memory directory and the four (empty) category files when
/// missing. Idempotent.
pub fn ensure_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for cat in CATEGORIES {
        let path = category_path(dir, cat);
        if !path.exists() {
            fs::write(&path, "")?;
        }
    }
    Ok(())
}

/// One-time, idempotent migration of the legacy single-file memory
/// (`~/.mimofan/memory.md`) into the directory layout. Legacy content seeds
/// `project.md`; the index is (re)built. A missing/empty legacy file only
/// ensures a consistent directory + index. Existing category content is
/// never clobbered. The legacy file is left in place (archival).
pub fn migrate_legacy(legacy_path: &Path, dir: &Path) -> io::Result<()> {
    if !legacy_path.exists() {
        return Ok(());
    }
    let legacy = fs::read_to_string(legacy_path)?;
    ensure_dir(dir)?;
    if !legacy.trim().is_empty() {
        let project = category_path(dir, "project");
        let existing = fs::read_to_string(&project).unwrap_or_default();
        if existing.trim().is_empty() {
            fs::write(&project, legacy)?;
        }
    }
    write_index(dir)?;
    Ok(())
}

/// Read the index file, returning `None` when missing or empty.
#[must_use]
pub fn load_index(dir: &Path) -> Option<String> {
    let content = fs::read_to_string(index_path(dir)).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    Some(content)
}

/// Build the index (`MEMORY.md`) content from the current category files.
/// Each non-empty category gets a concise pointer plus a short summary
/// derived from its first non-empty line, so the model knows what to read.
#[must_use]
pub fn build_index(dir: &Path) -> String {
    let mut out = String::from(
        "# Memory Index\n\n\
         Concise pointers to durable memory. Load category details on demand \
         with the Read tool (paths below). Keep entries declarative.\n\n",
    );
    for cat in CATEGORIES {
        let path = category_path(dir, cat);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }
        let summary: String = trimmed
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .chars()
            .take(80)
            .collect();
        let count = trimmed.lines().filter(|l| !l.trim().is_empty()).count();
        let desc = if summary.is_empty() {
            format!("{count} entries")
        } else {
            format!("{summary}  ({count} entries)")
        };
        out.push_str(&format!("- [{cat}]({cat}.md) — {desc}\n"));
    }
    out
}

/// Persist a freshly built index to `MEMORY.md`.
pub fn write_index(dir: &Path) -> io::Result<()> {
    fs::write(index_path(dir), build_index(dir))
}

/// Inline `<!-- paths: ... -->` tag on a memory bullet line. When present,
/// the bullet is only injected when the session's active file paths match one
/// of the globs (see [`paths_match`]). Returns the cleaned bullet text and the
/// optional glob list.
///
/// `# foo` quick-adds strip only a leading `#`; a trailing paths tag is kept
/// verbatim so callers can opt into conditional injection.
#[must_use]
pub fn split_paths_tag(line: &str) -> (String, Option<Vec<String>>) {
    let re = paths_tag_regex();
    if let Some(m) = re.find(line) {
        let tag = &line[m.start()..m.end()];
        // Extract the comma/space separated globs inside `<!-- paths: ... -->`.
        let inner = tag
            .trim_start_matches("<!--")
            .trim_end_matches("-->")
            .trim()
            .trim_start_matches("paths:")
            .trim();
        let globs: Vec<String> = inner
            .split([',', '\n'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let text = format!("{}{}", &line[..m.start()], &line[m.end()..])
            .trim()
            .to_string();
        let globs = if globs.is_empty() { None } else { Some(globs) };
        (text, globs)
    } else {
        (line.trim().to_string(), None)
    }
}

/// Whether any of `active` paths matches any of the `globs` (case-insensitive).
/// Globs use `*` (and `**`) as a wildcard matched as an ordered-substring
/// pattern, so both `src/api/*.ts` and `src/api/**/*.ts` work without a
/// dedicated glob crate. Returns `true` when `globs` is empty (always-apply
/// default).
#[must_use]
pub fn paths_match(globs: &[String], active: &[String]) -> bool {
    if globs.is_empty() {
        return true;
    }
    active.iter().any(|p| {
        let path = p.to_ascii_lowercase();
        globs.iter().any(|g| glob_matches(&path, g))
    })
}

/// Ordered-substring glob match: split the glob on `*` and require each
/// literal segment to appear in the path in order. `**` is normalized to `*`.
fn glob_matches(path: &str, glob: &str) -> bool {
    let norm_glob = glob.replace("**", "*");
    let parts: Vec<&str> = norm_glob.split('*').collect();
    if parts.len() == 1 {
        return path.contains(parts[0]);
    }
    let mut cursor = 0;
    for part in parts.iter().filter(|p| !p.is_empty()) {
        match path[cursor..].find(part) {
            Some(idx) => cursor += idx + part.len(),
            None => return false,
        }
    }
    true
}

fn paths_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)<!--\s*paths:\s*[^>]*-->").expect("paths tag regex should compile")
    })
}

/// Strip a leading `(timestamp)` prefix from a bullet body, leaving the
/// declarative content. Used when inlining path-scoped bullets so the model
/// sees the fact, not the bookkeeping.
fn strip_timestamp(body: &str) -> String {
    let re = timestamp_regex();
    re.replace(body, "").trim().to_string()
}

fn timestamp_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\([^)]*\)\s*").expect("timestamp regex should compile"))
}

/// Compose the `<user_memory_index>` block for the system prompt, honouring
/// the opt-in toggle. Returns `None` when disabled, the directory is missing,
/// or the index is empty — so callers don't need to check both conditions.
///
/// When `active_paths` is provided, bullets carrying a `<!-- paths: ... -->`
/// tag whose globs match one of the active paths are inlined into a
/// `<memory_paths_matches>` segment, so path-relevant memory surfaces without
/// the model having to Read the category file. Bullets without a tag are not
/// inlined (still read on demand). Passing `None` keeps the historic behaviour
/// (index pointers only), preserving the stable KV-cache prefix.
#[must_use]
pub fn compose_index_block(
    enabled: bool,
    dir: &Path,
    active_paths: Option<&[String]>,
) -> Option<String> {
    if !enabled {
        return None;
    }
    let content = load_index(dir)?;
    let display = index_path(dir).display().to_string();
    let payload = if content.len() > MAX_INDEX_SIZE {
        let cutoff = previous_char_boundary(&content, MAX_INDEX_SIZE);
        let omitted = content.len() - cutoff;
        let mut head = content[..cutoff].to_string();
        head.push_str(&format!(
            "\n<truncated bytes={omitted} source=\"{display}\">"
        ));
        head
    } else {
        content
    };

    let mut block = format!("<user_memory_index source=\"{display}\">\n{payload}");

    // Conditionally inline path-scoped bullets when we know the active paths.
    if let Some(active) = active_paths
        && !active.is_empty()
    {
        let mut matches_lines = Vec::new();
        for cat in CATEGORIES {
            let Some(file_content) = read_category(dir, cat) else {
                continue;
            };
            for line in file_content.lines() {
                let line = line.trim();
                if !line.starts_with("- ") {
                    continue;
                }
                let (text, paths) = split_paths_tag(line);
                if let Some(globs) = paths
                    && paths_match(&globs, active)
                {
                    let body = strip_timestamp(text.trim_start_matches("- ").trim());
                    matches_lines.push(format!("- [{cat}] {body}"));
                }
            }
        }
        if !matches_lines.is_empty() {
            block.push_str("\n\n<memory_paths_matches>\n");
            block.push_str(&matches_lines.join("\n"));
            block.push_str("\n</memory_paths_matches>");
        }
    }

    block.push_str("\n</user_memory_index>");
    Some(block)
}

/// Append `entry` to the given category file (creating the dir + file),
/// timestamped, then refresh the index. Errors on an unknown category or
/// I/O failure.
pub fn append_entry(dir: &Path, category: &str, entry: &str) -> io::Result<()> {
    if !is_category(category) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unknown memory category `{category}` (expected one of: {})",
                CATEGORIES.join(", ")
            ),
        ));
    }
    let trimmed = entry.trim_start_matches('#').trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "memory entry is empty after stripping `#` prefix",
        ));
    }
    ensure_dir(dir)?;
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M UTC");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(category_path(dir, category))?;
    writeln!(file, "- ({timestamp}) {trimmed}")?;
    write_index(dir)?;
    Ok(())
}

/// Read a category file's content for on-demand inspection (e.g. `/memory
/// show <category>`). Returns `None` when the category is unknown or the
/// file is missing/empty.
#[must_use]
pub fn read_category(dir: &Path, category: &str) -> Option<String> {
    if !is_category(category) {
        return None;
    }
    let content = fs::read_to_string(category_path(dir, category)).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    Some(content)
}

/// Delete the first bullet whose text (after stripping the leading
/// `(timestamp)` prefix and surrounding whitespace) contains `matcher` as a
/// substring. Returns `true` when a line was removed. Refreshes the index.
/// Errors on an unknown category or I/O failure.
pub fn remove_entry(dir: &Path, category: &str, matcher: &str) -> io::Result<bool> {
    if !is_category(category) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unknown memory category `{category}` (expected one of: {})",
                CATEGORIES.join(", ")
            ),
        ));
    }
    let path = category_path(dir, category);
    let content = fs::read_to_string(&path)?;
    let needle = matcher.trim();
    let mut removed = false;
    let mut kept: Vec<&str> = Vec::new();
    for line in content.lines() {
        if !removed && line.trim_start().starts_with("- ") {
            let body = strip_timestamp(line.trim_start_matches("- ").trim());
            if body.contains(needle) {
                removed = true;
                continue;
            }
        }
        kept.push(line);
    }
    if removed {
        let rewritten = kept.join("\n");
        fs::write(&path, rewritten)?;
        write_index(dir)?;
    }
    Ok(removed)
}

/// Replace the text of the first bullet matching `matcher` with `new_text`.
/// The matched line keeps its `- (timestamp)` prefix but gets a fresh
/// timestamp and the new body. Returns `true` when a line was replaced.
/// Refreshes the index. Errors on an unknown category or I/O failure.
pub fn replace_entry(
    dir: &Path,
    category: &str,
    matcher: &str,
    new_text: &str,
) -> io::Result<bool> {
    if !is_category(category) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unknown memory category `{category}` (expected one of: {})",
                CATEGORIES.join(", ")
            ),
        ));
    }
    let new_text = new_text.trim_start_matches('#').trim();
    if new_text.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "replacement memory text is empty",
        ));
    }
    let path = category_path(dir, category);
    let content = fs::read_to_string(&path)?;
    let needle = matcher.trim();
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M UTC");
    let mut replaced = false;
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        if !replaced && line.trim_start().starts_with("- ") {
            let body = strip_timestamp(line.trim_start_matches("- ").trim());
            if body.contains(needle) {
                out.push(format!("- ({timestamp}) {new_text}"));
                replaced = true;
                continue;
            }
        }
        out.push(line.to_string());
    }
    if replaced {
        fs::write(&path, out.join("\n"))?;
        write_index(dir)?;
    }
    Ok(replaced)
}

/// Parse a `# foo` quick-add into an optional explicit category and the
/// remaining entry text.
///
/// - `# user I prefer Rust` → `(Some("user"), "I prefer Rust")`
/// - `# note` → `(None, "note")` (caller applies the default category)
///
/// The leading `#` is consumed; the entry text is returned without it.
#[must_use]
pub fn parse_quick_add(input: &str) -> (Option<&str>, String) {
    let without_hash = input.trim_start_matches('#').trim();
    let mut parts = without_hash.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();
    if is_category(first) && !rest.is_empty() {
        (Some(first), rest.to_string())
    } else {
        (None, without_hash.to_string())
    }
}

fn previous_char_boundary(value: &str, mut index: usize) -> usize {
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

// ============================================================================
// Structured decision pages (`decisions.md`)
// ----------------------------------------------------------------------------
// Inspired by brain.md's `compiled_truth` (rewritable current understanding)
// + `timeline` (append-only evidence chain, including `reversal`). mimofan's
// existing `project.md` bullets capture scattered facts with no audit trail;
// this layer adds a *decision* abstraction with an immutable history so a
// future session can see *why* a choice was made and *why* it was later
// revised or overturned. It lives alongside the bullet layer and shares the
// same memory directory. Decisions are written atomically (whole-file
// rewrite) so a crash mid-write never leaves a half-edited entry.
// ============================================================================

/// Kind of event in a decision's [`DecisionEntry::history`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionEventKind {
    /// Initial decision captured via [`decision_create`].
    Decision,
    /// Current understanding rewritten via [`decision_revise`].
    Revision,
    /// Decision overturned via [`decision_reverse`] (entry is kept, not deleted).
    Reversal,
}

impl DecisionEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionEventKind::Decision => "Decision",
            DecisionEventKind::Revision => "Revision",
            DecisionEventKind::Reversal => "Reversal",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Decision" => Some(DecisionEventKind::Decision),
            "Revision" => Some(DecisionEventKind::Revision),
            "Reversal" => Some(DecisionEventKind::Reversal),
            _ => None,
        }
    }
}

/// One entry in a decision's audit trail. Append-only: revisions and
/// reversals add events but never remove earlier ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionEvent {
    /// RFC3339-ish timestamp, `YYYY-MM-DD HH:MM UTC`.
    pub time: String,
    pub kind: DecisionEventKind,
    /// Human-readable rationale (why made / why changed / why overturned).
    pub summary: String,
    /// Where the decision came from (e.g. "user", "agent", "issue #123").
    pub source: String,
}

/// A single durable decision with its rewritable `current` understanding and
/// its immutable [`history`](DecisionEntry::history) audit trail. Mirrors
/// brain.md's `compiled_truth` (the `current` field) + `timeline` (history).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionEntry {
    /// Stable opaque id (slug or short hash). Used as the lookup key.
    pub id: String,
    /// Short title for the decision.
    pub title: String,
    /// The current best understanding. Rewritten on revision; frozen (and
    /// marked reversed) on reversal.
    pub current: String,
    /// Free-form category / tag for grouping (e.g. "architecture", "policy").
    pub category: String,
    /// Creation timestamp.
    pub created: String,
    /// Last-mutation timestamp.
    pub updated: String,
    /// When `true`, the decision was overturned and `current` is frozen.
    pub reversed: bool,
    /// Append-only audit trail.
    pub history: Vec<DecisionEvent>,
}

/// Path to `decisions.md` inside `dir`.
#[must_use]
pub fn decisions_path(dir: &Path) -> PathBuf {
    dir.join("decisions.md")
}

/// Capture a new decision. Returns an error if `id` already exists (use
/// [`decision_revise`] to change an existing one). The initial event is a
/// `Decision` entry in the history.
pub fn decision_create(
    dir: &Path,
    id: &str,
    title: &str,
    category: &str,
    current: &str,
) -> io::Result<()> {
    let id = id.trim();
    let current = current.trim();
    if id.is_empty() || current.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "decision `id` and `current` must be non-empty",
        ));
    }
    ensure_dir(dir)?;
    let mut entries = read_decisions(dir);
    if entries.iter().any(|e| e.id == id) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("decision `{id}` already exists; use revise/reverse to change it"),
        ));
    }
    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let entry = DecisionEntry {
        id: id.to_string(),
        title: title.trim().to_string(),
        current: current.to_string(),
        category: category.trim().to_string(),
        created: now.clone(),
        updated: now.clone(),
        reversed: false,
        history: vec![DecisionEvent {
            time: now,
            kind: DecisionEventKind::Decision,
            summary: current.to_string(),
            source: "agent".to_string(),
        }],
    };
    entries.push(entry);
    write_decisions_atomic(dir, &entries)
}

/// Rewrite a decision's `current` understanding and append a `Revision` event
/// recording `why`. Returns `Ok(false)` when the id is unknown. No-op (and
/// `Ok(false)`) when the decision is already reversed — reversals are final.
pub fn decision_revise(dir: &Path, id: &str, new_current: &str, why: &str) -> io::Result<bool> {
    let new_current = new_current.trim();
    if new_current.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "decision revision text must be non-empty",
        ));
    }
    let mut entries = read_decisions(dir);
    let Some(pos) = entries.iter().position(|e| e.id == id) else {
        return Ok(false);
    };
    if entries[pos].reversed {
        return Ok(false);
    }
    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    entries[pos].current = new_current.to_string();
    entries[pos].updated = now.clone();
    entries[pos].history.push(DecisionEvent {
        time: now,
        kind: DecisionEventKind::Revision,
        summary: why.trim().to_string(),
        source: "agent".to_string(),
    });
    write_decisions_atomic(dir, &entries)?;
    Ok(true)
}

/// Overturn a decision: append a `Reversal` event recording `why`, mark the
/// entry `reversed`, and freeze `current` (it is NOT deleted, preserving the
/// evidence chain). Returns `Ok(false)` when the id is unknown. Reversing an
/// already-reversed decision is a no-op `Ok(false)`.
pub fn decision_reverse(dir: &Path, id: &str, why: &str) -> io::Result<bool> {
    let mut entries = read_decisions(dir);
    let Some(pos) = entries.iter().position(|e| e.id == id) else {
        return Ok(false);
    };
    if entries[pos].reversed {
        return Ok(false);
    }
    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    entries[pos].reversed = true;
    entries[pos].updated = now.clone();
    entries[pos].history.push(DecisionEvent {
        time: now,
        kind: DecisionEventKind::Reversal,
        summary: why.trim().to_string(),
        source: "agent".to_string(),
    });
    write_decisions_atomic(dir, &entries)?;
    Ok(true)
}

/// Read all decision entries from `decisions.md`. Returns an empty vec when
/// the file is missing or unparseable (fail-soft — never errors).
#[must_use]
pub fn read_decisions(dir: &Path) -> Vec<DecisionEntry> {
    let content = match fs::read_to_string(decisions_path(dir)) {
        Ok(c) if !c.trim().is_empty() => c,
        _ => return Vec::new(),
    };
    parse_decisions(&content)
}

/// Parse `decisions.md` content into entries. Tolerant: malformed entries are
/// skipped rather than aborting the whole file.
#[must_use]
pub fn parse_decisions(content: &str) -> Vec<DecisionEntry> {
    let mut entries = Vec::new();
    let mut current: Option<DecisionEntry> = None;
    let mut in_history = false;

    for raw_line in content.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim();

        // Heading `# <title>  [<category>]  (id: <id>)  _(reversed)_` starts a
        // new entry.
        if let Some(rest) = trimmed.strip_prefix("# ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            in_history = false;
            let (title_part, id, category, reversed) = parse_decision_heading(rest);
            current = Some(DecisionEntry {
                id,
                title: title_part,
                current: String::new(),
                category,
                created: String::new(),
                updated: String::new(),
                reversed,
                history: Vec::new(),
            });
            continue;
        }

        let Some(entry) = current.as_mut() else {
            continue;
        };

        if let Some(rest) = trimmed.strip_prefix("## History") {
            in_history = true;
            let _ = rest;
            continue;
        }
        if trimmed.starts_with("## ") {
            // Any other H2 closes history but we keep reading the body.
            in_history = false;
            continue;
        }

        if in_history {
            if let Some(ev) = parse_history_line(trimmed) {
                entry.history.push(ev);
            }
            continue;
        }

        // Body line of `current` understanding (everything before History).
        // Skip the `_created:` / `_updated:` metadata lines we emit.
        if !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("_created:")
            && !trimmed.starts_with("_updated:")
        {
            if !entry.current.is_empty() {
                entry.current.push('\n');
            }
            entry.current.push_str(trimmed);
        }
    }

    if let Some(entry) = current.take() {
        entries.push(entry);
    }
    entries.retain(|e| !e.id.is_empty());
    entries
}

/// Render entries to `decisions.md` Markdown. Human-readable and git-trackable.
#[must_use]
pub fn render_decisions(entries: &[DecisionEntry]) -> String {
    let mut out = String::from(
        "# Decision Log\n\n\
         Durable decisions with an audit trail (current understanding + why \
         revised/overturned). Append-only history; reversals keep the entry.\n",
    );
    for e in entries {
        let status = if e.reversed { "  _(reversed)_" } else { "" };
        let cat = if e.category.is_empty() {
            String::new()
        } else {
            format!("  [{category}]", category = e.category)
        };
        out.push_str(&format!(
            "\n# {title}{cat}  (id: {id}){status}\n\n",
            title = e.title,
            id = e.id,
        ));
        if !e.current.is_empty() {
            out.push_str(e.current.trim());
            out.push('\n');
        }
        if !e.created.is_empty() {
            out.push_str(&format!("\n_created: {}_", e.created));
        }
        if !e.updated.is_empty() {
            out.push_str(&format!("\n_updated: {}_", e.updated));
        }
        out.push_str("\n\n## History\n");
        for ev in &e.history {
            out.push_str(&format!(
                "- time: {}  kind: {}  summary: {}  source: {}\n",
                ev.time,
                ev.kind.as_str(),
                ev.summary,
                ev.source,
            ));
        }
    }
    out
}

/// Compose the `<decision_brief>` block for the system prompt. Surfaces the
/// most recent durable decisions (with their current understanding and a
/// one-line why-trail) so a new session starts with the project's settled
/// choices in context — brain.md's SessionStart injection, mimofan-style.
///
/// Returns `None` when memory is disabled, the directory is missing, or there
/// are no decisions — callers inject nothing rather than failing. `limit`
/// bounds how many decisions are inlined so a large log can't blow the
/// context window.
#[must_use]
pub fn compose_decision_block(enabled: bool, dir: &Path, limit: usize) -> Option<String> {
    if !enabled {
        return None;
    }
    let mut entries = read_decisions(dir);
    if entries.is_empty() {
        return None;
    }
    // Most-recently-updated first; reversed ones still surface (they explain
    // why a past choice no longer holds).
    entries.sort_by(|a, b| b.updated.cmp(&a.updated));
    let display = decisions_path(dir).display().to_string();
    let mut block = format!("<decision_brief source=\"{display}\">\n");
    let take = if limit == 0 {
        entries.len()
    } else {
        limit.min(entries.len())
    };
    for e in &entries[..take] {
        let status = if e.reversed { " _(reversed)_" } else { "" };
        block.push_str(&format!("- [{id}]{status}: ", id = e.id));
        let current_one_line = e.current.lines().next().unwrap_or("").trim().to_string();
        block.push_str(&current_one_line);
        // Append the latest rationale (last history event) when present.
        if let Some(last) = e.history.last() {
            block.push_str(&format!("  (why: {})", last.summary));
        }
        block.push('\n');
    }
    block.push_str("</decision_brief>");
    Some(block)
}

/// Atomic write: render to a temp file in `dir`, then rename over the target
/// so a crash mid-write never corrupts `decisions.md`.
fn write_decisions_atomic(dir: &Path, entries: &[DecisionEntry]) -> io::Result<()> {
    ensure_dir(dir)?;
    let target = decisions_path(dir);
    let tmp = dir.join(format!(".decisions.{}.tmp", std::process::id()));
    fs::write(&tmp, render_decisions(entries))?;
    fs::rename(&tmp, &target)?;
    Ok(())
}

/// Parse a decision heading line into `(title, id, category, reversed)`.
fn parse_decision_heading(rest: &str) -> (String, String, String, bool) {
    // Reversed entries carry a trailing `_(reversed)_` marker.
    let reversed = rest.contains("_(reversed)_");
    let rest = rest.replace("_(reversed)_", "").trim().to_string();
    // Split off `(id: ...)` suffix first.
    let (title_cat, id) = if let Some(idx) = rest.find("(id:") {
        let id = rest[idx + 5..].trim_end_matches(')').trim().to_string();
        (rest[..idx].trim().to_string(), id)
    } else {
        (rest.trim().to_string(), String::new())
    };
    // Then split off `[category]` if present.
    let (title, category) = if let Some(open) = title_cat.find('[') {
        if let Some(close) = title_cat[open..].find(']') {
            let category = title_cat[open + 1..open + close].trim().to_string();
            let title = title_cat[..open].trim().to_string();
            (title, category)
        } else {
            (title_cat, String::new())
        }
    } else {
        (title_cat, String::new())
    };
    (title, id, category, reversed)
}

/// Parse a `- time: ... kind: ... summary: ... source: ...` history line.
fn parse_history_line(line: &str) -> Option<DecisionEvent> {
    let body = line.trim_start_matches('-').trim();
    if !body.starts_with("time:") {
        return None;
    }
    let time = field(body, "time:")?;
    let kind_str = field(body, "kind:")?;
    let kind = DecisionEventKind::from_str(kind_str)?;
    let summary = field(body, "summary:").unwrap_or_default().to_string();
    let source = field(body, "source:").unwrap_or_default().to_string();
    Some(DecisionEvent {
        time: time.trim().to_string(),
        kind,
        summary: summary.trim().to_string(),
        source: source.trim().to_string(),
    })
}

/// Extract the value following `key` in a free-form `key: val key2: val2` line.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start();
    // Value ends at the next ` key:` boundary (a recognized field key).
    let end = ["time:", "kind:", "summary:", "source:"]
        .iter()
        .filter_map(|k| if *k == key { None } else { rest.find(*k) })
        .min();
    match end {
        Some(e) => Some(rest[..e].trim()),
        None => Some(rest.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_memory_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "mimofan-memory-test-{}-{}-{}",
            std::process::id(),
            nanos,
            seq
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn append_and_index() {
        let dir = tmp_memory_dir();
        append_entry(&dir, "user", "prefers Rust").unwrap();
        append_entry(&dir, "project", "uses pytest").unwrap();
        let index = load_index(&dir).expect("index built");
        assert!(index.contains("[user](user.md)"));
        assert!(index.contains("[project](project.md)"));
        assert!(index.contains("prefers Rust"));
        let user = read_category(&dir, "user").expect("user file");
        assert!(user.contains("prefers Rust"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_category_errors() {
        let dir = tmp_memory_dir();
        assert!(append_entry(&dir, "bogus", "x").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_quick_add_categories() {
        assert_eq!(
            parse_quick_add("# user I prefer Rust"),
            (Some("user"), "I prefer Rust".to_string())
        );
        assert_eq!(parse_quick_add("# note"), (None, "note".to_string()));
        assert_eq!(
            parse_quick_add("# feedback  use tabs"),
            (Some("feedback"), "use tabs".to_string())
        );
    }

    #[test]
    fn legacy_migration_seeds_project() {
        let dir = tmp_memory_dir();
        let legacy = std::env::temp_dir().join(format!("mimofan-legacy-{}.md", std::process::id()));
        fs::write(&legacy, "- old note one\n- old note two\n").unwrap();
        migrate_legacy(&legacy, &dir).unwrap();
        let project = read_category(&dir, "project").expect("project seeded");
        assert!(project.contains("old note one"));
        assert!(load_index(&dir).is_some());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&legacy);
    }

    #[test]
    fn memory_category_parsing() {
        use MemoryCategory::*;
        assert_eq!(MemoryCategory::from_str("user"), Some(User));
        assert_eq!(MemoryCategory::from_str("FEEDBACK"), Some(Feedback));
        assert_eq!(MemoryCategory::from_str("Project"), Some(Project));
        assert_eq!(MemoryCategory::from_str("reference"), Some(Reference));
        assert_eq!(MemoryCategory::from_str("bogus"), None);
        assert_eq!(MemoryCategory::from_str(""), None);
        for cat in MemoryCategory::ALL {
            assert_eq!(MemoryCategory::from_str(cat.as_str()), Some(*cat));
            assert_eq!(cat.as_str(), cat.as_str().to_ascii_lowercase().as_str());
        }
    }

    #[test]
    fn split_paths_tag_parses() {
        let (text, paths) =
            split_paths_tag("- (2026-01-01) API auth uses Bearer <!-- paths: src/api/**/*.ts -->");
        assert_eq!(text, "- (2026-01-01) API auth uses Bearer");
        assert_eq!(paths, Some(vec!["src/api/**/*.ts".to_string()]));

        let (text2, paths2) = split_paths_tag("- plain note");
        assert_eq!(text2, "- plain note");
        assert!(paths2.is_none());

        let (text3, paths3) =
            split_paths_tag("- (ts) multi <!-- paths: src/api/*.ts, **/auth*.ts --> trailing");
        assert!(text3.contains("multi"));
        assert!(text3.contains("trailing"));
        assert_eq!(
            paths3,
            Some(vec!["src/api/*.ts".to_string(), "**/auth*.ts".to_string()])
        );
    }

    #[test]
    fn paths_match_behaviour() {
        let active = vec!["src/api/v1/login.ts".to_string(), "README.md".to_string()];
        // `src/api/**/*.ts` should match a file under src/api.
        assert!(paths_match(&["src/api/**/*.ts".to_string()], &active));
        // `**/auth*.ts` should NOT match the active set (no auth file present).
        assert!(!paths_match(&["**/auth*.ts".to_string()], &active));
        // Non-matching glob.
        assert!(!paths_match(&["tests/**/*.rs".to_string()], &active));
        // Empty globs => always apply.
        assert!(paths_match(&[], &active));
        // Empty active => nothing matches (unless always-apply).
        assert!(!paths_match(&["src/api/**/*.ts".to_string()], &[]));
    }

    #[test]
    fn compose_index_block_inlines_paths_matches() {
        let dir = tmp_memory_dir();
        ensure_dir(&dir);
        append_entry(&dir, "project", "always-on project note").unwrap();
        append_entry(
            &dir,
            "project",
            "API auth uses Bearer <!-- paths: src/api/**/*.ts -->",
        )
        .unwrap();
        append_entry(
            &dir,
            "reference",
            "deploy runbook <!-- paths: deploy/** -->",
        )
        .unwrap();

        // No active paths => only index pointers, no <memory_paths_matches>.
        let block_none = compose_index_block(true, &dir, None).expect("block built");
        assert!(block_none.contains("- [project](project.md)"));
        assert!(!block_none.contains("<memory_paths_matches>"));

        // Active path matches the API glob => its bullet inlined (and only
        // the matched one — the deploy runbook is excluded).
        let active = vec!["src/api/v1/login.ts".to_string()];
        let block = compose_index_block(true, &dir, Some(&active)).expect("block built");
        assert!(block.contains("<memory_paths_matches>"));
        let matches_seg = block
            .split("<memory_paths_matches>")
            .nth(1)
            .unwrap()
            .split("</memory_paths_matches>")
            .next()
            .unwrap();
        assert!(matches_seg.contains("[project] API auth uses Bearer"));
        assert!(!matches_seg.contains("[reference] deploy runbook"));
        assert!(!matches_seg.contains("<!-- paths:"));
        // The always-on bullet has no paths tag, so it is NOT inlined.
        assert!(!matches_seg.contains("always-on project note"));
    }

    #[test]
    fn remove_entry_deletes_matching_bullet() {
        let dir = tmp_memory_dir();
        ensure_dir(&dir);
        append_entry(&dir, "feedback", "use tabs not spaces").unwrap();
        append_entry(&dir, "feedback", "keep PRs small").unwrap();
        // Matches the first bullet by substring; timestamp prefix is ignored.
        assert!(remove_entry(&dir, "feedback", "tabs").unwrap());
        let fb = read_category(&dir, "feedback").expect("feedback file");
        assert!(!fb.contains("tabs"));
        assert!(fb.contains("keep PRs small"));
        // No match => nothing removed, still returns Ok(false).
        assert!(!remove_entry(&dir, "feedback", "nonexistent").unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_entry_updates_matching_bullet() {
        let dir = tmp_memory_dir();
        ensure_dir(&dir);
        append_entry(&dir, "user", "prefers Rust").unwrap();
        assert!(replace_entry(&dir, "user", "Rust", "prefers Go now").unwrap());
        let user = read_category(&dir, "user").expect("user file");
        assert!(user.contains("prefers Go now"));
        assert!(!user.contains("prefers Rust"));
        // Empty replacement is rejected.
        assert!(replace_entry(&dir, "user", "Go", "").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_and_replace_reject_unknown_category() {
        let dir = tmp_memory_dir();
        assert!(remove_entry(&dir, "bogus", "x").is_err());
        assert!(replace_entry(&dir, "bogus", "x", "y").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    // ----- Structured decision pages (`decisions.md`) -----

    #[test]
    fn decision_create_then_read() {
        let dir = tmp_memory_dir();
        decision_create(
            &dir,
            "api-auth",
            "API auth scheme",
            "architecture",
            "Use Bearer tokens for the public API.",
        )
        .unwrap();
        let entries = read_decisions(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "api-auth");
        assert_eq!(entries[0].title, "API auth scheme");
        assert!(entries[0].current.contains("Bearer tokens"));
        assert_eq!(entries[0].history.len(), 1);
        assert_eq!(entries[0].history[0].kind, DecisionEventKind::Decision);
        // Duplicate id is rejected.
        assert!(decision_create(&dir, "api-auth", "x", "y", "z").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decision_revise_appends_revision_event() {
        let dir = tmp_memory_dir();
        decision_create(&dir, "d1", "Title", "", "v1").unwrap();
        assert!(decision_revise(&dir, "d1", "v2", "switched to mTLS").unwrap());
        let entries = read_decisions(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].current, "v2");
        assert_eq!(entries[0].history.len(), 2);
        assert_eq!(entries[0].history[1].kind, DecisionEventKind::Revision);
        assert!(entries[0].history[1].summary.contains("mTLS"));
        // Unknown id => false, no-op.
        assert!(!decision_revise(&dir, "nope", "x", "y").unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decision_reverse_marks_reversed_and_keeps_entry() {
        let dir = tmp_memory_dir();
        decision_create(&dir, "d1", "Title", "", "v1").unwrap();
        assert!(decision_reverse(&dir, "d1", "superseded by gateway").unwrap());
        let entries = read_decisions(&dir);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].reversed);
        // current is frozen, not blanked.
        assert_eq!(entries[0].current, "v1");
        assert_eq!(entries[0].history.len(), 2);
        assert_eq!(entries[0].history[1].kind, DecisionEventKind::Reversal);
        // Reversing again is a no-op (reversals are final).
        assert!(!decision_reverse(&dir, "d1", "again").unwrap());
        // And revising a reversed decision is refused.
        assert!(!decision_revise(&dir, "d1", "v2", "late change").unwrap());
        // Unknown id => false.
        assert!(!decision_reverse(&dir, "nope", "x").unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decision_roundtrip_preserves_history() {
        let dir = tmp_memory_dir();
        decision_create(&dir, "d1", "Title", "policy", "v1").unwrap();
        decision_revise(&dir, "d1", "v2", "reason A").unwrap();
        decision_revise(&dir, "d1", "v3", "reason B").unwrap();
        decision_reverse(&dir, "d1", "overturned").unwrap();

        // Re-read from disk and verify the whole trail survived a full
        // parse/render/parse round-trip.
        let entries = read_decisions(&dir);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert!(e.reversed);
        assert_eq!(e.current, "v3");
        assert_eq!(e.category, "policy");
        assert_eq!(e.history.len(), 4);
        assert_eq!(e.history[0].kind, DecisionEventKind::Decision);
        assert_eq!(e.history[1].kind, DecisionEventKind::Revision);
        assert_eq!(e.history[2].kind, DecisionEventKind::Revision);
        assert_eq!(e.history[3].kind, DecisionEventKind::Reversal);
        assert!(e.history[3].summary.contains("overturned"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decision_atomic_write_is_idempotent_on_reparse() {
        // Rendering then re-parsing must be stable (no data loss / drift).
        let dir = tmp_memory_dir();
        decision_create(&dir, "d1", "Title", "cat", "line1\nline2").unwrap();
        let first = read_decisions(&dir);
        let rendered = render_decisions(&first);
        let second = parse_decisions(&rendered);
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(&dir);
    }
}
