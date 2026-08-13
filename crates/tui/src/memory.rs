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
//! Default behavior is **opt-in**: requires `[memory] enabled = true` or
//! `MIMOFAN_MEMORY=on`.

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
    if let Some(active) = active_paths {
        if !active.is_empty() {
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
                    if let Some(globs) = paths {
                        if paths_match(&globs, active) {
                            let body = strip_timestamp(text.trim_start_matches("- ").trim());
                            matches_lines.push(format!("- [{cat}] {body}"));
                        }
                    }
                }
            }
            if !matches_lines.is_empty() {
                block.push_str("\n\n<memory_paths_matches>\n");
                block.push_str(&matches_lines.join("\n"));
                block.push_str("\n</memory_paths_matches>");
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_memory_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "mimofan-memory-test-{}-{}",
            std::process::id(),
            nanos
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
}
