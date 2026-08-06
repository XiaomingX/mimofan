#![allow(clippy::uninlined_format_args)]

pub mod metrics;
pub(crate) mod update;

use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, CommandFactory, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use mimofan_agent::ModelRegistry;
use mimofan_app_server::{
    AppServerOptions, run as run_app_server, run_stdio as run_app_server_stdio,
};
use mimofan_config::{
    CliRuntimeOverrides, ConfigStore, ProviderKind, ResolvedRuntimeOptions, RuntimeApiKeySource,
};
use mimofan_mcp::{McpServerDefinition, run_stdio_server};
use mimofan_secrets::Secrets;
use mimofan_state::{StateStore, ThreadListFilters};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ProviderArg {
    /// OpenAI-compatible `/v1/chat/completions` endpoint.
    #[value(alias = "openai", alias = "openai-compatible", alias = "custom")]
    OpenAiCompatible,
    /// Anthropic Messages API compatible endpoint.
    #[value(alias = "anthropic", alias = "anthropic-compatible")]
    AnthropicCompatible,
    /// Google Gemini compatible endpoint.
    #[value(alias = "gemini", alias = "gemini-compatible", alias = "google")]
    GeminiCompatible,
}

impl From<ProviderArg> for ProviderKind {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::OpenAiCompatible => ProviderKind::OpenAiCompatible,
            ProviderArg::AnthropicCompatible => ProviderKind::AnthropicCompatible,
            ProviderArg::GeminiCompatible => ProviderKind::GeminiCompatible,
        }
    }
}

// ── Auth command types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct AuthArgs {
    #[command(subcommand)]
    pub(crate) command: AuthCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum AuthCommand {
    /// Show current provider and credential source state.
    Status {
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
    },
    /// Save an API key to the shared user config file.
    Set {
        #[arg(long, value_enum)]
        provider: ProviderArg,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long = "api-key-stdin", default_value_t = false)]
        api_key_stdin: bool,
    },
    /// Report whether a provider has a key configured.
    Get {
        #[arg(long, value_enum)]
        provider: ProviderArg,
    },
    /// Delete a provider's key from config and secret-store storage.
    Clear {
        #[arg(long, value_enum)]
        provider: ProviderArg,
    },
    /// List all known providers with their auth state.
    List,
    /// Advanced: migrate config-file keys into a platform credential store.
    #[command(hide = true)]
    Migrate {
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

// ── Config command types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum ConfigCommand {
    Get { key: String },
    Set { key: String, value: String },
    Unset { key: String },
    List,
    Path,
}

// ── Model command types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct ModelArgs {
    #[command(subcommand)]
    pub(crate) command: ModelCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum ModelCommand {
    List {
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
    },
    Resolve {
        model: Option<String>,
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
    },
    Set {
        model: String,
    },
}

// ── Thread command types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct ThreadArgs {
    #[command(subcommand)]
    pub(crate) command: ThreadCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum ThreadCommand {
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
    ClearName {
        thread_id: String,
    },
}

// ── Login command types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct LoginArgs {
    #[arg(long, value_enum, hide = true)]
    pub(crate) provider: Option<ProviderArg>,
    #[arg(long)]
    pub(crate) api_key: Option<String>,
}

// ── AppServer command types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct AppServerArgs {
    #[arg(long, conflicts_with_all = ["stdio", "mobile"])]
    pub(crate) http: bool,
    #[arg(long, conflicts_with = "stdio")]
    pub(crate) mobile: bool,
    #[arg(long, default_value_t = false)]
    pub(crate) stdio: bool,
    #[arg(long, requires = "mobile")]
    pub(crate) qr: bool,
    #[arg(long)]
    pub(crate) host: Option<String>,
    #[arg(long)]
    pub(crate) port: Option<u16>,
    #[arg(long)]
    pub(crate) workers: Option<usize>,
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "auth-token")]
    pub(crate) auth_token: Option<String>,
    #[arg(long, default_value_t = false)]
    pub(crate) insecure_no_auth: bool,
    #[arg(long = "cors-origin")]
    pub(crate) cors_origin: Vec<String>,
}

// ── Metrics command types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct MetricsArgs {
    #[arg(long)]
    pub(crate) json: bool,
    #[arg(long, value_name = "DURATION")]
    pub(crate) since: Option<String>,
}

// ── Update command types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct UpdateArgs {
    #[arg(long)]
    pub(crate) beta: bool,
    #[arg(long)]
    pub(crate) check: bool,
    #[arg(long, value_name = "URL")]
    pub(crate) proxy: Option<String>,
}

// ── Helper functions ────────────────────────────────────────────────────────

