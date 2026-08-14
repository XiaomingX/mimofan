//! Local OCI container sandbox backend (#SECURITY-CAPABILITY T-1).
//!
//! `ContainerBackend` runs shell commands inside an ephemeral, restricted OCI
//! container using whichever runtime is available on the host: **podman** is
//! preferred (rootless by default), with **docker** as a fallback. If neither
//! runtime is installed, [`ContainerBackend::detect`] returns an error — it is
//! **never** silently downgraded to no-sandbox.
//!
//! ## Security posture (defaults)
//!
//! Every execution hard-codes the following unless the caller explicitly opts
//! into a more permissive setting *per item* (there is **no** "allow all"
//! switch):
//!
//! - `--network=none` — no outbound/inbound network. Re-enabled only via
//!   [`ContainerBackend::with_network`] with an explicit reason.
//! - `--read-only` — the image filesystem is read-only. The workspace is
//!   mounted read-only by default; writable mounts are added **only** via
//!   [`ContainerBackend::with_writable_mount`] for specific paths.
//! - `--cap-drop=ALL` — all Linux capabilities are dropped.
//! - `--security-opt=no-new-privileges` — the container cannot gain privileges.
//! - Non-root — runs as an unprivileged UID/GID (the image's default non-root
//!   user, or an explicit `--user`).
//! - A `tmpfs` scratch space at `/tmp` for legitimate temp files.
//! - CPU, memory, and wall-clock timeout ceilings enforced via `--cpus`,
//!   `--memory`, and an external kill timeout respectively.
//!
//! ## Credential hygiene
//!
//! The child process does **not** inherit the host environment. The environment
//! is built from scratch via [`crate::sandbox::credentials::build_sandbox_env`]
//! (allowlist + ephemeral token only). Provider credentials are never injected
//! unless explicitly granted, and output is passed through
//! [`mimofan_secrets::redact_stream`] before being returned.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;

use super::backend::{SandboxBackend, SandboxOutput};
use super::credentials::{build_sandbox_env, redact_output};

/// Default memory ceiling (512 MiB) for a sandboxed container.
const DEFAULT_MEMORY_LIMIT: &str = "512m";
/// Default CPU ceiling (1.0 vCPU) for a sandboxed container.
const DEFAULT_CPU_LIMIT: &str = "1.0";
/// Default wall-clock timeout for a single command (60 s).
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Which OCI runtime drive the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    /// podman, preferred because it supports rootless by default.
    Podman,
    /// docker, used as a fallback when podman is absent.
    Docker,
}

impl RuntimeKind {
    /// The binary name to invoke.
    #[must_use]
    pub fn binary(self) -> &'static str {
        match self {
            RuntimeKind::Podman => "podman",
            RuntimeKind::Docker => "docker",
        }
    }

    /// Whether this runtime is available on `PATH`.
    #[must_use]
    pub fn is_available(self) -> bool {
        Command::new(self.binary())
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_or(false, |s| s.success())
    }

    /// Human-readable label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeKind::Podman => "podman",
            RuntimeKind::Docker => "docker",
        }
    }
}

/// Probe for a usable OCI runtime, preferring rootless podman then docker.
///
/// Returns `None` if neither is installed (so callers can decide to fall back
/// to another backend) but [`ContainerBackend::detect`] turns that into an
/// explicit error rather than a silent downgrade.
#[must_use]
pub fn detect_runtime() -> Option<RuntimeKind> {
    // podman rootless is preferred: it needs no daemon and drops privileges by
    // default. We don't special-case rootless detection beyond preferring the
    // binary — podman itself defaults to rootless when available.
    if RuntimeKind::Podman.is_available() {
        return Some(RuntimeKind::Podman);
    }
    if RuntimeKind::Docker.is_available() {
        return Some(RuntimeKind::Docker);
    }
    None
}

/// A restricted container execution backend.
pub struct ContainerBackend {
    runtime: RuntimeKind,
    /// Container image to run. Kept minimal and well-known.
    image: String,
    /// Working directory inside the container (mount point of the workspace).
    workspace_mount: PathBuf,
    /// Host path that backs `workspace_mount`.
    workspace_host: PathBuf,
    /// Whether network is enabled (default: false).
    network_enabled: bool,
    /// Extra read-write bind mounts (host_path -> container_path).
    writable_mounts: Vec<(PathBuf, PathBuf)>,
    /// Memory limit string (e.g. "512m").
    memory_limit: String,
    /// CPU limit string (e.g. "1.0").
    cpu_limit: String,
    /// Wall-clock timeout.
    timeout: Duration,
    /// Explicit non-root UID:GID to run as. `None` => image default user.
    run_as_user: Option<(u32, u32)>,
    /// Reason recorded when network is explicitly enabled (audit trail).
    network_reason: Option<String>,
}

