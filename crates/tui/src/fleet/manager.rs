//! Local-first fleet manager loop and operator controls.
//!
//! This module is intentionally ledger-first: the first manager can run in the
//! foreground and coordinate logical local workers while later host adapters
//! add real process and SSH execution behind the same records.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use mimofan_protocol::fleet::*;
use serde_json::Value;
use uuid::Uuid;

use super::executor::{
    FleetExecutor, FleetWorkerTerminalEvent, build_worker_exec_command_with_profiles,
};
use super::host::FleetHostErrorKind;
use super::ledger::{FleetLedger, FleetLedgerState, FleetTaskLedgerStatus, FleetTaskState};
use super::scheduler::{FleetScheduler, FleetSchedulerPolicy};
use super::task_spec::{
    FleetTaskSpecDocument, FleetTaskVerificationInput, load_task_spec_document,
    record_verification_receipt, validate_task_spec_document, verify_task_result,
};
use super::worker_runtime;
use crate::tools::subagent::SharedSubAgentManager;

const DEFAULT_STALE_AFTER_SECONDS: u64 = 300;

pub struct FleetManager {
    workspace: PathBuf,
    ledger: FleetLedger,
    stale_after: Duration,
    exec_config: mimofan_config::FleetExecConfig,
    /// Optional sub-agent manager for headless worker execution.
    /// When set, fleet workers spawn real sub-agents; when None,
    /// the manager falls back to local simulation.
    sub_agent_manager: Option<SharedSubAgentManager>,
}

