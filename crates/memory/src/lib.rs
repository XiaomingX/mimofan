//! Memory system for mimofan
//!
//! This crate provides cross-session memory capabilities including:
//! - Text embedding via API (OpenAI/DeepSeek)
//! - Vector storage and search via hnsw-rs + sled
//! - Observation compression and summarization
//! - Cross-session memory injection
//! - Knowledge agent and corpus functionality
//!
//! ## ⚠️ 实验性 / 默认编译但运行时按需启用（见 ARCHITECTURE_IMPROVEMENT_PLAN.md Phase D）
//! 本 crate 实现向量记忆（embedding + hnsw_rs + sled）。自 2026-08-07 起已通过
//! tui 的 `vector-memory` feature（已加入 `default` features）接入主流程，见
//! `crates/tui/src/vector_memory/mod.rs`：作为 `crate::memory` 文件型用户记忆的
//! **互补层**（文件记忆管确定性偏好，向量记忆管跨会话语义召回）。
//!
//! **运行时优雅降级**：仅当环境变量 `MIMOFAN_MEMORY_API_KEY` 配置时，才真正建立
//! embedding 服务与向量库（`enabled() == true`）；未配置时 `open()` 不报错、
//! `enabled() == false`、所有读写安全降级，对未配置用户零网络零磁盘副作用。
//!
//! 仍标记 experimental：语义召回质量、embedding 成本、sled 本地存储上限等
//! 仍在评估中。不要假设其行为已稳定，但可默认编译、按需启用。

pub mod backend;
pub mod category;
pub mod codebase;
pub mod compressor;
pub mod consolidation;
pub mod consolidation_stages;
pub mod embedding;
pub mod error;
pub mod injector;
pub mod knowledge;
pub mod optimization;
pub mod retrieval;
pub mod tokenizer;
pub mod user_profile;
pub mod vector;

pub use backend::{MemoryBackend, SharedMemoryBackend};
pub use category::MemoryCategory;
pub use codebase::{RetrievalHit, RetrievalSource};
pub use compressor::{CompressionStrategy, ObservationCompressor, SessionSummary};
pub use consolidation::{
    ACCESS_REINFORCE_GAIN, ConsolidationScheduler, DECAY_LAMBDA, DEDUP_SIMILARITY_THRESHOLD,
    DEFAULT_CONSOLIDATION_INTERVAL, DEFAULT_IMPORTANCE, IMPORTANCE_MAX, IMPORTANCE_MIN,
    MemoryEntry, MemoryKind, RETENTION_FREQ_WEIGHT, content_similarity, dedup, evict_to_budget,
    rollup,
};
pub use consolidation_stages::{
    ABSTRACT_TOPIC_MIN_FREQUENCY, DreamResult, EXTRACT_IMPORTANCE_THRESHOLD, abstract_rules,
    dream_cycle, extract, integrate,
};
pub use embedding::{ApiEmbedder, Embedder, EmbeddingConfig, EmbeddingService};
pub use error::MemoryError;
pub use injector::{InjectionConfig, MemoryInjection, MemoryInjector, MemoryStats};
pub use knowledge::{CorpusAnswer, CorpusSource, KnowledgeAgent, KnowledgeCorpus};
pub use optimization::{
    BatchProcessor, LongTaskManager, LongTaskResult, ObservationStore, RateLimiter, SearchCache,
};
pub use retrieval::{Assembler, ConcatAssembler, Retriever};
pub use user_profile::{
    Bucket, ProfileEntry, UserProfile, distill_from_transcript, distill_session,
    inject_user_profile, render_for_injection,
};
pub use vector::{MemoryOrigin, Observation, SearchFilters, VectorMatch, VectorStore};

/// Result type for memory operations
pub type Result<T> = std::result::Result<T, MemoryError>;
