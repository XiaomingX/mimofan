//! Doctor / diagnostics functions extracted from `lib.rs`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::model_cmd::{rustc_version, test_api_connectivity};
use super::setup::{default_plugins_dir, default_tools_dir};
use super::*;
use crate::features::Feature;
use crate::mcp::McpServerConfig;

// ---------------------------------------------------------------------------
// ApiKeySource + resolution helpers
// ---------------------------------------------------------------------------

/// Source of the resolved DeepSeek API key, used in status reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiKeySource {
    Command,
    Env,
    Config,
    Keyring,
    Secret,
    Missing,
}

pub(crate) fn resolve_api_key_source(config: &Config) -> ApiKeySource {
    let provider = config.api_provider();
    if std::env::var("MIMOFAN_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .is_some()
    {
        match std::env::var("MIMOFAN_API_KEY_SOURCE").ok().as_deref() {
            Some("config") => return ApiKeySource::Config,
            Some("keyring") => return ApiKeySource::Keyring,
            _ => {}
        }
    }

    let provider_config_key = config
        .provider_config()
        .and_then(|entry| entry.api_key.as_ref())
        .is_some_and(|k| !k.trim().is_empty());
    let root_deepseek_key = matches!(provider, crate::config::ApiProvider::XiaomiMimo)
        && config
            .api_key
            .as_ref()
            .is_some_and(|k| !k.trim().is_empty());

    if provider_config_key || root_deepseek_key {
        ApiKeySource::Config
    } else if let Some(auth) = config
        .provider_config()
        .and_then(|entry| entry.auth.as_ref())
    {
        match auth.source {
            mimofan_config::AuthSourceKind::Command => ApiKeySource::Command,
            mimofan_config::AuthSourceKind::Secret => ApiKeySource::Secret,
        }
    } else if provider_env_key_source(provider).is_some() {
        ApiKeySource::Env
    } else {
        ApiKeySource::Missing
    }
}

pub(crate) fn provider_env_key_source(
    provider: crate::config::ApiProvider,
) -> Option<&'static str> {
    provider
        .env_vars()
        .iter()
        .copied()
        .find(|var| std::env::var(var).is_ok_and(|value| !value.trim().is_empty()))
}

pub(crate) fn provider_env_vars_label(provider: crate::config::ApiProvider) -> String {
    provider.env_vars_label()
}

pub(crate) fn provider_config_table_key(provider: crate::config::ApiProvider) -> &'static str {
    provider
        .metadata()
        .map(|metadata| metadata.provider_config_key())
        .unwrap_or("deepseek_cn")
}

pub(crate) fn provider_auth_hint(provider: crate::config::ApiProvider) -> String {
    if provider == crate::config::ApiProvider::XiaomiMimo {
        "see docs/PROVIDERS.md for ChatGPT/Codex OAuth setup".to_string()
    } else {
        format!(
            "mimofan auth set --provider {} --api-key \"...\"",
            provider.as_str()
        )
    }
}

// ---------------------------------------------------------------------------
// Directory / skill counting helpers
// ---------------------------------------------------------------------------

pub(crate) fn count_dir_entries(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(std::result::Result::ok).count())
        .unwrap_or(0)
}

pub(crate) fn skills_count_for(dir: &Path) -> usize {
    if !dir.exists() {
        return 0;
    }
    crate::skills::SkillRegistry::discover(dir).len()
}

// ---------------------------------------------------------------------------
// run_doctor — human-readable diagnostics
// ---------------------------------------------------------------------------

