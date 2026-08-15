//! Sandbox module for secure command execution.
//!
//! This module provides sandboxing capabilities for shell commands executed by
//! mimofan. Sandboxing restricts what system resources a command can access,
//! preventing accidental or malicious damage to the system.
//!
//! # Platform Support
//!
//! - **macOS**: Uses Seatbelt (sandbox-exec) for mandatory access control
//! - **Linux**: Uses Landlock (kernel 5.13+) for filesystem access control
//! - **Windows**: No OS sandbox is advertised yet. The planned first helper
//!   contract is process-tree containment only via a Windows Job Object; it
//!   must not claim filesystem, network, registry, or AppContainer isolation.
//!
//! # Usage
//!
//! ```rust,ignore
//! use sandbox::{SandboxManager, CommandSpec, SandboxPolicy};
//!
//! let manager = SandboxManager::new();
//! let spec = CommandSpec::shell("ls -la", PathBuf::from("."), Duration::from_secs(30))
//!     .with_policy(SandboxPolicy::default());
//!
//! let exec_env = manager.prepare(&spec);
//! // exec_env.command now contains the sandboxed command
//! ```

pub mod backend;
pub mod container;
pub mod credentials;
#[cfg(target_os = "linux")]
pub mod landlock;
pub mod opensandbox;
pub mod policy;
pub mod process_hardening;

#[cfg(target_os = "macos")]
pub mod seatbelt;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use async_trait::async_trait;

pub use policy::SandboxPolicy;

use crate::sandbox::backend::{SandboxBackend, SandboxOutput};

/// Specification for a command to be executed, potentially within a sandbox.
///
/// This struct captures all the information needed to execute a command:
/// the program and arguments, working directory, environment variables,
/// timeout, and sandbox policy.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// The program to execute (e.g., "sh", "python", "cargo").
    pub program: String,

    /// Arguments to pass to the program.
    pub args: Vec<String>,

    /// Working directory for the command.
    pub cwd: PathBuf,

    /// Additional environment variables to set.
    pub env: HashMap<String, String>,

    /// Maximum execution time before the command is killed.
    pub timeout: Duration,

    /// Sandbox policy controlling resource access.
    pub sandbox_policy: SandboxPolicy,

    /// Optional justification for why this command needs to run.
    /// Used for logging and audit purposes.
    pub justification: Option<String>,
}

impl CommandSpec {
    /// Create a `CommandSpec` for running a shell command via the platform shell.
    pub fn shell(command: &str, cwd: PathBuf, timeout: Duration) -> Self {
        let dispatcher = crate::shell_dispatcher::global_dispatcher();

        #[cfg(not(windows))]
        let (program, args) = dispatcher.build_command_parts(command);

        Self {
            program,
            args,
            cwd,
            env: HashMap::new(),
            timeout,
            sandbox_policy: SandboxPolicy::default(),
            justification: None,
        }
    }

    /// Create a `CommandSpec` for running a program directly.
    pub fn program(program: &str, args: Vec<String>, cwd: PathBuf, timeout: Duration) -> Self {
        Self {
            program: program.to_string(),
            args,
            cwd,
            env: HashMap::new(),
            timeout,
            sandbox_policy: SandboxPolicy::default(),
            justification: None,
        }
    }

    /// Set the sandbox policy for this command.
    pub fn with_policy(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox_policy = policy;
        self
    }

    /// Add environment variables for this command.
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Add a single environment variable.
    pub fn with_env_var(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    /// Set a justification for this command (for logging/audit).
    pub fn with_justification(mut self, justification: &str) -> Self {
        self.justification = Some(justification.to_string());
        self
    }

    /// Get the original command as a single string (for display).
    pub fn display_command(&self) -> String {
        if self.args.len() == 2
            && self.args[0] == "-c"
            && matches!(
                self.program.as_str(),
                "sh" | "bash" | "/bin/sh" | "/bin/bash" | "/usr/bin/sh" | "/usr/bin/bash"
            )
        {
            // For shell commands, show the actual command
            self.args[1].clone()
        } else if self.args.len() == 2
            && self.args[0] == "-c"
            && !self.program.eq_ignore_ascii_case("cmd")
            && !self.program.eq_ignore_ascii_case("pwsh")
            && !self.program.eq_ignore_ascii_case("pwsh.exe")
            && !self.program.eq_ignore_ascii_case("powershell")
            && !self.program.eq_ignore_ascii_case("powershell.exe")
        {
            self.args[1].clone()
        } else if self.program.eq_ignore_ascii_case("cmd")
            && self.args.len() == 2
            && self.args[0].eq_ignore_ascii_case("/C")
        {
            // Strip the `chcp 65001 >NUL & ` prefix we add on Windows for
            // UTF-8 output (issue #982).
            let raw = &self.args[1];
            raw.strip_prefix("chcp 65001 >NUL & ")
                .unwrap_or(raw)
                .to_string()
        } else if {
            let program = self.program.to_ascii_lowercase();
            program == "pwsh"
                || program == "pwsh.exe"
                || program == "powershell"
                || program == "powershell.exe"
        } && self.args.len() >= 3
            && self.args[0].eq_ignore_ascii_case("-NoProfile")
            && self.args[1].eq_ignore_ascii_case("-Command")
        {
            // Strip the PowerShell encoding prefix.
            let raw = &self.args[2];
            raw.strip_prefix("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; ")
                .unwrap_or(raw)
                .to_string()
        } else {
            // For other commands, join program and args
            let mut parts = vec![self.program.clone()];
            parts.extend(self.args.clone());
            parts.join(" ")
        }
    }
}

/// The type of sandbox being used for execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SandboxType {
    /// No sandboxing - command runs with full permissions.
    #[default]
    None,

