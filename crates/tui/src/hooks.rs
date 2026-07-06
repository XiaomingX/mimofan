//! Hooks system for `DeepSeek` CLI
//!
//! Provides lifecycle hooks that execute user-defined shell commands at:
//! - Session start/end
//! - Tool call before/after

//! - Mode changes
//! - Message submission
//! - Error events
//! - Turn completion
//!
//! Configuration is done via `[[hooks.hooks]]` in config.toml.

// Note: anyhow is available if needed for future error handling
#[allow(unused_imports)]
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

/// Events that can trigger hook execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Triggered when a new session starts
    SessionStart,
    /// Triggered when a session ends (quit, Ctrl+C)
    SessionEnd,
    /// Triggered before a user message is sent to the LLM
    MessageSubmit,
    /// Triggered before a tool is executed
    ToolCallBefore,
    /// Triggered after a tool completes (success or failure)
    ToolCallAfter,
    /// Triggered when the user changes modes (Plan, Agent, Yolo)
    ModeChange,
    /// Triggered when an error occurs
    OnError,
    /// Triggered after a turn completes and post-turn state has been updated
    TurnEnd,
    /// Triggered when a sub-agent is spawned
    SubagentSpawn,
    /// Triggered when a sub-agent reaches a terminal state
    SubagentComplete,
    /// Triggered immediately before each `exec_shell` invocation. The hook's
    /// stdout is parsed as `KEY=VALUE\n` lines and merged on top of the
    /// shell command's environment — useful for ephemeral credentials,
    /// per-skill PATH adjustments, or short-lived tokens (#456). Hooks that
    /// fail or time out are logged but do *not* abort the shell call; they
    /// simply contribute no env vars.
    ShellEnv,
}

impl HookEvent {
    /// Get string representation for environment variable
    #[allow(dead_code)] // Used in tests and future hook dispatch
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "session_start",
            HookEvent::SessionEnd => "session_end",
            HookEvent::MessageSubmit => "message_submit",
            HookEvent::ToolCallBefore => "tool_call_before",
            HookEvent::ToolCallAfter => "tool_call_after",
            HookEvent::ModeChange => "mode_change",
            HookEvent::OnError => "on_error",
            HookEvent::TurnEnd => "turn_end",
            HookEvent::SubagentSpawn => "subagent_spawn",
            HookEvent::SubagentComplete => "subagent_complete",
            HookEvent::ShellEnv => "shell_env",
        }
    }
}

/// Condition for when a hook should run
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum HookCondition {
    /// Always run this hook
    #[default]
    Always,
    /// Only run for specific tool names
    ToolName {
        /// Tool name to match (e.g., "`exec_shell`", "`write_file`")
        name: String,
    },
    /// Only run for specific tool categories
    ToolCategory {
        /// Category: "safe", "`file_write`", "shell"
        category: String,
    },
    /// Only run in specific modes
    Mode {
        /// Mode: "plan", "agent", "yolo"
        mode: String,
    },
    /// Only run when exit code matches (for `ToolCallAfter`)
    ExitCode {
        /// Exit code to match
        code: i32,
    },
    /// Combine multiple conditions with AND
    All { conditions: Vec<HookCondition> },
    /// Combine multiple conditions with OR
    Any { conditions: Vec<HookCondition> },
}

/// A single hook definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    /// The event that triggers this hook
    pub event: HookEvent,

    /// Shell command to execute (platform shell: `sh -c` on Unix, `cmd /C` on Windows)
    pub command: String,

    /// Optional condition for when this hook should run
    #[serde(default)]
    pub condition: Option<HookCondition>,

    /// Timeout in seconds (default: 30)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Run in background (don't wait for completion)
    #[serde(default)]
    pub background: bool,

    /// Continue if this hook fails (default: true)
    #[serde(default = "default_continue_on_error")]
    pub continue_on_error: bool,

    /// Optional name for logging/debugging
    #[serde(default)]
    pub name: Option<String>,
}

fn default_timeout() -> u64 {
    30
}
fn default_continue_on_error() -> bool {
    true
}

impl Hook {
    /// Create a new hook with minimal configuration
    #[allow(dead_code)] // Public builder API, used in tests
    pub fn new(event: HookEvent, command: &str) -> Self {
        Self {
            event,
            command: command.to_string(),
            condition: None,
            timeout_secs: 30,
            background: false,
            continue_on_error: true,
            name: None,
        }
    }

    /// Builder: set condition
    #[allow(dead_code)] // Public builder API, used in tests
    pub fn with_condition(mut self, condition: HookCondition) -> Self {
        self.condition = Some(condition);
        self
    }

    /// Builder: set timeout
    #[allow(dead_code)] // Public builder API, used in tests
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Builder: run in background
    #[allow(dead_code)] // Public builder API, used in tests
    pub fn background(mut self) -> Self {
        self.background = true;
        self
    }

    /// Builder: set name
    #[allow(dead_code)] // Public builder API, used in tests
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }
}

