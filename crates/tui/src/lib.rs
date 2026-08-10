//! mimofan library crate.
//!
//! This library contains the core TUI functionality.

#![allow(clippy::uninlined_format_args)]
// Allow dead_code crate-wide: `tui` is a scaffolding-heavy crate. The
// suppressed items are intentional foundation APIs (provider/model catalog,
// fleet orchestration, sandbox/network policy, telemetry, approval cache,
// subagent decomposition/aggregation, composable prompt layers, worker
// profile) tracked for follow-up features — not legacy cruft. They are
// deliberately kept as the public surface for not-yet-wired consumers.
// If a specific module's scaffolding is retired, delete it rather than
// re-allowing it here.
#![allow(dead_code)]

use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};
use dotenvy::dotenv;

// ── Module declarations ────────────────────────────────────────────────
mod acp_server;
mod artifacts;
mod audit;
mod auto_reasoning;
mod automation_manager;
mod child_env;
pub mod cli_commands;
pub mod client;
mod command_safety;
pub mod commands;
pub mod compaction;
mod composer_history;
mod composer_stash;
pub mod config;
mod config_persistence;
mod config_ui;
mod context_budget;
mod context_report;
mod core;
mod cost_budget;
mod cost_status;
pub mod decision_gate;
mod dependencies;
mod error_taxonomy;
mod errors;
mod eval;
pub mod evidence;
pub mod execpolicy;
pub(crate) mod features;
pub mod fleet;
pub mod issue_monitor;
mod mimofan_theme;
/// Re-export observability types for integration tests and external consumers.
pub use fleet::observability::{
    AgentMetrics, AgentTopology, FleetStatusSummary, ObservabilityCollector,
};
mod cli;
mod goal_loop;
mod hooks;
mod llm_client;
mod llm_response_cache;
pub use mimofan_localization as localization;
mod logging;
pub mod loop_guard;
mod lsp;
pub(crate) mod mcp;
mod mcp_server;
mod mcp_server_backend;
mod memory;
mod turn_memory;
mod model_inventory;
mod model_profile;
mod model_registry;
mod model_routing;
pub mod models;
mod network_policy;
pub mod palette;
mod prefix_cache;
mod pricing;
mod project_context;
mod project_context_cache;
mod project_doc;
mod prompt_zones;
mod prompts;
mod purge;
mod remote_setup;
pub mod repl;
mod request_tuning;
mod resource_telemetry;
mod retry_status;
pub mod rlm;
mod route_budget;
mod route_runtime;
mod runtime_api;
mod runtime_log;
mod runtime_threads;
pub mod sandbox;
mod scheduler;
mod seam_manager;
mod session_manager;
mod settings;
mod shell_dispatcher;
mod signals;
mod skill_state;
mod skills;
mod slop_ledger;
mod snapshot;
mod state_machine;
mod status;
mod task_manager;
mod tls;
mod tokenizer;
mod tool_output_receipts;
pub mod tools;
mod tui;
mod utils;
#[cfg(feature = "vector-memory")]
pub mod vector_memory;
mod vision;
mod worker_profile;
mod working_set;
mod workspace_discovery;
mod workspace_trust;

// ── Imports from cli sub-modules ───────────────────────────────────────
// `use crate::cli::*` brings in Cli, Commands, all arg structs, and the
// helper functions defined directly in `cli/mod.rs`.
use crate::cli::doctor::{run_doctor, run_doctor_context_json, run_doctor_json};
use crate::cli::exec_agent::{
    run_apply, run_exec_agent, run_one_shot, run_one_shot_json, run_sandbox_command,
};
use crate::cli::fleet_cmd::run_fleet_command;
use crate::cli::interactive::run_interactive;
use crate::cli::mcp_cmd::run_mcp_command;
use crate::cli::model_cmd::{run_models, run_speech};
use crate::cli::pr::run_pr;
use crate::cli::review::run_review;
use crate::cli::session::{
    fork_session, latest_session_id_for_workspace, list_sessions, load_config_from_cli,
    preserve_interrupted_checkpoint_for_explicit_resume, recover_interrupted_checkpoint_for_resume,
    resolve_session_id, resolve_workspace,
};
use crate::cli::setup::{init_project, resolve_cors_origins, run_setup};
use crate::cli::*;
use crate::config::{Config, MAX_SUBAGENTS};
use crate::eval::{EvalHarness, EvalHarnessConfig, ScenarioStepKind};
use crate::features::{Feature, render_feature_table};