impl std::fmt::Debug for FleetManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FleetManager")
            .field("workspace", &self.workspace)
            .field("ledger", &self.ledger)
            .field("stale_after", &self.stale_after)
            .field("exec_config", &self.exec_config)
            .field(
                "sub_agent_manager",
                &self
                    .sub_agent_manager
                    .as_ref()
                    .map(|_| "SharedSubAgentManager"),
            )
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct FleetRunReport {
    pub run_id: FleetRunId,
    pub task_count: usize,
    pub leased: usize,
    pub queued: usize,
    pub worker_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FleetTickReport {
    pub leased: usize,
    pub heartbeats: usize,
}

#[derive(Debug, Clone, Default)]
pub struct FleetExecutorTickReport {
    pub started: usize,
    pub events: usize,
    pub terminals: usize,
}

#[derive(Debug, Clone, Default)]
pub struct FleetStatusSnapshot {
    pub runs: usize,
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub partial: usize,
    pub failed: usize,
    pub restarted: usize,
    pub escalated: usize,
    pub transport_failed: usize,
    pub task_failed: usize,
    pub verifier_failed: usize,
    pub cancelled: usize,
    pub stale: usize,
    pub workers: BTreeMap<String, FleetWorkerStatus>,
}

/// Outcome of resuming a fleet run from durable ledger state after a manager
/// restart. The counts reflect the reconciliation pass; `status` is the
/// post-resume inspectable snapshot.
#[derive(Debug, Clone)]
pub struct FleetResumeReport {
    pub run_id: FleetRunId,
    /// Orphaned in-flight leases detected as stale and reclaimed.
    pub reclaimed_stale: usize,
    /// Stale leases retried within their retry budget.
    pub restarted: usize,
    /// Stale leases that exhausted their retry budget and were failed.
    pub failed: usize,
    /// Escalation alerts emitted for exhausted tasks.
    pub escalated: usize,
    /// Inspectable run status after the resume pass.
    pub status: FleetStatusSnapshot,
}

#[derive(Debug, Clone)]
pub struct FleetWorkerInspection {
    pub worker_id: String,
    pub status: FleetWorkerStatus,
    pub current_run_id: Option<FleetRunId>,
    pub current_task_id: Option<String>,
    pub objective: Option<String>,
    pub role: Option<String>,
    pub host: Option<String>,
    pub latest_heartbeat_at: Option<String>,
    pub latest_event: Option<FleetWorkerEvent>,
    pub artifacts: Vec<FleetArtifactRef>,
    pub receipt_summary: Option<String>,
    pub last_error: Option<String>,
    pub alert_state: Option<String>,
    /// Lightweight projection from the sub-agent worker runtime.
    /// Populated when a sub-agent manager is attached.
    pub runtime_state: Option<FleetWorkerRuntimeProjection>,
}

/// Lightweight TUI projection of a headless sub-agent worker's current state.
///
/// Derived from the sub-agent manager's `AgentWorkerRecord`.
#[derive(Debug, Clone)]
pub struct FleetWorkerRuntimeProjection {
    /// Sub-agent lifecycle status (Queued, Starting, Running, Completed, etc.)
    pub agent_status: String,
    /// Steps taken so far (tool calls + model turns)
    pub steps_taken: u32,
    /// Latest human-readable message from the worker
    pub latest_message: Option<String>,
    /// Error message if the worker failed
    pub error: Option<String>,
    /// Result summary if the worker completed
    pub result_summary: Option<String>,
    /// Whether the worker has a sub-agent session running
    pub has_session: bool,
}

#[derive(Debug, Clone)]
struct FleetExecutorTaskContext {
    entry: FleetInboxEntry,
    task_spec: FleetTaskSpec,
    worker_id: String,
}

impl FleetManager {
    pub fn open(workspace: impl AsRef<Path>) -> Result<Self> {
        let workspace = workspace.as_ref().to_path_buf();
        let ledger = FleetLedger::open(&workspace)?;
        Ok(Self {
            workspace,
            ledger,
            stale_after: Duration::from_secs(DEFAULT_STALE_AFTER_SECONDS),
            exec_config: mimofan_config::FleetExecConfig::default(),
            sub_agent_manager: None,
        })
    }

    pub fn with_stale_after(mut self, stale_after: Duration) -> Self {
        self.stale_after = stale_after;
        self
    }

    /// Apply fleet headless-worker execution policy from config.
    pub fn with_exec_config(mut self, exec_config: mimofan_config::FleetExecConfig) -> Self {
        self.exec_config = exec_config;
        self
    }

    /// Attach a sub-agent manager so fleet workers can spawn real headless agents.
    pub fn with_sub_agent_manager(mut self, mgr: SharedSubAgentManager) -> Self {
        self.sub_agent_manager = Some(mgr);
        self
    }

    /// True when the manager has a sub-agent runtime for headless worker execution.
    pub fn has_worker_runtime(&self) -> bool {
        self.sub_agent_manager.is_some()
    }

    pub fn ledger_path(&self) -> &Path {
        self.ledger.path()
    }

    pub fn rebuild_state(&self) -> Result<FleetLedgerState> {
        self.ledger.rebuild_state()
    }

    pub fn load_task_spec(path: &Path) -> Result<FleetTaskSpecDocument> {
        load_task_spec_document(path)
    }

    pub fn create_run_from_task_spec_path(
        &self,
        path: &Path,
        max_workers: usize,
    ) -> Result<FleetRunReport> {
        let doc = Self::load_task_spec(path)?;
        self.create_run(doc, max_workers)
    }

    pub fn create_run(
        &self,
        mut doc: FleetTaskSpecDocument,
        max_workers: usize,
    ) -> Result<FleetRunReport> {
        validate_task_spec_document(&doc)?;
        let agent_profiles = super::profile::load_workspace_agent_profiles(&self.workspace)?;
        worker_runtime::validate_task_agent_profiles(&doc.tasks, &agent_profiles)?;
        let max_workers = max_workers.clamp(1, 128);
        let run_id = FleetRunId::from(format!(
            "fleet-{}",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        let now = timestamp();
        if doc.workers.is_empty() {
            doc.workers = default_local_workers(&run_id, max_workers);
        }
        let run = FleetRun {
            id: run_id.clone(),
            name: doc.name.unwrap_or_else(|| run_id.0.clone()),
            status: FleetRunStatus::Queued,
            task_specs: doc.tasks.clone(),
            worker_specs: doc.workers.clone(),
            labels: doc.labels,
            security_policy: doc.security_policy.clone(),
            created_at: now.clone(),
            updated_at: Some(now.clone()),
            completed_at: None,
        };
        self.ledger.create_run(&run)?;
        for task in &run.task_specs {
            self.ledger.enqueue(FleetInboxEntry {
                run_id: run.id.clone(),
                task_id: task.id.clone(),
                priority: task_priority(task),
                enqueued_at: now.clone(),
                lease_deadline: None,
                attempts: 0,
            })?;
        }
        let initial_status = if run.task_specs.is_empty() {
            FleetRunStatus::Completed
        } else {
            FleetRunStatus::Running
        };
        self.ledger
            .update_run_status(&run.id, initial_status, &timestamp())?;
        let tick = self.schedule_run(&run.id, max_workers)?;
        self.refresh_run_status(&run.id)?;
        let state = self.ledger.rebuild_state()?;
        let snapshot = self.status_from_state(Some(&run.id), &state);
        Ok(FleetRunReport {
            run_id: run.id,
            task_count: run.task_specs.len(),
            leased: tick.leased,
            queued: snapshot.queued,
            worker_ids: run.worker_specs.iter().map(|w| w.id.clone()).collect(),
        })
    }

    pub fn schedule_run(&self, run_id: &FleetRunId, max_workers: usize) -> Result<FleetTickReport> {
        let max_workers = max_workers.clamp(1, 128);
        let mut report = FleetTickReport::default();
        let state = self.ledger.rebuild_state()?;
        let run = state
            .runs
            .get(&run_id.0)
            .cloned()
            .ok_or_else(|| anyhow!("fleet run {} does not exist", run_id.0))?;
        let worker_ids = worker_ids_for_run(&run, max_workers);

        for task in active_tasks_for_run(&state, run_id) {
            if let Some(worker_id) = task.leased_to.as_deref()
                && worker_ids.iter().any(|id| id == worker_id)
            {
                self.ledger.heartbeat(worker_id, &timestamp(), None, None)?;
                report.heartbeats += 1;
            }
        }

        loop {
            let state = self.ledger.rebuild_state()?;
            let active_workers = active_workers_for_run(&state, run_id);
            if active_workers.len() >= max_workers {
                break;
            }
            let Some(worker_id) = worker_ids
                .iter()
                .find(|id| !active_workers.contains(*id))
                .cloned()
            else {
                break;
            };
            let Some((entry, task_spec)) = next_enqueued_task_for_run(&state, run_id) else {
                break;
            };
            self.start_worker_task(&worker_id, &entry, &task_spec)?;
            report.leased += 1;
        }

        self.refresh_run_status(run_id)?;
        Ok(report)
    }

    pub fn status(&self) -> Result<FleetStatusSnapshot> {
        let state = self.ledger.rebuild_state()?;
        Ok(self.status_from_state(None, &state))
    }

    pub fn run_status(&self, run_id: &FleetRunId) -> Result<FleetStatusSnapshot> {
        let state = self.ledger.rebuild_state()?;
        Ok(self.status_from_state(Some(run_id), &state))
    }

    pub fn run_has_open_work(&self, run_id: &FleetRunId) -> Result<bool> {
        let status = self.run_status(run_id)?;
        Ok(status.queued + status.running + status.stale > 0)
    }

    /// Resume a run from durable ledger state after a manager restart.
    ///
    /// A crashed or detached manager can leave in-flight tasks `Leased` to
    /// workers whose processes are gone. Resume rebuilds run state from the
    /// ledger, reconciles those orphaned/stale leases through the shared
    /// scheduler recovery semantics (retry within budget, else fail and
    /// escalate), records every decision durably, and returns an inspectable
    /// status. It launches no new work and does not re-process tasks that
    /// already reached a terminal state, so it is safe to call repeatedly.
    pub fn resume_run(&self, run_id: &FleetRunId) -> Result<FleetResumeReport> {
        self.resume_run_at(run_id, Utc::now())
    }

    /// Resume reconciliation at an explicit instant. This is the deterministic
    /// seam behind [`resume_run`]'s wall clock: stale detection compares the
    /// last heartbeat against `now`.
    pub(crate) fn resume_run_at(
        &self,
        run_id: &FleetRunId,
        now: DateTime<Utc>,
    ) -> Result<FleetResumeReport> {
        // Reuse the shared scheduler recovery engine over the same ledger so
        // resume and steady-state supervision converge on one store and one
        // retry/escalation policy. The manager's `stale_after` becomes the
        // scheduler's heartbeat timeout so both surfaces agree on staleness.
        let policy = FleetSchedulerPolicy {
            heartbeat_timeout: self.stale_after,
            ..FleetSchedulerPolicy::default()
        };
        let mut scheduler = FleetScheduler::open(&self.workspace, policy)?;
        scheduler.set_now(now);
        let report = scheduler.resume_run(run_id)?;
        let status = self.run_status(run_id)?;
        Ok(FleetResumeReport {
            run_id: run_id.clone(),
            reclaimed_stale: report.marked_stale,
            restarted: report.restarted,
            failed: report.failed,
            escalated: report.alerts,
            status,
        })
    }

    pub async fn run_to_completion(
        &self,
        run_id: &FleetRunId,
        max_workers: usize,
        executor: &mut FleetExecutor,
        mimofan_binary: &str,
        model: Option<&str>,
        tick_interval: Duration,
    ) -> Result<FleetStatusSnapshot> {
        let max_workers = max_workers.clamp(1, 128);
        loop {
            self.schedule_run(run_id, max_workers)?;
            self.drive_executor_tick(run_id, executor, mimofan_binary, model)?;
            self.refresh_run_status(run_id)?;
            if !self.run_has_open_work(run_id)? {
                return self.run_status(run_id);
            }
            tokio::time::sleep(tick_interval).await;
        }
    }

    pub fn drive_executor_tick(
        &self,
        run_id: &FleetRunId,
        executor: &mut FleetExecutor,
        mimofan_binary: &str,
        model: Option<&str>,
    ) -> Result<FleetExecutorTickReport> {
        let mut report = FleetExecutorTickReport::default();
        report.started += self.start_leased_workers(run_id, executor, mimofan_binary, model)?;

        for worker_id in executor.worker_ids() {
            for payload in executor.drain_events(&worker_id) {
                // The subprocess exit is the task-completion authority. Stream
                // `done` / `error` lines are useful progress signals, but
                // appending them as terminal ledger events before the process
                // exits would free the logical worker too early.
                if is_terminal_payload(&payload) {
                    continue;
                }
                let Some(task) = self.executor_task_context(&worker_id)? else {
                    continue;
                };
                self.append_worker_event(
                    &task.entry.run_id,
                    &worker_id,
                    &task.entry.task_id,
                    payload,
                )?;
                self.ledger
                    .heartbeat(&worker_id, &timestamp(), None, None)?;
                report.events += 1;
            }

            if let Some(terminal) = executor.poll_terminal_with_status(&worker_id) {
                let Some(task) = self.executor_task_context(&worker_id)? else {
                    executor.forget_worker(&worker_id);
                    continue;
                };
                if self.record_task_outcome(&task, terminal)? {
                    report.terminals += 1;
                }
                executor.forget_worker(&worker_id);
            }
        }

        self.refresh_run_status(run_id)?;
        Ok(report)
    }

    pub fn inspect_worker(&self, worker_id: &str) -> Result<FleetWorkerInspection> {
        let state = self.ledger.rebuild_state()?;
        let latest_event = latest_event_for_worker(&state, worker_id).cloned();
        let current = active_task_for_worker(&state, worker_id)
            .or_else(|| latest_task_for_worker(&state, worker_id));
        let current_run_id = current.as_ref().map(|task| task.entry.run_id.clone());
        let current_task_id = current.as_ref().map(|task| task.entry.task_id.clone());
        let (objective, role) = current
            .as_ref()
            .and_then(|task| task_spec_for_state(&state, task))
            .map(|task_spec| {
                (
                    task_spec.objective.or(task_spec.description),
                    task_spec.worker.and_then(|worker| worker.role),
                )
            })
            .unwrap_or((None, None));
        let host = current_run_id
            .as_ref()
            .and_then(|run_id| worker_host_for_run(&state, run_id, worker_id));
        let artifacts = state
            .artifact_events
            .values()
            .filter(|event| event.worker_id == worker_id)
            .filter_map(|event| match &event.payload {
                FleetWorkerEventPayload::Artifact(artifact) => Some(artifact.clone()),
                _ => None,
            })
            .chain(
                state
                    .receipts
                    .values()
                    .filter(|receipt| receipt.worker_id == worker_id)
                    .flat_map(|receipt| receipt.artifacts.clone()),
            )
            .collect();
        let receipt_summary = latest_receipt_for_worker(&state, worker_id).map(receipt_summary);
        let last_error = latest_error_for_worker(&state, worker_id);
        let status = state
            .workers
            .get(worker_id)
            .cloned()
            .unwrap_or(FleetWorkerStatus::Unknown);
        let latest_heartbeat_at = state
            .heartbeats
            .get(worker_id)
            .map(|heartbeat| heartbeat.timestamp.clone());
        let alert_state = latest_alert_for_worker(&state, worker_id);

        // Enrich with sub-agent worker runtime state when available.
        let runtime_state = self.sub_agent_manager.as_ref().and_then(|mgr| {
            mgr.try_read()
                .ok()
                .and_then(|guard| guard.get_worker_record(worker_id))
                .map(|record| FleetWorkerRuntimeProjection {
                    agent_status: format!("{:?}", record.status).to_lowercase(),
                    steps_taken: record.steps_taken,
                    latest_message: record.latest_message,
                    error: record.error,
                    result_summary: record.result_summary,
                    has_session: !matches!(
                        record.status,
                        crate::tools::subagent::AgentWorkerStatus::Completed
                            | crate::tools::subagent::AgentWorkerStatus::Failed
                            | crate::tools::subagent::AgentWorkerStatus::Cancelled
                    ),
                })
        });

        Ok(FleetWorkerInspection {
            worker_id: worker_id.to_string(),
            status,
            current_run_id,
            current_task_id,
            objective,
            role,
            host,
            latest_heartbeat_at,
            latest_event,
            artifacts,
            receipt_summary,
            last_error,
            alert_state,
            runtime_state,
        })
    }

    pub fn interrupt_worker(&self, worker_id: &str) -> Result<FleetWorkerInspection> {
        let state = self.ledger.rebuild_state()?;
        let Some(task) = active_task_for_worker(&state, worker_id) else {
            bail!("worker {worker_id} has no running fleet task");
        };
        self.append_worker_event(
            &task.entry.run_id,
            worker_id,
            &task.entry.task_id,
            FleetWorkerEventPayload::Interrupted {
                signal: Some("operator".to_string()),
            },
        )?;
        self.append_worker_event(
            &task.entry.run_id,
            worker_id,
            &task.entry.task_id,
            FleetWorkerEventPayload::Cancelled {
                cancelled_by: Some("operator".to_string()),
            },
        )?;
        self.refresh_run_status(&task.entry.run_id)?;
        self.inspect_worker(worker_id)
    }

    pub fn restart_worker(&self, worker_id: &str) -> Result<FleetWorkerInspection> {
        let state = self.ledger.rebuild_state()?;
        let Some(task) = active_task_for_worker(&state, worker_id)
            .or_else(|| latest_task_for_worker(&state, worker_id))
        else {
            bail!("worker {worker_id} has no fleet task to restart");
        };
        let now = timestamp();
        self.ledger.lease_task(
            &task.entry.run_id,
            &task.entry.task_id,
            worker_id,
            &now,
            None,
        )?;
        self.append_worker_event(
            &task.entry.run_id,
            worker_id,
            &task.entry.task_id,
            FleetWorkerEventPayload::Restarted { restart_count: 1 },
        )?;
        self.append_worker_event(
            &task.entry.run_id,
            worker_id,
            &task.entry.task_id,
            FleetWorkerEventPayload::Running,
        )?;
        self.ledger.heartbeat(worker_id, &timestamp(), None, None)?;
        self.ledger
            .update_run_status(&task.entry.run_id, FleetRunStatus::Running, &timestamp())?;
        self.inspect_worker(worker_id)
    }

    pub fn stop_all(&self) -> Result<usize> {
        let state = self.ledger.rebuild_state()?;
        let now = timestamp();
        let mut affected_runs = BTreeSet::new();
        let mut stopped = 0usize;
        for task in state.tasks.values() {
            if !matches!(
                task.status,
                FleetTaskLedgerStatus::Enqueued | FleetTaskLedgerStatus::Leased
            ) {
                continue;
            }
            if let Some(worker_id) = task.leased_to.as_deref() {
                self.append_worker_event(
                    &task.entry.run_id,
                    worker_id,
                    &task.entry.task_id,
                    FleetWorkerEventPayload::Interrupted {
                        signal: Some("stop_all".to_string()),
                    },
                )?;
            }
            self.ledger.mark_task_terminal_status(
                &task.entry.run_id,
                &task.entry.task_id,
                task.leased_to.as_deref(),
                &now,
                FleetTaskLedgerStatus::Cancelled,
            )?;
            affected_runs.insert(task.entry.run_id.0.clone());
            stopped += 1;
        }
        for run_id in affected_runs {
            self.ledger.update_run_status(
                &FleetRunId::from(run_id),
                FleetRunStatus::Cancelled,
                &timestamp(),
            )?;
        }
        Ok(stopped)
    }

    pub fn stop_run(&self, run_id: &FleetRunId) -> Result<usize> {
        let state = self.ledger.rebuild_state()?;
        if !state.runs.contains_key(&run_id.0) {
            bail!("fleet run {} does not exist", run_id.0);
        }
        let now = timestamp();
        let mut stopped = 0usize;
        for task in state
            .tasks
            .values()
            .filter(|task| task.entry.run_id == *run_id)
        {
            if !matches!(
                task.status,
                FleetTaskLedgerStatus::Enqueued | FleetTaskLedgerStatus::Leased
            ) {
                continue;
            }
            if let Some(worker_id) = task.leased_to.as_deref() {
                self.append_worker_event(
                    &task.entry.run_id,
                    worker_id,
                    &task.entry.task_id,
                    FleetWorkerEventPayload::Interrupted {
                        signal: Some("stop_run".to_string()),
                    },
                )?;
            }
            self.ledger.mark_task_terminal_status(
                &task.entry.run_id,
                &task.entry.task_id,
                task.leased_to.as_deref(),
                &now,
                FleetTaskLedgerStatus::Cancelled,
            )?;
            stopped += 1;
        }
        self.ledger
            .update_run_status(run_id, FleetRunStatus::Cancelled, &timestamp())?;
        Ok(stopped)
    }

    fn start_worker_task(
        &self,
        worker_id: &str,
        entry: &FleetInboxEntry,
        task_spec: &FleetTaskSpec,
    ) -> Result<()> {
        let sub_agent_worker = if self.sub_agent_manager.is_some() {
            let run = self
                .ledger
                .rebuild_state()
                .ok()
                .and_then(|state| state.runs.get(&entry.run_id.0).cloned());
            let worker_spec = run
                .as_ref()
                .and_then(|r| r.worker_specs.iter().find(|w| w.id == worker_id).cloned())
                .unwrap_or_else(|| FleetWorkerSpec {
                    id: worker_id.to_string(),
                    name: worker_id.to_string(),
                    host: FleetHostSpec::Local,
                    trust_level: Some(FleetTrustLevel::Local),
                    labels: BTreeMap::new(),
                    capabilities: vec![],
                    max_concurrent_tasks: Some(1),
                });
            let agent_profiles = super::profile::load_workspace_agent_profiles(&self.workspace)?;
            let worker = worker_runtime::fleet_task_to_worker_spec_with_profiles(
                worker_id,
                &entry.run_id.0,
                task_spec,
                &worker_spec,
                "auto",
                &self.workspace,
                &agent_profiles,
                None,
            )?;
            Some(worker_runtime::apply_exec_hardening(
                worker,
                &self.exec_config,
            ))
        } else {
            None
        };
        let now = timestamp();
        self.ledger
            .lease_task(&entry.run_id, &entry.task_id, worker_id, &now, None)?;
        self.append_worker_event(
            &entry.run_id,
            worker_id,
            &entry.task_id,
            FleetWorkerEventPayload::Leased {
                lease_expires_at: None,
            },
        )?;
        self.append_worker_event(
            &entry.run_id,
            worker_id,
            &entry.task_id,
            FleetWorkerEventPayload::Starting,
        )?;
        let log_artifact = self.write_log_artifact(&entry.run_id, worker_id, task_spec)?;
        self.append_worker_event(
            &entry.run_id,
            worker_id,
            &entry.task_id,
            FleetWorkerEventPayload::Artifact(log_artifact.clone()),
        )?;
        self.append_worker_event(
            &entry.run_id,
            worker_id,
            &entry.task_id,
            FleetWorkerEventPayload::Running,
        )?;
        self.ledger.heartbeat(worker_id, &timestamp(), None, None)?;

        // Register with the sub-agent manager for headless worker tracking.
        // The engine's agent path handles actual sub-agent spawning.
        if let Some(ref mgr) = self.sub_agent_manager
            && let Some(worker) = sub_agent_worker
            && let Ok(mut guard) = mgr.try_write()
        {
            guard.register_worker(worker);
        }

        Ok(())
    }

    fn start_leased_workers(
        &self,
        run_id: &FleetRunId,
        executor: &mut FleetExecutor,
        mimofan_binary: &str,
        model: Option<&str>,
    ) -> Result<usize> {
        let state = self.ledger.rebuild_state()?;
        let run = state
            .runs
            .get(&run_id.0)
            .cloned()
            .ok_or_else(|| anyhow!("fleet run {} does not exist", run_id.0))?;
        let agent_profiles = super::profile::load_workspace_agent_profiles(&self.workspace)?;
        let mut started = 0usize;
        for task in active_tasks_for_run(&state, run_id) {
            let Some(worker_id) = task.leased_to.as_deref() else {
                continue;
            };
            if executor.is_tracking(worker_id) {
                continue;
            }
            let Some(task_spec) = run
                .task_specs
                .iter()
                .find(|spec| spec.id == task.entry.task_id)
                .cloned()
            else {
                continue;
            };
            let worker_spec = run
                .worker_specs
                .iter()
                .find(|worker| worker.id == worker_id)
                .cloned()
                .unwrap_or_else(|| default_local_worker(worker_id));
            let command = build_worker_exec_command_with_profiles(
                mimofan_binary,
                &task_spec,
                &self.exec_config,
                model,
                &agent_profiles,
            )?;
            let cwd = resolve_task_cwd(&self.workspace, &task_spec);
            match executor.start_worker_on_host(worker_id, &worker_spec.host, command, Some(cwd)) {
                Ok(handle) => {
                    let artifact = self.host_log_artifact(&handle.log_path);
                    self.append_worker_event(
                        run_id,
                        worker_id,
                        &task.entry.task_id,
                        FleetWorkerEventPayload::Artifact(artifact),
                    )?;
                    started += 1;
                }
                Err(err) => {
                    let recoverable = matches!(err.kind, FleetHostErrorKind::Retryable);
                    let task = FleetExecutorTaskContext {
                        entry: task.entry.clone(),
                        task_spec,
                        worker_id: worker_id.to_string(),
                    };
                    let terminal = FleetWorkerTerminalEvent {
                        payload: FleetWorkerEventPayload::Failed {
                            reason: err.message,
                            recoverable,
                        },
                        exit_code: None,
                    };
                    let _ = self.record_task_outcome(&task, terminal)?;
                }
            }
        }
        Ok(started)
    }

    fn executor_task_context(&self, worker_id: &str) -> Result<Option<FleetExecutorTaskContext>> {
        let state = self.ledger.rebuild_state()?;
        let Some(task) = active_task_for_worker(&state, worker_id)
            .or_else(|| latest_task_for_worker(&state, worker_id))
        else {
            return Ok(None);
        };
        let Some(run) = state.runs.get(&task.entry.run_id.0) else {
            return Ok(None);
        };
        let Some(task_spec) = run
            .task_specs
            .iter()
            .find(|spec| spec.id == task.entry.task_id)
            .cloned()
        else {
            return Ok(None);
        };
        Ok(Some(FleetExecutorTaskContext {
            entry: task.entry.clone(),
            task_spec,
            worker_id: worker_id.to_string(),
        }))
    }

    fn record_task_outcome(
        &self,
        task: &FleetExecutorTaskContext,
        terminal: FleetWorkerTerminalEvent,
    ) -> Result<bool> {
        let state = self.ledger.rebuild_state()?;
        let key = task_key(&task.entry.run_id.0, &task.entry.task_id);
        let Some(current) = state.tasks.get(&key) else {
            return Ok(false);
        };
        if !matches!(current.status, FleetTaskLedgerStatus::Leased) {
            return Ok(false);
        }

        let (receipt_result, failure_kind, exit_code) =
            task_receipt_outcome(&terminal.payload, terminal.exit_code);
        let terminal_completed =
            matches!(&terminal.payload, FleetWorkerEventPayload::Completed { .. });
        self.append_worker_event(
            &task.entry.run_id,
            &task.worker_id,
            &task.entry.task_id,
            terminal.payload,
        )?;

        let artifacts = self.task_artifacts_for_receipt(
            &task.entry.run_id,
            &task.entry.task_id,
            &task.worker_id,
        )?;
        // Mint the resolved-route snapshot once (#3154) so every receipt path —
        // verification and the simulated/transport fallback below — persists the
        // same honest, secret-free route detail.
        let resolved_route = self.resolve_task_route(&task.task_spec);
        let verification_input = FleetTaskVerificationInput {
            run_id: task.entry.run_id.clone(),
            task_id: task.entry.task_id.clone(),
            worker_id: task.worker_id.clone(),
            exit_code,
            artifacts,
            resolved_route,
        };
        if task.task_spec.scorer.is_some() {
            let verification =
                verify_task_result(&self.workspace, &task.task_spec, &verification_input);
            let receipt = record_verification_receipt(
                &self.ledger,
                &self.workspace,
                &verification_input,
                verification,
            )?;
            if matches!(
                receipt.result,
                FleetTaskResult::Fail | FleetTaskResult::Timeout
            ) {
                self.ledger.mark_task_terminal_status(
                    &task.entry.run_id,
                    &task.entry.task_id,
                    Some(&task.worker_id),
                    &timestamp(),
                    FleetTaskLedgerStatus::Failed,
                )?;
            }
            return Ok(true);
        }
        if terminal_completed {
            let verification =
                verify_task_result(&self.workspace, &task.task_spec, &verification_input);
            record_verification_receipt(
                &self.ledger,
                &self.workspace,
                &verification_input,
                verification,
            )?;
            return Ok(true);
        }
        self.ledger.record_receipt(FleetReceipt {
            run_id: task.entry.run_id.clone(),
            task_id: task.entry.task_id.clone(),
            worker_id: task.worker_id.clone(),
            completed_at: timestamp(),
            result: receipt_result,
            failure_kind,
            artifacts: verification_input.artifacts,
            score: None,
            resolved_route: verification_input.resolved_route,
        })?;
        Ok(true)
    }

    /// Resolve the route snapshot to persist on a task's receipt (#3154).
    ///
    /// Loads workspace agent profiles so role/loadout intent composes the same
    /// way as the worker-spec path, then mints a secret-free route candidate via
    /// the hermetic resolver bridge. Returns `None` (never a fabricated route)
    /// when profiles or resolution are unavailable.
    fn resolve_task_route(&self, task_spec: &FleetTaskSpec) -> Option<FleetResolvedRoute> {
        let agent_profiles = super::profile::load_workspace_agent_profiles(&self.workspace)
            .ok()
            .unwrap_or_default();
        worker_runtime::resolve_fleet_route(task_spec, &agent_profiles)
    }

    fn task_artifacts_for_receipt(
        &self,
        run_id: &FleetRunId,
        task_id: &str,
        worker_id: &str,
    ) -> Result<Vec<FleetArtifactRef>> {
        let state = self.ledger.rebuild_state()?;
        Ok(state
            .artifact_events
            .values()
            .filter(|event| {
                event.run_id == *run_id && event.task_id == task_id && event.worker_id == worker_id
            })
            .filter_map(|event| match &event.payload {
                FleetWorkerEventPayload::Artifact(artifact) => {
                    Some(self.refresh_artifact_size(artifact.clone()))
                }
                _ => None,
            })
            .collect())
    }

    fn refresh_artifact_size(&self, mut artifact: FleetArtifactRef) -> FleetArtifactRef {
        let path = if artifact.path.is_absolute() {
            artifact.path.clone()
        } else {
            self.workspace.join(&artifact.path)
        };
        artifact.size_bytes = std::fs::metadata(path).ok().map(|meta| meta.len());
        artifact
    }

    fn host_log_artifact(&self, path: &Path) -> FleetArtifactRef {
        let rel_path = path
            .strip_prefix(&self.workspace)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.to_path_buf());
        let size_bytes = std::fs::metadata(path).ok().map(|meta| meta.len());
        FleetArtifactRef {
            kind: FleetArtifactKind::Log,
            path: rel_path,
            checksum: None,
            mime_type: Some("application/x-ndjson".to_string()),
            size_bytes,
        }
    }

    fn append_worker_event(
        &self,
        run_id: &FleetRunId,
        worker_id: &str,
        task_id: &str,
        payload: FleetWorkerEventPayload,
    ) -> Result<FleetWorkerEvent> {
        let state = self.ledger.rebuild_state()?;
        let key = event_key(worker_id, &run_id.0, task_id);
        let seq = state.latest_seq.get(&key).copied().unwrap_or(0) + 1;
        let event = FleetWorkerEvent {
            seq,
            run_id: run_id.clone(),
            worker_id: worker_id.to_string(),
            task_id: task_id.to_string(),
            timestamp: timestamp(),
            payload,
            extra: BTreeMap::new(),
        };
        self.ledger.append_event(event.clone())?;
        Ok(event)
    }

    fn write_log_artifact(
        &self,
        run_id: &FleetRunId,
        worker_id: &str,
        task_spec: &FleetTaskSpec,
    ) -> Result<FleetArtifactRef> {
        let rel_path = PathBuf::from(".mimofan")
            .join("fleet")
            .join(safe_path_segment(&run_id.0))
            .join(safe_path_segment(&task_spec.id))
            .join(format!("{}.log", safe_path_segment(worker_id)));
        let abs_path = self.workspace.join(&rel_path);
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating fleet artifact dir {}", parent.display()))?;
        }
        let contents = format!(
            "run_id={}\ntask_id={}\ntask_name={}\nworker_id={}\nstatus=started\n",
            run_id.0, task_spec.id, task_spec.name, worker_id
        );
        std::fs::write(&abs_path, contents)
            .with_context(|| format!("writing fleet worker log {}", abs_path.display()))?;
        let size_bytes = std::fs::metadata(&abs_path).ok().map(|m| m.len());
        Ok(FleetArtifactRef {
            kind: FleetArtifactKind::Log,
            path: rel_path,
            checksum: None,
            mime_type: Some("text/plain".to_string()),
            size_bytes,
        })
    }

    fn refresh_run_status(&self, run_id: &FleetRunId) -> Result<()> {
        let state = self.ledger.rebuild_state()?;
        let mut has_queued = false;
        let mut has_running = false;
        let mut has_failed = false;
        let mut has_cancelled = false;
        let mut has_tasks = false;
        for task in state
            .tasks
            .values()
            .filter(|task| task.entry.run_id == *run_id)
        {
            has_tasks = true;
            match task.status {
                FleetTaskLedgerStatus::Enqueued => has_queued = true,
                FleetTaskLedgerStatus::Leased => has_running = true,
                FleetTaskLedgerStatus::Failed => has_failed = true,
                FleetTaskLedgerStatus::Cancelled => has_cancelled = true,
                FleetTaskLedgerStatus::Completed => {}
            }
        }
        let status = if !has_tasks {
            FleetRunStatus::Completed
        } else if has_queued || has_running {
            FleetRunStatus::Running
        } else if has_failed {
            FleetRunStatus::Failed
        } else if has_cancelled {
            FleetRunStatus::Cancelled
        } else {
            FleetRunStatus::Completed
        };
        self.ledger
            .update_run_status(run_id, status, &timestamp())
            .context("updating fleet run status")
    }

    fn status_from_state(
        &self,
        run_filter: Option<&FleetRunId>,
        state: &FleetLedgerState,
    ) -> FleetStatusSnapshot {
        let mut snapshot = FleetStatusSnapshot {
            runs: state.runs.len(),
            workers: state.workers.clone(),
            ..FleetStatusSnapshot::default()
        };
        for task in state.tasks.values() {
            if run_filter.is_some_and(|run_id| task.entry.run_id != *run_id) {
                continue;
            }
            match task.status {
                FleetTaskLedgerStatus::Enqueued => snapshot.queued += 1,
                FleetTaskLedgerStatus::Leased => {
                    if self.task_is_stale(task, state) {
                        snapshot.stale += 1;
                    } else {
                        snapshot.running += 1;
                    }
                }
                FleetTaskLedgerStatus::Completed => snapshot.completed += 1,
                FleetTaskLedgerStatus::Failed => snapshot.failed += 1,
                FleetTaskLedgerStatus::Cancelled => snapshot.cancelled += 1,
            }
        }
        for receipt in state.receipts.values() {
            if run_filter.is_some_and(|run_id| receipt.run_id != *run_id) {
                continue;
            }
            if receipt.result == FleetTaskResult::Partial {
                snapshot.partial += 1;
            }
            match &receipt.failure_kind {
                Some(FleetTaskFailureKind::Transport) => snapshot.transport_failed += 1,
                Some(FleetTaskFailureKind::Task) => snapshot.task_failed += 1,
                Some(FleetTaskFailureKind::Verifier) => snapshot.verifier_failed += 1,
                None => {}
            }
        }
        snapshot.restarted = state
            .restarted_events
            .values()
            .filter(|event| run_filter.is_none_or(|run_id| event.run_id == *run_id))
            .count();
        snapshot.escalated = state
            .escalated_events
            .values()
            .filter(|event| run_filter.is_none_or(|run_id| event.run_id == *run_id))
            .count();
        snapshot
    }

    fn task_is_stale(&self, task: &FleetTaskState, state: &FleetLedgerState) -> bool {
        let Some(worker_id) = task.leased_to.as_deref() else {
            return true;
        };
        let Some(heartbeat) = state.heartbeats.get(worker_id) else {
            return true;
        };
        let Ok(last) = DateTime::parse_from_rfc3339(&heartbeat.timestamp) else {
            return true;
        };
        let age = Utc::now().signed_duration_since(last.with_timezone(&Utc));
        age.to_std()
            .is_ok_and(|duration| duration > self.stale_after)
    }
}

fn default_local_workers(run_id: &FleetRunId, max_workers: usize) -> Vec<FleetWorkerSpec> {
    (1..=max_workers)
        .map(|index| {
            default_local_worker_with_name(&format!("{}-local-{}", run_id.0, index), index)
        })
        .collect()
}

fn default_local_worker_with_name(worker_id: &str, index: usize) -> FleetWorkerSpec {
    FleetWorkerSpec {
        id: worker_id.to_string(),
        name: format!("Local worker {index}"),
        host: FleetHostSpec::Local,
        trust_level: Some(FleetTrustLevel::Local),
        labels: BTreeMap::new(),
        capabilities: vec!["local".to_string()],
        max_concurrent_tasks: Some(1),
    }
}

fn default_local_worker(worker_id: &str) -> FleetWorkerSpec {
    FleetWorkerSpec {
        id: worker_id.to_string(),
        name: worker_id.to_string(),
        host: FleetHostSpec::Local,
        trust_level: Some(FleetTrustLevel::Local),
        labels: BTreeMap::new(),
        capabilities: vec!["local".to_string()],
        max_concurrent_tasks: Some(1),
    }
}

fn worker_ids_for_run(run: &FleetRun, max_workers: usize) -> Vec<String> {
    run.worker_specs
        .iter()
        .take(max_workers)
        .map(|worker| worker.id.clone())
        .collect()
}

fn active_workers_for_run(state: &FleetLedgerState, run_id: &FleetRunId) -> BTreeSet<String> {
    active_tasks_for_run(state, run_id)
        .filter_map(|task| task.leased_to.clone())
        .collect()
}

fn active_tasks_for_run<'a>(
    state: &'a FleetLedgerState,
    run_id: &'a FleetRunId,
) -> impl Iterator<Item = &'a FleetTaskState> {
    state.tasks.values().filter(move |task| {
        task.entry.run_id == *run_id && matches!(task.status, FleetTaskLedgerStatus::Leased)
    })
}