/// Configuration for hooks (loaded from config.toml)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    /// List of hooks to execute
    #[serde(default)]
    pub hooks: Vec<Hook>,

    /// Global enable/disable for all hooks
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Global timeout override (applies if hook doesn't specify one)
    #[serde(default)]
    pub default_timeout_secs: Option<u64>,

    /// Working directory for hook execution (default: workspace)
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

fn default_enabled() -> bool {
    true
}

impl HooksConfig {
    /// Load global hooks merged with project-local `.mimofan/hooks.toml` (#3026).
    ///
    /// Project hooks are executable repository configuration, so they are only
    /// honored after the workspace has been trusted in user-owned config.
    /// Trusted project hooks are appended after global hooks.  A malformed
    /// trusted project file logs a warning and falls back to global-only.
    pub fn load_with_project(global: HooksConfig, workspace: &Path) -> HooksConfig {
        let project_path = workspace.join(".mimofan").join("hooks.toml");
        if !project_path.exists() || !workspace_allows_project_hooks(workspace) {
            return global;
        }
        let Ok(contents) = std::fs::read_to_string(&project_path) else {
            return global;
        };
        let project: HooksConfig = match toml::from_str(&contents) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse project hooks at {}: {e}; falling back to global hooks only",
                    project_path.display()
                );
                return global;
            }
        };
        let mut merged = global;
        merged.hooks.extend(project.hooks);
        merged
    }

    /// Get hooks for a specific event
    pub fn hooks_for_event(&self, event: HookEvent) -> Vec<&Hook> {
        if !self.enabled {
            return Vec::new();
        }
        self.hooks.iter().filter(|h| h.event == event).collect()
    }

    /// Check if hooks are configured and enabled
    #[allow(dead_code)] // Public API for hook system consumers
    pub fn has_hooks(&self) -> bool {
        self.enabled && !self.hooks.is_empty()
    }
}

fn workspace_allows_project_hooks(workspace: &Path) -> bool {
    crate::config::is_workspace_trusted(workspace)
}

/// Context passed to hooks via environment variables
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    /// Tool name (for ToolCallBefore/After)
    pub tool_name: Option<String>,
    /// Tool arguments as JSON string
    pub tool_args: Option<String>,
    /// Tool result output (truncated)
    pub tool_result: Option<String>,
    /// Tool exit code if applicable
    pub tool_exit_code: Option<i32>,
    /// Whether tool succeeded
    pub tool_success: Option<bool>,
    /// Current mode
    pub mode: Option<String>,
    /// Previous mode (for `ModeChange`)
    pub previous_mode: Option<String>,
    /// Session ID
    pub session_id: Option<String>,
    /// User message content
    pub message: Option<String>,
    /// Error message (for `OnError`)
    pub error_message: Option<String>,
    /// Workspace path
    pub workspace: Option<PathBuf>,
    /// Current model name
    pub model: Option<String>,
    /// Total tokens used
    pub total_tokens: Option<u32>,
    /// Session cost in USD
    pub session_cost: Option<f64>,
}

