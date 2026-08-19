//! Dreaming 三阶段巩固流水线（#623 切片）。
//!
//! 把 [`crate::consolidation::ConsolidationScheduler`]（#829 周期调度器）暴露的
//! "单阶段归并"升级为 **Dreaming** 三阶段记忆巩固：
//!
//! 1. **抽取（extract）**：从原始交互/情景记忆中筛出"高信号"片段。
//!    信号强度 = `importance`（已含访问强化与时近衰减，见 `consolidation.rs`）。
//!    低于 [`EXTRACT_IMPORTANCE_THRESHOLD`] 的弱信号片段在这个阶段被丢弃，
//!    避免噪声进入后续阶段。
//! 2. **整合（integrate）**：对抽取出的片段做去重 + 相似归并，得到一组
//!    精简、无冗余的中层记忆。复用 `consolidation.rs` 的 [`dedup`] / [`rollup`]
//!    启发式，保证与现有归并逻辑一致，不引入第二套去重口径。
//! 3. **抽象（abstract）**：从整合结果中提炼出**可复用的高层规则/结论**
//!    （语义记忆）。采用简单主题聚类启发式——按 token 出现频次聚合高频主题，
//!    每个显著主题产出一条 [`MemoryKind::Abstracted`] 规则条目，作为面向未来的
//!    可迁移知识。抽象阶段不依赖外部模型，纯确定性，便于单测。
//!
//! 设计理由：
//! - 三阶段解耦让每一阶段可独立测试与替换（例如未来 `abstract` 可接 LLM 总结）。
//! - [`dream_cycle`] 串联三阶段，返回一条完整的"梦境"产物（抽取数 / 整合数 /
//!   抽象规则），供调度器在回合边界调用。它与既有 [`maybe_consolidate`] 兼容：
//!   调度器到达间隔时，把 `run` 回调指向 `dream_cycle` 即可，无需改动调用链。
//! - 固定建设：本模块纯计算、无 IO、确定性，所有阶段均可用构造的 `MemoryEntry`
//!   在单测中验证，不依赖嵌入/向量库。

use std::collections::{HashMap, HashSet};

use crate::consolidation::{rollup, DEDUP_SIMILARITY_THRESHOLD, MemoryEntry, MemoryKind};

/// 抽取阶段的重要性门槛：低于此值的情景记忆视为弱信号，不进入整合阶段。
///
/// 取 `DEFAULT_IMPORTANCE`(0.5) 作为基线——只有被访问强化或本身高重要性的
/// 条目才越过此线，实现"遗忘 + 聚焦"的 Dreaming 第一道滤网。
pub const EXTRACT_IMPORTANCE_THRESHOLD: f64 = 0.5;

/// 抽象阶段判定"显著主题"所需的最小 token 频次。
///
/// 一个 token 在多条整合记忆中出现次数 ≥ 此值，才被聚为一类并产出抽象规则，
/// 避免基于单条偶发内容生成伪规则。取 2 即"至少在两条记忆中共现"。
pub const ABSTRACT_TOPIC_MIN_FREQUENCY: usize = 2;

/// 阶段一：抽取高信号片段。
///
/// 输入原始记忆（情景 + 其他），返回 `importance >= EXTRACT_IMPORTANCE_THRESHOLD`
/// 的条目。严格筛选、不改写内容。空输入返回空向量。
pub fn extract<I>(entries: I) -> Vec<MemoryEntry>
where
    I: IntoIterator<Item = MemoryEntry>,
{
    entries
        .into_iter()
        .filter(|e| e.importance >= EXTRACT_IMPORTANCE_THRESHOLD)
        .collect()
}

/// 阶段二：整合（相似归并 + 去重）。
///
/// 用连通聚类把相似条目聚成簇：任两条目 [`content_similarity`] ≥
/// [`DEDUP_SIMILARITY_THRESHOLD`] 即同簇（传递闭包）。每簇用 [`rollup`] 合并为
/// 一条 [`MemoryKind::Semantic`] 中层记忆；孤立（无可并对象）条目原样保留。
/// 先归并再对簇间做轻量去重，避免与 [`dedup`] 抢同一门槛导致近重复被滤掉、
/// 归并无对象可合的问题。返回去重归并后的精简中层记忆集合。
pub fn integrate<I>(entries: I) -> Vec<MemoryEntry>
where
    I: IntoIterator<Item = MemoryEntry>,
{
    let all: Vec<MemoryEntry> = entries.into_iter().collect();
    if all.is_empty() {
        return Vec::new();
    }
    // 连通聚类：贪心把相似条目并入已建立的簇。
    let mut clusters: Vec<Vec<MemoryEntry>> = Vec::new();
    for e in all {
        let mut merged_into = false;
        for cluster in clusters.iter_mut() {
            let close = cluster.iter().any(|c| {
                crate::consolidation::content_similarity(&c.content, &e.content)
                    >= DEDUP_SIMILARITY_THRESHOLD
            });
            if close {
                cluster.push(e.clone());
                merged_into = true;
                break;
            }
        }
        if !merged_into {
            clusters.push(vec![e]);
        }
    }
    // 每簇归并；单条簇保留原条目。
    let mut integrated: Vec<MemoryEntry> = Vec::new();
    for cluster in clusters {
        if cluster.len() == 1 {
            integrated.push(cluster.into_iter().next().unwrap());
        } else if let Some(merged) = rollup(cluster.clone(), DEDUP_SIMILARITY_THRESHOLD) {
            integrated.push(merged);
        } else {
            integrated.extend(cluster);
        }
    }
    integrated
}

