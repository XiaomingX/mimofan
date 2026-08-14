//! Pluggable sandbox backend abstraction.
//!
//! External sandbox backends route shell command execution to a remote service
//! (e.g. Alibaba OpenSandbox) instead of spawning a local process. This is
//! complementary to the OS-level sandbox module (Seatbelt / Landlock / Windows)
//! — the external backend *replaces* local execution entirely when configured.

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

/// Output from a sandbox backend execution.
#[derive(Debug, Clone)]
pub struct SandboxOutput {
    /// Standard output from the command.
    pub stdout: String,
    /// Standard error from the command.
    pub stderr: String,
    /// Exit code (0 for success).
    pub exit_code: i32,
}

/// The kind of external sandbox backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxKind {
    /// No external sandbox — execute commands locally.
    None,
    /// Alibaba OpenSandbox remote execution.
    OpenSandbox,
    /// Local OCI container (podman preferred, docker fallback) execution.
    ///
    /// Commands run inside an ephemeral, restricted container: no network by
    /// default, read-only workspace mount, non-root user, tmpfs scratch space,
    /// all capabilities dropped, and CPU/memory/timeout ceilings. Network and
    /// writable mounts must be enabled explicitly and per-item — there is no
    /// "allow all" escape hatch.
    Container,
}

impl SandboxKind {
    /// Parse a sandbox backend name from config (case-insensitive).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "" => Some(Self::None),
            "opensandbox" | "open-sandbox" | "open_sandbox" => Some(Self::OpenSandbox),
            "container" | "podman" | "docker" => Some(Self::Container),
            _ => None,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::OpenSandbox => "opensandbox",
            Self::Container => "container",
        }
    }
}

/// Abstract interface for an external sandbox backend.
///
/// Implementations send commands to a remote execution environment and return
/// structured output. The trait is `Send + Sync` so it can be stored in an
/// `Arc` and shared across async tasks.
#[async_trait]
pub trait SandboxBackend: Send + Sync {
    /// Execute a shell command and return its output.
    ///
    /// `cmd` is the full shell command string (e.g. `"ls -la"`).
    /// `env` contains additional environment variables to set.
    async fn exec(&self, cmd: &str, env: &HashMap<String, String>) -> Result<SandboxOutput>;
}

use crate::config::Config;

/// Create the configured sandbox backend from config.
///
/// Returns `None` when no external sandbox backend is configured (i.e. the
/// `sandbox_backend` key is absent, empty, or `"none"`). When `"opensandbox"`
/// is set, constructs an [`OpenSandboxBackend`](super::opensandbox::OpenSandboxBackend) using `sandbox_url` and
/// `sandbox_api_key`. When `"container"` is set, probes for a local OCI
/// runtime (podman preferred, then docker) and returns a
/// [`ContainerBackend`](super::container::ContainerBackend). If neither
/// runtime is installed, returns an error — this is **never** silently
/// downgraded to no-sandbox.
pub fn create_backend(config: &Config) -> Result<Option<Box<dyn SandboxBackend>>> {
    let kind = config
        .sandbox_backend
        .as_deref()
        .and_then(SandboxKind::parse)
        .unwrap_or(SandboxKind::None);

    match kind {
        SandboxKind::None => Ok(None),
        SandboxKind::OpenSandbox => {
            let base_url = config
                .sandbox_url
                .clone()
                .unwrap_or_else(|| "http://localhost:8080".to_string());
            let api_key = config.sandbox_api_key.clone();
            let backend = super::opensandbox::OpenSandboxBackend::new(base_url, api_key, 30)?;
            Ok(Some(Box::new(backend)))
        }
        SandboxKind::Container => {
            let backend = super::container::ContainerBackend::detect()?;
            Ok(Some(Box::new(backend)))
        }
    }
}
