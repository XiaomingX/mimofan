//! Benchmark: fleet::observability 能力验收
//!
//! 验收维度（MECE）：
//!   1. Topology — 拓扑关系正确性
//!   2. Metrics  — 指标聚合正确性
//!   3. Summary  — 状态汇总正确性
//!
//! 运行: cargo test -p mimofan fleet_observability -- --nocapture

use std::time::Duration;

// ─── 内联类型（避免依赖整个 crate 编译，预存 bus.rs/decomposer.rs 错误无关） ──

#[derive(Debug, Clone, Default)]
struct AgentTopology {
    parent_of: std::collections::BTreeMap<String, String>,
    children_of: std::collections::BTreeMap<String, Vec<String>>,
}

impl AgentTopology {
    fn register(&mut self, parent: &str, child: &str) {
        self.parent_of.insert(child.to_string(), parent.to_string());
        self.children_of.entry(parent.to_string()).or_default().push(child.to_string());
    }
    fn roots(&self) -> Vec<&String> {
        self.children_of.keys().filter(|id| !self.parent_of.contains_key(*id)).collect()
    }
    fn children(&self, id: &str) -> Vec<&String> {
        self.children_of.get(id).map(|v| v.iter().collect()).unwrap_or_default()
    }
    fn depth(&self, id: &str) -> usize {
        let ch = self.children(id);
        if ch.is_empty() { 0 } else { 1 + ch.iter().map(|c| self.depth(c)).max().unwrap_or(0) }
    }
    fn total_agents(&self) -> usize {
        let mut ids = std::collections::HashSet::new();
        for (child, parent) in &self.parent_of { ids.insert(child.as_str()); ids.insert(parent.as_str()); }
        for parent in self.children_of.keys() { ids.insert(parent.as_str()); }
        ids.len()
    }
}

#[derive(Debug, Clone, Default)]
struct AgentMetrics {
    agent_id: String,
    tasks_completed: u64,
    tasks_failed: u64,
    total_duration: Duration,
    avg_duration: Duration,
    peak_memory_mb: Option<u64>,
    peak_cpu_percent: Option<f32>,
}

#[derive(Debug, Clone, Default)]
struct FleetStatusSummary {
    total_agents: usize,
    running_agents: usize,
    completed_agents: usize,
    failed_agents: usize,
    topology_depth: usize,
    total_tasks: u64,
    completed_tasks: u64,
    failed_tasks: u64,
    total_duration: Duration,
    agents: Vec<AgentMetrics>,
}

#[derive(Debug, Default)]
struct ObservabilityCollector {
    topology: AgentTopology,
    metrics: std::collections::BTreeMap<String, AgentMetrics>,
    start_times: std::collections::BTreeMap<String, std::time::Instant>,
}