impl HookContext {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)] // Public builder API, used in tests
    pub fn with_tool_name(mut self, name: &str) -> Self {
        self.tool_name = Some(name.to_string());
        self
    }

    #[allow(dead_code)] // Public builder API
    pub fn with_tool_args(mut self, args: &serde_json::Value) -> Self {
        self.tool_args = Some(args.to_string());
        self
    }

    #[allow(dead_code)] // Public builder API
    pub fn with_tool_result(mut self, result: &str, success: bool, exit_code: Option<i32>) -> Self {
        self.tool_result = Some(result.to_string());
        self.tool_success = Some(success);
        self.tool_exit_code = exit_code;
        self
    }

    #[allow(dead_code)] // Public builder API, used in tests
    pub fn with_mode(mut self, mode: &str) -> Self {
        self.mode = Some(mode.to_string());
        self
    }

    pub fn with_previous_mode(mut self, mode: &str) -> Self {
        self.previous_mode = Some(mode.to_string());
        self
    }

    #[allow(dead_code)] // Public builder API, used in tests
    pub fn with_workspace(mut self, path: PathBuf) -> Self {
        self.workspace = Some(path);
        self
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    pub fn with_session_id(mut self, session_id: &str) -> Self {
        self.session_id = Some(session_id.to_string());
        self
    }

    #[allow(dead_code)] // Public builder API
    pub fn with_message(mut self, message: &str) -> Self {
        self.message = Some(message.to_string());
        self
    }

    #[allow(dead_code)] // Public builder API
    pub fn with_error(mut self, error: &str) -> Self {
        self.error_message = Some(error.to_string());
        self
    }

    pub fn with_tokens(mut self, tokens: u32) -> Self {
        self.total_tokens = Some(tokens);
        self
    }

    #[allow(dead_code)] // Public builder API
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.session_cost = Some(cost);
        self
    }

    /// Convert to environment variables
    pub fn to_env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();

        if let Some(ref name) = self.tool_name {
            env.insert("DEEPSEEK_TOOL_NAME".to_string(), name.clone());
        }
        if let Some(ref args) = self.tool_args {
            env.insert("DEEPSEEK_TOOL_ARGS".to_string(), args.clone());
        }
        if let Some(ref result) = self.tool_result {
            // Truncate result to 10KB to avoid environment variable size limits
            let truncated = if result.len() > 10000 {
                let safe_end = result
                    .char_indices()
                    .take_while(|(i, _)| *i < 10000)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}...[truncated]", &result[..safe_end])
            } else {
                result.clone()
            };
            env.insert("DEEPSEEK_TOOL_RESULT".to_string(), truncated);
        }
        if let Some(code) = self.tool_exit_code {
            env.insert("DEEPSEEK_TOOL_EXIT_CODE".to_string(), code.to_string());
        }
        if let Some(success) = self.tool_success {
            env.insert("DEEPSEEK_TOOL_SUCCESS".to_string(), success.to_string());
        }
        if let Some(ref mode) = self.mode {
            env.insert("DEEPSEEK_MODE".to_string(), mode.clone());
        }
        if let Some(ref prev) = self.previous_mode {
            env.insert("DEEPSEEK_PREVIOUS_MODE".to_string(), prev.clone());
        }
        if let Some(ref session_id) = self.session_id {
            env.insert("DEEPSEEK_SESSION_ID".to_string(), session_id.clone());
        }
        if let Some(ref message) = self.message {
            // Truncate message to prevent env var issues
            let truncated = if message.len() > 5000 {
                let safe_end = message
                    .char_indices()
                    .take_while(|(i, _)| *i < 5000)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}...[truncated]", &message[..safe_end])
            } else {
                message.clone()
            };
            env.insert("DEEPSEEK_MESSAGE".to_string(), truncated);
        }
        if let Some(ref error) = self.error_message {
            env.insert("DEEPSEEK_ERROR".to_string(), error.clone());
        }
        if let Some(ref ws) = self.workspace {
            env.insert("DEEPSEEK_WORKSPACE".to_string(), ws.display().to_string());
        }
        if let Some(ref model) = self.model {
            env.insert("DEEPSEEK_MODEL".to_string(), model.clone());
        }
        if let Some(tokens) = self.total_tokens {
            env.insert("DEEPSEEK_TOTAL_TOKENS".to_string(), tokens.to_string());
        }
        if let Some(cost) = self.session_cost {
            env.insert("DEEPSEEK_SESSION_COST".to_string(), format!("{cost:.6}"));
        }

        env
    }
}

/// Result of a hook execution
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields are part of public API for hook consumers
pub struct HookResult {
    /// Hook name (if specified)
    pub name: Option<String>,
    /// Whether the hook succeeded
    pub success: bool,
    /// Exit code from the hook command
    pub exit_code: Option<i32>,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Time taken to execute
    pub duration: Duration,
    /// Error message if execution failed
    pub error: Option<String>,
}

/// Result of running mutable `message_submit` hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageSubmitOutcome {
    /// No hook changed the submitted text.
    Unchanged { warning: Option<String> },
    /// One or more hooks replaced the submitted text.
    Replaced {
        text: String,
        warning: Option<String>,
    },
    /// A hook intentionally blocked the submission.
    Blocked { reason: String },
}

impl MessageSubmitOutcome {
    pub fn unchanged() -> Self {
        Self::Unchanged { warning: None }
    }

    pub fn replaced(text: String) -> Self {
        Self::Replaced {
            text,
            warning: None,
        }
    }

    fn with_warning(self, warning: Option<String>) -> Self {
        match self {
            Self::Unchanged { .. } => Self::Unchanged { warning },
            Self::Replaced { text, .. } => Self::Replaced { text, warning },
            Self::Blocked { reason } => Self::Blocked { reason },
        }
    }

