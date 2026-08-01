//! Error types for the memory system

use thiserror::Error;

/// Errors that can occur in the memory system
#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Embedding model error: {0}")]
    Embedding(String),

    #[error("Vector store error: {0}")]
    VectorStore(String),

    #[error("Sled database error: {0}")]
    Sled(#[from] sled::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Bincode error: {0}")]
    Bincode(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    #[error("Observation not found: {0}")]
    ObservationNotFound(i64),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("API error: {status} - {message}")]
    ApiError { status: u16, message: String },
}

impl From<Box<bincode::ErrorKind>> for MemoryError {
    fn from(err: Box<bincode::ErrorKind>) -> Self {
        MemoryError::Bincode(err.to_string())
    }
}