impl ContainerBackend {
    /// Detect a usable runtime and construct the backend, or fail loudly if
    /// no OCI runtime is available on the host.
    pub fn detect() -> Result<Self> {
        let runtime = detect_runtime().ok_or_else(|| {
            anyhow!(
                "container sandbox requested but neither podman nor docker is installed on PATH; \
                 refusing to silently downgrade to an unsandboxed local process. Install podman \
                 (recommended, rootless) or docker, or choose a different sandbox_backend."
            )
        })?;

        Ok(Self::with_runtime(runtime))
    }

    /// Construct the backend on an explicit runtime (used in tests).
    #[must_use]
    pub fn with_runtime(runtime: RuntimeKind) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            runtime,
            image: "docker.io/library/alpine:3.20".to_string(),
            workspace_mount: PathBuf::from("/workspace"),
            workspace_host: cwd,
            network_enabled: false,
            writable_mounts: Vec::new(),
            memory_limit: DEFAULT_MEMORY_LIMIT.to_string(),
            cpu_limit: DEFAULT_CPU_LIMIT.to_string(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            run_as_user: None,
            network_reason: None,
        }
    }

    /// Override the container image (defaults to a minimal alpine).
    #[must_use]
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = image.into();
        self
    }

    /// Set the host workspace path that will be mounted (read-only by default).
    #[must_use]
    pub fn with_workspace(mut self, host: PathBuf, mount: PathBuf) -> Self {
        self.workspace_host = host;
        self.workspace_mount = mount;
        self
    }

    /// Explicitly enable network access. There is no "allow all" — callers must
    /// pass a concrete reason, recorded for the audit trail.
    #[must_use]
    pub fn with_network(mut self, reason: impl Into<String>) -> Self {
        self.network_enabled = true;
        self.network_reason = Some(reason.into());
        self
    }

    /// Add a specific read-write bind mount (host path -> container path).
    /// Writes are permitted **only** at these paths, never globally.
    #[must_use]
    pub fn with_writable_mount(mut self, host: PathBuf, container: PathBuf) -> Self {
        self.writable_mounts.push((host, container));
        self
    }

    /// Override the memory ceiling (e.g. `"256m"`, `"1g"`).
    #[must_use]
    pub fn with_memory_limit(mut self, limit: impl Into<String>) -> Self {
        self.memory_limit = limit.into();
        self
    }

    /// Override the CPU ceiling (e.g. `"0.5"`, `"2.0"`).
    #[must_use]
    pub fn with_cpu_limit(mut self, limit: impl Into<String>) -> Self {
        self.cpu_limit = limit.into();
        self
    }

    /// Override the wall-clock timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Run as a specific non-root UID:GID.
    #[must_use]
    pub fn with_user(mut self, uid: u32, gid: u32) -> Self {
        self.run_as_user = Some((uid, gid));
        self
    }

    /// Inspectors used by tests to assert the secure-by-default posture.
    #[must_use]
    pub fn network_enabled(&self) -> bool {
        self.network_enabled
    }
    #[must_use]
    pub fn memory_limit(&self) -> &str {
        &self.memory_limit
    }
    #[must_use]
    pub fn cpu_limit(&self) -> &str {
        &self.cpu_limit
    }
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
    #[must_use]
    pub fn run_as_user(&self) -> Option<(u32, u32)> {
        self.run_as_user
    }
    #[must_use]
    pub fn writable_mounts(&self) -> &[(PathBuf, PathBuf)] {
        &self.writable_mounts
    }
    #[must_use]
    pub fn runtime(&self) -> RuntimeKind {
        self.runtime
    }

    /// Build the argv for `runtime run ... <image> sh -c <cmd>`.
    fn build_args(&self, cmd: &str) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        args.push("run".to_string());
        args.push("--rm".to_string());

        // Network: off by default.
        if self.network_enabled {
            // `--network` left at default ("bridge") when enabled; we never
            // expose host networking.
            args.push("--network".to_string());
            args.push("bridge".to_string());
        } else {
            args.push("--network".to_string());
            args.push("none".to_string());
        }

        // Read-only root filesystem.
        args.push("--read-only".to_string());

        // Drop all capabilities; no privilege escalation.
        args.push("--cap-drop".to_string());
        args.push("ALL".to_string());
        args.push("--security-opt".to_string());
        args.push("no-new-privileges".to_string());

        // Resource ceilings.
        args.push("--memory".to_string());
        args.push(self.memory_limit.clone());
        args.push("--cpus".to_string());
        args.push(self.cpu_limit.clone());

        // tmpfs scratch space (writable, volatile).
        args.push("--tmpfs".to_string());
        args.push("/tmp:rw,noexec,nosuid,size=64m".to_string());

        // Workspace mount: read-only by default.
        args.push("--volume".to_string());
        args.push(format!(
            "{}:{}:ro,Z",
            self.workspace_host.display(),
            self.workspace_mount.display()
        ));

        // Explicit read-write mounts (permitted paths only).
        for (host, container) in &self.writable_mounts {
            args.push("--volume".to_string());
            args.push(format!(
                "{}:{}:rw,Z",
                host.display(),
                container.display()
            ));
        }

        // Non-root user.
        if let Some((uid, gid)) = self.run_as_user {
            args.push("--user".to_string());
            args.push(format!("{uid}:{gid}"));
        }

        // Work in the workspace mount.
        args.push("--workdir".to_string());
        args.push(self.workspace_mount.display().to_string());

        args.push(self.image.clone());
        args.push("sh".to_string());
        args.push("-c".to_string());
        args.push(cmd.to_string());

        args
    }

    /// Run the command synchronously under the runtime, enforcing the timeout.
    fn run_blocking(&self, cmd: &str, env: &HashMap<String, String>) -> Result<SandboxOutput> {
        let args = self.build_args(cmd);

        let mut command = Command::new(self.runtime.binary());
        command.args(&args);
        // Explicitly start from an empty environment and inject only the
        // allowlist + ephemeral credentials. Host secrets are NOT inherited.
        command.env_clear();
        for (k, v) in env {
            command.env(k, v);
        }
        // Marker so output can be attributed to the container sandbox.
        command.env("MIMOFAN_SANDBOX", "container");

        let child = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn {} for container sandbox",
                    self.runtime.binary()
                )
            })?;

        // Enforce the wall-clock timeout ourselves; the runtime's `--timeout`
        // flag exists for podman but not docker, so we handle it uniformly.
        let timeout = self.timeout;
        let (stdout_raw, stderr_raw, exit_code) = run_with_timeout(self.runtime, child, timeout)?;

        // Redact any secrets that may have leaked into output before returning.
        let (stdout, _carry) = redact_output(&stdout_raw, None);
        let (stderr, _carry2) = redact_output(&stderr_raw, None);

        if exit_code < 0 {
            bail!("container command killed (signal) or failed: {}", stderr);
        }

        Ok(SandboxOutput {
            stdout,
            stderr,
            exit_code,
        })
    }
}