/// Run system diagnostics
pub(crate) async fn run_doctor(
    config: &Config,
    workspace: &Path,
    config_path_override: Option<&Path>,
) {
    use crate::palette;
    use colored::Colorize;

    let (accent_r, accent_g, accent_b) = palette::MIMOFAN_ACCENT_PRIMARY_RGB;
    let (sky_r, sky_g, sky_b) = palette::MIMOFAN_SKY_RGB;
    let (aqua_r, aqua_g, aqua_b) = palette::MIMOFAN_SKY_RGB;
    let (red_r, red_g, red_b) = palette::MIMOFAN_RED_RGB;

    println!(
        "{}",
        "mimofan Doctor"
            .truecolor(accent_r, accent_g, accent_b)
            .bold()
    );
    println!("{}", "==================".truecolor(sky_r, sky_g, sky_b));
    println!();

    // Version info
    println!("{}", "Version Information:".bold());
    println!("  mimofan: {}", env!("MIMOFAN_BUILD_VERSION"));
    println!("  rust: {}", rustc_version());
    println!();

    println!("{}", "Updates:".bold());
    let current_version = env!("CARGO_PKG_VERSION");
    println!("  · current: v{current_version}");
    match mimofan_release::latest_release_tag_async(mimofan_release::ReleaseChannel::Stable).await {
        Ok(latest_tag) => {
            match mimofan_release::compare_release_versions(current_version, &latest_tag) {
                Ok(std::cmp::Ordering::Less) => {
                    println!(
                        "  {} latest: {latest_tag}",
                        "!".truecolor(sky_r, sky_g, sky_b)
                    );
                    println!("    Update available. Run `mimo update` to install.");
                }
                Ok(std::cmp::Ordering::Equal) => {
                    println!(
                        "  {} latest: {latest_tag}",
                        "✓".truecolor(aqua_r, aqua_g, aqua_b)
                    );
                    println!("    Already up to date.");
                }
                Ok(std::cmp::Ordering::Greater) => {
                    println!("  {} latest: {latest_tag}", "·".dimmed());
                    println!("    Current build is newer than the latest published release.");
                }
                Err(err) => {
                    println!(
                        "  {} latest: {latest_tag}",
                        "!".truecolor(sky_r, sky_g, sky_b)
                    );
                    println!("    Version comparison failed: {err}");
                }
            }
        }
        Err(err) => {
            println!(
                "  {} latest release check failed: {err}",
                "!".truecolor(sky_r, sky_g, sky_b)
            );
            println!("    Run `mimo update --check` to retry.");
        }
    }
    println!();

    // Configuration summary
    println!("{}", "Configuration:".bold());
    let config_path = config_path_override
        .map(PathBuf::from)
        .or_else(|| mimofan_config::resolve_config_path(None).ok())
        .unwrap_or_else(|| {
            mimofan_config::mimofan_home()
                .unwrap_or_else(|_| PathBuf::from(".mimofan"))
                .join("config.toml")
        });

    if config_path.exists() {
        println!(
            "  {} config.toml found at {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&config_path)
        );
    } else {
        println!(
            "  {} config.toml not found at {} (using defaults/env)",
            "!".truecolor(sky_r, sky_g, sky_b),
            crate::utils::display_path(&config_path)
        );
    }
    println!("  workspace: {}", crate::utils::display_path(workspace));
    println!("  {}", doctor_search_provider_line(config));

    // State root
    println!();
    println!("{}", "State Root:".bold());
    let code_home = mimofan_config::mimofan_home().unwrap_or_else(|_| PathBuf::from("~/.mimofan"));
    println!("  active: {}", crate::utils::display_path(&code_home));

    // Check API keys
    println!();
    println!("{}", "API Keys:".bold());

    // Per-provider state: env + config file only (no values printed).
    // Keep doctor/status prompt-free even for unsigned rebuilt binaries.
    let dispatcher_api_key_source = std::env::var("MIMOFAN_API_KEY_SOURCE").ok();
    for provider in crate::config::ApiProvider::all().iter().copied() {
        let slot = provider.as_str();
        let in_env = provider.env_vars().iter().any(|var| {
            std::env::var(var)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .is_some()
        });
        let injected_runtime_key = matches!(
            dispatcher_api_key_source.as_deref(),
            Some("keyring" | "env" | "cli")
        );
        let in_config = config
            .provider_config_for(provider)
            .and_then(|entry| entry.api_key.as_ref())
            .is_some_and(|v| !v.trim().is_empty())
            || (matches!(provider, crate::config::ApiProvider::XiaomiMimo)
                && !injected_runtime_key
                && config
                    .api_key
                    .as_ref()
                    .is_some_and(|v| !v.trim().is_empty()));
        let icon = if in_env || in_config {
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        } else {
            "·".dimmed()
        };
        println!(
            "  {} {slot}: env={}, config={}",
            icon,
            if in_env { "yes" } else { "no" },
            if in_config { "yes" } else { "no" }
        );
    }
    println!("  · credential precedence: ~/.mimofan/config.toml, OS keyring, then env");

    let api_key_source = resolve_api_key_source(config);
    let has_api_key = if config.api_key().is_ok() {
        let source_label = match api_key_source {
            ApiKeySource::Command => "configured auth command",
            ApiKeySource::Config => "config.toml",
            ApiKeySource::Keyring => "OS keyring",
            ApiKeySource::Secret => "configured secret source",
            ApiKeySource::Env => "environment",
            ApiKeySource::Missing => "unknown source",
        };
        println!(
            "  {} active provider key resolved from {source_label}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        );
        true
    } else {
        println!(
            "  {} active provider key not configured",
            "✗".truecolor(red_r, red_g, red_b)
        );
        println!(
            "    Run 'mimofan auth set --provider <name>' to save a key to ~/.mimofan/config.toml."
        );
        false
    };

    // API connectivity test
    println!();
    println!("{}", "API Connectivity:".bold());
    let api_target = doctor_api_target(config);
    println!("  · provider: {}", api_target.provider);
    println!(
        "  · base_url: {}",
        crate::client::redact_url_for_display(&api_target.base_url)
    );
    println!("  · model: {}", api_target.model);
    let tls_status = doctor_tls_status(config);
    if !tls_status.certificate_verification {
        println!("  ! {}", tls_status.message);
        println!("    Prefer SSL_CERT_FILE with a trusted custom CA bundle when possible.");
    }
    let strict_tool_mode = doctor_strict_tool_mode_status(config);
    let strict_icon = match strict_tool_mode.status {
        "ready" => "✓".truecolor(aqua_r, aqua_g, aqua_b),
        "fallback_non_beta" | "custom_endpoint" => "!".truecolor(sky_r, sky_g, sky_b),
        _ => "·".dimmed(),
    };
    println!(
        "  {} strict_tool_mode: {}",
        strict_icon, strict_tool_mode.message
    );
    if let Some(recommended) = strict_tool_mode.recommended_base_url.as_ref() {
        println!("    Use `base_url = \"{recommended}\"` for DeepSeek strict schemas.");
    }
    let capability = crate::config::provider_capability(config.api_provider(), &api_target.model);
    if let Some(alias) = capability.alias_deprecation.as_ref() {
        println!(
            "  ! model alias {} retires {}; switch to {}",
            alias.alias, alias.retirement_date, alias.replacement
        );
    }
    if has_api_key {
        print!("  {} Testing connection...", "·".dimmed());
        use std::io::Write;
        std::io::stdout().flush().ok();

        match test_api_connectivity(config).await {
            Ok(()) => {
                println!(
                    "\r  {} API connection successful",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b)
                );
            }
            Err(e) => {
                let error_msg = e.to_string();
                println!(
                    "\r  {} API connection failed",
                    "✗".truecolor(red_r, red_g, red_b)
                );
                if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                    println!(
                        "    Invalid API key. Check `mimofan auth status`, MIMOFAN_API_KEY, or config.toml"
                    );
                    if matches!(api_key_source, ApiKeySource::Keyring) {
                        println!(
                            "    The rejected key came from the OS keyring via the dispatcher."
                        );
                        println!(
                            "    Run `mimofan auth status` to inspect config/keyring/env sources."
                        );
                    } else if matches!(api_key_source, ApiKeySource::Env) {
                        println!(
                            "    The rejected key came from MIMOFAN_API_KEY; no saved config key is present."
                        );
                        println!(
                            "    Run `mimofan auth set --provider deepseek` to save a config key that overrides stale env."
                        );
                    }
                } else if error_msg.contains("403") || error_msg.contains("Forbidden") {
                    println!(
                        "    API key lacks permissions. Verify key is active at platform.deepseek.com"
                    );
                } else if error_msg.contains("timeout") || error_msg.contains("Timeout") {
                    for line in doctor_timeout_recovery_lines(config) {
                        println!("    {line}");
                    }
                } else if error_msg.contains("dns") || error_msg.contains("resolve") {
                    println!("    DNS resolution failed. Check your network connection");
                } else if error_msg.contains("connect") {
                    println!("    Connection failed. Check firewall settings or try again");
                } else {
                    println!("    Error: {error_msg}");
                }
            }
        }
    } else {
        println!("  {} Skipped (no API key configured)", "·".dimmed());
    }

    // MCP configuration
    println!();
    println!("{}", "MCP Servers:".bold());
    let features = config.features();
    if features.enabled(Feature::Mcp) {
        println!(
            "  {} MCP feature flag enabled",
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        );
    } else {
        println!(
            "  {} MCP feature flag disabled",
            "!".truecolor(sky_r, sky_g, sky_b)
        );
    }

    let mcp_config_path = config.mcp_config_path();
    let project_mcp_config_path = crate::mcp::workspace_mcp_config_path(workspace);
    if mcp_config_path.exists() {
        println!(
            "  {} MCP config found at {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&mcp_config_path)
        );
    } else {
        println!(
            "  {} MCP config not found at {}",
            "·".dimmed(),
            crate::utils::display_path(&mcp_config_path)
        );
    }
    if project_mcp_config_path.exists() {
        println!(
            "  {} Project MCP config found at {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&project_mcp_config_path)
        );
    } else {
        println!(
            "  {} Project MCP config not found at {}",
            "·".dimmed(),
            crate::utils::display_path(&project_mcp_config_path)
        );
    }

    match crate::mcp::load_config_with_workspace(&mcp_config_path, workspace) {
        Ok(cfg) if cfg.servers.is_empty() => {
            println!("  {} 0 merged server(s) configured", "·".dimmed());
            if !mcp_config_path.exists() && !project_mcp_config_path.exists() {
                println!("    Run `mimo mcp init` or add `.mimofan/mcp.json`.");
            }
        }
        Ok(cfg) => {
            println!(
                "  {} {} merged server(s) configured",
                "·".dimmed(),
                cfg.servers.len()
            );
            for (name, server) in &cfg.servers {
                let status = doctor_check_mcp_server(server);
                let icon = match status {
                    McpServerDoctorStatus::Ok(ref detail) => {
                        format!(
                            "  {} {name}: {}",
                            "✓".truecolor(aqua_r, aqua_g, aqua_b),
                            detail
                        )
                    }
                    McpServerDoctorStatus::Warning(ref detail) => {
                        format!(
                            "  {} {name}: {}",
                            "!".truecolor(sky_r, sky_g, sky_b),
                            detail
                        )
                    }
                    McpServerDoctorStatus::Error(ref detail) => {
                        format!(
                            "  {} {name}: {}",
                            "✗".truecolor(red_r, red_g, red_b),
                            detail
                        )
                    }
                };
                println!("{icon}");
                if !server.enabled {
                    println!("      (disabled)");
                }
            }
        }
        Err(err) => {
            println!(
                "  {} MCP config parse error: {}",
                "✗".truecolor(red_r, red_g, red_b),
                err
            );
        }
    }

    // Skills configuration
    println!();
    println!("{}", "Skills:".bold());
    let global_skills_dir = config.skills_dir();
    let agents_skills_dir = workspace.join(".agents").join("skills");
    let local_skills_dir = workspace.join("skills");
    let agents_global_skills_dir = crate::skills::agents_global_skills_dir();
    // #432: cross-tool skill discovery dirs. Presence is reported here
    // even though they sit lower in the precedence chain so users can
    // see at a glance whether a `.opencode/skills/`, `.claude/skills/`,
    // `.cursor/skills/`, or global agentskills.io directory is contributing
    // to the merged catalogue.
    let opencode_skills_dir = workspace.join(".opencode").join("skills");
    let claude_skills_dir = workspace.join(".claude").join("skills");
    let selected_skills_dir = if agents_skills_dir.exists() {
        agents_skills_dir.clone()
    } else if local_skills_dir.exists() {
        local_skills_dir.clone()
    } else if config.skills_dir.is_none()
        && let Some(global_agents) = agents_global_skills_dir.as_ref()
        && global_agents.exists()
    {
        global_agents.clone()
    } else {
        global_skills_dir.clone()
    };

    let describe_dir = |dir: &Path| -> usize {
        std::fs::read_dir(dir)
            .map(|entries| entries.filter_map(std::result::Result::ok).count())
            .unwrap_or(0)
    };

    if local_skills_dir.exists() {
        println!(
            "  {} local skills dir found at {} ({} items)",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&local_skills_dir),
            describe_dir(&local_skills_dir)
        );
    } else {
        println!(
            "  {} local skills dir not found at {}",
            "·".dimmed(),
            crate::utils::display_path(&local_skills_dir)
        );
    }

    if agents_skills_dir.exists() {
        println!(
            "  {} .agents skills dir found at {} ({} items)",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&agents_skills_dir),
            describe_dir(&agents_skills_dir)
        );
    } else {
        println!(
            "  {} .agents skills dir not found at {}",
            "·".dimmed(),
            crate::utils::display_path(&agents_skills_dir)
        );
    }

    if let Some(agents_global_skills_dir) = agents_global_skills_dir.as_ref() {
        if agents_global_skills_dir.exists() {
            println!(
                "  {} global .agents skills dir found at {} ({} items)",
                "✓".truecolor(aqua_r, aqua_g, aqua_b),
                crate::utils::display_path(agents_global_skills_dir),
                describe_dir(agents_global_skills_dir)
            );
        } else {
            println!(
                "  {} global .agents skills dir not found at {}",
                "·".dimmed(),
                crate::utils::display_path(agents_global_skills_dir)
            );
        }
    }

    if global_skills_dir.exists() {
        println!(
            "  {} global skills dir found at {} ({} items)",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&global_skills_dir),
            describe_dir(&global_skills_dir)
        );
    } else {
        println!(
            "  {} global skills dir not found at {}",
            "·".dimmed(),
            crate::utils::display_path(&global_skills_dir)
        );
    }

    // #432: only print interop dirs when they're populated — empty
    // .opencode/.claude folders are common and would just clutter
    // the report with false-positive "absent" lines.
    if opencode_skills_dir.exists() {
        println!(
            "  {} .opencode skills dir found at {} ({} items)",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&opencode_skills_dir),
            describe_dir(&opencode_skills_dir)
        );
    }
    if claude_skills_dir.exists() {
        println!(
            "  {} .claude skills dir found at {} ({} items)",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&claude_skills_dir),
            describe_dir(&claude_skills_dir)
        );
    }

    println!(
        "  {} selected skills dir: {}",
        "·".dimmed(),
        crate::utils::display_path(&selected_skills_dir)
    );
    if !agents_skills_dir.exists()
        && !local_skills_dir.exists()
        && !agents_global_skills_dir
            .as_ref()
            .is_some_and(|dir| dir.exists())
        && !global_skills_dir.exists()
    {
        println!("    Run `mimo setup --skills` (or add --local for ./skills).");
    }

    // Tools directory
    println!();
    println!("{}", "Tools:".bold());
    let tools_dir = default_tools_dir();
    if tools_dir.exists() {
        let count = count_dir_entries(&tools_dir);
        println!(
            "  {} tools dir found at {} ({} items)",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&tools_dir),
            count
        );
    } else {
        println!(
            "  {} tools dir not found at {}",
            "·".dimmed(),
            crate::utils::display_path(&tools_dir)
        );
        println!("    Run `mimo setup --tools` to scaffold a starter dir.");
    }

    // Plugins directory
    println!();
    println!("{}", "Plugins:".bold());
    let plugins_dir = default_plugins_dir();
    if plugins_dir.exists() {
        let count = count_dir_entries(&plugins_dir);
        println!(
            "  {} plugins dir found at {} ({} items)",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            crate::utils::display_path(&plugins_dir),
            count
        );
    } else {
        println!(
            "  {} plugins dir not found at {}",
            "·".dimmed(),
            crate::utils::display_path(&plugins_dir)
        );
        println!("    Run `mimo setup --plugins` to scaffold a starter dir.");
    }

    // Storage surfaces (#422 / #440 / #500)
    println!();
    println!("{}", "Storage:".bold());
    if let Some(spillover_root) = crate::tools::truncate::spillover_root() {
        let (present, count) = if spillover_root.is_dir() {
            (true, count_dir_entries(&spillover_root))
        } else {
            (false, 0)
        };
        if present {
            println!(
                "  {} tool-output spillover at {} ({} file{})",
                "✓".truecolor(aqua_r, aqua_g, aqua_b),
                crate::utils::display_path(&spillover_root),
                count,
                if count == 1 { "" } else { "s" }
            );
        } else {
            println!(
                "  {} tool-output spillover dir not yet created at {}",
                "·".dimmed(),
                crate::utils::display_path(&spillover_root)
            );
        }
    }
    let stash_path = mimofan_config::mimofan_home()
        .ok()
        .map(|h| h.join("composer_stash.jsonl"));
    if let Some(stash_path) = stash_path {
        let stash_count = crate::composer_stash::load_stash().len();
        if stash_path.exists() {
            println!(
                "  {} composer stash at {} ({} parked draft{})",
                "✓".truecolor(aqua_r, aqua_g, aqua_b),
                crate::utils::display_path(&stash_path),
                stash_count,
                if stash_count == 1 { "" } else { "s" }
            );
        } else {
            println!(
                "  {} composer stash empty (Ctrl+S in the composer to park a draft)",
                "·".dimmed()
            );
        }
    }

    // Tool dependencies — probe external binaries that individual
    // tools rely on (Python for code_execution, pdftotext for PDF
    // reading) so users see explicit ✓/✗ rather than the tool failing
    // at execution time with "program not found". New in v0.8.31.
    println!();
    println!("{}", "Tool Dependencies:".bold());

    match crate::dependencies::resolve_python_interpreter() {
        Some(name) => println!(
            "  {} Python: {} → code_execution tool registered",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            name
        ),
        None => {
            println!(
                "  {} Python: not found (tried {:?})",
                "✗".truecolor(red_r, red_g, red_b),
                crate::dependencies::PYTHON_CANDIDATES,
            );
            println!("    code_execution tool is NOT advertised to the model on this install.");
            println!("    Install Python 3 and ensure one of those names is on PATH:");
            match std::env::consts::OS {
                "macos" => {
                    println!("      brew install python@3.12   (or download from python.org)")
                }
                "linux" => println!(
                    "      sudo apt install python3    (Debian/Ubuntu) — or your distro's equivalent"
                ),
                "windows" => {
                    println!("      winget install Python.Python.3   (or download from python.org)")
                }
                other => println!("      install Python 3 for {other} from python.org"),
            }
        }
    }

    match crate::dependencies::resolve_node() {
        Some(_) => println!(
            "  {} Node.js: present → js_execution tool registered",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
        ),
        None => {
            println!(
                "  {} Node.js: not found (tried `node`)",
                "✗".truecolor(red_r, red_g, red_b),
            );
            println!("    js_execution tool is NOT advertised to the model on this install.");
            println!("    Install Node 18+ and ensure `node` is on PATH:");
            match std::env::consts::OS {
                "macos" => println!("      brew install node   (or download from nodejs.org)"),
                "linux" => println!(
                    "      sudo apt install nodejs    (Debian/Ubuntu) — or your distro's equivalent"
                ),
                "windows" => {
                    println!("      winget install OpenJS.NodeJS   (or download from nodejs.org)")
                }
                other => println!("      install Node.js for {other} from nodejs.org"),
            }
        }
    }

    match crate::dependencies::resolve_pandoc() {
        Some(_) => println!(
            "  {} pandoc: present → pandoc_convert tool registered",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
        ),
        None => {
            println!("  {} pandoc: not found (optional)", "·".dimmed(),);
            println!(
                "    pandoc_convert tool is NOT advertised to the model. Install pandoc to enable:"
            );
            match std::env::consts::OS {
                "macos" => println!("      brew install pandoc"),
                "linux" => println!(
                    "      sudo apt install pandoc    (Debian/Ubuntu) — or your distro's equivalent"
                ),
                "windows" => {
                    println!("      winget install JohnMacFarlane.Pandoc")
                }
                other => println!("      install pandoc for {other} from pandoc.org"),
            }
        }
    }

    match crate::dependencies::resolve_tesseract() {
        Some(_) => {
            if cfg!(target_os = "macos") {
                println!(
                    "  {} OCR: macOS Vision + tesseract available → image_ocr/read_file screenshot OCR enabled",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                );
            } else {
                println!(
                    "  {} tesseract: present → image_ocr/read_file screenshot OCR enabled",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                );
            }
        }
        None => {
            if cfg!(target_os = "macos") {
                println!(
                    "  {} OCR: macOS Vision available → image_ocr/read_file screenshot OCR enabled",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                );
                println!(
                    "    tesseract not found (optional; install only for alternate OCR packs)."
                );
            } else {
                println!("  {} tesseract: not found (optional)", "·".dimmed(),);
                println!(
                    "    image_ocr tool is NOT advertised to the model. Install tesseract to enable:"
                );
                match std::env::consts::OS {
                    "macos" => println!("      brew install tesseract"),
                    "linux" => println!(
                        "      sudo apt install tesseract-ocr    (Debian/Ubuntu) — or your distro's equivalent"
                    ),
                    "windows" => println!("      winget install UB-Mannheim.TesseractOCR"),
                    other => {
                        println!("      install tesseract for {other} from tesseract-ocr.github.io")
                    }
                }
            }
        }
    }

    // PDF reader: pure-Rust `pdf-extract` is the v0.8.32 default, so
    // `pdftotext` is no longer required for `read_file` to handle PDFs.
    // We still surface its presence (a) so users with column-heavy PDFs
    // know they can opt in via `prefer_external_pdftotext = true`, and
    // (b) so users who *did* opt in get a clean signal when the binary
    // is missing rather than discovering it on the next PDF read.
    let prefer_external = crate::settings::Settings::load()
        .map(|s| s.prefer_external_pdftotext)
        .unwrap_or(false);
    match crate::dependencies::resolve_pdftotext() {
        Some(_) => {
            if prefer_external {
                println!(
                    "  {} pdftotext: available → read_file routes PDFs through Poppler (prefer_external_pdftotext = true)",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                );
            } else {
                println!(
                    "  {} pdftotext: available (optional — pure-Rust extractor is the default in v0.8.32)",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                );
                println!(
                    "    Set `prefer_external_pdftotext = true` in settings.json for column-heavy PDFs."
                );
            }
        }
        None => {
            if prefer_external {
                println!(
                    "  {} pdftotext: not found, but `prefer_external_pdftotext = true` is set → PDF reads will return `binary_unavailable`",
                    "✗".truecolor(red_r, red_g, red_b),
                );
                println!(
                    "    Either install Poppler or unset `prefer_external_pdftotext` to fall back to the bundled pure-Rust extractor."
                );
                match std::env::consts::OS {
                    "macos" => println!("    Install via: brew install poppler"),
                    "linux" => println!(
                        "    Install via: sudo apt install poppler-utils   (Debian/Ubuntu)"
                    ),
                    "windows" => println!(
                        "    Install Poppler for Windows from https://blog.alivate.com.au/poppler-windows/"
                    ),
                    _ => {}
                }
            } else {
                println!(
                    "  {} pdftotext: not found (optional — pure-Rust extractor is the default in v0.8.32)",
                    "·".dimmed(),
                );
                println!(
                    "    Install Poppler only if you want to opt into pdftotext for column-heavy PDFs."
                );
            }
        }
    }

    // Terminal-quirk overrides currently active. Mirrors the env
    // signals checked by `Settings::apply_env_overrides` so users
    // can see at a glance which a11y/compat overrides fired.
    println!();
    println!("{}", "Terminal Quirks:".bold());
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let term_program_lc = term_program.to_ascii_lowercase();
    let mut any_quirk = false;
    if matches!(term_program.as_str(), "vscode" | "ghostty") {
        println!(
            "  {} TERM_PROGRAM={} → low_motion + fancy_animations=false (auto)",
            "•".truecolor(sky_r, sky_g, sky_b),
            term_program
        );
        any_quirk = true;
    }
    if term_program == "Termius"
        || std::env::var_os("SSH_CLIENT").is_some_and(|v| !v.is_empty())
        || std::env::var_os("SSH_TTY").is_some_and(|v| !v.is_empty())
    {
        println!(
            "  {} SSH/Termius session → low_motion + fancy_animations=false (auto, #1433)",
            "•".truecolor(sky_r, sky_g, sky_b)
        );
        any_quirk = true;
    }
    if term_program_lc.contains("ptyxis")
        || std::env::var_os("PTYXIS_VERSION").is_some_and(|v| !v.is_empty())
    {
        println!(
            "  {} Ptyxis detected → synchronized_output=off (auto, v0.8.31)",
            "•".truecolor(sky_r, sky_g, sky_b)
        );
        any_quirk = true;
    }
    if crate::settings::detected_legacy_windows_console_host() {
        println!(
            "  {} legacy Windows console host → low_motion + fancy_animations=false + bracketed_paste=false + synchronized_output=off (auto)",
            "•".truecolor(sky_r, sky_g, sky_b)
        );
        any_quirk = true;
    }
    if !any_quirk {
        println!(
            "  {} no env-driven terminal-quirk overrides active",
            "·".dimmed()
        );
    }

    // Platform and sandbox checks
    println!();
    println!("{}", "Platform:".bold());
    println!("  OS: {}", std::env::consts::OS);
    println!("  Arch: {}", std::env::consts::ARCH);

    let sandbox = crate::sandbox::get_platform_sandbox();
    if let Some(kind) = sandbox {
        println!(
            "  {} sandbox available: {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            kind
        );
    } else {
        println!(
            "  {} sandbox not available (commands run best-effort)",
            "!".truecolor(sky_r, sky_g, sky_b)
        );
    }

    println!();
    println!(
        "{}",
        "All checks complete!"
            .truecolor(aqua_r, aqua_g, aqua_b)
            .bold()
    );
}

