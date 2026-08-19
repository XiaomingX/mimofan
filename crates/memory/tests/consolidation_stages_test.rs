use mimofan_memory::consolidation::{MemoryEntry, MemoryKind};
use mimofan_memory::consolidation_stages::{abstract_rules, dream_cycle, extract, integrate};

fn entry(id: &str, content: &str, importance: f64) -> MemoryEntry {
    MemoryEntry::with_id(id, content, MemoryKind::Episodic, importance, chrono::Utc::now(), 1)
}

#[test]
fn extract_filters_low_importance() {
    let entries = vec![
        entry("a", "high signal deploy fix", 0.9),
        entry("b", "noise to forget", 0.2),
        entry("c", "medium signal refactor", 0.5), // 恰好等于门槛，保留
    ];
    let out = extract(entries);
    assert_eq!(out.len(), 2, "low importance dropped");
    let ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
    assert!(ids.contains(&"a"));
    assert!(ids.contains(&"c"));
    assert!(!ids.contains(&"b"), "weak signal excluded");
}

#[test]
fn extract_empty_input() {
    let out: Vec<MemoryEntry> = extract(Vec::new());
    assert!(out.is_empty());
}

#[test]
fn integrate_dedups_and_merges() {
    let entries = vec![
        entry("a", "use cargo test to verify", 0.9),
        entry("b", "use cargo test to verify module", 0.8),
        entry("c", "unrelated lunch memory", 0.7),
    ];
    let out = integrate(entries);
    // a、b 归并为一条语义记忆，c 独立保留 → 2 条。
    assert_eq!(out.len(), 2, "similar pair merged, unrelated kept");
    let semantic = out.iter().any(|e| e.kind == MemoryKind::Semantic);
    assert!(semantic, "a merge produced a Semantic entry");
}

#[test]
fn integrate_no_false_merge_on_disjoint() {
    let entries = vec![
        entry("x", "alpha beta gamma", 0.9),
        entry("y", "zeta theta iota", 0.9),
    ];
    let out = integrate(entries);
    assert_eq!(out.len(), 2, "disjoint contents stay separate");
}

#[test]
fn abstract_produces_nonempty_when_themes_recur() {
    let integrated = vec![
        entry("a", "cargo test verifies the module", 0.7),
        entry("b", "cargo test verifies the build", 0.8),
        entry("c", "cargo test catches regressions", 0.6),
    ];
    let rules = abstract_rules(integrated);
    assert!(
        !rules.is_empty(),
        "recurring 'cargo'/'test' theme must yield a rule"
    );
    assert!(
        rules.iter().all(|r| r.kind == MemoryKind::Abstracted),
        "abstract rules are Abstracted kind"
    );
    let contents: Vec<&str> = rules.iter().map(|e| e.content.as_str()).collect();
    // "cargo" 与 "test" 都跨 ≥2 条共现（"the" 是停用词被过滤）。
    assert!(contents.iter().any(|c| c.contains("cargo")));
    assert!(contents.iter().any(|c| c.contains("test")));
}

#[test]
fn abstract_empty_when_too_sparse() {
    // 单条无法形成共现主题。
    let single = vec![entry("s", "solo unique memory here", 0.9)];
    assert!(abstract_rules(single).is_empty());
    // 两条但无共现 token（停用词外）。
    let disjoint = vec![
        entry("x", "alpha beta gamma", 0.9),
        entry("y", "delta epsilon zeta", 0.9),
    ];
    assert!(abstract_rules(disjoint).is_empty(), "no shared theme => no rule");
}

#[test]
fn dream_cycle_runs_all_three_stages() {
    let raw = vec![
        entry("a", "cargo test verifies the module", 0.9),
        entry("b", "cargo test verifies the build", 0.8),
        entry("c", "cargo test catches regressions", 0.7),
        entry("d", "weak noise to forget entirely", 0.1),
        entry("e", "unrelated lunch memory topic", 0.6),
        entry("f", "unrelated lunch memory elsewhere", 0.6),
    ];
    let res = dream_cycle(raw);
    assert_eq!(res.raw_count, 6);
    assert_eq!(res.extracted_count, 5, "weak entry filtered");
    assert!(res.integrated_count >= 1, "integration keeps something");
    assert!(
        !res.abstractions.is_empty(),
        "dream produced abstract rules"
    );
    assert!(res.extracted_count >= res.integrated_count || res.integrated_count == 1);
}

#[test]
fn dream_cycle_empty_input() {
    let res = dream_cycle(Vec::<MemoryEntry>::new());
    assert_eq!(res.raw_count, 0);
    assert_eq!(res.extracted_count, 0);
    assert_eq!(res.integrated_count, 0);
    assert!(res.abstractions.is_empty());
}
