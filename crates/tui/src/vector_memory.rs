//! 向量记忆后端（feature-gated: `vector-memory`）
//!
//! 本模块把僵尸 crate `mimofan-memory` 接入 tui，提供语义检索能力
//! （embedding + hnsw-rs + sled），作为 `crate::memory` 文件型用户记忆的
//! **互补**层：文件记忆负责确定性偏好，向量记忆负责跨会话语义召回。
//!
//! 默认关闭，需构建时 `--features vector-memory` 且运行时配置 embedding API。
//! 不替换生产路径中的 `crate::memory`，对默认构建零影响。

#[cfg(feature = "vector-memory")]
use std::path::{Path, PathBuf};

#[cfg(feature = "vector-memory")]
use anyhow::{Context, Result};
#[cfg(feature = "vector-memory")]
use mimofan_memory::{
    EmbeddingConfig, EmbeddingService, MemoryInjection, Observation, ObservationKind,
    SearchFilters, VectorStore,
};

/// 向量记忆后端：持有向量库与 embedding 服务，提供 remember/recall/list/inject。
#[cfg(feature = "vector-memory")]
pub struct VectorMemoryBackend {
    store: VectorStore,
    embeddings: EmbeddingService,
    dimension: usize,
    root: PathBuf,
}

#[cfg(feature = "vector-memory")]
impl VectorMemoryBackend {
    /// 在 `<memory_dir>/vector` 下打开（或创建）向量记忆库。
    ///
    /// Embedding 配置取自环境变量：
    /// - `MIMOFAN_MEMORY_API_KEY`（必填，OpenAI 兼容 embedding 密钥）
    /// - `MIMOFAN_MEMORY_API_BASE_URL`（默认 `https://api.openai.com/v1`，可指向 DeepSeek 等）
    /// - `MIMOFAN_MEMORY_MODEL`（默认 `text-embedding-3-small`）
    /// - `MIMOFAN_MEMORY_DIMENSION`（默认 `1536`；须与所选模型维度一致）
    pub fn open(memory_dir: &Path) -> Result<Self> {
        let api_key = std::env::var("MIMOFAN_MEMORY_API_KEY")
            .context("启用 vector-memory 需设置环境变量 MIMOFAN_MEMORY_API_KEY")?;
        let api_base_url = std::env::var("MIMOFAN_MEMORY_API_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let model =
            std::env::var("MIMOFAN_MEMORY_MODEL").unwrap_or_else(|_| "text-embedding-3-small".to_string());
        let dimension: usize = std::env::var("MIMOFAN_MEMORY_DIMENSION")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1536);

        let emb_config = EmbeddingConfig {
            api_base_url,
            api_key,
            model,
            dimension,
        };
        let embeddings = EmbeddingService::new(emb_config)?;
        let root = memory_dir.join("vector");
        let store = VectorStore::open(&root, dimension)?;

        Ok(Self {
            store,
            embeddings,
            dimension,
            root,
        })
    }

    /// 记录一条 observation（自动 embedding 后写入向量库）。
    pub async fn remember(
        &self,
        project: &str,
        kind: ObservationKind,
        content: &str,
    ) -> Result<i64> {
        let embedding = self.embeddings.embed_text(content).await?;
        let obs = Observation::new(project.to_string(), kind, content.to_string());
        let id = self.store.store_observation(&obs, &embedding)?;
        Ok(id)
    }

    /// 语义检索与查询最相关的 observation（按相似度降序）。
    pub async fn recall(
        &self,
        project: Option<&str>,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<(Observation, f32)>> {
        let query_emb = self.embeddings.embed_text(query).await?;
        let filters = SearchFilters {
            project: project.map(|p| p.to_string()),
            ..Default::default()
        };
        let matches = self.store.search(&query_emb, top_k, &filters)?;
        Ok(matches.into_iter().map(|m| (m.observation, m.score)).collect())
    }

    /// 列出某项目最近的 observation。
    ///
    /// 注：向量库按相似度检索，无原生"按时间列出"接口；此处用零向量近似召回
    /// 最近写入的条目（feature-gated 实验实现，结果不保证严格时间序）。
    pub async fn list_recent(
        &self,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Observation>> {
        let filters = SearchFilters {
            project: project.map(|p| p.to_string()),
            ..Default::default()
        };
        let zero = vec![0.0f32; self.dimension];
        let matches = self.store.search(&zero, limit, &filters)?;
        Ok(matches.into_iter().map(|m| m.observation).collect())
    }

    /// 为系统提示生成跨会话记忆注入摘要（互补于文件记忆，默认不启用）。
    pub async fn inject(&self, project: &str, current_context: &str) -> Result<MemoryInjection> {
        let results = self.recall(Some(project), current_context, 10).await?;
        let mut key_decisions = Vec::new();
        let mut recent_changes = Vec::new();
        let mut files_modified = Vec::new();
        let mut summary_parts = Vec::new();
        let mut estimated_tokens = 0usize;

        for (obs, _score) in results {
            estimated_tokens += obs.content.chars().count() / 2;
            summary_parts.push(format!("- [{}] {}", obs.kind, obs.content));
            match obs.kind {
                ObservationKind::Decision => key_decisions.push(obs.content.clone()),
                ObservationKind::Change => {
                    key_decisions.push(obs.content.clone());
                    recent_changes.push(obs.content.clone());
                }
                ObservationKind::Feature => recent_changes.push(obs.content.clone()),
                _ => {}
            }
            for f in obs.files_modified {
                if !files_modified.contains(&f) {
                    files_modified.push(f);
                }
            }
        }

        Ok(MemoryInjection {
            summary: summary_parts.join("\n"),
            key_decisions,
            recent_changes,
            files_modified,
            estimated_tokens,
        })
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

/// 解析 observation kind 字符串（供 `/vmemory` 命令与工具使用）。
#[cfg(feature = "vector-memory")]
pub fn parse_observation_kind(s: &str) -> Result<ObservationKind> {
    s.parse::<ObservationKind>()
        .map_err(|e| anyhow::anyhow!("无效的 observation kind `{s}`: {e}"))
}