/// 阶段三：抽象出可复用规则/结论。
///
/// 对整合结果做主题 token 频次聚类：统计每条记忆 token 在集合中出现的文档频次，
/// 频次 ≥ [`ABSTRACT_TOPIC_MIN_FREQUENCY`] 的 token 聚为显著主题；每个主题生成
/// 一条 [`MemoryKind::Abstracted`] 规则条目，内容为高频 token 拼接的人类可读断言。
///
/// 返回抽象规则集合（可能为 0 条——当整合结果太稀疏无法形成共现主题时不臆造）。
/// 抽象规则 `importance` 取被代表主题的整合记忆最大 importance，作为可迁移信号强度。
pub fn abstract_rules<I>(entries: I) -> Vec<MemoryEntry>
where
    I: IntoIterator<Item = MemoryEntry>,
{
    let integrated: Vec<MemoryEntry> = entries.into_iter().collect();
    if integrated.len() < 2 {
        return Vec::new();
    }
    // 每条记忆的 token 集合（去重），用于文档频次计数。
    let token_sets: Vec<HashSet<String>> = integrated
        .iter()
        .map(|e| {
            e.content
                .split(|c: char| !c.is_alphanumeric())
                .map(|t| t.to_lowercase())
                .filter(|t| !t.is_empty())
                .collect::<HashSet<String>>()
        })
        .collect();

    // 统计 token 文档频次。
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    for ts in &token_sets {
        for t in ts {
            *doc_freq.entry(t.clone()).or_insert(0) += 1;
        }
    }
    // 显著主题：文档频次达标且非停用虚词。
    let mut topics: Vec<String> = doc_freq
        .into_iter()
        .filter(|(t, freq)| *freq >= ABSTRACT_TOPIC_MIN_FREQUENCY && !is_stopword(t))
        .map(|(t, _)| t)
        .collect();
    topics.sort();
    topics.dedup();
    if topics.is_empty() {
        return Vec::new();
    }

    // 每个主题产一条抽象规则；importance 取包含该主题的记忆的最大 importance。
    let mut rules = Vec::new();
    for topic in topics {
        let max_imp = integrated
            .iter()
            .zip(&token_sets)
            .filter(|(_, ts)| ts.contains(&topic))
            .map(|(e, _)| e.importance)
            .fold(0.0_f64, f64::max);
        let content = format!("Recurring theme across memories: \"{}\"", topic);
        rules.push(MemoryEntry::with_kind(content, MemoryKind::Abstracted).with_importance(max_imp));
    }
    rules
}

/// 最小英文停用词表，过滤掉无法构成"规则"的虚词（the/and/of...）。
fn is_stopword(t: &str) -> bool {
    const STOP: &[&str] = &[
        "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "is", "are", "be", "with",
        "that", "this", "it", "as", "at", "by", "from", "use", "using",
    ];
    STOP.contains(&t)
}

/// 一次完整 Dreaming 循环：抽取 → 整合 → 抽象。
///
/// 输入原始记忆集合，依次经过 [`extract`]/[`integrate`]/[`abstract_rules`] 三阶段，
/// 返回 [`DreamResult`]，含各阶段产出与抽象的语义规则，便于调用方观测与持久化。
///
/// 与 [`crate::consolidation::ConsolidationScheduler::maybe_consolidate`] 兼容：
/// 调度器到达间隔时把 `run` 回调指向 [`dream_cycle`] 即可无缝接入现有调用链。
pub fn dream_cycle<I>(entries: I) -> DreamResult
where
    I: IntoIterator<Item = MemoryEntry>,
{
    let raw: Vec<MemoryEntry> = entries.into_iter().collect();
    let extracted = extract(raw.iter().cloned());
    let integrated = integrate(extracted.iter().cloned());
    let abstractions = abstract_rules(integrated.iter().cloned());
    DreamResult {
        extracted_count: extracted.len(),
        integrated_count: integrated.len(),
        abstractions: abstractions.clone(),
        raw_count: raw.len(),
    }
}

/// 一次 Dreaming 循环的产出摘要。
#[derive(Debug, Clone, PartialEq)]
pub struct DreamResult {
    /// 原始输入条目数。
    pub raw_count: usize,
    /// 抽取阶段保留的高信号条目数。
    pub extracted_count: usize,
    /// 整合阶段去重归并后的中层条目数。
    pub integrated_count: usize,
    /// 抽象阶段提炼出的可复用规则/结论（语义记忆）。
    pub abstractions: Vec<MemoryEntry>,
}