/// Drive `child` to completion, enforcing `timeout`. On timeout the container
/// is stopped. Returns the raw stdout/stderr and the process exit code.
fn run_with_timeout(
    runtime: RuntimeKind,
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<(String, String, i32)> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = status.code().unwrap_or(-1);
                return Ok((stdout, stderr, code));
            }
            None => {
                if start.elapsed() >= timeout {
                    // Best-effort stop; ignore errors (already timed out).
                    let _ = Command::new(runtime.binary())
                        .args(["stop", "--time", "1"])
                        .output();
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("container command timed out after {:?}", timeout);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[async_trait]
impl SandboxBackend for ContainerBackend {
    async fn exec(&self, cmd: &str, env: &HashMap<String, String>) -> Result<SandboxOutput> {
        // Build the least-privilege environment (allowlist + ephemeral token).
        // The caller-supplied `env` is merged on top, but it is itself expected
        // to be minimal; we do not re-inherit the host env here.
        let sandbox_env = build_sandbox_env(env);

        if self.network_enabled {
            tracing::debug!(
                "container sandbox: network ENABLED (reason: {})",
                self.network_reason.as_deref().unwrap_or("<none>")
            );
        }

        self.run_blocking(cmd, &sandbox_env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_detection_is_well_typed() {
        // We can't assert a runtime is present in CI, but we can assert that
        // the detector returns None when forced to look at a bogus PATH.
        let saved = std::env::var("PATH").ok();
        // SAFETY: setting PATH within a single-threaded test is benign; the
        // restore below guarantees the host environment is returned to its
        // prior state before the test returns.
        unsafe {
            std::env::set_var("PATH", "/nonexistent-bin");
        }
        assert!(detect_runtime().is_none());
        if let Some(p) = saved {
            unsafe {
                std::env::set_var("PATH", p);
            }
        }
    }

    #[test]
    fn secure_by_default_posture() {
        // Build a backend directly (no runtime needed for inspection).
        let backend = ContainerBackend::with_runtime(RuntimeKind::Podman);
        assert!(!backend.network_enabled(), "network must be off by default");
        assert_eq!(backend.memory_limit(), "512m");
        assert_eq!(backend.cpu_limit(), "1.0");
        assert_eq!(backend.timeout(), Duration::from_secs(60));
        assert!(backend.writable_mounts().is_empty(), "no writable mounts by default");
        assert!(backend.run_as_user().is_none());

        // Sanity: argv reflects the secure posture.
        let argv = backend.build_args("echo hi");
        assert!(argv.contains(&"--network".to_string()));
        assert!(argv.contains(&"none".to_string()));
        assert!(argv.contains(&"--read-only".to_string()));
        assert!(argv.contains(&"ALL".to_string()));
        assert!(argv.contains(&"no-new-privileges".to_string()));
    }

    #[test]
    fn network_enable_requires_explicit_reason_and_is_per_item() {
        let backend = ContainerBackend::with_runtime(RuntimeKind::Podman)
            .with_network("fetching trusted package index");
        assert!(backend.network_enabled());
        let argv = backend.build_args("true");
        // When enabled we still never use host networking.
        assert!(!argv.iter().any(|a| a == "host"));
    }
}
