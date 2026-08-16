//! Programmable workflow orchestration: `workflow`.
//!
//! A declarative JSON DAG engine (#T-Q1). Workflow nodes are sub-agent calls;
//! edges express dependency / ordering. The engine supports three primitives:
//!
//! - **parallel** — nodes with no remaining unsatisfied dependencies are
//!   launched concurrently (bounded by `max_parallel`).
//! - **sequential** — a node lists its predecessors in `depends_on`, so it only
//!   starts after they finish (expressed naturally by the DAG edges).
//! - **conditional** — a node may carry a `when` gate that only fires when the
//!   referenced upstream results match an expected status.
//!
//! ## Reuse, not re-invention
//!
//! Every node is executed by spawning a *real* sub-agent through the existing
//! dispatch path (`tools::subagent::tool::spawn_subagent_from_input`), so:
//!
//! - **token budget** is inherited from the shared `SubAgentManager`
//!   (`with_default_token_budget` / `resolve_spawn_budget_scope`); the workflow
//!   engine only *forwards* a node's `token_budget` into the spawn request and
//!   never re-implements budget accounting.
//! - **worktree isolation** is delegated to the existing parser/manager worktree
//!   creation (`parse_optional_worktree_request` / `prepare_child_workspace`).
//!   Setting `worktree: true` on a node reuses that exact code path.
//! - **heartbeat / stall** — the upstream manager only has a heartbeat *timeout*
//!   (it never retries a stalled agent). This engine adds the missing stall→retry
//!   behavior on top, by watching each running node's worker-record
//!   `updated_at_ms` (public `get_worker_record`) and respawning after
//!   `stall_timeout_ms` of no progress, up to the node's `retry` budget.
//! - **journal / resume** — the upstream `SubAgentPersistence` has no
//!   resume/replay. This engine adds a workflow-level journal: it persists node
//!   state transitions to `<workspace>/.mimofan/workflows/<run_id>.json` and can
//!   replay a prior run id, skipping `completed` nodes and re-driving the rest.
//!
//! ## The `NodeExecutor` seam
//!
//! The scheduling logic (topological readiness, parallel fan-out, condition
//! gating, stall retry, journal replay) is decoupled from the actual agent
//! spawn behind the `NodeExecutor` trait. The default production executor
//! (`SubagentNodeExecutor`) drives the real sub-agent manager; unit tests
//! inject a deterministic in-memory executor so the full DAG semantics can be
//! exercised without a model backend.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};
use super::subagent::{
    SharedSubAgentManager, SubAgentAssignment, SubAgentResult, SubAgentStatus, SubAgentType,
    tool::spawn_subagent_from_input,
};

/// Tool name exposed to the model.
pub const WORKFLOW_TOOL_NAME: &str = "workflow";

/// Persisted journal directory relative to the workspace `.mimofan` folder.
const WORKFLOW_JOURNAL_DIR: &str = ".mimofan/workflows";

/// Per-node terminal state used by the engine and the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// Not yet launched (or waiting on dependencies / a `when` gate).
    Pending,
    /// Agent spawned; awaiting completion.
    Running,
    /// Completed successfully.
    Completed,
    /// Exhausted its retry budget or hit a non-retryable failure.
    Failed,
    /// Skipped because its `when` condition was not satisfied.
    Skipped,
}

impl NodeState {
    /// Terminal states no longer need scheduling.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            NodeState::Completed | NodeState::Failed | NodeState::Skipped
        )
    }
}

/// A declarative workflow node — one sub-agent call in the DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeSpec {
    /// Unique node id within the workflow.
    pub id: String,
    /// Sub-agent type: `general`, `explore`, `plan`, `review`,
    /// `implementer`, `verifier`, `custom`. Defaults to `general`.
    #[serde(default)]
    pub r#type: Option<String>,
    /// Prompt handed to the sub-agent.
    pub prompt: String,
    /// Node ids this node depends on. The node becomes ready only once every
    /// dependency is `Completed` (sequential composition is expressed this way).
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Optional stable session name for the spawned sub-agent.
    #[serde(default)]
    pub name: Option<String>,
    /// Run this node in an isolated git worktree (reuses the existing
    /// `worktree: true` path in the spawn parser).
    #[serde(default)]
    pub worktree: bool,
    /// Optional per-node token budget forwarded to the sub-agent spawn
    /// (inherited from the manager default when omitted).
    #[serde(default)]
    pub token_budget: Option<u64>,
    /// Optional model override for this node.
    #[serde(default)]
    pub model: Option<String>,
    /// Number of stall/transient retries before the node is marked `Failed`.
    /// Defaults to `0` (no retry).
    #[serde(default)]
    pub retry: u32,
    /// Optional conditional gate. The node only launches when `when` evaluates
    /// true against the already-known statuses of the referenced upstream nodes.
    #[serde(default)]
    pub r#when: Option<WhenGate>,
}

/// A lightweight conditional gate on upstream node statuses.
///
/// Only the simplest, fully deterministic form is supported: every referenced
/// `depends_on` id must have a status contained in `expect_status`. This avoids
/// embedding a script interpreter while still enabling real branch pruning
/// (e.g. "only run the verifier if the implementer succeeded").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhenGate {
    /// Node ids whose terminal status is checked.
    #[serde(default)]
    pub on: Vec<String>,
    /// Allowed terminal statuses (default `["completed"]`).
    #[serde(default)]
    pub expect_status: Vec<String>,
}

impl Default for WhenGate {
    fn default() -> Self {
        Self {
            on: Vec::new(),
            expect_status: vec!["completed".to_string()],
        }
    }
}