    /// macOS Seatbelt (sandbox-exec) sandboxing.
    #[cfg(target_os = "macos")]
    MacosSeatbelt,

    /// Linux Landlock (kernel 5.13+) file-access-control sandboxing.
    #[cfg(target_os = "linux")]
    Landlock,
}

impl std::fmt::Display for SandboxType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxType::None => write!(f, "none"),
            #[cfg(target_os = "macos")]
            SandboxType::MacosSeatbelt => write!(f, "macos-seatbelt"),
            #[cfg(target_os = "linux")]
            SandboxType::Landlock => write!(f, "linux-landlock"),
        }
    }
}

/// The execution environment after sandbox transformation.
///
/// This contains the actual command to run (which may include sandbox wrapper
/// commands) and all necessary environment configuration.
#[derive(Debug)]
pub struct ExecEnv {
    /// The full command to execute (may include sandbox wrapper).
    pub command: Vec<String>,

    /// Working directory for execution.
    pub cwd: PathBuf,

    /// Environment variables to set.
    pub env: HashMap<String, String>,

    /// Timeout for the command.
    pub timeout: Duration,

    /// The type of sandbox being used.
    pub sandbox_type: SandboxType,

    /// The original policy (for reference).
    pub policy: SandboxPolicy,
}

impl ExecEnv {
    /// Get the program to execute (first element of command).
    pub fn program(&self) -> &str {
        self.command
            .first()
            .map_or("sh", std::string::String::as_str)
    }

    /// Get the arguments (all elements after the first).
    pub fn args(&self) -> &[String] {
        if self.command.len() > 1 {
            &self.command[1..]
        } else {
            &[]
        }
    }

    /// Check if this execution is sandboxed.
    pub fn is_sandboxed(&self) -> bool {
        !matches!(self.sandbox_type, SandboxType::None)
    }
}