    pub fn warning(&self) -> Option<&str> {
        match self {
            Self::Unchanged { warning } | Self::Replaced { warning, .. } => warning.as_deref(),
            Self::Blocked { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MessageSubmitStdout {
    Unchanged,
    Replaced(String),
    Invalid(String),
}

/// Parsed stdout from a `tool_call_before` hook (#3026).
///
/// Hooks may emit a JSON decision on stdout:
/// `{"decision": "allow"|"deny"|"ask", "reason": "...",
///   "updatedInput": {...}, "additionalContext": "..."}`
/// Non-JSON or empty stdout → legacy passthrough (allow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallBeforeStdout {
    pub decision: Option<ToolCallDecision>,
    pub reason: Option<String>,
    pub updated_input: Option<serde_json::Value>,
    pub additional_context: Option<String>,
}

/// Decision a hook can return for a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallDecision {
    Allow,
    Deny,
    Ask,
}

pub(crate) fn parse_tool_call_before_stdout(stdout: &str) -> ToolCallBeforeStdout {
    let passthrough = ToolCallBeforeStdout {
        decision: None,
        reason: None,
        updated_input: None,
        additional_context: None,
    };
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return passthrough;
    }
    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        // Non-JSON stdout → legacy passthrough (allow).
        Err(_) => return passthrough,
    };
    let Some(obj) = value.as_object() else {
        tracing::warn!(
            "tool_call_before hook stdout is JSON but not an object; \
             ignoring it (legacy passthrough)"
        );
        return passthrough;
    };
    let decision = obj
        .get("decision")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "allow" => Some(ToolCallDecision::Allow),
            "deny" => Some(ToolCallDecision::Deny),
            "ask" => Some(ToolCallDecision::Ask),
            other => {
                tracing::warn!(
                    "tool_call_before hook returned unrecognized decision \
                     '{other}' (expected allow|deny|ask); treating as allow"
                );
                None
            }
        });
    let reason = obj
        .get("reason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let updated_input = obj.get("updatedInput").cloned().filter(|v| {
        if v.is_object() {
            true
        } else {
            tracing::warn!("tool_call_before hook updatedInput must be a JSON object; ignoring");
            false
        }
    });
    let additional_context = obj
        .get("additionalContext")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    ToolCallBeforeStdout {
        decision,
        reason,
        updated_input,
        additional_context,
    }
}

/// Post-turn accumulated totals included in the `turn_end` observer payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnEndTotals {
    pub session_tokens: u32,
    pub conversation_tokens: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Input used to build the structured `turn_end` observer payload.
pub struct TurnEndPayloadInput<'a> {
    pub context: &'a HookContext,
    pub turn_id: Option<&'a str>,
    pub status: &'a str,
    pub error: Option<&'a str>,
    pub duration: Duration,
    pub usage: &'a crate::models::Usage,
    pub totals: TurnEndTotals,
    pub tool_count: usize,
    pub queued_message_count: usize,
}

/// Executor for running hooks
#[derive(Debug, Clone)]
pub struct HookExecutor {
    config: HooksConfig,
    default_working_dir: PathBuf,
    session_id: String,
}

