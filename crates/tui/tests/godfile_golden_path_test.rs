//! #871 — godfile 拆分前置黄金路径快照测试（回归护栏）
//!
//! `engine.rs`(3765 行) 与 `turn_loop.rs`(3640 行) 是两个巨型单体文件。本次
//! 接线把 5 个已落库模块真正接入主流程，任何未来的拆分都不能破坏这些接线。
//! 本文件在 `tests/` 下补最小成本、零网络、可重复运行的回归断言，作为拆分护栏。
//!
//! **为什么不直接实例化 `Engine`**：`Engine` 构造依赖 `ApiClient`、LSP、
//! `SubAgentManager`、向量记忆后端等大量运行时组件，在集成测试中以最小成本
//! 完整实例化成本极高且脆弱（见 CODEBUDDY 全局约束）。因此本测试退而求其次，
//! 直接对本次接线所依赖的**轻量、纯函数 / 状态机原语**做黄金路径断言——
//! 它们正是 `engine.rs`/`turn_loop.rs` 在回合边界真正调用的同一批 API：
//!
//! 1. `ConsolidationScheduler` 回合边界触发（#871 子项 1 的调用链入口）；
//! 2. `dream_cycle` 三阶段抽象产出（#871 子项 1 的核心算法）；
//! 3. `DecisionLog` 记录 + `summary()` 注入标记（#871 子项 2 的数据路径）。
//!
//! 这三条契约若被拆分改坏，本测试会立即红，从而保护 P0 接线不被回归。

use mimofan::compaction::decision_log::{DecisionEvent, DecisionLog, Kind};
use mimofan_memory::consolidation::{ConsolidationScheduler, MemoryEntry, MemoryKind};
use mimofan_memory::consolidation_stages::dream_cycle;
use std::time::Instant;

/// 黄金路径 1：回合边界调度器确实在到达间隔时触发合并回调。
///
/// 这是 `engine.rs::run_consolidation_tick` 在每回合边界所走的同一入口；拆分
/// 后该入口语义必须保持不变——`Some(true)` 表示合并实际发生。
#[test]
fn golden_consolidation_scheduler_fires_at_interval() {
    let mut scheduler = ConsolidationScheduler::with_interval(3);
    assert_eq!(
        scheduler.maybe_consolidate(|| false, || true),
        None,
        "未到间隔不应触发"
    );
    scheduler.tick();
    assert_eq!(scheduler.maybe_consolidate(|| false, || true), None);
    scheduler.tick();
    assert_eq!(scheduler.maybe_consolidate(|| false, || true), None);
    scheduler.tick(); // 第 3 回合到达间隔
    assert_eq!(
        scheduler.maybe_consolidate(|| false, || true),
        Some(true),
        "到达间隔必须触发合并"
    );
}

/// 黄金路径 1b：压缩进行中时调度器跳过，避免与 compaction 争抢存储层。
///
/// `engine.rs::run_consolidation_tick` 用 `compacting` 回调表达该闸门，此契约
/// 是拆分的硬约束。
#[test]
fn golden_consolidation_skips_while_compacting() {
    let mut scheduler = ConsolidationScheduler::with_interval(2);
    scheduler.tick();
    scheduler.tick();
    let mut ran = false;
    let res = scheduler.maybe_consolidate(
        || true,
        || {
            ran = true;
            true
        },
    );
    assert_eq!(res, Some(false), "压缩中必须跳过");
    assert!(!ran, "压缩中不得调用合并回调");
}

/// 黄金路径 1c：Dreaming 三阶段在召回数据上确实产出抽象规则。
///
/// 这是 `engine.rs::run_dreaming_cycle` 在合并回调内调用的同一 `dream_cycle`；
/// 拆分会话记忆时该输出（可复用高层规则）不得被静默丢弃。
#[test]
fn golden_dream_cycle_produces_abstractions() {
    let raw = vec![
        MemoryEntry::with_kind("cargo test verifies the module", MemoryKind::Episodic),
        MemoryEntry::with_kind("cargo test verifies the build", MemoryKind::Episodic),
        MemoryEntry::with_kind("cargo test catches regressions", MemoryKind::Episodic),
        MemoryEntry::with_kind("weak noise to forget entirely", MemoryKind::Episodic),
    ];
    let res = dream_cycle(raw);
    assert!(res.extracted_count >= 1, "高信号片段被抽取");
    assert!(!res.abstractions.is_empty(), "三阶段必须产出抽象规则");
    assert!(
        res.abstractions
            .iter()
            .all(|a| a.kind == MemoryKind::Abstracted),
        "抽象产物为 Abstracted 类别"
    );
}

/// 黄金路径 2：决策事件被记录且 `summary()` 产出注入标记。
///
/// `engine.rs::record_decision` + `with_decision_trail` 把该 summary 注入压缩
/// 系统提示；本断言锁定其输出契约——拆分后注入内容格式变坏（缺标记）会立即红。
#[test]
fn golden_decision_log_summary_injection_contract() {
    let mut log = DecisionLog::new();
    // 模拟 turn_loop 在工具分派点记录一条 ToolChosen 决策。
    log.record(DecisionEvent::new(
        1,
        Kind::ToolChosen,
        "chose tool `edit_file`",
    ));
    log.record(DecisionEvent::new(
        2,
        Kind::BranchTaken,
        "took the refactor branch",
    ));

    let summary = log.summary().expect("决策轨迹应可注入");
    assert!(
        summary.contains("Decision Trail"),
        "注入块必须含 Decision Trail 标记"
    );
    assert!(
        summary.contains("edit_file"),
        "注入块必须含已记录的工具选择"
    );
    assert!(summary.contains("branch_taken"), "注入块必须含分支决策类型");
}

/// 黄金路径 2b：空决策日志不污染压缩提示（summary 返回 None）。
#[test]
fn golden_decision_log_empty_is_noop() {
    let log = DecisionLog::new();
    assert!(log.summary().is_none(), "空日志不应注入任何内容");
}

/// 黄金路径 3（性能护栏）：`dream_cycle` 在中等规模输入下保持亚秒级，确保
/// 接入回合边界后不会显著拖慢主流程。正确性（抽象产出）已由
/// `golden_dream_cycle_produces_abstractions` 单独守护；此处只守卫性能契约——
/// 600 条输入下三阶段必须 < 2s 完成且不 panic。
#[test]
fn golden_dream_cycle_stays_fast() {
    let raw: Vec<MemoryEntry> = (0..600)
        .map(|i| {
            MemoryEntry::with_kind(
                format!("cargo test verifies the build for module {i}"),
                MemoryKind::Episodic,
            )
        })
        .collect();
    let start = Instant::now();
    let res = dream_cycle(raw);
    let elapsed = start.elapsed();
    // 规模输入必须可处理（抽取/整合阶段在 600 条下仍产出结构化结果）。
    assert!(
        res.raw_count == 600,
        "必须处理全部 600 条输入，实际 {}",
        res.raw_count
    );
    assert!(
        elapsed.as_millis() < 2000,
        "dream_cycle 在 600 条输入下应 < 2s，实际 {elapsed:?}"
    );
}
