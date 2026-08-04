//! Fleet command implementation extracted from `lib.rs`.

use std::time::Duration;

use anyhow::Context;

use super::*;

pub(crate) async fn run_fleet_command(
    workspace: &Path,
    config: &Config,
    args: FleetArgs,
) -> Result<()> {
    use crate::fleet::alerts::{
        FleetAlertAdapterConfig, FleetAlertConfig, FleetAlertDispatcher, FleetAlertEvent,
        FleetEnvSecretResolver,
    };
    use crate::fleet::executor::FleetExecutor;
    use crate::fleet::manager::{FleetManager, FleetStatusSnapshot, FleetWorkerInspection};
    use mimofan_protocol::fleet::{
        FleetAlertEventClass, FleetArtifactKind, FleetRunId, FleetWorkerEventPayload,
        FleetWorkerStatus,
    };

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

    fn event_label(payload: &FleetWorkerEventPayload) -> String {
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
            FleetWorkerEventPayload::Completed { exit_code, summary } => match (exit_code, summary)
            {
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

    fn print_status(status: &FleetStatusSnapshot) {
        println!(
            "fleet: runs={} queued={} running={} completed={} partial={} failed={} restarted={} escalated={} transport_failed={} task_failed={} verifier_failed={} cancelled={} stale={}",
            status.runs,
            status.queued,
            status.running,
            status.completed,
            status.partial,
            status.failed,
            status.restarted,
            status.escalated,
            status.transport_failed,
            status.task_failed,
            status.verifier_failed,
            status.cancelled,
            status.stale
        );
        if !status.workers.is_empty() {
            println!("workers:");
            for (worker_id, worker_status) in &status.workers {
                println!("  {worker_id} {}", worker_status_label(worker_status));
            }
        }
    }

    fn print_inspection(inspection: &FleetWorkerInspection) {
        println!("worker: {}", inspection.worker_id);
        println!("status: {}", worker_status_label(&inspection.status));
        if let Some(run_id) = &inspection.current_run_id {
            println!("run: {}", run_id.0);
        }
        if let Some(task_id) = &inspection.current_task_id {
            println!("task: {task_id}");
        }
        if let Some(objective) = &inspection.objective {
            println!("objective: {objective}");
        }
        if let Some(role) = &inspection.role {
            println!("role: {role}");
        }
        if let Some(host) = &inspection.host {
            println!("host: {host}");
        }
        if let Some(heartbeat) = &inspection.latest_heartbeat_at {
            println!("heartbeat: {heartbeat}");
        }
        if let Some(event) = &inspection.latest_event {
            println!(
                "latest_event: seq={} {}",
                event.seq,
                event_label(&event.payload)
            );
        }
        if !inspection.artifacts.is_empty() {
            println!("artifacts:");
            for artifact in &inspection.artifacts {
                println!(
                    "  {} {}",
                    artifact_kind_label(&artifact.kind),
                    artifact.path.display()
                );
            }
        }
        if let Some(receipt) = &inspection.receipt_summary {
            println!("receipt: {receipt}");
        }
        if let Some(error) = &inspection.last_error {
            println!("last_error: {error}");
        }
        if let Some(alert) = &inspection.alert_state {
            println!("alert: {alert}");
        }
    }

    fn print_artifacts(inspection: &FleetWorkerInspection) {
        if inspection.artifacts.is_empty() {
            println!("artifacts: none");
            return;
        }
        println!("artifacts:");
        for artifact in &inspection.artifacts {
            let size = artifact
                .size_bytes
                .map(|size| format!(" size={size}"))
                .unwrap_or_default();
            let mime = artifact
                .mime_type
                .as_ref()
                .map(|mime| format!(" mime={mime}"))
                .unwrap_or_default();
            println!(
                "  {} {}{}{}",
                artifact_kind_label(&artifact.kind),
                artifact.path.display(),
                size,
                mime
            );
        }
    }

    fn print_logs(workspace: &Path, inspection: &FleetWorkerInspection) -> Result<()> {
        let mut printed = false;
        for artifact in inspection
            .artifacts
            .iter()
            .filter(|artifact| matches!(artifact.kind, FleetArtifactKind::Log))
        {
            let path = workspace.join(&artifact.path);
            println!("== {} ==", artifact.path.display());
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("reading fleet log {}", path.display()))?;
            let preview: String = contents.chars().take(16 * 1024).collect();
            print!("{preview}");
            if contents.chars().count() > preview.chars().count() {
                println!("\n[truncated]");
            } else if !preview.ends_with('\n') {
                println!();
            }
            printed = true;
        }
        if !printed {
            println!("logs: none");
        }
        Ok(())
    }

    fn alert_event_class(arg: FleetAlertEventArg) -> FleetAlertEventClass {
        match arg {
            FleetAlertEventArg::Stale => FleetAlertEventClass::Stale,
            FleetAlertEventArg::RestartExhausted => FleetAlertEventClass::RestartExhausted,
            FleetAlertEventArg::NeedsHuman => FleetAlertEventClass::NeedsHuman,
            FleetAlertEventArg::BudgetExceeded => FleetAlertEventClass::BudgetExceeded,
            FleetAlertEventArg::VerifierFailed => FleetAlertEventClass::VerifierFailed,
            FleetAlertEventArg::RunCompleted => FleetAlertEventClass::RunCompleted,
        }
    }

    fn alert_status(class: FleetAlertEventClass, override_status: Option<String>) -> String {
        if let Some(status) = override_status {
            return status;
        }
        match class {
            FleetAlertEventClass::Stale => "stale",
            FleetAlertEventClass::RestartExhausted => "failed",
            FleetAlertEventClass::NeedsHuman => "needs_human",
            FleetAlertEventClass::BudgetExceeded => "budget_exceeded",
            FleetAlertEventClass::VerifierFailed => "verifier_failed",
            FleetAlertEventClass::RunCompleted => "completed",
        }
        .to_string()
    }

    fn alert_adapter(args: &FleetAlertDryRunArgs) -> FleetAlertAdapterConfig {
        match args.adapter {
            FleetAlertAdapterArg::Slack => FleetAlertAdapterConfig::Slack {
                webhook_env: args.slack_webhook_env.clone(),
                channel: None,
            },
            FleetAlertAdapterArg::Webhook => FleetAlertAdapterConfig::Webhook {
                url_env: args.webhook_url_env.clone(),
                secret_env: args.webhook_secret_env.clone(),
            },
            FleetAlertAdapterArg::PagerDuty => FleetAlertAdapterConfig::PagerDuty {
                routing_key_env: args.pagerduty_routing_key_env.clone(),
                severity: args.pagerduty_severity.clone(),
            },
        }
    }

    fn fleet_mimofan_binary() -> String {
        std::env::var("MIMOFAN_FLEET_MIMOFAN_BINARY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "mimofan".to_string())
    }

    let exec_config = config
        .fleet
        .as_ref()
        .map(|fleet| fleet.exec.clone())
        .unwrap_or_default();
    let manager = FleetManager::open(workspace)?.with_exec_config(exec_config);
    match args.command {
        FleetCommand::Init => {
            println!("fleet ledger: {}", manager.ledger_path().display());
            Ok(())
        }
        FleetCommand::Run(args) => {
            let max_workers = args.max_workers.clamp(1, 128);
            let manager =
                manager.with_stale_after(Duration::from_secs(args.stale_after_seconds.max(1)));
            let report = manager.create_run_from_task_spec_path(&args.task_spec, max_workers)?;
            println!(
                "fleet run: {} tasks={} leased={} queued={}",
                report.run_id.0, report.task_count, report.leased, report.queued
            );
            println!("workers:");
            for worker_id in &report.worker_ids {
                println!("  {worker_id}");
            }
            if args.once {
                print_status(&manager.run_status(&report.run_id)?);
                return Ok(());
            }
            println!(
                "manager loop running; use `mimofan fleet status`, `inspect`, `interrupt`, or `stop --all` from another terminal."
            );
            let mut executor = FleetExecutor::new(workspace);
            let mimofan_binary = fleet_mimofan_binary();
            let status = manager
                .run_to_completion(
                    &report.run_id,
                    max_workers,
                    &mut executor,
                    &mimofan_binary,
                    None,
                    Duration::from_secs(2),
                )
                .await?;
            print_status(&status);
            Ok(())
        }
        FleetCommand::Status => {
            print_status(&manager.status()?);
            Ok(())
        }
        FleetCommand::Inspect { worker_id } => {
            print_inspection(&manager.inspect_worker(&worker_id)?);
            Ok(())
        }
        FleetCommand::Logs { worker_id } => {
            let inspection = manager.inspect_worker(&worker_id)?;
            print_logs(workspace, &inspection)
        }
        FleetCommand::Artifacts { worker_id } => {
            let inspection = manager.inspect_worker(&worker_id)?;
            print_artifacts(&inspection);
            Ok(())
        }
        FleetCommand::Interrupt { worker_id } => {
            let inspection = manager.interrupt_worker(&worker_id)?;
            print_inspection(&inspection);
            Ok(())
        }
        FleetCommand::Restart { worker_id } => {
            let inspection = manager.restart_worker(&worker_id)?;
            print_inspection(&inspection);
            Ok(())
        }
        FleetCommand::Resume {
            run_id,
            stale_after_seconds,
        } => {
            let manager = manager.with_stale_after(Duration::from_secs(stale_after_seconds.max(1)));
            let report = manager.resume_run(&FleetRunId::from(run_id))?;
            println!(
                "fleet resume: {} reclaimed_stale={} restarted={} failed={} escalated={}",
                report.run_id.0,
                report.reclaimed_stale,
                report.restarted,
                report.failed,
                report.escalated
            );
            print_status(&report.status);
            Ok(())
        }
        FleetCommand::Stop { all } => {
            if !all {
                bail!("pass --all to stop all fleet work");
            }
            let stopped = manager.stop_all()?;
            println!("stopped: {stopped}");
            Ok(())
        }
        FleetCommand::AlertDryRun(args) => {
            let class = alert_event_class(args.event);
            let adapter = alert_adapter(&args);
            let event = FleetAlertEvent {
                class,
                run_id: FleetRunId::from(args.run_id.clone()),
                worker_id: args.worker_id.clone(),
                task_id: args.task_id.clone(),
                status: alert_status(class, args.status.clone()),
                reason: args.reason.clone(),
            };
            let dispatcher = FleetAlertDispatcher::new(
                FleetAlertConfig::dry_run_for_adapter(adapter),
                FleetEnvSecretResolver,
            );
            let deliveries = dispatcher.dispatch(&event)?;
            for delivery in deliveries {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&delivery.redacted_payload)?
                );
            }
            Ok(())
        }
    }
}
