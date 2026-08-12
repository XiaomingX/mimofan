//! 记忆巩固（Consolidation）基础数据结构 —— #716 切片 A
//!
//! 本模块当前只引入 **确定性重要性评分所需的数据结构** 与 **访问时更新** 逻辑，
//! 不引入任何检索行为变更（不接 decay / 淘汰 / 去重 / rollup，那些是切片 B–E）。
//!
//! 设计要点：
//! - `MemoryEntry` 是纯数据结构，携带 `importance` / `last_accessed_at` / `access_count`
//!   三个巩固字段，可被切片 B–E 直接复用。
//! - `record_access` 在每次被检索/使用时调用，更新 `access_count` 并刷新
//!   `last_accessed_at`，同时按可配置的强化增益提升 `importance`。
//! - 时间戳使用 `chrono::Utc`，与 `user_profile.rs` 的 JSON 持久化风格保持一致，
//!   但本切片不做持久化（由后续切片或既有向量/文件记忆层负责落盘）。
//!
//! 命名与 `crates/tui/src/turn_memory.rs` 的 `is_duplicate` 启发式区分：去重逻辑
//! 留给切片 C，本切片不实现去重，避免两套去重并存（见 #716 范围说明）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 记忆条目类别。用于切片 B 的 kind 权重评分（本切片仅透传，不参与计算）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryKind {
    /// 情景记忆：具体某次交互/观察
    Episodic,
    /// 语义记忆：从多条情景中归纳出的高层知识
    Semantic,
    /// 程序性记忆：操作/流程偏好
    Procedural,
}

impl Default for MemoryKind {
    fn default() -> Self {
        MemoryKind::Episodic
    }
}

/// 一条可巩固的记忆条目（切片 A 数据结构）。
///
/// 不要求 `content` 非空；纯加固逻辑只依赖三个巩固字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// 全局唯一 id（默认由 uuid 生成；反序列化时透传）。
    pub id: String,
    /// 记忆正文。
    pub content: String,
    /// 记忆类别，供切片 B 的 kind 权重使用。
    pub kind: MemoryKind,
    /// 确定性重要性评分，范围约定为 `[0.0, 1.0]`（切片 B 的 decay 会在此基础上衰减）。
    pub importance: f64,
    /// 最近一次被访问/使用的时间（UTC）。
    pub last_accessed_at: DateTime<Utc>,
    /// 累计被访问次数；每次 `record_access` +1。
    pub access_count: u64,
}

impl MemoryEntry {
    /// 以默认重要性与"当前时刻"创建一条情景记忆条目。
    pub fn new(content: impl Into<String>) -> Self {
        Self::with_kind(content, MemoryKind::Episodic)
    }

    /// 指定类别创建条目。`importance` 取默认基线 [`DEFAULT_IMPORTANCE`]。
    pub fn with_kind(content: impl Into<String>, kind: MemoryKind) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.into(),
            kind,
            importance: DEFAULT_IMPORTANCE,
            last_accessed_at: Utc::now(),
            access_count: 0,
        }
    }

    /// 以显式 id 与创建时间重建（用于持久化层反序列化后复用）。
    pub fn with_id(
        id: impl Into<String>,
        content: impl Into<String>,
        kind: MemoryKind,
        importance: f64,
        last_accessed_at: DateTime<Utc>,
        access_count: u64,
    ) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            kind,
            importance,
            last_accessed_at,
            access_count,
        }
    }

    /// 记录一次访问：自增 `access_count`、刷新 `last_accessed_at`、
    /// 并按 [`ACCESS_REINFORCE_GAIN`] 提升 `importance`（封顶 1.0）。
    ///
    /// 这是切片 A 唯一的"行为"：`importance` 在访问时单调强化，
    /// 方便切片 B 的时近衰减在"强化 vs 衰减"之间取得平衡（M7 访问强化）。
    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_accessed_at = Utc::now();
        self.importance = (self.importance + ACCESS_REINFORCE_GAIN).min(IMPORTANCE_MAX);
    }
}

/// 新建条目的重要性基线。
pub const DEFAULT_IMPORTANCE: f64 = 0.5;
/// 重要性评分上限。
pub const IMPORTANCE_MAX: f64 = 1.0;
/// 每次访问对 `importance` 的强化增益（访问强化 M7）。
pub const ACCESS_REINFORCE_GAIN: f64 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_entry_has_baseline_importance_and_zero_access() {
        let e = MemoryEntry::new("remember the deadline");
        assert_eq!(e.importance, DEFAULT_IMPORTANCE);
        assert_eq!(e.access_count, 0);
        assert_eq!(e.kind, MemoryKind::Episodic);
        assert!(!e.id.is_empty());
        assert_eq!(e.content, "remember the deadline");
    }

    #[test]
    fn with_kind_sets_category() {
        let e = MemoryEntry::with_kind("how to deploy", MemoryKind::Procedural);
        assert_eq!(e.kind, MemoryKind::Procedural);
        assert_eq!(e.importance, DEFAULT_IMPORTANCE);
    }

    #[test]
    fn record_access_increments_count_and_refreshes_timestamp() {
        let before = Utc::now();
        let mut e = MemoryEntry::with_kind("x", MemoryKind::Semantic);
        // 强制旧时间戳，确证 record_access 会刷新
        e.last_accessed_at = before - chrono::Duration::hours(1);
        e.record_access();
        assert_eq!(e.access_count, 1);
        assert!(e.last_accessed_at >= before);
        assert!((e.importance - (DEFAULT_IMPORTANCE + ACCESS_REINFORCE_GAIN)).abs() < 1e-9);
    }

    #[test]
    fn record_access_reinforces_but_caps_at_max() {
        let mut e = MemoryEntry::with_id(
            "id1",
            "x",
            MemoryKind::Episodic,
            IMPORTANCE_MAX - 0.01,
            Utc::now(),
            0,
        );
        e.record_access();
        assert_eq!(e.importance, IMPORTANCE_MAX);
        assert_eq!(e.access_count, 1);
    }

    #[test]
    fn with_id_preserves_identity_and_fields() {
        let ts = Utc::now();
        let e = MemoryEntry::with_id("fixed-id", "payload", MemoryKind::Semantic, 0.3, ts, 7);
        assert_eq!(e.id, "fixed-id");
        assert_eq!(e.content, "payload");
        assert_eq!(e.importance, 0.3);
        assert_eq!(e.access_count, 7);
        assert_eq!(e.last_accessed_at, ts);
    }

    #[test]
    fn entry_round_trips_through_json() {
        let e = MemoryEntry::with_id("j-id", "json content", MemoryKind::Procedural, 0.8, Utc::now(), 3);
        let json = serde_json::to_string(&e).expect("serialize");
        let back: MemoryEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);
    }
}
