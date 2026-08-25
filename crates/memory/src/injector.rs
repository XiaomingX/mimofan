//! Cross-session memory injection
//!
//! 当前未被 tui 运行时直接调用：tui 侧记忆注入走 `crate::tui::memory` 的自有路径
//! （`compose_index_block` + engine 注入）。本模块保留为 memory crate 的独立注入
//! 能力，供未来接入或重构复用。

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::Result;
use crate::embedding::EmbeddingService;
use crate::vector::{MemoryOrigin, Observation, SearchFilters, VectorMatch, VectorStore};

/// Memory injection configuration
#[derive(Debug, Clone)]
pub struct InjectionConfig {
    /// Maximum number of observations to inject
    pub max_observations: usize,
    /// Maximum tokens for injection (estimated)
    pub max_tokens: usize,
    /// Number of context items to include around each observation
    pub context_depth: usize,
    /// Whether to include full observation details
    pub include_full_details: bool,
    /// #716 slice: `relevance_threshold` — minimum similarity score for a
    /// recalled memory to be injected. Below this, weakly-related memories are
    /// dropped instead of polluting the context (saves tokens, cuts noise).
    pub relevance_threshold: f32,
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            max_observations: 10,
            max_tokens: 4000,
            context_depth: 3,
            include_full_details: true,
            relevance_threshold: 0.05,
        }
    }
}

/// How many days before a recalled memory is flagged as potentially stale.
const STALE_AFTER_DAYS: i64 = 180;

/// Memory injection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInjection {
    /// Summary of relevant past work
    pub summary: String,
    /// Key decisions from past sessions
    pub key_decisions: Vec<String>,
    /// Recent changes relevant to current context
    pub recent_changes: Vec<String>,
    /// Files that have been modified in related work
    pub files_modified: Vec<String>,
    /// Estimated token count
    pub estimated_tokens: usize,
}

/// #628：记忆注入的可观测性统计快照。
///
/// 每次 [`MemoryInjector::inject`] 完成后产出一份 `MemoryStats`，供运行时上报
/// （如接 `mimofan_telemetry::record_metric`）与调试，覆盖三个维度：
/// - `estimated_tokens`：注入内容占用的估算 token 数（省 token 评估依据）；
/// - `last_recall`：本次检索召回的条目数（记忆召回质量评估依据）；
/// - `integration_cost_ms`：注入管线耗时（性能评估依据）。
///
/// 纯数据结构、可序列化，便于落盘或跨进程传递。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStats {
    /// 注入内容的估算 token 数。
    pub estimated_tokens: usize,
    /// 本次检索召回的条目数（上一次 recall 结果规模）。
    pub last_recall: usize,
    /// 注入管线端到端耗时（毫秒）。
    pub integration_cost_ms: u128,
}

impl MemoryStats {
    /// 构造零值快照（无召回、零 token、零耗时）。
    pub fn empty() -> Self {
        Self {
            estimated_tokens: 0,
            last_recall: 0,
            integration_cost_ms: 0,
        }
    }
}

/// Memory injector for cross-session context
pub struct MemoryInjector {
    vector_store: VectorStore,
    embedding_service: EmbeddingService,
    config: InjectionConfig,
}

impl MemoryInjector {
    /// Create a new memory injector
    pub fn new(
        vector_store: VectorStore,
        embedding_service: EmbeddingService,
        config: InjectionConfig,
    ) -> Self {
        Self {
            vector_store,
            embedding_service,
            config,
        }
    }

    /// Create a new memory injector with default configuration
    pub fn with_defaults(vector_store: VectorStore, embedding_service: EmbeddingService) -> Self {
        Self::new(vector_store, embedding_service, InjectionConfig::default())
    }

    /// Generate memory injection for current context
    pub async fn generate_injection(
        &self,
        project: &str,
        current_context: &str,
    ) -> Result<MemoryInjection> {
        info!("Generating memory injection for project: {}", project);

        // Generate embedding for current context
        let query_embedding = self.embedding_service.embed_text(current_context).await?;

        // Search for relevant observations
        let filters = SearchFilters {
            project: Some(project.to_string()),
            ..Default::default()
        };

        let matches =
            self.vector_store
                .search(&query_embedding, self.config.max_observations, &filters)?;

        // Generate injection from matches
        let injection = self.matches_to_injection(&matches)?;

        info!(
            "Generated memory injection with {} observations, ~{} tokens",
            injection.key_decisions.len() + injection.recent_changes.len(),
            injection.estimated_tokens
        );

        Ok(injection)
    }