// ---------------------------------------------------------------------------
// run_doctor_json — machine-readable diagnostics
// ---------------------------------------------------------------------------

/// Machine-readable counterpart to `run_doctor`. Skips the live API call so it
/// is safe to run in CI and from non-interactive scripts.
pub(crate) fn run_doctor_json(
    config: &Config,
    workspace: &Path,
    config_path_override: Option<&Path>,
) -> Result<()> {
    use serde_json::json;

    let config_path = config_path_override
        .map(PathBuf::from)
        .or_else(|| mimofan_config::resolve_config_path(None).ok())
        .unwrap_or_else(|| {
            mimofan_config::mimofan_home()
                .unwrap_or_else(|_| PathBuf::from(".mimofan"))
                .join("config.toml")
        });

    let api_key_state = match resolve_api_key_source(config) {
        ApiKeySource::Command => "command",
        ApiKeySource::Env => "env",
        ApiKeySource::Config => "config",
        ApiKeySource::Keyring => "keyring",
        ApiKeySource::Secret => "secret",
        ApiKeySource::Missing => "missing",
    };

    let mcp_config_path = config.mcp_config_path();
    let project_mcp_config_path = crate::mcp::workspace_mcp_config_path(workspace);
    let mcp_present = mcp_config_path.exists();
    let project_mcp_present = project_mcp_config_path.exists();
    let mcp_summary = match crate::mcp::load_config_with_workspace(&mcp_config_path, workspace) {
        Ok(cfg) => {
            let servers: Vec<serde_json::Value> = cfg
                .servers
                .iter()
                .map(|(name, server)| {
                    let status = doctor_check_mcp_server(server);
                    let (kind, detail) = match &status {
                        McpServerDoctorStatus::Ok(d) => ("ok", d.clone()),
                        McpServerDoctorStatus::Warning(d) => ("warning", d.clone()),
                        McpServerDoctorStatus::Error(d) => ("error", d.clone()),
                    };
                    json!({
                        "name": name,
                        "enabled": server.enabled && !server.disabled,
                        "status": kind,
                        "detail": detail,
                    })
                })
                .collect();
            json!({
                "config_path": mcp_config_path.display().to_string(),
                "present": mcp_present,
                "project_config_path": project_mcp_config_path.display().to_string(),
                "project_present": project_mcp_present,
                "servers": servers,
            })
        }
        Err(err) => json!({
            "config_path": mcp_config_path.display().to_string(),
            "present": mcp_present,
            "project_config_path": project_mcp_config_path.display().to_string(),
            "project_present": project_mcp_present,
            "servers": [],
            "error": err.to_string(),
        }),
    };

    let global_skills_dir = config.skills_dir();
    let agents_skills_dir = workspace.join(".agents").join("skills");
    let local_skills_dir = workspace.join("skills");
    let agents_global_skills_dir = crate::skills::agents_global_skills_dir();
    // #432: cross-tool skill discovery dirs surface in the JSON
    // report so external dashboards can see whether any
    // `.opencode/skills/`, `.claude/skills/`, `.cursor/skills/`, or
    // global agentskills.io content is contributing to the merged catalogue.
    let opencode_skills_dir = workspace.join(".opencode").join("skills");
    let claude_skills_dir = workspace.join(".claude").join("skills");
    let selected_skills_dir = if agents_skills_dir.exists() {
        agents_skills_dir.clone()
    } else if local_skills_dir.exists() {
        local_skills_dir.clone()
    } else if config.skills_dir.is_none()
        && let Some(global_agents) = agents_global_skills_dir.as_ref()
        && global_agents.exists()
    {
        global_agents.clone()
    } else {
        global_skills_dir.clone()
    };
    let agents_global_summary = agents_global_skills_dir
        .as_ref()
        .map(|path| {
            json!({
                "path": path.display().to_string(),
                "present": path.exists(),
                "count": skills_count_for(path),
            })
        })
        .unwrap_or_else(|| {
            json!({
                "path": null,
                "present": false,
                "count": 0,
            })
        });

    let tools_dir = default_tools_dir();
    let plugins_dir = default_plugins_dir();

    // Memory feature state (#489). Operators ask "is memory on?" and
    // "where does it live?" — surface both here so the question can be
    // answered without booting the TUI. Both inputs are checked: the
    // config flag and the env-var override that the runtime would
    // honour. (The dedicated `Config::memory_enabled()` accessor lives
    // on the memory-MVP branch (#518); this duplicates the same logic
    // until the two PRs land and it can be replaced with a single
    // method call.)
    let memory_dir = config.memory_dir();
    let memory_enabled_env = std::env::var("MIMOFAN_MEMORY")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "on" | "true" | "yes" | "y" | "enabled"
            )
        })
        .unwrap_or(false);
    let memory_index_present = crate::memory::index_path(&memory_dir).exists();
    let memory_categories_present: Vec<String> = crate::memory::CATEGORIES
        .iter()
        .filter(|cat| {
            let p = crate::memory::category_path(&memory_dir, cat);
            p.exists()
                && std::fs::read_to_string(&p)
                    .map(|c| !c.trim().is_empty())
                    .unwrap_or(false)
        })
        .map(|s| s.to_string())
        .collect();
    let memory_summary = json!({
        // The MVP feature is opt-in by default; this defaults to false
        // on branches without the [memory] section in `Config`.
        "enabled": memory_enabled_env,
        "dir": memory_dir.display().to_string(),
        "index_present": memory_index_present,
        "categories_present": memory_categories_present,
    });
    let api_target = doctor_api_target(config);
    let strict_tool_mode = doctor_strict_tool_mode_status(config);
    let tls_status = doctor_tls_status(config);

    let report = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "config_path": config_path.display().to_string(),
        "config_present": config_path.exists(),
        "workspace": workspace.display().to_string(),
        "api_key": {
            "source": api_key_state,
        },
        "base_url": crate::client::redact_url_for_display(&api_target.base_url),
        "default_text_model": api_target.model,
        "route": doctor_route_report(config),
        "strict_tool_mode": {
            "enabled": strict_tool_mode.enabled,
            "status": strict_tool_mode.status,
            "function_strict_sent": strict_tool_mode.function_strict_sent,
            "message": strict_tool_mode.message,
            "recommended_base_url": strict_tool_mode.recommended_base_url,
        },
        "tls": {
            "certificate_verification": tls_status.certificate_verification,
            "insecure_skip_tls_verify": tls_status.insecure_skip_tls_verify,
            "provider": tls_status.provider,
            "message": tls_status.message,
        },
        "search_provider": doctor_search_provider_json(config),
        "memory": memory_summary,
        "mcp": mcp_summary,
        "skills": {
            "selected": selected_skills_dir.display().to_string(),
            "global": {
                "path": global_skills_dir.display().to_string(),
                "present": global_skills_dir.exists(),
                "count": skills_count_for(&global_skills_dir),
            },
            "agents": {
                "path": agents_skills_dir.display().to_string(),
                "present": agents_skills_dir.exists(),
                "count": skills_count_for(&agents_skills_dir),
            },
            "agents_global": agents_global_summary,
            "local": {
                "path": local_skills_dir.display().to_string(),
                "present": local_skills_dir.exists(),
                "count": skills_count_for(&local_skills_dir),
            },
            "opencode": {
                "path": opencode_skills_dir.display().to_string(),
                "present": opencode_skills_dir.exists(),
                "count": skills_count_for(&opencode_skills_dir),
            },
            "claude": {
                "path": claude_skills_dir.display().to_string(),
                "present": claude_skills_dir.exists(),
                "count": skills_count_for(&claude_skills_dir),
            },
        },
        "tools": {
            "path": tools_dir.display().to_string(),
            "present": tools_dir.exists(),
            "count": if tools_dir.exists() { count_dir_entries(&tools_dir) } else { 0 },
        },
        "plugins": {
            "path": plugins_dir.display().to_string(),
            "present": plugins_dir.exists(),
            "count": if plugins_dir.exists() { count_dir_entries(&plugins_dir) } else { 0 },
        },
        "storage": {
            "spillover": {
                "path": crate::tools::truncate::spillover_root()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                "present": crate::tools::truncate::spillover_root()
                    .is_some_and(|p| p.is_dir()),
                "count": crate::tools::truncate::spillover_root()
                    .filter(|p| p.is_dir())
                    .map(|p| count_dir_entries(&p))
                    .unwrap_or(0),
            },
            "stash": {
                "path": mimofan_config::mimofan_home()
                    .ok()
                    .map(|h| h.join("composer_stash.jsonl").display().to_string())
                    .unwrap_or_default(),
                "present": mimofan_config::mimofan_home()
                    .ok()
                    .map(|h| h.join("composer_stash.jsonl"))
                    .is_some_and(|p| p.exists()),
                "count": crate::composer_stash::load_stash().len(),
            },
        },
        "sandbox": match crate::sandbox::get_platform_sandbox() {
            Some(kind) => json!({"available": true, "kind": kind.to_string()}),
            None => json!({"available": false, "kind": null}),
        },
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        },
        "api_connectivity": {
            "checked": false,
            "note": "Skipped in --json mode; run `mimo doctor` for a live check.",
        },
        "capability": provider_capability_report(config),
    });

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// run_doctor_context_json
// ---------------------------------------------------------------------------