fn active_task_for_worker<'a>(
    state: &'a FleetLedgerState,
    worker_id: &str,
) -> Option<&'a FleetTaskState> {
    state.tasks.values().find(|task| {
        task.leased_to.as_deref() == Some(worker_id)
            && matches!(task.status, FleetTaskLedgerStatus::Leased)
    })
}

fn latest_task_for_worker<'a>(
    state: &'a FleetLedgerState,
    worker_id: &str,
) -> Option<&'a FleetTaskState> {
    state
        .tasks
        .values()
        .filter(|task| task.leased_to.as_deref() == Some(worker_id))
        .max_by_key(|task| task.completed_at.as_deref().or(task.leased_at.as_deref()))
}

fn next_enqueued_task_for_run(
    state: &FleetLedgerState,
    run_id: &FleetRunId,
) -> Option<(FleetInboxEntry, FleetTaskSpec)> {
    let run = state.runs.get(&run_id.0)?;
    let task = state
        .tasks
        .values()
        .filter(|task| {
            task.entry.run_id == *run_id && matches!(task.status, FleetTaskLedgerStatus::Enqueued)
        })
        .min_by_key(|task| {
            (
                task.entry.priority,
                task.entry.enqueued_at.clone(),
                task.entry.task_id.clone(),
            )
        })?;
    let task_spec = run
        .task_specs
        .iter()
        .find(|spec| spec.id == task.entry.task_id)
        .cloned()?;
    Some((task.entry.clone(), task_spec))
}