/// Full workflow declaration submitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSpec {
    /// Human-readable workflow name (used in journal/UI).
    #[serde(default)]
    pub name: Option<String>,
    /// DAG nodes.
    pub nodes: Vec<WorkflowNodeSpec>,
    /// Max nodes launched concurrently (default `4`). The dependency graph
    /// always takes precedence; this only bounds fan-out.
    #[serde(default)]
    pub max_parallel: Option<usize>,
    /// Stall timeout in milliseconds. If a running node's worker record shows
    /// no progress for this long, it is retried (default 300_000 = 5 min).
    #[serde(default)]
    pub stall_timeout_ms: Option<u64>,
    /// Resume a previous run by id instead of starting fresh. Completed nodes
    /// are skipped; incomplete nodes are re-driven (journal replay).
    #[serde(default)]
    pub resume_run_id: Option<String>,
}

/// A node's live state inside the running engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntime {
    pub id: String,
    pub state: NodeState,
    /// Sub-agent id from the last spawn (for status polling / journal replay).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Number of attempts already made (including the current one).
    #[serde(default)]
    pub attempts: u32,
    /// How many retries remain (from `node.retry`).
    #[serde(default)]
    pub retries_left: u32,
    /// Captured final result text once terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Instant `updated_at_ms` was last observed, for stall detection.
    #[serde(skip)]
    last_progress_at: Option<Instant>,
}

impl NodeRuntime {
    fn new(id: String, retries: u32) -> Self {
        Self {
            id,
            state: NodeState::Pending,
            agent_id: None,
            attempts: 0,
            retries_left: retries,
            result: None,
            last_progress_at: None,
        }
    }
}

/// The persisted journal for a single workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowJournal {
    pub run_id: String,
    pub name: String,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    pub nodes: HashMap<String, NodeRuntime>,
}

impl WorkflowJournal {
    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn save(&self, dir: &Path) -> std::io::Result<()> {
        let dir = dir.join(WORKFLOW_JOURNAL_DIR);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", self.run_id));
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, bytes)
    }

    fn load(dir: &Path, run_id: &str) -> std::io::Result<Option<Self>> {
        let path = dir
            .join(WORKFLOW_JOURNAL_DIR)
            .join(format!("{run_id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        let journal: Self = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Some(journal))
    }
}

/// Trait that actually executes a node.
///
/// Decoupled so the DAG scheduler is unit-testable without a live model. The
/// production impl (`SubagentNodeExecutor`) spawns real sub-agents through the
/// shared manager; tests inject a deterministic in-memory executor.
#[async_trait]
pub trait NodeExecutor: Send + Sync {
    /// Spawn (or re-spawn) a node and return the sub-agent id. The executor is
    /// responsible for reusing the manager's budget and worktree machinery.
    async fn launch(
        &self,
        node: &WorkflowNodeSpec,
        run_id: &str,
        attempt: u32,
    ) -> Result<String, ToolError>;

    /// Poll the current status of a previously launched node.
    async fn poll(&self, agent_id: &str) -> Result<SubAgentResult, ToolError>;

    /// Cancel a running node (used before a stall retry).
    async fn cancel(&self, agent_id: &str) -> Result<(), ToolError>;

    /// Last progress timestamp (ms) for a running node, used for stall
    /// detection. Returns `None` when unknown.
    async fn last_progress_ms(&self, agent_id: &str) -> Option<u64>;
}

/// Production executor that drives the real shared sub-agent manager.
pub struct SubagentNodeExecutor {
    manager: SharedSubAgentManager,
    runtime: super::subagent::SubAgentRuntime,
}

impl SubagentNodeExecutor {
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: super::subagent::SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

/// Build the spawn input JSON for a node, reusing the existing `agent` tool's
/// request schema (so `worktree`, `token_budget`, `model`, `name`, `prompt`,
/// and agent `type` all flow through the already-validated parser path).
fn node_spawn_input(node: &WorkflowNodeSpec, run_id: &str, attempt: u32) -> Value {
    let agent_type = node.r#type.clone().unwrap_or_else(|| "general".to_string());
    let session_name = node.name.clone().unwrap_or_else(|| {
        if attempt == 0 {
            format!("{run_id}:{}", node.id)
        } else {
            format!("{run_id}:{}#{}", node.id, attempt)
        }
    });
    json!({
        "action": "start",
        "name": session_name,
        "prompt": node.prompt,
        "subagent_type": agent_type,
        "worktree": node.worktree,
        "token_budget": node.token_budget,
        "model": node.model,
    })
}

#[async_trait]
impl NodeExecutor for SubagentNodeExecutor {
    async fn launch(
        &self,
        node: &WorkflowNodeSpec,
        run_id: &str,
        attempt: u32,
    ) -> Result<String, ToolError> {
        let input = node_spawn_input(node, run_id, attempt);
        let result =
            spawn_subagent_from_input(input, self.manager.clone(), self.runtime.clone()).await?;
        Ok(result.agent_id)
    }

    async fn poll(&self, agent_id: &str) -> Result<SubAgentResult, ToolError> {
        let guard = self.manager.read().await;
        let result = guard
            .get_result_by_ref(agent_id)
            .map_err(|e| ToolError::execution_failed(format!("workflow poll failed: {e}")))?;
        Ok(result)
    }

    async fn cancel(&self, agent_id: &str) -> Result<(), ToolError> {
        // Reuse the manager's cancel path via the public `agent` action on the
        // manager (it resolves by id/name and tears down the worktree).
        let mut guard = self.manager.write().await;
        if let Err(e) = guard.cancel_agent(agent_id) {
            tracing::warn!("workflow stall-retry cancel of {agent_id} failed: {e}");
        }
        Ok(())
    }

    async fn last_progress_ms(&self, agent_id: &str) -> Option<u64> {
        let guard = self.manager.read().await;
        guard.get_worker_record(agent_id).map(|r| r.updated_at_ms)
    }
}

/// Convenience helper: is a `SubAgentStatus` terminal for workflow purposes?
fn is_terminal_status(status: &SubAgentStatus) -> bool {
    !matches!(status, SubAgentStatus::Running)
}

/// Topologically sort node ids; returns `None` on a dependency cycle.
fn topo_order(specs: &[WorkflowNodeSpec]) -> Option<Vec<String>> {
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in specs {
        indeg.entry(n.id.as_str()).or_insert(0);
        for dep in &n.depends_on {
            adj.entry(dep.as_str()).or_default().push(n.id.as_str());
            *indeg.entry(n.id.as_str()).or_insert(0) += 1;
        }
    }
    let mut queue: VecDeque<&str> = indeg
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(&k, _)| k)
        .collect();
    let mut order = Vec::with_capacity(specs.len());
    while let Some(id) = queue.pop_front() {
        order.push(id.to_string());
        if let Some(outs) = adj.get(id) {
            for &out in outs {
                let d = indeg.get_mut(out).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(out);
                }
            }
        }
    }
    if order.len() == specs.len() {
        Some(order)
    } else {
        None
    }
}