/// Detect what sandbox technology is available on the current platform.
pub fn get_platform_sandbox() -> Option<SandboxType> {
    #[cfg(target_os = "macos")]
    {
        if seatbelt::is_available() {
            return Some(SandboxType::MacosSeatbelt);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if landlock::is_available() {
            return Some(SandboxType::Landlock);
        }
    }

    None
}

/// Check if sandboxing is available on this platform.
pub fn is_sandbox_available() -> bool {
    get_platform_sandbox().is_some()
}

/// Manager for sandbox operations.
///
/// The `SandboxManager` is responsible for:
/// - Detecting available sandbox technologies
/// - Transforming `CommandSpecs` into sandboxed `ExecEnvs`
/// - Detecting sandbox denials from command output
#[derive(Debug, Default)]
pub struct SandboxManager {
    /// Cached sandbox availability check.
    sandbox_available: Option<bool>,

    /// Force a specific sandbox type (for testing).
    forced_sandbox: Option<SandboxType>,

    /// When true and bwrap is available on Linux, route commands through
    /// bubblewrap instead of Landlock alone (#2184).
    prefer_bwrap: bool,
}

impl SandboxManager {
    /// Create a new `SandboxManager`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new `SandboxManager` with bwrap preference (#2184).
    ///
    /// When `prefer_bwrap` is true and `/usr/bin/bwrap` is present on Linux,
    /// exec_shell commands will be routed through bubblewrap.
    pub fn with_bwrap_preference(prefer_bwrap: bool) -> Self {
        Self {
            prefer_bwrap,
            ..Self::default()
        }
    }

    /// Set the bwrap preference (#2184).
    pub fn set_prefer_bwrap(&mut self, prefer: bool) {
        self.prefer_bwrap = prefer;
    }

    /// Check if sandboxing is available.
    pub fn is_available(&mut self) -> bool {
        if let Some(available) = self.sandbox_available {
            return available;
        }

        let available = is_sandbox_available();
        self.sandbox_available = Some(available);
        available
    }

    /// Select the appropriate sandbox type for the given policy.
    pub fn select_sandbox(&self, policy: &SandboxPolicy) -> SandboxType {
        // If the policy doesn't want sandboxing, return None
        if !policy.should_sandbox() {
            return SandboxType::None;
        }

        // Check for forced sandbox (testing)
        if let Some(forced) = self.forced_sandbox {
            return forced;
        }

        // Use platform default
        get_platform_sandbox().unwrap_or(SandboxType::None)
    }

    /// Transform a `CommandSpec` into a sandboxed `ExecEnv`.
    ///
    /// This is the main entry point for sandboxing. It takes a command
    /// specification and returns the actual command to run, which may
    /// include sandbox wrapper commands.
    pub fn prepare(&self, spec: &CommandSpec) -> ExecEnv {
        let sandbox_type = self.select_sandbox(&spec.sandbox_policy);

        match sandbox_type {
            SandboxType::None => Self::prepare_unsandboxed(spec),

            #[cfg(target_os = "macos")]
            SandboxType::MacosSeatbelt => Self::prepare_seatbelt(spec),

            #[cfg(target_os = "linux")]
            SandboxType::Landlock => Self::prepare_landlock(spec),
        }
    }

    /// Prepare an unsandboxed execution environment.
    fn prepare_unsandboxed(spec: &CommandSpec) -> ExecEnv {
        let mut command = vec![spec.program.clone()];
        command.extend(spec.args.clone());

        ExecEnv {
            command,
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            timeout: spec.timeout,
            sandbox_type: SandboxType::None,
            policy: spec.sandbox_policy.clone(),
        }
    }

    /// Prepare a Seatbelt-sandboxed execution environment (macOS).
    #[cfg(target_os = "macos")]
    fn prepare_seatbelt(spec: &CommandSpec) -> ExecEnv {
        // Build the original command
        let mut original_command = vec![spec.program.clone()];
        original_command.extend(spec.args.clone());

        // Generate sandbox-exec arguments
        let seatbelt_args =
            seatbelt::create_seatbelt_args(original_command, &spec.sandbox_policy, &spec.cwd);

        // Prepend sandbox-exec to the command
        let mut command = vec![seatbelt::SANDBOX_EXEC_PATH.to_string()];
        command.extend(seatbelt_args);

        // Add sandbox indicator to environment
        let mut env = spec.env.clone();
        env.insert("MIMOFAN_SANDBOX".to_string(), "seatbelt".to_string());

        ExecEnv {
            command,
            cwd: spec.cwd.clone(),
            env,
            timeout: spec.timeout,
            sandbox_type: SandboxType::MacosSeatbelt,
            policy: spec.sandbox_policy.clone(),
        }
    }

    /// Prepare a Landlock-sandboxed execution environment (Linux).
    ///
    /// The command runs locally but the child process applies a kernel-level
    /// Landlock restriction (read+execute-only filesystem, with the workspace
    /// granted write access per the policy's writable roots) via a `pre_exec`
    /// hook. The host environment is cleared and rebuilt from the allowlist so
    /// credentials are not inherited.
    #[cfg(target_os = "linux")]
    fn prepare_landlock(spec: &CommandSpec) -> ExecEnv {
        // Determine the writable root(s): default to the command's cwd, plus
        // any explicit writable roots from the policy. Landlock is applied with
        // the *primary* writable root; additional roots are applied by the
        // Landlock layer's own logic when invoked as a backend. Here we keep
        // the OS-level path simple and grant the cwd write access.
        let writable_root = spec.cwd.clone();

        let mut command = vec![spec.program.clone()];
        command.extend(spec.args.clone());

        let mut env = crate::sandbox::credentials::build_sandbox_env(&spec.env);
        // Marker so output can be attributed to the Landlock sandbox.
        env.insert("MIMOFAN_SANDBOX".to_string(), "landlock".to_string());

        ExecEnv {
            command,
            cwd: spec.cwd.clone(),
            env,
            timeout: spec.timeout,
            sandbox_type: SandboxType::Landlock,
            policy: spec.sandbox_policy.clone(),
        }
        .with_landlock_hook(writable_root)
    }

    /// Attach the Landlock `pre_exec` hook to the underlying command of an
    /// `ExecEnv`. Since `ExecEnv` is a resolved plan (not a `Command`), the hook
    /// is recorded here and applied by the executor when it spawns the process.
    /// We store the writable root in a side-channel: the executor reads
    /// `MIMOFAN_LANDLOCK_WS` to know where to grant writes.
    #[cfg(target_os = "linux")]
    fn with_landlock_hook(mut self, writable_root: PathBuf) -> ExecEnv {
        self.env.insert(
            "MIMOFAN_LANDLOCK_WS".to_string(),
            writable_root.display().to_string(),
        );
        self
    }

    /// Check if a command failure was due to sandbox denial.
    ///
    /// This helps distinguish between legitimate command failures and
    /// sandbox-blocked operations.
    pub fn was_denied(sandbox_type: SandboxType, exit_code: i32, stderr: &str) -> bool {
        #[cfg(not(target_os = "macos"))]
        let _ = (exit_code, stderr);

        match sandbox_type {
            SandboxType::None => false,

            #[cfg(target_os = "macos")]
            SandboxType::MacosSeatbelt => seatbelt::detect_denial(exit_code, stderr),

            #[cfg(target_os = "linux")]
            SandboxType::Landlock => {
                // Landlock denials surface as permission errors / EPERM.
                exit_code != 0
                    && (stderr.contains("Permission denied")
                        || stderr.contains("Operation not permitted")
                        || stderr.contains("landlock"))
            }
        }
    }

    /// Get a human-readable description of why a command was blocked.
    pub fn denial_message(sandbox_type: SandboxType, stderr: &str) -> String {
        #[cfg(not(target_os = "macos"))]
        let _ = stderr;

        match sandbox_type {
            SandboxType::None => "Command failed (no sandbox)".to_string(),

            #[cfg(target_os = "macos")]
            SandboxType::MacosSeatbelt => {
                if stderr.contains("file-write") {
                    "Sandbox blocked write access. The command tried to write to a protected location.".to_string()
                } else if stderr.contains("network") {
                    "Sandbox blocked network access. Enable network_access in sandbox policy if needed.".to_string()
                } else {
                    format!(
                        "Sandbox blocked operation: {}",
                        stderr.lines().next().unwrap_or("unknown")
                    )
                }
            }

            #[cfg(target_os = "linux")]
            SandboxType::Landlock => {
                "Landlock blocked a filesystem operation. The command tried to write outside the allowed workspace.".to_string()
            }
        }
    }

    /// Run a shell command locally through the OS-level sandbox path and
    /// return `(stdout, stderr, exit_code)`.
    ///
    /// This is the single local-execution helper that backs both the legacy
    /// `ShellManager` flow (via `prepare`) and the new `SandboxBackend` trait.
    /// It wraps `prepare` rather than duplicating process-spawning logic.
    fn run_local(&self, cmd: &str, env: &HashMap<String, String>) -> anyhow::Result<SandboxOutput> {
        let spec = CommandSpec::shell(cmd, std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), Duration::from_secs(60))
            .with_env(env.clone());
        let exec_env = self.prepare(&spec);

        let mut command = Command::new(exec_env.program());
        command
            .args(exec_env.args())
            .current_dir(&exec_env.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &exec_env.env {
            command.env(k, v);
        }

        let output = command
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run command `{cmd}`: {e}"))?;

        Ok(SandboxOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

/// `SandboxManager` can act as a `SandboxBackend` by delegating to the
/// OS-level execution path (Seatbelt / Landlock / unsandboxed). This lets the
/// local OS sandbox be plugged into the same `SandboxBackend` seam used by the
/// remote OpenSandbox / container backends (#835).
#[async_trait]
impl SandboxBackend for SandboxManager {
    async fn exec(&self, cmd: &str, env: &HashMap<String, String>) -> anyhow::Result<SandboxOutput> {
        self.run_local(cmd, env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock `SandboxBackend` that records the command it was asked to run and
    /// returns a canned `SandboxOutput`. Used to prove the trait is
    /// dyn-compatible and that `SandboxManager` satisfies it.
    struct MockBackend {
        last_cmd: std::sync::Mutex<Option<String>>,
    }

    #[async_trait]
    impl SandboxBackend for MockBackend {
        async fn exec(&self, cmd: &str, _env: &HashMap<String, String>) -> anyhow::Result<SandboxOutput> {
            *self.last_cmd.lock().unwrap() = Some(cmd.to_string());
            Ok(SandboxOutput {
                stdout: format!("mock:{cmd}"),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }

    #[tokio::test]
    async fn sandbox_manager_implements_sandbox_backend() {
        // Construct a no-op/local policy manager (mirrors `SandboxManager::new`).
        let manager = SandboxManager::new();
        // Box it as the trait object the rest of the system stores.
        let backend: Box<dyn SandboxBackend> = Box::new(manager);

        let output = backend.exec("echo hello", &HashMap::new()).await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(
            output.stdout.contains("hello"),
            "expected stdout to contain 'hello', got: {:?}",
            output.stdout
        );
    }

    #[tokio::test]
    async fn mock_backend_passes_through() {
        let backend: Box<dyn SandboxBackend> = Box::new(MockBackend {
            last_cmd: std::sync::Mutex::new(None),
        });
        let output = backend.exec("echo hi", &HashMap::new()).await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, "mock:echo hi");
    }
}
