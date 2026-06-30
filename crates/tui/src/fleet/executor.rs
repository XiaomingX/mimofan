//! Fleet executor — runs a fleet worker as a real `mimofan exec` subprocess.
//!
//! A fleet worker IS a headless `mimofan exec` run. There is no separate
//! "fleet worker" execution engine: the sub-agent runtime, full tool surface,
//! and recursion depth all come from the one `mimofan exec` runtime, so
//! fleet and sub-agents are one substrate (not two moving targets).
//!
//! This module is the bridge:
//! - [`build_worker_exec_command`] turns a `FleetTaskSpec` + `FleetExecConfig`
//!   into the `mimofan exec --output-format stream-json …` argv that a host
//!   adapter ([`super::host`]) launches locally or over SSH.
//! - [`map_exec_stream_line`] maps one stream-json line emitted by that worker
//!   into a [`FleetWorkerEventPayload`] for the durable ledger, so the ledger
//!   persists the worker's own event vocabulary instead of a simulated one.
//! - [`classify_worker_exit`] turns the process exit into a terminal event.
//!
//! The TUI/CLI/Runtime API observe the ledger's compact event stream — they
//! never render a child session, which is what keeps the orchestrator light at
//! high fanout.

#![allow(dead_code)]

use anyhow::Result;
use mimofan_config::FleetExecConfig;
use mimofan_protocol::fleet::{FleetHostSpec, FleetTaskSpec, FleetWorkerEventPayload};

use super::host::{FleetHostAdapter, FleetWorkerCommand};
use super::profile::AgentProfile;
use super::worker_runtime::{fleet_task_prompt, fleet_task_prompt_with_profiles};

/// Build the `mimofan exec` argv that runs a fleet task headlessly.
///
/// `--auto` is always passed: a headless worker has no human to approve tool
/// calls, so it runs with full (policy-gated) tool access. `--output-format
/// stream-json` makes the worker emit the NDJSON event stream this module
/// parses. Fleet recursion depth is inherited from the worker's own config
/// (`[fleet.exec] max_spawn_depth`, default [`mimofan_config::DEFAULT_SPAWN_DEPTH`]).
///
/// Secrets are NEVER placed on the argv: provider credentials are resolved by
/// the worker process from its own config/keyring exactly like an interactive
/// run. The host adapter additionally refuses secret-bearing env keys.
pub fn build_worker_exec_command(
    mimofan_binary: &str,
    task_spec: &FleetTaskSpec,
    exec_config: &FleetExecConfig,
    model: Option<&str>,
) -> FleetWorkerCommand {
    build_worker_exec_command_from_prompt(
        mimofan_binary,
        fleet_task_prompt(task_spec),
        exec_config,
        model,
    )
}

/// Build a worker command after resolving workspace Fleet profile input.
pub fn build_worker_exec_command_with_profiles(
    mimofan_binary: &str,
    task_spec: &FleetTaskSpec,
    exec_config: &FleetExecConfig,
    model: Option<&str>,
    agent_profiles: &[AgentProfile],
) -> Result<FleetWorkerCommand> {
    Ok(build_worker_exec_command_from_prompt(
        mimofan_binary,
        fleet_task_prompt_with_profiles(task_spec, agent_profiles)?,
        exec_config,
        model,
    ))
}

fn build_worker_exec_command_from_prompt(
    mimofan_binary: &str,
    task_prompt: String,
    exec_config: &FleetExecConfig,
    model: Option<&str>,
) -> FleetWorkerCommand {
    let mut args: Vec<String> = vec![
        "exec".to_string(),
        "--auto".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
    ];

    if let Some(model) = model.map(str::trim).filter(|m| !m.is_empty()) {
        args.push("--model".to_string());
        args.push(model.to_string());
    }

    if !exec_config.allowed_tools.is_empty() {
        args.push("--allowed-tools".to_string());
        args.push(exec_config.allowed_tools.join(","));
    }
    if !exec_config.disallowed_tools.is_empty() {
        args.push("--disallowed-tools".to_string());
        args.push(exec_config.disallowed_tools.join(","));
    }
    if exec_config.max_turns > 0 && exec_config.max_turns != u32::MAX {
        args.push("--max-turns".to_string());
        args.push(exec_config.max_turns.to_string());
    }
    if !exec_config.append_system_prompt.trim().is_empty() {
        args.push("--append-system-prompt".to_string());
        args.push(exec_config.append_system_prompt.clone());
    }

    // The composed task prompt is the final positional argument.
    args.push(task_prompt);

    FleetWorkerCommand::new(mimofan_binary.to_string(), args)
}

