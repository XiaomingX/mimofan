//! Memory system for mimofan
//!
//! This crate provides cross-session memory capabilities including:
//! - Text embedding via API (OpenAI/DeepSeek)
//! - Vector storage and search via hnsw-rs + sled
//! - Observation compression and summarization
//! - Cross-session memory injection
//! - Knowledge agent and corpus functionality

pub mod compressor;
pub mod embedding;
pub mod error;
pub mod injector;
pub mod knowledge;
pub mod optimization;
pub mod vector;

pub use compressor::{CompressionStrategy, ObservationCompressor, SessionSummary};
pub use embedding::{EmbeddingConfig, EmbeddingService};
pub use error::MemoryError;
pub use injector::{InjectionConfig, MemoryInjection, MemoryInjector};
pub use knowledge::{CorpusAnswer, CorpusSource, KnowledgeAgent, KnowledgeCorpus};
pub use optimization::{
    BatchProcessor, LongTaskManager, LongTaskResult, ObservationStore, RateLimiter, SearchCache,
};
pub use vector::{Observation, ObservationKind, SearchFilters, VectorMatch, VectorStore};

/// Result type for memory operations
pub type Result<T> = std::result::Result<T, MemoryError>;