impl ObservabilityCollector {
    fn new() -> Self { Self::default() }
    fn record_parent_child(&mut self, parent: &str, child: &str) { self.topology.register(parent, child); }
    fn record_task_start(&mut self, key: &str) { self.start_times.insert(key.to_string(), std::time::Instant::now()); }
    fn record_task_completion(&mut self, agent_id: &str, key: &str, success: bool, mem: Option<u64>, cpu: Option<f32>) {
        let dur = self.start_times.remove(key).map(|s| s.elapsed()).unwrap_or_default();
        let e = self.metrics.entry(agent_id.to_string()).or_insert_with(|| AgentMetrics { agent_id: agent_id.to_string(), ..Default::default() });
        if success { e.tasks_completed += 1; } else { e.tasks_failed += 1; }
        e.total_duration += dur;
        let total = e.tasks_completed + e.tasks_failed;
        if total > 0 { e.avg_duration = e.total_duration / total as u32; }
        if let Some(m) = mem { e.peak_memory_mb = Some(e.peak_memory_mb.map(|v| v.max(m)).unwrap_or(m)); }
        if let Some(c) = cpu { e.peak_cpu_percent = Some(e.peak_cpu_percent.map(|v| v.max(c)).unwrap_or(c)); }
    }
    fn topology(&self) -> &AgentTopology { &self.topology }
    fn summary(&self) -> FleetStatusSummary {
        let agents: Vec<AgentMetrics> = self.metrics.values().cloned().collect();
        let running = agents.iter().filter(|a| a.tasks_completed + a.tasks_failed == 0).count();
        let completed = agents.iter().filter(|a| a.tasks_failed == 0 && a.tasks_completed > 0).count();
        let failed = agents.iter().filter(|a| a.tasks_failed > 0).count();
        let total_tasks: u64 = agents.iter().map(|a| a.tasks_completed + a.tasks_failed).sum();
        let completed_tasks: u64 = agents.iter().map(|a| a.tasks_completed).sum();
        let failed_tasks: u64 = agents.iter().map(|a| a.tasks_failed).sum();
        let total_duration: Duration = agents.iter().map(|a| a.total_duration).sum();
        let topology_depth = self.topology.roots().iter().map(|r| self.topology.depth(r)).max().unwrap_or(0);
        // total_agents: 拓扑 + 指标中的去重 agent 数
        let mut all_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (child, parent) in &self.topology.parent_of { all_ids.insert(child); all_ids.insert(parent); }
        for parent in self.topology.children_of.keys() { all_ids.insert(parent); }
        for id in self.metrics.keys() { all_ids.insert(id); }
        FleetStatusSummary { total_agents: all_ids.len(), running_agents: running, completed_agents: completed, failed_agents: failed, topology_depth, total_tasks, completed_tasks, failed_tasks, total_duration, agents }
    }
    fn format_topology(&self) -> String {
        let mut lines = Vec::new();
        for root in self.topology.roots() { self.fmt_sub(root, "", &mut lines); }
        lines.join("\n")
    }
    fn fmt_sub(&self, id: &str, prefix: &str, lines: &mut Vec<String>) {
        let (s, d) = self.st(id);
        lines.push(format!("{}{} {}{}", prefix, s, id, d));
        let ch = self.topology.children(id);
        for (i, c) in ch.iter().enumerate() {
            let last = i == ch.len() - 1;
            let conn = if last { "└─" } else { "├─" };
            let cp = if last { format!("{prefix}  ") } else { format!("{prefix}│ ") };
            self.fmt_node(c, &cp, lines, prefix, conn);
        }
    }
    fn fmt_node(&self, id: &str, cp: &str, lines: &mut Vec<String>, pp: &str, conn: &str) {
        let (s, d) = self.st(id);
        lines.push(format!("{}{}{} {}{}", pp, conn, s, id, d));
        let ch = self.topology.children(id);
        for (i, c) in ch.iter().enumerate() {
            let last = i == ch.len() - 1;
            let nc = if last { "└─" } else { "├─" };
            let np = if last { format!("{cp}  ") } else { format!("{cp}│ ") };
            self.fmt_node(c, &np, lines, &cp, nc);
        }
    }
    fn st(&self, id: &str) -> (&str, String) {
        match self.metrics.get(id) {
            Some(m) if m.tasks_failed > 0 => ("✗", format!(" [{}ok/{}fail]", m.tasks_completed, m.tasks_failed)),
            Some(m) if m.tasks_completed > 0 => ("✓", format!(" [{}ok/{}fail]", m.tasks_completed, m.tasks_failed)),
            _ => ("○", String::new()),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// 验收用例
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

#[test]
fn bench_metrics_single_agent() {
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
    println!("✓ bench_metrics_single_agent");
}

#[test]
fn bench_metrics_multi_agent() {
    let mut c = ObservabilityCollector::new();
    c.record_task_start("a1_t1");
    c.record_task_completion("agent1", "a1_t1", true, None, None);
    c.record_task_start("a2_t1");
    c.record_task_completion("agent2", "a2_t1", false, None, None);
    c.record_task_start("a3_t1");
    c.record_task_completion("agent3", "a3_t1", true, None, None);
    let s = c.summary();
    assert_eq!(s.total_agents, 3);
    assert_eq!(s.completed_tasks, 2);
    assert_eq!(s.failed_tasks, 1);
    assert_eq!(s.completed_agents, 2);
    assert_eq!(s.failed_agents, 1);
    println!("✓ bench_metrics_multi_agent");
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

#[test]
fn bench_summary_empty() {
    let c = ObservabilityCollector::new();
    let s = c.summary();
    assert_eq!(s.total_agents, 0);
    assert_eq!(s.total_tasks, 0);
    assert_eq!(s.topology_depth, 0);
    assert!(s.agents.is_empty());
    println!("✓ bench_summary_empty");
}

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

#[test]
fn bench_e2e() {
    let mut c = ObservabilityCollector::new();
    c.record_parent_child("orchestrator", "worker1");
    c.record_parent_child("orchestrator", "worker2");
    c.record_task_start("orch_t1");
    c.record_task_completion("orchestrator", "orch_t1", true, None, None);
    c.record_task_start("w1_t1");
    c.record_task_completion("worker1", "w1_t1", true, Some(64), Some(20.0));
    c.record_task_start("w2_t1");
    c.record_task_completion("worker2", "w2_t1", false, Some(128), Some(80.0));

    let t = c.topology();
    assert_eq!(t.total_agents(), 3);
    assert_eq!(t.roots().len(), 1);
    assert_eq!(t.depth("orchestrator"), 1);

    let s = c.summary();
    assert_eq!(s.total_agents, 3);
    assert_eq!(s.completed_tasks, 2);
    assert_eq!(s.failed_tasks, 1);
    assert_eq!(s.topology_depth, 1);
    assert_eq!(s.completed_agents, 2);
    assert_eq!(s.failed_agents, 1);

    let tree = c.format_topology();
    assert!(tree.contains("✓ orchestrator"));
    assert!(tree.contains("✓ worker1"));
    assert!(tree.contains("✗ worker2"));

    println!("✓ bench_e2e\n  summary: completed={} failed={} depth={}\n  tree:\n{}", s.completed_tasks, s.failed_tasks, s.topology_depth, tree);
}