/// The workflow execution engine. Holds the DAG and drives node scheduling.
pub struct WorkflowEngine<E: NodeExecutor> {
    spec: WorkflowSpec,
    specs_by_id: HashMap<String, WorkflowNodeSpec>,
    run_id: String,
    journal: WorkflowJournal,
    executor: Arc<E>,
    workspace: PathBuf,
    max_parallel: usize,
    stall_timeout: Duration,
}

impl<E: NodeExecutor> WorkflowEngine<E> {
    /// Build a fresh engine from a spec.
    pub fn new(
        spec: WorkflowSpec,
        executor: Arc<E>,
        workspace: PathBuf,
        run_id: Option<String>,
    ) -> Result<Self, ToolError> {
        // Validate dependency references + cycle freedom up front.
        topo_order(&spec.nodes)
            .ok_or_else(|| ToolError::invalid_input("workflow has a dependency cycle"))?;
        let mut specs_by_id = HashMap::new();
        for n in &spec.nodes {
            if specs_by_id.insert(n.id.clone(), n.clone()).is_some() {
                return Err(ToolError::invalid_input(format!(
                    "duplicate node id '{}'",
                    n.id
                )));
            }
            for dep in &n.depends_on {
                if !spec.nodes.iter().any(|x| &x.id == dep) {
                    return Err(ToolError::invalid_input(format!(
                        "node '{}' depends on unknown node '{}'",
                        n.id, dep
                    )));
                }
            }
        }
        let run_id = run_id.unwrap_or_else(uuid_short);
        let nodes: HashMap<String, NodeRuntime> = spec
            .nodes
            .iter()
            .map(|n| (n.id.clone(), NodeRuntime::new(n.id.clone(), n.retry)))
            .collect();
        let journal = WorkflowJournal {
            run_id: run_id.clone(),
            name: spec.name.clone().unwrap_or_else(|| "workflow".to_string()),
            created_at_ms: WorkflowJournal::now_ms(),
            updated_at_ms: WorkflowJournal::now_ms(),
            nodes,
        };
        let max_parallel = spec.max_parallel.unwrap_or(4).max(1);
        let stall_timeout = Duration::from_millis(spec.stall_timeout_ms.unwrap_or(300_000));
        Ok(Self {
            spec,
            specs_by_id,
            run_id,
            journal,
            executor,
            workspace,
            max_parallel,
            stall_timeout,
        })
    }

    /// Rebuild an engine from a persisted journal (resume). The caller must
    /// then call `inject_spec` with the original DAG shape so scheduling can
    /// proceed (only runtime state is stored in the journal).
    pub fn resume(
        journal: WorkflowJournal,
        executor: Arc<E>,
        workspace: PathBuf,
    ) -> Result<Self, ToolError> {
        let max_parallel = 4usize.max(1);
        let stall_timeout = Duration::from_millis(300_000);
        Ok(Self {
            spec: WorkflowSpec {
                name: Some(journal.name.clone()),
                nodes: Vec::new(),
                max_parallel: None,
                stall_timeout_ms: None,
                resume_run_id: Some(journal.run_id.clone()),
            },
            specs_by_id: HashMap::new(),
            run_id: journal.run_id.clone(),
            journal,
            executor,
            workspace,
            max_parallel,
            stall_timeout,
        })
    }

