//! Interactive TUI startup and CLI auto-route helpers extracted from `lib.rs`.

use super::*;

use crate::config::{Config, MAX_SUBAGENTS};
use crate::logging;
use crate::model_routing;
use crate::session_manager;
use crate::tui::{self, InitialInput};

pub(crate) fn should_use_alt_screen(_cli: &Cli, _config: &Config) -> bool {
    true
}

pub(crate) fn should_use_mouse_capture(cli: &Cli, config: &Config, use_alt_screen: bool) -> bool {
    let terminal_emulator = std::env::var("TERMINAL_EMULATOR").ok();
    let wt_session = std::env::var("WT_SESSION").ok().filter(|s| !s.is_empty());
    let conemu_pid = std::env::var("ConEmuPID").ok().filter(|s| !s.is_empty());
    should_use_mouse_capture_with(
        cli,
        config,
        use_alt_screen,
        terminal_emulator.as_deref(),
        wt_session.as_deref(),
        conemu_pid.as_deref(),
    )
}

pub(crate) fn should_use_mouse_capture_with(
    cli: &Cli,
    config: &Config,
    use_alt_screen: bool,
    terminal_emulator: Option<&str>,
    wt_session: Option<&str>,
    conemu_pid: Option<&str>,
) -> bool {
    if !use_alt_screen || cli.no_mouse_capture {
        return false;
    }
    if cli.mouse_capture {
        return true;
    }
    config
        .tui
        .as_ref()
        .and_then(|tui| tui.mouse_capture)
        .unwrap_or_else(|| default_mouse_capture_enabled(terminal_emulator, wt_session, conemu_pid))
}

/// Whether to enable terminal mouse capture by default for this platform/host.
///
/// On Windows the default depends on the host: Windows Terminal (which sets
/// `WT_SESSION`) and ConEmu/Cmder (which set `ConEmuPID`) handle mouse-mode
/// reporting cleanly, so default-on there gives users in-app text selection
/// and keeps the application's selection clamped to the transcript area
/// (#1169). Legacy conhost (CMD without either env var) stays default-off
/// because its mouse-mode reporting can leak SGR escape sequences as raw
/// text into the composer (#878 / #898).
///
/// Off elsewhere only for JetBrains' JediTerm, which advertises mouse
/// support but forwards the same SGR escape sequences as raw input. The
/// user can still opt back in with `[tui] mouse_capture = true` in
/// `~/.mimofan/config.toml` or `--mouse-capture`.
pub(crate) fn default_mouse_capture_enabled(
    terminal_emulator: Option<&str>,
    wt_session: Option<&str>,
    conemu_pid: Option<&str>,
) -> bool {
    if cfg!(windows) {
        return wt_session.is_some() || conemu_pid.is_some();
    }
    if matches!(terminal_emulator, Some(t) if t.eq_ignore_ascii_case("JetBrains-JediTerm")) {
        return false;
    }
    true
}

