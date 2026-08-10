//! 向量记忆后端（feature-gated: `vector-memory`，默认开启）
//!
//! 本模块把僵尸 crate `mimofan-memory` 接入 tui，提供语义检索能力
//! （embedding + hnsw-rs + sled），作为 `crate::memory` 文件型用户记忆的
//! **互补**层：文件记忆负责确定性偏好，向量记忆负责跨会话语义召回。
//!
//! # 默认开启 + 运行时优雅降级
//!
//! `vector-memory` 已加入 tui 的 default features，编译进默认二进制。但运行
//! 时是否真正启用取决于环境变量 `MIMOFAN_MEMORY_API_KEY` 是否配置：
//!
//! - **已配置**：`open()` 建立 embedding 服务 + 向量库，`enabled()` 为 `true`，
//!   提供 remember / recall / list / 系统提示注入等能力。
//! - **未配置**：`open()` 不报错，仅将 `embeddings`/`store` 置为 `None`，
//!   `enabled()` 为 `false`。所有写/读操作安全降级（search 返回友好错误），
//!   零网络、零磁盘 I/O，对未配置用户完全无副作用。
//!
//! # Send 安全（关键约束）
//!
//! `VectorStore` 内部含非 `Send` 的 sqlite/RefCell 状态，因此 `&VectorMemory`
//! 本身不是 `Send`。工具执行器与引擎在跨线程的 `Send` future 中调用本模块，
//! 故 **绝不可在 `.await` 期间持有 `&VectorMemory`**。API 设计为：调用方先
//! `take_embedder()` 取出 `Send` 的 `EmbeddingService` 并持有它跨 `.await`，
//! 嵌入完成后再以同步方式调用 `store_observation` / `search_embedded`
//! （仅借用 `&self` 且无 await，不受 Send 限制）。

#[cfg(feature = "vector-memory")]
use std::path::{Path, PathBuf};

#[cfg(feature = "vector-memory")]
use anyhow::{Context, Result};
#[cfg(feature = "vector-memory")]
use mimofan_memory::{
    EmbeddingConfig, EmbeddingService, Observation, SearchFilters, VectorStore,
};
#[cfg(feature = "vector-memory")]
use mimofan_memory::MemoryCategory;

/// 向量记忆后端：持有 embedding 服务与向量库，提供 remember/recall/list/inject 的
/// 构建块。
///
/// `embeddings` 与 `store` 拆为独立 `Option` 字段，使 `.await` 期间只需持有
/// `Send` 的 `EmbeddingService`，绝不持有整块 `&VectorMemory`（非 `Send`）。
#[cfg(feature = "vector-memory")]
pub struct VectorMemory {
    embeddings: Option<EmbeddingService>,
    store: Option<VectorStore>,
    root: PathBuf,
    dimension: usize,
}

#[cfg(feature = "vector-memory")]
impl VectorMemory {
    /// 是否配置了 embedding 后端（运行时启用条件）。
    ///
    /// 供工具注册等场景判断，避免在缺少密钥时把 `remember_vector` 暴露给模型。
    #[must_use]
    pub fn is_configured() -> bool {
        std::env::var("MIMOFAN_MEMORY_API_KEY").is_ok()
    }

    /// 在 `<memory_dir>/vector` 下打开（或创建）向量记忆库。
    ///
    /// Embedding 配置取自环境变量：
    /// - `MIMOFAN_MEMORY_API_KEY`（OpenAI 兼容 embedding 密钥）
    /// - `MIMOFAN_MEMORY_API_BASE_URL`（默认 `https://api.openai.com/v1`，可指向 DeepSeek 等）
    /// - `MIMOFAN_MEMORY_MODEL`（默认 `text-embedding-3-small`）
    /// - `MIMOFAN_MEMORY_DIMENSION`（默认 `1536`；须与所选模型维度一致）
    ///
    /// **优雅降级**：若 `MIMOFAN_MEMORY_API_KEY` 缺失或 embedding 服务/向量库
    /// 初始化失败，`embeddings`/`store` 置为 `None` 并返回 `Ok`，不会令调用方
    /// （如引擎构造、命令、工具）失败。
    pub fn open(memory_dir: &Path) -> Result<Self> {
        let dimension: usize = std::env::var("MIMOFAN_MEMORY_DIMENSION")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1536);
        let root = memory_dir.join("vector");

