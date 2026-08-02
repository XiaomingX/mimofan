// clap 命令行定义（从 lib.rs 抽离，纯物理拆分，零行为变化）
use crate::config::Config;
use crate::latest_session_id_for_workspace;
use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "mimofan",
    bin_name = "mimofan",
    author,
    version = env!("MIMOFAN_BUILD_VERSION"),
    about = "mimofan terminal coding agent",
    long_about = "Terminal-native TUI and CLI for open-source and open-weight coding models.\n\nRun 'mimo' to start.\n\nProvider routes include DeepSeek, Arcee, Hugging Face, OpenRouter, Xiaomi MiMo, and more."
)]
pub(crate) struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,

    #[command(flatten)]
    pub(crate) feature_toggles: FeatureToggles,

    /// Initial prompt to submit in the interactive TUI. Use `exec` for non-interactive runs.
    #[arg(short, long, value_name = "PROMPT", num_args = 1..)]
    pub(crate) prompt: Vec<String>,

    /// YOLO mode: enable agent tools + shell execution
    #[arg(long)]
    pub(crate) yolo: bool,

    /// Maximum number of concurrent sub-agents (1-20)
    #[arg(long)]
    pub(crate) max_subagents: Option<usize>,

    /// Path to config file
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long)]
    pub(crate) verbose: bool,

    /// Config profile name
    #[arg(long)]
    pub(crate) profile: Option<String>,

    /// Workspace directory for file operations
    #[arg(short, long)]
    pub(crate) workspace: Option<PathBuf>,

    /// Resume a previous session by ID or prefix
    #[arg(short, long)]
    pub(crate) resume: Option<String>,

    /// Continue the most recent session in this workspace
    #[arg(short = 'c', long = "continue")]
    pub(crate) continue_session: bool,

    /// Deprecated compatibility flag; the interactive TUI always owns the
    /// alternate screen so terminal scrollback cannot hijack the viewport.
    #[arg(long = "no-alt-screen", hide = true)]
    pub(crate) no_alt_screen: bool,

    /// Enable TUI mouse capture for internal scrolling, transcript selection,
    /// and scrollbar dragging
    /// (default off on Windows)
    #[arg(long = "mouse-capture", conflicts_with = "no_mouse_capture")]
    pub(crate) mouse_capture: bool,

    /// Disable TUI mouse capture so terminal-native text selection works
    #[arg(long = "no-mouse-capture", conflicts_with = "mouse_capture")]
    pub(crate) no_mouse_capture: bool,

    /// Skip onboarding screens
    #[arg(long)]
    pub(crate) skip_onboarding: bool,

    /// Start a fresh session, ignoring any crash-recovery checkpoint
    #[arg(long = "fresh")]
    pub(crate) fresh: bool,

    /// Skip loading project-level config from $WORKSPACE/.mimofan/config.toml
    #[arg(long = "no-project-config")]
    pub(crate) no_project_config: bool,
}