impl HookExecutor {
    fn build_shell_command(command: &str) -> Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            let mut cmd = Command::new("cmd");
            // raw_arg: cmd.exe does not parse the CRT-style \" escapes that
            // Command::arg would insert, so pass the command line verbatim.
            cmd.arg("/C").raw_arg(command);
            cmd
        }
        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(command);
            cmd
        }
    }

    /// Create a new `HookExecutor` with configuration
    pub fn new(config: HooksConfig, default_working_dir: PathBuf) -> Self {
        // Generate a session ID
        let session_id = format!("sess_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        Self {
            config,
            default_working_dir,
            session_id,
        }
    }

    /// Create a disabled `HookExecutor` (no hooks will run)
    #[allow(dead_code)] // Used in tests and as convenience constructor
    pub fn disabled() -> Self {
        Self {
            config: HooksConfig {
                enabled: false,
                ..Default::default()
            },
            default_working_dir: PathBuf::from("."),
            session_id: String::new(),
        }
    }

    /// Check if hooks are enabled
    #[allow(dead_code)] // Public API for hook system consumers
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the session ID
    /// Read-only access to the underlying configuration. Used by
    /// `/hooks` (#460 read-only MVP) so the user can list configured
    /// hooks without reaching for `cat ~/.mimofanfan/config.toml`.
    pub fn config(&self) -> &HooksConfig {
        &self.config
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Cheap pre-check: are there any enabled hooks for this event?
    /// Lets call sites avoid building a [`HookContext`] (which allocates
    /// for `workspace`, `model`, `session_id`, …) on every tool call
    /// when the user hasn't configured any hooks. The cost matters
    /// because `ToolCallBefore` / `ToolCallAfter` fire from
    /// `tool_routing.rs` on every tool dispatch (#455).
    #[must_use]
    pub fn has_hooks_for_event(&self, event: HookEvent) -> bool {
        self.config.enabled && self.config.hooks.iter().any(|h| h.event == event)
    }

    /// Check if there are any background hooks configured for a specific event.
    ///
    /// Background hooks fire and forget — their `exit_code` is always `None`,
    /// so they cannot deny tool calls. This is a known limitation; the check
    /// is used to warn operators when a `ToolCallBefore` hook is configured
    /// as background but expects to block a tool.
    #[must_use]
    pub fn has_background_hooks_for_event(&self, event: HookEvent) -> bool {
        if !self.config.enabled {
            return false;
        }
        self.config
            .hooks
            .iter()
            .any(|h| h.event == event && h.background)
    }

    /// Run configured `message_submit` hooks as a mutable submit pipeline.
    ///
    /// This is deliberately separate from [`Self::execute`]: most hook events
    /// are observer-only, while `message_submit` has a narrow stdout JSON
    /// contract that can replace or block the submitted text.
    pub fn execute_message_submit_transform(
        &self,
        context: &HookContext,
        original_text: &str,
    ) -> MessageSubmitOutcome {
        if !self.config.enabled {
            return MessageSubmitOutcome::unchanged();
        }

        let hooks = self.config.hooks_for_event(HookEvent::MessageSubmit);
        if hooks.is_empty() {
            return MessageSubmitOutcome::unchanged();
        }

        let mut current_text = original_text.to_string();
        let mut warning = None;

        for hook in hooks {
            let hook_context = context.clone().with_message(&current_text);
            if !self.matches_condition(hook, &hook_context) {
                continue;
            }

            let env_vars = hook_context.to_env_vars();
            if hook.background {
                let _ = self.execute_background(hook, &env_vars);
                continue;
            }

            let payload = message_submit_payload(&hook_context, &current_text);
            let result = self.execute_sync_with_stdin(hook, &env_vars, &payload);

            if result.exit_code == Some(2) {
                return MessageSubmitOutcome::Blocked {
                    reason: message_submit_block_reason(
                        &result,
                        "message_submit hook blocked submission",
                    ),
                };
            }

            if !result.success {
                let label = result.name.as_deref().unwrap_or("(unnamed)");
                tracing::warn!(
                    target: "hooks",
                    hook = label,
                    event = "message_submit",
                    exit_code = ?result.exit_code,
                    duration_ms = result.duration.as_millis() as u64,
                    error = result.error.as_deref().unwrap_or(""),
                    stderr_head = %result.stderr.lines().next().unwrap_or(""),
                    "message_submit hook failed"
                );

                if hook.continue_on_error {
                    warning = message_submit_continue_warning(&result).or(warning);
                    continue;
                }

                return MessageSubmitOutcome::Blocked {
                    reason: message_submit_block_reason(
                        &result,
                        "message_submit hook failed and blocked submission",
                    ),
                };
            }

            match parse_message_submit_stdout(&result.stdout) {
                MessageSubmitStdout::Unchanged => {}
                MessageSubmitStdout::Replaced(text) => {
                    current_text = text;
                }
                MessageSubmitStdout::Invalid(reason) => {
                    tracing::warn!(
                        target: "hooks",
                        hook = result.name.as_deref().unwrap_or("(unnamed)"),
                        event = "message_submit",
                        reason = %reason,
                        "ignored invalid message_submit hook stdout"
                    );
                }
            }
        }

        if current_text == original_text {
            MessageSubmitOutcome::unchanged().with_warning(warning)
        } else {
            MessageSubmitOutcome::replaced(current_text).with_warning(warning)
        }
    }

    /// Run every `ShellEnv` hook for this context and merge their stdout
    /// (`KEY=VALUE\n` lines) into a single env-var map. Used by the
    /// `exec_shell` tool to inject ephemeral credentials, per-skill PATH
    /// adjustments, etc. (#456). Failures don't abort the shell call —
    /// the hook simply contributes no vars and a `tracing::warn!` lands.
    ///
    /// Each successful hook's keys (NOT values) are written to the audit
    /// log so a session can be reconciled later without leaking the
    /// secret material itself.
    pub fn collect_shell_env(&self, context: &HookContext) -> HashMap<String, String> {
        let mut merged: HashMap<String, String> = HashMap::new();
        if !self.config.enabled {
            return merged;
        }
        let hooks = self.config.hooks_for_event(HookEvent::ShellEnv);
        if hooks.is_empty() {
            return merged;
        }
        let env_vars = context.to_env_vars();
        for hook in hooks {
            if !self.matches_condition(hook, context) {
                continue;
            }
            // ShellEnv hooks must be synchronous — their stdout is the contract.
            let result = self.execute_sync(hook, &env_vars);
            if !result.success {
                tracing::warn!(
                    target: "hooks",
                    hook = result.name.as_deref().unwrap_or("(unnamed)"),
                    event = "shell_env",
                    exit_code = ?result.exit_code,
                    error = result.error.as_deref().unwrap_or(""),
                    "shell_env hook failed; contributing no env vars"
                );
                continue;
            }
            let parsed = parse_env_lines(&result.stdout);
            if parsed.is_empty() {
                continue;
            }
            // Audit-log the *keys* — never the values.
            crate::audit::log_sensitive_event(
                "shell_env_hook",
                serde_json::json!({
                    "hook": result.name,
                    "tool": context.tool_name,
                    "keys": parsed.keys().cloned().collect::<Vec<_>>(),
                }),
            );
            // Later hooks override earlier ones. Documented behavior.
            merged.extend(parsed);
        }
        merged
    }

    /// Execute all hooks for an event
    pub fn execute(&self, event: HookEvent, context: &HookContext) -> Vec<HookResult> {
        if !self.config.enabled {
            return Vec::new();
        }

        let hooks = self.config.hooks_for_event(event);
        if hooks.is_empty() {
            // Fast path: no hooks for this event → skip the
            // `context.to_env_vars()` HashMap allocation. With
            // `tool_call_before` / `tool_call_after` firing per-tool
            // (#455) this allocation would otherwise happen on every
            // tool dispatch even for users with zero hooks configured.
            return Vec::new();
        }
        let env_vars = context.to_env_vars();
        let mut results = Vec::new();

        for hook in hooks {
            if !self.matches_condition(hook, context) {
                continue;
            }

            let result = if hook.background {
                self.execute_background(hook, &env_vars)
            } else {
                self.execute_sync(hook, &env_vars)
            };

            // Log failures via tracing so operators tailing
            // `deepseek` with `RUST_LOG=warn` can see hook errors
            // without instrumenting each call site. Successful runs
            // log nothing (would be too noisy on per-tool events).
            if !result.success {
                let label = result.name.as_deref().unwrap_or("(unnamed)");
                tracing::warn!(
                    target: "hooks",
                    hook = label,
                    event = event.as_str(),
                    exit_code = ?result.exit_code,
                    duration_ms = result.duration.as_millis() as u64,
                    error = result.error.as_deref().unwrap_or(""),
                    stderr_head = %result.stderr.lines().next().unwrap_or(""),
                    "hook failed"
                );
            }

            let should_continue = result.success || hook.continue_on_error;
            results.push(result);

            if !should_continue {
                break;
            }
        }

        results
    }

    /// Execute observer hooks with a structured JSON stdin payload.
    ///
    /// Unlike `message_submit`, stdout is deliberately ignored by callers:
    /// these hooks are lifecycle observers and cannot mutate or block the
    /// underlying action.
    pub fn execute_json_observer(
        &self,
        event: HookEvent,
        context: &HookContext,
        payload: &serde_json::Value,
    ) -> Vec<HookResult> {
        if !self.config.enabled {
            return Vec::new();
        }

        let hooks = self.config.hooks_for_event(event);
        if hooks.is_empty() {
            return Vec::new();
        }

        let env_vars = context.to_env_vars();
        let mut results = Vec::new();
        for hook in hooks {
            if !self.matches_condition(hook, context) {
                continue;
            }

            let result = if hook.background {
                self.execute_background_with_stdin(hook, &env_vars, payload)
            } else {
                self.execute_sync_with_stdin(hook, &env_vars, payload)
            };

            if !result.success {
                let label = result.name.as_deref().unwrap_or("(unnamed)");
                tracing::warn!(
                    target: "hooks",
                    hook = label,
                    event = event.as_str(),
                    exit_code = ?result.exit_code,
                    duration_ms = result.duration.as_millis() as u64,
                    error = result.error.as_deref().unwrap_or(""),
                    stderr_head = %result.stderr.lines().next().unwrap_or(""),
                    "observer hook failed"
                );
            }

            results.push(result);
        }

        results
    }

    /// Check whether a tool name matches a condition pattern with `*` glob support.
    fn tool_name_matches_condition(tool_name: &str, pattern: &str) -> bool {
        if !pattern.contains('*') {
            return tool_name == pattern;
        }
        // Escape regex metacharacters except `*`, which becomes `.*`.
        let escaped = regex::escape(pattern);
        let regex_pattern = escaped.replace(r"\*", ".*");
        let anchored = format!("^{regex_pattern}$");
        regex::Regex::new(&anchored).is_ok_and(|re| re.is_match(tool_name))
    }

    /// Check if a hook's condition matches the context
    #[allow(clippy::only_used_in_recursion)]
    fn matches_condition(&self, hook: &Hook, context: &HookContext) -> bool {
        match &hook.condition {
            None | Some(HookCondition::Always) => true,
            Some(HookCondition::ToolName { name }) => {
                // #3026: Support `*` globs in tool_name conditions so
                // `mcp__*` matches all MCP tools.  Exact names keep working.
                context
                    .tool_name
                    .as_ref()
                    .is_some_and(|n| Self::tool_name_matches_condition(n, name))
            }
            Some(HookCondition::ToolCategory { category }) => {
                // Map tool names to categories
                let tool_category = context.tool_name.as_ref().map(|name| match name.as_str() {
                    "exec_shell" => "shell",
                    "write_file" | "edit_file" | "apply_patch" => "file_write",
                    "read_file" | "list_dir" | "grep_files" => "safe",
                    _ => "other",
                });
                tool_category.is_some_and(|c| c == category.as_str())
            }
            Some(HookCondition::Mode { mode }) => context
                .mode
                .as_ref()
                .is_some_and(|m| m.to_lowercase() == mode.to_lowercase()),
            Some(HookCondition::ExitCode { code }) => context.tool_exit_code == Some(*code),
            Some(HookCondition::All { conditions }) => conditions.iter().all(|c| {
                self.matches_condition(
                    &Hook {
                        condition: Some(c.clone()),
                        ..hook.clone()
                    },
                    context,
                )
            }),
            Some(HookCondition::Any { conditions }) => conditions.iter().any(|c| {
                self.matches_condition(
                    &Hook {
                        condition: Some(c.clone()),
                        ..hook.clone()
                    },
                    context,
                )
            }),
        }
    }

    /// Execute a hook synchronously
    fn execute_sync(&self, hook: &Hook, env_vars: &HashMap<String, String>) -> HookResult {
        self.execute_sync_inner(hook, env_vars, None)
    }

    /// Execute a hook synchronously with a structured JSON stdin payload.
    ///
    /// Used by mutable `message_submit` hooks. Existing observer hooks keep the
    /// stdin-less [`Self::execute_sync`] path so their behavior is unchanged.
    fn execute_sync_with_stdin(
        &self,
        hook: &Hook,
        env_vars: &HashMap<String, String>,
        stdin_json: &serde_json::Value,
    ) -> HookResult {
        self.execute_sync_inner(hook, env_vars, Some(stdin_json))
    }

    fn execute_sync_inner(
        &self,
        hook: &Hook,
        env_vars: &HashMap<String, String>,
        stdin_json: Option<&serde_json::Value>,
    ) -> HookResult {
        let started = Instant::now();
        let working_dir = self
            .config
            .working_dir
            .clone()
            .unwrap_or_else(|| self.default_working_dir.clone());

        let timeout_secs = self
            .config
            .default_timeout_secs
            .unwrap_or(hook.timeout_secs);
        let timeout = Duration::from_secs(timeout_secs);

        let stdin_bytes = match stdin_json.map(serde_json::to_vec).transpose() {
            Ok(bytes) => bytes,
            Err(e) => {
                return HookResult {
                    name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: started.elapsed(),
                    error: Some(format!("Failed to encode hook stdin: {e}")),
                };
            }
        };

        let mut command = Self::build_shell_command(&hook.command);
        command
            .current_dir(&working_dir)
            .envs(env_vars)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if stdin_bytes.is_some() {
            command.stdin(Stdio::piped());
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                return HookResult {
                    name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: started.elapsed(),
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
            Ok(Some(status)) => HookResult {
                name: hook.name.clone(),
                success: status.success(),
                exit_code: status.code(),
                stdout: join_reader(stdout_reader),
                stderr: join_reader(stderr_reader),
                duration: started.elapsed(),
                error: None,
            },
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                // Do not join pipe threads on timeout: descendant processes can
                // inherit pipe fds, and waiting for those threads would defeat
                // the hook timeout we just enforced.
                HookResult {
                    name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: started.elapsed(),
                    error: Some(format!("Hook timed out after {timeout_secs}s")),
                }
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                HookResult {
                    name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: started.elapsed(),
                    error: Some(format!("Failed to wait for hook: {e}")),
                }
            }
        }
    }

    /// Execute a hook in the background (non-blocking)
    fn execute_background(&self, hook: &Hook, env_vars: &HashMap<String, String>) -> HookResult {
        self.execute_background_inner(hook, env_vars, None)
    }

    fn execute_background_with_stdin(
        &self,
        hook: &Hook,
        env_vars: &HashMap<String, String>,
        stdin_json: &serde_json::Value,
    ) -> HookResult {
        self.execute_background_inner(hook, env_vars, Some(stdin_json))
    }

    fn execute_background_inner(
        &self,
        hook: &Hook,
        env_vars: &HashMap<String, String>,
        stdin_json: Option<&serde_json::Value>,
    ) -> HookResult {
        let started = Instant::now();
        let working_dir = self
            .config
            .working_dir
            .clone()
            .unwrap_or_else(|| self.default_working_dir.clone());

        let stdin_bytes = match stdin_json.map(serde_json::to_vec).transpose() {
            Ok(bytes) => bytes,
            Err(e) => {
                return HookResult {
                    name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: started.elapsed(),
                    error: Some(format!("Failed to encode hook stdin: {e}")),
                };
            }
        };
        let cmd = hook.command.clone();
        let env = env_vars.clone();
        let wd = working_dir.clone();

        // Spawn in a detached thread (fire-and-forget hook execution).
        std::thread::spawn(move || {
            let mut command = HookExecutor::build_shell_command(&cmd);
            command
                .current_dir(&wd)
                .envs(&env)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if stdin_bytes.is_some() {
                command.stdin(Stdio::piped());
            }

            let Ok(mut child) = command.spawn() else {
                return;
            };
            if let (Some(mut bytes), Some(mut stdin)) = (stdin_bytes, child.stdin.take()) {
                bytes.push(b'\n');
                let _ = stdin.write_all(&bytes);
                let _ = stdin.flush();
            }
            let _ = child.wait();
        });

        // Return immediately with success (background execution is fire-and-forget)
        HookResult {
            name: hook.name.clone(),
            success: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration: started.elapsed(),
            error: None,
        }
    }
}