fn task_spec_for_state(state: &FleetLedgerState, task: &FleetTaskState) -> Option<FleetTaskSpec> {
    state
        .runs
        .get(&task.entry.run_id.0)?
        .task_specs
        .iter()
        .find(|spec| spec.id == task.entry.task_id)
        .cloned()
}

fn worker_host_for_run(
    state: &FleetLedgerState,
    run_id: &FleetRunId,
    worker_id: &str,
) -> Option<String> {
    let run = state.runs.get(&run_id.0)?;
    let worker = run
        .worker_specs
        .iter()
        .find(|worker| worker.id == worker_id)?;
    Some(host_label(&worker.host))
}

fn host_label(host: &FleetHostSpec) -> String {
    match host {
        FleetHostSpec::Local => "local".to_string(),
        FleetHostSpec::Ssh { host, .. } => format!("ssh:{host}"),
        FleetHostSpec::Docker { image, .. } => format!("docker:{image}"),
    }
}

fn latest_event_for_worker<'a>(
    state: &'a FleetLedgerState,
    worker_id: &str,
) -> Option<&'a FleetWorkerEvent> {
    state
        .latest_events
        .values()
        .filter(|event| event.worker_id == worker_id)
        .max_by_key(|event| event.seq)
}

fn latest_alert_for_worker(state: &FleetLedgerState, worker_id: &str) -> Option<String> {
    state
        .escalated_events
        .values()
        .filter(|event| event.worker_id == worker_id)
        .filter_map(|event| match &event.payload {
            FleetWorkerEventPayload::Escalated { channel, alert_id } => Some((
                event.seq,
                alert_id
                    .as_ref()
                    .map(|alert_id| format!("escalated via {channel} alert_id={alert_id}"))
                    .unwrap_or_else(|| format!("escalated via {channel}")),
            )),
            _ => None,
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, message)| message)
}

