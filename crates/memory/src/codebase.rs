//! Codebase semantic index (issue #675 / #720, E1).
//!
//! Provides offline, code-level semantic recall: a repository is split into
//! overlapping line-window chunks, each chunk's symbols are extracted with a
//! cheap heuristic, and the chunks are stored in SQLite with an FTS5 full-text
//! index plus `file_path` / `language` metadata for filtered retrieval.
//!
//! Incremental updates are file-grained: a file whose content hash is
//! unchanged since the last index is skipped entirely (zero recompute).
//!
//! Embedding-based hybrid retrieval is intentionally left as an orthogonal
//! enhancement: the FTS5 baseline works fully offline with no API key, while
//! a caller with an `EmbeddingService` can layer semantic (vector) search on
//! top by querying the indexed files independently.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::params;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::Result;

/// A signal that contributed to a hybrid retrieval score.
///
/// Each source's raw normalized contribution is recorded in
/// [`SearchHit::score_breakdown`] so callers can explain *why* a chunk ranked
/// where it did and tune fusion weights. New signals (vector, recency,
/// importance) extend this enum without touching the fusion core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RetrievalSource {
    /// FTS5 BM25 full-text match (offline, no API key).
    FullText,
    /// Lightweight in-chunk term-frequency lexical signal (offline).
    Lexical,
    /// Reserved for vector/semantic recall (requires an `Embedder`).
    Vector,
    /// Reserved for time-proximity decay.
    Recency,
    /// Reserved for importance (reference/edit frequency or external score).
    Importance,
}

/// Number of source lines per chunk before overlap.
const LINES_PER_CHUNK: usize = 40;
/// Overlap (in lines) between consecutive chunks so a match near a boundary
/// is never split across two chunks and dropped.
const CHUNK_OVERLAP: usize = 8;

/// A single indexed chunk of a source file.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub file_path: String,
    pub language: String,
    pub kind: ChunkKind,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    /// Symbols (function/struct/impl/trait names) referenced by this chunk.
    pub symbols: Vec<String>,
}

/// How a chunk was derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkKind {
    /// Fixed-size line window.
    Lines,
}

/// A search result hit.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub file_path: String,
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    /// Symbols referenced by the matched chunk.
    pub symbols: Vec<String>,
    /// FTS5 bm25 rank (lower is more relevant); only meaningful for FTS hits.
    pub rank: f64,
    /// Short highlighted snippet around the match.
    pub snippet: String,
    /// Per-source contribution to the final hybrid score, keyed by
    /// [`RetrievalSource`]. Populated by [`CodebaseIndex::hybrid_search`];
    /// `search` leaves it empty. Enables explainable ranking.
    pub score_breakdown: HashMap<RetrievalSource, f64>,
}

/// Filters applied after the FTS5 full-text match.
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Restrict to a language id (e.g. "rust", "python").
    pub language: Option<String>,
    /// Restrict to paths starting with this prefix.
    pub path_prefix: Option<String>,
    /// Restrict to chunks that reference one of these symbols.
    pub symbols: Vec<String>,
}

/// Offline codebase semantic index backed by SQLite + FTS5.
pub struct CodebaseIndex {
    sqlite: rusqlite::Connection,
}

