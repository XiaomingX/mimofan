//! Fleet management route handlers.

use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Value, json};

use mimofan_protocol::fleet::{
    FleetArtifactKind, FleetRun, FleetRunId, FleetWorkerEventPayload, FleetWorkerStatus,
};

use crate::fleet::ledger::{FleetLedgerState, FleetTaskLedgerStatus};
use crate::fleet::manager::{
    FleetManager, FleetStatusSnapshot, FleetWorkerInspection, FleetWorkerRuntimeProjection,
};

use super::RuntimeApiState;
use super::types::ApiError;

pub(crate) async fn list_fleet_runs(
    State(state): State<RuntimeApiState>,
) -> Result<Json<Value>, ApiError> {
    let manager = open_fleet_manager(&state)?;
    let ledger_state = manager
        .rebuild_state()
        .map_err(|err| ApiError::internal(format!("Failed to rebuild fleet state: {err}")))?;
    let runs: Vec<_> = ledger_state
        .runs
        .values()
        .map(|run| fleet_run_summary_json(&manager, run, &ledger_state))
        .collect::<Result<Vec<_>, _>>()?;
    let status = manager
        .status()
        .map_err(|err| ApiError::internal(format!("Failed to read fleet status: {err}")))?;
    Ok(Json(json!({
        "status": fleet_status_json(&status),
        "runs": runs,
    })))
}

pub(crate) async fn get_fleet_run(
    State(state): State<RuntimeApiState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let manager = open_fleet_manager(&state)?;
    let ledger_state = manager
        .rebuild_state()
        .map_err(|err| ApiError::internal(format!("Failed to rebuild fleet state: {err}")))?;
    let run = ledger_state
        .runs
        .get(&run_id)
        .ok_or_else(|| ApiError::not_found(format!("fleet run '{run_id}' not found")))?;
    Ok(Json(fleet_run_detail_json(&manager, run, &ledger_state)?))
}