#[derive(Subcommand, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Commands {
    /// Run system diagnostics and check configuration
    Doctor(DoctorArgs),
    /// Bootstrap MCP config and/or skills directories
    Setup(SetupArgs),
    /// Generate a remote mimofan agent deploy bundle (cloud + chat bridge)
    RemoteSetup(crate::remote_setup::RemoteSetupArgs),
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// List saved sessions
    Sessions {
        /// Maximum number of sessions to display
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Search sessions by title
        #[arg(short, long)]
        search: Option<String>,
    },
    /// Create default AGENTS.md in current directory
    Init,
    /// Save an API key to the shared user config
    Login {
        /// Provider to authenticate with
        #[arg(long, value_enum, hide = true)]
        provider: Option<crate::cli_commands::ProviderArg>,
        /// API key to store (otherwise read from stdin)
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Remove the saved API key
    Logout,
    /// List available models from the configured API endpoint
    Models(ModelsArgs),
    /// Generate speech audio with Xiaomi MiMo TTS models
    #[command(visible_alias = "tts")]
    Speech(SpeechArgs),
    /// Run a non-interactive prompt. Use --auto for tool-backed agent mode.
    Exec(ExecArgs),
    /// Manage local Agent Fleet runs and workers
    Fleet(FleetArgs),
    /// Run a code review over a git diff
    Review(ReviewArgs),
    /// Open the TUI pre-seeded with a GitHub PR's title, body, and diff (#451)
    Pr {
        /// PR number
        #[arg(value_name = "NUMBER")]
        number: u32,
        /// Repository in `owner/name` form. Defaults to the current
        /// workspace's `gh` config (i.e. the repo gh thinks you're in).
        #[arg(short = 'R', long)]
        repo: Option<String>,
        /// Skip `gh pr checkout` even if gh is available. By default
        /// the working tree is left as-is — checkout is opt-in via
        /// `--checkout` because dirty trees fail it loudly.
        #[arg(long, default_value_t = false)]
        checkout: bool,
    },
    /// Apply a patch file (or stdin) to the working tree
    Apply(ApplyArgs),
    /// Run the offline evaluation harness (no network/LLM calls)
    Eval(EvalArgs),
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    /// Execpolicy tooling
    Execpolicy(ExecpolicyCommand),
    /// Inspect feature flags
    Features(FeaturesCli),
    /// Run a command inside the sandbox
    Sandbox(SandboxArgs),
    /// Run a local server (e.g. MCP)
    Serve(ServeArgs),
    /// Resume a previous session by ID (use --last for most recent)
    Resume {
        /// Conversation/session id (UUID or prefix)
        #[arg(value_name = "SESSION_ID")]
        session_id: Option<String>,
        /// Continue the most recent session in this workspace without a picker
        #[arg(long = "last", default_value_t = false, conflicts_with = "session_id")]
        last: bool,
    },
    /// Fork a previous session by ID (use --last for most recent)
    Fork {
        /// Conversation/session id (UUID or prefix)
        #[arg(value_name = "SESSION_ID")]
        session_id: Option<String>,
        /// Fork the most recent session in this workspace without a picker
        #[arg(long = "last", default_value_t = false, conflicts_with = "session_id")]
        last: bool,
    },
    /// Manage authentication credentials and provider mode.
    Auth(crate::cli_commands::AuthArgs),
    /// Run MCP server mode over stdio.
    McpServer,
    /// Read/write/list config values.
    Config(crate::cli_commands::ConfigArgs),
    /// Resolve or list available models across providers.
    Model(crate::cli_commands::ModelArgs),
    /// Manage thread/session metadata and resume/fork flows.
    Thread(crate::cli_commands::ThreadArgs),
    /// Run the canonical runtime API / control plane (HTTP/SSE, mobile, stdio).
    AppServer(crate::cli_commands::AppServerArgs),
    /// Generate shell completions.
    #[command(after_help = r#"Examples:
  Bash (current shell only):
    source <(mimofan completion bash)

  Zsh:
    mkdir -p ~/.zfunc
    mimofan completion zsh > ~/.zfunc/_mimofan
    # Add to ~/.zshrc if needed:
    #   fpath=(~/.zfunc $fpath)
    #   autoload -Uz compinit && compinit

  Fish:
    mkdir -p ~/.config/fish/completions
    mimofan completion fish > ~/.config/fish/completions/mimofan.fish

  PowerShell (current shell only):
    mimofan completion powershell | Out-String | Invoke-Expression

The command prints the completion script to stdout; redirect it to a path your shell loads automatically."#)]
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Print a usage rollup from the audit log and session store.
    Metrics(crate::cli_commands::MetricsArgs),
    /// Check for and apply updates to the `mimofan` binary.
    Update(crate::cli_commands::UpdateArgs),
}

#[derive(Args, Debug, Clone)]
#[command(after_help = "\
Examples:
  mimofan exec \"explain this function\"
  mimofan exec --auto \"list crates/ with ls\"
  mimofan exec --auto --output-format stream-json \"fix the failing test\"

