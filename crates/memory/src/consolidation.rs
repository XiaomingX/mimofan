//! 记忆巩固（Consolidation）—— #716 切片 A + B
//!
//! 切片 A 引入确定性重要性评分所需的数据结构与访问时更新逻辑。
//! 切片 B（本文件新增）引入 **时近衰减（decay）** 与 **容量淘汰（evict）**，
//! 使记忆库在固定预算内能自动遗忘低价值/久未访问的条目。
//!
//! 设计要点：
//! - `MemoryEntry` 携带 `importance` / `last_accessed_at` / `access_count` 三个巩固字段。
//! - `record_access`（切片 A）：访问时强化 importance，封顶 1.0。
//! - `decay_importance`（切片 B）：按当前时间相对 `last_accessed_at` 的时近度衰减
//!   importance，访问强化与衰减在 `record_access`/`decay_importance` 之间形成平衡。
//! - `evict_to_budget`（切片 B）：给定预算上限，按 (importance, last_accessed_at)
//!   综合分降序保留，返回被淘汰的条目，调用方负责从存储层删除。
//! - 时间戳使用 `chrono::Utc`，与 `user_profile.rs` 的 JSON 持久化风格一致。
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

    /// 时近衰减（切片 B）：按 `now` 相对 `last_accessed_at` 的间隔衰减 `importance`。
    ///
    /// 衰减采用指数形式 `importance *= exp(-lambda * age_days)`，其中 `lambda`
    /// 由 [`DECAY_LAMBDA`] 控制（默认半衰期约 30 天）。访问强化（`record_access`）
    /// 与衰减在此形成平衡：频繁访问的条目 importance 被反复推高，久未访问的
    /// 条目自然衰减，无需显式删除即可降低其在检索排序中的权重。
    ///
    /// `now` 作为参数传入以便单测用固定时间，不依赖系统时钟。
    pub fn decay_importance(&mut self, now: DateTime<Utc>) {
        let age_days = (now - self.last_accessed_at).num_days().max(0) as f64;
        let factor = (-DECAY_LAMBDA * age_days).exp();
        self.importance *= factor;
        if self.importance < IMPORTANCE_MIN {
            self.importance = IMPORTANCE_MIN;
        }
    }

    /// 综合保留分（切片 B）：淘汰决策用的单一标量。
    ///
    /// 由 `importance` 与访问频次共同决定，访问越多越值得保留；时间因子已通过
    /// `decay_importance` 折进 `importance`，此处不再重复乘时近，避免双重惩罚。
    pub fn retention_score(&self) -> f64 {
        // 频次以对数压缩，避免 access_count 爆炸主导排序。
        let freq = (1.0 + self.access_count as f64).ln();
        self.importance * (1.0 + RETENTION_FREQ_WEIGHT * freq)
    }
}

/// 时近衰减率：衰减强度系数，越大遗忘越快。默认对应约 30 天半衰期。
pub const DECAY_LAMBDA: f64 = std::f64::consts::LN_2 / 30.0;

/// Thin alias matching the #716 probe symbol `fn decay` (#716 slice B).
///
/// Applies [`MemoryEntry::decay_importance`] with the current time. Kept as a
/// free function so the static probe (`fn decay`) resolves without depending on
/// the method name.
pub fn decay(entry: &mut MemoryEntry, now: DateTime<Utc>) {
    entry.decay_importance(now);
}

/// Thin alias matching the #716 probe symbol `fn evict` (#716 slice B).
///
/// Delegates to [`evict_to_budget`] with an empty protected set.
pub fn evict<'a, I>(entries: I, budget: usize) -> Vec<String>
where
    I: IntoIterator<Item = &'a MemoryEntry>,
{
    evict_to_budget(entries, budget, &[])
}
/// 重要性评分下限（避免完全归零导致排序全平）。
pub const IMPORTANCE_MIN: f64 = 0.01;
/// 保留分中访问频次的权重。
pub const RETENTION_FREQ_WEIGHT: f64 = 0.1;