pub(crate) async fn list_fleet_run_workers(
    State(state): State<RuntimeApiState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let manager = open_fleet_manager(&state)?;
    let ledger_state = manager
        .rebuild_state()
        .map_err(|err| ApiError::internal(format!("Failed to rebuild fleet state: {err}")))?;
    let run = ledger_state
        .runs
        .get(&run_id)
        .ok_or_else(|| ApiError::not_found(format!("fleet run '{run_id}' not found")))?;
    let workers = run
        .worker_specs
        .iter()
        .map(|worker| {
            manager
                .inspect_worker(&worker.id)
                .map(|inspection| fleet_worker_json(&inspection))
                .map_err(|err| {
                    ApiError::internal(format!(
                        "Failed to inspect fleet worker {}: {err}",
                        worker.id
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({
        "run_id": run_id,
        "workers": workers,
    })))
}

pub(crate) async fn get_fleet_worker(
    State(state): State<RuntimeApiState>,
    Path(worker_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let manager = open_fleet_manager(&state)?;
    let inspection = manager.inspect_worker(&worker_id).map_err(|err| {
        ApiError::not_found(format!("fleet worker '{worker_id}' not found: {err}"))
    })?;
    Ok(Json(fleet_worker_json(&inspection)))
}

pub(crate) async fn interrupt_fleet_worker(
    State(state): State<RuntimeApiState>,
    Path(worker_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let manager = open_fleet_manager(&state)?;
    let inspection = manager.interrupt_worker(&worker_id).map_err(|err| {
        ApiError::bad_request(format!(
            "Failed to interrupt fleet worker '{worker_id}': {err}"
        ))
    })?;
    Ok(Json(json!({
        "action": "interrupt",
        "worker": fleet_worker_json(&inspection),
    })))
}

pub(crate) async fn restart_fleet_worker(
    State(state): State<RuntimeApiState>,
    Path(worker_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let manager = open_fleet_manager(&state)?;
    let inspection = manager.restart_worker(&worker_id).map_err(|err| {
        ApiError::bad_request(format!(
            "Failed to restart fleet worker '{worker_id}': {err}"
        ))
    })?;
    Ok(Json(json!({
        "action": "restart",
        "worker": fleet_worker_json(&inspection),
    })))
}

pub(crate) async fn stop_fleet_run(
    State(state): State<RuntimeApiState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let manager = open_fleet_manager(&state)?;
    let run_id = FleetRunId::from(run_id);
    let stopped = manager.stop_run(&run_id).map_err(|err| {
        ApiError::bad_request(format!("Failed to stop fleet run '{}': {err}", run_id.0))
    })?;
    let status = manager
        .run_status(&run_id)
        .map_err(|err| ApiError::internal(format!("Failed to read fleet run status: {err}")))?;
    Ok(Json(json!({
        "action": "stop",
        "run_id": run_id.0,
        "stopped": stopped,
        "status": fleet_status_json(&status),
    })))
}

pub(crate) async fn list_agent_runs(
    State(state): State<RuntimeApiState>,
) -> Result<Json<super::types::AgentRunsResponse>, ApiError> {
    let runs = crate::tools::subagent::load_persisted_agent_worker_records(&state.workspace)
        .map_err(|err| {
            ApiError::internal(format!("Failed to load persisted agent run records: {err}"))
        })?;
    Ok(Json(super::types::AgentRunsResponse { runs }))
}

pub(crate) async fn get_agent_run(
    State(state): State<RuntimeApiState>,
    Path(run_id): Path<String>,
) -> Result<Json<crate::tools::subagent::AgentWorkerRecord>, ApiError> {
    let runs = crate::tools::subagent::load_persisted_agent_worker_records(&state.workspace)
        .map_err(|err| {
            ApiError::internal(format!("Failed to load persisted agent run records: {err}"))
        })?;
    let run = runs
        .into_iter()
        .find(|record| {
            let effective_run_id = if record.spec.run_id.is_empty() {
                record.spec.worker_id.as_str()
            } else {
                record.spec.run_id.as_str()
            };
            effective_run_id == run_id || record.spec.worker_id == run_id
        })
        .ok_or_else(|| ApiError::not_found(format!("agent run '{run_id}' not found")))?;
    Ok(Json(run))
}

// ── Helper functions ────────────────────────────────────────────────

fn open_fleet_manager(state: &RuntimeApiState) -> Result<FleetManager, ApiError> {
    let exec_config = state
        .config
        .fleet
        .as_ref()
        .map(|fleet| fleet.exec.clone())
        .unwrap_or_default();
    FleetManager::open(&state.workspace)
        .map(|manager| {
            manager
                .with_exec_config(exec_config)
                .with_sub_agent_manager(state.sub_agent_manager.clone())
        })
        .map_err(|err| ApiError::internal(format!("Failed to open fleet manager: {err}")))
}

fn fleet_run_summary_json(
    manager: &FleetManager,
    run: &FleetRun,
    ledger_state: &FleetLedgerState,
) -> Result<Value, ApiError> {
    let status = manager
        .run_status(&run.id)
        .map_err(|err| ApiError::internal(format!("Failed to read fleet run status: {err}")))?;
    let task_statuses = ledger_state
        .tasks
        .values()
        .filter(|task| task.entry.run_id == run.id)
        .map(|task| {
            json!({
                "task_id": task.entry.task_id.clone(),
                "status": fleet_task_status_label(task.status),
                "leased_to": task.leased_to.clone(),
                "attempts": task.entry.attempts,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "id": run.id.0.clone(),
        "name": run.name.clone(),
        "status": fleet_status_json(&status),
        "task_count": run.task_specs.len(),
        "worker_count": run.worker_specs.len(),
        "tasks": task_statuses,
        "labels": run.labels.clone(),
        "created_at": run.created_at.clone(),
        "updated_at": run.updated_at.clone(),
        "completed_at": run.completed_at.clone(),
    }))
}

fn fleet_run_detail_json(
    manager: &FleetManager,
    run: &FleetRun,
    ledger_state: &FleetLedgerState,
) -> Result<Value, ApiError> {
    let mut value = fleet_run_summary_json(manager, run, ledger_state)?;
    if let Some(map) = value.as_object_mut() {
        map.insert("task_specs".to_string(), json!(run.task_specs.clone()));
        map.insert("worker_specs".to_string(), json!(run.worker_specs.clone()));
    }
    Ok(value)
}

fn fleet_status_json(status: &FleetStatusSnapshot) -> Value {
    json!({
        "runs": status.runs,
        "queued": status.queued,
        "running": status.running,
        "completed": status.completed,
        "partial": status.partial,
        "failed": status.failed,
        "restarted": status.restarted,
        "escalated": status.escalated,
        "transport_failed": status.transport_failed,
        "task_failed": status.task_failed,
        "verifier_failed": status.verifier_failed,
        "cancelled": status.cancelled,
        "stale": status.stale,
        "workers": status
            .workers
            .iter()
            .map(|(worker_id, status)| {
                (
                    worker_id.clone(),
                    Value::String(worker_status_label(status).to_string()),
                )
            })
            .collect::<serde_json::Map<String, Value>>(),
    })
}

fn fleet_worker_json(inspection: &FleetWorkerInspection) -> Value {
    json!({
        "worker_id": inspection.worker_id.clone(),
        "status": worker_status_label(&inspection.status),
        "run_id": inspection.current_run_id.as_ref().map(|run_id| run_id.0.clone()),
        "task_id": inspection.current_task_id.clone(),
        "objective": inspection.objective.clone(),
        "role": inspection.role.clone(),
        "host": inspection.host.clone(),
        "latest_heartbeat_at": inspection.latest_heartbeat_at.clone(),
        "latest_event": inspection.latest_event.as_ref().map(fleet_event_json),
        "artifacts": inspection.artifacts.iter().map(fleet_artifact_json).collect::<Vec<_>>(),
        "last_error": inspection.last_error.clone(),
        "alert_state": inspection.alert_state.clone(),
        "runtime_state": inspection.runtime_state.as_ref().map(fleet_worker_runtime_json),
    })
}

fn fleet_worker_runtime_json(runtime: &FleetWorkerRuntimeProjection) -> Value {
    json!({
        "agent_status": runtime.agent_status.clone(),
        "steps_taken": runtime.steps_taken,
        "latest_message": runtime.latest_message.clone(),
        "error": runtime.error.clone(),
        "result_summary": runtime.result_summary.clone(),
        "has_session": runtime.has_session,
    })
}

fn fleet_artifact_json(artifact: &mimofan_protocol::fleet::FleetArtifactRef) -> Value {
    json!({
        "kind": artifact_kind_label(&artifact.kind),
        "path": artifact.path.clone(),
        "checksum": artifact.checksum.clone(),
        "mime_type": artifact.mime_type.clone(),
        "size_bytes": artifact.size_bytes,
    })
}

fn fleet_event_json(event: &mimofan_protocol::fleet::FleetWorkerEvent) -> Value {
    json!({
        "seq": event.seq,
        "run_id": event.run_id.0.clone(),
        "worker_id": event.worker_id.clone(),
        "task_id": event.task_id.clone(),
        "timestamp": event.timestamp.clone(),
        "label": fleet_event_label(&event.payload),
        "payload": event.payload.clone(),
    })
}

fn worker_status_label(status: &FleetWorkerStatus) -> &'static str {
    match status {
        FleetWorkerStatus::Unknown => "unknown",
        FleetWorkerStatus::Online => "online",
        FleetWorkerStatus::Busy => "busy",
        FleetWorkerStatus::Offline => "offline",
        FleetWorkerStatus::Unhealthy => "unhealthy",
        FleetWorkerStatus::Draining => "draining",
        FleetWorkerStatus::Retired => "retired",
    }
}

fn fleet_task_status_label(status: FleetTaskLedgerStatus) -> &'static str {
    match status {
        FleetTaskLedgerStatus::Enqueued => "enqueued",
        FleetTaskLedgerStatus::Leased => "leased",
        FleetTaskLedgerStatus::Completed => "completed",
        FleetTaskLedgerStatus::Failed => "failed",
        FleetTaskLedgerStatus::Cancelled => "cancelled",
    }
}

fn artifact_kind_label(kind: &FleetArtifactKind) -> String {
    match kind {
        FleetArtifactKind::Log => "log".to_string(),
        FleetArtifactKind::Patch => "patch".to_string(),
        FleetArtifactKind::TestResult => "test_result".to_string(),
        FleetArtifactKind::Report => "report".to_string(),
        FleetArtifactKind::Checkpoint => "checkpoint".to_string(),
        FleetArtifactKind::Receipt => "receipt".to_string(),
        FleetArtifactKind::Other(value) => value.clone(),
    }
}

fn fleet_event_label(payload: &FleetWorkerEventPayload) -> String {
    match payload {
        FleetWorkerEventPayload::Queued => "queued".to_string(),
        FleetWorkerEventPayload::Leased { .. } => "leased".to_string(),
        FleetWorkerEventPayload::Starting => "starting".to_string(),
        FleetWorkerEventPayload::Running => "running".to_string(),
        FleetWorkerEventPayload::ModelWait { model } => model
            .as_ref()
            .map(|model| format!("model_wait model={model}"))
            .unwrap_or_else(|| "model_wait".to_string()),
        FleetWorkerEventPayload::RunningTool { tool, call_id } => call_id
            .as_ref()
            .map(|call_id| format!("running_tool tool={tool} call_id={call_id}"))
            .unwrap_or_else(|| format!("running_tool tool={tool}")),
        FleetWorkerEventPayload::Heartbeat { .. } => "heartbeat".to_string(),
        FleetWorkerEventPayload::Artifact(artifact) => {
            format!("artifact kind={}", artifact_kind_label(&artifact.kind))
        }
        FleetWorkerEventPayload::Completed { exit_code, summary } => match (exit_code, summary) {
            (Some(code), Some(summary)) => format!("completed exit_code={code} {summary}"),
            (Some(code), None) => format!("completed exit_code={code}"),
            (None, Some(summary)) => format!("completed {summary}"),
            (None, None) => "completed".to_string(),
        },
        FleetWorkerEventPayload::Failed {
            reason,
            recoverable,
        } => {
            format!("failed recoverable={recoverable} reason={reason}")
        }
        FleetWorkerEventPayload::Cancelled { cancelled_by } => cancelled_by
            .as_ref()
            .map(|by| format!("cancelled by={by}"))
            .unwrap_or_else(|| "cancelled".to_string()),
        FleetWorkerEventPayload::Interrupted { signal } => signal
            .as_ref()
            .map(|signal| format!("interrupted signal={signal}"))
            .unwrap_or_else(|| "interrupted".to_string()),
        FleetWorkerEventPayload::Stale { last_heartbeat_at } => last_heartbeat_at
            .as_ref()
            .map(|ts| format!("stale last_heartbeat_at={ts}"))
            .unwrap_or_else(|| "stale".to_string()),
        FleetWorkerEventPayload::Restarted { restart_count } => {
            format!("restarted count={restart_count}")
        }
        FleetWorkerEventPayload::Escalated { channel, alert_id } => alert_id
            .as_ref()
            .map(|alert_id| format!("escalated channel={channel} alert_id={alert_id}"))
            .unwrap_or_else(|| format!("escalated channel={channel}")),
    }
}
