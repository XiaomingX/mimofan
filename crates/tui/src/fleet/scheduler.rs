//! Fleet scheduler policy: leases, heartbeats, backpressure, and recovery.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, SecondsFormat, Utc};
use mimofan_protocol::fleet::*;
use serde_json::Value;

use super::ledger::{FleetLedger, FleetLedgerState, FleetTaskLedgerStatus, FleetTaskState};

#[derive(Debug, Clone)]
pub struct FleetSchedulerPolicy {
    pub max_workers_per_run: usize,
    pub max_workers_per_host: usize,
    pub max_workers_per_task_class: usize,
    pub lease_seconds: u64,
    pub heartbeat_timeout: Duration,
}

impl Default for FleetSchedulerPolicy {
    fn default() -> Self {
        Self {
            max_workers_per_run: 4,
            max_workers_per_host: 4,
            max_workers_per_task_class: 4,
            lease_seconds: 300,
            heartbeat_timeout: Duration::from_secs(120),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FleetSchedulerReport {
    pub launched: usize,
    pub heartbeats: usize,
    pub marked_stale: usize,
    pub restarted: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub alerts: usize,
}

#[derive(Debug)]
pub struct FleetScheduler {
    ledger: FleetLedger,
    policy: FleetSchedulerPolicy,
    now: DateTime<Utc>,
}

impl FleetScheduler {
    pub fn open(workspace: impl AsRef<Path>, policy: FleetSchedulerPolicy) -> Result<Self> {
        Ok(Self {
            ledger: FleetLedger::open(workspace.as_ref())?,
            policy,
            now: Utc::now(),
        })
    }

    pub fn set_now(&mut self, now: DateTime<Utc>) {
        self.now = now;
    }

    pub fn tick_run(&self, run_id: &FleetRunId) -> Result<FleetSchedulerReport> {
        let mut report = FleetSchedulerReport::default();
        self.recover_unhealthy_work(run_id, &mut report)?;
        self.launch_queued_work(run_id, &mut report)?;
        self.refresh_run_status(run_id)?;
        Ok(report)
    }

    /// Resume reconciliation after a manager restart: detect orphaned/stale
    /// in-flight leases left by a prior process and apply retry/escalation
    /// policy, then recompute run status.
    ///
    /// Unlike [`tick_run`], this launches no new queued work and does not
    /// re-process tasks that already reached a terminal state, so it is safe
    /// and idempotent to call on a fresh process: a task re-leased by an
    /// earlier resume is no longer stale at the same instant, and a terminally
    /// failed task is never failed or escalated twice.
    pub fn resume_run(&self, run_id: &FleetRunId) -> Result<FleetSchedulerReport> {
        let mut report = FleetSchedulerReport::default();
        self.reconcile_stale_leases(run_id, &mut report)?;
        self.refresh_run_status(run_id)?;
        Ok(report)
    }

    pub fn cancel_run(&self, run_id: &FleetRunId, reason: &str) -> Result<FleetSchedulerReport> {
        let state = self.ledger.rebuild_state()?;
        let mut report = FleetSchedulerReport::default();
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
                        signal: Some(reason.to_string()),
                    },
                )?;
                self.append_worker_event(
                    &task.entry.run_id,
                    worker_id,
                    &task.entry.task_id,
                    FleetWorkerEventPayload::Cancelled {
                        cancelled_by: Some("scheduler".to_string()),
                    },
                )?;
            } else {
                self.ledger.mark_task_terminal_status(
                    &task.entry.run_id,
                    &task.entry.task_id,
                    None,
                    &self.timestamp(),
                    FleetTaskLedgerStatus::Cancelled,
                )?;
            }
            report.cancelled += 1;
        }
        self.ledger
            .update_run_status(run_id, FleetRunStatus::Cancelled, &self.timestamp())?;
        Ok(report)
    }

    fn recover_unhealthy_work(
        &self,
        run_id: &FleetRunId,
        report: &mut FleetSchedulerReport,
    ) -> Result<()> {
        let state = self.ledger.rebuild_state()?;
        for task in state
            .tasks
            .values()
            .filter(|task| task.entry.run_id == *run_id)
        {
            let Some(task_spec) = task_spec_for(&state, task) else {
                continue;
            };
            match task.status {
                FleetTaskLedgerStatus::Leased if self.task_is_stale(task, &state) => {
                    let worker_id = task
                        .leased_to
                        .clone()
                        .unwrap_or_else(|| "fleet-scheduler".to_string());
                    self.append_worker_event(
                        &task.entry.run_id,
                        &worker_id,
                        &task.entry.task_id,
                        FleetWorkerEventPayload::Stale {
                            last_heartbeat_at: state
                                .heartbeats
                                .get(&worker_id)
                                .map(|heartbeat| heartbeat.timestamp.clone()),
                        },
                    )?;
                    report.marked_stale += 1;
                    self.retry_or_fail(task, &task_spec, &worker_id, report)
                        .with_context(|| format!("recovering stale task {}", task.entry.task_id))?;
                }
                FleetTaskLedgerStatus::Failed => {
                    let worker_id = task
                        .leased_to
                        .clone()
                        .unwrap_or_else(|| "fleet-scheduler".to_string());
                    self.retry_or_fail(task, &task_spec, &worker_id, report)
                        .with_context(|| {
                            format!("recovering failed task {}", task.entry.task_id)
                        })?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Reconcile only orphaned/stale in-flight leases (the restart-recovery
    /// subset of [`recover_unhealthy_work`]): a `Leased` task whose worker has
    /// stopped heartbeating is marked stale and routed through the shared
    /// retry/escalation budget. Terminal and healthy tasks are left untouched,
    /// which keeps [`resume_run`] idempotent.
    fn reconcile_stale_leases(
        &self,
        run_id: &FleetRunId,
        report: &mut FleetSchedulerReport,
    ) -> Result<()> {
        let state = self.ledger.rebuild_state()?;
        for task in state
            .tasks
            .values()
            .filter(|task| task.entry.run_id == *run_id)
        {
            if !matches!(task.status, FleetTaskLedgerStatus::Leased)
                || !self.task_is_stale(task, &state)
            {
                continue;
            }
            let Some(task_spec) = task_spec_for(&state, task) else {
                continue;
            };
            let worker_id = task
                .leased_to
                .clone()
                .unwrap_or_else(|| "fleet-scheduler".to_string());
            self.append_worker_event(
                &task.entry.run_id,
                &worker_id,
                &task.entry.task_id,
                FleetWorkerEventPayload::Stale {
                    last_heartbeat_at: state
                        .heartbeats
                        .get(&worker_id)
                        .map(|heartbeat| heartbeat.timestamp.clone()),
                },
            )?;
            report.marked_stale += 1;
            self.retry_or_fail(task, &task_spec, &worker_id, report)
                .with_context(|| format!("resuming stale task {}", task.entry.task_id))?;
        }
        Ok(())
    }

    fn retry_or_fail(
        &self,
        task: &FleetTaskState,
        task_spec: &FleetTaskSpec,
        worker_id: &str,
        report: &mut FleetSchedulerReport,
    ) -> Result<()> {
        let retry_policy = task_spec.retry_policy.clone().unwrap_or_default();
        if task.entry.attempts < retry_policy.max_attempts {
            let lease_expires_at = self.lease_expires_at();
            self.ledger.lease_task(
                &task.entry.run_id,
                &task.entry.task_id,
                worker_id,
                &self.timestamp(),
                Some(&lease_expires_at),
            )?;
            self.append_worker_event(
                &task.entry.run_id,
                worker_id,
                &task.entry.task_id,
                FleetWorkerEventPayload::Restarted {
                    restart_count: task.entry.attempts,
                },
            )?;
            self.append_worker_event(
                &task.entry.run_id,
                worker_id,
                &task.entry.task_id,
                FleetWorkerEventPayload::Running,
            )?;
            self.ledger
                .heartbeat(worker_id, &self.timestamp(), None, None)?;
            report.restarted += 1;
            return Ok(());
        }

        self.append_worker_event(
            &task.entry.run_id,
            worker_id,
            &task.entry.task_id,
            FleetWorkerEventPayload::Failed {
                reason: format!(
                    "retry attempts exhausted after {} attempt(s)",
                    task.entry.attempts
                ),
                recoverable: false,
            },
        )?;
        report.failed += 1;
        report.alerts += self.record_alerts(
            &task.entry.run_id,
            &task.entry.task_id,
            worker_id,
            task_spec,
            FleetAlertEventClass::RestartExhausted,
        )?;
        Ok(())
    }

    fn launch_queued_work(
        &self,
        run_id: &FleetRunId,
        report: &mut FleetSchedulerReport,
    ) -> Result<()> {
        loop {
            let state = self.ledger.rebuild_state()?;
            let run = state
                .runs
                .get(&run_id.0)
                .ok_or_else(|| anyhow!("fleet run {} does not exist", run_id.0))?;
            let active = active_tasks_for_run(&state, run_id);
            if active.len() >= self.policy.max_workers_per_run {
                return Ok(());
            }
            let counts = active_counts(&state, run);
            let Some((worker_id, task)) = self.next_launch(run, &state, &counts) else {
                return Ok(());
            };
            let lease_expires_at = self.lease_expires_at();
            self.ledger.lease_task(
                &task.entry.run_id,
                &task.entry.task_id,
                &worker_id,
                &self.timestamp(),
                Some(&lease_expires_at),
            )?;
            self.append_worker_event(
                &task.entry.run_id,
                &worker_id,
                &task.entry.task_id,
                FleetWorkerEventPayload::Leased {
                    lease_expires_at: Some(lease_expires_at),
                },
            )?;
            self.append_worker_event(
                &task.entry.run_id,
                &worker_id,
                &task.entry.task_id,
                FleetWorkerEventPayload::Starting,
            )?;
            self.append_worker_event(
                &task.entry.run_id,
                &worker_id,
                &task.entry.task_id,
                FleetWorkerEventPayload::Running,
            )?;
            self.ledger
                .heartbeat(&worker_id, &self.timestamp(), None, None)?;
            report.launched += 1;
            report.heartbeats += 1;
        }
    }

    fn next_launch(
        &self,
        run: &FleetRun,
        state: &FleetLedgerState,
        counts: &ActiveCounts,
    ) -> Option<(String, FleetTaskState)> {
        let active_workers: BTreeSet<_> = active_tasks_for_run(state, &run.id)
            .into_iter()
            .filter_map(|task| task.leased_to)
            .collect();
        let mut queued: Vec<_> = state
            .tasks
            .values()
            .filter(|task| {
                task.entry.run_id == run.id
                    && matches!(task.status, FleetTaskLedgerStatus::Enqueued)
            })
            .cloned()
            .collect();
        queued.sort_by_key(|task| {
            (
                task.entry.priority,
                task.entry.enqueued_at.clone(),
                task.entry.task_id.clone(),
            )
        });
        for task in queued {
            let task_spec = run
                .task_specs
                .iter()
                .find(|spec| spec.id == task.entry.task_id)?;
            let task_class = task_class(task_spec);
            if counts.by_task_class.get(&task_class).copied().unwrap_or(0)
                >= self.policy.max_workers_per_task_class
            {
                continue;
            }
            for worker in &run.worker_specs {
                if active_workers.contains(&worker.id) {
                    continue;
                }
                let host_key = host_key(worker);
                if counts.by_host.get(&host_key).copied().unwrap_or(0)
                    >= self.policy.max_workers_per_host
                {
                    continue;
                }
                return Some((worker.id.clone(), task));
            }
        }
        None
    }

    fn task_is_stale(&self, task: &FleetTaskState, state: &FleetLedgerState) -> bool {
        if let Some(worker_id) = task.leased_to.as_deref()
            && let Some(heartbeat) = state.heartbeats.get(worker_id)
            && let Ok(last) = DateTime::parse_from_rfc3339(&heartbeat.timestamp)
        {
            let age = self.now.signed_duration_since(last.with_timezone(&Utc));
            return age
                .to_std()
                .map_or(true, |age| age > self.policy.heartbeat_timeout);
        }
        if let Some(deadline) = task.entry.lease_deadline.as_deref()
            && let Ok(deadline) = DateTime::parse_from_rfc3339(deadline)
        {
            return self.now > deadline.with_timezone(&Utc);
        }
        true
    }

    fn record_alerts(
        &self,
        run_id: &FleetRunId,
        task_id: &str,
        worker_id: &str,
        task_spec: &FleetTaskSpec,
        event_class: FleetAlertEventClass,
    ) -> Result<usize> {
        let Some(policy) = &task_spec.alert_policy else {
            return Ok(0);
        };
        if !alert_policy_matches(policy, event_class) {
            return Ok(0);
        }
        let mut count = 0;
        for channel in &policy.channels {
            let label = alert_channel_label(channel);
            self.ledger
                .record_alert(run_id, task_id, label, &self.timestamp())?;
            self.append_worker_event(
                run_id,
                worker_id,
                task_id,
                FleetWorkerEventPayload::Escalated {
                    channel: label.to_string(),
                    alert_id: None,
                },
            )?;
            count += 1;
        }
        Ok(count)
    }

    fn refresh_run_status(&self, run_id: &FleetRunId) -> Result<()> {
        let state = self.ledger.rebuild_state()?;
        let mut has_open = false;
        let mut has_failed = false;
        let mut has_cancelled = false;
        for task in state
            .tasks
            .values()
            .filter(|task| task.entry.run_id == *run_id)
        {
            match task.status {
                FleetTaskLedgerStatus::Enqueued | FleetTaskLedgerStatus::Leased => has_open = true,
                FleetTaskLedgerStatus::Failed => has_failed = true,
                FleetTaskLedgerStatus::Cancelled => has_cancelled = true,
                FleetTaskLedgerStatus::Completed => {}
            }
        }
        let status = if has_open {
            FleetRunStatus::Running
        } else if has_failed {
            FleetRunStatus::Failed
        } else if has_cancelled {
            FleetRunStatus::Cancelled
        } else {
            FleetRunStatus::Completed
        };
        self.ledger
            .update_run_status(run_id, status, &self.timestamp())
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
            timestamp: self.timestamp(),
            payload,
            extra: BTreeMap::new(),
        };
        self.ledger.append_event(event.clone())?;
        Ok(event)
    }

    fn timestamp(&self) -> String {
        self.now.to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    fn lease_expires_at(&self) -> String {
        (self.now + chrono::Duration::seconds(self.policy.lease_seconds as i64))
            .to_rfc3339_opts(SecondsFormat::Secs, true)
    }
}

#[derive(Debug, Default)]
struct ActiveCounts {
    by_host: BTreeMap<String, usize>,
    by_task_class: BTreeMap<String, usize>,
}

fn active_counts(state: &FleetLedgerState, run: &FleetRun) -> ActiveCounts {
    let mut counts = ActiveCounts::default();
    for task in active_tasks_for_run(state, &run.id) {
        if let Some(worker_id) = task.leased_to.as_deref()
            && let Some(worker) = run
                .worker_specs
                .iter()
                .find(|worker| worker.id == worker_id)
        {
            *counts.by_host.entry(host_key(worker)).or_default() += 1;
        }
        if let Some(task_spec) = run
            .task_specs
            .iter()
            .find(|spec| spec.id == task.entry.task_id)
        {
            *counts
                .by_task_class
                .entry(task_class(task_spec))
                .or_default() += 1;
        }
    }
    counts
}

fn active_tasks_for_run(state: &FleetLedgerState, run_id: &FleetRunId) -> Vec<FleetTaskState> {
    state
        .tasks
        .values()
        .filter(|task| {
            task.entry.run_id == *run_id && matches!(task.status, FleetTaskLedgerStatus::Leased)
        })
        .cloned()
        .collect()
}

fn task_spec_for(state: &FleetLedgerState, task: &FleetTaskState) -> Option<FleetTaskSpec> {
    state
        .runs
        .get(&task.entry.run_id.0)?
        .task_specs
        .iter()
        .find(|spec| spec.id == task.entry.task_id)
        .cloned()
}

fn host_key(worker: &FleetWorkerSpec) -> String {
    match &worker.host {
        FleetHostSpec::Local => "local".to_string(),
        FleetHostSpec::Ssh { host, .. } => format!("ssh:{host}"),
        FleetHostSpec::Docker { image, .. } => format!("docker:{image}"),
    }
}

fn task_class(task: &FleetTaskSpec) -> String {
    task.metadata
        .get("class")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("default")
        .to_string()
}

fn alert_channel_label(channel: &FleetAlertChannel) -> &'static str {
    match channel {
        FleetAlertChannel::Slack { .. } => "slack",
        FleetAlertChannel::Webhook { .. } => "webhook",
        FleetAlertChannel::PagerDuty { .. } => "pagerduty",
    }
}

fn alert_policy_matches(policy: &FleetAlertPolicy, class: FleetAlertEventClass) -> bool {
    policy.events.is_empty() || policy.events.contains(&class)
}

fn event_key(worker_id: &str, run_id: &str, task_id: &str) -> String {
    format!("{worker_id}:{run_id}:{task_id}")
}

#[cfg(test)]
mod tests {}