pub(crate) fn run_doctor_context_json(config: &Config, workspace: &Path) -> Result<()> {
    let report = crate::context_report::build_headless_context_report(config, workspace);
    println!("{}", crate::context_report::context_report_json(&report));
    Ok(())
}

// ---------------------------------------------------------------------------
// Capability / route / provider / protocol / TLS / timeout helpers
// ---------------------------------------------------------------------------

/// Build the `capability` section for the machine-readable doctor report.
///
/// Returns a JSON value with the resolved provider, resolved model, context
/// window, max output, thinking support, cache telemetry support, and request
/// payload mode.
pub(crate) fn provider_capability_report(config: &Config) -> serde_json::Value {
    use serde_json::json;

    let provider = config.api_provider();
    let model = config.default_model();

    let cap = crate::config::provider_capability(provider, &model);

    json!({
        "resolved_provider": provider.as_str(),
        "resolved_model": cap.resolved_model,
        "context_window": cap.context_window,
        "max_output": cap.max_output,
        "thinking_supported": cap.thinking_supported,
        "cache_telemetry_supported": cap.cache_telemetry_supported,
        "request_payload_mode": serde_json::to_value(cap.request_payload_mode).unwrap_or_default(),
        "alias_deprecation": cap.alias_deprecation,
    })
}