/// Map one `mimofan exec` stream-json line into a fleet ledger event.
///
/// Returns `None` for lines that don't correspond to a worker lifecycle
/// transition (e.g. `session_capture`, `metadata`). The exec event schema is
/// `{"type": "...", ...}` (see `ExecStreamEvent` in `main.rs`).
pub fn map_exec_stream_line(line: &str) -> Option<FleetWorkerEventPayload> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match value.get("type").and_then(serde_json::Value::as_str)? {
        "tool_use" => {
            let tool = value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let call_id = value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            Some(FleetWorkerEventPayload::RunningTool { tool, call_id })
        }
        // Streaming model output / tool results mean the worker is alive and
        // making progress; surface a coarse Running heartbeat.
        "content" | "tool_result" => Some(FleetWorkerEventPayload::Running),
        "done" => Some(FleetWorkerEventPayload::Completed {
            exit_code: Some(0),
            summary: None,
        }),
        "error" => {
            let reason = value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("worker reported an error")
                .to_string();
            Some(FleetWorkerEventPayload::Failed {
                reason,
                recoverable: false,
            })
        }
        _ => None,
    }
}

/// Classify a worker process exit into a terminal fleet event.
///
/// `stopped` means the operator stopped the worker (cancellation), which takes
/// precedence over the exit code.
pub fn classify_worker_exit(exit_code: Option<i32>, stopped: bool) -> FleetWorkerEventPayload {
    if stopped {
        return FleetWorkerEventPayload::Cancelled { cancelled_by: None };
    }
    match exit_code {
        Some(0) => FleetWorkerEventPayload::Completed {
            exit_code: Some(0),
            summary: None,
        },
        Some(code) => FleetWorkerEventPayload::Failed {
            reason: format!("worker exited with code {code}"),
            recoverable: true,
        },
        None => FleetWorkerEventPayload::Failed {
            reason: "worker exited without a status code".to_string(),
            recoverable: true,
        },
    }
}

/// Drives fleet workers as real `mimofan exec` subprocesses on the local
/// host, incrementally draining each worker's stream-json output into fleet
/// ledger events.
///
/// The caller (the `mimofan fleet run` loop / `FleetManager`) owns the
/// ledger; the executor owns the OS process boundary and the incremental log
/// parse. Because the worker is a separate process, its heavy runtime/tool
/// construction never touches the orchestrator — the parent only ingests a
/// compact event stream, which is what keeps it light at high fanout.
pub struct FleetExecutor {
    workspace: std::path::PathBuf,
    adapter: super::host::LocalProcessFleetHostAdapter,
    ssh_adapters: std::collections::BTreeMap<String, super::host::SshFleetHostAdapter>,
    streams: std::collections::BTreeMap<String, WorkerStream>,
}

struct WorkerStream {
    log_path: std::path::PathBuf,
    host: WorkerStreamHost,
    offset: u64,
    pending: String,
    terminal: bool,
}

enum WorkerStreamHost {
    Local,
    Ssh(String),
}

#[derive(Debug, Clone)]
pub struct FleetWorkerTerminalEvent {
    pub payload: FleetWorkerEventPayload,
    pub exit_code: Option<i32>,
}

impl FleetExecutor {
    pub fn new(workspace: impl AsRef<std::path::Path>) -> Self {
        let workspace = workspace.as_ref().to_path_buf();
        Self {
            adapter: super::host::LocalProcessFleetHostAdapter::new(&workspace),
            workspace,
            ssh_adapters: std::collections::BTreeMap::new(),
            streams: std::collections::BTreeMap::new(),
        }
    }

    /// Start a worker process and begin tracking its event stream.
    pub fn start_worker(
        &mut self,
        worker_id: &str,
        command: FleetWorkerCommand,
        cwd: Option<std::path::PathBuf>,
    ) -> super::host::FleetHostResult<super::host::FleetWorkerHandle> {
        self.start_worker_on_host(worker_id, &FleetHostSpec::Local, command, cwd)
    }

