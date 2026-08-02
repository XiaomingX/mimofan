//! Fleet observability — topology, metrics, and status summary.
//!
//! Provides a lightweight observability layer on top of the existing fleet
//! ledger, enabling:
//! - Parent-child agent topology tracking
//! - Aggregated performance metrics (duration, token usage, cost)
//! - Status summary for `/fleet status` command
//!
//! This module is purely additive — it reads from existing ledger state
//! and does not modify any fleet execution paths.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Unique identifier for an agent (worker or sub-agent).
pub type AgentId = String;

/// Parent-child relationship between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTopology {
    /// Map from child agent id to its parent agent id.
    pub parent_of: BTreeMap<AgentId, AgentId>,
    /// Map from parent agent id to its children.
    pub children_of: BTreeMap<AgentId, Vec<AgentId>>,
}

impl AgentTopology {
    pub fn new() -> Self {
        Self {
            parent_of: BTreeMap::new(),
            children_of: BTreeMap::new(),
        }
    }
}

impl Default for AgentTopology {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentTopology {
    /// Register a parent-child relationship.
    pub fn register(&mut self, parent: AgentId, child: AgentId) {
        self.parent_of.insert(child.clone(), parent.clone());
        self.children_of.entry(parent).or_default().push(child);
    }

    /// Get the root agents (those with no parent).
    pub fn roots(&self) -> Vec<&AgentId> {
        self.children_of
            .keys()
            .filter(|id| !self.parent_of.contains_key(*id))
            .collect()
    }

    /// Get children of a given agent.
    pub fn children(&self, agent_id: &str) -> Vec<&AgentId> {
        self.children_of
            .get(agent_id)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get the depth of the tree from a given agent.
    pub fn depth(&self, agent_id: &str) -> usize {
        let children = self.children(agent_id);
        if children.is_empty() {
            0
        } else {
            1 + children.iter().map(|c| self.depth(c)).max().unwrap_or(0)
        }
    }

    /// Total number of agents in the topology.
    pub fn total_agents(&self) -> usize {
        let mut ids: std::collections::HashSet<&AgentId> = std::collections::HashSet::new();
        for (child, parent) in &self.parent_of {
            ids.insert(child);
            ids.insert(parent);
        }
        // Also include root agents that only appear in children_of
        for parent in self.children_of.keys() {
            ids.insert(parent);
        }
        ids.len()
    }
}

/// Aggregated performance metrics for a single agent or a fleet summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub agent_id: AgentId,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub total_duration: Duration,
    pub avg_duration: Duration,
    pub peak_memory_mb: Option<u64>,
    pub peak_cpu_percent: Option<f32>,
}

/// Summary of fleet-wide observability state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetStatusSummary {
    pub total_agents: usize,
    pub running_agents: usize,
    pub completed_agents: usize,
    pub failed_agents: usize,
    pub topology_depth: usize,
    pub total_tasks: u64,
    pub completed_tasks: u64,
    pub failed_tasks: u64,
    pub total_duration: Duration,
    pub agents: Vec<AgentMetrics>,
}

/// Observability collector that accumulates metrics from ledger events.
#[derive(Debug)]
pub struct ObservabilityCollector {
    topology: AgentTopology,
    metrics: BTreeMap<AgentId, AgentMetrics>,
    start_times: BTreeMap<String, Instant>,
}

impl ObservabilityCollector {
    pub fn new() -> Self {
        Self {
            topology: AgentTopology::new(),
            metrics: BTreeMap::new(),
            start_times: BTreeMap::new(),
        }
    }

    /// Record a parent-child agent relationship.
    pub fn record_parent_child(&mut self, parent: impl Into<String>, child: impl Into<String>) {
        self.topology.register(parent.into(), child.into());
    }

    /// Record task start time for duration tracking.
    pub fn record_task_start(&mut self, task_key: &str) {
        self.start_times
            .insert(task_key.to_string(), Instant::now());
    }

    /// Record task completion and update metrics.
    pub fn record_task_completion(
        &mut self,
        agent_id: &str,
        task_key: &str,
        success: bool,
        memory_mb: Option<u64>,
        cpu_percent: Option<f32>,
    ) {
        let duration = self
            .start_times
            .remove(task_key)
            .map(|start| start.elapsed())
            .unwrap_or_default();

        let entry = self
            .metrics
            .entry(agent_id.to_string())
            .or_insert_with(|| AgentMetrics {
                agent_id: agent_id.to_string(),
                ..Default::default()
            });

        if success {
            entry.tasks_completed += 1;
        } else {
            entry.tasks_failed += 1;
        }

        entry.total_duration += duration;
        let total = entry.tasks_completed + entry.tasks_failed;
        if total > 0 {
            entry.avg_duration = entry.total_duration / total as u32;
        }

        if let Some(mem) = memory_mb {
            entry.peak_memory_mb = Some(entry.peak_memory_mb.map(|m| m.max(mem)).unwrap_or(mem));
        }
        if let Some(cpu) = cpu_percent {
            entry.peak_cpu_percent =
                Some(entry.peak_cpu_percent.map(|c| c.max(cpu)).unwrap_or(cpu));
        }
    }

    /// Get the topology reference.
    pub fn topology(&self) -> &AgentTopology {
        &self.topology
    }