pub(crate) fn doctor_route_report(config: &Config) -> serde_json::Value {
    use serde_json::json;

    let target = doctor_api_target(config);
    let provider = config.api_provider();
    let redacted_base_url = crate::client::redact_url_for_display(&target.base_url);

    json!({
        "provider": target.provider,
        "provider_source": doctor_provider_source(config),
        "provider_config_table": provider_config_table_key(provider),
        "model": target.model,
        "wire_protocol": doctor_wire_protocol(provider),
        "base_url": {
            "redacted": redacted_base_url,
            "class": doctor_base_url_class(provider, &target.base_url),
            "fingerprint": crate::utils::redacted_identifier_for_log(&target.base_url),
        },
        "auth": {
            "scheme": doctor_auth_scheme(config),
            "source": doctor_api_key_source_label(resolve_api_key_source(config)),
        },
    })
}

pub(crate) fn doctor_provider_source(config: &Config) -> &'static str {
    if config
        .provider
        .as_ref()
        .is_some_and(|provider| !provider.trim().is_empty())
    {
        "config"
    } else {
        "default"
    }
}

pub(crate) fn doctor_wire_protocol(provider: crate::config::ApiProvider) -> &'static str {
    match provider
        .metadata()
        .map(|metadata| metadata.wire())
        .unwrap_or(mimofan_config::provider::WireFormat::ChatCompletions)
    {
        mimofan_config::provider::WireFormat::ChatCompletions => "chat_completions",
        mimofan_config::provider::WireFormat::Responses => "responses",
        mimofan_config::provider::WireFormat::AnthropicMessages => "anthropic_messages",
    }
}