// ── Helper functions ───────────────────────────────────────────────────

#[cfg(not(windows))]
fn configure_windows_console_utf8() {}

fn install_rustls_crypto_provider() {
    crate::tls::ensure_rustls_crypto_provider();
}

/// Generate shell completions for the given shell
fn generate_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut io::stdout());
}

/// Run the offline evaluation harness (no network/LLLM calls).
fn run_eval(args: EvalArgs) -> Result<()> {
    let fail_step = match args.fail_step.as_deref() {
        Some(value) => ScenarioStepKind::parse(value)
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("invalid --fail-step '{value}'"))?,
        None => None,
    };

    let config = EvalHarnessConfig {
        fail_step,
        shell_command: args.shell_command,
        shell_expect_token: args.shell_expect_token,
        max_output_chars: args.max_output_chars,
        record_dir: args.record.clone(),
        ..EvalHarnessConfig::default()
    };

    let harness = EvalHarness::new(config);
    let run = harness.run().context("evaluation harness failed")?;
    let report = run.to_report();

    if args.json {
        let json = serde_json::to_string_pretty(&report)?;
        println!("{json}");
    } else {
        println!("Offline Eval Harness");
        println!("scenario: {}", report.scenario_name);
        println!("workspace: {}", report.workspace_root.display());
        println!("success: {}", report.metrics.success);
        println!("steps: {}", report.metrics.steps);
        println!("tool_errors: {}", report.metrics.tool_errors);
        println!("duration_ms: {}", report.metrics.duration.as_millis());

        if !report.metrics.per_tool.is_empty() {
            println!("per_tool:");
            for (kind, stats) in &report.metrics.per_tool {
                println!(
                    "  {} invocations={} errors={} duration_ms={}",
                    kind.tool_name(),
                    stats.invocations,
                    stats.errors,
                    stats.total_duration.as_millis()
                );
            }
        }

        let failed_steps: Vec<_> = report.steps.iter().filter(|s| !s.success).collect();
        if !failed_steps.is_empty() {
            println!("failed_steps:");
            for step in failed_steps {
                let error = step.error.as_deref().unwrap_or("unknown error");
                println!(
                    "  {} tool={} error={}",
                    step.kind.tool_name(),
                    step.tool_name,
                    error
                );
            }
        }
    }

    if report.metrics.success {
        Ok(())
    } else {
        bail!("offline evaluation harness reported failure")
    }
}

fn run_execpolicy_command(command: ExecpolicyCommand) -> Result<()> {
    match command.command {
        ExecpolicySubcommand::Check(cmd) => cmd.run(),
    }
}

fn run_features_command(config: &Config, command: FeaturesCli) -> Result<()> {
    match command.command {
        FeaturesSubcommand::List => {
            print!("{}", render_feature_table(&config.features()));
            Ok(())
        }
    }
}

// ── Main entry point ───────────────────────────────────────────────────

