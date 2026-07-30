//! Vector storage and search using hnsw-rs + sled

use std::path::Path;

use hnsw_rs::prelude::*;
use serde::{Deserialize, Serialize};
use sled::Db;
use tracing::{debug, info};

use crate::error::MemoryError;
use crate::Result;

/// Types of observations that can be stored
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ObservationKind {
    /// Bug fix
    Bugfix,
    /// Feature implementation
    Feature,
    /// Architecture/design decision
    Decision,
    /// Code discovery/pattern
    Discovery,
    /// Code change/refactor
    Change,
    /// Manual observation
    Manual,
}

impl std::fmt::Display for ObservationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObservationKind::Bugfix => write!(f, "bugfix"),
            ObservationKind::Feature => write!(f, "feature"),
            ObservationKind::Decision => write!(f, "decision"),
            ObservationKind::Discovery => write!(f, "discovery"),
            ObservationKind::Change => write!(f, "change"),
            ObservationKind::Manual => write!(f, "manual"),
        }
    }
}

impl std::str::FromStr for ObservationKind {
    type Err = MemoryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "bugfix" => Ok(ObservationKind::Bugfix),
            "feature" => Ok(ObservationKind::Feature),
            "decision" => Ok(ObservationKind::Decision),
            "discovery" => Ok(ObservationKind::Discovery),
            "change" => Ok(ObservationKind::Change),
            "manual" => Ok(ObservationKind::Manual),
            _ => Err(MemoryError::InvalidConfig(format!(
                "Unknown observation kind: {}",
                s
            ))),
        }
    }
}

/// An observation with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Unique identifier
    pub id: i64,
    /// Text content
    pub content: String,
    /// Type of observation
    pub kind: ObservationKind,
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
}

/// Metadata for searching observations
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Filter by project name
    pub project: Option<String>,
    /// Filter by observation kind
    pub kind: Option<ObservationKind>,
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
                created_at INTEGER NOT NULL
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
        Ok(())
    }

    /// Load or create HNSW index
    fn load_or_create_index(vectors: &Db, _dimension: usize) -> Result<Hnsw<f32, DistL2>> {
        // Create a new index
        let index = Hnsw::<f32, DistL2>::new(
            16, // max_nb_connection
            20000, // max_elements
            100, // max_layer
            100, // ef_construction
            DistL2,
        );

        // Load existing vectors from sled
        for entry in vectors.iter() {
            let (key, value) = entry?;
            let id: u64 = bincode::deserialize(&key)?;
            let embedding: Vec<f32> = bincode::deserialize(&value)?;
            index.insert((&embedding, id as usize));
        }

        Ok(index)
    }

    /// Store an observation with its embedding
    pub fn store_observation(
        &self,
        observation: &Observation,
        embedding: &[f32],
    ) -> Result<i64> {
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
            INSERT INTO observations (content, kind, project, files_read_json, files_modified_json, concepts_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            rusqlite::params![
                observation.content,
                observation.kind.to_string(),
                observation.project,
                serde_json::to_string(&observation.files_read)?,
                serde_json::to_string(&observation.files_modified)?,
                serde_json::to_string(&observation.concepts)?,
                observation.created_at,
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
                    matches.push(VectorMatch {
                        observation,
                        score,
                    });

                    if matches.len() >= limit {
                        break;
                    }
                }
            }
        }

        debug!("Found {} matches", matches.len());

        Ok(matches)
    }

    /// Load an observation by ID
    fn load_observation(&self, id: i64) -> Result<Option<Observation>> {
        let mut stmt = self.sqlite.prepare(
            r#"
            SELECT id, content, kind, project, files_read_json, files_modified_json, concepts_json, created_at
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
                kind: kind_str.parse().unwrap_or(ObservationKind::Manual),
                project: row.get(3)?,
                files_read: serde_json::from_str(&files_read_json).unwrap_or_default(),
                files_modified: serde_json::from_str(&files_modified_json).unwrap_or_default(),
                concepts: serde_json::from_str(&concepts_json).unwrap_or_default(),
                created_at: row.get(7)?,
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
        if let Some(ref project) = filters.project {
            if observation.project.as_ref() != Some(project) {
                return false;
            }
        }

        // Filter by kind
        if let Some(ref kind) = filters.kind {
            if observation.kind != *kind {
                return false;
            }
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
            let has_match = filters.concepts.iter().any(|c| observation.concepts.contains(c));
            if !has_match {
                return false;
            }
        }

        // Filter by time range
        if let Some(start) = filters.start_time {
            if observation.created_at < start {
                return false;
            }
        }
        if let Some(end) = filters.end_time {
            if observation.created_at > end {
                return false;
            }
        }

        true
    }

    /// Get the number of observations
    pub fn count(&self) -> Result<usize> {
        let count: i64 = self.sqlite.query_row("SELECT COUNT(*) FROM observations", [], |row| {
            row.get(0)
        })?;
        Ok(count as usize)
    }

    /// Delete an observation by ID
    pub fn delete_observation(&self, id: i64) -> Result<()> {
        debug!("Deleting observation: {}", id);

        // Delete from SQLite (cascades to files and concepts)
        self.sqlite
            .execute("DELETE FROM observations WHERE id = ?1", rusqlite::params![id])?;

        // Delete from sled
        let key = bincode::serialize(&id)?;
        self.vectors.remove(key)?;

        // TODO: Remove from HNSW index (requires rebuild or lazy deletion)

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_observation() -> Observation {
        Observation {
            id: 1,
            content: "Test observation".to_string(),
            kind: ObservationKind::Bugfix,
            project: Some("test-project".to_string()),
            files_read: vec!["src/main.rs".to_string()],
            files_modified: vec!["src/lib.rs".to_string()],
            concepts: vec!["bugfix".to_string(), "test".to_string()],
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    #[test]
    fn test_vector_store_creation() {
        let temp_dir = TempDir::new().unwrap();
        let store = VectorStore::open(temp_dir.path(), 384);
        assert!(store.is_ok());
    }

    #[test]
    fn test_store_and_load_observation() {
        let temp_dir = TempDir::new().unwrap();
        let store = VectorStore::open(temp_dir.path(), 384).unwrap();

        let observation = test_observation();
        let embedding = vec![0.0; 384];

        let id = store.store_observation(&observation, &embedding).unwrap();
        assert!(id > 0);

        let loaded = store.load_observation(id).unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.content, observation.content);
        assert_eq!(loaded.kind, observation.kind);
    }

    #[test]
    fn test_search() {
        let temp_dir = TempDir::new().unwrap();
        let store = VectorStore::open(temp_dir.path(), 384).unwrap();

        // Store some observations
        for i in 0..5 {
            let mut obs = test_observation();
            obs.id = i;
            obs.content = format!("Observation {}", i);
            let embedding = vec![i as f32; 384];
            store.store_observation(&obs, &embedding).unwrap();
        }

        // Search
        let query = vec![0.0; 384];
        let results = store.search(&query, 3, &SearchFilters::default()).unwrap();
        assert!(!results.is_empty());
    }
}