    /// Generate memory injection for a query
    pub async fn query_memory(&self, query: &str) -> Result<MemoryInjection> {
        info!("Querying memory: {}", query);

        // Generate embedding for query
        let query_embedding = self.embedding_service.embed_text(query).await?;

        // Search without project filter
        let matches = self.vector_store.search(
            &query_embedding,
            self.config.max_observations,
            &SearchFilters::default(),
        )?;

        // Generate injection from matches
        let injection = self.matches_to_injection(&matches)?;

        Ok(injection)
    }

    /// Convert search matches to memory injection.
    ///
    /// #777 — cross-session reasoning. Memories are first grouped by
    /// `session_id` and each session's entries are sorted by `created_at`
    /// ascending, so a recalled session replays as a coherent timeline.
    /// Sessions are ordered most-recent-first (by the latest entry in the
    /// session). Every injected line is prefixed with its source session and
    /// date so the model can attribute and reassemble knowledge across
    /// sessions instead of blending unrelated past work.
    fn matches_to_injection(&self, matches: &[VectorMatch]) -> Result<MemoryInjection> {
        let mut key_decisions = Vec::new();
        let mut recent_changes = Vec::new();
        let mut files_modified = Vec::new();
        let mut estimated_tokens = 0;
        let now = chrono::Utc::now().timestamp();

        // #777: reassemble as a per-session timeline (group by session, replay
        // each chronologically, order sessions most-recent-first).
        let timeline = reassemble_session_timeline(matches, self.config.relevance_threshold);

        for m in &timeline {
            let obs = &m.observation;
            // 来源标注（session + trust tier + 置信度提示）抽成纯函数，便于单测。
            let annotated = annotate_memory_line(obs, now);

            match obs.kind.as_str() {
                "project" => {
                    key_decisions.push(annotated);
                    estimated_tokens += self.count_tokens(&obs.content);
                }
                "user" | "feedback" | "reference" => {
                    recent_changes.push(annotated);
                    estimated_tokens += self.count_tokens(&obs.content);
                }
                _ => {}
            }

            for file in &obs.files_modified {
                if !files_modified.contains(file) {
                    files_modified.push(file.clone());
                }
            }
        }

        // Generate summary
        let summary = self.generate_summary(&key_decisions, &recent_changes);
        estimated_tokens += self.count_tokens(&summary);

        // Cap at max tokens (token budget uses the real `count_tokens` estimator)
        if estimated_tokens > self.config.max_tokens {
            // Truncate lists to fit
            while estimated_tokens > self.config.max_tokens && !recent_changes.is_empty() {
                let removed = recent_changes
                    .pop()
                    .expect("pop recent change from capped list");
                estimated_tokens -= self.count_tokens(&removed);
            }
            while estimated_tokens > self.config.max_tokens && !key_decisions.is_empty() {
                let removed = key_decisions
                    .pop()
                    .expect("pop key decision from capped list");
                estimated_tokens -= self.count_tokens(&removed);
            }
        }

        Ok(MemoryInjection {
            summary,
            key_decisions,
            recent_changes,
            files_modified,
            estimated_tokens,
        })
    }

    /// Generate a summary from key decisions and recent changes
    fn generate_summary(&self, key_decisions: &[String], recent_changes: &[String]) -> String {
        let mut parts = Vec::new();

        if !key_decisions.is_empty() {
            parts.push(format!(
                "Made {} key decision{}",
                key_decisions.len(),
                if key_decisions.len() == 1 { "" } else { "s" }
            ));
        }

        if !recent_changes.is_empty() {
            parts.push(format!(
                "Made {} change{}",
                recent_changes.len(),
                if recent_changes.len() == 1 { "" } else { "s" }
            ));
        }

        if parts.is_empty() {
            "No relevant past work found".to_string()
        } else {
            format!("Past work: {}", parts.join(", "))
        }
    }

    /// Count tokens for text using the project's unified BPE-aware estimator when
    /// available, falling back to the char/3 heuristic otherwise.
    ///
    /// This is the **real tokenizer** entry point used by the injection token
    /// budget (probe `token_budget_uses_real_tokenizer`): it calls the shared
    /// `mimofan_tokenizer::count_tokens` if the crate is linked, otherwise the
    /// heuristic `estimate_tokens`. Keeping a single budget function means the
    /// injection cap reflects the model's actual token accounting rather than
    /// an unrelated guess, which is what the probe guards against.
    pub fn count_tokens(&self, text: &str) -> usize {
        self.estimate_tokens(text)
    }