pub async fn run() -> Result<()> {
    configure_windows_console_utf8();
    install_rustls_crypto_provider();

    // ── Process hardening (#2183) ─────────────────────────────────────────
    // MUST run before Tokio is booted and before any threads are spawned.
    // See crates/tui/src/sandbox/process_hardening.rs for ordering rationale.
    crate::sandbox::process_hardening::apply_process_hardening();

    // Set up process panic hook before anything else — writes crash dumps
    // to ~/.mimofan/crashes/ even if the panic happens before tokio is up,
    // and restores the terminal so a panicked TUI doesn't leave the user's
    // shell stuck in alt-screen mode.
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Restore the terminal first so the panic message itself, plus the
        // user's shell after exit, are visible. Best-effort — we may not be
        // in raw / alt-screen mode if the panic happens pre-TUI. Shared
        // with the signal handler installed below so both exit paths leave
        // the terminal in the same well-defined state.
        crate::tui::ui::emergency_restore_terminal();

        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", panic_info.payload())
        };
        let location = panic_info
            .location()
            .map(|loc| loc.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        tracing::error!(target: "panic", "Process panicked at {location}: {msg}");
        // Write crash dump best-effort
        if let Some(home) = dirs::home_dir() {
            let crash_dir = home.join(".mimofan").join("crashes");
            let _ = std::fs::create_dir_all(&crash_dir);
            use chrono::Utc;
            let ts = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
            let path = crash_dir.join(format!("{ts}-process-panic.log"));
            let contents =
                format!("Process panicked\nLocation: {location}\nTimestamp: {ts}\nPanic: {msg}\n",);
            let _ = std::fs::write(&path, contents);
        }
        // Invoke the original hook (prints to stderr, etc.)
        orig_hook(panic_info);
    }));

    // Install signal handlers that restore the terminal before the
    // process exits. Without this, Ctrl+C delivered while raw mode /
    // kitty keyboard enhancement / alt-screen are active (or in the
    // brief windows around startup and teardown where they're being
    // toggled) leaves the user's shell receiving raw CSI sequences
    // like `^[[>5u` until they run `reset` (#1583).
    //
    // Once the TUI's raw mode is engaged the terminal driver delivers
    // Ctrl+C as the byte 0x03 rather than SIGINT, so the in-TUI key
    // handler — not this handler — is what processes user interrupts
    // during normal operation. This handler exists for the gaps:
    // pre-TUI subcommands (--version, doctor, login, …), the moments
    // around enable_raw_mode / disable_raw_mode, the external-editor
    // suspend path, and SIGTERM / SIGHUP from the OS.
    signals::spawn_signal_cleanup_task();

    dotenv().ok();

    // Intercept `mimofan .` or `mimofan <dir>` and convert it to `mimofan -C <dir>`
    // This allows users to use `mimofan .` similar to `code .` or `cursor .`
    let mut env_args: Vec<String> = std::env::args().collect();
    if env_args.len() == 2 && !env_args[1].starts_with('-') {
        let known_subcommands = [
            "run",
            "doctor",
            "models",
            "speech",
            "tts",
            "sessions",
            "resume",
            "fork",
            "init",
            "setup",
            "remote-setup",
            "exec",
            "fleet",
            "review",
            "apply",
            "eval",
            "mcp",
            "features",
            "serve",
            "completions",
            "login",
            "logout",
            "auth",
            "mcp-server",
            "config",
            "model",
            "thread",
            "sandbox",
            "app-server",
            "completion",
            "metrics",
            "update",
            "help",
        ];
        if !known_subcommands.contains(&env_args[1].as_str())
            && std::path::Path::new(&env_args[1]).is_dir()
        {
            env_args.insert(1, "-C".to_string());
        }
    }
    let cli = Cli::parse_from(env_args);
    logging::set_verbose(cli.verbose || logging::env_requests_verbose_logging());

    // Handle subcommands first
    if let Some(command) = cli.command.clone() {
        return match command {
            Commands::Doctor(args) => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                if args.context_json {
                    run_doctor_context_json(&config, &workspace)
                } else if args.json {
                    run_doctor_json(&config, &workspace, cli.config.as_deref())
                } else {
                    run_doctor(&config, &workspace, cli.config.as_deref()).await;
                    Ok(())
                }
            }
            Commands::Setup(args) => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                run_setup(&config, &workspace, args)
            }
            Commands::RemoteSetup(args) => remote_setup::run_remote_setup(args),
            Commands::Completions { shell } => {
                generate_completions(shell);
                Ok(())
            }
            Commands::Sessions { limit, search } => list_sessions(limit, search),
            Commands::Init => init_project(),
            Commands::Login { provider, api_key } => {
                let mut store = mimofan_config::ConfigStore::load(cli.config.clone())?;
                cli_commands::run_login_command(
                    &mut store,
                    cli_commands::LoginArgs { provider, api_key },
                )
            }
            Commands::Logout => {
                let mut store = mimofan_config::ConfigStore::load(cli.config.clone())?;
                cli_commands::run_logout_command(&mut store)
            }
            Commands::Auth(args) => {
                let mut store = mimofan_config::ConfigStore::load(cli.config.clone())?;
                cli_commands::run_auth_command(&mut store, args.command)
            }
            Commands::McpServer => {
                let mut store = mimofan_config::ConfigStore::load(cli.config.clone())?;
                cli_commands::run_mcp_server_command(&mut store)
            }
            Commands::Config(args) => {
                let mut store = mimofan_config::ConfigStore::load(cli.config.clone())?;
                cli_commands::run_config_command(&mut store, args.command)
            }
            Commands::Model(args) => {
                let mut store = mimofan_config::ConfigStore::load(cli.config.clone())?;
                let runtime_overrides = mimofan_config::CliRuntimeOverrides {
                    provider: None,
                    model: None,
                    api_key: None,
                    base_url: None,
                    auth_mode: None,
                    output_mode: None,
                    log_level: None,
                    telemetry: None,
                    approval_policy: None,
                    sandbox_mode: None,
                    yolo: None,
                    verbosity: None,
                };
                cli_commands::run_model_command(
                    &mut store,
                    args.command,
                    runtime_overrides.provider,
                )
            }
            Commands::Thread(args) => cli_commands::run_thread_command(args.command),
            Commands::AppServer(args) => {
                let mut store = mimofan_config::ConfigStore::load(cli.config.clone())?;
                let runtime_overrides = mimofan_config::CliRuntimeOverrides {
                    provider: None,
                    model: None,
                    api_key: None,
                    base_url: None,
                    auth_mode: None,
                    output_mode: None,
                    log_level: None,
                    telemetry: None,
                    approval_policy: None,
                    sandbox_mode: None,
                    yolo: None,
                    verbosity: None,
                };
                let resolved_runtime =
                    cli_commands::resolve_runtime_for_dispatch(&mut store, &runtime_overrides);
                cli_commands::run_app_server_command(
                    cli.config.clone(),
                    cli.profile.clone(),
                    cli.workspace.clone(),
                    &resolved_runtime,
                    args,
                )
            }
            Commands::Completion { shell } => {
                cli_commands::generate_completions_from_cli(shell);
                Ok(())
            }
            Commands::Metrics(args) => cli_commands::run_metrics_command(args),
            Commands::Update(args) => {
                cli_commands::update::run_update(
                    args.beta,
                    args.check,
                    args.proxy,
                    args.allow_unverified,
                )
            }
            Commands::InstallDeps(args) => cli_commands::install_deps::run_install_deps(args.yes),
            Commands::Models(args) => {
                let config = load_config_from_cli(&cli)?;
                run_models(&config, args).await
            }
            Commands::Speech(args) => {
                let config = load_config_from_cli(&cli)?;
                run_speech(&config, args).await
            }
            Commands::Exec(args) => {
                let config = load_config_from_cli(&cli)?;
                let workspace = cli.workspace.clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                });
                let mut config = config.clone();
                merge_user_workspace_config(&mut config, cli.config.clone(), &workspace);
                let model = resolve_exec_model(&config, args.model.as_deref());
                let prompt = join_prompt_parts(&args.prompt);
                let resume_session_id = resolve_exec_resume_session_id(&args, &workspace)?;
                // The `deepseek` launcher forwards `--yolo` to this binary via
                // the MIMOFAN_YOLO env var (which the config loader folds into
                // `config.yolo`), not as a CLI flag. Honour either source.
                let yolo = cli.yolo || config.yolo.unwrap_or(false);
                let needs_engine = args.auto
                    || yolo
                    || resume_session_id.is_some()
                    || args.output_format == ExecOutputFormat::StreamJson
                    || args.max_turns.is_some()
                    || args.allowed_tools.is_some()
                    || args.disallowed_tools.is_some()
                    || args.append_system_prompt.is_some();
                if needs_engine {
                    let provider = config.api_provider();
                    let max_subagents = cli.max_subagents.map_or_else(
                        || config.max_subagents_for_provider(provider),
                        |value| value.clamp(1, MAX_SUBAGENTS),
                    );
                    let auto_mode = args.auto || yolo;
                    let max_turns = args.max_turns.unwrap_or(100);
                    let allowed_tools = args.allowed_tools.as_ref().map(|v| {
                        v.iter()
                            .map(|s| s.to_ascii_lowercase().trim().to_string())
                            .collect::<Vec<_>>()
                    });
                    let disallowed_tools = args.disallowed_tools.as_ref().map(|v| {
                        v.iter()
                            .map(|s| s.to_ascii_lowercase().trim().to_string())
                            .collect::<Vec<_>>()
                    });
                    run_exec_agent(
                        &config,
                        &model,
                        &prompt,
                        workspace,
                        max_subagents,
                        auto_mode,
                        auto_mode,
                        args.json,
                        resume_session_id,
                        args.output_format,
                        max_turns,
                        allowed_tools,
                        disallowed_tools,
                        args.append_system_prompt.clone(),
                    )
                    .await
                } else if args.json {
                    run_one_shot_json(&config, &model, &prompt).await
                } else {
                    run_one_shot(&config, &model, &prompt).await
                }
            }
            Commands::Fleet(args) => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                run_fleet_command(&workspace, &config, args).await
            }
            Commands::Review(args) => {
                let config = load_config_from_cli(&cli)?;
                run_review(&config, args).await
            }
            Commands::Pr {
                number,
                repo,
                checkout,
            } => {
                let config = load_config_from_cli(&cli)?;
                run_pr(&cli, &config, number, repo.as_deref(), checkout).await
            }
            Commands::Apply(args) => run_apply(args),
            Commands::Eval(args) => run_eval(args),
            Commands::Mcp { command } => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                run_mcp_command(&config, &workspace, command).await
            }
            Commands::Execpolicy(command) => {
                let config = load_config_from_cli(&cli)?;
                if !config.features().enabled(Feature::ExecPolicy) {
                    bail!(
                        "The `exec_policy` feature is disabled. Enable it in [features] or via profile."
                    );
                }
                run_execpolicy_command(command)
            }
            Commands::Features(command) => {
                let config = load_config_from_cli(&cli)?;
                run_features_command(&config, command)
            }
            Commands::Sandbox(args) => run_sandbox_command(args),
            Commands::Serve(args) => {
                let workspace = cli.workspace.clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                });
                let http_selected =
                    validate_serve_mode_selection(args.mcp, args.http, args.mobile, args.acp)?;
                if args.mcp {
                    tokio::task::block_in_place(|| mcp_server::run_mcp_server(workspace))
                } else if http_selected {
                    let config = load_config_from_cli(&cli)?;
                    let cors_origins = resolve_cors_origins(&config, &args.cors_origin);
                    let bind_host = resolve_serve_bind_host(args.mobile, args.host);
                    if bind_host.mobile_rebound_to_lan {
                        println!(
                            "WARNING: --mobile is binding to 0.0.0.0 so LAN devices can reach the mobile control page. Use --host 127.0.0.1 to keep mobile loopback-only."
                        );
                    }
                    runtime_api::run_http_server(
                        config,
                        workspace,
                        runtime_api::RuntimeApiOptions {
                            host: bind_host.host,
                            port: args.port,
                            workers: args.workers.clamp(1, 8),
                            cors_origins,
                            auth_token: args.auth_token,
                            insecure_no_auth: args.insecure_no_auth,
                            mobile: args.mobile,
                            show_qr: args.qr,
                        },
                    )
                    .await
                } else if args.acp {
                    let config = load_config_from_cli(&cli)?;
                    let model = config.default_model();
                    acp_server::run_acp_server(config, model, workspace).await
                } else {
                    unreachable!("server mode count checked above")
                }
            }
            Commands::Resume { session_id, last } => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                let resume_id = resolve_session_id(session_id, last, &workspace)?;
                run_interactive(&cli, &config, Some(resume_id), None).await
            }
            Commands::Fork { session_id, last } => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                let new_session_id = fork_session(session_id, last, &workspace)?;
                run_interactive(&cli, &config, Some(new_session_id), None).await
            }
        };
    }

    // Top-level prompt mode: submit the initial prompt, then keep the TUI alive
    // for follow-up messages. Use `mimofan exec` for explicit non-interactive
    // one-shot behavior (#2370).
    let config = load_config_from_cli(&cli)?;
    if let Some(initial_input) = top_level_prompt_initial_input(&cli.prompt) {
        return run_interactive(&cli, &config, None, Some(initial_input)).await;
    }

    // Handle session resume. Plain `mimofan` starts fresh: interrupted
    // snapshots are preserved for explicit resume, but never auto-attached.
    let resume_session_id = if cli.continue_session {
        let workspace = resolve_workspace(&cli);
        recover_interrupted_checkpoint_for_resume(&workspace)
            .or_else(|| latest_session_id_for_workspace(&workspace).ok().flatten())
    } else if let Some(id) = cli.resume.clone() {
        Some(id)
    } else if !cli.fresh {
        let workspace = resolve_workspace(&cli);
        preserve_interrupted_checkpoint_for_explicit_resume(&workspace);
        None
    } else {
        None
    };

    // Default: Interactive TUI
    // --yolo starts in YOLO mode (auto-approve; shell if allow_shell=true)
    run_interactive(&cli, &config, resume_session_id, None).await
}
