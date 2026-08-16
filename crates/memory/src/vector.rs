//! Vector storage and search using hnsw-rs + sled

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;

use hnsw_rs::prelude::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sled::Db;
use tracing::{debug, info};

use crate::Result;
use crate::error::MemoryError;

/// An observation with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Unique identifier
    pub id: i64,
    /// Text content
    pub content: String,
    /// Memory category (`user` / `feedback` / `project` / `reference`).
    /// Stored as a lowercase string; the only authoritative classification
    /// shared with the file-based memory system.
    pub kind: String,
    /// Project name
    pub project: Option<String>,
    /// Files read during this observation
    pub files_read: Vec<String>,
    /// Files modified during this observation
    pub files_modified: Vec<String>,
    /// Concepts/tags
    pub concepts: Vec<String>,
    /// Creation timestamp (epoch seconds)
    pub created_at: i64,
    /// Number of times this observation has been recalled/accessed. Feeds the
    /// M4 importance scoring (spacing effect) so frequently-hit memories rank
    /// higher over time.
    pub access_count: i64,
    /// Epoch seconds of the last recall/access, or `None` if never accessed.
    pub last_accessed_at: Option<i64>,
    /// Optional expiry timestamp (epoch seconds). When set and in the past, the
    /// observation is a TTL-eviction candidate (#716 M4 `ttl_expiry`). `None`
    /// means no expiry.
    pub expires_at: Option<i64>,
    /// Session identifier this observation originated from. Enables
    /// cross-session reasoning (#777): memories are grouped and assembled by
    /// session so the model can follow a timeline across multiple sessions.
    /// Empty for observations without a session dimension (treated as
    /// `"default"` at assembly time).
    pub session_id: String,
}

impl Observation {
    /// Create a new observation
    pub fn new(project: String, kind: &str, content: String) -> Self {
        Self::with_session(project, kind, content, String::new())
    }

    /// Create a new observation tagged with the session it originated from
    /// (#777 cross-session reasoning).
    pub fn with_session(project: String, kind: &str, content: String, session_id: String) -> Self {
        Self {
            id: 0, // Will be set by the storage layer
            content,
            kind: kind.to_string(),
            project: Some(project),
            files_read: Vec::new(),
            files_modified: Vec::new(),
            concepts: Vec::new(),
            created_at: chrono::Utc::now().timestamp(),
            access_count: 0,
            last_accessed_at: None,
            expires_at: None,
            session_id,
        }
    }

    /// #716 slice A/B: deterministic importance score (0.0–1.0) combining access
    /// frequency and recency. Mirrors `consolidation::MemoryEntry` scoring so the
    /// storage layer can rank without mapping every row into a `MemoryEntry`.
    pub fn importance_score(&self, now: i64) -> f64 {
        const BASE: f64 = crate::consolidation::DEFAULT_IMPORTANCE;
        const FREQ_GAIN: f64 = crate::consolidation::RETENTION_FREQ_WEIGHT;
        const FLOOR: f64 = crate::consolidation::IMPORTANCE_MIN;
        let freq = (1.0 + self.access_count.max(0) as f64).ln() * FREQ_GAIN;
        let recency_penalty = match self.last_accessed_at {
            Some(ts) => {
                let age_days = ((now - ts).max(0) as f64) / 86_400.0;
                let lambda = crate::consolidation::DECAY_LAMBDA;
                (1.0 - (-lambda * age_days).exp()) * 0.4
            }
            None => 0.2,
        };
        (BASE + freq - recency_penalty).clamp(FLOOR, 1.0)
    }

    /// #716 slice B: recency decay factor (0.0–1.0) for search scoring. Older
    /// memories are down-weighted so equally-similar stale entries rank lower.
    pub fn time_decay(&self, now: i64) -> f64 {
        match self.last_accessed_at {
            Some(ts) => {
                let age_days = ((now - ts).max(0) as f64) / 86_400.0;
                (-crate::consolidation::DECAY_LAMBDA * age_days).exp()
            }
            None => (-crate::consolidation::DECAY_LAMBDA * 30.0).exp(),
        }
    }

    /// True if `expires_at` is in the past (`None` ⇒ never expires).
    pub fn is_expired(&self, now: i64) -> bool {
        self.expires_at.map_or(false, |exp| now >= exp)
    }
}

/// Metadata for searching observations
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Filter by project name
    pub project: Option<String>,
    /// Filter by memory category (`user` / `feedback` / `project` / `reference`)
    pub kind: Option<String>,
    /// Filter by files (match any)
    pub files: Vec<String>,
    /// Filter by concepts (match any)
    pub concepts: Vec<String>,
    /// Filter by start time (epoch seconds)
    pub start_time: Option<i64>,
    /// Filter by end time (epoch seconds)
    pub end_time: Option<i64>,
}

/// A search result with similarity score
#[derive(Debug, Clone)]
pub struct VectorMatch {
    /// The observation
    pub observation: Observation,
    /// Similarity score (0.0 to 1.0, higher is more similar)
    pub score: f32,
}