fn latest_receipt_for_worker<'a>(
    state: &'a FleetLedgerState,
    worker_id: &str,
) -> Option<&'a FleetReceipt> {
    state
        .receipts
        .values()
        .filter(|receipt| receipt.worker_id == worker_id)
        .max_by_key(|receipt| &receipt.completed_at)
}

fn receipt_summary(receipt: &FleetReceipt) -> String {
    let result = match receipt.result {
        FleetTaskResult::Pass => "pass",
        FleetTaskResult::Partial => "partial",
        FleetTaskResult::Fail => "fail",
        FleetTaskResult::Skip => "skip",
        FleetTaskResult::Timeout => "timeout",
    };
    let mut summary = format!("result={result}");
    if let Some(kind) = &receipt.failure_kind {
        let kind = match kind {
            FleetTaskFailureKind::Transport => "transport",
            FleetTaskFailureKind::Task => "task",
            FleetTaskFailureKind::Verifier => "verifier",
        };
        summary.push_str(&format!(" failure_kind={kind}"));
    }
    if let Some(notes) = receipt
        .score
        .as_ref()
        .and_then(|score| score.notes.as_deref())
        .filter(|notes| !notes.trim().is_empty())
    {
        summary.push_str(&format!(" notes={notes}"));
    }
    summary
}