        let (embeddings, store) = match Self::try_open_inner(&root, dimension) {
            Ok((emb, st)) => (Some(emb), Some(st)),
            Err(err) => {
                tracing::info!("vector-memory disabled (no embedding backend configured): {err}");
                (None, None)
            }
        };

        Ok(Self {
            embeddings,
            store,
            root,
            dimension,
        })
    }

    fn try_open_inner(root: &Path, dimension: usize) -> Result<(EmbeddingService, VectorStore)> {
        let api_key = std::env::var("MIMOFAN_MEMORY_API_KEY")
            .context("启用 vector-memory 需设置环境变量 MIMOFAN_MEMORY_API_KEY")?;
        let api_base_url = std::env::var("MIMOFAN_MEMORY_API_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let model = std::env::var("MIMOFAN_MEMORY_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-small".to_string());

        let emb_config = EmbeddingConfig {
            api_base_url,
            api_key,
            model,
            dimension,
        };
        let embeddings = EmbeddingService::new(emb_config)?;
        let store = VectorStore::open(root, dimension)?;
        Ok((embeddings, store))
    }

    /// 后端是否已真正启用（embedding 后端已配置且向量库已打开）。
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.embeddings.is_some() && self.store.is_some()
    }

    /// 取出 embedding 服务，供调用方在 `.await` 期间持有（仅 `EmbeddingService`
    /// 是 `Send`，而整块 `&VectorMemory` 不是）。取出后本后端不再可嵌入，符合
    /// "单次会话仅注入一次"等一次性使用场景。
    pub fn take_embedder(&mut self) -> Option<EmbeddingService> {
        self.embeddings.take()
    }

    /// 用预计算的 embedding 写入一条 observation（同步，不跨 await）。
    pub fn store_observation(
        &self,
        project: &str,
        kind: &str,
        content: &str,
        embedding: &[f32],
    ) -> Result<i64> {
        let store = self.store.as_ref().ok_or_else(|| {
            anyhow::anyhow!("vector-memory 未启用：请配置 MIMOFAN_MEMORY_API_KEY 后重启")
        })?;
        let obs = Observation::new(project.to_string(), kind, content.to_string());
        Ok(store.store_observation(&obs, embedding)?)
    }

    /// 用预计算的 embedding 做语义检索（同步，不跨 await）。
    pub fn search_embedded(
        &self,
        embedding: &[f32],
        project: Option<&str>,
        top_k: usize,
    ) -> Result<Vec<(Observation, f32)>> {
        let store = self.store.as_ref().ok_or_else(|| {
            anyhow::anyhow!("vector-memory 未启用：请配置 MIMOFAN_MEMORY_API_KEY 后重启")
        })?;
        let filters = SearchFilters {
            project: project.map(|p| p.to_string()),
            ..Default::default()
        };
        let matches = store.search(embedding, top_k, &filters)?;
        Ok(matches
            .into_iter()
            .map(|m| (m.observation, m.score))
            .collect())
    }

    /// 列出某项目最近的 observation（同步，按写入时间倒序）。
    ///
    /// 直接走向量库的 `created_at` 索引确定性排序，不依赖相似度检索，
    /// 因此返回结果严格按时间倒序（最新在前）。
    pub fn list_recent(&self, project: Option<&str>, limit: usize) -> Result<Vec<Observation>> {
        let store = self.store.as_ref().ok_or_else(|| {
            anyhow::anyhow!("vector-memory 未启用：请配置 MIMOFAN_MEMORY_API_KEY 后重启")
        })?;
        Ok(store.list_recent(project, limit)?)
    }

    /// 为系统提示生成 `<vector_memory>` 注入块（已启用且有召回结果时返回 `Some`）。
    ///
    /// 与 `crate::memory` 的文件记忆共用同一套 [`MemoryCategory`] 四分类
    /// （user/feedback/project/reference）：语义召回更适合"跨会话想起相关项目
    /// 背景/偏好"，而文件记忆适合确定性偏好。二者并列出现在系统提示中，互不影响。
    ///
    /// 调用方负责取出 embedder 并跨 await 完成嵌入，再传入本同步方法。
    pub fn format_injection_block(project: &str, matches: &[(Observation, f32)]) -> Option<String> {
        if matches.is_empty() {
            return None;
        }
        let mut lines = Vec::with_capacity(matches.len() + 2);
        lines.push(format!("<vector_memory project=\"{project}\">"));
        for (obs, score) in matches {
            lines.push(format!(
                "- [{}] {}  (score {:.2})",
                obs.kind, obs.content, score
            ));
        }
        lines.push("</vector_memory>".to_string());
        Some(lines.join("\n"))
    }

    /// 记忆库根目录（供 `/vmemory` 命令展示状态）。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// embedding 维度（供 `/vmemory` 命令展示状态）。
    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