pub(crate) fn doctor_base_url_class(
    provider: crate::config::ApiProvider,
    base_url: &str,
) -> &'static str {
    let normalized = base_url.trim_end_matches('/').to_ascii_lowercase();
    if normalized.starts_with("http://localhost")
        || normalized.starts_with("http://127.0.0.1")
        || normalized.starts_with("http://[::1]")
    {
        return "local";
    }
    if normalized
        == provider
            .default_base_url()
            .trim_end_matches('/')
            .to_ascii_lowercase()
    {
        "default"
    } else {
        "custom"
    }
}

pub(crate) fn doctor_auth_scheme(config: &Config) -> &'static str {
    let provider = config.api_provider();
    if provider == crate::config::ApiProvider::XiaomiMimo {
        if doctor_xiaomi_mimo_base_url_uses_token_plan(&config.api_base_url())
            || config
                .api_key()
                .ok()
                .is_some_and(|key| key.trim_start().starts_with("tp-"))
        {
            "api-key"
        } else {
            "x-api-key"
        }
    } else {
        "bearer"
    }
}

pub(crate) fn doctor_xiaomi_mimo_base_url_uses_token_plan(base_url: &str) -> bool {
    let normalized = base_url.trim_end_matches('/').to_ascii_lowercase();
    [
        crate::config::XIAOMI_MIMO_TOKEN_PLAN_CN_BASE_URL,
        crate::config::DEFAULT_XIAOMI_MIMO_BASE_URL,
    ]
    .iter()
    .any(|candidate| normalized == candidate.trim_end_matches('/').to_ascii_lowercase())
}

