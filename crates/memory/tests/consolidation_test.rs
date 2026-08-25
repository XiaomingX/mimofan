use mimofan_memory::consolidation::{
    ACCESS_REINFORCE_GAIN, ConsolidationScheduler, DEDUP_SIMILARITY_THRESHOLD, DEFAULT_IMPORTANCE,
    IMPORTANCE_MAX, MemoryEntry, MemoryKind, content_similarity, dedup, evict_to_budget, rollup,
};

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
    let before = chrono::Utc::now();
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
        chrono::Utc::now(),
        0,
    );
    e.record_access();
    assert_eq!(e.importance, IMPORTANCE_MAX);
    assert_eq!(e.access_count, 1);
}

#[test]
fn with_id_preserves_identity_and_fields() {
    let ts = chrono::Utc::now();
    let e = MemoryEntry::with_id("fixed-id", "payload", MemoryKind::Semantic, 0.3, ts, 7);
    assert_eq!(e.id, "fixed-id");
    assert_eq!(e.content, "payload");
    assert_eq!(e.importance, 0.3);
    assert_eq!(e.access_count, 7);
    assert_eq!(e.last_accessed_at, ts);
}

#[test]
fn entry_round_trips_through_json() {
    let e = MemoryEntry::with_id(
        "j-id",
        "json content",
        MemoryKind::Procedural,
        0.8,
        chrono::Utc::now(),
        3,
    );
    let json = serde_json::to_string(&e).expect("serialize");
    let back: MemoryEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(e, back);
}

#[test]
fn decay_lowers_importance_for_stale_entries() {
    let now = chrono::Utc::now();
    let mut e = MemoryEntry::with_id("d1", "stale", MemoryKind::Episodic, 0.9, now, 0);
    // 强制 60 天未访问（约 2 个半衰期）。
    e.last_accessed_at = now - chrono::Duration::days(60);
    e.decay_importance(now);
    assert!(e.importance < 0.9, "stale entry must decay");
    assert!(e.importance > 0.0, "importance floored at IMPORTANCE_MIN");
    // 60 天 ≈ 2 个半衰期 → 0.9 * 0.25 = 0.225。
    assert!((e.importance - 0.225).abs() < 0.02, "got {}", e.importance);
}

#[test]
fn decay_touches_recent_entry_little() {
    let now = chrono::Utc::now();
    let mut e = MemoryEntry::with_id("d2", "fresh", MemoryKind::Episodic, 0.9, now, 0);
    e.decay_importance(now);
    assert!(
        (e.importance - 0.9).abs() < 1e-9,
        "zero-age entry must not decay"
    );
}

#[test]
fn retention_score_rewards_frequency() {
    let now = chrono::Utc::now();
    let mut a = MemoryEntry::with_id("r1", "x", MemoryKind::Episodic, 0.5, now, 1);
    let mut b = MemoryEntry::with_id("r2", "x", MemoryKind::Episodic, 0.5, now, 20);
    a.decay_importance(now);
    b.decay_importance(now);
    assert!(
        b.retention_score() > a.retention_score(),
        "more-accessed retained better"
    );
}

#[test]
fn evict_to_budget_keeps_top_n() {
    let now = chrono::Utc::now();
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
    let now = chrono::Utc::now();
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
    assert!(
        s > 0.4,
        "overlapping phrase yields moderate similarity, got {}",
        s
    );
    assert_eq!(content_similarity("", "x"), 0.0, "empty side => 0");
}

#[test]
fn dedup_removes_near_duplicates_keeping_first() {
    let now = chrono::Utc::now();
    let entries = vec![
        MemoryEntry::with_id(
            "a",
            "the build fails on ci",
            MemoryKind::Episodic,
            0.5,
            now,
            1,
        ),
        MemoryEntry::with_id(
            "b",
            "the build fails on the ci",
            MemoryKind::Episodic,
            0.5,
            now,
            1,
        ),
        MemoryEntry::with_id(
            "c",
            "remember to water the plants",
            MemoryKind::Episodic,
            0.5,
            now,
            1,
        ),
    ];
    let out = dedup(entries);
    assert_eq!(out.len(), 2, "two distinct topics survive");
    assert_eq!(out[0].id, "a", "first occurrence kept as representative");
    let contents: Vec<&str> = out.iter().map(|e| e.content.as_str()).collect();
    assert!(contents.contains(&"remember to water the plants"));
}