fn spawn_pipe_reader(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = pipe.read_to_string(&mut buf);
        buf
    })
}

fn join_reader(reader: Option<JoinHandle<String>>) -> String {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn spawn_stdin_writer(mut stdin: std::process::ChildStdin, mut bytes: Vec<u8>) -> JoinHandle<()> {
    thread::spawn(move || {
        bytes.push(b'\n');
        let _ = stdin.write_all(&bytes);
        let _ = stdin.flush();
    })
}

fn message_submit_payload(context: &HookContext, text: &str) -> serde_json::Value {
    json!({
        "event": HookEvent::MessageSubmit.as_str(),
        "text": text,
        "session_id": context.session_id.as_deref(),
        "workspace": context.workspace.as_ref().map(|path| path.display().to_string()),
        "mode": context.mode.as_deref(),
        "model": context.model.as_deref(),
        "total_tokens": context.total_tokens,
    })
}

pub fn turn_end_payload(input: TurnEndPayloadInput<'_>) -> serde_json::Value {
    json!({
        "event": HookEvent::TurnEnd.as_str(),
        "session_id": input.context.session_id.as_deref(),
        "workspace": input.context.workspace.as_ref().map(|path| path.display().to_string()),
        "mode": input.context.mode.as_deref(),
        "model": input.context.model.as_deref(),
        "turn_id": input.turn_id,
        "status": input.status,
        "error": input.error,
        "duration_ms": duration_ms_saturating(input.duration),
        "usage": {
            "input_tokens": input.usage.input_tokens,
            "output_tokens": input.usage.output_tokens,
            "prompt_cache_hit_tokens": input.usage.prompt_cache_hit_tokens,
            "prompt_cache_miss_tokens": input.usage.prompt_cache_miss_tokens,
            "reasoning_tokens": input.usage.reasoning_tokens,
            "reasoning_replay_tokens": input.usage.reasoning_replay_tokens,
        },
        "totals": {
            "session_tokens": input.totals.session_tokens,
            "conversation_tokens": input.totals.conversation_tokens,
            "input_tokens": input.totals.input_tokens,
            "output_tokens": input.totals.output_tokens,
        },
        "tool_count": input.tool_count,
        "queued_message_count": input.queued_message_count,
        "stop_hook_active": false,
    })
}

fn duration_ms_saturating(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn parse_message_submit_stdout(stdout: &str) -> MessageSubmitStdout {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return MessageSubmitStdout::Unchanged;
    }

    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(e) => return MessageSubmitStdout::Invalid(format!("invalid JSON: {e}")),
    };

    let Some(object) = value.as_object() else {
        return MessageSubmitStdout::Invalid("stdout JSON must be an object".to_string());
    };

    match object.get("text") {
        Some(serde_json::Value::String(text)) if !text.is_empty() => {
            MessageSubmitStdout::Replaced(text.clone())
        }
        Some(serde_json::Value::String(_)) => {
            MessageSubmitStdout::Invalid("stdout `text` field must not be empty".to_string())
        }
        Some(_) => MessageSubmitStdout::Invalid("stdout `text` field must be a string".to_string()),
        None => MessageSubmitStdout::Unchanged,
    }
}