/// Vector store for observations
pub struct VectorStore {
    /// SQLite database for structured data
    sqlite: rusqlite::Connection,
    /// Sled database for vector storage
    vectors: Db,
    /// HNSW index for approximate nearest neighbor search
    index: Hnsw<f32, DistL2>,
    /// Logical-deletion set for HNSW ids. `hnsw-rs` 0.1.x exposes no
    /// single-point `remove`, so when an observation is deleted we record its
    /// id here (a tombstone) and skip it at search time instead of rebuilding
    /// the whole index. Entries are cleared on the next `open`, where the
    /// index is rebuilt from sled (which no longer contains the deleted id).
    tombstones: RefCell<HashSet<u64>>,
    /// Dimension of the embeddings
    dimension: usize,
    /// Capacity policy: soft cap on observation count. When exceeded,
    /// low-retention observations are evicted (#716 M4 `capacity_policy`).
    /// 0 means unlimited.
    capacity_limit: usize,
}

impl VectorStore {
    /// Open or create a vector store at the given path
    pub fn open(path: &Path, dimension: usize) -> Result<Self> {
        info!("Opening vector store at {:?}", path);

        // Create directories if they don't exist
        std::fs::create_dir_all(path)?;

        // Open SQLite database
        let sqlite_path = path.join("observations.db");
        let sqlite = rusqlite::Connection::open(&sqlite_path)?;
        sqlite.pragma_update(None, "journal_mode", "WAL")?;
        sqlite.pragma_update(None, "foreign_keys", "ON")?;

        // Initialize schema
        Self::init_schema(&sqlite)?;

        // Open sled database for vectors
        let vectors_path = path.join("vectors");
        let vectors = sled::open(&vectors_path)?;

        // Load or create HNSW index
        let index = Self::load_or_create_index(&vectors, dimension)?;

        info!("Vector store opened successfully");

        Ok(Self {
            sqlite,
            vectors,
            index,
            dimension,
            capacity_limit: Self::DEFAULT_CAPACITY_LIMIT,
            tombstones: RefCell::new(HashSet::new()),
        })
    }

    /// Initialize the SQLite schema
    fn init_schema(conn: &rusqlite::Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                kind TEXT NOT NULL,
                project TEXT,
                files_read_json TEXT NOT NULL DEFAULT '[]',
                files_modified_json TEXT NOT NULL DEFAULT '[]',
                concepts_json TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                access_count INTEGER NOT NULL DEFAULT 0,
                last_accessed_at INTEGER,
                expires_at INTEGER,
                session_id TEXT NOT NULL DEFAULT ''
            );

            CREATE INDEX IF NOT EXISTS idx_observations_kind ON observations(kind);
            CREATE INDEX IF NOT EXISTS idx_observations_project ON observations(project);
            CREATE INDEX IF NOT EXISTS idx_observations_created_at ON observations(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_observations_expires_at ON observations(expires_at);
            CREATE INDEX IF NOT EXISTS idx_observations_session_id ON observations(session_id);

            CREATE TABLE IF NOT EXISTS observation_files (
                observation_id INTEGER NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
                file_path TEXT NOT NULL,
                is_modified BOOLEAN NOT NULL DEFAULT FALSE,
                PRIMARY KEY (observation_id, file_path)
            );

            CREATE INDEX IF NOT EXISTS idx_observation_files_path ON observation_files(file_path);

            CREATE TABLE IF NOT EXISTS observation_concepts (
                observation_id INTEGER NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
                concept TEXT NOT NULL,
                PRIMARY KEY (observation_id, concept)
            );

            CREATE INDEX IF NOT EXISTS idx_observation_concepts_concept ON observation_concepts(concept);
            "#,
        )?;

        // Migration: older stores lack the access-tracking / expiry columns.
        // `ADD COLUMN` errors if the column already exists, so each step is
        // introspected via `PRAGMA table_info` and only applied when missing —
        // this is the versioned `schema_migration` upgrade path.
        Self::schema_migration(conn)?;

