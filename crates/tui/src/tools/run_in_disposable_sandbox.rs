//! Disposable sandbox runner for security PoCs (T-9).
//!
//! Runs an untrusted command (e.g. a gadget-chain PoC, an exploit probe, or a
//! fuzzing harness) inside an ephemeral, restricted OCI container so a
//! suspected-vulnerability proof-of-concept can be *demonstrated* without
//! risking the host.
//!
//! This reuses the sandbox group's [`ContainerBackend`] (wired through
//! `SandboxKind::Container`) — we deliberately do **not** re-implement a
//! container runtime or the `SandboxBackend` trait. For callers that already
//! hold a backend (tests, or the OpenSandbox path), an injected
//! `&dyn SandboxBackend` is accepted so the function works without a local
//! OCI runtime.
//!
//! Hardening defaults (no network, read-only workspace, non-root, tmpfs
//! scratch, CPU/memory/timeout ceilings) are enforced by `ContainerBackend`;
//! this module only supplies the command and surfaces the backend's verdict.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::sandbox::backend::{SandboxBackend, SandboxKind};
use crate::sandbox::container::ContainerBackend;

/// Specification for a disposable sandbox run.
#[derive(Debug, Clone)]
pub struct DisposableRun {
    /// The command to execute inside the sandbox (e.g. `java PoC.java`).
    pub command: String,
    /// Workspace directory to mount read-only into the container.
    pub workspace: String,
    /// Wall-clock timeout for the command.
    pub timeout: Duration,
    /// Enable network egress (default false). Must be justified.
    pub network_enabled: bool,
    /// Audit reason when network is enabled.
    pub network_reason: Option<String>,
    /// Memory limit (e.g. "512m").
    pub memory_limit: String,
    /// CPU limit (e.g. "1.0").
    pub cpu_limit: String,
}

impl Default for DisposableRun {
    fn default() -> Self {
        DisposableRun {
            command: String::new(),
            workspace: ".".to_string(),
            timeout: Duration::from_secs(30),
            network_enabled: false,
            network_reason: None,
            memory_limit: "512m".to_string(),
            cpu_limit: "1.0".to_string(),
        }
    }
}

/// Outcome of a disposable sandbox run.
#[derive(Debug, Clone)]
pub struct DisposableResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    /// Which backend actually executed the command.
    pub backend: String,
}

/// Run `command` in a disposable sandbox.
///
/// Resolution order:
/// 1. If `injected` is `Some`, use that backend (tests / OpenSandbox path).
/// 2. Otherwise, if the configured [`SandboxKind`] is `Container`, detect and
///    use the [`ContainerBackend`].
/// 3. Otherwise return a clear error — we refuse to silently downgrade a
///    security PoC to an unsandboxed local process.
pub async fn run_in_disposable_sandbox(
    kind: SandboxKind,
    spec: &DisposableRun,
    injected: Option<&dyn SandboxBackend>,
) -> Result<DisposableResult> {
    let env: HashMap<String, String> = HashMap::new();

    enum Selected<'a> {
        Injected(&'a dyn SandboxBackend),
        Container(ContainerBackend),
    }

    let selected = if let Some(b) = injected {
        Selected::Injected(b)
    } else if kind == SandboxKind::Container {
        let cb = ContainerBackend::detect().context(
            "disposable sandbox requires a Container backend, but no OCI runtime is available",
        )?;
        Selected::Container(configure_container(cb, spec))
    } else {
        anyhow::bail!(
            "run_in_disposable_sandbox: sandbox_backend is {:?}, not Container; refusing to run \
             an untrusted PoC unsandboxed. Configure `sandbox_backend: container` or pass a backend.",
            kind
        );
    };

    let (out, name) = match selected {
        Selected::Injected(b) => (
            b.exec(&spec.command, &env)
                .await
                .map_err(|e| anyhow::anyhow!("disposable sandbox execution failed: {e}"))?,
            "injected".to_string(),
        ),
        Selected::Container(cb) => (
            cb.exec(&spec.command, &env)
                .await
                .map_err(|e| anyhow::anyhow!("disposable sandbox execution failed: {e}"))?,
            "container".to_string(),
        ),
    };

    Ok(DisposableResult {
        stdout: out.stdout,
        stderr: out.stderr,
        exit_code: out.exit_code,
        backend: name,
    })
}

/// Apply the hardening spec onto a detected `ContainerBackend`.
fn configure_container(mut cb: ContainerBackend, spec: &DisposableRun) -> ContainerBackend {
    // Network is opt-in and audited; everything else defaults to the backend's
    // safe defaults (read-only workspace, non-root, tmpfs, limits).
    if spec.network_enabled {
        cb = cb
            .with_network(spec.network_reason.clone().unwrap_or_else(|| "PoC requires egress".into()))
    }
    cb.with_timeout(spec.timeout)
        .with_memory_limit(spec.memory_limit.clone())
        .with_cpu_limit(spec.cpu_limit.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A tiny in-memory backend for tests (no real container needed).
    struct FakeBackend {
        last_cmd: Mutex<Option<String>>,
    }

    #[async_trait::async_trait]
    impl SandboxBackend for FakeBackend {
        async fn exec(
            &self,
            cmd: &str,
            _env: &HashMap<String, String>,
        ) -> Result<crate::sandbox::backend::SandboxOutput> {
            *self.last_cmd.lock().unwrap() = Some(cmd.to_string());
            Ok(crate::sandbox::backend::SandboxOutput {
                stdout: "poc-ran".into(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }

    #[tokio::test]
    async fn runs_poc_through_injected_backend() {
        let fake = FakeBackend {
            last_cmd: Mutex::new(None),
        };
        let spec = DisposableRun {
            command: "java PoC.class".into(),
            workspace: ".".into(),
            timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let res = run_in_disposable_sandbox(SandboxKind::None, &spec, Some(&fake)).await.unwrap();
        assert_eq!(res.exit_code, 0);
        assert_eq!(res.stdout, "poc-ran");
        assert_eq!(res.backend, "injected");
    }

    #[tokio::test]
    async fn refuses_unsandboxed_local_run() {
        // No injected backend and kind != Container => must error loudly.
        let spec = DisposableRun {
            command: "touch /etc/pwned".into(),
            ..Default::default()
        };
        let res = run_in_disposable_sandbox(SandboxKind::None, &spec, None).await;
        assert!(res.is_err(), "must not silently run unsandboxed");
    }
}
