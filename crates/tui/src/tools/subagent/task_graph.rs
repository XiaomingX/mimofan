//! DAG-based sub-agent orchestration tool (`run_task_graph`).
//!
//! Wires the previously-unused [`super::decomposer`] algorithms into the live
//! sub-agent runtime: the model submits a list of tasks with `depends_on`
//! edges, the runner validates the graph (cycle / duplicate / missing-dep
//! checks), then executes it wave-by-wave. Independent nodes within a wave run
//! concurrently; a wave only starts after the previous one fully completes.
//! If a node fails, its downstream dependents are skipped (failure
//! propagation), mirroring nac's orchestrator behavior.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::decomposer::{TaskDecomposer, TaskGraph, TaskNodeStatus};
use super::tool::spawn_subagent_from_input;
use super::{
    SharedSubAgentManager, SubAgentCompletion, SubAgentRuntime, SubAgentStatus, SubAgentType,
};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Maximum times a single node's spawn is retried when the manager rejects it
/// for admission capacity (large DAGs routinely hit `max_admitted_agents`).
const NODE_SPAWN_MAX_RETRIES: u32 = 4;

/// A single task node supplied by the model in the `run_task_graph` call.
#[derive(Debug, Clone)]
struct GraphTaskArg {
    id: String,
    description: String,
    agent_type: SubAgentType,
    depends_on: Vec<String>,
    worktree: bool,
    prompt_override: Option<String>,
}

/// Model-facing tool that runs a dependency graph of sub-agent tasks.
pub struct TaskGraphTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
}

impl TaskGraphTool {
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for TaskGraphTool {
    fn name(&self) -> &'static str {
        "run_task_graph"
    }

    fn description(&self) -> &'static str {
        concat!(
            "Run a dependency graph of focused sub-agent tasks as one coordinated batch. ",
            "Pass `tasks`, each with a unique `id`, a `description`, an `agent_type` ",
            "(general|implementer|explore|plan|review|verifier), and an optional `depends_on` ",
            "list of other task ids. mimofan topologically sorts the graph and executes it in ",
            "waves: tasks with no unsatisfied dependencies run concurrently, and each later wave ",
            "starts only after the previous wave fully completes. If a task fails, every task that ",
            "depends on it (directly or transitively) is skipped automatically. ",
            "Prefer this over many separate `agent` calls when the work has clear dependencies ",
            "(e.g. 'implement A and B, then integrate them'). Set `worktree: true` on edit tasks ",
            "that must not collide. Returns a per-task status summary (completed / failed / skipped)."
        )
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "Ordered list of tasks forming the dependency graph.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Unique identifier for this task within the graph. Referenced by other tasks' depends_on."
                            },
                            "description": {
                                "type": "string",
                                "description": "What this task should accomplish. Used as the child agent prompt unless prompt_override is given."
                            },
                            "agent_type": {
                                "type": "string",
                                "description": "Sub-agent role: general, implementer, explore, plan, review, or verifier."
                            },
                            "depends_on": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Task ids that must complete before this task starts. Empty or omitted for roots."
                            },
                            "worktree": {
                                "type": "boolean",
                                "description": "When true, run this task in an isolated git worktree so parallel edits do not collide. Use for edit/implement tasks."
                            },
                            "prompt_override": {
                                "type": "string",
                                "description": "Optional full prompt overriding `description` for the child agent."
                            }
                        },
                        "required": ["id", "description", "agent_type"]
                    }
                },
                "fail_fast": {
                    "type": "boolean",
                    "description": "Reserved. Downstream skipping on failure is always on; this field is accepted for forward compatibility."
                }
            },
            "required": ["tasks"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let tasks = parse_tasks(&input)?;
        if tasks.is_empty() {
            return Err(ToolError::invalid_input(
                "run_task_graph requires at least one task in `tasks`.",
            ));
        }

        // Build + validate the graph (cycle / duplicate-id / missing-dep checks).
        let decompose_tasks = tasks
            .iter()
            .map(|t| {
                (
                    t.id.clone(),
                    t.description.clone(),
                    t.agent_type.clone(),
                    t.depends_on.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut graph = TaskDecomposer::new()
            .decompose(decompose_tasks)
            .map_err(|e| ToolError::invalid_input(format!("Invalid task graph: {e}")))?;

        let groups = graph
            .parallel_groups()
            .map_err(|e| ToolError::invalid_input(format!("Failed to schedule graph: {e}")))?;

        // Execute wave by wave.
        for group in &groups {
            run_wave(
                &self.manager,
                &self.runtime,
                &mut graph,
                group,
                &tasks,
            )
            .await?;
        }

        Ok(build_result(&graph))
    }
}

/// Parse the model-supplied `tasks` array, defaulting missing fields.
fn parse_tasks(input: &Value) -> Result<Vec<GraphTaskArg>, ToolError> {
    let Some(tasks) = input.get("tasks").and_then(|v| v.as_array()) else {
        return Err(ToolError::invalid_input(
            "run_task_graph requires a `tasks` array.",
        ));
    };
    let mut out = Vec::with_capacity(tasks.len());
    for (i, item) in tasks.iter().enumerate() {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::invalid_input(format!("task[{i}] is missing a non-empty `id`")))?
            .to_string();
        let description = item
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolError::invalid_input(format!("task `{id}` is missing a non-empty `description`"))
            })?
            .to_string();
        let type_str = item
            .get("agent_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolError::invalid_input(format!("task `{id}` is missing `agent_type`"))
            })?;
        let agent_type = SubAgentType::from_str(type_str).ok_or_else(|| {
            ToolError::invalid_input(format!(
                "task `{id}` has unknown agent_type `{type_str}` (use general|implementer|explore|plan|review|verifier)"
            ))
        })?;
        let depends_on = item
            .get("depends_on")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let worktree = item
            .get("worktree")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let prompt_override = item
            .get("prompt_override")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        out.push(GraphTaskArg {
            id,
            description,
            agent_type,
            depends_on,
            worktree,
            prompt_override,
        });
    }
    Ok(out)
}