Plain `mimofan exec` is a one-shot model response. Use `--auto` for
non-interactive filesystem/shell tool use.
")]
pub(crate) struct ExecArgs {
    /// Override model for this run
    #[arg(long)]
    pub(crate) model: Option<String>,
    /// Enable tool-backed agent mode with auto-approvals
    #[arg(long, default_value_t = false)]
    pub(crate) auto: bool,
    /// Emit machine-readable JSON output
    #[arg(long, default_value_t = false, conflicts_with = "output_format")]
    pub(crate) json: bool,
    /// Resume a previous session by ID or prefix
    #[arg(long, value_name = "SESSION_ID", conflicts_with_all = ["session_id", "continue_session"])]
    pub(crate) resume: Option<String>,
    /// Resume a previous session by ID or prefix
    #[arg(long = "session-id", value_name = "SESSION_ID", conflicts_with_all = ["resume", "continue_session"])]
    pub(crate) session_id: Option<String>,
    /// Continue the most recent session for this workspace
    #[arg(long = "continue", default_value_t = false, conflicts_with_all = ["resume", "session_id"])]
    pub(crate) continue_session: bool,
    /// Output format for exec mode
    #[arg(long, value_enum, default_value_t = ExecOutputFormat::Text)]
    pub(crate) output_format: ExecOutputFormat,
    /// Comma-separated list of tools to allow (all others denied).
    /// Lowercase catalog names: read_file, write_file, exec_shell, grep_files, etc.
    #[arg(long, value_delimiter = ',')]
    pub(crate) allowed_tools: Option<Vec<String>>,
    /// Comma-separated list of tools to deny (deny wins over allow).
    #[arg(long, value_delimiter = ',')]
    pub(crate) disallowed_tools: Option<Vec<String>>,
    /// Maximum number of model steps (tool calls) before the run ends.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) max_turns: Option<u32>,
    /// Extra text appended to the system prompt for this run.
    #[arg(long)]
    pub(crate) append_system_prompt: Option<String>,
    /// Prompt to send to the model
    #[arg(
        value_name = "PROMPT",
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub(crate) prompt: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ExecOutputFormat {
    Text,
    #[value(name = "stream-json")]
    StreamJson,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct FleetArgs {
    #[command(subcommand)]
    pub(crate) command: FleetCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum FleetCommand {
    /// Initialize the local fleet ledger for this workspace
    Init,
    /// Create a run from a task spec and start the foreground manager loop
    Run(FleetRunArgs),
    /// Show queued/running/completed/failed/stale fleet counts
    Status,
    /// Inspect one worker's status, heartbeat, latest event, and artifacts
    Inspect {
        /// Worker id printed by `mimofan fleet run`
        worker_id: String,
    },
    /// Print bounded log artifacts for one worker
    Logs {
        /// Worker id printed by `mimofan fleet run`
        worker_id: String,
    },
    /// List artifact refs for one worker
    Artifacts {
        /// Worker id printed by `mimofan fleet run`
        worker_id: String,
    },
    /// Interrupt a running worker task and record a terminal cancellation
    Interrupt {
        /// Worker id printed by `mimofan fleet run`
        worker_id: String,
    },
    /// Restart the latest task for a worker
    Restart {
        /// Worker id printed by `mimofan fleet run`
        worker_id: String,
    },
    /// Resume a run from durable ledger state, reconciling orphaned/stale leases
    Resume {
        /// Run id printed by `mimofan fleet run`
        run_id: String,
        /// Seconds without heartbeat before a leased task is treated as stale
        #[arg(long, default_value_t = 300)]
        stale_after_seconds: u64,
    },
    /// Stop all queued and running fleet work
    Stop {
        /// Confirm stopping all queued and running fleet tasks
        #[arg(long, required = true)]
        all: bool,
    },
    /// Render a redacted fleet alert payload without sending it
    AlertDryRun(FleetAlertDryRunArgs),
}

#[derive(Args, Debug, Clone)]
pub(crate) struct FleetRunArgs {
    /// JSON or TOML task spec to enqueue
    #[arg(value_name = "TASK_SPEC")]
    pub(crate) task_spec: PathBuf,
    /// Maximum local workers to lease concurrently
    #[arg(long, default_value_t = 4)]
    pub(crate) max_workers: usize,
    /// Seconds without heartbeat before a running task is counted stale
    #[arg(long, default_value_t = 300)]
    pub(crate) stale_after_seconds: u64,
    /// Schedule once and return instead of staying in the manager loop
    #[arg(long, hide = true, default_value_t = false)]
    pub(crate) once: bool,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct FleetAlertDryRunArgs {
    /// Alert event class to render
    #[arg(long, value_enum)]
    pub(crate) event: FleetAlertEventArg,
    /// Fleet run id
    #[arg(long)]
    pub(crate) run_id: String,
    /// Worker id, when the event belongs to one worker
    #[arg(long)]
    pub(crate) worker_id: Option<String>,
    /// Task id, when the event belongs to one task
    #[arg(long)]
    pub(crate) task_id: Option<String>,
    /// Short human-readable reason for the alert
    #[arg(long, default_value = "manual fleet alert dry-run")]
    pub(crate) reason: String,
    /// Status label to include in the payload
    #[arg(long)]
    pub(crate) status: Option<String>,
    /// Adapter payload shape to render
    #[arg(long, value_enum, default_value_t = FleetAlertAdapterArg::Slack)]
    pub(crate) adapter: FleetAlertAdapterArg,
    /// Environment variable containing the Slack webhook URL
    #[arg(long, default_value = "MIMOFAN_FLEET_SLACK_WEBHOOK")]
    pub(crate) slack_webhook_env: String,
    /// Environment variable containing the generic webhook URL
    #[arg(long, default_value = "MIMOFAN_FLEET_WEBHOOK_URL")]
    pub(crate) webhook_url_env: String,
    /// Optional environment variable containing the generic webhook secret
    #[arg(long)]
    pub(crate) webhook_secret_env: Option<String>,
    /// Environment variable containing the PagerDuty routing key
    #[arg(long, default_value = "MIMOFAN_FLEET_PAGERDUTY_ROUTING_KEY")]
    pub(crate) pagerduty_routing_key_env: String,
    /// PagerDuty severity to render
    #[arg(long, default_value = "error")]
    pub(crate) pagerduty_severity: String,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub(crate) enum FleetAlertEventArg {
    Stale,
    RestartExhausted,
    NeedsHuman,
    BudgetExceeded,
    VerifierFailed,
    RunCompleted,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub(crate) enum FleetAlertAdapterArg {
    Slack,
    Webhook,
    PagerDuty,
}

pub(crate) fn join_prompt_parts(parts: &[String]) -> String {
    parts.join(" ")
}

pub(crate) fn resolve_exec_model(config: &Config, explicit_model: Option<&str>) -> String {
    explicit_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .or_else(exec_model_env_override)
        .unwrap_or_else(|| config.default_model())
}

pub(crate) fn exec_model_env_override() -> Option<String> {
    ["MIMOFAN_MODEL"].into_iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty())
    })
}

pub(crate) fn top_level_prompt_initial_input(parts: &[String]) -> Option<crate::tui::InitialInput> {
    (!parts.is_empty()).then(|| crate::tui::InitialInput::Submit(join_prompt_parts(parts)))
}

pub(crate) fn resolve_exec_resume_session_id(
    args: &ExecArgs,
    workspace: &Path,
) -> Result<Option<String>> {
    if let Some(id) = args.resume.as_ref().or(args.session_id.as_ref()) {
        return Ok(Some(id.clone()));
    }
    if !args.continue_session {
        return Ok(None);
    }
    latest_session_id_for_workspace(workspace)?.map_or_else(
        || {
            bail!(
                "No saved sessions found for workspace {}. Use `mimofan sessions` to list sessions, or pass `mimo exec --resume <SESSION_ID> ...`.",
                workspace.display()
            )
        },
        |id| Ok(Some(id)),
    )
}

#[derive(Args, Debug, Clone, Default)]
pub(crate) struct SetupArgs {
    /// Initialize MCP configuration at the configured path
    #[arg(long, default_value_t = false)]
    pub(crate) mcp: bool,
    /// Initialize skills directory and an example skill
    #[arg(long, default_value_t = false)]
    pub(crate) skills: bool,
    /// Initialize tools directory with a self-describing example script
    #[arg(long, default_value_t = false)]
    pub(crate) tools: bool,
    /// Initialize plugins directory with a self-describing example
    #[arg(long, default_value_t = false)]
    pub(crate) plugins: bool,
    /// Initialize MCP config, skills, tools, and plugins
    #[arg(long, default_value_t = false)]
    pub(crate) all: bool,
    /// Create a local workspace skills directory (./skills)
    #[arg(long, default_value_t = false)]
    pub(crate) local: bool,
    /// Overwrite existing template files
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,
    /// Print a compact, read-only status report (no network calls)
    #[arg(long, default_value_t = false, conflicts_with_all = ["mcp", "skills", "tools", "plugins", "all", "local", "clean"])]
    pub(crate) status: bool,
    /// Remove regenerable session checkpoints (latest + offline_queue)
    #[arg(long, default_value_t = false, conflicts_with_all = ["mcp", "skills", "tools", "plugins", "all", "local", "status"])]
    pub(crate) clean: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub(crate) struct DoctorArgs {
    /// Emit machine-readable JSON output (skips live API connectivity check)
    #[arg(long, default_value_t = false)]
    pub(crate) json: bool,
    /// Emit only the diagnostic context source map as JSON
    #[arg(long, default_value_t = false, conflicts_with = "json")]
    pub(crate) context_json: bool,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct EvalArgs {
    /// Intentionally fail a specific step (list, read, search, edit, patch, shell)
    #[arg(long, value_name = "STEP")]
    pub(crate) fail_step: Option<String>,
    /// Shell command to run during the exec step
    #[arg(long, default_value = "printf eval-harness")]
    pub(crate) shell_command: String,
    /// Token that must appear in shell output for validation
    #[arg(long, default_value = "eval-harness")]
    pub(crate) shell_expect_token: String,
    /// Maximum characters stored per step output summary
    #[arg(long, default_value_t = 240)]
    pub(crate) max_output_chars: usize,
    /// Emit machine-readable JSON output
    #[arg(long, default_value_t = false)]
    pub(crate) json: bool,
    /// Append one JSONL fixture line per step to `<DIR>/<scenario>.jsonl`.
    /// Mock LLM tests can later replay these fixtures.
    #[arg(long, value_name = "DIR")]
    pub(crate) record: Option<PathBuf>,
}

#[derive(Args, Debug, Clone, Default)]
pub(crate) struct ModelsArgs {
    /// Print models as pretty JSON
    #[arg(long, default_value_t = false)]
    pub(crate) json: bool,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct SpeechArgs {
    /// Text to synthesize. This is sent as the assistant message content.
    #[arg(value_name = "TEXT")]
    pub(crate) text: String,

    /// Output audio path. Defaults to speech.<format> in --output-dir,
    /// [speech].output_dir, or the current directory.
    #[arg(short, long, value_name = "FILE")]
    pub(crate) output: Option<PathBuf>,

    /// Directory for the default speech.<format> output file when -o/--output is omitted.
    #[arg(long = "output-dir", value_name = "DIR")]
    pub(crate) output_dir: Option<PathBuf>,

    /// TTS model. Defaults to built-in voices, or is inferred from --voice-prompt/--clone-voice.
    #[arg(long)]
    pub(crate) model: Option<String>,

    /// Built-in voice ID, or a data:audio/...;base64,... URI for voice clone.
    #[arg(long)]
    pub(crate) voice: Option<String>,

    /// Natural language style instruction; not spoken verbatim.
    #[arg(long)]
    pub(crate) instruction: Option<String>,

    /// Voice design prompt. Implies mimo-v2.5-tts-voicedesign when --model is omitted.
    #[arg(long = "voice-prompt")]
    pub(crate) voice_prompt: Option<String>,

    /// MP3/WAV sample used for voice cloning. Implies mimo-v2.5-tts-voiceclone when --model is omitted.
    #[arg(long = "clone-voice", value_name = "FILE")]
    pub(crate) clone_voice: Option<PathBuf>,

    /// Output audio format requested from the API
    #[arg(long, default_value = "wav")]
    pub(crate) format: String,

    /// Emit machine-readable JSON output
    #[arg(long, default_value_t = false)]
    pub(crate) json: bool,
}

#[derive(Args, Debug, Default, Clone)]
pub(crate) struct FeatureToggles {
    /// Enable a feature (repeatable). Equivalent to `features.<name>=true`.
    #[arg(long = "enable", value_name = "FEATURE", action = clap::ArgAction::Append, global = true)]
    pub(crate) enable: Vec<String>,

    /// Disable a feature (repeatable). Equivalent to `features.<name>=false`.
    #[arg(long = "disable", value_name = "FEATURE", action = clap::ArgAction::Append, global = true)]
    pub(crate) disable: Vec<String>,
}

impl FeatureToggles {
    pub(crate) fn apply(&self, config: &mut Config) -> Result<()> {
        for feature in &self.enable {
            config.set_feature(feature, true)?;
        }
        for feature in &self.disable {
            config.set_feature(feature, false)?;
        }
        Ok(())
    }
}

#[derive(Args, Debug, Clone)]
pub(crate) struct ReviewArgs {
    /// Review staged changes instead of the working tree
    #[arg(long, conflicts_with = "base")]
    pub(crate) staged: bool,
    /// Base ref to diff against (e.g. origin/main)
    #[arg(long)]
    pub(crate) base: Option<String>,
    /// Limit diff to a specific path
    #[arg(long)]
    pub(crate) path: Option<PathBuf>,
    /// Override model for this review
    #[arg(long)]
    pub(crate) model: Option<String>,
    /// Maximum diff characters to include
    #[arg(long, default_value_t = 200_000)]
    pub(crate) max_chars: usize,
    /// Write a durable pre-push review receipt after a successful review
    #[arg(long, default_value_t = false)]
    pub(crate) write_receipt: bool,
    /// Validate the current diff against a durable review receipt without calling a model
    #[arg(long, default_value_t = false)]
    pub(crate) check_receipt: bool,
    /// Override where the review receipt is written or read
    #[arg(long)]
    pub(crate) receipt_path: Option<PathBuf>,
    /// Emit machine-readable JSON output
    #[arg(long, default_value_t = false)]
    pub(crate) json: bool,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct ApplyArgs {
    /// Patch file to apply (defaults to stdin)
    #[arg(value_name = "PATCH_FILE")]
    pub(crate) patch_file: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct ServeArgs {
    /// Start MCP server over stdio
    #[arg(long)]
    pub(crate) mcp: bool,
    /// Start runtime HTTP/SSE API server
    #[arg(long)]
    pub(crate) http: bool,
    /// Start runtime HTTP/SSE API server with the built-in mobile control page
    #[arg(long)]
    pub(crate) mobile: bool,
    /// Show a QR code for the mobile URL in the terminal (requires --mobile)
    #[arg(long, requires = "mobile")]
    pub(crate) qr: bool,
    /// Start ACP server over stdio for editor clients such as Zed
    #[arg(long)]
    pub(crate) acp: bool,
    /// Bind host for HTTP server (default localhost; --mobile defaults to 0.0.0.0)
    #[arg(long)]
    pub(crate) host: Option<String>,
    /// Bind port for HTTP server
    #[arg(long, default_value_t = 7878)]
    pub(crate) port: u16,
    /// Background task worker count (1-8)
    #[arg(long, default_value_t = 2)]
    pub(crate) workers: usize,
    /// Additional CORS origin to allow (repeatable). Stacks on top of the
    /// built-in defaults (localhost:3000, localhost:1420, tauri://localhost).
    /// Also reads `MIMOFAN_CORS_ORIGINS` (comma-separated), then
    /// `MIMOFAN_CORS_ORIGINS` as an alias, and `[runtime_api] cors_origins`
    /// from `config.toml`. Mimofanscale#255.
    #[arg(long = "cors-origin", value_name = "URL")]
    pub(crate) cors_origin: Vec<String>,
    /// Require this bearer token for `/v1/*` runtime API routes. Also reads
    /// `MIMOFAN_RUNTIME_TOKEN` when omitted, then `MIMOFAN_RUNTIME_TOKEN`
    /// as an alias.
    #[arg(long = "auth-token", value_name = "TOKEN")]
    pub(crate) auth_token: Option<String>,
    /// Disable runtime API auth when no token is configured. Only use on a trusted loopback.
    #[arg(long = "insecure")]
    pub(crate) insecure_no_auth: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServeBindHost {
    pub(crate) host: String,
    pub(crate) mobile_rebound_to_lan: bool,
}

pub(crate) fn resolve_serve_bind_host(mobile: bool, host: Option<String>) -> ServeBindHost {
    match (mobile, host) {
        (true, None) => ServeBindHost {
            host: "0.0.0.0".to_string(),
            mobile_rebound_to_lan: true,
        },
        (_, Some(host)) => ServeBindHost {
            host,
            mobile_rebound_to_lan: false,
        },
        (false, None) => ServeBindHost {
            host: "127.0.0.1".to_string(),
            mobile_rebound_to_lan: false,
        },
    }
}

pub(crate) fn validate_serve_mode_selection(
    mcp: bool,
    http: bool,
    mobile: bool,
    acp: bool,
) -> Result<bool> {
    if http && mobile {
        bail!("--http and --mobile are mutually exclusive; choose one");
    }
    let http_selected = http || mobile;
    let selected_modes = [mcp, http_selected, acp]
        .into_iter()
        .filter(|selected| *selected)
        .count();
    if selected_modes != 1 {
        bail!("Choose exactly one server mode: --mcp, --http/--mobile, or --acp");
    }
    Ok(http_selected)
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum McpCommand {
    /// List configured MCP servers
    List,
    /// Create a template MCP config at the configured path
    Init {
        /// Overwrite an existing MCP config file
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Connect to MCP servers and report status
    Connect {
        /// Optional server name to connect to
        #[arg(value_name = "SERVER")]
        server: Option<String>,
    },
    /// List tools discovered from MCP servers
    Tools {
        /// Optional server name to list tools for
        #[arg(value_name = "SERVER")]
        server: Option<String>,
    },
    /// Add an MCP server entry
    Add {
        /// Server name
        name: String,
        /// Command to launch stdio server
        #[arg(long, conflicts_with = "url")]
        command: Option<String>,
        /// URL for streamable HTTP/SSE server
        #[arg(long, conflicts_with = "command")]
        url: Option<String>,
        /// Explicit URL transport override. Use "sse" for legacy SSE endpoints.
        #[arg(long, requires = "url")]
        transport: Option<String>,
        /// Environment variable containing a bearer token for URL-based servers
        #[arg(long, requires = "url")]
        bearer_token_env_var: Option<String>,
        /// OAuth client ID for servers that do not support dynamic registration
        #[arg(long, requires = "url")]
        oauth_client_id: Option<String>,
        /// OAuth resource parameter to append to the authorization URL
        #[arg(long, requires = "url")]
        oauth_resource: Option<String>,
        /// OAuth scope to request during login. Repeat or comma-separate.
        #[arg(long = "scope", requires = "url", value_delimiter = ',')]
        scopes: Vec<String>,
        /// Arguments for command-based servers
        #[arg(long = "arg")]
        args: Vec<String>,
    },
    /// Authenticate to a URL-based MCP server using OAuth
    Login {
        /// Server name
        name: String,
        /// OAuth scope to request. Repeat or comma-separate; defaults to config/discovery.
        #[arg(long = "scope", value_delimiter = ',')]
        scopes: Vec<String>,
    },
    /// Delete stored OAuth credentials for a URL-based MCP server
    Logout {
        /// Server name
        name: String,
    },
    /// Remove an MCP server entry
    Remove {
        /// Server name
        name: String,
    },
    /// Enable an MCP server
    Enable {
        /// Server name
        name: String,
    },
    /// Disable an MCP server
    Disable {
        /// Server name
        name: String,
    },
    /// Validate MCP config and required servers
    Validate,
    /// Register this mimofan binary as a local MCP stdio server.
    ///
    /// This adds a config entry that runs `mimofan serve --mcp` (stdio protocol).
    /// For the HTTP/SSE runtime API, use `mimo serve --http` directly instead.
    #[command(
        name = "add-self",
        long_about = "Register this mimofan binary as a local MCP stdio server.\n\nAdds a config entry to ~/.mimofan/mcp.json that launches `mimo serve --mcp`\nvia the stdio transport. Other mimofan sessions (or any MCP client) can then\ndiscover and call tools exposed by this server.\n\nUse `mimo serve --http` instead if you need the HTTP/SSE runtime API."
    )]
    AddSelf {
        /// Server name in mcp.json (default: "mimofan")
        #[arg(long, default_value = "mimofan")]
        name: String,
        /// Workspace directory for the MCP server
        #[arg(long)]
        workspace: Option<String>,
    },
}

#[derive(Args, Debug, Clone)]
pub(crate) struct ExecpolicyCommand {
    #[command(subcommand)]
    pub(crate) command: ExecpolicySubcommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum ExecpolicySubcommand {
    /// Check execpolicy files against a command
    Check(crate::execpolicy::ExecPolicyCheckCommand),
}

#[derive(Args, Debug, Clone)]
pub(crate) struct FeaturesCli {
    #[command(subcommand)]
    pub(crate) command: FeaturesSubcommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum FeaturesSubcommand {
    /// List known feature flags and their state
    List,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct SandboxArgs {
    #[command(subcommand)]
    pub(crate) command: SandboxCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum SandboxCommand {
    /// Run a command with sandboxing
    Run {
        /// Sandbox policy (danger-full-access, read-only, external-sandbox, workspace-write)
        #[arg(long, default_value = "workspace-write")]
        policy: String,
        /// Allow outbound network access
        #[arg(long)]
        network: bool,
        /// Additional writable roots (repeatable)
        #[arg(long, value_name = "PATH")]
        writable_root: Vec<PathBuf>,
        /// Exclude TMPDIR from writable paths
        #[arg(long)]
        exclude_tmpdir: bool,
        /// Exclude /tmp from writable paths
        #[arg(long)]
        exclude_slash_tmp: bool,
        /// Command working directory
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Timeout in milliseconds
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        /// Command and arguments to run
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
}