    /// Start a worker on the requested fleet host.
    pub fn start_worker_on_host(
        &mut self,
        worker_id: &str,
        host: &FleetHostSpec,
        command: FleetWorkerCommand,
        cwd: Option<std::path::PathBuf>,
    ) -> super::host::FleetHostResult<super::host::FleetWorkerHandle> {
        let mut request = super::host::FleetWorkerStartRequest::new(worker_id, command);
        request.cwd = cwd;
        let (handle, host) = match host {
            FleetHostSpec::Local => {
                let handle = self.adapter.start_worker(request)?;
                (handle, WorkerStreamHost::Local)
            }
            FleetHostSpec::Ssh { .. } => {
                let config = super::host::SshFleetHostConfig::from_host_spec(host)?;
                let key = worker_id.to_string();
                let adapter = self.ssh_adapters.entry(key.clone()).or_insert(
                    super::host::SshFleetHostAdapter::new(&self.workspace, config)?,
                );
                let handle = adapter.start_worker(request)?;
                (handle, WorkerStreamHost::Ssh(key))
            }
            FleetHostSpec::Docker { image, .. } => {
                return Err(super::host::FleetHostError {
                    kind: super::host::FleetHostErrorKind::Configuration,
                    message: format!("docker fleet workers are not wired yet (image {image})"),
                });
            }
        };
        self.streams.insert(
            worker_id.to_string(),
            WorkerStream {
                log_path: handle.log_path.clone(),
                host,
                offset: 0,
                pending: String::new(),
                terminal: false,
            },
        );
        Ok(handle)
    }

    pub fn is_tracking(&self, worker_id: &str) -> bool {
        self.streams.contains_key(worker_id)
    }

    pub fn worker_ids(&self) -> Vec<String> {
        self.streams.keys().cloned().collect()
    }

    /// Stop tracking a terminal worker so the scheduler can reuse the same
    /// logical worker id for the next queued task.
    pub fn forget_worker(&mut self, worker_id: &str) {
        let Some(stream) = self.streams.remove(worker_id) else {
            return;
        };
        match stream.host {
            WorkerStreamHost::Local => {
                let _ = self.adapter.cleanup_worker(worker_id);
            }
            WorkerStreamHost::Ssh(key) => {
                if let Some(adapter) = self.ssh_adapters.get_mut(&key) {
                    let _ = adapter.cleanup_worker(worker_id);
                }
                self.ssh_adapters.remove(&key);
            }
        }
    }

    /// Read any newly-written stream-json lines for a worker and map them to
    /// fleet ledger events. Safe to call repeatedly; only new bytes are parsed,
    /// and a trailing partial line is buffered until its newline arrives.
    pub fn drain_events(&mut self, worker_id: &str) -> Vec<FleetWorkerEventPayload> {
        let Some(stream) = self.streams.get_mut(worker_id) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let Ok(mut file) = std::fs::File::open(&stream.log_path) else {
            return events;
        };
        use std::io::{Read, Seek, SeekFrom};
        if file.seek(SeekFrom::Start(stream.offset)).is_err() {
            return events;
        }
        let mut buf = Vec::new();
        if let Ok(read) = file.read_to_end(&mut buf) {
            stream.offset += read as u64;
            stream.pending.push_str(&String::from_utf8_lossy(&buf));
            while let Some(idx) = stream.pending.find('\n') {
                let line: String = stream.pending.drain(..=idx).collect();
                if let Some(event) = map_exec_stream_line(line.trim_end()) {
                    events.push(event);
                }
            }
        }
        events
    }

    /// Poll the worker process; once it exits, return the terminal event exactly
    /// once. Returns `None` while the worker is still running or already
    /// finalized.
    pub fn poll_terminal(&mut self, worker_id: &str) -> Option<FleetWorkerEventPayload> {
        self.poll_terminal_with_status(worker_id)
            .map(|event| event.payload)
    }

    /// Poll the worker process and include the raw exit code for receipt
    /// verification.
    pub fn poll_terminal_with_status(
        &mut self,
        worker_id: &str,
    ) -> Option<FleetWorkerTerminalEvent> {
        if self.streams.get(worker_id).is_none_or(|s| s.terminal) {
            return None;
        }
        let status = match self.streams.get(worker_id).map(|s| &s.host)? {
            WorkerStreamHost::Local => self.adapter.read_status(worker_id).ok()?,
            WorkerStreamHost::Ssh(key) => self
                .ssh_adapters
                .get_mut(key)
                .and_then(|adapter| adapter.read_status(worker_id).ok())?,
        };
        let terminal = match status.state {
            super::host::FleetHostWorkerState::Running
            | super::host::FleetHostWorkerState::Unknown => return None,
            super::host::FleetHostWorkerState::Stopped => {
                classify_worker_exit(status.exit_code, true)
            }
            super::host::FleetHostWorkerState::Exited
            | super::host::FleetHostWorkerState::Failed => {
                classify_worker_exit(status.exit_code, false)
            }
        };
        if let Some(stream) = self.streams.get_mut(worker_id) {
            stream.terminal = true;
        }
        Some(FleetWorkerTerminalEvent {
            payload: terminal,
            exit_code: status.exit_code,
        })
    }

    /// True once every started worker has reached a terminal state.
    pub fn all_terminal(&self) -> bool {
        !self.streams.is_empty() && self.streams.values().all(|s| s.terminal)
    }
}

#[cfg(test)]
mod tests {}