    /// Overlay the node definitions + run params needed for scheduling. Used by
    /// the resume entry point, which first rebuilds state from the journal and
    /// then attaches the (re-supplied) DAG shape.
    pub fn inject_spec(&mut self, spec: WorkflowSpec) -> Result<(), ToolError> {
        topo_order(&spec.nodes)
            .ok_or_else(|| ToolError::invalid_input("resumed workflow has a dependency cycle"))?;
        let mut specs_by_id = HashMap::new();
        for n in &spec.nodes {
            specs_by_id.insert(n.id.clone(), n.clone());
        }
        // Resume semantics:
        //  - already `Completed` nodes are preserved (skipped on resume);
        //  - `Failed` / `Pending` nodes are re-driven with their retry budget;
        //  - nodes absent from the journal get a fresh runtime.
        for n in &spec.nodes {
            if let Some(rt) = self.journal.nodes.get_mut(&n.id) {
                match rt.state {
                    NodeState::Completed => { /* keep as-is, skip on resume */ }
                    NodeState::Failed | NodeState::Pending => {
                        rt.state = NodeState::Pending;
                        rt.retries_left = n.retry;
                        rt.attempts = 0;
                        rt.agent_id = None;
                        rt.last_progress_at = None;
                        rt.result = None;
                    }
                    NodeState::Running => {
                        // A node left "running" by a crashed run is re-driven.
                        rt.state = NodeState::Pending;
                        rt.retries_left = n.retry;
                        rt.attempts = 0;
                        rt.agent_id = None;
                        rt.last_progress_at = None;
                        rt.result = None;
                    }
                    NodeState::Skipped => { /* keep as-is, already pruned */ }
                }
            } else {
                self.journal
                    .nodes
                    .insert(n.id.clone(), NodeRuntime::new(n.id.clone(), n.retry));
            }
        }
        self.spec = spec;
        self.specs_by_id = specs_by_id;
        self.max_parallel = self.spec.max_parallel.unwrap_or(4).max(1);
        self.stall_timeout = Duration::from_millis(self.spec.stall_timeout_ms.unwrap_or(300_000));
        Ok(())
    }

    /// Persist the journal to disk (best-effort; engine continues on failure).
    fn persist(&self) {
        if let Err(e) = self.journal.save(&self.workspace) {
            tracing::warn!("workflow journal save failed for {}: {e}", self.run_id);
        }
    }

