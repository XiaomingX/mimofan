//! Shell-command execution for hook sinks.
//!
//! Extracted from the TUI's `tui::hooks::HookExecutor` so that any process
//! (interactive TUI or headless `core`/`app-server`) can run a user-configured
//! shell command in response to a hook event without re-implementing process
//! spawning, pipe draining, stdin delivery and timeout handling.
//!
//! [`CommandHookSink`] is the sink form: it runs a fixed shell command with the
//! event's JSON payload on stdin. [`run_shell_command`] is the lower-level,
//! synchronous, reusable primitive.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use wait_timeout::ChildExt;

use crate::{HookEvent, HookSink};

/// Outcome of running a shell hook command (mirrors the TUI's `HookResult`).
#[derive(Debug, Clone, Default)]
pub struct CommandResult {
    /// Whether the process exited successfully (exit code 0).
    pub success: bool,
    /// Numeric exit code, when the process exited normally.
    pub exit_code: Option<i32>,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Human-readable error, when spawn/wait/timing failed.
    pub error: Option<String>,
}

impl CommandResult {
    /// Return a parse of `stdout` as `KEY=VALUE` lines.
    ///
    /// Used by the `ShellEnv`-style hooks to inject environment variables into
    /// a subsequent process. Lines that do not contain `=` are ignored.
    pub fn parse_env_assignments(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        for line in self.stdout.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                if !key.is_empty() {
                    env.insert(key.to_string(), value.trim().to_string());
                }
            }
        }
        env
    }
}

/// Build the platform shell command wrapper.
///
/// Unix: `sh -c <command>`. On Windows the TUI currently uses `cmd /C`; the
/// non-Unix branch is left for a future Windows implementation and is treated
/// as a direct exec of the command string.
pub fn build_shell_command(command: &str) -> Command {
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
}

/// Spawn a pipe-reader thread that drains a child's stdout/stderr to a string.
fn spawn_pipe_reader(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = pipe.read_to_string(&mut buf);
        buf
    })
}

/// Join a pipe-reader thread, tolerating a missing handle.
fn join_reader(reader: Option<JoinHandle<String>>) -> String {
    reader.and_then(|h| h.join().ok()).unwrap_or_default()
}

/// Spawn a stdin-writer thread that delivers `bytes` to the child's stdin.
fn spawn_stdin_writer(mut stdin: std::process::ChildStdin, mut bytes: Vec<u8>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let _ = stdin.write_all(&mut bytes);
    })
}

/// Run a shell command synchronously with a timeout, env, optional stdin JSON
/// and a working directory. Mirrors the TUI's `HookExecutor::execute_sync_inner`.
///
/// The child is killed if it exceeds `timeout`. Pipe-reader threads are not
/// joined on timeout because descendant processes can inherit the pipe fds and
/// block the join indefinitely.
#[allow(clippy::too_many_arguments)]
pub fn run_shell_command(
    command: &str,
    env_vars: &HashMap<String, String>,
    stdin_json: Option<&serde_json::Value>,
    working_dir: &PathBuf,
    timeout: Duration,
) -> CommandResult {
    let stdin_bytes = match stdin_json.map(serde_json::to_vec).transpose() {
        Ok(bytes) => bytes,
        Err(e) => {
            return CommandResult {
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("Failed to encode hook stdin: {e}")),
            };
        }
    };

    let mut command = build_shell_command(command);
    command
        .current_dir(working_dir)
        .envs(env_vars)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin_bytes.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return CommandResult {
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("Failed to spawn hook: {e}")),
            };
        }
    };

    let stdout_reader = child.stdout.take().map(spawn_pipe_reader);
    let stderr_reader = child.stderr.take().map(spawn_pipe_reader);
    let _stdin_writer = match (stdin_bytes, child.stdin.take()) {
        (Some(bytes), Some(stdin)) => Some(spawn_stdin_writer(stdin, bytes)),
        _ => None,
    };

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => CommandResult {
            success: status.success(),
            exit_code: status.code(),
            stdout: join_reader(stdout_reader),
            stderr: join_reader(stderr_reader),
            error: None,
        },
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            CommandResult {
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("Hook timed out after {}s", timeout.as_secs())),
            }
        }
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            CommandResult {
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("Failed to wait for hook: {e}")),
            }
        }
    }
}

