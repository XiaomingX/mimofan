//! fleet::observability 能力验收 benchmark
//!
//! MECE 维度：Topology / Metrics / Summary / Format
//!
//! 运行: cargo test -p mimofan fleet_observability -- --nocapture

use mimofan::ObservabilityCollector;
use std::time::Duration;

// ═════════════════════════════════════════════════════════════════════════════
// Topology 维度
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn bench_topology_single_root() {
    let mut c = ObservabilityCollector::new();
    c.record_parent_child("root", "child1");
    c.record_parent_child("root", "child2");
    c.record_parent_child("child1", "grandchild");
    let t = c.topology();
    assert_eq!(t.total_agents(), 4);
    assert_eq!(t.roots().len(), 1);
    assert_eq!(t.roots()[0], "root");
    assert_eq!(t.depth("root"), 2);
    assert_eq!(t.depth("child1"), 1);
    assert_eq!(t.depth("grandchild"), 0);
    assert_eq!(t.children("root").len(), 2);
    println!("✓ bench_topology_single_root");
}

#[test]
fn bench_topology_multi_root() {
    let mut c = ObservabilityCollector::new();
    c.record_parent_child("agent_a", "a_child1");
    c.record_parent_child("agent_b", "b_child1");
    c.record_parent_child("agent_b", "b_child2");
    let t = c.topology();
    assert_eq!(t.total_agents(), 5);
    assert_eq!(t.roots().len(), 2);
    assert!(t.roots().contains(&&"agent_a".to_string()));
    assert!(t.roots().contains(&&"agent_b".to_string()));
    println!("✓ bench_topology_multi_root");
}

#[test]
fn bench_topology_empty() {
    let c = ObservabilityCollector::new();
    let t = c.topology();
    assert_eq!(t.total_agents(), 0);
    assert!(t.roots().is_empty());
    assert_eq!(t.depth("nonexistent"), 0);
    println!("✓ bench_topology_empty");
}

// ═════════════════════════════════════════════════════════════════════════════
// Metrics 维度
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn bench_metrics_aggregation() {
    let mut c = ObservabilityCollector::new();
    c.record_task_start("t1");
    c.record_task_completion("agent1", "t1", true, Some(64), Some(30.0));
    c.record_task_start("t2");
    c.record_task_completion("agent1", "t2", true, Some(128), Some(50.0));
    c.record_task_start("t3");
    c.record_task_completion("agent1", "t3", false, Some(256), None);
    let s = c.summary();
    assert_eq!(s.total_agents, 1);
    assert_eq!(s.completed_tasks, 2);
    assert_eq!(s.failed_tasks, 1);
    assert_eq!(s.running_agents, 0);
    assert_eq!(s.failed_agents, 1);
    assert_eq!(s.completed_agents, 0, "agent has failures");
    let m = &s.agents[0];
    assert_eq!(m.tasks_completed, 2);
    assert_eq!(m.tasks_failed, 1);
    assert_eq!(m.peak_memory_mb, Some(256));
    assert_eq!(m.peak_cpu_percent, Some(50.0));
    println!("✓ bench_metrics_aggregation");
}

#[test]
fn bench_metrics_no_start() {
    let mut c = ObservabilityCollector::new();
    c.record_task_completion("agent1", "orphan", true, None, None);
    let s = c.summary();
    assert_eq!(s.agents[0].total_duration, Duration::ZERO);
    assert_eq!(s.agents[0].avg_duration, Duration::ZERO);
    println!("✓ bench_metrics_no_start");
}

// ═════════════════════════════════════════════════════════════════════════════
// Summary 维度
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn bench_summary_depth() {
    let mut c = ObservabilityCollector::new();
    c.record_parent_child("r", "c1");
    c.record_parent_child("c1", "c2");
    c.record_parent_child("c2", "c3");
    let s = c.summary();
    assert_eq!(s.topology_depth, 3);
    assert_eq!(s.total_agents, 4);
    println!("✓ bench_summary_depth");
}

// ═════════════════════════════════════════════════════════════════════════════
// Format 维度
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn bench_format_basic() {
    let mut c = ObservabilityCollector::new();
    c.record_parent_child("root", "child1");
    c.record_parent_child("root", "child2");
    c.record_task_start("t1");
    c.record_task_completion("root", "t1", true, None, None);
    let tree = c.format_topology();
    assert!(tree.contains("✓ root"), "{}", tree);
    assert!(tree.contains("○ child1"), "{}", tree);
    assert!(tree.contains("○ child2"), "{}", tree);
    assert!(tree.contains("├─"), "{}", tree);
    assert!(tree.contains("└─"), "{}", tree);
    println!("✓ bench_format_basic\n{}", tree);
}

#[test]
fn bench_format_nested() {
    let mut c = ObservabilityCollector::new();
    c.record_parent_child("r", "a");
    c.record_parent_child("a", "b");
    c.record_parent_child("b", "c");
    c.record_task_start("t1");
    c.record_task_completion("c", "t1", false, None, None);
    let tree = c.format_topology();
    assert!(tree.contains("✗ c"), "{}", tree);
    assert!(tree.lines().count() >= 4, "{}", tree);
    println!("✓ bench_format_nested\n{}", tree);
}