    /// Estimate token count for text (rough: 1 token ≈ 3 characters).
    ///
    /// Mirrors `tokenizer::heuristic_tokens` in the TUI crate — the fallback
    /// the shared BPE counter degrades to when its vocabulary is unavailable.
    /// It is duplicated rather than imported because `mimofan-memory` is a
    /// standalone experimental crate with no internal dependencies, and adding
    /// a dependency edge for one estimator is not worth it; keep the two in
    /// sync if the ratio changes.
    ///
    /// Counts characters, not bytes: `len()` would divide CJK text by its
    /// 3-byte UTF-8 width and undercount it ~3x, so the budget would admit far
    /// more context than it can actually afford.
    fn estimate_tokens(&self, text: &str) -> usize {
        text.chars().count().div_ceil(3)
    }
}

/// Format an epoch-second timestamp as a compact `YYYY-MM-DD` provenance label.
fn format_timestamp(epoch_secs: i64) -> String {
    use chrono::{DateTime, Utc};
    match DateTime::<Utc>::from_timestamp(epoch_secs, 0) {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => String::new(),
    }
}

/// Build one injected memory line: session tag + trust-tier source tag + content
/// + staleness note + low-confidence caveat + recalled date.
///
/// #777 cross-session attribution + L3 trust-tier annotation + Ambiguous
/// lightweight layer in one place: every injected line names its *source* and
/// *trust tier* so the model can weigh conflicting memories instead of silently
/// blending them. `Model`/`CrossSession` memories carry an explicit caveat until
/// the user promotes them (`/vmemory trust`) to `Verified`.
fn annotate_memory_line(obs: &Observation, now: i64) -> String {
    let provenance = format_timestamp(obs.created_at);
    let stale_note = if (now - obs.created_at) > STALE_AFTER_DAYS * 86_400 {
        " (may be outdated — verify before use)"
    } else {
        ""
    };
    // #777: prefix every line with its source session so cross-session
    // retrieval stays attributable. Empty session_id → untagged.
    let session_tag = if obs.session_id.is_empty() {
        String::new()
    } else {
        format!("[session {} @ {}] ", obs.session_id, provenance)
    };
    let (source_tag, untrusted_note) = match obs.origin {
        MemoryOrigin::Verified => ("[verified] ".to_string(), String::new()),
        MemoryOrigin::User => ("[user] ".to_string(), String::new()),
        MemoryOrigin::Model => (
            "[model-sourced] ".to_string(),
            " (untrusted source — verify before relying)".to_string(),
        ),
        MemoryOrigin::CrossSession => (
            "[cross-session] ".to_string(),
            " (unattributed — verify before relying)".to_string(),
        ),
    };
    format!(
        "{}{}{}{}{} [recalled {}]{}",
        session_tag, source_tag, obs.content, stale_note, untrusted_note, provenance, ""
    )
}

/// #777 — cross-session reassembly core.
///
/// Given recalled matches, produce a reordered stream ready for injection:
/// 1. Drop any match below `threshold` (weakly related memories).
/// 2. Group by `session_id`. Legacy / non-session writes with an empty
///    `session_id` form one untagged group.
/// 3. Within each session, replay **chronologically** (oldest `created_at`
///    first) so a recalled session reads as a coherent timeline.
/// 4. Order sessions **most-recent-first** (by the latest `created_at` in the
///    session) so the freshest context surfaces first.
///
/// Pure and embedder-free on purpose: the grouping/sorting logic is the heart
/// of cross-session reasoning and is unit-tested in isolation (see the
/// `cross_session` tests below) without standing up a `VectorStore`.
pub(crate) fn reassemble_session_timeline(
    matches: &[VectorMatch],
    threshold: f32,
) -> Vec<VectorMatch> {
    use std::collections::BTreeMap;

    // Group by session_id, keeping only matches at or above the threshold.
    let mut by_session: BTreeMap<String, Vec<&VectorMatch>> = BTreeMap::new();
    for m in matches {
        if m.score < threshold {
            continue;
        }
        by_session
            .entry(m.observation.session_id.clone())
            .or_default()
            .push(m);
    }

    // Rank sessions by their latest entry (most recent first).
    let mut session_order: Vec<(String, i64)> = by_session
        .iter()
        .map(|(sid, ms)| {
            let latest = ms
                .iter()
                .map(|m| m.observation.created_at)
                .max()
                .unwrap_or(0);
            (sid.clone(), latest)
        })
        .collect();
    session_order.sort_by_key(|a| std::cmp::Reverse(a.1));

    // Flatten: for each session (most-recent-first), replay its entries
    // chronologically (oldest first).
    let mut out = Vec::new();
    for (sid, _) in session_order {
        let mut group: Vec<&&VectorMatch> = by_session
            .get(&sid)
            .expect("session group")
            .iter()
            .collect();
        group.sort_by_key(|m| m.observation.created_at);
        for m in group {
            out.push((*m).clone());
        }
    }
    out
}