fn message_submit_continue_warning(result: &HookResult) -> Option<String> {
    message_submit_stdout_reason(&result.stdout)
        .or_else(|| first_non_empty_line(&result.stderr))
        .or_else(|| first_non_empty_line(&result.stdout))
        .or_else(|| result.error.as_deref().and_then(first_non_empty_line))
}

fn message_submit_block_reason(result: &HookResult, fallback: &str) -> String {
    if let Some(reason) = message_submit_stdout_reason(&result.stdout) {
        return reason;
    }
    if let Some(reason) = first_non_empty_line(&result.stderr) {
        return reason;
    }
    if let Some(reason) = first_non_empty_line(&result.stdout) {
        return reason;
    }
    if let Some(reason) = result.error.as_deref().and_then(first_non_empty_line) {
        return reason;
    }
    fallback.to_string()
}

fn message_submit_stdout_reason(stdout: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .map(truncate_hook_message)
}

fn first_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(truncate_hook_message)
}

fn truncate_hook_message(message: &str) -> String {
    const MAX_CHARS: usize = 240;
    let mut chars = message.chars();
    let mut out: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// Parse `KEY=VALUE\n` lines from a `shell_env` hook's stdout into a map.
///
/// Tolerated: blank lines, leading whitespace, `#` comment lines (ignored),
/// `export KEY=VALUE` (the `export ` prefix is dropped), surrounding quotes
/// on the value. Lines without `=` are silently dropped — easier than
/// failing the whole hook for one stray line of human-friendly output.
/// Values are otherwise taken verbatim; we don't run them through a shell
/// for variable expansion to avoid surprises.
fn parse_env_lines(stdout: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let stripped = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        out.insert(key.to_string(), stripped.to_string());
    }
    out
}

// === Unit Tests ===

#[cfg(test)]
mod tests {}
