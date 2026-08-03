//! Task decomposition engine for DAG-based sub-agent orchestration.
//!
//! Breaks a complex task into a directed acyclic graph (DAG) of smaller
//! [`TaskNode`]s, each assigned to a [`SubAgentType`]. The graph supports
//! dependency tracking, topological scheduling, and parallel group resolution
//! so the runtime can fan-out independent nodes concurrently.

use std::collections::{HashMap, HashSet, VecDeque};

use super::SubAgentType;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Execution status of a single task node.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskNodeStatus {
    /// Waiting for dependencies to complete.
    #[default]
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Explicitly cancelled.
    Cancelled,
}

/// A single unit of work inside a [`TaskGraph`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskNode {
    /// Unique identifier within the graph.
    pub id: String,
    /// Human-readable description of what this node does.
    pub description: String,
    /// Which sub-agent type should execute this node.
    pub agent_type: SubAgentType,
    /// IDs of nodes that must complete before this one can start.
    pub dependencies: Vec<String>,
    /// Current execution status.
    pub status: TaskNodeStatus,
    /// Result payload after completion (set by the runtime).
    pub result: Option<String>,
}

/// A directed acyclic graph of [`TaskNode`]s representing a decomposed task.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TaskGraph {
    /// All nodes keyed by their unique ID.
    pub nodes: HashMap<String, TaskNode>,
    /// Explicit edges as `(from, to)` pairs – derived from node dependencies.
    pub edges: Vec<(String, String)>,
}

/// Decomposes a high-level task description into a DAG of sub-agent tasks.
///
/// The decomposer itself is stateless; all mutable state lives in the
/// [`TaskGraph`] it produces.
#[derive(Debug, Clone, Default)]
pub struct TaskDecomposer;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors specific to task decomposition and graph operations.
#[derive(Debug, thiserror::Error)]
pub enum DecomposerError {
    #[error("node not found: {0}")]
    NodeNotFound(String),

    #[error("duplicate node id: {0}")]
    DuplicateNodeId(String),

    #[error("cycle detected involving node: {0}")]
    CycleDetected(String),

    #[error("missing dependency `{dep}` for node `{node}`")]
    MissingDependency { node: String, dep: String },

    #[error("node `{0}` is not in a terminal state")]
    NotTerminal(String),
}

// ---------------------------------------------------------------------------
// TaskGraph implementation
// ---------------------------------------------------------------------------

