//! 记忆存储后端抽象（可插拔 backend）
//!
//! 本模块把「记忆怎么存」从「记忆存在哪」中解耦：`MemoryBackend` 描述统一的
//! upsert / query / delete / rebuild / count 语义，本 crate 内置的
//! [`VectorStore`](crate::vector::VectorStore)（sqlite + sled + hnsw）作为默认
//! 实现，外部系统（mem0 / honcho / 远端向量服务等）只要实现本 trait 即可接入，
//! 无需改动 [`crate::injector`] / [`crate::knowledge`] 等消费方。
//!
//! # 为什么 trait 不要求 `Sync`
//!
//! 默认实现 `VectorStore` 内部持有 `rusqlite::Connection`，它是 `Send` 但**不是**
//! `Sync`（sqlite 句柄不允许多线程并发共享引用）。若在此处写下
//! `trait MemoryBackend: Send + Sync`，`impl MemoryBackend for VectorStore` 会直接
//! 编译失败，默认后端反而进不了这套抽象。
//!
//! 因此这里只要求 [`Send`]：足以支持 `Box<dyn MemoryBackend>` 在线程间**移动**
//! （tui 侧 `VectorMemory` 正是这种用法——跨 `.await` 只持有 `Send` 的
//! `EmbeddingService`，向量库本身仅在同步区借用）。对于真正线程安全的远端后端，
//! 可额外实现标记 trait [`SharedMemoryBackend`]，从而以 `Arc<dyn SharedMemoryBackend>`
//! 形式在多线程间共享。

use std::sync::Arc;

use crate::Result;
use crate::vector::{Observation, SearchFilters, VectorMatch, VectorStore};

/// 记忆存储后端的统一抽象。
///
/// 现有 [`VectorStore`] 实现作为默认 backend，外部系统（mem0 / honcho 等）
/// 可通过实现本 trait 接入。
///
/// 方法签名与 [`VectorStore`] 的既有方法保持一致（包括 `query` 沿用
/// `search(embedding, limit, filters)` 的参数顺序），返回值统一为
/// [`crate::Result`]，便于默认实现零成本委托。
pub trait MemoryBackend: Send {
    /// 写入/更新一条观察，返回其存储 id。
    ///
    /// `embedding` 的维度必须与后端初始化时声明的维度一致，否则应返回
    /// [`MemoryError::DimensionMismatch`](crate::error::MemoryError::DimensionMismatch)。
    fn upsert(&self, observation: &Observation, embedding: &[f32]) -> Result<i64>;

    /// 按向量相似度 + 过滤器检索，最多返回 `limit` 条。
    fn query(
        &self,
        embedding: &[f32],
        limit: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<VectorMatch>>;

    /// 按 id 删除一条观察。删除不存在的 id 视为成功（幂等）。
    fn delete(&self, id: i64) -> Result<()>;

    /// 重建索引。
    ///
    /// 默认实现为 no-op：内置后端在 `upsert` 时即时维护 HNSW 索引，无需显式重建；
    /// 外部后端若有离线索引流程可覆写本方法。
    fn rebuild(&self) -> Result<()> {
        Ok(())
    }

    /// 统计当前条目数。
    fn count(&self) -> Result<usize>;
}

/// 线程安全后端的标记 trait。
///
/// 仅当后端同时满足 `Send + Sync` 时才应实现它——此时可用
/// `Arc<dyn SharedMemoryBackend>` 在多线程间共享同一后端实例。内置的
/// [`VectorStore`] **不**实现本 trait（sqlite 句柄非 `Sync`）；远端 HTTP 后端
/// 之类的实现通常可以。
pub trait SharedMemoryBackend: MemoryBackend + Sync {}

impl<T: MemoryBackend + Sync> SharedMemoryBackend for T {}

/// 默认后端：把 trait 方法逐一委托给 [`VectorStore`] 的既有实现。
impl MemoryBackend for VectorStore {
    fn upsert(&self, observation: &Observation, embedding: &[f32]) -> Result<i64> {
        self.store_observation(observation, embedding)
    }

    fn query(
        &self,
        embedding: &[f32],
        limit: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<VectorMatch>> {
        self.search(embedding, limit, filters)
    }

    fn delete(&self, id: i64) -> Result<()> {
        self.delete_observation(id)
    }

    // rebuild 沿用默认 no-op：HNSW 索引在 open() 时从 sled 重建、
    // 在 store_observation 时增量插入，无独立重建阶段。

    fn count(&self) -> Result<usize> {
        VectorStore::count(self)
    }
}

/// 便捷构造：把任意后端装箱为 trait object。
///
/// 供消费方以「后端无关」的方式持有实现，例如
/// `let backend = boxed(VectorStore::open(path, 384)?);`
pub fn boxed<B: MemoryBackend + 'static>(backend: B) -> Box<dyn MemoryBackend> {
    Box::new(backend)
}

/// 便捷构造：把线程安全后端装入 [`Arc`]，用于多线程共享。
pub fn shared<B: SharedMemoryBackend + 'static>(backend: B) -> Arc<dyn SharedMemoryBackend> {
    Arc::new(backend)
}