    /// Whether a node's `when` gate is satisfied given current node states.
    fn gate_open(&self, node: &WorkflowNodeSpec) -> bool {
        let Some(gate) = &node.r#when else {
            return true;
        };
        let refs: Vec<&String> = if gate.on.is_empty() {
            node.depends_on.iter().collect()
        } else {
            gate.on.iter().collect()
        };
        for r in refs {
            let Some(rt) = self.journal.nodes.get(r) else {
                return false;
            };
            let status = match rt.state {
                NodeState::Completed => "completed",
                NodeState::Failed => "failed",
                NodeState::Skipped => "skipped",
                NodeState::Running | NodeState::Pending => return false,
            };
            if !gate.expect_status.iter().any(|s| s == status) {
                return false;
            }
        }
        true
    }

    /// A node is ready to launch when all deps are `Completed` and it is still
    /// `Pending` and its `when` gate is open.
    fn is_ready(&self, id: &str) -> bool {
        let Some(node) = self.specs_by_id.get(id) else {
            return false;
        };
        if self.journal.nodes.get(id).map(|r| r.state) != Some(NodeState::Pending) {
            return false;
        }
        for dep in &node.depends_on {
            if self.journal.nodes.get(dep).map(|r| r.state) != Some(NodeState::Completed) {
                return false;
            }
        }
        self.gate_open(node)
    }

    /// Launch a single node (or retry it).
    async fn launch_node(&mut self, id: &str) -> Result<(), ToolError> {
        let node = self
            .specs_by_id
            .get(id)
            .ok_or_else(|| ToolError::execution_failed(format!("unknown node {id}")))?
            .clone();
        let rt = self.journal.nodes.get_mut(id).expect("node runtime exists");
        rt.attempts += 1;
        let agent_id = self
            .executor
            .launch(&node, &self.run_id, rt.attempts.saturating_sub(1))
            .await?;
        rt.agent_id = Some(agent_id);
        rt.state = NodeState::Running;
        rt.last_progress_at = Some(Instant::now());
        self.journal.updated_at_ms = WorkflowJournal::now_ms();
        self.persist();
        Ok(())
    }

    /// Retry a stalled/failed node, if retries remain.
    async fn retry_node(&mut self, id: &str) -> Result<bool, ToolError> {
        let (retries_left, agent_id) = {
            let rt = self.journal.nodes.get(id).expect("node runtime exists");
            (rt.retries_left, rt.agent_id.clone())
        };
        if retries_left == 0 {
            return Ok(false);
        }
        if let Some(aid) = &agent_id {
            self.executor.cancel(aid).await?;
        }
        let rt = self.journal.nodes.get_mut(id).expect("node runtime exists");
        rt.retries_left -= 1;
        rt.state = NodeState::Pending;
        rt.agent_id = None;
        rt.last_progress_at = None;
        self.journal.updated_at_ms = WorkflowJournal::now_ms();
        self.persist();
        Ok(true)
    }

    /// Poll a running node; advance its state and capture the result.
    async fn poll_node(&mut self, id: &str) -> Result<(), ToolError> {
        let agent_id = {
            let rt = self.journal.nodes.get(id).expect("node runtime exists");
            match &rt.agent_id {
                Some(a) => a.clone(),
                None => return Ok(()),
            }
        };
        let result = self.executor.poll(&agent_id).await?;
        let terminal = is_terminal_status(&result.status);
        if terminal {
            let rt = self.journal.nodes.get_mut(id).expect("node runtime exists");
            rt.result = result.result.clone();
            rt.state = if matches!(result.status, SubAgentStatus::Completed) {
                NodeState::Completed
            } else {
                NodeState::Failed
            };
            self.journal.updated_at_ms = WorkflowJournal::now_ms();
            self.persist();
        } else {
            // Stall detection: compare worker-record progress timestamp.
            let last_ms = self.executor.last_progress_ms(&agent_id).await;
            if let Some(ms) = last_ms {
                let now_ms = WorkflowJournal::now_ms();
                let stalled = now_ms.saturating_sub(ms) >= self.stall_timeout.as_millis() as u64;
                if stalled {
                    tracing::warn!("workflow node {id} stalled; retrying");
                    // Treat stall as a retry trigger; if no retries remain, fail.
                    let retried = self.retry_node(id).await?;
                    if !retried {
                        let rt = self.journal.nodes.get_mut(id).expect("node runtime exists");
                        rt.state = NodeState::Failed;
                        rt.result = Some(format!(
                            "stalled > {}ms with no retries left",
                            self.stall_timeout.as_millis()
                        ));
                        self.persist();
                    }
                }
            }
            // Refresh the in-memory progress marker so tests using a synthetic
            // clock can still observe the poll happened.
            if let Some(rt) = self.journal.nodes.get_mut(id) {
                rt.last_progress_at = Some(Instant::now());
            }
        }
        Ok(())
    }

    /// Run the full DAG to completion. Returns a summary of node outcomes.
    pub async fn run(&mut self) -> Result<WorkflowRunReport, ToolError> {
        loop {
            // 1. Skip `when`-gated nodes whose gate will never open (a dep failed).
            for id in self.pending_ids() {
                let node = self.specs_by_id.get(&id).cloned();
                if let Some(node) = node {
                    let will_never_open = {
                        let gate = node.r#when.clone().unwrap_or_else(WhenGate::default);
                        let refs: Vec<String> = if gate.on.is_empty() {
                            node.depends_on.clone()
                        } else {
                            gate.on.clone()
                        };
                        // A `when` gate can never open if any referenced node has
                        // reached a terminal state that is NOT in `expect_status`
                        // (e.g. the gate expects "failed" but the node Completed).
                        refs.iter().any(|r| {
                            let s = self.journal.nodes.get(r).map(|rt| rt.state);
                            let Some(state) = s else {
                                return false;
                            };
                            let has_terminal = matches!(
                                state,
                                NodeState::Completed | NodeState::Failed | NodeState::Skipped
                            );
                            if !has_terminal {
                                return false;
                            }
                            let got = match state {
                                NodeState::Completed => "completed",
                                NodeState::Failed => "failed",
                                NodeState::Skipped => "skipped",
                                _ => unreachable!(),
                            };
                            !gate.expect_status.iter().any(|want| want == got)
                        })
                    };
                    if will_never_open {
                        let rt = self.journal.nodes.get_mut(&id).expect("rt exists");
                        rt.state = NodeState::Skipped;
                        self.journal.updated_at_ms = WorkflowJournal::now_ms();
                        self.persist();
                    }
                }
            }

            // 2. Launch ready nodes up to max_parallel.
            let running_count = self.running_count();
            let mut budget = self.max_parallel.saturating_sub(running_count);
            let ready: Vec<String> = self
                .pending_ids()
                .into_iter()
                .filter(|id| self.is_ready(id))
                .collect();
            for id in ready {
                if budget == 0 {
                    break;
                }
                if let Err(e) = self.launch_node(&id).await {
                    let rt = self.journal.nodes.get_mut(&id).expect("rt exists");
                    rt.state = NodeState::Failed;
                    rt.result = Some(format!("launch failed: {e}"));
                    self.persist();
                } else {
                    budget -= 1;
                }
            }

            // 3. If nothing is running and nothing is pending, we're done.
            if self.running_count() == 0 && self.pending_ids().is_empty() {
                break;
            }

            // 4. Poll running nodes for progress.
            let running: Vec<String> = self.running_ids();
            for id in running {
                if let Err(e) = self.poll_node(&id).await {
                    tracing::warn!("workflow poll error on {id}: {e}");
                    let rt = self.journal.nodes.get_mut(&id).expect("rt exists");
                    rt.state = NodeState::Failed;
                    rt.result = Some(format!("poll error: {e}"));
                }
            }

            // 5. If still nothing progressed and nothing is pending, bail to
            //    avoid a busy loop (all remaining nodes are blocked).
            if self.running_count() == 0 && self.pending_ids().is_empty() {
                break;
            }

            // Cooperative yield between scheduler iterations.
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        Ok(self.report())
    }

    fn pending_ids(&self) -> Vec<String> {
        self.journal
            .nodes
            .iter()
            .filter(|(_, rt)| rt.state == NodeState::Pending)
            .map(|(id, _)| id.clone())
            .collect()
    }

    fn running_ids(&self) -> Vec<String> {
        self.journal
            .nodes
            .iter()
            .filter(|(_, rt)| rt.state == NodeState::Running)
            .map(|(id, _)| id.clone())
            .collect()
    }

    fn running_count(&self) -> usize {
        self.journal
            .nodes
            .values()
            .filter(|rt| rt.state == NodeState::Running)
            .count()
    }

    /// Build the run report.
    fn report(&self) -> WorkflowRunReport {
        let mut nodes = HashMap::new();
        for (id, rt) in &self.journal.nodes {
            nodes.insert(
                id.clone(),
                NodeOutcome {
                    id: id.clone(),
                    state: rt.state,
                    attempts: rt.attempts,
                    result: rt.result.clone(),
                },
            );
        }
        let all_terminal = self.journal.nodes.values().all(|rt| rt.state.is_terminal());
        WorkflowRunReport {
            run_id: self.run_id.clone(),
            name: self.journal.name.clone(),
            finished: all_terminal,
            nodes,
        }
    }

    /// Expose the run id (for journal replay / resume).
    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
}

/// Per-node outcome in the run report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOutcome {
    pub id: String,
    pub state: NodeState,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// Summary returned to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunReport {
    pub run_id: String,
    pub name: String,
    pub finished: bool,
    pub nodes: HashMap<String, NodeOutcome>,
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let s = format!("{ns:x}");
    s[..12.min(s.len())].to_string()
}

