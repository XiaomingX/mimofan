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

use chrono::Utc;

/// 唯一权威记忆分类（对齐 CodeBuddy Typed Memory）。
///
/// 文件记忆与向量记忆共用同一套分类，避免两套并存的命名体系。
/// 字符串值固定为小写：`user` / `feedback` / `project` / `reference`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCategory {
    /// 用户的角色、目标、偏好与知识背景。
    User,
    /// 用户对协作方式的纠正与指导（已确认的偏好 / 验证过的方法）。
    Feedback,
    /// 项目进行中的工作、目标与决策（无法从代码直接推导的背景）。
    Project,
    /// 外部系统与资源的指引（去哪里查信息）。
    Reference,
}

impl MemoryCategory {
    /// 所有分类，按注入/展示的稳定顺序排列。
    pub const ALL: &'static [MemoryCategory] = &[
        MemoryCategory::User,
        MemoryCategory::Feedback,
        MemoryCategory::Project,
        MemoryCategory::Reference,
    ];

    /// 返回分类的小写字符串形式。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryCategory::User => "user",
            MemoryCategory::Feedback => "feedback",
            MemoryCategory::Project => "project",
            MemoryCategory::Reference => "reference",
        }
    }

    /// 从字符串解析分类（大小写不敏感）。
    #[must_use]
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

/// Compose the `<user_memory_index>` block for the system prompt, honouring
/// the opt-in toggle. Returns `None` when disabled, the directory is missing,
/// or the index is empty — so callers don't need to check both conditions.
#[must_use]
pub fn compose_index_block(enabled: bool, dir: &Path) -> Option<String> {
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
    Some(format!(
        "<user_memory_index source=\"{display}\">\n{payload}\n</user_memory_index>"
    ))
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
}