    /// Generate a fleet status summary.
    pub fn summary(&self) -> FleetStatusSummary {
        let agents: Vec<AgentMetrics> = self.metrics.values().cloned().collect();
        let running = agents
            .iter()
            .filter(|a| a.tasks_completed + a.tasks_failed == 0)
            .count();
        let completed = agents
            .iter()
            .filter(|a| a.tasks_failed == 0 && a.tasks_completed > 0)
            .count();
        let failed = agents.iter().filter(|a| a.tasks_failed > 0).count();

        let total_tasks: u64 = agents
            .iter()
            .map(|a| a.tasks_completed + a.tasks_failed)
            .sum();
        let completed_tasks: u64 = agents.iter().map(|a| a.tasks_completed).sum();
        let failed_tasks: u64 = agents.iter().map(|a| a.tasks_failed).sum();
        let total_duration: Duration = agents.iter().map(|a| a.total_duration).sum();

        let topology_depth = self
            .topology
            .roots()
            .iter()
            .map(|r| self.topology.depth(r))
            .max()
            .unwrap_or(0);

        // total_agents: 拓扑 + 指标中的去重 agent 数
        let mut all_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (child, parent) in &self.topology.parent_of {
            all_ids.insert(child);
            all_ids.insert(parent);
        }
        for parent in self.topology.children_of.keys() {
            all_ids.insert(parent);
        }
        for id in self.metrics.keys() {
            all_ids.insert(id);
        }
        FleetStatusSummary {
            total_agents: all_ids.len(),
            running_agents: running,
            completed_agents: completed,
            failed_agents: failed,
            topology_depth,
            total_tasks,
            completed_tasks,
            failed_tasks,
            total_duration,
            agents,
        }
    }

    /// Format the topology as an ASCII tree for TUI display.
    pub fn format_topology(&self) -> String {
        let mut lines = Vec::new();
        for root in self.topology.roots() {
            self.format_subtree(root, "", &mut lines);
        }
        lines.join("\n")
    }

    fn format_subtree(&self, agent_id: &str, prefix: &str, lines: &mut Vec<String>) {
        let metrics = self.metrics.get(agent_id);
        let status = match metrics {
            Some(m) if m.tasks_failed > 0 => "✗",
            Some(m) if m.tasks_completed > 0 => "✓",
            _ => "○",
        };
        let detail = metrics
            .map(|m| format!(" [{}ok/{}fail]", m.tasks_completed, m.tasks_failed))
            .unwrap_or_default();

        lines.push(format!("{}{} {}{}", prefix, status, agent_id, detail));

        let children = self.topology.children(agent_id);
        for (i, child) in children.iter().enumerate() {
            let is_last = i == children.len() - 1;
            let connector = if is_last { "└─" } else { "├─" };
            let child_prefix = if is_last {
                format!("{prefix}  ")
            } else {
                format!("{prefix}│ ")
            };
            self.format_subtree_with_prefix(child, &child_prefix, lines, prefix, connector);
        }
    }

    fn format_subtree_with_prefix(
        &self,
        agent_id: &str,
        child_prefix: &str,
        lines: &mut Vec<String>,
        parent_prefix: &str,
        connector: &str,
    ) {
        let metrics = self.metrics.get(agent_id);
        let status = match metrics {
            Some(m) if m.tasks_failed > 0 => "✗",
            Some(m) if m.tasks_completed > 0 => "✓",
            _ => "○",
        };
        let detail = metrics
            .map(|m| format!(" [{}ok/{}fail]", m.tasks_completed, m.tasks_failed))
            .unwrap_or_default();

        lines.push(format!(
            "{}{}{} {}{}",
            parent_prefix, connector, status, agent_id, detail
        ));

        let children = self.topology.children(agent_id);
        for (i, child) in children.iter().enumerate() {
            let is_last = i == children.len() - 1;
            let next_connector = if is_last { "└─" } else { "├─" };
            let next_prefix = if is_last {
                format!("{child_prefix}  ")
            } else {
                format!("{child_prefix}│ ")
            };
            self.format_subtree_with_prefix(
                child,
                &next_prefix,
                lines,
                child_prefix,
                next_connector,
            );
        }
    }
}

impl Default for ObservabilityCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_basics() {
        let mut topo = AgentTopology::new();
        topo.register("root".into(), "child1".into());
        topo.register("root".into(), "child2".into());
        topo.register("child1".into(), "grandchild".into());

        assert_eq!(topo.total_agents(), 4);
        assert_eq!(topo.depth("root"), 2);
        assert_eq!(topo.depth("child1"), 1);
        assert_eq!(topo.depth("grandchild"), 0);
        assert_eq!(topo.roots().len(), 1);
        assert_eq!(topo.children("root").len(), 2);
    }

    #[test]
    fn test_metrics_aggregation() {
        let mut collector = ObservabilityCollector::new();
        collector.record_task_start("task1");
        collector.record_task_completion("agent1", "task1", true, Some(128), Some(45.0));
        collector.record_task_start("task2");
        collector.record_task_completion("agent1", "task2", false, Some(256), None);

        let summary = collector.summary();
        assert_eq!(summary.total_agents, 1);
        assert_eq!(summary.completed_tasks, 1);
        assert_eq!(summary.failed_tasks, 1);
    }
}