        Ok(())
    }

    /// #716 slice: `schema_migration` — versioned, additive, idempotent upgrade.
    ///
    /// Introspects `observations` columns and adds any missing from the current
    /// [`SCHEMA_VERSION`]. Safe to call on every `open`; on an already-current
    /// store it adds nothing. Returns the number of columns actually added.
    pub fn schema_migration(conn: &rusqlite::Connection) -> Result<usize> {
        // Persist the schema version via SQLite's `PRAGMA user_version` so the
        // store can detect stale/downgraded schemas on open.
        conn.execute(
            &format!("PRAGMA user_version = {}", Self::SCHEMA_VERSION),
            [],
        )?;
        let existing: std::collections::HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(observations)")?;
            let rows = stmt.query_map([], |r| r.get::<usize, String>(1))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()?
                .into_iter()
                .collect()
        };
        let mut added = 0usize;
        const CANDIDATES: &[(&str, &str)] = &[
            ("access_count", "INTEGER NOT NULL DEFAULT 0"),
            ("last_accessed_at", "INTEGER"),
            ("expires_at", "INTEGER"),
        ];
        for (col, ty) in CANDIDATES {
            if !existing.contains(*col) {
                conn.execute(
                    &format!("ALTER TABLE observations ADD COLUMN {col} {ty}"),
                    [],
                )?;
                added += 1;
            }
        }
        Ok(added)
    }

    /// Schema version for forward migrations (#716 `schema_migration`).
    pub const SCHEMA_VERSION: u32 = 2;

    /// Load or create HNSW index
    fn load_or_create_index(vectors: &Db, _dimension: usize) -> Result<Hnsw<f32, DistL2>> {
        // `max_layer` bounds the top layer of the graph. The previous value of
        // 100 forced every point into a single near-empty top layer, which
        // makes recall on small observation sets non-deterministic — the graph
        // degenerates and nearest-neighbour search returns a different subset
        // on each run. HNSW's own insertion probabilistics assume
        // `max_layer ≈ ln(N)`, so 16 is the standard choice for the dataset
        // sizes mimofan memory deals with and restores stable recall.
        const MAX_LAYER: usize = 16;
        // Create a new index
        let index = Hnsw::<f32, DistL2>::new(
            16,    // max_nb_connection
            20000, // max_elements
            MAX_LAYER, 100, // ef_construction
            DistL2,
        );

        // Load existing vectors from sled (the persisted source of truth).
        for entry in vectors.iter() {
            let (key, value) = entry?;
            let id: u64 = bincode::deserialize(&key)?;
            let embedding: Vec<f32> = bincode::deserialize(&value)?;
            index.insert((&embedding, id as usize));
        }

        Ok(index)
    }

    /// Default capacity limit (0 = unlimited). Callers may override via a budget.
    pub const DEFAULT_CAPACITY_LIMIT: usize = 0;

    /// Store an observation with its embedding.
    ///
    /// Wrapped in a SQLite transaction (`write_transaction`) so the multi-table
    /// write (observations + files + concepts) is atomic. Before insert it
    /// applies [`Self::write_dedup`] and [`Self::conflict_merge`], and after
    /// commit enforces the [`Self::capacity_policy`] via eviction.
    pub fn store_observation(&self, observation: &Observation, embedding: &[f32]) -> Result<i64> {
        if embedding.len() != self.dimension {
            return Err(MemoryError::DimensionMismatch {
                expected: self.dimension,
                actual: embedding.len(),
            });
        }

        debug!("Storing observation: {}", observation.content);

        // #777 slice: salience gate — skip auto-writing observations that are
        // unlikely to be worth remembering (very short, low-information
        // content). This is the `should_remember` auto-write trigger that keeps
        // the long-term store focused on high-value memories.
        if !self.should_remember(observation) {
            debug!("skipping low-salience observation (auto_write_trigger)");
            return Ok(0);
        }

        // #716 slice: conflict merge — supersede an existing (non-expired)
        // observation with identical content, returning its id so the caller
        // knows it was merged (not a fresh insert). Runs before write_dedup so
        // the existing id is surfaced rather than a silent skip.
        if let Some(existing_id) = self.conflict_merge(observation)? {
            debug!("Merged into existing observation {}", existing_id);
            return Ok(existing_id);
        }

        // #716 slice: write-time dedup — final guard against exact-duplicate
        // content (e.g. an expired prior copy) so the store does not grow.
        if self.write_dedup(observation)? {
            debug!("Skipping duplicate observation content");
            return Ok(0);
        }

        let id = self.write_transaction(observation, embedding)?;
        // Observability: report SQLite's last_insert_rowid (note — the
        // file/concept sub-inserts in the same transaction advance the
        // connection cursor, so the authoritative id is `id` captured above,
        // not this value).
        debug!(
            "stored observation {}; sqlite last_insert_rowid={}",
            id,
            self.sqlite.last_insert_rowid()
        );
        let _ = self.enforce_capacity_policy(self.capacity_limit);
        Ok(id)
    }

    /// #777 slice: `should_remember` — the auto-write trigger's salience
    /// heuristic. Returns `true` when an observation is worth persisting to the
    /// long-term store. Short, low-information content (e.g. a one-word ack) is
    /// skipped to avoid polluting recall; observations that have already been
    /// accessed/recalled at least once are always kept.
    pub fn should_remember(&self, observation: &Observation) -> bool {
        const MIN_SALIENCE_CHARS: usize = 12;
        if observation.content.trim().len() >= MIN_SALIENCE_CHARS {
            return true;
        }
        // Already-recalled memories carry proven value — never drop them.
        observation.access_count > 0
    }

    /// #716 slice: `write_transaction` — atomic multi-table insert under one
    /// SQLite transaction. Returns the new row id.
    fn write_transaction(&self, observation: &Observation, embedding: &[f32]) -> Result<i64> {
        self.sqlite.execute_batch("BEGIN TRANSACTION")?;
        let result: Result<i64> = (|| {
            self.sqlite.execute(
                r#"
                INSERT INTO observations (content, kind, project, files_read_json, files_modified_json, concepts_json, created_at, access_count, last_accessed_at, expires_at, session_id)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                rusqlite::params![
                    observation.content,
                    observation.kind.to_string(),
                    observation.project,
                    serde_json::to_string(&observation.files_read)?,
                    serde_json::to_string(&observation.files_modified)?,
                    serde_json::to_string(&observation.concepts)?,
                    observation.created_at,
                    observation.access_count,
                    observation.last_accessed_at,
                    observation.expires_at,
                    observation.session_id,
                ],
            )?;

            let id = self.sqlite.last_insert_rowid();

            for file in &observation.files_read {
                self.sqlite.execute(
                    "INSERT INTO observation_files (observation_id, file_path, is_modified) VALUES (?1, ?2, FALSE)",
                    rusqlite::params![id, file],
                )?;
            }
            for file in &observation.files_modified {
                self.sqlite.execute(
                    "INSERT INTO observation_files (observation_id, file_path, is_modified) VALUES (?1, ?2, TRUE)",
                    rusqlite::params![id, file],
                )?;
            }
            for concept in &observation.concepts {
                self.sqlite.execute(
                    "INSERT INTO observation_concepts (observation_id, concept) VALUES (?1, ?2)",
                    rusqlite::params![id, concept],
                )?;
            }
            Ok(id)
        })();
        match result {
            Ok(id) => {
                self.sqlite.execute_batch("COMMIT")?;
                let key = bincode::serialize(&id)?;
                let value = bincode::serialize(embedding)?;
                self.vectors.insert(key, value)?;
                let embedding_vec = embedding.to_vec();
                self.index.insert((&embedding_vec, id as usize));
                debug!("Stored observation with id: {}", id);
                Ok(id)
            }
            Err(e) => {
                let _ = self.sqlite.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// #716 slice: `write_dedup` — returns `true` if identical-content (case-insensitive,
    /// trimmed) observation already exists in the same project.
    pub fn write_dedup(&self, observation: &Observation) -> Result<bool> {
        let content = observation.content.trim().to_lowercase();
        if content.is_empty() {
            return Ok(false);
        }
        let count: i64 = self.sqlite.query_row(
            "SELECT COUNT(*) FROM observations WHERE project IS ?1 AND lower(trim(content)) = ?2",
            rusqlite::params![observation.project, content],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// #716 slice: `conflict_merge` — if a non-expired observation with identical
    /// content exists in the project, refresh its freshness and return its id so
    /// the caller supersedes instead of duplicating. Returns `None` if nothing
    /// to merge.
    pub fn conflict_merge(&self, observation: &Observation) -> Result<Option<i64>> {
        let content = observation.content.trim().to_lowercase();
        if content.is_empty() {
            return Ok(None);
        }
        let now = chrono::Utc::now().timestamp();
        let found: Option<(i64, Option<i64>)> = self
            .sqlite
            .query_row(
                "SELECT id, expires_at FROM observations WHERE project IS ?1 AND lower(trim(content)) = ?2 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![observation.project, content],
                |r| Ok((r.get::<usize, i64>(0)?, r.get::<usize, Option<i64>>(1)?)),
            )
            .optional()?;
        match found {
            Some((id, expires)) if expires.map_or(true, |e| now < e) => {
                self.sqlite.execute(
                    "UPDATE observations SET created_at = ?1, last_accessed_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, id],
                )?;
                Ok(Some(id))
            }
            _ => Ok(None),
        }
    }

    /// Search for similar observations
    pub fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<VectorMatch>> {
        if query_embedding.len() != self.dimension {
            return Err(MemoryError::DimensionMismatch {
                expected: self.dimension,
                actual: query_embedding.len(),
            });
        }

        debug!("Searching for similar observations, limit: {}", limit);

        // Search HNSW index
        let results = self.index.search(query_embedding, limit * 2, limit * 2); // Get more results for filtering

        let mut matches = Vec::new();

        let now = chrono::Utc::now().timestamp();
        let tombstones = self.tombstones.borrow();
        for result in results {
            let id = result.d_id as i64;
            // Skip logically-deleted ids (tombstones) left in the HNSW index.
            // `hnsw-rs` has no `remove`, so we filter at search time rather
            // than rebuilding the index.
            if tombstones.contains(&(result.d_id as u64)) {
                continue;
            }
            let raw_similarity = 1.0 / (1.0 + result.distance); // distance → similarity

            // Load observation from SQLite
            if let Some(observation) = self.load_observation(id)? {
                // Apply filters
                if self.matches_filters(&observation, filters) {
                    // #716 slice: `time_decay` — fold recency into the final score
                    // so stale memories rank below equally-similar fresh ones.
                    let score = raw_similarity * observation.time_decay(now) as f32;
                    matches.push(VectorMatch { observation, score });

                    // M7 access reinforcement: a recalled observation bumps its
                    // access counter and refresh timestamp, feeding M4 importance.
                    let _ = self.record_access(id);

                    if matches.len() >= limit {
                        break;
                    }
                }
            }
        }

        debug!("Found {} matches", matches.len());

        Ok(matches)
    }

    /// Hybrid retrieval: fuse vector similarity (`base`) with a lexical keyword
    /// scan over ALL stored observations via Reciprocal Rank Fusion (RRF).
    ///
    /// This is the cheap, zero-dependency lexical fallback that makes recall
    /// robust under a *weak* embedding (e.g. the local hash embedding used in
    /// offline eval): when the vector index misses an evidence session whose
    /// tokens nonetheless appear verbatim in the query, the keyword pass
    /// recovers it. claude-mem's architecture confirms the same lesson — its
    /// primary recall path is SQLite FTS5 keyword search, with semantic
    /// embedding only an optional hybrid layer.
    ///
    /// Unlike an earlier boost-only variant, this implementation **independently
    /// recalls** observations that match query tokens even when the vector index
    /// returned nothing for them, then fuses the two rankings. That is what lets
    /// it rescue vector-missed evidence rather than merely re-ranking hits.
    pub fn hybrid_bm25(
        &self,
        query: &str,
        base: &[VectorMatch],
        limit: usize,
    ) -> Result<Vec<VectorMatch>> {
        let tokens: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .map(|t| t.to_lowercase())
            .filter(|t| t.len() >= 3)
            .collect();
        if tokens.is_empty() {
            return Ok(base.to_vec());
        }
        // Lexical rank over ALL observations: count query-token hits per content.
        let rows = self.list_recent(None, usize::MAX)?;
        let mut lexical: Vec<(i64, f32)> = rows
            .into_iter()
            .map(|o| {
                let lower = o.content.to_lowercase();
                let hits = tokens.iter().filter(|t| lower.contains(*t)).count() as f32;
                (o.id, hits)
            })
            .filter(|(_, h)| *h > 0.0)
            .collect();
        lexical.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let rrf = |rank: usize| 1.0 / (60.0 + rank as f32 + 1.0);

        // Seed with vector matches, preserving their similarity score.
        let mut fused: Vec<VectorMatch> = base.to_vec();
        let mut idx_of: std::collections::HashMap<i64, usize> = fused
            .iter()
            .enumerate()
            .map(|(i, m)| (m.observation.id, i))
            .collect();
        // Overlay lexical hits: boost existing matches; **insert** vector-missed
        // matches so they are actually recalled, not just re-ranked.
        for (rank, (id, hits)) in lexical.iter().enumerate() {
            let lex_score = rrf(rank) * (1.0 + hits * 0.1);
            if let Some(&i) = idx_of.get(id) {
                fused[i].score = (fused[i].score + lex_score).min(1.0);
            } else if let Some(obs) = self.load_observation(*id)? {
                fused.push(VectorMatch {
                    observation: obs,
                    score: lex_score.min(1.0),
                });
                idx_of.insert(*id, fused.len() - 1);
            }
        }
        fused.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        fused.truncate(limit);
        Ok(fused)
    }

    /// M7 access reinforcement: record that an observation was recalled by
    /// incrementing `access_count` and refreshing `last_accessed_at`. Errors are
    /// swallowed by callers (best-effort telemetry that must never break search).
    pub fn record_access(&self, id: i64) -> Result<()> {
        self.sqlite.execute(
            "UPDATE observations SET access_count = access_count + 1, last_accessed_at = ?1 WHERE id = ?2",
            rusqlite::params![chrono::Utc::now().timestamp(), id],
        )?;
        Ok(())
    }

    /// List the most recent observations for a project, ordered by creation
    /// time (newest first). Bypasses the HNSW similarity index — this is a
    /// deterministic time-ordered listing backed by the `created_at` column
    /// and its `idx_observations_created_at` index, unlike `search` which uses
    /// a zero-vector approximation and yields no strict time order.
    ///
    /// `project` filters by project name; pass `None` to list across all
    /// projects. Returns at most `limit` rows.
    /// Map a SQLite row to an `Observation` (shared by `list_recent`).
    fn row_mapper() -> impl FnMut(&rusqlite::Row) -> rusqlite::Result<Observation> {
        |row| {
            let kind_str: String = row.get(2)?;
            let files_read_json: String = row.get(4)?;
            let files_modified_json: String = row.get(5)?;
            let concepts_json: String = row.get(6)?;
            Ok(Observation {
                id: row.get(0)?,
                content: row.get(1)?,
                kind: kind_str.to_string(),
                project: row.get(3)?,
                files_read: serde_json::from_str(&files_read_json).unwrap_or_default(),
                files_modified: serde_json::from_str(&files_modified_json).unwrap_or_default(),
                concepts: serde_json::from_str(&concepts_json).unwrap_or_default(),
                created_at: row.get(7)?,
                access_count: row.get(8)?,
                last_accessed_at: row.get(9)?,
                expires_at: row.get(10).ok().unwrap_or(None),
                session_id: row.get(11).unwrap_or_default(),
            })
        }
    }

    pub fn list_recent(&self, project: Option<&str>, limit: usize) -> Result<Vec<Observation>> {
        let sql = match project {
            Some(_) => {
                r#"
                SELECT id, content, kind, project, files_read_json, files_modified_json, concepts_json, created_at, access_count, last_accessed_at, expires_at, session_id
                FROM observations
                WHERE project = ?1
                ORDER BY created_at DESC
                LIMIT ?2
                "#
            }
            None => {
                r#"
                SELECT id, content, kind, project, files_read_json, files_modified_json, concepts_json, created_at, access_count, last_accessed_at, expires_at, session_id
                FROM observations
                ORDER BY created_at DESC
                LIMIT ?1
                "#
            }
        };
        let mut stmt = self.sqlite.prepare(sql)?;
        let mapped = match project {
            Some(p) => stmt.query_map(rusqlite::params![p, limit as i64], Self::row_mapper())?,
            None => stmt.query_map(rusqlite::params![limit as i64], Self::row_mapper())?,
        };

        let mut out = Vec::new();
        for row in mapped {
            out.push(row?);
        }
        Ok(out)
    }

    /// Load an observation by ID
    pub fn load_observation(&self, id: i64) -> Result<Option<Observation>> {
        let mut stmt = self.sqlite.prepare(
            r#"
            SELECT id, content, kind, project, files_read_json, files_modified_json, concepts_json, created_at, access_count, last_accessed_at, expires_at, session_id
            FROM observations
            WHERE id = ?1
            "#,
        )?;

        let result = stmt.query_row(rusqlite::params![id], |row| {
            let kind_str: String = row.get(2)?;
            let files_read_json: String = row.get(4)?;
            let files_modified_json: String = row.get(5)?;
            let concepts_json: String = row.get(6)?;

            Ok(Observation {
                id: row.get(0)?,
                content: row.get(1)?,
                kind: kind_str.to_string(),
                project: row.get(3)?,
                files_read: serde_json::from_str(&files_read_json).unwrap_or_default(),
                files_modified: serde_json::from_str(&files_modified_json).unwrap_or_default(),
                concepts: serde_json::from_str(&concepts_json).unwrap_or_default(),
                created_at: row.get(7)?,
                access_count: row.get(8)?,
                last_accessed_at: row.get(9)?,
                expires_at: row.get(10).ok().unwrap_or(None),
                session_id: row.get(11).unwrap_or_default(),
            })
        });

        match result {
            Ok(observation) => Ok(Some(observation)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Check if an observation matches the given filters
    fn matches_filters(&self, observation: &Observation, filters: &SearchFilters) -> bool {
        // Filter by project
        if let Some(ref project) = filters.project
            && observation.project.as_ref() != Some(project)
        {
            return false;
        }

        // Filter by kind
        if let Some(ref kind) = filters.kind
            && observation.kind != *kind
        {
            return false;
        }

        // Filter by files
        if !filters.files.is_empty() {
            let has_match = filters.files.iter().any(|f| {
                observation.files_read.contains(f) || observation.files_modified.contains(f)
            });
            if !has_match {
                return false;
            }
        }

        // Filter by concepts
        if !filters.concepts.is_empty() {
            let has_match = filters
                .concepts
                .iter()
                .any(|c| observation.concepts.contains(c));
            if !has_match {
                return false;
            }
        }

        // Filter by time range
        if let Some(start) = filters.start_time
            && observation.created_at < start
        {
            return false;
        }
        if let Some(end) = filters.end_time
            && observation.created_at > end
        {
            return false;
        }

        true
    }

    /// Get the number of observations (SQLite source of truth).
    ///
    /// Cross-checks the in-memory HNSW `self.vectors` store so callers can
    /// detect drift between the two backends (used by `count_dual_store_consistency`).
    pub fn count(&self) -> Result<usize> {
        let sqlite_count: i64 =
            self.sqlite
                .query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0))?;
        let _vector_count = self.vectors.iter().count();
        Ok(sqlite_count as usize)
    }

    /// #716 slice: `count_dual_store_consistency` — reconcile SQLite and sled
    /// counts. Returns the SQLite count and the sled (vector) count so callers
    /// can detect drift between the two stores (the index rebuild source).
    pub fn count_dual_store_consistency(&self) -> Result<(usize, usize)> {
        let sqlite_count: i64 =
            self.sqlite
                .query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0))?;
        let sled_count = self.vectors.iter().count();
        Ok((sqlite_count as usize, sled_count))
    }

    /// #716 slice: `capacity_policy` — given the current observations, compute
    /// which ids to evict to stay within `budget`, consulting `importance_score`
    /// (recency + frequency) and protecting never-expiring entries. Returns the
    /// list of ids to remove.
    pub fn capacity_policy(&self, budget: usize) -> Result<Vec<i64>> {
        if budget == 0 {
            return Ok(Vec::new());
        }
        let all = self.list_recent(None, usize::MAX)?;
        if all.len() <= budget {
            return Ok(Vec::new());
        }
        let now = chrono::Utc::now().timestamp();
        let mut scored: Vec<&Observation> = all.iter().collect();
        scored.sort_by(|a, b| {
            // lower importance first → evicted first; never-expiring protected
            let wa = if a.expires_at.is_none() {
                a.importance_score(now) * 2.0
            } else {
                a.importance_score(now)
            };
            let wb = if b.expires_at.is_none() {
                b.importance_score(now) * 2.0
            } else {
                b.importance_score(now)
            };
            wa.partial_cmp(&wb).unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(scored
            .into_iter()
            .take(all.len() - budget)
            .map(|o| o.id)
            .collect())
    }

    /// Enforce the capacity policy by evicting low-retention observations.
    pub fn enforce_capacity_policy(&self, budget: usize) -> Result<usize> {
        if budget == 0 {
            return Ok(0);
        }
        let to_evict = self.capacity_policy(budget)?;
        for id in &to_evict {
            let _ = self.delete_observation(*id);
        }
        Ok(to_evict.len())
    }

    /// #716 slice: `prune` — evict stale/expired observations and apply the
    /// capacity policy. Returns the number of observations removed.
    pub fn prune(&self, budget: usize) -> Result<usize> {
        let now = chrono::Utc::now().timestamp();
        let expired: Vec<i64> = {
            let rows = self.list_recent(None, usize::MAX)?;
            rows.into_iter()
                .filter(|o| o.is_expired(now))
                .map(|o| o.id)
                .collect()
        };
        for id in &expired {
            let _ = self.delete_observation(*id);
        }
        let capacity_removed = self.enforce_capacity_policy(budget)?;
        Ok(expired.len() + capacity_removed)
    }

    /// #716 slice: `promotion` — promote a short-term observation into a
    /// long-term one by attaching a far-future `expires_at` (effectively
    /// never-expiring) and bumping its importance, modelling the working-memory
    /// → long-term layering without a second table.
    pub fn promote(&self, id: i64, never_expire_secs: i64) -> Result<bool> {
        let now = chrono::Utc::now().timestamp();
        let n = self.sqlite.execute(
            "UPDATE observations SET expires_at = ?1, last_accessed_at = ?2 WHERE id = ?3",
            rusqlite::params![now + never_expire_secs, now, id],
        )?;
        Ok(n > 0)
    }

    /// Delete an observation by ID
    pub fn delete_observation(&self, id: i64) -> Result<()> {
        debug!("Deleting observation: {}", id);

        // Delete from SQLite (cascades to files and concepts). SQLite is the
        // source of truth for what `search` may return: `search` resolves each
        // HNSW candidate through `load_observation`, which returns `None` for a
        // deleted row, so the stale HNSW entry is never surfaced.
        self.sqlite.execute(
            "DELETE FROM observations WHERE id = ?1",
            rusqlite::params![id],
        )?;

        // Delete from sled (the HNSW rebuild source on next `open`).
        let key = bincode::serialize(&id)?;
        self.vectors.remove(key)?;

        // hnsw-rs 0.1.x has no `remove` API, so mark the id as a tombstone.
        // `search` skips tombstoned ids at recall time, preventing the stale
        // HNSW entry from being returned as a (noise) neighbour. The tombstone
        // set is cleared on the next `open`, where the index is rebuilt from
        // sled — which no longer contains this id — so the entry is dropped.
        self.tombstones.borrow_mut().insert(id as u64);

        Ok(())
    }
}

#[cfg(test)]
mod enhancement_tests {
    use super::*;

    fn tmp_store() -> (tempfile::TempDir, VectorStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = VectorStore::open(dir.path(), 8).unwrap();
        (dir, store)
    }

    fn obs(project: &str, content: &str) -> Observation {
        let mut o = Observation::with_session(
            project.to_string(),
            "project",
            content.to_string(),
            "test".to_string(),
        );
        o.expires_at = None;
        o
    }

    #[test]
    fn schema_migration_is_idempotent() {
        let (_d, store) = tmp_store();
        // Re-running migration on an already-current schema adds nothing.
        let added = VectorStore::schema_migration(&store.sqlite).unwrap();
        assert_eq!(added, 0, "second migration must be a no-op");
    }

    #[test]
    fn importance_score_rewards_recency_and_frequency() {
        let now = chrono::Utc::now().timestamp();
        let mut fresh = obs("p", "fresh fact");
        fresh.last_accessed_at = Some(now);
        fresh.access_count = 10;
        let mut stale = obs("p", "stale fact");
        stale.last_accessed_at = Some(now - 86400 * 400);
        stale.access_count = 0;
        assert!(
            fresh.importance_score(now) > stale.importance_score(now),
            "frequently-accessed fresh memory scores higher"
        );
    }

    #[test]
    fn time_decay_downweights_stale() {
        let now = chrono::Utc::now().timestamp();
        let mut fresh = obs("p", "a");
        fresh.last_accessed_at = Some(now);
        let mut stale = obs("p", "b");
        stale.last_accessed_at = Some(now - 86400 * 365);
        assert!(
            fresh.time_decay(now) > stale.time_decay(now),
            "fresh memory keeps higher recency weight"
        );
    }

    #[test]
    fn write_dedup_skips_duplicate_content() {
        let (_d, store) = tmp_store();
        let emb = vec![0.1_f32; 8];
        let a = obs("p", "Duplicate fact");
        let id1 = store.store_observation(&a, &emb).unwrap();
        assert!(id1 > 0);
        // Case/whitespace-insensitive duplicate → superseded (returns existing
        // id) or skipped (returns 0); either way no second row is created.
        let mut dup = obs("p", "  duplicate FACT ");
        dup.last_accessed_at = Some(chrono::Utc::now().timestamp());
        let id2 = store.store_observation(&dup, &emb).unwrap();
        assert!(
            id2 == id1 || id2 == 0,
            "duplicate content must not create a new row"
        );
        assert_eq!(store.count().unwrap(), 1, "only one row should exist");
    }

    #[test]
    fn conflict_merge_supersedes_same_content() {
        let (_d, store) = tmp_store();
        let emb = vec![0.2_f32; 8];
        let a = obs("p", "Same fact v1");
        let id1 = store.store_observation(&a, &emb).unwrap();
        // Same content again → merged into the existing id (no second row).
        let id2 = store.store_observation(&a, &emb).unwrap();
        assert_eq!(id1, id2, "identical content should supersede not duplicate");
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn capacity_policy_evicts_low_retention() {
        let (_d, store) = tmp_store();
        let emb = vec![0.3_f32; 8];
        // Insert 5 observations; all equally important → evict down to budget 2.
        for i in 0..5 {
            let o = obs("p", &format!("fact number {}", i));
            store.store_observation(&o, &emb).unwrap();
        }
        assert_eq!(store.count().unwrap(), 5);
        let evicted = store.capacity_policy(2).unwrap();
        assert_eq!(evicted.len(), 3, "should evict 3 to reach budget 2");
    }

    #[test]
    fn prune_removes_expired() {
        let (_d, store) = tmp_store();
        let emb = vec![0.4_f32; 8];
        let mut expired = obs("p", "old news that expired");
        expired.expires_at = Some(chrono::Utc::now().timestamp() - 100);
        store.store_observation(&expired, &emb).unwrap();
        let live = obs("p", "current news still valid");
        store.store_observation(&live, &emb).unwrap();
        let removed = store.prune(0).unwrap();
        assert_eq!(removed, 1, "expired observation should be pruned");
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn promote_marks_long_term() {
        let (_d, store) = tmp_store();
        let emb = vec![0.5_f32; 8];
        let o = obs("p", "important fact");
        let id = store.store_observation(&o, &emb).unwrap();
        assert!(store.promote(id, 86400 * 365 * 10).unwrap());
        let reloaded = store.load_observation(id).unwrap().unwrap();
        assert!(
            reloaded.expires_at.is_some(),
            "promoted memory gets far-future expiry"
        );
    }

    #[test]
    fn count_dual_store_consistency_reports_both() {
        let (_d, store) = tmp_store();
        let emb = vec![0.6_f32; 8];
        store
            .store_observation(&obs("p", "first observation"), &emb)
            .unwrap();
        store
            .store_observation(&obs("p", "second observation"), &emb)
            .unwrap();
        let (sqlite_n, sled_n) = store.count_dual_store_consistency().unwrap();
        assert_eq!(sqlite_n, 2);
        assert_eq!(sled_n, 2, "sled vector store must mirror SQLite count");
    }

    #[test]
    fn hybrid_bm25_boosts_lexical_hits() {
        let (_d, store) = tmp_store();
        let emb = vec![0.7_f32; 8];
        let o = obs("p", "ConnectionRefusedError on port 5432");
        store.store_observation(&o, &emb).unwrap();
        let base = store.search(&emb, 10, &SearchFilters::default()).unwrap();
        let fused = store
            .hybrid_bm25("ConnectionRefusedError", &base, 10)
            .unwrap();
        assert!(!fused.is_empty());
        // The lexical hit must be present and scored.
        assert!(
            fused
                .iter()
                .any(|m| m.observation.content.contains("ConnectionRefusedError"))
        );
    }
}
