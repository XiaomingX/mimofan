#![allow(clippy::uninlined_format_args)]

mod metrics;
mod update;

use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use mimofan_agent::ModelRegistry;
use mimofan_app_server::{
    AppServerOptions, run as run_app_server, run_stdio as run_app_server_stdio,
};
use mimofan_config::{
    CliRuntimeOverrides, ConfigStore, ProviderKind, ResolvedRuntimeOptions, RuntimeApiKeySource,
};
use mimofan_execpolicy::{AskForApproval, ExecPolicyContext, ExecPolicyEngine};
use mimofan_mcp::{McpServerDefinition, run_stdio_server};
use mimofan_secrets::Secrets;
use mimofan_state::{StateStore, ThreadListFilters};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ProviderArg {
    Deepseek,
    NvidiaNim,
    Openai,
    Atlascloud,
    WanjieArk,
    Volcengine,
    Openrouter,
    XiaomiMimo,
    Novita,
    Fireworks,
    Siliconflow,
    #[value(
        alias = "silicon-flow-cn",
        alias = "siliconflow-CN",
        alias = "silicon_flow_cn",
        alias = "siliconflow_cn",
        alias = "siliconflow-china",
        alias = "siliconflow_china"
    )]
    SiliconflowCn,
    Arcee,
    Moonshot,
    Huggingface,
    Together,
    OpenaiCodex,
    Anthropic,
    Zai,
    Stepfun,
    Minimax,
    #[value(alias = "deep-infra", alias = "deep_infra")]
    Deepinfra,
}

impl From<ProviderArg> for ProviderKind {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::Deepseek => ProviderKind::XiaomiMimo,
            ProviderArg::NvidiaNim => ProviderKind::XiaomiMimo,
            ProviderArg::Openai => ProviderKind::XiaomiMimo,
            ProviderArg::Atlascloud => ProviderKind::XiaomiMimo,
            ProviderArg::WanjieArk => ProviderKind::XiaomiMimo,
            ProviderArg::Volcengine => ProviderKind::XiaomiMimo,
            ProviderArg::Openrouter => ProviderKind::XiaomiMimo,
            ProviderArg::XiaomiMimo => ProviderKind::XiaomiMimo,
            ProviderArg::Novita => ProviderKind::XiaomiMimo,
            ProviderArg::Fireworks => ProviderKind::XiaomiMimo,
            ProviderArg::Siliconflow => ProviderKind::XiaomiMimo,
            ProviderArg::SiliconflowCn => ProviderKind::XiaomiMimo,
            ProviderArg::Arcee => ProviderKind::XiaomiMimo,
            ProviderArg::Moonshot => ProviderKind::XiaomiMimo,
            ProviderArg::Huggingface => ProviderKind::XiaomiMimo,
            ProviderArg::Together => ProviderKind::XiaomiMimo,
            ProviderArg::OpenaiCodex => ProviderKind::XiaomiMimo,
            ProviderArg::Anthropic => ProviderKind::XiaomiMimo,
            ProviderArg::Zai => ProviderKind::XiaomiMimo,
            ProviderArg::Stepfun => ProviderKind::XiaomiMimo,
            ProviderArg::Minimax => ProviderKind::XiaomiMimo,
            ProviderArg::Deepinfra => ProviderKind::XiaomiMimo,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "mimofan",
    version = env!("DEEPSEEK_BUILD_VERSION"),
    bin_name = "mimofan",
    override_usage = "mimofan [OPTIONS] [PROMPT]\n       mimofan [OPTIONS] <COMMAND> [ARGS]"
)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "Advanced provider selector for non-TUI registry/config commands"
    )]
    provider: Option<ProviderArg>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long = "output-mode")]
    output_mode: Option<String>,
    #[arg(
        long = "verbosity",
        value_name = "LEVEL",
        help = "Controls transcript and output verbosity (normal, concise)"
    )]
    verbosity: Option<String>,
    #[arg(long = "log-level")]
    log_level: Option<String>,
    #[arg(long)]
    telemetry: Option<bool>,
    #[arg(long)]
    approval_policy: Option<String>,
    #[arg(long)]
    sandbox_mode: Option<String>,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    /// Workspace directory for TUI file tools
    #[arg(short = 'C', long = "workspace", alias = "cd", value_name = "DIR")]
    workspace: Option<PathBuf>,
    #[arg(long = "no-alt-screen", hide = true)]
    no_alt_screen: bool,
    #[arg(long = "mouse-capture", conflicts_with = "no_mouse_capture")]
    mouse_capture: bool,
    #[arg(long = "no-mouse-capture", conflicts_with = "mouse_capture")]
    no_mouse_capture: bool,
    #[arg(long = "skip-onboarding")]
    skip_onboarding: bool,
    /// YOLO mode: auto-approve all tools
    #[arg(long)]
    yolo: bool,
    /// Continue the most recent interactive session for this workspace.
    #[arg(short = 'c', long = "continue")]
    continue_session: bool,
    #[arg(short = 'p', long = "prompt", value_name = "PROMPT")]
    prompt_flag: Option<String>,
    #[arg(
        value_name = "PROMPT",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    prompt: Vec<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run interactive/non-interactive flows via the TUI binary.
    Run(RunArgs),
    /// Run mimofan diagnostics.
    Doctor(TuiPassthroughArgs),
    /// List live provider API models via the TUI binary.
    Models(TuiPassthroughArgs),
    /// Generate speech audio with Xiaomi MiMo TTS models via the TUI binary.
    #[command(visible_alias = "tts")]
    Speech(TuiPassthroughArgs),
    /// List saved TUI sessions.
    Sessions(TuiPassthroughArgs),
    /// Resume a saved TUI session.
    Resume(TuiPassthroughArgs),
    /// Fork a saved TUI session.
    Fork(TuiPassthroughArgs),
    /// Create a default AGENTS.md in the current directory.
    Init(TuiPassthroughArgs),
    /// Bootstrap MCP config and/or skills directories.
    Setup(TuiPassthroughArgs),
    /// Generate a remote mimofan agent deploy bundle (cloud + chat bridge).
    RemoteSetup(RemoteSetupArgs),
    /// Run a non-interactive prompt through the TUI runtime.
    #[command(after_help = "\
Examples:
  mimofan exec \"explain this function\"
  mimofan exec --auto \"list crates/ with ls\"
  mimofan exec --auto --output-format stream-json \"fix the failing test\"

Common forwarded flags:
  --auto                           Enable tool-backed agent mode with auto-approvals
  --json                           Emit summary JSON
  --resume <SESSION_ID>            Resume a previous session by ID or prefix
  --session-id <SESSION_ID>        Resume a previous session by ID or prefix
  --continue                       Continue the most recent session for this workspace
  --output-format <FORMAT>         Output format: text or stream-json

Plain `mimofan exec` is a one-shot model response. Use `--auto` for
non-interactive filesystem/shell tool use, matching the supported automation
path used by stream-json wrappers.
")]
    Exec(TuiPassthroughArgs),
    /// Manage durable Agent Fleet runs via the TUI runtime.
    Fleet(TuiPassthroughArgs),
    /// Run a mimofan-powered code review over a git diff.
    Review(TuiPassthroughArgs),
    /// Apply a patch file or stdin to the working tree.
    Apply(TuiPassthroughArgs),
    /// Run the offline TUI evaluation harness.
    Eval(TuiPassthroughArgs),
    /// Manage TUI MCP servers.
    Mcp(TuiPassthroughArgs),
    /// Inspect TUI feature flags.
    Features(TuiPassthroughArgs),
    /// Run a local TUI server.
    #[command(after_help = "\