fn latest_error_for_worker(state: &FleetLedgerState, worker_id: &str) -> Option<String> {
    state
        .latest_events
        .values()
        .filter(|event| event.worker_id == worker_id)
        .filter_map(|event| match &event.payload {
            FleetWorkerEventPayload::Failed { reason, .. } => {
                Some((event.seq, format!("failed: {reason}")))
            }
            FleetWorkerEventPayload::Cancelled { cancelled_by } => Some((
                event.seq,
                cancelled_by
                    .as_ref()
                    .map(|by| format!("cancelled by {by}"))
                    .unwrap_or_else(|| "cancelled".to_string()),
            )),
            FleetWorkerEventPayload::Interrupted { signal } => Some((
                event.seq,
                signal
                    .as_ref()
                    .map(|signal| format!("interrupted by {signal}"))
                    .unwrap_or_else(|| "interrupted".to_string()),
            )),
            FleetWorkerEventPayload::Stale { last_heartbeat_at } => Some((
                event.seq,
                last_heartbeat_at
                    .as_ref()
                    .map(|ts| format!("stale since {ts}"))
                    .unwrap_or_else(|| "stale".to_string()),
            )),
            _ => None,
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, message)| message)
}

fn task_priority(task: &FleetTaskSpec) -> i32 {
    task.metadata
        .get("priority")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0)
}