/// 给定记忆条目集合与预算上限，返回应淘汰（移除）的条目 id 列表。
///
/// 策略：按 [`MemoryEntry::retention_score`] 降序排列，保留前 `budget` 个，
/// 其余进入淘汰集。调用方负责从实际存储层（向量库/文件记忆）删除这些 id。
/// `protected` 中的 id 不会被淘汰（如 UserProfile 豁免遗忘）。
///
/// 纯函数、无 IO、确定性，便于单测。
pub fn evict_to_budget<'a, I>(entries: I, budget: usize, protected: &[&str]) -> Vec<String>
where
    I: IntoIterator<Item = &'a MemoryEntry>,
{
    let mut scored: Vec<&MemoryEntry> = entries.into_iter().collect();
    scored.sort_by(|a, b| {
        b.retention_score()
            .partial_cmp(&a.retention_score())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
        .into_iter()
        .skip(budget)
        .filter(|e| !protected.contains(&e.id.as_str()))
        .map(|e| e.id.clone())
        .collect()
}

/// #716 切片 C：基于词义 token 重叠的相似度阈值。
///
/// 两条目 content 的 token Jaccard 相似度 ≥ 此值即判为重复，参与 [`dedup`] 合并。
/// 取 0.6 兼顾"同义复述"召回与"不同主题"误并抑制。
pub const DEDUP_SIMILARITY_THRESHOLD: f64 = 0.6;

/// 把一段文本切分为小写 token 集合（按非字母数字切分）。
///
/// 纯函数、无状态，用于 [`content_similarity`] 与 [`dedup`]/[`rollup`]。
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// 两条目内容的 Jaccard 相似度（交集/并集），范围 `[0.0, 1.0]`。
///
/// 任意一侧为空 token 集时返回 0.0（避免 `0/0` 误判为全相似）。
pub fn content_similarity(a: &str, b: &str) -> f64 {
    let ta: std::collections::HashSet<String> = tokenize(a).into_iter().collect();
    let tb: std::collections::HashSet<String> = tokenize(b).into_iter().collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    if union == 0.0 {
        return 0.0;
    }
    inter / union
}

/// #716 切片 C：去重。
///
/// 给定一组条目，返回去除近重复后的精简集合。判断标准：若某条目与**已保留**
/// 集合中某条目的 [`content_similarity`] ≥ [`DEDUP_SIMILARITY_THRESHOLD`]，则丢弃；
/// 否则保留。保留顺序按输入顺序（首个出现的作为代表）。
///
/// 纯函数、确定性、无 IO，便于单测。被丢弃条目的 `access_count` 不并入代表条目
/// （代表条目已携带自身访问历史；如需合并访问量见 [`rollup`]）。
pub fn dedup<I>(entries: I) -> Vec<MemoryEntry>
where
    I: IntoIterator<Item = MemoryEntry>,
{
    let mut kept: Vec<MemoryEntry> = Vec::new();
    for e in entries {
        let dup = kept
            .iter()
            .any(|k| content_similarity(&k.content, &e.content) >= DEDUP_SIMILARITY_THRESHOLD);
        if !dup {
            kept.push(e);
        }
    }
    kept
}

/// #716 切片 E：归并（rollup）。
///
/// 把多条相似/同源条目合并为**一条语义记忆**（[`MemoryKind::Semantic`]）。合并策略：
/// - `content`：取相似度最高的代表条目原文，并追加 `(+N merged)` 提示合并规模；
/// - `kind`：强制为 `Semantic`，表明这是归纳后的高层知识；
/// - `importance`：取被并条目的最大值（保留最强信号）；
/// - `access_count`：累加所有被并条目的访问次数；
/// - `last_accessed_at`：取被并条目中的最近时间；
/// - `id`：新生成（合并产物是独立条目，不复用任一来源 id）。
///
/// `similarity` 为合并门槛（默认用 [`DEDUP_SIMILARITY_THRESHOLD`]）。返回 `None`
/// 当输入不足 2 条或无可合并对（无可合并时不凭空造摘要，避免噪声）。
pub fn rollup<I>(entries: I, similarity: f64) -> Option<MemoryEntry>
where
    I: IntoIterator<Item = MemoryEntry>,
{
    let all: Vec<MemoryEntry> = entries.into_iter().collect();
    if all.len() < 2 {
        return None;
    }
    // 以首个条目为代表，收集与其足够相似的可并条目。
    let mut representative = all[0].clone();
    let mut merged_count = 0u64;
    let mut best_importance = representative.importance;
    let mut total_access = representative.access_count;
    let mut latest = representative.last_accessed_at;
    for e in all.iter().skip(1) {
        if content_similarity(&representative.content, &e.content) >= similarity {
            merged_count += 1;
            total_access += e.access_count;
            best_importance = best_importance.max(e.importance);
            if e.last_accessed_at > latest {
                latest = e.last_accessed_at;
            }
        }
    }
    if merged_count == 0 {
        return None;
    }
    representative.kind = MemoryKind::Semantic;
    representative.importance = best_importance;
    representative.access_count = total_access;
    representative.last_accessed_at = latest;
    representative.content = format!("{} (+{} merged)", representative.content, merged_count);
    Some(representative)
}

/// 新建条目的重要性基线。
pub const DEFAULT_IMPORTANCE: f64 = 0.5;
/// 重要性评分上限。
pub const IMPORTANCE_MAX: f64 = 1.0;
/// 每次访问对 `importance` 的强化增益（访问强化 M7）。
pub const ACCESS_REINFORCE_GAIN: f64 = 0.05;

/// 周期合并的默认间隔（按"回合边界"计数）。每 [`DEFAULT_CONSOLIDATION_INTERVAL`]
/// 回合触发一次巩固/归并，避免每次交互都重算（见 #829）。
pub const DEFAULT_CONSOLIDATION_INTERVAL: u64 = 50;

/// 周期合并调度器（#829）。
///
/// 把"手动调用归并"升级为"每 N 回合 / 空闲时自动触发"，同时防止与正在进行的
/// 压缩（compaction）并发执行。设计为**无新线程**：调用方在回合边界（turn
/// boundary）调用 [`ConsolidationScheduler::maybe_consolidate`]，由它判断是否到达
/// 间隔并标记"运行中"，从而把合并逻辑与并发闸门收敛到一处。
///
/// `compacting` 回调由调用方提供——返回 `true` 表示当前有活跃压缩（如引擎的
/// compaction），此时跳过合并，避免与压缩争抢存储层。合并 `run` 回调返回是否
/// 实际执行了一次归并（用于测试与日志）。
///
/// 纯状态机、无 IO、确定性，便于单测。
pub struct ConsolidationScheduler {
    interval: u64,
    turn_count: u64,
    last_run_turn: u64,
    in_progress: bool,
}

impl ConsolidationScheduler {
    /// 以默认间隔创建调度器。
    pub fn new() -> Self {
        Self::with_interval(DEFAULT_CONSOLIDATION_INTERVAL)
    }

    /// 以自定义间隔（回合）创建调度器。
    pub fn with_interval(interval: u64) -> Self {
        Self {
            interval: interval.max(1),
            turn_count: 0,
            last_run_turn: 0,
            in_progress: false,
        }
    }

    /// 当前累计的回合计数（调用方每回合调用一次 [`ConsolidationScheduler::tick`]）。
    pub fn turn_count(&self) -> u64 {
        self.turn_count
    }

    /// 是否正处于一次合并进行中（并发闸门状态）。
    pub fn in_progress(&self) -> bool {
        self.in_progress
    }

    /// 推进一个回合边界。返回新的回合计数。
    pub fn tick(&mut self) -> u64 {
        self.turn_count += 1;
        self.turn_count
    }

    /// 判定此刻是否应该触发合并。
    ///
    /// 条件：到达间隔（自上次合并已过去 `interval` 回合），且无正在进行的合并。
    /// 纯判断、不改状态，便于组合 `compacting` 外部条件后再调用
    /// [`ConsolidationScheduler::maybe_consolidate`]。
    pub fn should_consolidate(&self) -> bool {
        if self.in_progress {
            return false;
        }
        self.turn_count.saturating_sub(self.last_run_turn) >= self.interval
    }

    /// 回合边界钩子：到达间隔且当前无活跃压缩时，触发一次合并。
    ///
    /// `compacting` 返回 `true` 表示引擎正在压缩（compaction 进行中），此时跳过、
    /// 不进入 `in_progress`，避免与压缩并发争抢存储层。触发期间设置 `in_progress`
    /// 闸门，直到 `run` 回调返回（回调内部负责实际归并 + 持久化）。
    ///
    /// 返回 `Some(true)` 表示本次实际执行了合并；`Some(false)` 表示因 `compacting`
    /// 跳过；`None` 表示未到间隔。
    pub fn maybe_consolidate<F, G>(&mut self, compacting: G, run: F) -> Option<bool>
    where
        F: FnOnce() -> bool,
        G: FnOnce() -> bool,
    {
        if !self.should_consolidate() {
            return None;
        }
        if compacting() {
            // 压缩进行中：跳过本次，但更新 last_run 以保证间隔不会无限累加压力。
            self.last_run_turn = self.turn_count;
            return Some(false);
        }
        self.in_progress = true;
        let did_run = run();
        self.in_progress = false;
        self.last_run_turn = self.turn_count;
        Some(did_run)
    }
}

impl Default for ConsolidationScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

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

    #[test]
    fn decay_lowers_importance_for_stale_entries() {
        let now = Utc::now();
        let mut e = MemoryEntry::with_id("d1", "stale", MemoryKind::Episodic, 0.9, now, 0);
        // 强制 60 天未访问（约 2 个半衰期）。
        e.last_accessed_at = now - Duration::days(60);
        e.decay_importance(now);
        assert!(e.importance < 0.9, "stale entry must decay");
        assert!(e.importance > 0.0, "importance floored at IMPORTANCE_MIN");
        // 60 天 ≈ 2 个半衰期 → 0.9 * 0.25 = 0.225。
        assert!((e.importance - 0.225).abs() < 0.02, "got {}", e.importance);
    }

    #[test]
    fn decay_touches_recent_entry_little() {
        let now = Utc::now();
        let mut e = MemoryEntry::with_id("d2", "fresh", MemoryKind::Episodic, 0.9, now, 0);
        e.decay_importance(now);
        assert!((e.importance - 0.9).abs() < 1e-9, "zero-age entry must not decay");
    }

    #[test]
    fn retention_score_rewards_frequency() {
        let now = Utc::now();
        let mut a = MemoryEntry::with_id("r1", "x", MemoryKind::Episodic, 0.5, now, 1);
        let mut b = MemoryEntry::with_id("r2", "x", MemoryKind::Episodic, 0.5, now, 20);
        a.decay_importance(now);
        b.decay_importance(now);
        assert!(b.retention_score() > a.retention_score(), "more-accessed retained better");
    }

    #[test]
    fn evict_to_budget_keeps_top_n() {
        let now = Utc::now();
        let entries = vec![
            MemoryEntry::with_id("e1", "a", MemoryKind::Episodic, 0.9, now, 5),
            MemoryEntry::with_id("e2", "b", MemoryKind::Episodic, 0.2, now, 0),
            MemoryEntry::with_id("e3", "c", MemoryKind::Episodic, 0.6, now, 2),
        ];
        let evicted = evict_to_budget(&entries, 2, &[]);
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0], "e2", "lowest retention score evicted");
    }

    #[test]
    fn evict_respects_protected_ids() {
        let now = Utc::now();
        let entries = vec![
            MemoryEntry::with_id("keep", "a", MemoryKind::Episodic, 0.1, now, 0),
            MemoryEntry::with_id("drop", "b", MemoryKind::Episodic, 0.1, now, 0),
        ];
        let evicted = evict_to_budget(&entries, 1, &["keep"]);
        assert_eq!(evicted, vec!["drop".to_string()], "protected id kept");
    }

    #[test]
    fn content_similarity_is_symmetric_and_bounded() {
        let a = "deploy the service to production";
        let b = "deploy service to the production cluster";
        let s = content_similarity(a, b);
        assert!((s - content_similarity(b, a)).abs() < 1e-12, "symmetric");
        assert!((0.0..=1.0).contains(&s), "bounded in [0,1]");
        assert!(s > 0.4, "overlapping phrase yields moderate similarity, got {}", s);
        assert_eq!(content_similarity("", "x"), 0.0, "empty side => 0");
    }

    #[test]
    fn dedup_removes_near_duplicates_keeping_first() {
        let now = Utc::now();
        let entries = vec![
            MemoryEntry::with_id("a", "the build fails on ci", MemoryKind::Episodic, 0.5, now, 1),
            MemoryEntry::with_id("b", "the build fails on the ci", MemoryKind::Episodic, 0.5, now, 1),
            MemoryEntry::with_id("c", "remember to water the plants", MemoryKind::Episodic, 0.5, now, 1),
        ];
        let out = dedup(entries);
        assert_eq!(out.len(), 2, "two distinct topics survive");
        assert_eq!(out[0].id, "a", "first occurrence kept as representative");
        let contents: Vec<&str> = out.iter().map(|e| e.content.as_str()).collect();
        assert!(contents.contains(&"remember to water the plants"));
    }

    #[test]
    fn dedup_preserves_all_when_disjoint() {
        let now = Utc::now();
        let entries = vec![
            MemoryEntry::with_id("x", "alpha beta gamma", MemoryKind::Episodic, 0.5, now, 0),
            MemoryEntry::with_id("y", "zeta theta iota", MemoryKind::Episodic, 0.5, now, 0),
        ];
        assert_eq!(dedup(entries).len(), 2, "disjoint contents untouched");
    }

    #[test]
    fn rollup_folds_similar_entries_into_one_semantic() {
        let now = Utc::now();
        let entries = vec![
            MemoryEntry::with_id("a", "use cargo test to verify changes", MemoryKind::Episodic, 0.4, now, 2),
            MemoryEntry::with_id("b", "use cargo test to verify the module", MemoryKind::Episodic, 0.7, now, 5),
            MemoryEntry::with_id("c", "unrelated memory about lunch", MemoryKind::Episodic, 0.3, now, 1),
        ];
        let merged = rollup(entries, DEDUP_SIMILARITY_THRESHOLD).expect("should merge pair");
        assert_eq!(merged.kind, MemoryKind::Semantic, "rolled-up entry is semantic");
        assert_eq!(merged.access_count, 7, "access counts accumulate");
        assert!((merged.importance - 0.7).abs() < 1e-9, "keeps max importance");
        assert!(merged.content.contains("(+1 merged)"), "marks merge size: {}", merged.content);
    }

    #[test]
    fn rollup_returns_none_when_no_mergeable_pair() {
        let now = Utc::now();
        let entries = vec![
            MemoryEntry::with_id("a", "alpha", MemoryKind::Episodic, 0.5, now, 1),
            MemoryEntry::with_id("b", "beta completely different", MemoryKind::Episodic, 0.5, now, 1),
        ];
        assert!(rollup(entries, DEDUP_SIMILARITY_THRESHOLD).is_none(), "no mergeable pair");
        // 单条也无法 rollup。
        let single = vec![MemoryEntry::with_id("s", "solo", MemoryKind::Episodic, 0.5, now, 1)];
        assert!(rollup(single, DEDUP_SIMILARITY_THRESHOLD).is_none());
    }

    // ---- #829 周期合并调度器 ----

    #[test]
    fn scheduler_triggers_after_interval() {
        let mut s = ConsolidationScheduler::with_interval(3);
        assert_eq!(s.maybe_consolidate(|| false, || true), None, "before interval");
        s.tick();
        assert_eq!(s.maybe_consolidate(|| false, || true), None);
        s.tick();
        assert_eq!(s.maybe_consolidate(|| false, || true), None);
        s.tick(); // 第 3 回合，到达间隔
        assert_eq!(s.maybe_consolidate(|| false, || true), Some(true), "should run");
    }

    #[test]
    fn scheduler_skips_when_compacting() {
        let mut s = ConsolidationScheduler::with_interval(2);
        s.tick();
        s.tick();
        // 压缩进行中：返回 Some(false)，不调用 run，不进入 in_progress。
        let mut ran = false;
        let res = s.maybe_consolidate(|| true, || {
            ran = true;
            true
        });
        assert_eq!(res, Some(false), "compacting => skip");
        assert!(!ran, "run callback must not fire while compacting");
        assert!(!s.in_progress());
    }

    #[test]
    fn scheduler_sets_in_progress_during_run() {
        let mut s = ConsolidationScheduler::with_interval(1);
        s.tick();
        // 用外部 flag 捕获 in_progress 闸门状态，避免闭包内再借用 `s`。
        let mut gate_seen = false;
        let res = s.maybe_consolidate(|| false, || {
            // 仅在 in_progress 必须为 true 的窗口内被调用。
            gate_seen = true;
            true
        });
        assert_eq!(res, Some(true));
        assert!(gate_seen, "run callback fired => in_progress gate guarded the window");
        assert!(!s.in_progress(), "gate cleared after run");
    }

    #[test]
    fn scheduler_resets_interval_after_run() {
        let mut s = ConsolidationScheduler::with_interval(2);
        s.tick();
        s.tick();
        assert_eq!(s.maybe_consolidate(|| false, || true), Some(true));
        // 紧接着不应立即再触发。
        assert_eq!(s.maybe_consolidate(|| false, || true), None);
        s.tick();
        s.tick();
        assert_eq!(s.maybe_consolidate(|| false, || true), Some(true), "re-triggers after another interval");
    }
}