pub(crate) fn doctor_api_key_source_label(source: ApiKeySource) -> &'static str {
    match source {
        ApiKeySource::Command => "command",
        ApiKeySource::Env => "env",
        ApiKeySource::Config => "config",
        ApiKeySource::Keyring => "keyring",
        ApiKeySource::Secret => "secret",
        ApiKeySource::Missing => "missing",
    }
}

pub(crate) fn doctor_search_provider_line(config: &Config) -> String {
    let search_provider = config.search_provider_resolution();
    let switch_hint = if matches!(
        (search_provider.provider, search_provider.source),
        (
            crate::config::SearchProvider::DuckDuckGo,
            crate::config::SearchProviderSource::Default
        )
    ) {
        "; set [search] provider = \"bing\" | \"tavily\" | \"bocha\" to switch"
    } else {
        ""
    };

    format!(
        "search_provider: {} (source: {}{})",
        search_provider.provider.as_str(),
        search_provider.source.as_str(),
        switch_hint
    )
}

pub(crate) fn doctor_search_provider_json(config: &Config) -> serde_json::Value {
    use serde_json::json;

    let search_provider = config.search_provider_resolution();
    json!({
        "provider": search_provider.provider.as_str(),
        "source": search_provider.source.as_str(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorApiTarget {
    pub(crate) provider: &'static str,
    pub(crate) base_url: String,
    pub(crate) model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorStrictToolModeStatus {
    pub(crate) enabled: bool,
    pub(crate) status: &'static str,
    pub(crate) function_strict_sent: bool,
    pub(crate) message: String,
    pub(crate) recommended_base_url: Option<String>,
}

pub(crate) fn doctor_api_target(config: &Config) -> DoctorApiTarget {
    let provider = config.api_provider();
    DoctorApiTarget {
        provider: provider.as_str(),
        base_url: config.api_base_url(),
        model: config.default_model(),
    }
}

pub(crate) fn doctor_strict_tool_mode_status(config: &Config) -> DoctorStrictToolModeStatus {
    if !config.strict_tool_mode.unwrap_or(false) {
        return DoctorStrictToolModeStatus {
            enabled: false,
            status: "disabled",
            function_strict_sent: false,
            message: "disabled".to_string(),
            recommended_base_url: None,
        };
    }

    let target = doctor_api_target(config);
    match known_api_base_url_kind(&target.base_url) {
        Some(BaseUrlKind::Beta) => DoctorStrictToolModeStatus {
            enabled: true,
            status: "ready",
            function_strict_sent: true,
            message: "enabled; DeepSeek strict schemas use the beta endpoint".to_string(),
            recommended_base_url: None,
        },
        Some(BaseUrlKind::NonBeta) => {
            let recommended = recommended_strict_base_url(config, &target.base_url);
            DoctorStrictToolModeStatus {
                enabled: true,
                status: "fallback_non_beta",
                function_strict_sent: false,
                message:
                    "enabled, but function.strict is stripped for this non-beta DeepSeek endpoint"
                        .to_string(),
                recommended_base_url: Some(recommended.to_string()),
            }
        }
        None => DoctorStrictToolModeStatus {
            enabled: true,
            status: "custom_endpoint",
            function_strict_sent: true,
            message: "enabled; function.strict will be sent to this custom endpoint".to_string(),
            recommended_base_url: None,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorTlsStatus {
    pub(crate) certificate_verification: bool,
    pub(crate) insecure_skip_tls_verify: bool,
    pub(crate) provider: &'static str,
    pub(crate) message: String,
}

pub(crate) fn doctor_tls_status(config: &Config) -> DoctorTlsStatus {
    let provider = config.api_provider().as_str();
    let insecure_skip_tls_verify = config.insecure_skip_tls_verify();
    DoctorTlsStatus {
        certificate_verification: true,
        insecure_skip_tls_verify,
        provider,
        message: if insecure_skip_tls_verify {
            format!(
                "TLS certificate verification cannot be disabled for provider {provider}; use SSL_CERT_FILE with a trusted custom CA bundle"
            )
        } else {
            "TLS certificate verification enabled".to_string()
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaseUrlKind {
    Beta,
    NonBeta,
}

pub(crate) fn known_api_base_url_kind(base_url: &str) -> Option<BaseUrlKind> {
    match base_url.trim_end_matches('/').to_ascii_lowercase().as_str() {
        "https://api.deepseek.com/beta" | "https://api.deepseeki.com/beta" => {
            Some(BaseUrlKind::Beta)
        }
        "https://api.deepseek.com"
        | "https://api.deepseek.com/v1"
        | "https://api.deepseeki.com"
        | "https://api.deepseeki.com/v1" => Some(BaseUrlKind::NonBeta),
        _ => None,
    }
}

pub(crate) fn recommended_strict_base_url(_config: &Config, _base_url: &str) -> &'static str {
    crate::config::DEFAULT_MIMO_BASE_URL
}

pub(crate) fn doctor_timeout_recovery_lines(config: &Config) -> Vec<String> {
    let target = doctor_api_target(config);
    let mut lines = vec![format!(
        "Connection timed out while reaching {}.",
        target.base_url
    )];

    match config.api_provider() {
        crate::config::ApiProvider::XiaomiMimo
            if target.base_url.contains("api.deepseek.com")
                && !target.base_url.contains("api.deepseeki.com") =>
        {
            lines.push(
                "If this is a custom DeepSeek-compatible endpoint, set its HTTPS base URL in ~/.mimofan/config.toml and rerun `mimo doctor`."
                    .to_string(),
            );
        }
        crate::config::ApiProvider::XiaomiMimo => {
            lines.push(
                "If this is a custom DeepSeek-compatible endpoint, confirm it serves `/v1/models` and `/v1/chat/completions` over HTTPS."
                    .to_string(),
            );
        }
        _ => {
            lines.push(
                "Confirm the configured provider endpoint is reachable and OpenAI-compatible for `/v1/models` and `/v1/chat/completions`."
                    .to_string(),
            );
        }
    }

    lines.push(
        "Run `mimo doctor --json` and include `base_url`, `default_text_model`, and `api_connectivity` when filing an issue."
            .to_string(),
    );
    lines
}

// ---------------------------------------------------------------------------
// MCP server diagnostic check
// ---------------------------------------------------------------------------

/// Diagnostic status for an MCP server entry.
#[derive(Debug)]
pub(crate) enum McpServerDoctorStatus {
    Ok(String),
    Warning(String),
    Error(String),
}

/// Check an MCP server config entry for common issues.
pub(crate) fn doctor_check_mcp_server(server: &McpServerConfig) -> McpServerDoctorStatus {
    // No command or URL — incomplete entry.
    if server.command.is_none() && server.url.is_none() {
        return McpServerDoctorStatus::Error("no command or url configured".to_string());
    }

    // URL-based server — just report the URL.
    if let Some(ref url) = server.url {
        return McpServerDoctorStatus::Ok(format!("HTTP/SSE server at {url}"));
    }

    // Command-based: validate command path exists.
    let cmd = server.command.as_deref().unwrap_or("");
    if cmd.is_empty() {
        return McpServerDoctorStatus::Error("empty command".to_string());
    }

    let cmd_path = Path::new(cmd);
    // Also accept Unix-style `/` prefix on Windows, where Path::is_absolute()
    // requires a drive letter.
    let is_absolute = cmd_path.is_absolute() || cmd.starts_with('/');

    if is_absolute && !cmd_path.exists() {
        return McpServerDoctorStatus::Error(format!("command not found: {cmd}"));
    }

    // Detect self-hosted DeepSeek server entries.
    let is_self_hosted = server
        .args
        .windows(2)
        .any(|w| w[0] == "serve" && w[1] == "--mcp");

    let args_str = server.args.join(" ");
    if is_self_hosted {
        if is_absolute {
            McpServerDoctorStatus::Ok(format!("self-hosted MCP server ({cmd} {args_str})"))
        } else {
            McpServerDoctorStatus::Warning(format!(
                "self-hosted MCP server uses relative command \"{cmd}\" — consider using an absolute path"
            ))
        }
    } else {
        McpServerDoctorStatus::Ok(format!(
            "stdio server ({cmd}{})",
            if args_str.is_empty() {
                String::new()
            } else {
                format!(" {args_str}")
            }
        ))
    }
}
