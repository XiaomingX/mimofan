//! Vector storage and search using hnsw-rs + sled

use std::path::Path;

use hnsw_rs::prelude::*;
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
}

impl Observation {
    /// Create a new observation
    pub fn new(project: String, kind: &str, content: String) -> Self {
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
        }
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
    /// Dimension of the embeddings
    dimension: usize,
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
                last_accessed_at INTEGER
            );

            CREATE INDEX IF NOT EXISTS idx_observations_kind ON observations(kind);
            CREATE INDEX IF NOT EXISTS idx_observations_project ON observations(project);
            CREATE INDEX IF NOT EXISTS idx_observations_created_at ON observations(created_at DESC);

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

        // Migration: older stores lack the access-tracking columns. `ADD COLUMN`
        // errors if the column already exists, so ignore that specific failure.
        let _ = conn.execute(
            "ALTER TABLE observations ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE observations ADD COLUMN last_accessed_at INTEGER",
            [],
        );

        Ok(())
    }

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
            16,      // max_nb_connection
            20000,   // max_elements
            MAX_LAYER,
            100,     // ef_construction
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

    /// Store an observation with its embedding
    pub fn store_observation(&self, observation: &Observation, embedding: &[f32]) -> Result<i64> {
        if embedding.len() != self.dimension {
            return Err(MemoryError::DimensionMismatch {
                expected: self.dimension,
                actual: embedding.len(),
            });
        }

        debug!("Storing observation: {}", observation.content);

        // Insert into SQLite
        self.sqlite.execute(
            r#"
            INSERT INTO observations (content, kind, project, files_read_json, files_modified_json, concepts_json, created_at, access_count, last_accessed_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
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
            ],
        )?;

        let id = self.sqlite.last_insert_rowid();

        // Insert files
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

        // Insert concepts
        for concept in &observation.concepts {
            self.sqlite.execute(
                "INSERT INTO observation_concepts (observation_id, concept) VALUES (?1, ?2)",
                rusqlite::params![id, concept],
            )?;
        }

        // Store vector in sled
        let key = bincode::serialize(&id)?;
        let value = bincode::serialize(embedding)?;
        self.vectors.insert(key, value)?;

        // Insert into HNSW index
        let embedding_vec = embedding.to_vec();
        self.index.insert((&embedding_vec, id as usize));

        debug!("Stored observation with id: {}", id);

        Ok(id)
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

        for result in results {
            let id = result.d_id as i64;
            let score = 1.0 / (1.0 + result.distance); // Convert distance to similarity score

            // Load observation from SQLite
            if let Some(observation) = self.load_observation(id)? {
                // Apply filters
                if self.matches_filters(&observation, filters) {
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
            })
        }
    }

    pub fn list_recent(
        &self,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Observation>> {
        let sql = match project {
            Some(_) => r#"
                SELECT id, content, kind, project, files_read_json, files_modified_json, concepts_json, created_at, access_count, last_accessed_at
                FROM observations
                WHERE project = ?1
                ORDER BY created_at DESC
                LIMIT ?2
                "#,
            None => r#"
                SELECT id, content, kind, project, files_read_json, files_modified_json, concepts_json, created_at, access_count, last_accessed_at
                FROM observations
                ORDER BY created_at DESC
                LIMIT ?1
                "#,
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
            SELECT id, content, kind, project, files_read_json, files_modified_json, concepts_json, created_at, access_count, last_accessed_at
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

    /// Get the number of observations
    pub fn count(&self) -> Result<usize> {
        let count: i64 = self
            .sqlite
            .query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0))?;
        Ok(count as usize)
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

        // The HNSW entry is left in place. hnsw-rs 0.1.x has no `remove`
        // (lazy-deletion) API, and rebuilding the whole index here would be
        // expensive; the SQLite-backed `search` filter above already excludes
        // the deleted id, so correctness does not depend on the HNSW tombstone.
        // The stale entry only costs an extra over-fetch slot until the next
        // `open`, where it is dropped because it is absent from sled.

        Ok(())
    }
}