/// The `workflow` tool: parse a DAG spec and execute it.
pub struct WorkflowTool {
    manager: SharedSubAgentManager,
    runtime: super::subagent::SubAgentRuntime,
}

impl WorkflowTool {
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: super::subagent::SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowInput {
    /// Either an inline `spec`, or a `resume_run_id` to replay a journal.
    #[serde(default)]
    spec: Option<Value>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    nodes: Option<Value>,
    #[serde(default)]
    max_parallel: Option<usize>,
    #[serde(default)]
    stall_timeout_ms: Option<u64>,
    #[serde(default)]
    resume_run_id: Option<String>,
}

/// Helper to build a minimal `SubAgentResult` for tests / mocks.
fn mk_result(
    agent_id: &str,
    node_id: &str,
    status: SubAgentStatus,
    result: Option<String>,
) -> SubAgentResult {
    SubAgentResult {
        name: node_id.to_string(),
        agent_id: agent_id.to_string(),
        context_mode: "fresh".to_string(),
        fork_context: false,
        workspace: None,
        git_branch: None,
        agent_type: SubAgentType::General,
        assignment: SubAgentAssignment::new("x".to_string(), None),
        model: String::new(),
        nickname: None,
        status,
        worker_status: None,
        parent_run_id: None,
        spawn_depth: 0,
        result,
        steps_taken: 0,
        checkpoint: None,
        needs_input: None,
        duration_ms: 0,
        from_prior_session: false,
    }
}

#[async_trait]
impl ToolSpec for WorkflowTool {
    fn name(&self) -> &'static str {
        WORKFLOW_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Declarative DAG workflow orchestration. Submit a graph of sub-agent nodes with \
         `depends_on` edges; mimofan runs ready nodes in parallel (bounded by max_parallel), \
         sequences dependents after their parents, and prunes branches whose `when` gate is \
         unmet. Each node is a real sub-agent: it inherits the shared token budget and can run \
         in an isolated git worktree (set worktree:true). Stalled nodes are retried up to the \
         node's `retry` count. The run is journaled to .mimofan/workflows/<run_id>.json so you \
         can resume an interrupted run by passing resume_run_id. Returns a per-node outcome map."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Workflow name (journal label)." },
                "spec": {
                    "type": "object",
                    "description": "Inline workflow DAG: { name, nodes:[{id,prompt,type?,depends_on?,worktree?,token_budget?,model?,retry?,when?}], max_parallel?, stall_timeout_ms? }"
                },
                "nodes": {
                    "type": "array",
                    "description": "Convenience: inline nodes array (alternative to wrapping in spec)."
                },
                "max_parallel": { "type": "integer", "description": "Max concurrent nodes (default 4)." },
                "stall_timeout_ms": { "type": "integer", "description": "Stall retry threshold ms (default 300000)." },
                "resume_run_id": { "type": "string", "description": "Replay a prior journaled run id." }
            },
            "required": [],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::RequiresApproval, ToolCapability::Network]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn model_visible(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let parsed: WorkflowInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::invalid_input(format!("invalid workflow input: {e}")))?;

        // Resolve the spec: a resume carries the original DAG shape too.
        let spec = self.resolve_spec(&parsed)?;

        let executor = Arc::new(SubagentNodeExecutor::new(
            self.manager.clone(),
            self.runtime.clone(),
        ));

        let mut engine = if let Some(rid) = &parsed.resume_run_id {
            let journal = WorkflowJournal::load(&context.workspace, rid)
                .map_err(|e| ToolError::execution_failed(format!("journal load failed: {e}")))?
                .ok_or_else(|| {
                    ToolError::invalid_input(format!("no workflow journal for run '{rid}'"))
                })?;
            let mut engine = WorkflowEngine::resume(journal, executor, context.workspace.clone())?;
            engine.inject_spec(spec)?;
            engine
        } else {
            WorkflowEngine::new(spec, executor, context.workspace.clone(), None)?
        };

        let report = engine.run().await?;
        let mut result =
            ToolResult::json(&report).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        result = result.with_metadata(json!({
            "run_id": report.run_id,
            "finished": report.finished,
            "node_count": report.nodes.len(),
        }));
        Ok(result)
    }
}