pub(crate) async fn run_interactive(
    cli: &Cli,
    config: &Config,
    resume_session_id: Option<String>,
    initial_input: Option<InitialInput>,
) -> Result<()> {
    let workspace = cli
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Merge project-level config from $WORKSPACE/.mimofan/config.toml
    // unless --no-project-config was passed (#485).
    let mut merged_config = config.clone();
    crate::merge_user_workspace_config(&mut merged_config, cli.config.clone(), &workspace);
    if !cli.no_project_config {
        super::config_merge::merge_project_config(&mut merged_config, &workspace);
    }
    let config = &merged_config;

    if !cli.skip_onboarding {
        match crate::config::ensure_config_file_exists(cli.config.clone()) {
            Ok(Some(path)) => logging::info(format!(
                "Created first-run config file at {}",
                path.display()
            )),
            Ok(None) => {}
            Err(err) => logging::warn(format!("Failed to create first-run config file: {err}")),
        }
    }

    let model = config.default_model();
    let provider = config.api_provider();
    let max_subagents = cli.max_subagents.map_or_else(
        || config.max_subagents_for_provider(provider),
        |value| value.clamp(1, MAX_SUBAGENTS),
    );
    let use_alt_screen = should_use_alt_screen(cli, config);
    let use_mouse_capture = should_use_mouse_capture(cli, config, use_alt_screen);
    let use_bracketed_paste = crate::settings::Settings::load()
        .map(|s| s.effective_bracketed_paste())
        .unwrap_or_else(|_| !crate::settings::detected_legacy_windows_console_host());

    // Auto-install bundled system skills (e.g. skill-creator) on first launch.
    // Errors are non-fatal: log a warning and continue.
    let skills_dir = config.skills_dir();
    if let Err(e) = crate::skills::install_system_skills(&skills_dir) {
        logging::warn(format!("Failed to install system skills: {e}"));
    }

    // Prune stale workspace snapshots from prior sessions (7-day default).
    // Non-fatal: a flaky disk, missing `git`, or read-only home should
    // never block the TUI from starting.
    let snapshots = config.snapshots_config();
    if snapshots.enabled {
        session_manager::prune_workspace_snapshots(&workspace, snapshots.max_age());
    }

    // Prune stale tool-output spillover files (#422). Non-fatal: home
    // missing or directory unreadable just means nothing got pruned;
    // we never block startup. Runs unconditionally because the
    // spillover store is created lazily on first write — there's no
    // user-facing setting to gate.
    match crate::tools::truncate::prune_older_than(crate::tools::truncate::SPILLOVER_MAX_AGE) {
        Ok(0) => {}
        Ok(n) => tracing::debug!(
            target: "spillover",
            "boot prune removed {n} spillover file(s)"
        ),
        Err(err) => tracing::warn!(
            target: "spillover",
            ?err,
            "spillover prune skipped on boot"
        ),
    }

    // v0.8.44: prune managed sessions on boot to prevent unbounded growth.
    // Keeps at most MAX_SESSIONS (50) recent sessions; non-fatal on error.
    if let Ok(manager) = session_manager::SessionManager::default_location() {
        let _ = manager.cleanup_old_sessions();
    }

    // The `deepseek` launcher forwards `--yolo` to this binary via the
    // MIMOFAN_YOLO env var (config.yolo), not as a CLI flag. Honour either.
    let yolo = cli.yolo || config.yolo.unwrap_or(false);

    tui::run_tui(
        config,
        tui::TuiOptions {
            model,
            workspace,
            config_path: cli.config.clone(),
            config_profile: cli.profile.clone(),
            allow_shell: yolo || config.allow_shell(),
            use_alt_screen,
            use_mouse_capture,
            use_bracketed_paste,
            skills_dir,
            memory_dir: config.memory_dir(),
            notes_path: config.notes_path(),
            mcp_config_path: config.mcp_config_path(),
            use_memory: config.memory_enabled(),
            start_in_agent_mode: yolo,
            skip_onboarding: cli.skip_onboarding,
            yolo, // YOLO mode auto-approves all tool executions
            resume_session_id,
            initial_input,
            max_subagents,
        },
    )
    .await
}

#[derive(Debug)]
pub(crate) struct CliAutoRoute {
    pub(crate) provider: crate::config::ApiProvider,
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<crate::tui::app::ReasoningEffort>,
    pub(crate) auto_model: bool,
}

pub(crate) fn cli_reasoning_effort_value(
    config: &Config,
    effort: crate::tui::app::ReasoningEffort,
) -> Option<String> {
    effort
        .api_value_for_provider(config.api_provider())
        .map(str::to_string)
}

pub(crate) fn config_for_cli_route(config: &Config, route: &CliAutoRoute) -> Config {
    let mut execution_config = config.clone();
    execution_config.provider = Some(route.provider.as_str().to_string());
    execution_config
        .provider_config_for_mut(route.provider)
        .model = Some(route.model.clone());
    if matches!(route.provider, crate::config::ApiProvider::OpenAiCompatible) {
        execution_config.default_text_model = Some(route.model.clone());
    }
    execution_config
}

pub(crate) async fn resolve_cli_auto_route(
    config: &Config,
    model: &str,
    prompt: &str,
) -> Result<CliAutoRoute> {
    if model.trim().eq_ignore_ascii_case("auto") {
        let selection =
            model_routing::resolve_auto_route_with_inventory(config, prompt, "", "auto", "auto")
                .await?;
        Ok(CliAutoRoute {
            provider: selection.provider,
            model: selection.model,
            reasoning_effort: selection.reasoning_effort,
            auto_model: true,
        })
    } else {
        if let Some(selection) = model_routing::resolve_explicit_route_with_inventory(config, model)
        {
            return Ok(CliAutoRoute {
                provider: selection.provider,
                model: selection.model,
                reasoning_effort: selection.reasoning_effort,
                auto_model: false,
            });
        }

        let candidate_providers = model_routing::explicit_route_candidate_providers(config, model);
        if !candidate_providers.is_empty() && !candidate_providers.contains(&config.api_provider())
        {
            let providers = candidate_providers
                .iter()
                .map(|provider| provider.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "model `{model}` is available from configured provider route(s): {providers}. \
                 Pass `--provider <provider>` with `--model {model}` to choose one explicitly."
            );
        }

        // When --model is not `auto`, fall back to the reasoning_effort
        // declared in the user's config.toml. The previous hard-coded `None`
        // silently dropped the user's setting on every non-auto-route exec
        // call, which (for example) prevented custom-endpoint users from
        // disabling thinking via `reasoning_effort = "off"` and caused
        // 30+ second SSE idle timeouts on trivial prompts.
        Ok(CliAutoRoute {
            provider: config.api_provider(),
            model: model.to_string(),
            reasoning_effort: config
                .reasoning_effort()
                .map(crate::tui::app::ReasoningEffort::from_setting),
            auto_model: false,
        })
    }
}