/// 解析记忆分类字符串（供 `/vmemory` 命令与工具使用），复用文件记忆的唯一
/// 权威分类 [`MemoryCategory`]。
#[cfg(feature = "vector-memory")]
pub fn parse_memory_category(s: &str) -> Result<MemoryCategory> {
    MemoryCategory::from_str(s)
        .ok_or_else(|| anyhow::anyhow!("无效的记忆分类 `{s}`，应为 user/feedback/project/reference 之一"))
}

#[cfg(all(test, feature = "vector-memory"))]
mod tests {
    use super::*;
    use mimofan_memory::MemoryCategory;
    use mimofan_memory::Observation;

    fn obs(kind: &str, content: &str) -> Observation {
        Observation {
            id: 0,
            content: content.to_string(),
            kind: kind.to_string(),
            project: Some("demo".to_string()),
            files_read: Vec::new(),
            files_modified: Vec::new(),
            concepts: Vec::new(),
            created_at: 0,
        }
    }

    #[test]
    fn is_configured_false_without_api_key() {
        // Ensure the key is unset for this assertion; tests run in a clean
        // environment, but guard against CI leaking it.
        unsafe { std::env::remove_var("MIMOFAN_MEMORY_API_KEY"); };
        assert!(!VectorMemory::is_configured());
    }

    #[test]
    fn open_without_backend_is_disabled_and_safe() {
        unsafe { std::env::remove_var("MIMOFAN_MEMORY_API_KEY"); };
        let vm = VectorMemory::open(std::path::Path::new("/tmp/__vm_test_none")).unwrap();
        assert!(!vm.enabled());
    }

    #[test]
    fn parse_memory_category_roundtrip() {
        assert_eq!(
            parse_memory_category("project").unwrap(),
            MemoryCategory::Project
        );
        assert_eq!(
            parse_memory_category("FEEDBACK").unwrap(),
            MemoryCategory::Feedback
        );
        assert!(parse_memory_category("Nonsense").is_err());
    }

    #[test]
    fn injection_block_none_when_empty() {
        assert!(VectorMemory::format_injection_block("demo", &[]).is_none());
    }

    #[test]
    fn injection_block_renders_project_and_entries() {
        let matches = vec![(obs("project", "renamed module"), 0.91)];
        let block = VectorMemory::format_injection_block("demo", &matches).unwrap();
        assert!(block.starts_with("<vector_memory project=\"demo\">"));
        assert!(block.contains("[project] renamed module"));
        assert!(block.contains("score 0.91"));
        assert!(block.trim_end().ends_with("</vector_memory>"));
    }
}

