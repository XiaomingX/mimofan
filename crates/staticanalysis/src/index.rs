//! Persistent symbol index for SAST (issue #675 / #720).
//!
//! This is a *code* index — distinct from the `mimofan-memory` RAG/embedding
//! index that only serves conversational recall. It stores four tables:
//!
//! - `files`    — one row per indexed file, keyed by path, with a content hash
//!                and mtime used for incremental invalidation.
//! - `symbols`  — definitions discovered in a file (functions, classes, …).
//! - `imports`  — import/require/use statements per file.
//! - `refs`     — identifier references (call sites, usages) per file.
//!
//! Incremental update is driven by `(content_hash, mtime)`: a file whose hash
//! and mtime are unchanged is skipped entirely, so re-indexing a large tree
//! only touches files that actually changed. Building/refreshing the index is
//! intended to run in the background (a spawned task) and never blocks the
//! agent's turn — this module is synchronous and cheap per file, designed to
//! be driven from a background worker.
//!
//! The module only compiles with the `symbol-index` feature (it pulls in the
//! bundled SQLite build), keeping the default `mimofan-staticanalysis` build
//! lean.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::callgraph::CallGraph;
use crate::Language;

/// A single discovered definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub column: usize,
}

/// A single import statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub module: String,
    pub line: usize,
}

/// A single reference (usage / call site) to an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub name: String,
    pub line: usize,
    pub column: usize,
}

/// On-disk symbol index backed by SQLite.
pub struct SymbolIndex {
    conn: Connection,
}

