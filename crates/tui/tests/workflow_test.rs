//! `workflow` 引擎单元测试（DAG 调度 / stall 重试 / when 门 / journal 续跑）。
//!
//! 从 `crates/tui/src/tools/workflow.rs` 的内联 `#[cfg(test)] mod tests` 迁出；
//! `WORKFLOW_JOURNAL_DIR` / `uuid_short` / `mk_result` / `WorkflowJournal::load`
//! 已在本模块中 `pub` 暴露，供集成测试直接引用。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mimofan::tools::spec::ToolError;
use mimofan::tools::subagent::{SubAgentResult, SubAgentStatus};
use mimofan::tools::workflow::{
    NodeExecutor, NodeState, WhenGate, WorkflowEngine, WorkflowJournal, WorkflowNodeSpec,
    WorkflowSpec, WORKFLOW_JOURNAL_DIR, mk_result, uuid_short,
};

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