/// A [`HookSink`] that runs a fixed shell command for every emitted event,
/// passing the event's JSON payload on stdin.
///
/// This lets any [`HookDispatcher`] (in the interactive TUI or in a headless
/// process) treat a user-configured shell command as a sink alongside the
/// stdout/jsonl/webhook/unix-socket sinks. It is best-effort: failures are
/// logged via the returned [`CommandResult`] and do not abort the application.
#[derive(Clone)]
pub struct CommandHookSink {
    command: String,
    working_dir: PathBuf,
    timeout: Duration,
    env: HashMap<String, String>,
}

impl CommandHookSink {
    /// Create a new sink that runs `command` for each event.
    ///
    /// `env` is merged on top of the process environment for every invocation.
    pub fn new(
        command: impl Into<String>,
        working_dir: PathBuf,
        timeout: Duration,
        env: HashMap<String, String>,
    ) -> Self {
        Self {
            command: command.into(),
            working_dir,
            timeout,
            env,
        }
    }

    /// The shell command this sink runs.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Synchronously run the sink's command for a given event, returning the
    /// raw [`CommandResult`]. Useful when the caller needs the command's
    /// stdout (e.g. env injection or a deny decision) rather than fire-and-forget.
    pub fn run_for_event(&self, event: &HookEvent) -> CommandResult {
        run_shell_command(
            &self.command,
            &self.env,
            Some(&event.to_json()),
            &self.working_dir,
            self.timeout,
        )
    }
}

#[async_trait]
impl HookSink for CommandHookSink {
    async fn emit(&self, event: &HookEvent) -> Result<()> {
        // The shell execution itself is CPU/IO bound and short-lived; run it
        // on a blocking thread so the async runtime is not starved.
        let sink = self.clone();
        let event = event.clone();
        tokio::task::spawn_blocking(move || sink.run_for_event(&event))
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HookEvent;

    #[test]
    fn build_shell_command_is_platform_wrapper() {
        let cmd = build_shell_command("echo hi");
        // Assert the command string is preserved somewhere in argv.
        let argv: Vec<String> = cmd.get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        #[cfg(not(windows))]
        {
            assert_eq!(argv[0], "-c");
            assert!(argv.iter().any(|a| a == "echo hi"));
        }
    }

    #[test]
    fn run_shell_command_echoes_stdout() {
        let res = run_shell_command(
            "echo hello-hook",
            &HashMap::new(),
            None,
            &PathBuf::from("."),
            Duration::from_secs(5),
        );
        assert!(res.success, "expected success, got {res:?}");
        assert_eq!(res.stdout.trim(), "hello-hook");
        assert!(res.error.is_none());
    }

    #[test]
    fn run_shell_command_passes_stdin_json() {
        let res = run_shell_command(
            "cat",
            &HashMap::new(),
            Some(&serde_json::json!({"type":"session_start"})),
            &PathBuf::from("."),
            Duration::from_secs(5),
        );
        assert!(res.success, "expected success, got {res:?}");
        assert!(res.stdout.contains("session_start"), "stdout={}", res.stdout);
    }

    #[test]
    fn run_shell_command_abides_timeout() {
        let res = run_shell_command(
            "sleep 30",
            &HashMap::new(),
            None,
            &PathBuf::from("."),
            Duration::from_millis(200),
        );
        assert!(!res.success);
        assert!(res.error.as_deref().unwrap_or("").contains("timed out"));
    }

    #[test]
    fn parse_env_assignments_ignores_non_eq_lines() {
        let res = CommandResult {
            success: true,
            exit_code: Some(0),
            stdout: "FOO=bar\ncomment-line\nBAZ=qux\n".to_string(),
            stderr: String::new(),
            error: None,
        };
        let env = res.parse_env_assignments();
        assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(env.get("BAZ").map(String::as_str), Some("qux"));
        assert_eq!(env.len(), 2);
    }

    #[tokio::test]
    async fn command_sink_emits_event() {
        let sink = CommandHookSink::new(
            "echo from-sink",
            PathBuf::from("."),
            Duration::from_secs(5),
            HashMap::new(),
        );
        let event = HookEvent::ResponseStart {
            response_id: "r1".to_string(),
        };
        sink.emit(&event).await.unwrap();
    }
}