impl SymbolIndex {
    /// Open (or create) the index at `path`. Use `:memory:` for an ephemeral
    /// index. The schema is created on first open.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        let idx = SymbolIndex { conn };
        idx.init_schema()?;
        Ok(idx)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;",
        )?;
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                id          INTEGER PRIMARY KEY,
                path        TEXT NOT NULL UNIQUE,
                lang        TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                mtime_ms    INTEGER NOT NULL,
                indexed_at  INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS symbols (
                id      INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                name    TEXT NOT NULL,
                kind    TEXT NOT NULL,
                line    INTEGER NOT NULL,
                column0 INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS imports (
                id      INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                module  TEXT NOT NULL,
                line    INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS refs (
                id      INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                name    TEXT NOT NULL,
                line    INTEGER NOT NULL,
                column0 INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_symbols_name  ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_imports_mod   ON imports(module);
            CREATE INDEX IF NOT EXISTS idx_refs_name     ON refs(name);
            "#,
        )?;
        Ok(())
    }

    /// Compute a cheap content hash (sha2 is already a workspace dep of the
    /// consumer; we use a simple FNV-1a here to avoid a hard dependency on
    /// `sha2` inside this crate). Good enough for change detection.
    fn hash_content(source: &str) -> String {
        // FNV-1a 64-bit.
        let mut hash: u64 = 0xcbf29ce484222325;
        for b in source.bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{hash:016x}")
    }

    fn mtime_ms(path: &Path) -> i64 {
        std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Index a single file. If the file is unchanged (same content hash and
    /// mtime), the existing rows are kept and `false` is returned (no work).
    /// Otherwise the file's rows are replaced and `true` is returned.
    ///
    /// `source` may be supplied to avoid re-reading from disk; when `None` the
    /// file is read from `path`.
    pub fn index_file(
        &mut self,
        path: &Path,
        lang: Language,
        source: Option<&str>,
    ) -> anyhow::Result<bool> {
        let source = match source {
            Some(s) => s.to_string(),
            None => std::fs::read_to_string(path)?,
        };
        let mtime = Self::mtime_ms(path);
        let hash = Self::hash_content(&source);
        let path_str = path.to_string_lossy().to_string();

        // Skip if unchanged.
        if let Some(row) = self
            .conn
            .query_row(
                "SELECT content_hash, mtime_ms FROM files WHERE path = ?1",
                params![path_str],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
        {
            if row.0 == hash && row.1 == mtime {
                return Ok(false);
            }
        }

        let tx = self.conn.transaction()?;
        // Remove prior rows for this file (cascade deletes symbols/imports/refs).
        tx.execute("DELETE FROM files WHERE path = ?1", params![path_str])?;

        tx.execute(
            "INSERT INTO files (path, lang, content_hash, mtime_ms, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                path_str,
                lang.as_str(),
                hash,
                mtime,
                now_ms()
            ],
        )?;
        let file_id = tx.last_insert_rowid();

        // Symbols + refs come from the call graph (function defs + call sites);
        // this reuses the already-written tree-sitter traversal so we don't
        // duplicate AST walking logic.
        if let Ok(graph) = CallGraph::build(&path_str, &source, lang) {
            for f in graph.functions() {
                tx.execute(
                    "INSERT INTO symbols (file_id, name, kind, line, column0)
                     VALUES (?1, ?2, 'function', ?3, 1)",
                    params![file_id, f.name, f.line as i64],
                )?;
            }
            for f in graph.functions() {
                for edge in graph.calls_of(f.id) {
                    tx.execute(
                        "INSERT INTO refs (file_id, name, line, column0)
                         VALUES (?1, ?2, ?3, 1)",
                        params![file_id, edge.callee_name, f.line as i64],
                    )?;
                }
            }
        }

        // Imports: a lightweight, language-tolerant regex-free scan over lines.
        // This avoids pulling in extra grammar-specific import queries; it is a
        // best-effort supplemental table. Edits here stay conservative.
        for (i, line) in source.lines().enumerate() {
            if let Some(module) = extract_import_module(line, lang) {
                tx.execute(
                    "INSERT INTO imports (file_id, module, line) VALUES (?1, ?2, ?3)",
                    params![file_id, module, (i + 1) as i64],
                )?;
            }
        }

        tx.commit()?;
        Ok(true)
    }

    /// Index every source file under `root` (recursively). Returns the number
    /// of files that were actually (re)indexed. Designed to be called from a
    /// background worker so it does not block the agent turn.
    pub fn index_tree(&mut self, root: &Path, langs: &[Language]) -> anyhow::Result<usize> {
        let mut count = 0;
        for entry in walk_source_files(root) {
            let lang = Language::from_path(&entry.to_string_lossy());
            if !langs.contains(&lang) || lang == Language::Auto {
                continue;
            }
            if self.index_file(&entry, lang, None)? {
                count += 1;
            }
        }
        Ok(count)
    }

    /// All symbols whose name matches `name` (glob `%name%`).
    pub fn find_symbols(&self, name: &str) -> anyhow::Result<Vec<(String, String, usize)>> {
        let like = format!("%{name}%");
        let mut stmt = self.conn.prepare(
            "SELECT f.path, s.name, s.line FROM symbols s
             JOIN files f ON f.id = s.file_id
             WHERE s.name LIKE ?1 ORDER BY s.line",
        )?;
        let rows = stmt.query_map(params![like], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as usize,
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Files that import `module`.
    pub fn find_importers(&self, module: &str) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT f.path FROM imports i
             JOIN files f ON f.id = i.file_id
             WHERE i.module LIKE ?1",
        )?;
        let rows = stmt.query_map(params![format!("%{module}%")], |r| {
            r.get::<_, String>(0)
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Files that reference `name`.
    pub fn find_references(&self, name: &str) -> anyhow::Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.path, r.line FROM refs r
             JOIN files f ON f.id = r.file_id
             WHERE r.name = ?1 ORDER BY r.line",
        )?;
        let rows = stmt.query_map(params![name], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Drop a single file's rows (e.g. on deletion). Returns whether a row was
    /// removed.
    pub fn forget_file(&mut self, path: &Path) -> anyhow::Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM files WHERE path = ?1", params![path.to_string_lossy().to_string()])?;
        Ok(n > 0)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Best-effort import module extraction. Conservative keyword scan; we only
/// take the first quoted/identifier token after the import keyword. This is
/// intentionally language-tolerant and not exhaustive.
fn extract_import_module(line: &str, lang: Language) -> Option<String> {
    let trimmed = line.trim_start();
    let after = match lang {
        Language::Rust => trimmed.strip_prefix("use "),
        Language::Java | Language::TypeScript | Language::JavaScript | Language::Kotlin
        | Language::ObjectiveC => {
            trimmed.strip_prefix("import ").or_else(|| trimmed.strip_prefix("require("))
        }
        Language::Swift => trimmed.strip_prefix("import "),
        Language::Json => None,
        Language::Auto => None,
    }?;
    let tok = after
        .split(|c: char| c.is_whitespace() || c == ';' || c == '"' || c == '\'' || c == '(' || c == ')')
        .find(|s| !s.is_empty())?;
    Some(tok.to_string())
}

/// Recursively collect source files under `root` whose extension maps to a
/// known language. Yields absolute-ish paths as discovered.
fn walk_source_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(mut entries) = std::fs::read_dir(root) else {
        return out;
    };
    while let Some(Ok(e)) = entries.next() {
        let p = e.path();
        if p.is_dir() {
            // Skip VCS / build artifacts to keep indexing cheap.
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if matches!(name, ".git" | "target" | "node_modules" | "build" | ".next" | "dist") {
                    continue;
                }
            }
            out.extend(walk_source_files(&p));
        } else if Language::from_path(&p.to_string_lossy()) != Language::Auto {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST_SRC: &str = r#"
use std::process;
use std::collections::HashMap;

fn main() {
    helper();
}

fn helper() {
    leaf();
}

fn leaf() {}
"#;

    #[test]
    fn index_and_query_roundtrip() {
        let mut idx = SymbolIndex::open(Path::new(":memory:")).expect("open");
        let tmp = std::env::temp_dir().join(format!("mimofan_idx_test_{}.rs", std::process::id()));
        std::fs::write(&tmp, RUST_SRC).expect("write");

        let changed = idx
            .index_file(&tmp, Language::Rust, Some(RUST_SRC))
            .expect("index");
        assert!(changed, "first index must report changed");

        // Second index with identical content/mtime is a no-op.
        let changed2 = idx
            .index_file(&tmp, Language::Rust, Some(RUST_SRC))
            .expect("index2");
        assert!(!changed2, "unchanged file must be skipped (incremental)");

        let syms = idx.find_symbols("helper").expect("find");
        assert!(syms.iter().any(|(_, n, _)| n == "helper"));

        let importers = idx.find_importers("collections").expect("importers");
        assert!(importers.iter().any(|p| p == tmp.to_string_lossy().as_ref()));

        let refs = idx.find_references("leaf").expect("refs");
        assert!(!refs.is_empty(), "leaf must be referenced from helper");

        std::fs::remove_file(&tmp).ok();
    }
}