impl WorkflowTool {
    /// Build a `WorkflowSpec` from the (possibly split) input fields.
    fn resolve_spec(&self, input: &WorkflowInput) -> Result<WorkflowSpec, ToolError> {
        if let Some(spec_val) = &input.spec {
            let spec: WorkflowSpec = serde_json::from_value(spec_val.clone())
                .map_err(|e| ToolError::invalid_input(format!("invalid workflow spec: {e}")))?;
            return Ok(spec);
        }
        if let Some(nodes_val) = &input.nodes {
            let nodes: Vec<WorkflowNodeSpec> = serde_json::from_value(nodes_val.clone())
                .map_err(|e| ToolError::invalid_input(format!("invalid workflow nodes: {e}")))?;
            return Ok(WorkflowSpec {
                name: input.name.clone(),
                nodes,
                max_parallel: input.max_parallel,
                stall_timeout_ms: input.stall_timeout_ms,
                resume_run_id: input.resume_run_id.clone(),
            });
        }
        Err(ToolError::invalid_input(
            "workflow requires either 'spec' or 'nodes'",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Deterministic in-memory executor for scheduling tests.
    struct FakeExecutor {
        states: Arc<Mutex<HashMap<String, FakeAgent>>>,
        seq: Arc<Mutex<u32>>,
        stalled: Arc<Mutex<HashSet<String>>>,
        ran: Arc<Mutex<Vec<String>>>,
        // Per-agent last-progress timestamp on the REAL clock, so the engine's
        // stall detection (which uses the wall clock) observes genuine progress.
        last_progress: Arc<Mutex<HashMap<String, u64>>>,
    }

    #[derive(Clone)]
    struct FakeAgent {
        node_id: String,
        steps_left: u32,
        stalled: bool,
    }

    impl FakeExecutor {
        fn new() -> Self {
            Self {
                states: Arc::new(Mutex::new(HashMap::new())),
                seq: Arc::new(Mutex::new(0)),
                stalled: Arc::new(Mutex::new(HashSet::new())),
                ran: Arc::new(Mutex::new(Vec::new())),
                last_progress: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn mark_stalled(&self, node_id: &str) {
            self.stalled.lock().unwrap().insert(node_id.to_string());
        }

        fn ran_nodes(&self) -> Vec<String> {
            self.ran.lock().unwrap().clone()
        }
    }

    fn real_now_ms() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    #[async_trait]
    impl NodeExecutor for FakeExecutor {
        async fn launch(
            &self,
            node: &WorkflowNodeSpec,
            _run_id: &str,
            attempt: u32,
        ) -> Result<String, ToolError> {
            self.ran.lock().unwrap().push(node.id.clone());
            let mut seq = self.seq.lock().unwrap();
            *seq += 1;
            let agent_id = format!("agent_{}_{}", node.id, attempt);
            // Only the *first* attempt of a node is forced to stall; any retry
            // (attempt >= 1) runs cleanly so the stall→retry→recover path is
            // exercised. This matches the test intent: a stalled node retries
            // and the retry succeeds.
            let stalled = self.stalled.lock().unwrap().contains(&node.id) && attempt == 0;
            self.states.lock().unwrap().insert(
                agent_id.clone(),
                FakeAgent {
                    node_id: node.id.clone(),
                    steps_left: if stalled { u32::MAX } else { 1 },
                    stalled,
                },
            );
            self.last_progress
                .lock()
                .unwrap()
                .insert(agent_id.clone(), real_now_ms());
            Ok(agent_id)
        }

        async fn poll(&self, agent_id: &str) -> Result<SubAgentResult, ToolError> {
            let mut states = self.states.lock().unwrap();
            let agent = states.get_mut(agent_id).expect("agent exists");
            if agent.stalled {
                return Ok(mk_result(
                    agent_id,
                    &agent.node_id,
                    SubAgentStatus::Running,
                    None,
                ));
            }
            if agent.steps_left == 0 {
                self.last_progress
                    .lock()
                    .unwrap()
                    .insert(agent_id.to_string(), real_now_ms());
                return Ok(mk_result(
                    agent_id,
                    &agent.node_id,
                    SubAgentStatus::Completed,
                    Some(format!("done:{}", agent.node_id)),
                ));
            }
            agent.steps_left -= 1;
            // A running poll also counts as progress.
            self.last_progress
                .lock()
                .unwrap()
                .insert(agent_id.to_string(), real_now_ms());
            Ok(mk_result(
                agent_id,
                &agent.node_id,
                SubAgentStatus::Running,
                None,
            ))
        }

        async fn cancel(&self, _agent_id: &str) -> Result<(), ToolError> {
            Ok(())
        }

        async fn last_progress_ms(&self, agent_id: &str) -> Option<u64> {
            self.last_progress.lock().unwrap().get(agent_id).copied()
        }
    }

    fn node(id: &str, deps: &[&str], retry: u32) -> WorkflowNodeSpec {
        WorkflowNodeSpec {
            id: id.to_string(),
            r#type: Some("general".to_string()),
            prompt: format!("do {id}"),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            name: None,
            worktree: false,
            token_budget: None,
            model: None,
            retry,
            r#when: None,
        }
    }

    fn sample_spec() -> WorkflowSpec {
        WorkflowSpec {
            name: Some("demo".into()),
            nodes: vec![
                node("a", &[], 0),
                node("b", &["a"], 0),
                node("c", &["a"], 0),
                WorkflowNodeSpec {
                    id: "d".into(),
                    r#type: Some("verifier".into()),
                    prompt: "do d".into(),
                    depends_on: vec!["b".into(), "c".into()],
                    name: None,
                    worktree: true,
                    token_budget: Some(10_000),
                    model: None,
                    retry: 2,
                    r#when: None,
                },
            ],
            max_parallel: Some(2),
            stall_timeout_ms: Some(100),
            resume_run_id: None,
        }
    }

    fn tmp_ws(tag: &str) -> PathBuf {
        let ws = std::env::temp_dir().join(format!("wf_test_{}_{}", tag, uuid_short()));
        let _ = std::fs::remove_dir_all(&ws);
        std::fs::create_dir_all(&ws).unwrap();
        ws
    }

    #[tokio::test]
    async fn sequential_and_parallel_dag_completes() {
        let exec = Arc::new(FakeExecutor::new());
        let ws = tmp_ws("seq");
        let mut engine =
            WorkflowEngine::new(sample_spec(), exec.clone(), ws.clone(), Some("run1".into()))
                .unwrap();
        let report = engine.run().await.unwrap();

        assert!(report.finished, "all nodes should reach a terminal state");
        for id in ["a", "b", "c", "d"] {
            assert_eq!(
                report.nodes.get(id).unwrap().state,
                NodeState::Completed,
                "node {id} should complete"
            );
        }
        // Dependency order: a before b/c; b & c before d.
        let ran = exec.ran_nodes();
        let pos = |id: &str| ran.iter().position(|x| x == id).unwrap();
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));

        assert!(ws.join(WORKFLOW_JOURNAL_DIR).join("run1.json").exists());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[tokio::test]
    async fn stall_triggers_retry_then_completes() {
        let exec = Arc::new(FakeExecutor::new());
        let ws = tmp_ws("stall");

        let spec = WorkflowSpec {
            name: Some("stall".into()),
            nodes: vec![node("x", &[], 1)],
            max_parallel: Some(1),
            // Generous headroom: a *retry* agent makes real progress and is
            // polled roughly every 20ms, so the timeout must exceed that cadence
            // to avoid re-stalling a healthy retry. The *first* agent never
            // updates its progress timestamp, so it is still correctly flagged
            // stalled well within this window.
            stall_timeout_ms: Some(200),
            resume_run_id: None,
        };
        let mut engine =
            WorkflowEngine::new(spec, exec.clone(), ws.clone(), Some("run2".into())).unwrap();
        // Mark node x's agents as stalled. The engine will see no progress and
        // retry; the retry attempt is a fresh agent id (attempt index 1) which
        // is NOT in the stalled set, so it completes.
        exec.mark_stalled("x");
        let report = engine.run().await.unwrap();
        let outcome = report.nodes.get("x").unwrap();
        assert_eq!(outcome.state, NodeState::Completed, "retry should recover");
        assert!(
            outcome.attempts >= 2,
            "should have retried, got {}",
            outcome.attempts
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[tokio::test]
    async fn conditional_when_gate_prunes_branch() {
        let exec = Arc::new(FakeExecutor::new());
        let ws = tmp_ws("cond");
        let spec = WorkflowSpec {
            name: Some("cond".into()),
            nodes: vec![
                node("root", &[], 0),
                WorkflowNodeSpec {
                    id: "skipme".into(),
                    r#type: Some("general".into()),
                    prompt: "skip".into(),
                    depends_on: vec!["root".into()],
                    name: None,
                    worktree: false,
                    token_budget: None,
                    model: None,
                    retry: 0,
                    r#when: Some(WhenGate {
                        on: vec!["root".into()],
                        expect_status: vec!["failed".into()],
                    }),
                },
                node("always", &["root"], 0),
            ],
            max_parallel: Some(4),
            stall_timeout_ms: Some(100),
            resume_run_id: None,
        };
        let mut engine =
            WorkflowEngine::new(spec, exec.clone(), ws.clone(), Some("run3".into())).unwrap();
        let report = engine.run().await.unwrap();
        assert_eq!(
            report.nodes.get("root").unwrap().state,
            NodeState::Completed
        );
        assert_eq!(
            report.nodes.get("skipme").unwrap().state,
            NodeState::Skipped,
            "branch should be pruned by when gate"
        );
        assert_eq!(
            report.nodes.get("always").unwrap().state,
            NodeState::Completed
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn cycle_is_rejected() {
        let spec = WorkflowSpec {
            name: None,
            nodes: vec![node("x", &["y"], 0), node("y", &["x"], 0)],
            max_parallel: None,
            stall_timeout_ms: None,
            resume_run_id: None,
        };
        let exec = Arc::new(FakeExecutor::new());
        let ws = std::env::temp_dir().join("wf_cycle");
        let err = WorkflowEngine::new(spec, exec, ws, Some("cyc".into()));
        assert!(err.is_err(), "dependency cycle must be rejected");
    }

    #[tokio::test]
    async fn journal_resume_skips_completed() {
        // Phase 1: a and b complete; c stalls forever and exhausts its retry
        // budget (retry=0) -> Failed. Phase 2: resume with a fresh (non-stalled)
        // executor; inject_spec re-seeds c as Pending and it completes.
        let ws = tmp_ws("resume");

        let exec1 = Arc::new(FakeExecutor::new());
        exec1.mark_stalled("c");
        let spec1 = WorkflowSpec {
            name: Some("r".into()),
            nodes: vec![
                node("a", &[], 0),
                node("b", &["a"], 0),
                node("c", &["b"], 0),
            ],
            max_parallel: Some(1),
            stall_timeout_ms: Some(10),
            resume_run_id: None,
        };
        let mut engine1 =
            WorkflowEngine::new(spec1, exec1.clone(), ws.clone(), Some("runR".into())).unwrap();
        let _ = engine1.run().await.unwrap();
        let j = WorkflowJournal::load(&ws, "runR").unwrap().unwrap();
        assert_eq!(j.nodes.get("a").unwrap().state, NodeState::Completed);
        assert_eq!(j.nodes.get("b").unwrap().state, NodeState::Completed);
        assert_eq!(j.nodes.get("c").unwrap().state, NodeState::Failed);

        let exec2 = Arc::new(FakeExecutor::new());
        let spec2 = WorkflowSpec {
            name: Some("r".into()),
            nodes: vec![
                node("a", &[], 0),
                node("b", &["a"], 0),
                node("c", &["b"], 0),
            ],
            max_parallel: Some(1),
            stall_timeout_ms: Some(10),
            resume_run_id: Some("runR".into()),
        };
        let journal = WorkflowJournal::load(&ws, "runR").unwrap().unwrap();
        let mut engine2 = WorkflowEngine::resume(journal, exec2.clone(), ws.clone()).unwrap();
        engine2.inject_spec(spec2).unwrap();
        let report = engine2.run().await.unwrap();
        assert_eq!(report.nodes.get("a").unwrap().state, NodeState::Completed);
        assert_eq!(report.nodes.get("b").unwrap().state, NodeState::Completed);
        assert_eq!(
            report.nodes.get("c").unwrap().state,
            NodeState::Completed,
            "resume should re-drive c"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }
}