const MCP_SERVER_DEFINITIONS_KEY: &str = "mcp.server_definitions";

pub(crate) fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

// ── Auth command implementation ─────────────────────────────────────────────

pub(crate) fn run_auth_command(store: &mut ConfigStore, command: AuthCommand) -> Result<()> {
    run_auth_command_with_secrets(store, command, &Secrets::auto_detect())
}

pub(crate) fn run_auth_command_with_secrets(
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

pub(crate) fn auth_status_all_providers(store: &ConfigStore, secrets: &Secrets) -> Vec<String> {
    let active_provider = store.config.provider;
    let mut lines = Vec::new();
    lines.push(format!(
        "active provider: {} (set via config or MIMOFAN_PROVIDER)",
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

        let config_status = config_key.map(|_| "set").unwrap_or("-");
        let keyring_status = keyring_key.as_ref().map(|_| "set").unwrap_or("-");
        let env_status = env_key.as_ref().map(|_| "set").unwrap_or("-");

        let source = if config_key.is_some() {
            "config"
        } else if keyring_key.is_some() {
            "keyring"
        } else if env_key.is_some() {
            "env"
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
    lines.push("* = active provider (from config or MIMOFAN_PROVIDER)".to_string());
    lines.push("Run `mimofan auth status --provider <id>` for detailed info.".to_string());
    lines
}

pub(crate) fn auth_status_lines_for_provider(
    store: &ConfigStore,
    secrets: &Secrets,
    provider: ProviderKind,
) -> Vec<String> {
    let config_key = provider_config_api_key(store, provider);
    let keyring_key = provider_keyring_api_key(secrets, provider);
    let env_key = provider_env_value(provider);

    let active_source = if config_key.is_some() {
        "config"
    } else if keyring_key.is_some() {
        "secret store"
    } else if env_key.is_some() {
        "env"
    } else {
        "missing"
    };
    let active_last4 = config_key
        .map(last4_label)
        .or_else(|| keyring_key.as_deref().map(last4_label))
        .or_else(|| env_key.as_ref().map(|(_, value)| last4_label(value)));
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

    let lookup_order = "lookup order: config -> secret store -> env".to_string();
    let auth_mode = store.config.auth_mode.as_deref().unwrap_or("api_key");

    let lines = vec![
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
    lines
}

// ── Config command implementation ───────────────────────────────────────────

pub(crate) fn run_config_command(store: &mut ConfigStore, command: ConfigCommand) -> Result<()> {
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

// ── Model command implementation ────────────────────────────────────────────

pub(crate) fn model_command_provider_hint(
    command_provider: Option<ProviderArg>,
    top_level_provider: Option<ProviderKind>,
) -> Option<ProviderKind> {
    command_provider
        .map(ProviderKind::from)
        .or(top_level_provider)
}

pub(crate) fn run_model_command(
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
            let canonical = mimofan_config::canonical_model_name(trimmed)
                .map(|s| s.to_string())
                .unwrap_or_else(|| trimmed.to_string());
            store.config.default_text_model = Some(canonical.to_string());
            store.save()?;
            println!("Default model set to '{canonical}'");
            Ok(())
        }
    }
}

// ── Thread command implementation ───────────────────────────────────────────

pub(crate) fn run_thread_command(command: ThreadCommand) -> Result<()> {
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

// ── AppServer command implementation ────────────────────────────────────────

pub(crate) fn run_app_server_command(
    cli_config: Option<PathBuf>,
    cli_profile: Option<String>,
    cli_workspace: Option<PathBuf>,
    resolved_runtime: &ResolvedRuntimeOptions,
    args: AppServerArgs,
) -> Result<()> {
    if args.http || args.mobile {
        return delegate_server_to_tui(
            cli_config,
            cli_profile,
            cli_workspace,
            resolved_runtime,
            app_server_serve_passthrough(&args),
        );
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime")?;
    if args.stdio {
        return runtime.block_on(run_app_server_stdio(args.config));
    }
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

pub(crate) fn app_server_serve_passthrough(args: &AppServerArgs) -> Vec<String> {
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

pub(crate) fn app_server_token_from_env() -> Option<String> {
    std::env::var("MIMOFAN_APP_SERVER_TOKEN")
        .ok()
        .or_else(|| std::env::var("MIMOFAN_APP_SERVER_TOKEN").ok())
}

// ── MCP Server command implementation ───────────────────────────────────────

pub(crate) fn run_mcp_server_command(store: &mut ConfigStore) -> Result<()> {
    let persisted = load_mcp_server_definitions(store);
    let updated = run_stdio_server(persisted)?;
    persist_mcp_server_definitions(store, &updated)
}

pub(crate) fn load_mcp_server_definitions(store: &ConfigStore) -> Vec<McpServerDefinition> {
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

pub(crate) fn parse_mcp_server_definitions(raw: &str) -> Result<Vec<McpServerDefinition>> {
    if let Ok(parsed) = serde_json::from_str::<Vec<McpServerDefinition>>(raw) {
        return Ok(parsed);
    }

    let unwrapped: String = serde_json::from_str(raw)
        .with_context(|| format!("invalid JSON payload at key {MCP_SERVER_DEFINITIONS_KEY}"))?;
    serde_json::from_str::<Vec<McpServerDefinition>>(&unwrapped).with_context(|| {
        format!("invalid MCP server definition list in key {MCP_SERVER_DEFINITIONS_KEY}")
    })
}

pub(crate) fn persist_mcp_server_definitions(
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

// ── Login command implementation ────────────────────────────────────────────

pub(crate) fn run_login_command(store: &mut ConfigStore, args: LoginArgs) -> Result<()> {
    run_login_command_with_secrets(store, args, &Secrets::auto_detect())
}

pub(crate) fn run_login_command_with_secrets(
    store: &mut ConfigStore,
    args: LoginArgs,
    secrets: &Secrets,
) -> Result<()> {
    let provider: ProviderKind = args.provider.unwrap_or(ProviderArg::OpenAiCompatible).into();
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
    println!(
        "logged in using API key mode ({}); saved key to {destination}",
        provider.as_str(),
    );
    Ok(())
}

// ── Logout command implementation ───────────────────────────────────────────

pub(crate) fn run_logout_command(store: &mut ConfigStore) -> Result<()> {
    run_logout_command_with_secrets(store, &Secrets::auto_detect())
}

pub(crate) fn run_logout_command_with_secrets(
    store: &mut ConfigStore,
    secrets: &Secrets,
) -> Result<()> {
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

// ── Metrics command implementation ──────────────────────────────────────────

pub(crate) fn run_metrics_command(args: MetricsArgs) -> Result<()> {
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

// ── Completion command implementation ───────────────────────────────────────

pub(crate) fn generate_completions_from_cli(shell: Shell) {
    // Use the crate's own Cli parser for completions
    let mut cmd = <super::Cli as CommandFactory>::command();
    generate(shell, &mut cmd, "mimofan", &mut io::stdout());
}

// ── Helper functions ────────────────────────────────────────────────────────

pub(crate) fn provider_slot(provider: ProviderKind) -> &'static str {
    provider.provider().id()
}

pub(crate) fn write_provider_api_key_to_config(
    store: &mut ConfigStore,
    provider: ProviderKind,
    api_key: &str,
) {
    store.config.auth_mode = Some("api_key".to_string());
    store.config.providers.for_provider_mut(provider).api_key = Some(api_key.to_string());
}

pub(crate) fn clear_provider_api_key_from_config(store: &mut ConfigStore, provider: ProviderKind) {
    store.config.providers.for_provider_mut(provider).api_key = None;
}

pub(crate) fn provider_env_set(provider: ProviderKind) -> bool {
    provider_env_value(provider).is_some()
}

pub(crate) fn provider_env_vars(provider: ProviderKind) -> &'static [&'static str] {
    provider.provider().env_vars()
}

pub(crate) fn provider_env_value(provider: ProviderKind) -> Option<(&'static str, String)> {
    provider_env_vars(provider).iter().find_map(|var| {
        std::env::var(var)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| (*var, value))
    })
}

pub(crate) fn provider_config_api_key(store: &ConfigStore, provider: ProviderKind) -> Option<&str> {
    store
        .config
        .providers
        .for_provider(provider)
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
}

pub(crate) fn provider_config_set(store: &ConfigStore, provider: ProviderKind) -> bool {
    provider_config_api_key(store, provider).is_some()
}

pub(crate) fn provider_keyring_api_key(
    secrets: &Secrets,
    provider: ProviderKind,
) -> Option<String> {
    secrets
        .get(provider_slot(provider))
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty())
}

pub(crate) fn provider_keyring_set(secrets: &Secrets, provider: ProviderKind) -> bool {
    provider_keyring_api_key(secrets, provider).is_some()
}

pub(crate) fn write_provider_api_key_to_keyring(
    secrets: &Secrets,
    provider: ProviderKind,
    api_key: &str,
) -> bool {
    secrets.set(provider_slot(provider), api_key).is_ok()
}

pub(crate) fn clear_provider_api_key_from_keyring(secrets: &Secrets, provider: ProviderKind) {
    let _ = secrets.delete(provider_slot(provider));
}

pub(crate) fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no " }
}

pub(crate) fn keyring_status_short(state: Option<bool>) -> &'static str {
    match state {
        Some(true) => "yes",
        Some(false) => "no ",
        None => "n/a",
    }
}

pub(crate) fn prompt_api_key(slot: &str) -> Result<String> {
    use std::io::IsTerminal;
    eprint!("Enter API key for {slot}: ");
    io::stderr().flush().ok();
    if !io::stdin().is_terminal() {
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

pub(crate) fn read_api_key_from_stdin() -> Result<String> {
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

pub(crate) fn run_auth_migrate(
    store: &mut ConfigStore,
    secrets: &Secrets,
    dry_run: bool,
) -> Result<()> {
    let mut migrated: Vec<(ProviderKind, &'static str)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for provider in ProviderKind::ALL {
        let slot = provider_slot(provider);
        let value = store
            .config
            .providers
            .for_provider(provider)
            .api_key
            .clone()
            .filter(|v| !v.trim().is_empty());
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

// ── TUI delegation functions ────────────────────────────────────────────────

pub(crate) fn delegate_simple_tui(args: Vec<String>) -> Result<()> {
    let tui = std::env::current_exe().context("failed to locate current executable path")?;
    let status = Command::new(&tui)
        .args(args)
        .status()
        .map_err(|err| anyhow!("failed to spawn TUI binary: {err}"))?;
    match status.code() {
        Some(code) => std::process::exit(code),
        None => bail!("mimofan terminated by signal"),
    }
}

pub(crate) fn delegate_server_to_tui(
    cli_config: Option<PathBuf>,
    cli_profile: Option<String>,
    cli_workspace: Option<PathBuf>,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<()> {
    let mut cmd = build_tui_command_for_server(
        cli_config,
        cli_profile,
        cli_workspace,
        resolved_runtime,
        passthrough,
    )?;
    install_server_parent_death_signal(&mut cmd);
    let _tui = PathBuf::from(cmd.get_program());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create server-teardown runtime")?;
    runtime.block_on(async move {
        let mut cmd = tokio::process::Command::from(cmd);
        cmd.kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|err| anyhow!("failed to spawn TUI binary: {err}"))?;
        match supervise_server_child(&mut child, server_shutdown_signal()).await? {
            ServerTeardown::Exited(status) => {
                if let Some(code) = status.code() {
                    std::process::exit(code);
                } else {
                    bail!("mimofan terminated by signal");
                }
            }
            ServerTeardown::Signaled(code) => std::process::exit(code),
        }
    })
}

#[cfg(not(all(target_os = "linux", not(target_env = "ohos"))))]
fn install_server_parent_death_signal(_cmd: &mut Command) {}

#[derive(Debug)]
enum ServerTeardown {
    Exited(std::process::ExitStatus),
    Signaled(i32),
}

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
            let _ = child.start_kill();
            let _ = child.wait().await;
            Ok(ServerTeardown::Signaled(code))
        }
    }
}

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

fn build_tui_command_for_server(
    cli_config: Option<PathBuf>,
    cli_profile: Option<String>,
    cli_workspace: Option<PathBuf>,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<Command> {
    let tui = std::env::current_exe().context("failed to locate current executable path")?;
    let mut cmd = Command::new(&tui);
    if let Some(config) = cli_config.as_ref() {
        cmd.arg("--config").arg(config);
    }
    if let Some(profile) = cli_profile.as_ref() {
        cmd.arg("--profile").arg(profile);
    }
    if let Some(workspace) = cli_workspace.as_ref() {
        cmd.arg("--workspace").arg(workspace);
    }
    cmd.args(passthrough);

    if let Some(api_key) = resolved_runtime.api_key.as_ref() {
        cmd.env("MIMOFAN_API_KEY", api_key);
        for var in provider_env_vars(resolved_runtime.provider) {
            if *var != "MIMOFAN_API_KEY" {
                cmd.env(var, api_key);
            }
        }
        cmd.env(
            "MIMOFAN_API_KEY_SOURCE",
            RuntimeApiKeySource::Keyring.as_env_value(),
        );
    }

    Ok(cmd)
}

// ── Runtime resolution ──────────────────────────────────────────────────────

pub(crate) fn resolve_runtime_for_dispatch(
    store: &mut ConfigStore,
    runtime_overrides: &CliRuntimeOverrides,
) -> ResolvedRuntimeOptions {
    let runtime_secrets = Secrets::auto_detect();
    resolve_runtime_for_dispatch_with_secrets(store, runtime_overrides, &runtime_secrets)
}

pub(crate) fn resolve_runtime_for_dispatch_with_secrets(
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
