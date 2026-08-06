//! Memory system for mimofan
//!
//! This crate provides cross-session memory capabilities including:
//! - Text embedding via API (OpenAI/DeepSeek)
//! - Vector storage and search via hnsw-rs + sled
//! - Observation compression and summarization
//! - Cross-session memory injection
//! - Knowledge agent and corpus functionality
//!
//! ## ⚠️ 实验性 / 未集成（见 ARCHITECTURE_IMPROVEMENT_PLAN.md Phase D）
//! 本 crate 实现向量记忆（embedding + hnsw_rs + sled）。但主流程当前使用的是
//! `mimofan`(tui) crate 内的 `crate::memory` 简单文件记忆模块；本 crate 全仓
//! 无任何上游依赖（僵尸上下文），尚未接入产品。保留以供评估，不要在生产
//! 路径中依赖它。若评估后决定不接入，应整体移除本 crate。

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
pub use vector::{Observation, SearchFilters, VectorMatch, VectorStore};

/// Result type for memory operations
pub type Result<T> = std::result::Result<T, MemoryError>;