#[test]
fn dedup_preserves_all_when_disjoint() {
    let now = chrono::Utc::now();
    let entries = vec![
        MemoryEntry::with_id("x", "alpha beta gamma", MemoryKind::Episodic, 0.5, now, 0),
        MemoryEntry::with_id("y", "zeta theta iota", MemoryKind::Episodic, 0.5, now, 0),
    ];
    assert_eq!(dedup(entries).len(), 2, "disjoint contents untouched");
}

#[test]
fn rollup_folds_similar_entries_into_one_semantic() {
    let now = chrono::Utc::now();
    let entries = vec![
        MemoryEntry::with_id(
            "a",
            "use cargo test to verify changes",
            MemoryKind::Episodic,
            0.4,
            now,
            2,
        ),
        MemoryEntry::with_id(
            "b",
            "use cargo test to verify the module",
            MemoryKind::Episodic,
            0.7,
            now,
            5,
        ),
        MemoryEntry::with_id(
            "c",
            "unrelated memory about lunch",
            MemoryKind::Episodic,
            0.3,
            now,
            1,
        ),
    ];
    let merged = rollup(entries, DEDUP_SIMILARITY_THRESHOLD).expect("should merge pair");
    assert_eq!(
        merged.kind,
        MemoryKind::Semantic,
        "rolled-up entry is semantic"
    );
    assert_eq!(merged.access_count, 7, "access counts accumulate");
    assert!(
        (merged.importance - 0.7).abs() < 1e-9,
        "keeps max importance"
    );
    assert!(
        merged.content.contains("(+1 merged)"),
        "marks merge size: {}",
        merged.content
    );
}

#[test]
fn rollup_returns_none_when_no_mergeable_pair() {
    let now = chrono::Utc::now();
    let entries = vec![
        MemoryEntry::with_id("a", "alpha", MemoryKind::Episodic, 0.5, now, 1),
        MemoryEntry::with_id(
            "b",
            "beta completely different",
            MemoryKind::Episodic,
            0.5,
            now,
            1,
        ),
    ];
    assert!(
        rollup(entries, DEDUP_SIMILARITY_THRESHOLD).is_none(),
        "no mergeable pair"
    );
    // 单条也无法 rollup。
    let single = vec![MemoryEntry::with_id(
        "s",
        "solo",
        MemoryKind::Episodic,
        0.5,
        now,
        1,
    )];
    assert!(rollup(single, DEDUP_SIMILARITY_THRESHOLD).is_none());
}

// ---- #829 周期合并调度器 ----

#[test]
fn scheduler_triggers_after_interval() {
    let mut s = ConsolidationScheduler::with_interval(3);
    assert_eq!(
        s.maybe_consolidate(|| false, || true),
        None,
        "before interval"
    );
    s.tick();
    assert_eq!(s.maybe_consolidate(|| false, || true), None);
    s.tick();
    assert_eq!(s.maybe_consolidate(|| false, || true), None);
    s.tick(); // 第 3 回合，到达间隔
    assert_eq!(
        s.maybe_consolidate(|| false, || true),
        Some(true),
        "should run"
    );
}

#[test]
fn scheduler_skips_when_compacting() {
    let mut s = ConsolidationScheduler::with_interval(2);
    s.tick();
    s.tick();
    // 压缩进行中：返回 Some(false)，不调用 run，不进入 in_progress。
    let mut ran = false;
    let res = s.maybe_consolidate(
        || true,
        || {
            ran = true;
            true
        },
    );
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
    let res = s.maybe_consolidate(
        || false,
        || {
            // 仅在 in_progress 必须为 true 的窗口内被调用。
            gate_seen = true;
            true
        },
    );
    assert_eq!(res, Some(true));
    assert!(
        gate_seen,
        "run callback fired => in_progress gate guarded the window"
    );
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
    assert_eq!(
        s.maybe_consolidate(|| false, || true),
        Some(true),
        "re-triggers after another interval"
    );
}