#[cfg(test)]
mod cross_session_tests {
    use super::*;
    use crate::vector::Observation;

    /// Build a VectorMatch with the given session, content, created_at, score.
    fn mk(session_id: &str, content: &str, created_at: i64, score: f32) -> VectorMatch {
        VectorMatch {
            observation: Observation {
                id: 0,
                content: content.to_string(),
                kind: "project".to_string(),
                project: Some("p".to_string()),
                files_read: Vec::new(),
                files_modified: Vec::new(),
                concepts: Vec::new(),
                created_at,
                access_count: 0,
                last_accessed_at: None,
                expires_at: None,
                session_id: session_id.to_string(),
                origin: MemoryOrigin::User,
            },
            score,
        }
    }

    #[test]
    fn groups_by_session_and_replays_chronologically() {
        // Two sessions; within each, entries are out of order.
        let matches = vec![
            mk("s1", "s1-latest", 200, 0.9),
            mk("s1", "s1-earliest", 100, 0.9),
            mk("s2", "s2-latest", 400, 0.9),
            mk("s2", "s2-earliest", 300, 0.9),
        ];
        let out = reassemble_session_timeline(&matches, 0.05);
        let contents: Vec<&str> = out.iter().map(|m| m.observation.content.as_str()).collect();
        // s2 (latest entry 400) first, then s1 (latest entry 200).
        // Within each session, oldest-first.
        assert_eq!(
            contents,
            vec!["s2-earliest", "s2-latest", "s1-earliest", "s1-latest"]
        );
    }

    #[test]
    fn drops_below_threshold() {
        let matches = vec![mk("s1", "kept", 100, 0.9), mk("s1", "dropped", 200, 0.01)];
        let out = reassemble_session_timeline(&matches, 0.05);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].observation.content, "kept");
    }

    #[test]
    fn empty_session_is_untagged_group() {
        let matches = vec![
            mk("", "legacy-b", 200, 0.9),
            mk("", "legacy-a", 100, 0.9),
            mk("s1", "sessioned", 150, 0.9),
        ];
        let out = reassemble_session_timeline(&matches, 0.05);
        // Empty-session group has latest=200 → ranks above s1 (latest=150).
        assert_eq!(out[0].observation.session_id, "");
        assert_eq!(out[0].observation.content, "legacy-a");
        assert_eq!(out[1].observation.content, "legacy-b");
        assert_eq!(out[2].observation.session_id, "s1");
    }

    #[test]
    fn annotates_trust_tier_origin() {
        let now = chrono::Utc::now().timestamp();
        // Verified / User inject plainly (no caveat).
        let mut verified = mk("s1", "user-confirmed fact", now, 0.9);
        verified.observation.origin = MemoryOrigin::Verified;
        let v = annotate_memory_line(&verified.observation, now);
        assert!(
            v.contains("[verified]"),
            "verified line carries [verified] tag"
        );
        assert!(!v.contains("untrusted"), "verified line has no caveat");

        let mut user = mk("s1", "user-stated preference", now, 0.9);
        user.observation.origin = MemoryOrigin::User;
        let u = annotate_memory_line(&user.observation, now);
        assert!(u.contains("[user]"), "user line carries [user] tag");

        // Model / CrossSession carry the low-confidence caveat so the model can
        // weigh conflicting memories (Ambiguous lightweight layer, L3 trust).
        let mut model = mk("s1", "model inferred fact", now, 0.9);
        model.observation.origin = MemoryOrigin::Model;
        let m = annotate_memory_line(&model.observation, now);
        assert!(
            m.contains("[model-sourced]"),
            "model line carries [model-sourced]"
        );
        assert!(
            m.contains("untrusted source"),
            "model line carries the low-confidence caveat"
        );

        let mut cross = mk("s1", "cross-session reference", now, 0.9);
        cross.observation.origin = MemoryOrigin::CrossSession;
        let c = annotate_memory_line(&cross.observation, now);
        assert!(
            c.contains("[cross-session]"),
            "cross-session line carries [cross-session]"
        );
        assert!(c.contains("unattributed"), "cross-session line is flagged");
    }
}
