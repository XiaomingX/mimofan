//! 记忆系统可观测统计（issue #628）。
//!
//! 设计为**按需快照**：在 `/status`、`/debug` 被调用时实时读取文件式记忆
//! 目录，不产生任何埋点或持久计数。这样所有展示的数字都有真实数据源，
//! 不会出现"静默给出的合理数字"（假指标）。
//!
//! 维度取舍：
//! - `enabled` / `index_present` / `categories_present` / `index_token_estimate`
//!   全部取自文件式记忆（`crate::memory`），默认启用、零 feature、零网络。
//! - `last_recall`：文件式记忆无召回时间戳字段，明确标注 n/a，不编造。
//! - `consolidation_*`：整个项目当前无记忆整合触发点，明确标注 not tracked。
//! - 向量式记忆（需 `vector-memory` feature + embedding 配置）的统计由
//!   `/vmemory` 命令覆盖，本快照不重复造数，避免假指标。

use std::path::Path;

use crate::compaction::estimate_text_tokens_conservative;
use crate::memory::{self, CATEGORIES};

/// 记忆系统健康度快照。所有字段均为实时读取，无累计状态。
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// 记忆是否在当前会话启用（`use_memory`）。
    pub enabled: bool,
    /// `MEMORY.md` 索引是否存在且非空。
    pub index_present: bool,
    /// 四分类文件中非空（去空白后）的数量。
    pub categories_present: usize,
    /// 当前记忆索引块（含路径作用域 bullets）的 token 估算（快照，非累计）。
    pub index_token_estimate: usize,
    /// 召回时间戳说明：文件式记忆无此字段。
    pub last_recall_note: &'static str,
    /// 整合触发说明：项目内无整合入口。
    pub consolidation_note: &'static str,
}

impl MemoryStats {
    /// 渲染为多行文本，供 `/debug memory` 展示。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Memory Stats");
        let _ = writeln!(out, "=============");
        let _ = writeln!(out, "  enabled:            {}", self.enabled);
        let _ = writeln!(out, "  index_present:      {}", self.index_present);
        let _ = writeln!(
            out,
            "  categories_present: {} / {}",
            self.categories_present,
            CATEGORIES.len()
        );
        let _ = writeln!(
            out,
            "  index_token_est:    {} tokens (snapshot)",
            self.index_token_estimate
        );
        let _ = writeln!(out, "  last_recall:        {}", self.last_recall_note);
        let _ = writeln!(out, "  consolidation:      {}", self.consolidation_note);
        let _ = writeln!(
            out,
            "  vector_memory:      see `/vmemory` (requires MIMOFAN_MEMORY_API_KEY)"
        );
        out
    }

    /// 单行摘要，供 `/status` 展示。
    pub fn one_line(&self) -> String {
        if !self.enabled {
            return "disabled".to_string();
        }
        format!(
            "enabled, {} cat, ~{} tok",
            self.categories_present, self.index_token_estimate
        )
    }
}

/// 计算当前文件式记忆的快照统计。
///
/// `active_paths` 传 `None` 时只统计全量索引本身（与 `/status` 场景一致）。
pub fn compute_memory_stats(use_memory: bool, memory_dir: &Path) -> MemoryStats {
    let mut stats = MemoryStats {
        enabled: use_memory,
        last_recall_note: "n/a (file-based memory has no recall timestamp)",
        consolidation_note: "not tracked (no consolidation trigger in code yet)",
        ..Default::default()
    };
    if !use_memory {
        return stats;
    }

    stats.index_present = memory::load_index(memory_dir).is_some();

    stats.categories_present = CATEGORIES
        .iter()
        .filter(|cat| {
            let path = memory::category_path(memory_dir, cat);
            std::fs::read_to_string(&path)
                .map(|c| !c.trim().is_empty())
                .unwrap_or(false)
        })
        .count();

    if let Some(block) = memory::compose_index_block(use_memory, memory_dir, None) {
        stats.index_token_estimate = estimate_text_tokens_conservative(&block);
    }

    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_yields_disabled_snapshot_without_touching_fs() {
        let stats = compute_memory_stats(false, Path::new("/nonexistent-memory-dir"));
        assert!(!stats.enabled);
        assert_eq!(stats.one_line(), "disabled");
        assert!(!stats.index_present);
        assert_eq!(stats.categories_present, 0);
        assert_eq!(stats.index_token_estimate, 0);
    }

    #[test]
    fn render_includes_all_dimensions_and_never_panics() {
        let stats = compute_memory_stats(true, Path::new("/nonexistent-memory-dir"));
        let rendered = stats.render();
        assert!(rendered.contains("enabled:"));
        assert!(rendered.contains("categories_present:"));
        assert!(rendered.contains("last_recall:"));
        assert!(rendered.contains("consolidation:"));
        assert!(rendered.contains("/vmemory"));
    }
}