Forwarded serve options:
      --mcp                 Start MCP server over stdio
      --http                Start runtime HTTP/SSE API server
      --mobile              Start runtime HTTP/SSE API server with the mobile control page
      --qr                  Show a QR code for the mobile URL (requires --mobile)
      --acp                 Start ACP server over stdio for editor clients
      --host <HOST>         Bind host (default 127.0.0.1; --mobile defaults to 0.0.0.0)
      --port <PORT>         Bind port [default: 7878]
      --workers <WORKERS>   Background task worker count (1-8)
      --cors-origin <URL>   Additional CORS origin to allow (repeatable)
      --auth-token <TOKEN>  Require this bearer token for /v1/* runtime API routes
      --insecure            Disable runtime API auth when no token is configured

`mimofan serve --http` and `mimofan serve --mobile` remain compatibility
aliases for `mimofan app-server --http` and `mimofan app-server --mobile`.
New integrations should prefer `mimofan app-server`.")]
    Serve(TuiPassthroughArgs),
    /// Generate shell completions for the TUI binary.
    Completions(TuiPassthroughArgs),
    /// Configure provider credentials.
    Login(LoginArgs),
    /// Remove saved authentication state.
    Logout,
    /// Manage authentication credentials and provider mode.
    Auth(AuthArgs),
    /// Run MCP server mode over stdio.
    McpServer,
    /// Read/write/list config values.
    Config(ConfigArgs),
    /// Resolve or list available models across providers.
    Model(ModelArgs),
    /// Manage thread/session metadata and resume/fork flows.
    Thread(ThreadArgs),
    /// Evaluate sandbox/approval policy decisions.
    Sandbox(SandboxArgs),
    /// Run the canonical runtime API / control plane (HTTP/SSE, mobile, stdio).
    #[command(after_help = "\
Transports:
  mimofan app-server --http              Full HTTP/SSE runtime API (/v1/*) on 127.0.0.1:7878
  mimofan app-server --mobile            Runtime API + phone control page (binds 0.0.0.0)
  mimofan app-server --stdio             JSON-RPC control transport over stdio (no listener)
  mimofan app-server                     Legacy in-process app-server HTTP on 127.0.0.1:8787

`--http` and `--mobile` serve the same mature runtime API as `mimofan serve
--http`/`--mobile`, which remain as compatibility aliases. The runtime API token
is read from --auth-token, CODEWHALE_RUNTIME_TOKEN, or DEEPSEEK_RUNTIME_TOKEN.

See docs/RUNTIME_API.md.")]
    AppServer(AppServerArgs),
    /// Generate shell completions.
    #[command(after_help = r#"Examples:
  Bash (current shell only):
    source <(mimofan completion bash)

  Bash (persistent, Linux/bash-completion):
    mkdir -p ~/.local/share/bash-completion/completions
    mimofan completion bash > ~/.local/share/bash-completion/completions/mimofan
    # Requires bash-completion to be installed and loaded by your shell.

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
    Metrics(MetricsArgs),
    /// Check for and apply updates to the `mimofan` binary.
    Update(UpdateArgs),
}

#[derive(Debug, Args)]
struct UpdateArgs {
    /// Update to the latest beta release instead of the latest stable release.
    #[arg(long)]
    beta: bool,
    /// Only check the latest release; do not download or replace binaries.
    #[arg(long)]
    check: bool,
    /// Proxy URL to use for update HTTP requests.
    #[arg(long, value_name = "URL")]
    proxy: Option<String>,
}

#[derive(Debug, Args)]
struct MetricsArgs {
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
    /// Restrict to events newer than this duration (e.g. 7d, 24h, 30m, now-2h).
    #[arg(long, value_name = "DURATION")]
    since: Option<String>,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Debug, Args, Clone)]
struct TuiPassthroughArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

/// Flags for `mimofan remote-setup`. Forwarded to the TUI binary, which owns
/// the interactive wizard and bundle generation.
#[derive(Debug, Args, Clone, Default)]
struct RemoteSetupArgs {
    /// Cloud target slug (lighthouse, azure, digitalocean). Skips the prompt.
    #[arg(long)]
    cloud: Option<String>,
    /// Chat bridge slug (feishu, telegram). Skips the prompt.
    #[arg(long)]
    bridge: Option<String>,
    /// Provider slug; validated against the provider registry. Skips the prompt.
    #[arg(long)]
    provider: Option<String>,
    /// Bundle output directory (default `./mimofan-deploy/<cloud>-<bridge>`).
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// Emit the bundle, do not provision (default).
    #[arg(long, default_value_t = false)]
    generate_only: bool,
    /// Run the cloud CLI to auto-provision (not yet implemented).
    #[arg(long, default_value_t = false, conflicts_with = "generate_only")]
    apply: bool,
    /// Skip the final confirmation gate (CI / non-interactive).
    #[arg(long, default_value_t = false)]
    yes: bool,
    /// Fail instead of prompting if any required value is missing.
    #[arg(long, default_value_t = false)]
    non_interactive: bool,
}

/// Build the forwarded argv for the TUI `remote-setup` subcommand from the
/// structured CLI flags. Mirrors the named flags exactly so the TUI clap parser
/// re-derives the same `RemoteSetupArgs`.
fn remote_setup_tui_args(args: RemoteSetupArgs) -> Vec<String> {
    let mut forwarded = vec!["remote-setup".to_string()];
    if let Some(cloud) = args.cloud {
        forwarded.push("--cloud".to_string());
        forwarded.push(cloud);
    }
    if let Some(bridge) = args.bridge {
        forwarded.push("--bridge".to_string());
        forwarded.push(bridge);
    }
    if let Some(provider) = args.provider {
        forwarded.push("--provider".to_string());
        forwarded.push(provider);
    }
    if let Some(out) = args.out {
        forwarded.push("--out".to_string());
        forwarded.push(out.to_string_lossy().into_owned());
    }
    if args.generate_only {
        forwarded.push("--generate-only".to_string());
    }
    if args.apply {
        forwarded.push("--apply".to_string());
    }
    if args.yes {
        forwarded.push("--yes".to_string());
    }
    if args.non_interactive {
        forwarded.push("--non-interactive".to_string());
    }
    forwarded
}

#[derive(Debug, Args)]
struct LoginArgs {
    #[arg(long, value_enum, hide = true)]
    provider: Option<ProviderArg>,
    #[arg(long)]
    api_key: Option<String>,
}

#[derive(Debug, Args)]
struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Show current provider and credential source state.
    /// Without `--provider`, shows all known providers.
    /// With `--provider`, shows detailed status for that provider.
    Status {
        /// Show status for a specific provider only.
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
    },
    /// Save an API key to the shared user config file. Reads from
    /// `--api-key`, `--api-key-stdin`, or prompts on stdin when
    /// neither is given. Does not echo the key.
    Set {
        #[arg(long, value_enum)]
        provider: ProviderArg,
        /// Inline value (discouraged — appears in shell history).
        #[arg(long)]
        api_key: Option<String>,
        /// Read the key from stdin instead of prompting.
        #[arg(long = "api-key-stdin", default_value_t = false)]
        api_key_stdin: bool,
    },
    /// Report whether a provider has a key configured. Never prints
    /// the value; just `set` / `not set` plus the source layer.
    Get {
        #[arg(long, value_enum)]
        provider: ProviderArg,
    },
    /// Delete a provider's key from config and secret-store storage.
    Clear {
        #[arg(long, value_enum)]
        provider: ProviderArg,
    },
    /// List all known providers with their auth state, without
    /// revealing keys.
    List,
    /// Advanced: migrate config-file keys into a platform credential store.
    #[command(hide = true)]
    Migrate {
        /// Don't actually write anything; print what would change.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Get { key: String },
    Set { key: String, value: String },
    Unset { key: String },
    List,
    Path,
}

#[derive(Debug, Args)]
struct ModelArgs {
    #[command(subcommand)]
    command: ModelCommand,
}

#[derive(Debug, Subcommand)]
enum ModelCommand {
    List {
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
    },
    Resolve {
        model: Option<String>,
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
    },
    /// Set the default model (e.g. "pro", "flash", "deepseek-v4-pro").
    Set { model: String },
}

#[derive(Debug, Args)]
struct ThreadArgs {
    #[command(subcommand)]
    command: ThreadCommand,
}

#[derive(Debug, Subcommand)]
enum ThreadCommand {
    List {
        #[arg(long, default_value_t = false)]
        all: bool,
        #[arg(long)]
        limit: Option<usize>,
    },
    Read {
        thread_id: String,
    },
    Resume {
        thread_id: String,
    },
    Fork {
        thread_id: String,
    },
    Archive {
        thread_id: String,
    },
    Unarchive {
        thread_id: String,
    },
    SetName {
        thread_id: String,
        name: String,
    },
    /// Remove the custom name from a thread, restoring the default
    /// `(unnamed)` rendering in `thread list`.
    ClearName {
        thread_id: String,
    },
}

#[derive(Debug, Args)]
struct SandboxArgs {
    #[command(subcommand)]
    command: SandboxCommand,
}

#[derive(Debug, Subcommand)]
enum SandboxCommand {
    Check {
        command: String,
        #[arg(long, value_enum, default_value_t = ApprovalModeArg::OnRequest)]
        ask: ApprovalModeArg,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ApprovalModeArg {
    UnlessTrusted,
    OnFailure,
    OnRequest,
    Never,
}

impl From<ApprovalModeArg> for AskForApproval {
    fn from(value: ApprovalModeArg) -> Self {
        match value {
            ApprovalModeArg::UnlessTrusted => AskForApproval::UnlessTrusted,
            ApprovalModeArg::OnFailure => AskForApproval::OnFailure,
            ApprovalModeArg::OnRequest => AskForApproval::OnRequest,
            ApprovalModeArg::Never => AskForApproval::Never,
        }
    }
}

#[derive(Debug, Args)]
struct AppServerArgs {
    /// Serve the full HTTP/SSE runtime API (`/v1/*`: sessions, threads, turns,
    /// approvals, events, usage, fleet, tasks). This is the canonical runtime
    /// API surface; it delegates to the same server as `mimofan serve --http`.
    #[arg(long, conflicts_with_all = ["stdio", "mobile"])]
    http: bool,
    /// Serve the runtime API plus the phone-friendly mobile control page.
    /// Equivalent to the legacy `mimofan serve --mobile`.
    #[arg(long, conflicts_with = "stdio")]
    mobile: bool,
    /// Run the app-server JSON-RPC control transport over stdio (no listener).
    /// Used by local SDKs and JSON-RPC integrations.
    #[arg(long, default_value_t = false)]
    stdio: bool,
    /// Show a QR code for the mobile URL in the terminal (requires --mobile).
    #[arg(long, requires = "mobile")]
    qr: bool,
    /// Bind host. Defaults to 127.0.0.1; with --mobile and no host, binds
    /// 0.0.0.0 so LAN devices can reach the mobile page.
    #[arg(long)]
    host: Option<String>,
    /// Bind port. Defaults to 7878 for --http/--mobile (the runtime API) and
    /// 8787 for the legacy in-process app-server HTTP transport.
    #[arg(long)]
    port: Option<u16>,
    /// Background task worker count (1-8). Only used with --http/--mobile.
    #[arg(long)]
    workers: Option<usize>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long = "auth-token")]
    auth_token: Option<String>,
    #[arg(long, default_value_t = false)]
    insecure_no_auth: bool,
    #[arg(long = "cors-origin")]
    cors_origin: Vec<String>,
}

const MCP_SERVER_DEFINITIONS_KEY: &str = "mcp.server_definitions";

fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn run_cli() -> std::process::ExitCode {
    install_rustls_crypto_provider();

    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            // Use the full anyhow chain so callers see the underlying
            // cause (e.g. the actual TOML parse error with line/column)
            // instead of just the top-level context message. The bare
            // `{err}` Display impl drops the chain — see #767, where
            // users hit "failed to parse config at <path>" with no
            // hint that the real error was a stray BOM or unbalanced
            // quote a few lines down.
            eprintln!("error: {err}");
            for cause in err.chain().skip(1) {
                eprintln!("  caused by: {cause}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut cli = Cli::parse();

    let mut store = ConfigStore::load(cli.config.clone())?;
    let runtime_overrides = CliRuntimeOverrides {
        provider: cli.provider.map(Into::into),
        model: cli.model.clone(),
        api_key: cli.api_key.clone(),
        base_url: cli.base_url.clone(),
        auth_mode: None,
        output_mode: cli.output_mode.clone(),
        log_level: cli.log_level.clone(),
        telemetry: cli.telemetry,
        approval_policy: cli.approval_policy.clone(),
        sandbox_mode: cli.sandbox_mode.clone(),
        yolo: Some(cli.yolo),
        verbosity: cli.verbosity.clone(),
    };
    let command = cli.command.take();

    match command {
        Some(Commands::Run(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, args.args)
        }
        Some(Commands::Doctor(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("doctor", args))
        }
        Some(Commands::Models(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("models", args))
        }
        Some(Commands::Speech(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("speech", args))
        }
        Some(Commands::Sessions(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("sessions", args))
        }
        Some(Commands::Resume(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            run_resume_command(&cli, &resolved_runtime, args)
        }
        Some(Commands::Fork(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("fork", args))
        }
        Some(Commands::Init(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("init", args))
        }
        Some(Commands::Setup(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("setup", args))
        }
        Some(Commands::RemoteSetup(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, remote_setup_tui_args(args))
        }
        Some(Commands::Exec(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("exec", args))
        }
        Some(Commands::Fleet(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("fleet", args))
        }
        Some(Commands::Review(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("review", args))
        }
        Some(Commands::Apply(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("apply", args))
        }
        Some(Commands::Eval(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("eval", args))
        }
        Some(Commands::Mcp(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("mcp", args))
        }
        Some(Commands::Features(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("features", args))
        }
        Some(Commands::Serve(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            // `serve` starts a long-running runtime API listener; supervise the
            // delegated child so it is torn down with the dispatcher (#3259).
            delegate_server_to_tui(&cli, &resolved_runtime, tui_args("serve", args))
        }
        Some(Commands::Completions(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            delegate_to_tui(&cli, &resolved_runtime, tui_args("completions", args))
        }
        Some(Commands::Login(args)) => run_login_command(&mut store, args),
        Some(Commands::Logout) => run_logout_command(&mut store),
        Some(Commands::Auth(args)) => run_auth_command(&mut store, args.command),
        Some(Commands::McpServer) => run_mcp_server_command(&mut store),
        Some(Commands::Config(args)) => run_config_command(&mut store, args.command),
        Some(Commands::Model(args)) => {
            run_model_command(&mut store, args.command, runtime_overrides.provider)
        }
        Some(Commands::Thread(args)) => run_thread_command(args.command),
        Some(Commands::Sandbox(args)) => run_sandbox_command(args.command),
        Some(Commands::AppServer(args)) => {
            // The HTTP/mobile runtime API is delegated to the mature `serve` path
            // in the TUI binary, which reads the *global* --config. app-server has
            // historically taken a subcommand-level --config, so bridge it before
            // resolving runtime options (provider/keyring) for the delegated run.
            if (args.http || args.mobile) && cli.config.is_none() && args.config.is_some() {
                cli.config = args.config.clone();
                store = ConfigStore::load(cli.config.clone())?;
            }
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            run_app_server_command(&cli, &resolved_runtime, args)
        }
        Some(Commands::Completion { shell }) => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "mimofan", &mut io::stdout());
            Ok(())
        }
        Some(Commands::Metrics(args)) => run_metrics_command(args),
        Some(Commands::Update(args)) => update::run_update(args.beta, args.check, args.proxy),
        None => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
            let forwarded = root_tui_passthrough(&cli)?;
            delegate_to_tui(&cli, &resolved_runtime, forwarded)
        }
    }
}

fn root_tui_passthrough(cli: &Cli) -> Result<Vec<String>> {
    let mut forwarded = Vec::new();
    if cli.continue_session {
        forwarded.push("--continue".to_string());
    }

    let prompt =
        cli.prompt_flag
            .iter()
            .chain(cli.prompt.iter())
            .fold(String::new(), |mut acc, part| {
                if !acc.is_empty() {
                    acc.push(' ');
                }
                acc.push_str(part);
                acc
            });
    if !prompt.is_empty() {
        if cli.continue_session {
            bail!(
                "`mimofan --continue` resumes the interactive TUI. Use `mimofan exec --continue <PROMPT>` to continue a session non-interactively."
            );
        }
        forwarded.push("--prompt".to_string());
        forwarded.push(prompt);
    }

    Ok(forwarded)
}

fn resolve_runtime_for_dispatch(
    store: &mut ConfigStore,
    runtime_overrides: &CliRuntimeOverrides,
) -> ResolvedRuntimeOptions {
    let runtime_secrets = Secrets::auto_detect();
    resolve_runtime_for_dispatch_with_secrets(store, runtime_overrides, &runtime_secrets)
}

fn resolve_runtime_for_dispatch_with_secrets(
    store: &mut ConfigStore,
    runtime_overrides: &CliRuntimeOverrides,
    secrets: &Secrets,
) -> ResolvedRuntimeOptions {
    let mut resolved = store
        .config
        .resolve_runtime_options_with_secrets(runtime_overrides, secrets);

    if resolved.api_key_source == Some(RuntimeApiKeySource::Keyring)
        && !provider_config_set(store, resolved.provider)
        && let Some(api_key) = resolved.api_key.clone()
    {
        write_provider_api_key_to_config(store, resolved.provider, &api_key);
        match store.save() {
            Ok(()) => {
                eprintln!(
                    "info: recovered API key from secret store and saved it to {}",
                    store.path().display()
                );
                resolved.api_key_source = Some(RuntimeApiKeySource::ConfigFile);
            }
            Err(err) => {
                eprintln!(
                    "warning: recovered API key from secret store but failed to save {}: {err}",
                    store.path().display()
                );
            }
        }
    }

    resolved
}

fn tui_args(command: &str, args: TuiPassthroughArgs) -> Vec<String> {
    let mut forwarded = Vec::with_capacity(args.args.len() + 1);
    forwarded.push(command.to_string());
    forwarded.extend(args.args);
    forwarded
}

fn run_login_command(store: &mut ConfigStore, args: LoginArgs) -> Result<()> {
    run_login_command_with_secrets(store, args, &Secrets::auto_detect())
}

fn run_login_command_with_secrets(
    store: &mut ConfigStore,
    args: LoginArgs,
    secrets: &Secrets,
) -> Result<()> {
    let provider: ProviderKind = args.provider.unwrap_or(ProviderArg::Deepseek).into();
    store.config.provider = provider;

    let api_key = match args.api_key {
        Some(v) => v,
        None => read_api_key_from_stdin()?,
    };
    write_provider_api_key_to_config(store, provider, &api_key);
    let keyring_saved = write_provider_api_key_to_keyring(secrets, provider, &api_key);
    store.save()?;
    let destination = if keyring_saved {
        format!("{} and {}", store.path().display(), secrets.backend_name())
    } else {
        store.path().display().to_string()
    };
    if provider == ProviderKind::XiaomiMimo {
        println!("logged in using API key mode (deepseek); saved key to {destination}");
    } else {
        println!(
            "logged in using API key mode ({}); saved key to {destination}",
            provider.as_str(),
        );
    }
    Ok(())
}

fn run_logout_command(store: &mut ConfigStore) -> Result<()> {
    run_logout_command_with_secrets(store, &Secrets::auto_detect())
}

fn run_logout_command_with_secrets(store: &mut ConfigStore, secrets: &Secrets) -> Result<()> {
    let active_provider = store.config.provider;
    store.config.api_key = None;
    for provider in ProviderKind::ALL {
        clear_provider_api_key_from_config(store, provider);
    }
    clear_provider_api_key_from_keyring(secrets, active_provider);
    store.config.auth_mode = None;
    store.save()?;
    println!("logged out");
    Ok(())
}

/// Map [`ProviderKind`] to the canonical provider credential slot.
fn provider_slot(provider: ProviderKind) -> &'static str {
    match provider {
        // Keep the historical shared credential slot for the China endpoint.
        ProviderKind::XiaomiMimo => "siliconflow",
        _ => provider.provider().id(),
    }
}

#[cfg(test)]
fn no_keyring_secrets() -> Secrets {
    Secrets::new(std::sync::Arc::new(
        mimofan_secrets::InMemoryKeyringStore::new(),
    ))
}

fn write_provider_api_key_to_config(
    store: &mut ConfigStore,
    provider: ProviderKind,
    api_key: &str,
) {
    store.config.auth_mode = Some("api_key".to_string());
    store.config.providers.for_provider_mut(provider).api_key = Some(api_key.to_string());
    if provider == ProviderKind::XiaomiMimo {
        store.config.api_key = Some(api_key.to_string());
        if store.config.default_text_model.is_none() {
            store.config.default_text_model = Some(
                store
                    .config
                    .providers
                    .xiaomi_mimo
                    .model
                    .clone()
                    .unwrap_or_else(|| "mimo-v2-pro".to_string()),
            );
        }
    }
}

fn clear_provider_api_key_from_config(store: &mut ConfigStore, provider: ProviderKind) {
    store.config.providers.for_provider_mut(provider).api_key = None;
    if provider == ProviderKind::XiaomiMimo {
        store.config.api_key = None;
    }
}

fn provider_env_set(provider: ProviderKind) -> bool {
    provider_env_value(provider).is_some()
}

fn provider_env_vars(provider: ProviderKind) -> &'static [&'static str] {
    provider.provider().env_vars()
}

fn provider_env_value(provider: ProviderKind) -> Option<(&'static str, String)> {
    provider_env_vars(provider).iter().find_map(|var| {
        std::env::var(var)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| (*var, value))
    })
}

fn openai_codex_auth_file_path() -> PathBuf {
    if let Ok(path) = std::env::var("OPENAI_CODEX_AUTH_FILE") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    let codex_home = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codex")
        });
    codex_home.join("auth.json")
}

fn provider_oauth_file_path(provider: ProviderKind) -> Option<PathBuf> {
    (provider == ProviderKind::XiaomiMimo).then(openai_codex_auth_file_path)
}

fn provider_config_api_key(store: &ConfigStore, provider: ProviderKind) -> Option<&str> {
    let slot = store
        .config
        .providers
        .for_provider(provider)
        .api_key
        .as_deref();
    let root = (provider == ProviderKind::XiaomiMimo)
        .then_some(store.config.api_key.as_deref())
        .flatten();
    slot.or(root).filter(|v| !v.trim().is_empty())
}

fn provider_config_set(store: &ConfigStore, provider: ProviderKind) -> bool {
    provider_config_api_key(store, provider).is_some()
}

fn provider_keyring_api_key(secrets: &Secrets, provider: ProviderKind) -> Option<String> {
    secrets
        .get(provider_slot(provider))
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty())
}

fn provider_keyring_set(secrets: &Secrets, provider: ProviderKind) -> bool {
    provider_keyring_api_key(secrets, provider).is_some()
}

fn write_provider_api_key_to_keyring(
    secrets: &Secrets,
    provider: ProviderKind,
    api_key: &str,
) -> bool {
    secrets.set(provider_slot(provider), api_key).is_ok()
}

fn clear_provider_api_key_from_keyring(secrets: &Secrets, provider: ProviderKind) {
    let _ = secrets.delete(provider_slot(provider));
}

fn auth_status_all_providers(store: &ConfigStore, secrets: &Secrets) -> Vec<String> {
    let active_provider = store.config.provider;
    let mut lines = Vec::new();
    lines.push(format!(
        "active provider: {} (set via config or CODEWHALE_PROVIDER)",
        active_provider.as_str()
    ));
    lines.push(String::new());
    lines.push(format!(
        "{:<14} {:<8} {:<10} {:<8} {}",
        "provider", "config", "keyring", "env", "status"
    ));
    lines.push("-".repeat(70));

    for provider in ProviderKind::ALL {
        let config_key = provider_config_api_key(store, provider);
        let keyring_key = provider_keyring_api_key(secrets, provider);
        let env_key = provider_env_value(provider);
        let oauth_file_present = provider_oauth_file_path(provider).is_some_and(|p| p.exists());

        let config_status = config_key.map(|_| "set").unwrap_or("-");
        let keyring_status = keyring_key.as_ref().map(|_| "set").unwrap_or("-");
        let env_status = env_key.as_ref().map(|_| "set").unwrap_or("-");

        let source = if provider == ProviderKind::XiaomiMimo {
            // Keep the summary consistent with `auth status`: Codex auth is
            // OAuth-file (or env token) based — config/keyring keys are not
            // consulted for it.
            if env_key.is_some() {
                "env"
            } else if oauth_file_present {
                "oauth file"
            } else {
                "unset"
            }
        } else if config_key.is_some() {
            "config"
        } else if keyring_key.is_some() {
            "keyring"
        } else if env_key.is_some() {
            "env"
        } else if oauth_file_present {
            "oauth file"
        } else {
            "unset"
        };

        let active_marker = if provider == active_provider {
            " *"
        } else {
            ""
        };

        lines.push(format!(
            "{:<14} {:<8} {:<10} {:<8} {}{}",
            provider.as_str(),
            config_status,
            keyring_status,
            env_status,
            source,
            active_marker
        ));
    }

    lines.push(String::new());
    lines.push("* = active provider (from config or CODEWHALE_PROVIDER)".to_string());
    lines.push("Run `mimofan auth status --provider <id>` for detailed info.".to_string());
    lines
}

fn auth_status_lines_for_provider(
    store: &ConfigStore,
    secrets: &Secrets,
    provider: ProviderKind,
) -> Vec<String> {
    let config_key = provider_config_api_key(store, provider);
    let keyring_key = provider_keyring_api_key(secrets, provider);
    let env_key = provider_env_value(provider);
    let oauth_file = provider_oauth_file_path(provider);
    let oauth_file_present = oauth_file.as_ref().is_some_and(|path| path.exists());

    let active_source = if provider == ProviderKind::XiaomiMimo {
        if env_key.is_some() {
            "env"
        } else if oauth_file_present {
            "Codex OAuth file"
        } else {
            "missing"
        }
    } else if config_key.is_some() {
        "config"
    } else if keyring_key.is_some() {
        "secret store"
    } else if env_key.is_some() {
        "env"
    } else {
        "missing"
    };
    let active_last4 = if provider == ProviderKind::XiaomiMimo {
        env_key.as_ref().map(|(_, value)| last4_label(value))
    } else {
        config_key
            .map(last4_label)
            .or_else(|| keyring_key.as_deref().map(last4_label))
            .or_else(|| env_key.as_ref().map(|(_, value)| last4_label(value)))
    };
    let active_label = active_last4
        .map(|last4| format!("{active_source} (last4: {last4})"))
        .unwrap_or_else(|| active_source.to_string());

    let env_var_label = env_key
        .as_ref()
        .map(|(name, _)| (*name).to_string())
        .unwrap_or_else(|| provider_env_vars(provider).join("/"));
    let env_status = env_key
        .as_ref()
        .map(|(_, value)| format!("set, last4: {}", last4_label(value)))
        .unwrap_or_else(|| "unset".to_string());

    let is_active = provider == store.config.provider;
    let active_marker = if is_active { " (active provider)" } else { "" };

    let provider_cfg = store.config.providers.for_provider(provider);
    let base_url = provider_cfg.base_url.as_deref().unwrap_or("(default)");
    let model = provider_cfg.model.as_deref().unwrap_or("(default)");

    let lookup_order = if provider == ProviderKind::XiaomiMimo {
        "lookup order: env -> Codex OAuth file".to_string()
    } else {
        "lookup order: config -> secret store -> env".to_string()
    };
    let auth_mode = if provider == ProviderKind::XiaomiMimo {
        "codex_oauth"
    } else {
        store.config.auth_mode.as_deref().unwrap_or("api_key")
    };

    let mut lines = vec![
        format!("provider: {}{}", provider.as_str(), active_marker),
        format!("route: {}", base_url),
        format!("model: {}", model),
        format!("auth mode: {auth_mode}"),
        format!("active source: {active_label}"),
        lookup_order,
        format!(
            "config file: {} ({})",
            store.path().display(),
            source_status(config_key, "missing")
        ),
        format!(
            "secret store: {} ({})",
            secrets.backend_name(),
            source_status(keyring_key.as_deref(), "missing")
        ),
        format!("env var: {env_var_label} ({env_status})"),
    ];
    if let Some(path) = oauth_file {
        let status = if path.exists() { "present" } else { "missing" };
        lines.push(format!("Codex OAuth file: {} ({status})", path.display()));
    }
    lines
}

fn source_status(value: Option<&str>, missing_label: &str) -> String {
    value
        .map(|v| format!("set, last4: {}", last4_label(v)))
        .unwrap_or_else(|| missing_label.to_string())
}

fn last4_label(value: &str) -> String {
    let trimmed = value.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 4 {
        return "<redacted>".to_string();
    }
    let last4: String = chars[chars.len() - 4..].iter().collect();
    format!("...{last4}")
}

fn run_auth_command(store: &mut ConfigStore, command: AuthCommand) -> Result<()> {
    run_auth_command_with_secrets(store, command, &Secrets::auto_detect())
}

fn run_auth_command_with_secrets(
    store: &mut ConfigStore,
    command: AuthCommand,
    secrets: &Secrets,
) -> Result<()> {
    match command {
        AuthCommand::Status { provider } => {
            match provider {
                Some(p) => {
                    let provider: ProviderKind = p.into();
                    for line in auth_status_lines_for_provider(store, secrets, provider) {
                        println!("{line}");
                    }
                }
                None => {
                    for line in auth_status_all_providers(store, secrets) {
                        println!("{line}");
                    }
                }
            }
            Ok(())
        }
        AuthCommand::Set {
            provider,
            api_key,
            api_key_stdin,
        } => {
            let provider: ProviderKind = provider.into();
            let slot = provider_slot(provider);
            let api_key = match (api_key, api_key_stdin) {
                (Some(v), _) => v,
                (None, true) => read_api_key_from_stdin()?,
                (None, false) => prompt_api_key(slot)?,
            };
            write_provider_api_key_to_config(store, provider, &api_key);
            let keyring_saved = write_provider_api_key_to_keyring(secrets, provider, &api_key);
            store.save()?;
            // Don't print the key. Don't echo length.
            if keyring_saved {
                println!(
                    "saved API key for {slot} to {} and {}",
                    store.path().display(),
                    secrets.backend_name()
                );
            } else {
                println!("saved API key for {slot} to {}", store.path().display());
            }
            Ok(())
        }
        AuthCommand::Get { provider } => {
            let provider: ProviderKind = provider.into();
            let slot = provider_slot(provider);
            let in_file = provider_config_set(store, provider);
            let in_keyring = !in_file && provider_keyring_set(secrets, provider);
            let in_env = provider_env_set(provider);
            // Report the highest-priority source that has it.
            let source = if in_file {
                Some("config-file")
            } else if in_keyring {
                Some("secret-store")
            } else if in_env {
                Some("env")
            } else {
                None
            };
            match source {
                Some(source) => println!("{slot}: set (source: {source})"),
                None => println!("{slot}: not set"),
            }
            Ok(())
        }
        AuthCommand::Clear { provider } => {
            let provider: ProviderKind = provider.into();
            let slot = provider_slot(provider);
            clear_provider_api_key_from_config(store, provider);
            clear_provider_api_key_from_keyring(secrets, provider);
            store.save()?;
            println!("cleared API key for {slot} from config and secret store");
            Ok(())
        }
        AuthCommand::List => {
            println!("provider     config store env  active");
            for provider in ProviderKind::ALL {
                let slot = provider_slot(provider);
                let file = provider_config_set(store, provider);
                let keyring = (!file).then(|| provider_keyring_set(secrets, provider));
                let env = provider_env_set(provider);
                let active = if file {
                    "config"
                } else if keyring == Some(true) {
                    "store"
                } else if env {
                    "env"
                } else {
                    "missing"
                };
                println!(
                    "{slot:<12}  {}     {}      {}   {active}",
                    yes_no(file),
                    keyring_status_short(keyring),
                    yes_no(env)
                );
            }
            Ok(())
        }
        AuthCommand::Migrate { dry_run } => run_auth_migrate(store, secrets, dry_run),
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no " }
}

fn keyring_status_short(state: Option<bool>) -> &'static str {
    match state {
        Some(true) => "yes",
        Some(false) => "no ",
        None => "n/a",
    }
}

fn prompt_api_key(slot: &str) -> Result<String> {
    use std::io::{IsTerminal, Write};
    eprint!("Enter API key for {slot}: ");
    io::stderr().flush().ok();
    if !io::stdin().is_terminal() {
        // Non-interactive: read directly without prompting twice.
        return read_api_key_from_stdin();
    }
    let mut buf = String::new();
    io::stdin()
        .read_line(&mut buf)
        .context("failed to read API key from stdin")?;
    let key = buf.trim().to_string();
    if key.is_empty() {
        bail!("empty API key provided");
    }
    Ok(key)
}

/// Move plaintext keys from config.toml into the configured secret store.
/// Hidden in v0.8.8 because the normal setup path is config/env only.
fn run_auth_migrate(store: &mut ConfigStore, secrets: &Secrets, dry_run: bool) -> Result<()> {
    let mut migrated: Vec<(ProviderKind, &'static str)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for provider in ProviderKind::ALL {
        let slot = provider_slot(provider);
        let from_provider_block = store
            .config
            .providers
            .for_provider(provider)
            .api_key
            .clone()
            .filter(|v| !v.trim().is_empty());
        let from_root = (provider == ProviderKind::XiaomiMimo)
            .then(|| store.config.api_key.clone())
            .flatten()
            .filter(|v| !v.trim().is_empty());
        let value = from_provider_block.or(from_root);
        let Some(value) = value else { continue };

        if let Ok(Some(existing)) = secrets.get(slot)
            && existing == value
        {
            // Already migrated; safe to strip the file slot.
        } else if dry_run {
            migrated.push((provider, slot));
            continue;
        } else if let Err(err) = secrets.set(slot, &value) {
            warnings.push(format!(
                "skipped {slot}: failed to write to secret store: {err}"
            ));
            continue;
        }
        if !dry_run {
            store.config.providers.for_provider_mut(provider).api_key = None;
            if provider == ProviderKind::XiaomiMimo {
                store.config.api_key = None;
            }
        }
        migrated.push((provider, slot));
    }

    if !dry_run && !migrated.is_empty() {
        store
            .save()
            .context("failed to write updated config.toml")?;
    }

    println!("secret store backend: {}", secrets.backend_name());
    if migrated.is_empty() {
        println!("nothing to migrate (config.toml has no plaintext api_key entries)");
    } else {
        println!(
            "{} {} provider key(s):",
            if dry_run { "would migrate" } else { "migrated" },
            migrated.len()
        );
        for (_, slot) in &migrated {
            println!("  - {slot}");
        }
        if !dry_run {
            println!(
                "config.toml at {} no longer contains api_key entries for migrated providers.",
                store.path().display()
            );
        }
    }
    for w in warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

fn run_config_command(store: &mut ConfigStore, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Get { key } => {
            if let Some(value) = store.config.get_display_value(&key) {
                println!("{value}");
                return Ok(());
            }
            bail!("key not found: {key}");
        }
        ConfigCommand::Set { key, value } => {
            store.config.set_value(&key, &value)?;
            store.save()?;
            println!("set {key}");
            Ok(())
        }
        ConfigCommand::Unset { key } => {
            store.config.unset_value(&key)?;
            store.save()?;
            println!("unset {key}");
            Ok(())
        }
        ConfigCommand::List => {
            for (key, value) in store.config.list_values() {
                println!("{key} = {value}");
            }
            Ok(())
        }
        ConfigCommand::Path => {
            println!("{}", store.path().display());
            Ok(())
        }
    }
}

fn model_command_provider_hint(
    command_provider: Option<ProviderArg>,
    top_level_provider: Option<ProviderKind>,
) -> Option<ProviderKind> {
    command_provider
        .map(ProviderKind::from)
        .or(top_level_provider)
}

fn run_model_command(
    store: &mut ConfigStore,
    command: ModelCommand,
    top_level_provider: Option<ProviderKind>,
) -> Result<()> {
    let registry = ModelRegistry::default();
    match command {
        ModelCommand::List { provider } => {
            let filter = model_command_provider_hint(provider, top_level_provider);
            for model in registry.list().into_iter().filter(|m| match filter {
                Some(p) => m.provider == p,
                None => true,
            }) {
                println!("{} ({})", model.id, model.provider.as_str());
            }
            Ok(())
        }
        ModelCommand::Resolve { model, provider } => {
            let provider = model_command_provider_hint(provider, top_level_provider);
            let resolved = registry.resolve(model.as_deref(), provider);
            println!("requested: {}", resolved.requested.unwrap_or_default());
            println!("resolved: {}", resolved.resolved.id);
            println!("provider: {}", resolved.resolved.provider.as_str());
            println!("used_fallback: {}", resolved.used_fallback);
            Ok(())
        }
        ModelCommand::Set { model } => {
            let trimmed = model.trim();
            if trimmed.is_empty() {
                bail!("Model name cannot be empty");
            }
            let canonical = match trimmed.to_ascii_lowercase().as_str() {
                "pro" | "deepseek-v4pro" => "deepseek-v4-pro",
                "flash" | "deepseek-v4flash" => "deepseek-v4-flash",
                _ => trimmed,
            };
            store.config.default_text_model = Some(canonical.to_string());
            store.save()?;
            println!("Default model set to '{canonical}'");
            Ok(())
        }
    }
}

fn run_thread_command(command: ThreadCommand) -> Result<()> {
    let state = StateStore::open(None)?;
    match command {
        ThreadCommand::List { all, limit } => {
            let threads = state.list_threads(ThreadListFilters {
                include_archived: all,
                limit,
            })?;
            for thread in threads {
                println!(
                    "{} | {} | {} | {}",
                    thread.id,
                    thread
                        .name
                        .clone()
                        .unwrap_or_else(|| "(unnamed)".to_string()),
                    thread.model_provider,
                    thread.cwd.display()
                );
            }
            Ok(())
        }
        ThreadCommand::Read { thread_id } => {
            let thread = state.get_thread(&thread_id)?;
            println!("{}", serde_json::to_string_pretty(&thread)?);
            Ok(())
        }
        ThreadCommand::Resume { thread_id } => {
            let args = vec!["resume".to_string(), thread_id];
            delegate_simple_tui(args)
        }
        ThreadCommand::Fork { thread_id } => {
            let args = vec!["fork".to_string(), thread_id];
            delegate_simple_tui(args)
        }
        ThreadCommand::Archive { thread_id } => {
            state.mark_archived(&thread_id)?;
            println!("archived {thread_id}");
            Ok(())
        }
        ThreadCommand::Unarchive { thread_id } => {
            state.mark_unarchived(&thread_id)?;
            println!("unarchived {thread_id}");
            Ok(())
        }
        ThreadCommand::SetName { thread_id, name } => {
            let mut thread = state
                .get_thread(&thread_id)?
                .with_context(|| format!("thread not found: {thread_id}"))?;
            thread.name = Some(name);
            thread.updated_at = chrono::Utc::now().timestamp();
            state.upsert_thread(&thread)?;
            println!("renamed {thread_id}");
            Ok(())
        }
        ThreadCommand::ClearName { thread_id } => {
            let mut thread = state
                .get_thread(&thread_id)?
                .with_context(|| format!("thread not found: {thread_id}"))?;
            thread.name = None;
            thread.updated_at = chrono::Utc::now().timestamp();
            state.upsert_thread(&thread)?;
            println!("cleared name for {thread_id}");
            Ok(())
        }
    }
}

fn run_sandbox_command(command: SandboxCommand) -> Result<()> {
    match command {
        SandboxCommand::Check { command, ask } => {
            let engine = ExecPolicyEngine::new(Vec::new(), vec!["rm -rf".to_string()]);
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let decision = engine.check(ExecPolicyContext {
                command: &command,
                cwd: &cwd.display().to_string(),
                tool: Some("exec_shell"),
                path: None,
                ask_for_approval: ask.into(),
                sandbox_mode: Some("workspace-write"),
            })?;
            println!("{}", serde_json::to_string_pretty(&decision)?);
            Ok(())
        }
    }
}

fn run_app_server_command(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    args: AppServerArgs,
) -> Result<()> {
    // The full runtime API lives in the TUI crate behind `serve --http`/`--mobile`.
    // Rather than duplicate ~6.5k lines or add a CLI→TUI crate dependency, the
    // canonical `app-server --http`/`--mobile` entrypoint reuses that mature server
    // by delegating to the sibling TUI binary (the same mechanism `serve` uses).
    if args.http || args.mobile {
        // Delegated runtime API listener — supervise it so the child does not
        // outlive the dispatcher (#3259).
        return delegate_server_to_tui(cli, resolved_runtime, app_server_serve_passthrough(&args));
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime")?;
    if args.stdio {
        return runtime.block_on(run_app_server_stdio(args.config));
    }
    // Legacy in-process app-server HTTP transport (`/healthz`, `/thread`, `/app`,
    // `/prompt`, `/tool`, `/jobs`). Kept for backward compatibility; defaults to
    // 127.0.0.1:8787 to avoid colliding with the runtime API default of :7878.
    let host = args.host.as_deref().unwrap_or("127.0.0.1");
    let port = args.port.unwrap_or(8787);
    let listen: SocketAddr = format!("{host}:{port}")
        .parse()
        .with_context(|| format!("invalid app-server listen address {host}:{port}"))?;
    runtime.block_on(run_app_server(AppServerOptions {
        listen,
        config_path: args.config,
        auth_token: args.auth_token.or_else(app_server_token_from_env),
        insecure_no_auth: args.insecure_no_auth,
        cors_origins: args.cors_origin,
    }))
}

/// Build the `serve` argv forwarded to the TUI binary for
/// `mimofan app-server --http`/`--mobile`. Maps app-server flags onto the
/// matching `serve` flags (note `--insecure-no-auth` → `--insecure`). The
/// subcommand-level `--config` is bridged through the global `--config` in the
/// dispatcher, so it is intentionally not part of this passthrough. An auth
/// token from the environment is deliberately *not* forwarded into child argv;
/// the runtime API reads CODEWHALE_RUNTIME_TOKEN/DEEPSEEK_RUNTIME_TOKEN itself.
fn app_server_serve_passthrough(args: &AppServerArgs) -> Vec<String> {
    let mut forwarded = vec!["serve".to_string()];
    forwarded.push(if args.mobile { "--mobile" } else { "--http" }.to_string());
    if let Some(host) = args.host.as_ref() {
        forwarded.push("--host".to_string());
        forwarded.push(host.clone());
    }
    if let Some(port) = args.port {
        forwarded.push("--port".to_string());
        forwarded.push(port.to_string());
    }
    if let Some(workers) = args.workers {
        forwarded.push("--workers".to_string());
        forwarded.push(workers.to_string());
    }
    for origin in &args.cors_origin {
        forwarded.push("--cors-origin".to_string());
        forwarded.push(origin.clone());
    }
    if let Some(token) = args.auth_token.as_ref() {
        forwarded.push("--auth-token".to_string());
        forwarded.push(token.clone());
    }
    if args.insecure_no_auth {
        forwarded.push("--insecure".to_string());
    }
    if args.qr {
        forwarded.push("--qr".to_string());
    }
    forwarded
}

fn app_server_token_from_env() -> Option<String> {
    std::env::var("CODEWHALE_APP_SERVER_TOKEN")
        .ok()
        .or_else(|| std::env::var("DEEPSEEK_APP_SERVER_TOKEN").ok())
}

fn run_mcp_server_command(store: &mut ConfigStore) -> Result<()> {
    let persisted = load_mcp_server_definitions(store);
    let updated = run_stdio_server(persisted)?;
    persist_mcp_server_definitions(store, &updated)
}

fn load_mcp_server_definitions(store: &ConfigStore) -> Vec<McpServerDefinition> {
    let Some(raw) = store.config.get_value(MCP_SERVER_DEFINITIONS_KEY) else {
        return Vec::new();
    };

    match parse_mcp_server_definitions(&raw) {
        Ok(definitions) => definitions,
        Err(err) => {
            eprintln!(
                "warning: failed to parse persisted MCP server definitions ({MCP_SERVER_DEFINITIONS_KEY}): {err}"
            );
            Vec::new()
        }
    }
}

fn parse_mcp_server_definitions(raw: &str) -> Result<Vec<McpServerDefinition>> {
    if let Ok(parsed) = serde_json::from_str::<Vec<McpServerDefinition>>(raw) {
        return Ok(parsed);
    }

    let unwrapped: String = serde_json::from_str(raw)
        .with_context(|| format!("invalid JSON payload at key {MCP_SERVER_DEFINITIONS_KEY}"))?;
    serde_json::from_str::<Vec<McpServerDefinition>>(&unwrapped).with_context(|| {
        format!("invalid MCP server definition list in key {MCP_SERVER_DEFINITIONS_KEY}")
    })
}

fn persist_mcp_server_definitions(
    store: &mut ConfigStore,
    definitions: &[McpServerDefinition],
) -> Result<()> {
    let encoded =
        serde_json::to_string(definitions).context("failed to encode MCP server definitions")?;
    store
        .config
        .set_value(MCP_SERVER_DEFINITIONS_KEY, &encoded)?;
    store.save()
}

fn delegate_to_tui(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<()> {
    let mut cmd = build_tui_command(cli, resolved_runtime, passthrough)?;
    let tui = PathBuf::from(cmd.get_program());
    let status = cmd
        .status()
        .map_err(|err| anyhow!("{}", tui_spawn_error(&tui, &err)))?;
    exit_with_tui_status(status)
}

/// Delegate a long-running server command (`serve --http`/`--mobile`,
/// `app-server --http`/`--mobile`) to the sibling TUI binary, supervising the
/// child so its listener does not outlive the dispatcher (#3259).
///
/// Plain [`delegate_to_tui`] blocks on `Command::status()`, which reaps the
/// child only on the child's own exit. If the dispatcher is terminated while
/// the delegated server is still running, the child can be reparented and keep
/// its listener bound. Here the child runs under a Tokio supervisor that
/// forwards termination (Ctrl+C / SIGTERM / SIGHUP) by killing and reaping the
/// child before the dispatcher exits, and `kill_on_drop` tears the child down
/// if the dispatcher unwinds.
///
/// For an *uncatchable* dispatcher death (SIGKILL, a hard crash) the Tokio
/// supervisor above can't run, so two OS-level safety nets are installed as
/// well (#3259): on Linux the child sets `PR_SET_PDEATHSIG` so the kernel
/// signals it when the dispatcher dies; on Windows the child is placed in a
/// kill-on-job-close Job Object so closing the dispatcher's handle (which the
/// OS does on process death) terminates it. macOS has no equivalent primitive,
/// so an uncatchable dispatcher death there can still orphan the child.
fn delegate_server_to_tui(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<()> {
    let mut std_cmd = build_tui_command(cli, resolved_runtime, passthrough)?;
    install_server_parent_death_signal(&mut std_cmd);
    let tui = PathBuf::from(std_cmd.get_program());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create server-teardown runtime")?;
    runtime.block_on(async move {
        let mut cmd = tokio::process::Command::from(std_cmd);
        cmd.kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|err| anyhow!("{}", tui_spawn_error(&tui, &err)))?;
        // Windows: hold a kill-on-job-close Job Object for the dispatcher's
        // lifetime so an uncatchable dispatcher death tears the child down.
        // Bound for the whole `block_on` scope; never dropped early because the
        // match arms below `std::process::exit`.
        #[cfg(windows)]
        let _child_job = attach_server_child_job(&child);
        match supervise_server_child(&mut child, server_shutdown_signal()).await? {
            ServerTeardown::Exited(status) => exit_with_tui_status(status),
            // The child has been killed and reaped; exit with the conventional
            // 128 + signal code for the signal that initiated the shutdown.
            ServerTeardown::Signaled(code) => std::process::exit(code),
        }
    })
}

/// On Linux, ask the kernel to terminate the delegated server if the dispatcher
/// dies before it can run the graceful shutdown supervisor. This covers the
/// hard parent-death edge of #3259 for `SIGKILL`, OOM, or abrupt process exit.
#[cfg(all(target_os = "linux", not(target_env = "ohos")))]
fn install_server_parent_death_signal(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: `pre_exec` runs in the child between fork and exec. The closure
    // only calls `libc::prctl` with constant arguments and does not touch heap
    // memory or parent-held locks.
    unsafe {
        cmd.pre_exec(|| {
            let result = libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0);
            if result == -1 {
                // Best effort: the child only loses this OS-level safety net.
                let _ = std::io::Error::last_os_error();
            }
            Ok(())
        });
    }
}

#[cfg(not(all(target_os = "linux", not(target_env = "ohos"))))]
fn install_server_parent_death_signal(_cmd: &mut Command) {}

/// Outcome of supervising a delegated server child.
#[derive(Debug)]
enum ServerTeardown {
    /// The child exited on its own; its status is carried for propagation.
    Exited(std::process::ExitStatus),
    /// A shutdown signal fired; the child was killed and reaped. Carries the
    /// conventional `128 + signal` exit code to propagate.
    Signaled(i32),
}

/// Wait for the server `child` to exit, or for `shutdown` to fire first. On
/// shutdown, kill the child and reap it so no listener is left reparented.
async fn supervise_server_child<F>(
    child: &mut tokio::process::Child,
    shutdown: F,
) -> io::Result<ServerTeardown>
where
    F: std::future::Future<Output = i32>,
{
    tokio::select! {
        status = child.wait() => Ok(ServerTeardown::Exited(status?)),
        code = shutdown => {
            // Send the kill, then wait so the PID is reaped before the
            // dispatcher returns and exits.
            let _ = child.start_kill();
            let _ = child.wait().await;
            Ok(ServerTeardown::Signaled(code))
        }
    }
}

/// Resolve when the dispatcher should tear down a delegated server child, and
/// the conventional `128 + signal` exit code to propagate: Ctrl+C on every
/// platform (130), plus SIGTERM (143) and SIGHUP (129) on Unix.
#[cfg(unix)]
async fn server_shutdown_signal() -> i32 {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate()).ok();
    let mut hangup = signal(SignalKind::hangup()).ok();
    let term = async {
        match terminate.as_mut() {
            Some(s) => {
                s.recv().await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    let hup = async {
        match hangup.as_mut() {
            Some(s) => {
                s.recv().await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => 130,
        _ = term => 143,
        _ = hup => 129,
    }
}

#[cfg(not(unix))]
async fn server_shutdown_signal() -> i32 {
    let _ = tokio::signal::ctrl_c().await;
    130
}

/// Assign the delegated server `child` to a kill-on-job-close Job Object so the
/// OS terminates it when the dispatcher's handle to the job closes — which it
/// does on any dispatcher exit, including an uncatchable kill (#3259). The
/// returned guard must be held for the dispatcher's lifetime. Best-effort:
/// returns `None` if the job cannot be created or assigned. Mirrors the Job
/// Object idiom in `crates/tui/src/tools/shell.rs`.
#[cfg(windows)]
fn attach_server_child_job(child: &tokio::process::Child) -> Option<ServerChildJob> {
    let Some(child_handle) = child.raw_handle() else {
        tracing::warn!("delegated server child exited before a job object could be attached");
        return None;
    };

    match ServerChildJob::attach(child_handle) {
        Ok(job) => Some(job),
        Err(err) => {
            tracing::warn!("failed to place delegated server child in a job object: {err}");
            None
        }
    }
}

#[cfg(windows)]
struct ServerChildJob {
    handle: windows::Win32::Foundation::HANDLE,
}

// SAFETY: the wrapped value is a process-wide kernel handle; moving it across
// threads does not invalidate it, and it is only ever closed once, on drop.
#[cfg(windows)]
unsafe impl Send for ServerChildJob {}

#[cfg(windows)]
impl ServerChildJob {
    fn attach(child_handle: std::os::windows::io::RawHandle) -> std::io::Result<Self> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        use windows::core::PCWSTR;

        // SAFETY: FFI calls with valid arguments; results are checked via the
        // `windows` Result wrappers and the handle is stored for close-on-drop.
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(win_io_error)?;
        let job = Self { handle };

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(win_io_error)?;
            AssignProcessToJobObject(job.handle, HANDLE(child_handle)).map_err(win_io_error)?;
        }
        Ok(job)
    }
}

#[cfg(windows)]
impl Drop for ServerChildJob {
    fn drop(&mut self) {
        // Closing the last handle triggers KILL_ON_JOB_CLOSE. On a normal return
        // the child has already been reaped, so this is a no-op cleanup; an
        // uncatchable dispatcher death closes the handle via the OS instead.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
fn win_io_error(err: windows::core::Error) -> std::io::Error {
    std::io::Error::other(err)
}

#[cfg(all(test, unix))]
mod server_teardown_tests {
    use super::*;

    #[tokio::test]
    async fn supervisor_propagates_child_exit_when_no_shutdown() {
        // `true` exits immediately with success; a never-firing shutdown must
        // let the child's own exit win.
        let mut child = tokio::process::Command::new("true")
            .kill_on_drop(true)
            .spawn()
            .expect("spawn true");
        let outcome = supervise_server_child(&mut child, std::future::pending::<i32>())
            .await
            .expect("supervise");
        match outcome {
            ServerTeardown::Exited(status) => assert!(status.success()),
            other => panic!("expected Exited, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn shutdown_signal_kills_and_reaps_long_running_child() {
        // A long-lived child stands in for the delegated server listener; the
        // regression is that it outlives dispatcher teardown (#3259).
        let mut child = tokio::process::Command::new("sleep")
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .expect("spawn sleep");
        assert!(
            child.id().is_some(),
            "child should be running before shutdown"
        );
        // A ready future models an immediate shutdown signal carrying the
        // SIGTERM exit code (143).
        let outcome = supervise_server_child(&mut child, async { 143 })
            .await
            .expect("supervise");
        assert!(matches!(outcome, ServerTeardown::Signaled(143)));
        // Once supervise returns the child has been killed AND reaped, so tokio
        // drops the recorded pid — no listener is left reparented.
        assert!(
            child.id().is_none(),
            "delegated child must be reaped after dispatcher teardown"
        );
    }

    #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
    #[test]
    fn parent_death_signal_hook_does_not_break_spawn() {
        let mut cmd = Command::new("true");
        install_server_parent_death_signal(&mut cmd);
        let status = cmd.status().expect("spawn true with parent-death hook");
        assert!(status.success());
    }
}

fn run_resume_command(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    args: TuiPassthroughArgs,
) -> Result<()> {
    let passthrough = tui_args("resume", args);
    if should_pick_resume_in_dispatcher(&passthrough, cfg!(windows)) {
        return run_dispatcher_resume_picker(cli, resolved_runtime);
    }
    delegate_to_tui(cli, resolved_runtime, passthrough)
}

fn run_dispatcher_resume_picker(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
) -> Result<()> {
    let mut sessions_cmd = build_tui_command(cli, resolved_runtime, vec!["sessions".to_string()])?;
    let tui = PathBuf::from(sessions_cmd.get_program());
    let status = sessions_cmd
        .status()
        .map_err(|err| anyhow!("{}", tui_spawn_error(&tui, &err)))?;
    if !status.success() {
        return exit_with_tui_status(status);
    }

    println!();
    println!("Windows note: enter a session id or prefix from the list above.");
    println!("You can also run `mimofan resume --last` to skip this prompt.");
    print!("Session id/prefix (Enter to cancel): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read session selection")?;
    let session_id = input.trim();
    if session_id.is_empty() {
        bail!("No session selected.");
    }

    delegate_to_tui(
        cli,
        resolved_runtime,
        vec!["resume".to_string(), session_id.to_string()],
    )
}

fn should_pick_resume_in_dispatcher(passthrough: &[String], is_windows: bool) -> bool {
    is_windows && passthrough == ["resume"]
}

fn build_tui_command(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<Command> {
    let tui = locate_sibling_tui_binary()?;
    let mut verbosity = resolved_runtime.verbosity.clone();
    if verbosity.is_none()
        && passthrough
            .iter()
            .any(|arg| matches!(arg.as_str(), "exec" | "eval"))
    {
        verbosity = Some("concise".to_string());
    }

    let mut cmd = Command::new(&tui);
    if let Some(config) = cli.config.as_ref() {
        cmd.arg("--config").arg(config);
    }
    if let Some(profile) = cli.profile.as_ref() {
        cmd.arg("--profile").arg(profile);
    }
    if let Some(workspace) = cli.workspace.as_ref() {
        cmd.arg("--workspace").arg(workspace);
    }
    // Accepted for older scripts, but no longer forwarded: the interactive TUI
    // always owns the alternate screen to avoid host scrollback hijacking.
    let _ = cli.no_alt_screen;
    if cli.mouse_capture {
        cmd.arg("--mouse-capture");
    }
    if cli.no_mouse_capture {
        cmd.arg("--no-mouse-capture");
    }
    if cli.skip_onboarding {
        cmd.arg("--skip-onboarding");
    }
    cmd.args(passthrough);

    let keyring_bridge_provider = resolved_runtime.provider;
    let keyring_bridge_api_key = resolved_runtime.api_key.as_ref();
    let keyring_bridge_source = resolved_runtime.api_key_source;

    if let Some(provider) = cli.provider.map(ProviderKind::from) {
        cmd.env("DEEPSEEK_PROVIDER", provider.as_str());
    }
    if matches!(keyring_bridge_source, Some(RuntimeApiKeySource::Keyring))
        && let Some(api_key) = keyring_bridge_api_key
    {
        // TUI reloads auth_mode from config/profile, but it does not re-query the
        // platform keyring on normal startup. Bridge only the recovered secret;
        // replaying auth_mode here would turn it back into a profile override.
        cmd.env("DEEPSEEK_API_KEY", api_key);
        for var in provider_env_vars(keyring_bridge_provider) {
            if *var != "DEEPSEEK_API_KEY" {
                cmd.env(var, api_key);
            }
        }
        cmd.env(
            "DEEPSEEK_API_KEY_SOURCE",
            RuntimeApiKeySource::Keyring.as_env_value(),
        );
    }

    if let Some(model) = cli.model.as_ref() {
        cmd.env("DEEPSEEK_MODEL", model);
    }
    if let Some(output_mode) = cli.output_mode.as_ref() {
        cmd.env("DEEPSEEK_OUTPUT_MODE", output_mode);
    }
    if let Some(v) = verbosity.as_ref() {
        cmd.env("CODEWHALE_VERBOSITY", v);
        cmd.env("DEEPSEEK_VERBOSITY", v);
    }
    if let Some(log_level) = cli.log_level.as_ref() {
        cmd.env("DEEPSEEK_LOG_LEVEL", log_level);
    }
    if let Some(telemetry) = cli.telemetry {
        cmd.env("DEEPSEEK_TELEMETRY", telemetry.to_string());
    }
    if let Some(policy) = cli.approval_policy.as_ref() {
        cmd.env("DEEPSEEK_APPROVAL_POLICY", policy);
    }
    if let Some(mode) = cli.sandbox_mode.as_ref() {
        cmd.env("DEEPSEEK_SANDBOX_MODE", mode);
    }
    if cli.yolo {
        cmd.env("DEEPSEEK_YOLO", "true");
    }
    if let Some(api_key) = cli.api_key.as_ref() {
        cmd.env("DEEPSEEK_API_KEY", api_key);
        for var in provider_env_vars(resolved_runtime.provider) {
            if *var != "DEEPSEEK_API_KEY" {
                cmd.env(var, api_key);
            }
        }
        cmd.env("DEEPSEEK_API_KEY_SOURCE", "cli");
    }
    if let Some(base_url) = cli.base_url.as_ref() {
        cmd.env("DEEPSEEK_BASE_URL", base_url);
    }

    Ok(cmd)
}

fn exit_with_tui_status(status: std::process::ExitStatus) -> Result<()> {
    match status.code() {
        Some(code) => std::process::exit(code),
        None => bail!("mimofan-tui terminated by signal"),
    }
}

fn delegate_simple_tui(args: Vec<String>) -> Result<()> {
    let tui = locate_sibling_tui_binary()?;
    let status = Command::new(&tui)
        .args(args)
        .status()
        .map_err(|err| anyhow!("{}", tui_spawn_error(&tui, &err)))?;
    match status.code() {
        Some(code) => std::process::exit(code),
        None => bail!("mimofan-tui terminated by signal"),
    }
}

fn tui_spawn_error(tui: &Path, err: &io::Error) -> String {
    format!(
        "failed to spawn companion TUI binary at {}: {err}\n\
\n\
The `mimofan` dispatcher found a `mimofan-tui` file, but the OS refused \
to execute it. Common fixes:\n\
  - Reinstall with `npm install -g mimofan`, or run `mimofan update`.\n\
  - On Windows, run `where mimofan` and `where mimofan-tui`; both should \
come from the same install directory.\n\
  - If you downloaded release assets manually, keep both `mimofan` and \
`mimofan-tui` binaries together and make sure the TUI binary is executable.\n\
  - Set DEEPSEEK_TUI_BIN to the absolute path of a working `mimofan-tui` \
binary.",
        tui.display()
    )
}

/// Resolve the sibling `mimofan` executable next to the running
/// dispatcher. Honours platform executable suffix (`.exe` on Windows) so
/// the npm-distributed Windows package — which ships
/// `bin/downloads/mimofan.exe` — is found by `Path::exists` (#247).
///
/// `DEEPSEEK_TUI_BIN` is consulted first as an explicit override for
/// custom installs and CI test layouts. On Windows we additionally try
/// the suffix-less name as a fallback for users who already manually
/// renamed the file before this fix landed.
fn locate_sibling_tui_binary() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("DEEPSEEK_TUI_BIN") {
        let candidate = PathBuf::from(override_path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        bail!(
            "DEEPSEEK_TUI_BIN points at {}, which is not a regular file.",
            candidate.display()
        );
    }

    let current = std::env::current_exe().context("failed to locate current executable path")?;
    if let Some(found) = sibling_tui_candidate(&current) {
        return Ok(found);
    }

    // Build a stable error path so the user sees the platform-correct
    // expected name, not "mimofan" on Windows.
    let expected = current.with_file_name(format!("mimofan-tui{}", std::env::consts::EXE_SUFFIX));
    bail!(
        "Companion `mimofan-tui` binary not found at {}.\n\
\n\
The `mimofan` dispatcher delegates interactive sessions to a sibling \
`mimofan-tui` binary. To fix this, install one of:\n\
  • npm:    npm install -g mimofan                (downloads both binaries)\n\
  • cargo:  cargo install mimofan-cli mimofan-tui --locked\n\
  • GitHub Releases: download BOTH `mimofan-<platform>` AND \
`mimofan-tui-<platform>` from https://github.com/XiaomingX/mimofan/releases/latest \
and place them in the same directory.\n\
\n\
Or set DEEPSEEK_TUI_BIN to the absolute path of an existing `mimofan-tui` binary.",
        expected.display()
    );
}

/// Return the first existing sibling-binary path under any of the names
/// `mimofan` might use on this platform. Pure function to keep
/// `locate_sibling_tui_binary` testable.
fn sibling_tui_candidate(dispatcher: &Path) -> Option<PathBuf> {
    // Primary: platform-correct name. EXE_SUFFIX is "" on Unix and ".exe"
    // on Windows.
    let primary = dispatcher.with_file_name(format!("mimofan-tui{}", std::env::consts::EXE_SUFFIX));
    if primary.is_file() {
        return Some(primary);
    }
    // Windows fallback: a user who manually renamed `.exe` away (per the
    // workaround in #247) still launches successfully under the new code.
    if cfg!(windows) {
        let suffixless = dispatcher.with_file_name("mimofan-tui");
        if suffixless.is_file() {
            return Some(suffixless);
        }
    }
    None
}

fn run_metrics_command(args: MetricsArgs) -> Result<()> {
    let since = match args.since.as_deref() {
        Some(s) => {
            Some(metrics::parse_since(s).with_context(|| format!("invalid --since value: {s:?}"))?)
        }
        None => None,
    };
    metrics::run(metrics::MetricsArgs {
        json: args.json,
        since,
    })
}

fn read_api_key_from_stdin() -> Result<String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("failed to read api key from stdin")?;
    let key = input.trim().to_string();
    if key.is_empty() {
        bail!("empty API key provided");
    }
    Ok(key)
}

#[cfg(test)]
mod tests {}