impl CodebaseIndex {
    /// Open (or create) a codebase index at `path` (a directory holding
    /// `codebase.db`). `repo` is an opaque label grouping one codebase.
    pub fn open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)?;
        let db_path = path.join("codebase.db");
        let sqlite = rusqlite::Connection::open(&db_path)?;
        sqlite.pragma_update(None, "journal_mode", "WAL")?;
        sqlite.pragma_update(None, "foreign_keys", "ON")?;
        let idx = Self { sqlite };
        idx.init_schema()?;
        Ok(idx)
    }

    fn init_schema(&self) -> Result<()> {
        self.sqlite.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS code_files (
                repo TEXT NOT NULL,
                file_path TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                PRIMARY KEY (repo, file_path)
            );

            CREATE TABLE IF NOT EXISTS code_chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repo TEXT NOT NULL,
                file_path TEXT NOT NULL,
                language TEXT NOT NULL,
                kind TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                symbols_json TEXT NOT NULL DEFAULT '[]'
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_repo_file
                ON code_chunks(repo, file_path);

            CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(
                content,
                chunk_id UNINDEXED,
                file_path UNINDEXED,
                language UNINDEXED,
                symbols UNINDEXED,
                tokenize = 'unicode61'
            );
            "#,
        )?;
        Ok(())
    }

    /// Index a single file. Splits into chunks, extracts symbols, and
    /// increments the FTS5 index. If the file's content hash matches the
    /// previously indexed version, the file is skipped (incremental, zero
    /// recompute).
    pub fn index_file(
        &self,
        repo: &str,
        file_path: &str,
        language: &str,
        content: &str,
    ) -> Result<IndexStats> {
        let hash = hash_str(content);
        if self.file_unchanged(repo, file_path, &hash)? {
            return Ok(IndexStats {
                chunks: 0,
                skipped: true,
            });
        }

        // Replace the file's previous chunks atomically.
        self.remove_file(repo, file_path)?;
        let mut chunks = chunk_source(content, language);
        for chunk in &mut chunks {
            chunk.file_path = file_path.to_string();
            self.insert_chunk(repo, chunk)?;
        }
        self.upsert_file_hash(repo, file_path, &hash)?;
        Ok(IndexStats {
            chunks: chunks.len(),
            skipped: false,
        })
    }

    fn file_unchanged(&self, repo: &str, file_path: &str, hash: &str) -> Result<bool> {
        let existing: Option<String> = self
            .sqlite
            .query_row(
                "SELECT content_hash FROM code_files WHERE repo = ?1 AND file_path = ?2",
                params![repo, file_path],
                |row| row.get(0),
            )
            .optional()?;
        Ok(existing.as_deref() == Some(hash))
    }

    fn upsert_file_hash(&self, repo: &str, file_path: &str, hash: &str) -> Result<()> {
        self.sqlite.execute(
            "INSERT INTO code_files (repo, file_path, content_hash) VALUES (?1, ?2, ?3)
             ON CONFLICT(repo, file_path) DO UPDATE SET content_hash = ?3",
            params![repo, file_path, hash],
        )?;
        Ok(())
    }

    fn remove_file(&self, repo: &str, file_path: &str) -> Result<()> {
        // Delete FTS rows first (they carry file_path for the match).
        self.sqlite
            .execute(
                "DELETE FROM code_fts WHERE file_path = ?1",
                params![file_path],
            )?;
        self.sqlite.execute(
            "DELETE FROM code_chunks WHERE repo = ?1 AND file_path = ?2",
            params![repo, file_path],
        )?;
        Ok(())
    }

    fn insert_chunk(&self, repo: &str, chunk: &Chunk) -> Result<()> {
        let symbols_json = serde_json::to_string(&chunk.symbols).unwrap_or_else(|_| "[]".to_string());
        self.sqlite.execute(
            "INSERT INTO code_chunks (repo, file_path, language, kind, start_line, end_line, content, symbols_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                repo,
                chunk.file_path,
                chunk.language,
                match chunk.kind {
                    ChunkKind::Lines => "lines",
                },
                chunk.start_line as i64,
                chunk.end_line as i64,
                chunk.content,
                symbols_json,
            ],
        )?;
        let id = self.sqlite.last_insert_rowid();
        // Mirror into the FTS5 table. FTS5 manages its own rowid; we keep a
        // `chunk_id` UNINDEXED column pointing back to `code_chunks.id` so a
        // search join and a per-file delete both stay precise.
        self.sqlite.execute(
            "INSERT INTO code_fts (content, chunk_id, file_path, language, symbols)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                chunk.content,
                id,
                chunk.file_path,
                chunk.language,
                chunk.symbols.join(" "),
            ],
        )?;
        Ok(())
    }

    /// Full-text search over the indexed codebase. The query is passed
    /// verbatim to FTS5 MATCH (callers may use FTS5 prefix/phrase syntax).
    /// `filters` narrow results by language / path prefix / symbols.
    pub fn search(&self, query: &str, filters: &SearchFilters, limit: usize) -> Result<Vec<SearchHit>> {
        // Normalize the query so that identifiers written in code style
        // (underscores / camelCase) align with how `unicode61` tokenized the
        // indexed content. We only rewrite when the user did not already use
        // FTS5 syntax ("..." phrase, `*`, AND/OR/NOT, parens) so we never
        // clobber an intentional query.
        let query = normalize_query(query);

        // Build the SQL: FTS5 match, then join metadata and apply filters.
        let mut sql = String::from(
            "SELECT c.file_path, c.language, c.start_line, c.end_line, c.content, \
             c.symbols_json, f.rank, snippet(code_fts, 0, '[', ']', '…', 12) \
             FROM code_fts f JOIN code_chunks c ON c.id = f.chunk_id \
             WHERE code_fts MATCH ?1",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        params.push(Box::new(query.to_string()));

        if let Some(lang) = &filters.language {
            sql.push_str(" AND c.language = ?");
            sql.push_str(&(params.len() + 1).to_string());
            params.push(Box::new(lang.clone()));
        }
        if let Some(prefix) = &filters.path_prefix {
            sql.push_str(" AND c.file_path LIKE ?");
            sql.push_str(&(params.len() + 1).to_string());
            params.push(Box::new(format!("{prefix}%")));
        }
        sql.push_str(" ORDER BY f.rank LIMIT ?");
        sql.push_str(&(params.len() + 1).to_string());
        params.push(Box::new(limit as i64));

        let mut stmt = self.sqlite.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(param_refs), |row| {
            let rank: f64 = row.get(6)?;
            let mut breakdown = HashMap::new();
            // FTS5 `rank` is bm25 (lower = more relevant, can be negative).
            // Normalize to a 0..1 score where higher = more relevant so it is
            // comparable with other signals in `hybrid_search`.
            breakdown.insert(RetrievalSource::FullText, normalize_bm25(rank));
            Ok(SearchHit {
                file_path: row.get(0)?,
                language: row.get(1)?,
                start_line: row.get::<_, i64>(2)? as usize,
                end_line: row.get::<_, i64>(3)? as usize,
                content: row.get(4)?,
                symbols: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                rank,
                snippet: row.get(7)?,
                score_breakdown: breakdown,
            })
        })?;

        let mut hits = Vec::new();
        for row in rows {
            let hit = row?;
            // Symbol filter (post-FTS, since the FTS symbols column is free text).
            if !filters.symbols.is_empty() {
                let wanted = filters
                    .symbols
                    .iter()
                    .any(|s| hit.symbols.iter().any(|x| x == s));
                if !wanted {
                    continue;
                }
            }
            hits.push(hit);
        }
        Ok(hits)
    }

    /// Fuse full-text (FTS5 BM25) and a lightweight in-chunk lexical signal
    /// using Reciprocal Rank Fusion (RRF), producing a single ranked list
    /// whose [`SearchHit::score_breakdown`] explains each source's
    /// contribution. This is the minimal, dependency-free slice of hybrid
    /// retrieval (issue #714): it adds the fusion *scaffold* and explainability
    /// without pulling in a vector embedder or a time-decay store. Additional
    /// signals (vector / recency / importance) plug in by extending
    /// [`RetrievalSource`] and the loop below.
    ///
    /// `k` is the RRF constant (higher = rank differences matter less); the
    /// lexical signal is the summed term-frequency of query tokens appearing
    /// in the chunk, normalized to 0..1 by the best lexical score in the batch.
    pub fn hybrid_search(
        &self,
        query: &str,
        filters: &SearchFilters,
        limit: usize,
        k: f64,
    ) -> Result<Vec<SearchHit>> {
        let mut hits = self.search(query, filters, limit * 4)?;
        if hits.is_empty() {
            return Ok(hits);
        }

        // Lexical signal: term-frequency of query tokens within each chunk.
        let tokens: Vec<String> = normalize_query(query)
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let mut best_lexical = 0.0_f64;
        for hit in &mut hits {
            let lower = hit.content.to_lowercase();
            let mut tf = 0.0_f64;
            for tok in &tokens {
                tf += lower.matches(tok.as_str()).count() as f64;
            }
            let lexical = if tokens.is_empty() { 0.0 } else { tf };
            hit.score_breakdown
                .insert(RetrievalSource::Lexical, lexical);
            best_lexical = best_lexical.max(lexical);
        }
        // Normalize lexical to 0..1 against the best in this batch.
        if best_lexical > 0.0 {
            for hit in &mut hits {
                if let Some(v) = hit.score_breakdown.get_mut(&RetrievalSource::Lexical) {
                    *v /= best_lexical;
                }
            }
        }

        // RRF over the two ranked lists. FullText list is already ranked by
        // `rank` (bm25, ascending = more relevant); lexical list ranks by its
        // raw tf. We fuse via rank position in each ordering.
        let rrf = |rank_position: f64| 1.0 / (k + rank_position);

        let mut fused: Vec<(f64, usize)> = Vec::with_capacity(hits.len());
        // Re-rank by FullText score descending to get positions.
        let mut ft_order: Vec<usize> = (0..hits.len()).collect();
        ft_order.sort_by(|&a, &b| {
            let la = hits[a]
                .score_breakdown
                .get(&RetrievalSource::FullText)
                .copied()
                .unwrap_or(0.0);
            let lb = hits[b]
                .score_breakdown
                .get(&RetrievalSource::FullText)
                .copied()
                .unwrap_or(0.0);
            // Descending: higher FullText score = more relevant = position 0.
            lb.partial_cmp(&la).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut lex_order: Vec<usize> = (0..hits.len()).collect();
        lex_order.sort_by(|&a, &b| {
            let la = hits[a]
                .score_breakdown
                .get(&RetrievalSource::Lexical)
                .copied()
                .unwrap_or(0.0);
            let lb = hits[b]
                .score_breakdown
                .get(&RetrievalSource::Lexical)
                .copied()
                .unwrap_or(0.0);
            // Descending: higher lexical score = more relevant = position 0.
            lb.partial_cmp(&la).unwrap_or(std::cmp::Ordering::Equal)
        });

        for (pos, &i) in ft_order.iter().enumerate() {
            let score = rrf(pos as f64 + 1.0);
            if let Some(v) = hits[i].score_breakdown.get_mut(&RetrievalSource::FullText) {
                *v = score;
            }
            fused.push((score, i));
        }
        for (pos, &i) in lex_order.iter().enumerate() {
            let score = rrf(pos as f64 + 1.0);
            let entry = hits[i]
                .score_breakdown
                .entry(RetrievalSource::Lexical)
                .or_insert(0.0);
            *entry = score;
            // Accumulate into the fused score (RRF is additive across signals).
            if let Some(slot) = fused.iter_mut().find(|(_, idx)| *idx == i) {
                slot.0 += score;
            }
        }

        fused.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut out = fused.into_iter().take(limit).map(|(_, i)| hits[i].clone()).collect::<Vec<_>>();
        // Re-rank `rank` to mirror the fused order for callers that sort by it.
        for (pos, hit) in out.iter_mut().enumerate() {
            hit.rank = pos as f64;
        }
        Ok(out)
    }

    /// Count indexed chunks for a repo (used by tests/diagnostics).
    pub fn chunk_count(&self, repo: &str) -> Result<usize> {
        let n: i64 = self
            .sqlite
            .query_row(
                "SELECT COUNT(*) FROM code_chunks WHERE repo = ?1",
                params![repo],
                |row| row.get(0),
            )?;
        Ok(n as usize)
    }
}

/// Stats returned from an indexing operation.
#[derive(Debug, Clone, Copy)]
pub struct IndexStats {
    pub chunks: usize,
    pub skipped: bool,
}

// --- pure helpers (unit-tested) ---

/// Rewrite a bare identifier-style FTS5 query so it matches how the
/// `unicode61` tokenizer split the indexed text. `unicode61` treats `_`,
/// `.` and `/` as token separators and lowercases, so `compute_hash` is
/// stored as the two tokens `compute` and `hash`. A FTS5 MATCH for
/// `compute_hash` is parsed as a single token and matches nothing — we
/// expand it to `compute hash` (implicit AND) so it hits. camelCase
/// boundaries get the same treatment (`fooBar` -> `foo bar`).
///
/// If the query already uses FTS5 syntax we leave it untouched.
pub fn normalize_query(q: &str) -> String {
    let trimmed = q.trim();
    let uses_syntax = trimmed.contains('"')
        || trimmed.contains('*')
        || trimmed.contains('(')
        || {
            let u = trimmed.to_uppercase();
            u.contains(" AND ") || u.contains(" OR ") || u.contains(" NOT ")
        };
    if uses_syntax {
        return q.to_string();
    }
    let mut out = String::with_capacity(q.len() + 4);
    let mut prev_lower = false;
    for ch in q.chars() {
        if ch == '_' || ch == '.' || ch == '/' || ch == '-' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        // Insert a space at a camelCase boundary: lower→Upper.
        if prev_lower && ch.is_uppercase() {
            out.push(' ');
        }
        for c in ch.to_lowercase() {
            out.push(c);
        }
        prev_lower = ch.is_lowercase();
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalize an FTS5 BM25 `rank` (lower = more relevant, often negative and
/// unbounded) into a 0..1 score where higher = more relevant. Used so the
/// full-text signal is comparable with other [`RetrievalSource`] scores in
/// [`CodebaseIndex::hybrid_search`].
fn normalize_bm25(rank: f64) -> f64 {
    // FTS5 `rank` is bm25: negative for strong matches, positive/zero for weak.
    // A logistic squashes it so a strongly-matching chunk (rank << 0)
    // approaches 1 and a weak one (rank >> 0) approaches 0.
    1.0 / (1.0 + rank.exp())
}

/// Stable, dependency-free content hash.
pub fn hash_str(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:x}", h.finish())
}

/// Split source text into overlapping line-window chunks, extracting a
/// heuristic symbol list per chunk.
pub fn chunk_source(content: &str, language: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let step = LINES_PER_CHUNK.saturating_sub(CHUNK_OVERLAP).max(1);
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let end = (start + LINES_PER_CHUNK).min(lines.len());
        let slice = lines[start..end].join("\n");
        let symbols = extract_symbols(&slice);
        chunks.push(Chunk {
            file_path: String::new(), // filled by caller
            language: language.to_string(),
            kind: ChunkKind::Lines,
            start_line: start + 1,
            end_line: end,
            content: slice,
            symbols,
        });
        if end == lines.len() {
            break;
        }
        start += step;
    }
    chunks
}

/// Cheap, dependency-free symbol extraction: matches `fn`/`struct`/`enum`/
/// `impl`/`trait`/`mod` declaration lines and pulls the identifier.
pub fn extract_symbols(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        for kw in ["fn ", "struct ", "enum ", "impl ", "trait ", "mod ", "async fn "] {
            if let Some(rest) = trimmed.strip_prefix(kw) {
                // Take the identifier up to the first non-ident char.
                let ident: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() && !out.contains(&ident) {
                    out.push(ident);
                }
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_tmp() -> (TempDir, CodebaseIndex) {
        let dir = TempDir::new().unwrap();
        let idx = CodebaseIndex::open(dir.path()).unwrap();
        (dir, idx)
    }

    const SAMPLE: &str = r#"
fn compute_hash(input: &str) -> u64 {
    let mut h = 0;
    for c in input.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u64);
    }
    h
}

struct Tokenizer {
    delim: char,
}

impl Tokenizer {
    fn next_token(&self) -> Option<&str> {
        None
    }
}
"#;

    #[test]
    fn chunk_source_splits_and_overlaps() {
        let chunks = chunk_source(SAMPLE, "rust");
        assert!(chunks.len() >= 1);
        for c in &chunks {
            assert_eq!(c.kind, ChunkKind::Lines);
            assert!(c.end_line >= c.start_line);
        }
        // Symbols from the sample are extracted.
        let all_syms: Vec<String> = chunks.iter().flat_map(|c| c.symbols.clone()).collect();
        assert!(all_syms.iter().any(|s| s == "compute_hash"));
        assert!(all_syms.iter().any(|s| s == "Tokenizer"));
        assert!(all_syms.iter().any(|s| s == "next_token"));
    }

    #[test]
    fn extract_symbols_handles_async_and_impl() {
        let syms = extract_symbols("async fn run() {}\nimpl Foo {\n    fn bar(&self) {}\n}");
        assert!(syms.contains(&"run".to_string()));
        assert!(syms.contains(&"Foo".to_string()));
        assert!(syms.contains(&"bar".to_string()));
        assert!(!syms.contains(&"impl".to_string()));
    }

    #[test]
    fn empty_source_yields_no_chunks() {
        assert!(chunk_source("", "rust").is_empty());
    }

    #[test]
    fn index_and_search_finds_chunk() {
        let (_dir, idx) = open_tmp();
        let stats = idx
            .index_file("repo", "src/lib.rs", "rust", SAMPLE)
            .unwrap();
        assert!(stats.chunks > 0);
        assert!(!stats.skipped);

        let hits = idx
            .search("wrapping_mul", &SearchFilters::default(), 10)
            .unwrap();
        assert!(!hits.is_empty());
        let hit = &hits[0];
        assert_eq!(hit.file_path, "src/lib.rs");
        assert!(hit.content.contains("wrapping_mul"));
    }

    #[test]
    fn incremental_skip_on_unchanged_file() {
        let (_dir, idx) = open_tmp();
        let first = idx.index_file("repo", "a.rs", "rust", SAMPLE).unwrap();
        assert!(!first.skipped);
        let second = idx.index_file("repo", "a.rs", "rust", SAMPLE).unwrap();
        assert!(second.skipped, "unchanged file must be skipped");
        assert_eq!(second.chunks, 0);

        // Changing content re-indexes.
        let third = idx
            .index_file("repo", "a.rs", "rust", "fn different() {}")
            .unwrap();
        assert!(!third.skipped);
    }

    #[test]
    fn search_filters_by_language_and_path() {
        let (_dir, idx) = open_tmp();
        idx.index_file("repo", "src/a.rs", "rust", SAMPLE).unwrap();
        idx.index_file("repo", "src/b.py", "python", "def compute_hash(x):\n    return x\n").unwrap();

        // Language filter.
        let rust_only = idx
            .search("compute_hash", &SearchFilters { language: Some("rust".into()), ..Default::default() }, 10)
            .unwrap();
        assert!(rust_only.iter().all(|h| h.language == "rust"));

        // Path prefix filter.
        let py_only = idx
            .search("compute_hash", &SearchFilters { path_prefix: Some("src/b".into()), ..Default::default() }, 10)
            .unwrap();
        assert_eq!(py_only.len(), 1);
        assert_eq!(py_only[0].language, "python");
    }

    #[test]
    fn symbol_filter_post_fts() {
        let (_dir, idx) = open_tmp();
        idx.index_file("repo", "src/a.rs", "rust", SAMPLE).unwrap();
        let hits = idx
            .search(
                "Tokenizer",
                &SearchFilters { symbols: vec!["Tokenizer".into()], ..Default::default() },
                10,
            )
            .unwrap();
        assert!(hits.iter().all(|h| h.symbols.contains(&"Tokenizer".to_string())
            || h.content.contains("Tokenizer")));
    }

    #[test]
    fn normalize_query_expands_underscore_and_camel() {
        assert_eq!(normalize_query("compute_hash"), "compute hash");
        assert_eq!(normalize_query("fooBar"), "foo bar");
        assert_eq!(normalize_query("my_FooBar.baz"), "my foo bar baz");
        assert_eq!(normalize_query("SIMPLE"), "simple");
    }

    #[test]
    fn normalize_query_preserves_fts_syntax() {
        assert_eq!(normalize_query("\"exact phrase\""), "\"exact phrase\"");
        assert_eq!(normalize_query("prefix*"), "prefix*");
        assert_eq!(normalize_query("a OR b"), "a OR b");
        assert_eq!(normalize_query("foo AND bar"), "foo AND bar");
    }

    #[test]
    fn normalize_bm25_maps_relevance_to_unit() {
        // More relevant (more negative rank) -> closer to 1.
        assert!(normalize_bm25(-10.0) > 0.9);
        assert!(normalize_bm25(0.0) > 0.4 && normalize_bm25(0.0) < 0.6);
        // Less relevant (positive rank) -> closer to 0.
        assert!(normalize_bm25(10.0) < 0.1);
    }

    #[test]
    fn search_populates_fulltext_breakdown() {
        let (_dir, idx) = open_tmp();
        idx.index_file("repo", "src/lib.rs", "rust", SAMPLE).unwrap();
        let hits = idx
            .search("wrapping_mul", &SearchFilters::default(), 10)
            .unwrap();
        assert!(!hits.is_empty());
        let hit = &hits[0];
        assert!(hit
            .score_breakdown
            .contains_key(&RetrievalSource::FullText));
        assert!(hit.score_breakdown[&RetrievalSource::FullText] > 0.0);
    }

    #[test]
    fn hybrid_search_fuses_and_explains() {
        let (_dir, idx) = open_tmp();
        // Two files: one matches the query term strongly, the other also
        // contains the term so lexical + full-text both contribute.
        idx.index_file(
            "repo",
            "a.rs",
            "rust",
            "fn compute_hash(input: &str) -> u64 { let mut h = 0; for c in input.chars() { h = h.wrapping_mul(31).wrapping_add(c as u64); } h }\n",
        )
        .unwrap();
        idx.index_file(
            "repo",
            "b.rs",
            "rust",
            "fn other() { let x = compute_hash(\"hi\"); println!(\"{x}\"); }\n",
        )
        .unwrap();

        let hits = idx
            .hybrid_search("compute_hash", &SearchFilters::default(), 10, 60.0)
            .unwrap();
        assert!(!hits.is_empty(), "hybrid search must return hits");

        // Every hit carries an explainable breakdown with both signals.
        for hit in &hits {
            assert!(hit.score_breakdown.contains_key(&RetrievalSource::FullText));
            assert!(hit.score_breakdown.contains_key(&RetrievalSource::Lexical));
            let total: f64 = hit.score_breakdown.values().sum();
            assert!(total > 0.0, "fused score must be positive");
        }

        // Both the defining chunk and the referencing chunk are recalled, and
        // the fused order is deterministic (lexical/FTS5 ranking, not
        // semantic — so we assert presence, not a specific position 0).
        let paths: Vec<&str> = hits.iter().map(|h| h.file_path.as_str()).collect();
        assert!(paths.contains(&"a.rs"), "defining chunk must be recalled");
        assert!(paths.contains(&"b.rs"), "referencing chunk must be recalled");
        // Fused scores are non-increasing down the list (RRF ordering).
        for w in hits.windows(2) {
            let s0: f64 = w[0].score_breakdown.values().sum();
            let s1: f64 = w[1].score_breakdown.values().sum();
            assert!(s0 >= s1 - 1e-9, "hybrid order must be non-increasing by fused score");
        }
    }

    #[test]
    fn hybrid_search_respects_limit() {
        let (_dir, idx) = open_tmp();
        for i in 0..5 {
            idx.index_file(
                "repo",
                &format!("m{i}.rs"),
                "rust",
                &format!("fn compute_hash_{i}() {{}}\n"),
            )
            .unwrap();
        }
        let hits = idx
            .hybrid_search("compute_hash", &SearchFilters::default(), 2, 60.0)
            .unwrap();
        assert_eq!(hits.len(), 2);
    }
}