/// Build the `agent` tool input value for one node, reusing the exact shape
/// `spawn_subagent_from_input` already parses (prompt / type / name / worktree).
fn spawn_value_for(node: &GraphTaskArg) -> Value {
    let mut value = json!({
        "prompt": node.prompt_override.clone().unwrap_or_else(|| node.description.clone()),
        "type": node.agent_type.as_str(),
        "name": format!("task-{}", node.id),
    });
    if node.worktree {
        value["worktree"] = json!(true);
    }
    value
}

/// Run a single wave: spawn all nodes concurrently (each with its own
/// completion channel), then await every node's terminal state via the
/// `parent_completion_tx` channel. On any failure, skip downstream dependents.
async fn run_wave(
    manager: &SharedSubAgentManager,
    runtime: &SubAgentRuntime,
    graph: &mut TaskGraph,
    group: &[String],
    tasks: &[GraphTaskArg],
) -> Result<(), ToolError> {
    // Skip nodes already marked Skipped by an earlier wave's failure.
    let to_run: Vec<String> = group
        .iter()
        .filter(|id| {
            graph
                .nodes
                .get(*id)
                .map(|n| n.status == TaskNodeStatus::Pending)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    if to_run.is_empty() {
        return Ok(());
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<SubAgentCompletion>();

    // Map agent_id -> node id. The child's session_name is `task-<id>`, and the
    // completion envelope carries the agent_id the manager assigned.
    let mut node_for_name: HashMap<String, String> = HashMap::new();
    let mut spawned_nodes: Vec<String> = Vec::new();

    for node_id in &to_run {
        let node = match tasks.iter().find(|t| t.id == *node_id) {
            Some(n) => n,
            None => continue,
        };
        let child_runtime = runtime
            .child_runtime()
            .with_parent_completion_tx(tx.clone());
        let value = spawn_value_for(node);

        // Retry past admission-capacity rejections instead of failing the wave.
        let mut attempt = 0u32;
        let mut last_err: Option<ToolError> = None;
        loop {
            match spawn_subagent_from_input(value.clone(), manager.clone(), child_runtime.clone())
                .await
            {
                Ok(snapshot) => {
                    node_for_name.insert(snapshot.agent_id.clone(), node_id.clone());
                    spawned_nodes.push(node_id.clone());
                    let _ = graph.start_node(node_id);
                    break;
                }
                Err(e) => {
                    attempt += 1;
                    if attempt >= NODE_SPAWN_MAX_RETRIES {
                        last_err = Some(e);
                        break;
                    }
                    let backoff = Duration::from_millis(250) * (1u32 << attempt.saturating_sub(1));
                    tokio::time::sleep(backoff).await;
                }
            }
        }
        if let Some(e) = last_err {
            // This node could not be spawned — treat as failed and skip downstream.
            let _ = graph.fail_node(node_id);
            let skipped = graph.skip_downstream(node_id);
            return Err(ToolError::execution_failed(format!(
                "task `{}` could not be spawned after {} retries: {}. Downstream tasks skipped: {:?}",
                node_id, NODE_SPAWN_MAX_RETRIES, e, skipped
            )));
        }
    }

    // Await completions for this wave.
    let total = spawned_nodes.len();
    let mut completed = 0usize;
    while completed < total {
        match rx.recv().await {
            Some(completion) => {
                if let Some(node_id) = node_for_name.get(&completion.agent_id) {
                    // The completion payload is the human summary + sentinel;
                    // the actual terminal status lives in the manager.
                    let status = {
                        let guard = manager.read().await;
                        guard
                            .get_result_by_ref(&completion.agent_id)
                            .map(|r| r.status)
                            .unwrap_or(SubAgentStatus::Running)
                    };
                    match status {
                        SubAgentStatus::Failed(_) | SubAgentStatus::Cancelled => {
                            let _ = graph.fail_node(node_id);
                            graph.skip_downstream(node_id);
                        }
                        SubAgentStatus::Completed => {
                            let _ = graph.complete_node(node_id, None);
                        }
                        _ => {
                            // Interrupted / budget-exhausted: treat as a failure
                            // for propagation purposes.
                            let _ = graph.fail_node(node_id);
                            graph.skip_downstream(node_id);
                        }
                    }
                    completed += 1;
                }
                // Completions for nodes outside this wave are ignored here; the
                // child runtime only routes this wave's children to `tx`, so in
                // practice every envelope belongs to this wave.
            }
            None => break, // Channel closed: no more completions incoming.
        }
    }

    Ok(())
}

/// Render the final per-task summary as a tool result.
fn build_result(graph: &TaskGraph) -> ToolResult {
    let mut completed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut running = 0usize;
    let mut nodes_json = Vec::with_capacity(graph.nodes.len());
    // Preserve insertion order for stable output.
    let mut ordered: Vec<&String> = graph.nodes.keys().collect();
    ordered.sort();
    for id in ordered {
        let node = &graph.nodes[id];
        let (status_str, result) = match node.status {
            TaskNodeStatus::Completed => {
                completed += 1;
                ("completed", node.result.clone())
            }
            TaskNodeStatus::Failed => {
                failed += 1;
                ("failed", None)
            }
            TaskNodeStatus::Skipped => {
                skipped += 1;
                ("skipped", None)
            }
            TaskNodeStatus::Running => {
                running += 1;
                ("running", None)
            }
            TaskNodeStatus::Pending => {
                skipped += 1;
                ("skipped", None)
            }
            TaskNodeStatus::Cancelled => {
                skipped += 1;
                ("skipped", None)
            }
        };
        let mut entry = json!({ "id": id, "status": status_str });
        if let Some(r) = result {
            entry["result"] = json!(r);
        }
        nodes_json.push(entry);
    }

    let summary = format!(
        "Task graph finished: {completed} completed, {failed} failed, {skipped} skipped, {running} running."
    );
    let payload = json!({
        "summary": summary,
        "completed": completed,
        "failed": failed,
        "skipped": skipped,
        "running": running,
        "tasks": nodes_json,
    });
    match ToolResult::json(&payload) {
        Ok(mut tr) => {
            tr.metadata = Some(json!({ "summary": summary }));
            tr
        }
        Err(_) => ToolResult::success(summary),
    }
}