fn resolve_task_cwd(workspace: &Path, task: &FleetTaskSpec) -> PathBuf {
    let Some(root) = task
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.root.as_ref())
    else {
        return workspace.to_path_buf();
    };
    if root.is_absolute() {
        root.clone()
    } else {
        workspace.join(root)
    }
}

fn task_receipt_outcome(
    payload: &FleetWorkerEventPayload,
    exit_code: Option<i32>,
) -> (FleetTaskResult, Option<FleetTaskFailureKind>, Option<i32>) {
    match payload {
        FleetWorkerEventPayload::Completed {
            exit_code: payload_exit_code,
            ..
        } => (
            FleetTaskResult::Pass,
            None,
            exit_code.or(*payload_exit_code),
        ),
        FleetWorkerEventPayload::Cancelled { .. } => (FleetTaskResult::Skip, None, exit_code),
        FleetWorkerEventPayload::Failed { .. } => {
            let failure_kind = if exit_code.is_none() {
                FleetTaskFailureKind::Transport
            } else {
                FleetTaskFailureKind::Task
            };
            (FleetTaskResult::Fail, Some(failure_kind), exit_code)
        }
        _ => (FleetTaskResult::Partial, None, exit_code),
    }
}

fn is_terminal_payload(payload: &FleetWorkerEventPayload) -> bool {
    matches!(
        payload,
        FleetWorkerEventPayload::Completed { .. }
            | FleetWorkerEventPayload::Failed { .. }
            | FleetWorkerEventPayload::Cancelled { .. }
            | FleetWorkerEventPayload::Interrupted { .. }
    )
}

fn task_key(run_id: &str, task_id: &str) -> String {
    format!("{run_id}:{task_id}")
}

fn event_key(worker_id: &str, run_id: &str, task_id: &str) -> String {
    format!("{worker_id}:{run_id}:{task_id}")
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn safe_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {}