impl TaskGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node, rejecting duplicate IDs.
    pub fn add_node(&mut self, node: TaskNode) -> Result<(), DecomposerError> {
        if self.nodes.contains_key(&node.id) {
            return Err(DecomposerError::DuplicateNodeId(node.id));
        }
        let id = node.id.clone();
        self.nodes.insert(id, node);
        Ok(())
    }

    /// Build the edge list from node dependencies.
    ///
    /// Call after all nodes have been added to populate `self.edges`.
    pub fn build_edges(&mut self) -> Result<(), DecomposerError> {
        self.edges.clear();
        for node in self.nodes.values() {
            for dep in &node.dependencies {
                if !self.nodes.contains_key(dep) {
                    return Err(DecomposerError::MissingDependency {
                        node: node.id.clone(),
                        dep: dep.clone(),
                    });
                }
                self.edges.push((dep.clone(), node.id.clone()));
            }
        }
        Ok(())
    }

    /// Validate the graph: no missing dependencies, no cycles.
    pub fn validate(&self) -> Result<(), DecomposerError> {
        // Check all dependency references exist.
        for node in self.nodes.values() {
            for dep in &node.dependencies {
                if !self.nodes.contains_key(dep) {
                    return Err(DecomposerError::MissingDependency {
                        node: node.id.clone(),
                        dep: dep.clone(),
                    });
                }
            }
        }
        // Cycle detection via Kahn's algorithm.
        let _ = self.topological_sort()?;
        Ok(())
    }

    /// Return the number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Return nodes whose dependencies are all completed (ready to schedule).
    pub fn ready_nodes(&self) -> Vec<&TaskNode> {
        self.nodes
            .values()
            .filter(|n| {
                n.status == TaskNodeStatus::Pending
                    && n.dependencies.iter().all(|dep| {
                        self.nodes
                            .get(dep)
                            .is_some_and(|d| d.status == TaskNodeStatus::Completed)
                    })
            })
            .collect()
    }

    /// Mark a node as completed with an optional result.
    pub fn complete_node(
        &mut self,
        id: &str,
        result: Option<String>,
    ) -> Result<(), DecomposerError> {
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| DecomposerError::NodeNotFound(id.to_string()))?;
        node.status = TaskNodeStatus::Completed;
        node.result = result;
        Ok(())
    }

    /// Mark a node as failed.
    pub fn fail_node(&mut self, id: &str) -> Result<(), DecomposerError> {
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| DecomposerError::NodeNotFound(id.to_string()))?;
        node.status = TaskNodeStatus::Failed;
        Ok(())
    }

    /// Mark a node as running.
    pub fn start_node(&mut self, id: &str) -> Result<(), DecomposerError> {
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| DecomposerError::NodeNotFound(id.to_string()))?;
        node.status = TaskNodeStatus::Running;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Core algorithms
    // -----------------------------------------------------------------------

    /// Topological sort using Kahn's algorithm.
    ///
    /// Returns nodes in a valid execution order. Returns an error if the graph
    /// contains a cycle.
    pub fn topological_sort(&self) -> Result<Vec<String>, DecomposerError> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        for id in self.nodes.keys() {
            in_degree.entry(id.as_str()).or_insert(0);
            adjacency.entry(id.as_str()).or_default();
        }

        for (from, to) in &self.edges {
            adjacency
                .entry(from.as_str())
                .or_default()
                .push(to.as_str());
            *in_degree.entry(to.as_str()).or_insert(0) += 1;
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(id, _)| *id)
            .collect();

        let mut sorted = Vec::with_capacity(self.nodes.len());

        while let Some(id) = queue.pop_front() {
            sorted.push(id.to_string());
            if let Some(neighbors) = adjacency.get(id) {
                for &neighbor in neighbors {
                    let deg = in_degree.get_mut(neighbor).expect("in-degree entry missing for neighbor");
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        if sorted.len() != self.nodes.len() {
            // Find one node involved in the cycle for the error message.
            let cycle_node = self
                .nodes
                .keys()
                .find(|id| !sorted.contains(id))
                .cloned()
                .unwrap_or_default();
            return Err(DecomposerError::CycleDetected(cycle_node));
        }

        Ok(sorted)
    }

    /// Group nodes into parallel execution waves.
    ///
    /// Each group contains nodes whose dependencies are all within earlier
    /// groups. Nodes within the same group have no inter-dependencies and can
    /// run concurrently.
    pub fn parallel_groups(&self) -> Result<Vec<Vec<String>>, DecomposerError> {
        let sorted = self.topological_sort()?;

        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        for id in self.nodes.keys() {
            in_degree.entry(id.as_str()).or_insert(0);
            adjacency.entry(id.as_str()).or_default();
        }

        for (from, to) in &self.edges {
            adjacency
                .entry(from.as_str())
                .or_default()
                .push(to.as_str());
            *in_degree.entry(to.as_str()).or_insert(0) += 1;
        }

        let mut groups = Vec::new();
        let mut remaining: HashSet<&str> = sorted.iter().map(String::as_str).collect();

        while !remaining.is_empty() {
            let group: Vec<String> = sorted
                .iter()
                .filter(|id| remaining.contains(id.as_str()))
                .filter(|id| {
                    // All dependencies are either completed or not in remaining.
                    self.nodes
                        .get(id.as_str())
                        .map(|n| {
                            n.dependencies
                                .iter()
                                .all(|dep| !remaining.contains(dep.as_str()))
                        })
                        .unwrap_or(false)
                })
                .cloned()
                .collect();

            if group.is_empty() {
                // Safety: the topological sort guarantees progress.
                break;
            }

            for id in &group {
                remaining.remove(id.as_str());
            }
            groups.push(group);
        }

        Ok(groups)
    }
}

// ---------------------------------------------------------------------------
// TaskDecomposer
// ---------------------------------------------------------------------------

impl TaskDecomposer {
    /// Create a new decomposer.
    pub fn new() -> Self {
        Self
    }

    /// Decompose a list of task descriptions into a [`TaskGraph`].
    ///
    /// Each entry in `tasks` is `(id, description, agent_type, dependency_ids)`.
    pub fn decompose(
        &self,
        tasks: Vec<(String, String, SubAgentType, Vec<String>)>,
    ) -> Result<TaskGraph, DecomposerError> {
        let mut graph = TaskGraph::new();

        for (id, description, agent_type, dependencies) in tasks {
            graph.add_node(TaskNode {
                id,
                description,
                agent_type,
                dependencies,
                status: TaskNodeStatus::Pending,
                result: None,
            })?;
        }

        graph.build_edges()?;
        graph.validate()?;

        Ok(graph)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
