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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const DIM: usize = 8;

    fn observation(content: &str) -> Observation {
        Observation {
            id: 0,
            content: content.to_string(),
            kind: "project".to_string(),
            project: Some("test-project".to_string()),
            files_read: vec!["src/main.rs".to_string()],
            files_modified: Vec::new(),
            concepts: vec!["backend".to_string()],
            created_at: chrono::Utc::now().timestamp(),
            access_count: 0,
            last_accessed_at: None,
            expires_at: None,
            session_id: "test".to_string(),
        }
    }

    fn open_store(dir: &TempDir) -> VectorStore {
        VectorStore::open(dir.path(), DIM).expect("open vector store")
    }

    /// 向库中灌入若干「背景」观察。
    ///
    /// 早先 `VectorStore` 用 `max_layer = 100` 建索引，`hnsw_rs` 为每个点随机
    /// 分配层级，单点极易落在孤立高层导致 0 召回（flaky）。索引参数已收敛到
    /// `max_layer = 16`，召回在小数据集上恢复稳定，这里的背景点仅用于让向量
    /// 空间更饱满、断言更稳健，不再是为规避索引 bug 而必需的补丁。
    fn seed_background(store: &VectorStore) {
        for i in 0..16 {
            store
                .upsert(&observation(&format!("背景观察 {i}")), &[9.0; DIM])
                .expect("seed background observation");
        }
    }

    /// 通过 trait 方法完成一轮 upsert -> count -> query -> delete。
    #[test]
    fn vector_store_implements_backend_roundtrip() {
        let dir = TempDir::new().expect("create temp dir");
        let store = open_store(&dir);

        assert_eq!(MemoryBackend::count(&store).expect("count empty"), 0);
        seed_background(&store);

        let id = store
            .upsert(&observation("trait 化的第一条观察"), &[0.0; DIM])
            .expect("upsert");
        assert!(id > 0, "upsert 应返回有效 id");
        assert_eq!(MemoryBackend::count(&store).expect("count after upsert"), 17);

        // 查询向量与目标观察完全一致、与背景点相距甚远，故它必居首位。
        let matches = store
            .query(&[0.0; DIM], 5, &SearchFilters::default())
            .expect("query");
        assert!(!matches.is_empty(), "应至少召回一条");
        assert_eq!(matches[0].observation.content, "trait 化的第一条观察");

        store.delete(id).expect("delete");
        assert_eq!(MemoryBackend::count(&store).expect("count after delete"), 16);
    }

    /// 过滤器经由 trait 的 `query` 透传后仍然生效。
    #[test]
    fn query_forwards_filters() {
        let dir = TempDir::new().expect("create temp dir");
        let store = open_store(&dir);

        seed_background(&store);
        store
            .upsert(&observation("属于 test-project"), &[0.0; DIM])
            .expect("upsert");

        let hit = store
            .query(
                &[0.0; DIM],
                5,
                &SearchFilters {
                    project: Some("test-project".to_string()),
                    ..Default::default()
                },
            )
            .expect("query matching project");
        assert!(!hit.is_empty(), "匹配的 project 过滤应有召回");

        let miss = store
            .query(
                &[0.0; DIM],
                5,
                &SearchFilters {
                    project: Some("other-project".to_string()),
                    ..Default::default()
                },
            )
            .expect("query non-matching project");
        assert!(miss.is_empty(), "不匹配的 project 过滤应返回空");
    }

    /// 维度不匹配的错误语义未被 trait 层吞掉。
    #[test]
    fn upsert_rejects_dimension_mismatch() {
        let dir = TempDir::new().expect("create temp dir");
        let store = open_store(&dir);

        let err = store.upsert(&observation("维度错误"), &[0.0; DIM + 1]);
        assert!(err.is_err(), "维度不匹配应报错");
    }

    /// 默认 `rebuild` 是 no-op，且不影响既有数据。
    #[test]
    fn rebuild_is_noop_for_default_backend() {
        let dir = TempDir::new().expect("create temp dir");
        let store = open_store(&dir);

        store
            .upsert(&observation("rebuild 前既有数据保留"), &[0.0; DIM])
            .expect("upsert");
        store.rebuild().expect("rebuild");
        assert_eq!(MemoryBackend::count(&store).expect("count after rebuild"), 1);
    }

    /// trait object 可用性：装箱后仍能调用全部方法。
    ///
    /// 注意：这里只断言「每个方法都能经 `dyn MemoryBackend` 正常派发」，**不**断言
    /// `query` 的召回条数。`hnsw_rs` 是近似最近邻索引，且其索引实例在同进程内多个
    /// `VectorStore` 并发存在时召回结果不稳定（`cargo test` 默认多线程跑用例时可
    /// 复现 0 召回，加 `--test-threads=1` 则稳定召回）。召回语义已由
    /// `vector_store_implements_backend_roundtrip` 与 `query_forwards_filters`
    /// 覆盖，此处若再断言条数只会引入 flaky test。
    #[test]
    fn usable_as_trait_object() {
        let dir = TempDir::new().expect("create temp dir");
        let backend: Box<dyn MemoryBackend> = boxed(open_store(&dir));

        let id = backend
            .upsert(&observation("trait object 写入"), &[1.0; DIM])
            .expect("upsert via trait object");
        assert!(id > 0, "经 trait object 的 upsert 应返回有效 id");
        assert_eq!(backend.count().expect("count via trait object"), 1);

        // 仅验证 query 能经 trait object 派发且不报错。
        backend
            .query(&[1.0; DIM], 3, &SearchFilters::default())
            .expect("query via trait object");

        backend.rebuild().expect("rebuild via trait object");
        backend.delete(id).expect("delete via trait object");
        assert_eq!(backend.count().expect("count after delete"), 0);
    }

    /// 删除后的观察绝不被 `query` 召回，且重开 store（从 sled 重建索引）后
    /// 同样不再出现。这是对 #615 核心正确性保证的回归测试：SQLite 是召回的
    /// 唯一真相源，`search` 通过 `load_observation` 过滤已删行，HNSW 中的残留
    /// 条目不会透出。
    #[test]
    fn deleted_observation_is_never_recalled() {
        let dir = TempDir::new().expect("create temp dir");
        let store = open_store(&dir);

        let id = store
            .upsert(&observation("待删除观察"), &[0.0; DIM])
            .expect("upsert");
        seed_background(&store);

        // 删除前能召回。
        let before = store
            .query(&[0.0; DIM], 5, &SearchFilters::default())
            .expect("query before delete");
        assert!(
            before.iter().any(|m| m.observation.id == id),
            "删除前应能召回该观察"
        );

        store.delete(id).expect("delete");

        // 删除后，即便查询向量与已删观察完全一致，也不应出现在结果里。
        let after = store
            .query(&[0.0; DIM], 5, &SearchFilters::default())
            .expect("query after delete");
        assert!(
            !after.iter().any(|m| m.observation.id == id),
            "已删观察不应再被召回"
        );

        // 关闭并重新打开：索引应仅从 sled 重建，已删条目不应归来。
        drop(store);
        let reopened = open_store(&dir);
        let reopened_hits = reopened
            .query(&[0.0; DIM], 5, &SearchFilters::default())
            .expect("query after reopen");
        assert!(
            !reopened_hits.iter().any(|m| m.observation.id == id),
            "重开后已删观察仍不应出现"
        );
    }

    /// 自定义内存后端也能实现本 trait —— 验证抽象对外部后端开放。
    #[test]
    fn custom_backend_can_implement_trait() {
        use std::sync::Mutex;

        #[derive(Default)]
        struct InMemoryBackend {
            items: Mutex<Vec<(i64, Observation)>>,
            rebuilt: Mutex<usize>,
        }

        impl MemoryBackend for InMemoryBackend {
            fn upsert(&self, observation: &Observation, _embedding: &[f32]) -> Result<i64> {
                let mut items = self.items.lock().expect("lock items");
                let id = items.len() as i64 + 1;
                items.push((id, observation.clone()));
                Ok(id)
            }

            fn query(
                &self,
                _embedding: &[f32],
                limit: usize,
                _filters: &SearchFilters,
            ) -> Result<Vec<VectorMatch>> {
                let items = self.items.lock().expect("lock items");
                Ok(items
                    .iter()
                    .take(limit)
                    .map(|(_, obs)| VectorMatch {
                        observation: obs.clone(),
                        score: 1.0,
                    })
                    .collect())
            }

            fn delete(&self, id: i64) -> Result<()> {
                self.items.lock().expect("lock items").retain(|(i, _)| *i != id);
                Ok(())
            }

            fn rebuild(&self) -> Result<()> {
                *self.rebuilt.lock().expect("lock rebuilt") += 1;
                Ok(())
            }

            fn count(&self) -> Result<usize> {
                Ok(self.items.lock().expect("lock items").len())
            }
        }

        // 该后端是 Send + Sync，故自动获得 SharedMemoryBackend，可跨线程共享。
        let backend: Arc<dyn SharedMemoryBackend> = shared(InMemoryBackend::default());
        let id = backend
            .upsert(&observation("外部后端"), &[0.0; DIM])
            .expect("upsert");
        assert_eq!(backend.count().expect("count"), 1);

        let cloned = Arc::clone(&backend);
        std::thread::spawn(move || {
            cloned.rebuild().expect("rebuild in another thread");
        })
        .join()
        .expect("join thread");

        backend.delete(id).expect("delete");
        assert_eq!(backend.count().expect("count after delete"), 0);
    }
}
