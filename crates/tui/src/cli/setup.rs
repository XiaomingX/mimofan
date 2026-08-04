//! Setup/init functions extracted from `lib.rs`.

use super::*;
use std::path::{Path, PathBuf};

use crate::config::{Config, DEFAULT_TEXT_MODEL};
use crate::mcp::{McpConfig, McpServerConfig};
use anyhow::{Context, Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStatus {
    Created,
    Overwritten,
    SkippedExists,
}

pub(crate) fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory for {}", parent.display()))?;
    }
    Ok(())
}

pub(crate) fn write_template_file(path: &Path, contents: &str, force: bool) -> Result<WriteStatus> {
    ensure_parent_dir(path)?;

    if path.exists() && !force {
        return Ok(WriteStatus::SkippedExists);
    }

    let status = if path.exists() {
        WriteStatus::Overwritten
    } else {
        WriteStatus::Created
    };

    std::fs::write(path, contents)
        .with_context(|| format!("Failed to write template at {}", path.display()))?;

    Ok(status)
}

pub(crate) fn mcp_template_json() -> Result<String> {
    let mut cfg = McpConfig::default();
    cfg.servers.insert(
        "example".to_string(),
        McpServerConfig {
            command: Some("node".to_string()),
            args: vec!["./path/to/your-mcp-server.js".to_string()],
            env: std::collections::HashMap::new(),
            cwd: None,
            url: None,
            transport: None,
            connect_timeout: None,
            execute_timeout: None,
            read_timeout: None,
            disabled: true,
            enabled: true,
            required: false,
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            headers: std::collections::HashMap::new(),
            env_headers: std::collections::HashMap::new(),
            bearer_token_env_var: None,
            scopes: Vec::new(),
            oauth: None,
            oauth_resource: None,
        },
    );
    serde_json::to_string_pretty(&cfg)
        .map_err(|e| anyhow!("Failed to render MCP template JSON: {e}"))
}

pub(crate) fn init_mcp_config(path: &Path, force: bool) -> Result<WriteStatus> {
    let template = mcp_template_json()?;
    write_template_file(path, &template, force)
}

pub(crate) fn skills_template(name: &str) -> String {
    format!(
        "\
---\n\
name: {name}\n\
description: Quick repo diagnostics and setup guidance\n\
allowed-tools: diagnostics, list_dir, read_file, grep_files, git_status, git_diff\n\
---\n\n\
When this skill is active:\n\
1. Run the diagnostics tool to report workspace and sandbox status.\n\
2. Skim key project files (README.md, Cargo.toml, AGENTS.md) before editing.\n\
3. Prefer small, validated changes and summarize what you verified.\n\
"
    )
}

pub(crate) fn init_skills_dir(skills_dir: &Path, force: bool) -> Result<(PathBuf, WriteStatus)> {
    std::fs::create_dir_all(skills_dir)
        .with_context(|| format!("Failed to create skills dir {}", skills_dir.display()))?;

    let skill_name = "getting-started";
    let skill_path = skills_dir.join(skill_name).join("SKILL.md");
    ensure_parent_dir(&skill_path)?;

    let status = write_template_file(&skill_path, &skills_template(skill_name), force)?;
    Ok((skill_path, status))
}

pub(crate) fn tools_readme_template() -> &'static str {
    "# Local tools\n\n\
     Drop self-describing scripts here so they can be discovered by\n\
     `mimofan setup --status` and surfaced in `mimofan doctor`.\n\n\
     When `[tools.plugin_dir]` is set in config.toml (or when the default\n\
     `~/.mimofan/tools/` directory exists), they are auto-discovered and\n\
     registered as model-visible tools.\n\n\
     Each script should start with a frontmatter-style header so the\n\
     description is visible without executing the file and the agent knows\n\
     the tool name, description, and input schema:\n\n\
     ```\n\
     # name: my-tool\n\
     # description: One-line summary of what this tool does\n\
     # usage: my-tool [args...]\n\
     ```\n\n\
     The directory is intentionally not auto-loaded into the agent's tool\n\
     catalog. Wire individual tools through MCP, hooks, or skills when you\n\
     want them available inside a session.\n"
}

pub(crate) fn tools_example_script() -> &'static str {
    "#!/usr/bin/env sh\n\
     # name: example\n\
     # description: Print a confirmation that local tool discovery works\n\
     # usage: example [name]\n\
     printf 'mimofan local tool ok: %s\\n' \"${1:-world}\"\n"
}

pub(crate) fn init_tools_dir(
    tools_dir: &Path,
    force: bool,
) -> Result<(PathBuf, WriteStatus, WriteStatus)> {
    std::fs::create_dir_all(tools_dir)
        .with_context(|| format!("Failed to create tools dir {}", tools_dir.display()))?;

    let readme_path = tools_dir.join("README.md");
    let readme_status = write_template_file(&readme_path, tools_readme_template(), force)?;

    let example_path = tools_dir.join("example.sh");
    let example_status = write_template_file(&example_path, tools_example_script(), force)?;

    Ok((tools_dir.to_path_buf(), readme_status, example_status))
}

pub(crate) fn plugins_readme_template() -> &'static str {
    "# Local plugins\n\n\
     Plugins are richer than tools: each one lives in its own subdirectory\n\
     with a `PLUGIN.md` describing what it does and how to enable it. The\n\
     directory is created so users have a documented place to drop\n\
     experiments without touching `~/.mimofan/skills/`.\n\n\
     A plugin layout looks like:\n\n\
     ```\n\
     plugins/\n\
       my-plugin/\n\
         PLUGIN.md   # frontmatter + body, same shape as SKILL.md\n\
         scripts/    # optional helpers invoked by the plugin\n\
     ```\n\n\
     Plugins are not loaded automatically. Wire them up through skills,\n\
     hooks, or MCP servers when you want them active in a session.\n"
}

pub(crate) fn plugin_example_template() -> &'static str {
    "---\n\
     name: example\n\
     description: Placeholder plugin so /skills and doctor have something to show\n\
     status: example\n\
     ---\n\n\
     This is a starter plugin layout. Edit or replace it once you have a\n\
     real plugin. The agent does not load this file directly; reference it\n\
     from a skill or MCP wrapper if you want it active in a session.\n"
}

pub(crate) fn init_plugins_dir(
    plugins_dir: &Path,
    force: bool,
) -> Result<(PathBuf, PathBuf, WriteStatus, WriteStatus)> {
    std::fs::create_dir_all(plugins_dir)
        .with_context(|| format!("Failed to create plugins dir {}", plugins_dir.display()))?;

    let readme_path = plugins_dir.join("README.md");
    let readme_status = write_template_file(&readme_path, plugins_readme_template(), force)?;

    let example_path = plugins_dir.join("example").join("PLUGIN.md");
    ensure_parent_dir(&example_path)?;
    let example_status = write_template_file(&example_path, plugin_example_template(), force)?;

    Ok((readme_path, example_path, readme_status, example_status))
}

/// Resolve the user-supplied CORS origins for `mimo serve --http`.
///
/// Sources, in priority order (later sources extend earlier ones):
/// 1. `--cors-origin URL` flags (repeatable)
/// 2. `MIMOFAN_CORS_ORIGINS` env var (comma-separated),
///    then `MIMOFAN_CORS_ORIGINS` as an alias
/// 3. `[runtime_api] cors_origins = [...]` in `config.toml`
///
/// The runtime API always allows the built-in dev defaults
/// (localhost:3000, localhost:1420, tauri://localhost). User entries are
/// appended on top — empty strings are skipped, and duplicates are deduped
/// while preserving first-seen order. Mimofanscale#255 / #561.
pub(crate) fn resolve_cors_origins(config: &Config, flag_origins: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        if !out.iter().any(|existing| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    };
    for o in flag_origins {
        push(o);
    }
    if let Ok(env_value) = std::env::var("MIMOFAN_CORS_ORIGINS") {
        for piece in env_value.split(',') {
            push(piece);
        }
    }
    if let Some(rt) = &config.runtime_api
        && let Some(list) = &rt.cors_origins
    {
        for o in list {
            push(o);
        }
    }
    out
}

pub(crate) fn deepseek_home_dir() -> PathBuf {
    mimofan_config::mimofan_home().unwrap_or_else(|_| {
        dirs::home_dir().map_or_else(|| PathBuf::from(".mimofan"), |h| h.join(".mimofan"))
    })
}

/// Resolve the default tools directory. Mirrors `default_skills_dir` shape.
pub(crate) fn default_tools_dir() -> PathBuf {
    deepseek_home_dir().join("tools")
}

/// Resolve the default plugins directory.
pub(crate) fn default_plugins_dir() -> PathBuf {
    deepseek_home_dir().join("plugins")
}

/// Default location for crash/offline-queue checkpoints managed by the TUI.
pub(crate) fn default_checkpoints_dir() -> PathBuf {
    deepseek_home_dir().join("sessions").join("checkpoints")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CleanPlan {
    pub(crate) targets: Vec<PathBuf>,
}

pub(crate) fn collect_clean_targets(checkpoints_dir: &Path) -> CleanPlan {
    let candidates = ["latest.json", "offline_queue.json"];
    let targets = candidates
        .iter()
        .map(|name| checkpoints_dir.join(name))
        .filter(|p| p.exists())
        .collect();
    CleanPlan { targets }
}

pub(crate) fn execute_clean_plan(plan: &CleanPlan) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::with_capacity(plan.targets.len());
    for path in &plan.targets {
        std::fs::remove_file(path)
            .with_context(|| format!("Failed to remove {}", path.display()))?;
        removed.push(path.clone());
    }
    Ok(removed)
}

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

pub(crate) fn run_setup(config: &Config, workspace: &Path, args: SetupArgs) -> Result<()> {
    if args.status {
        return run_setup_status(config, workspace);
    }
    if args.clean {
        return run_setup_clean(&default_checkpoints_dir(), args.force);
    }

    use crate::palette;
    use colored::Colorize;

    let (aqua_r, aqua_g, aqua_b) = palette::MIMOFAN_SKY_RGB;
    let (sky_r, sky_g, sky_b) = palette::MIMOFAN_SKY_RGB;

    let any_explicit = args.mcp || args.skills || args.tools || args.plugins;
    let run_mcp = args.mcp || args.all || !any_explicit;
    let run_skills = args.skills || args.all || !any_explicit;
    let run_tools = args.tools || args.all;
    let run_plugins = args.plugins || args.all;

    println!(
        "{}",
        "DeepSeek Setup".truecolor(aqua_r, aqua_g, aqua_b).bold()
    );
    println!("{}", "==============".truecolor(sky_r, sky_g, sky_b));
    println!("Workspace: {}", crate::utils::display_path(workspace));

    if run_mcp {
        let mcp_path = config.mcp_config_path();
        let status = init_mcp_config(&mcp_path, args.force)?;
        match status {
            WriteStatus::Created => {
                println!("  ✓ Created MCP config at {}", mcp_path.display());
            }
            WriteStatus::Overwritten => {
                println!("  ✓ Overwrote MCP config at {}", mcp_path.display());
            }
            WriteStatus::SkippedExists => {
                println!("  · MCP config already exists at {}", mcp_path.display());
            }
        }
        println!("    Next: edit the file, then run `mimo mcp list` or `mimo mcp tools`.");
    }

    if run_skills {
        let skills_dir = if args.local {
            workspace.join("skills")
        } else {
            config.skills_dir()
        };
        let (skill_path, status) = init_skills_dir(&skills_dir, args.force)?;
        match status {
            WriteStatus::Created => {
                println!("  ✓ Created example skill at {}", skill_path.display());
            }
            WriteStatus::Overwritten => {
                println!("  ✓ Overwrote example skill at {}", skill_path.display());
            }
            WriteStatus::SkippedExists => {
                println!(
                    "  · Example skill already exists at {}",
                    skill_path.display()
                );
            }
        }
        if args.local {
            println!(
                "    Local skills dir enabled for this workspace: {}",
                crate::utils::display_path(&skills_dir)
            );
        } else {
            println!(
                "    Skills dir: {}",
                crate::utils::display_path(&skills_dir)
            );
        }
        println!("    Next: run the TUI and use `/skills` then `/skill getting-started`.");
    }

    if run_tools {
        let tools_dir = default_tools_dir();
        let (dir, readme_status, example_status) = init_tools_dir(&tools_dir, args.force)?;
        report_write_status("Tools README", &dir.join("README.md"), readme_status);
        report_write_status("Example tool", &dir.join("example.sh"), example_status);
        println!("    Tools dir: {}", crate::utils::display_path(&dir));
        println!("    Next: drop scripts here; surface them via skills/MCP when ready.");
    }

    if run_plugins {
        let plugins_dir = default_plugins_dir();
        let (readme_path, example_path, readme_status, example_status) =
            init_plugins_dir(&plugins_dir, args.force)?;
        report_write_status("Plugins README", &readme_path, readme_status);
        report_write_status("Example plugin", &example_path, example_status);
        println!(
            "    Plugins dir: {}",
            crate::utils::display_path(&plugins_dir)
        );
        println!("    Next: copy the example dir, edit PLUGIN.md, wire via skill/MCP.");
    }

    let sandbox = crate::sandbox::get_platform_sandbox();
    if let Some(kind) = sandbox {
        println!("  ✓ Sandbox available: {kind}");
    } else {
        println!("  · Sandbox not available on this platform (best-effort only).");
    }

    Ok(())
}

pub(crate) fn report_write_status(label: &str, path: &Path, status: WriteStatus) {
    match status {
        WriteStatus::Created => {
            println!("  ✓ Created {label} at {}", path.display());
        }
        WriteStatus::Overwritten => {
            println!("  ✓ Overwrote {label} at {}", path.display());
        }
        WriteStatus::SkippedExists => {
            println!("  · {label} already exists at {}", path.display());
        }
    }
}

pub(crate) fn run_setup_status(config: &Config, workspace: &Path) -> Result<()> {
    use crate::palette;
    use colored::Colorize;

    let (aqua_r, aqua_g, aqua_b) = palette::MIMOFAN_SKY_RGB;
    let (sky_r, sky_g, sky_b) = palette::MIMOFAN_SKY_RGB;
    let (red_r, red_g, red_b) = palette::MIMOFAN_RED_RGB;

    println!(
        "{}",
        "DeepSeek Status".truecolor(aqua_r, aqua_g, aqua_b).bold()
    );
    println!("{}", "===============".truecolor(sky_r, sky_g, sky_b));
    println!("workspace: {}", workspace.display());

    match resolve_api_key_source(config) {
        ApiKeySource::Command => println!(
            "  {} api_key: configured via auth command",
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        ),
        ApiKeySource::Env => {
            let env_vars = provider_env_key_source(config.api_provider())
                .map(str::to_string)
                .unwrap_or_else(|| provider_env_vars_label(config.api_provider()));
            println!(
                "  {} api_key: set via {env_vars}",
                "✓".truecolor(aqua_r, aqua_g, aqua_b)
            );
        }
        ApiKeySource::Keyring => println!(
            "  {} api_key: set via OS keyring",
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        ),
        ApiKeySource::Config => println!(
            "  {} api_key: set via config",
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        ),
        ApiKeySource::Secret => println!(
            "  {} api_key: configured via secret source",
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        ),
        ApiKeySource::Missing => {
            let provider = config.api_provider();
            let env_var = provider_env_vars_label(provider);
            let login_hint = provider_auth_hint(provider);
            let table_key = provider_config_table_key(provider);
            println!(
                "  {} api_key: missing  (set {env_var} or `[providers.{table_key}].api_key` in ~/.mimofan/config.toml; or run `{login_hint}`)",
                "✗".truecolor(red_r, red_g, red_b),
            );
        }
    }
    println!(
        "  · base_url: {}",
        crate::client::redact_url_for_display(&config.api_base_url())
    );
    let model = config
        .default_text_model
        .clone()
        .unwrap_or_else(|| DEFAULT_TEXT_MODEL.to_string());
    println!("  · default_text_model: {model}");

    let mcp_path = config.mcp_config_path();
    let project_mcp_path = crate::mcp::workspace_mcp_config_path(workspace);
    let mcp_count = match crate::mcp::load_config_with_workspace(&mcp_path, workspace) {
        Ok(cfg) => cfg.servers.len(),
        Err(_) => 0,
    };
    let mcp_present = if mcp_path.exists() { "" } else { "  (missing)" };
    let project_mcp_present = if project_mcp_path.exists() {
        ""
    } else {
        "  (missing)"
    };
    println!(
        "  · mcp servers: {mcp_count} from {}{mcp_present} + {}{project_mcp_present}",
        mcp_path.display(),
        project_mcp_path.display()
    );

    let skills_dir = config.skills_dir();
    println!(
        "  · skills: {} at {}",
        skills_count_for(&skills_dir),
        crate::utils::display_path(&skills_dir)
    );

    let tools_dir = default_tools_dir();
    let tools_present = if tools_dir.exists() {
        ""
    } else {
        "  (missing — run `setup --tools`)"
    };
    println!(
        "  · tools: {} entries at {}{tools_present}",
        if tools_dir.exists() {
            count_dir_entries(&tools_dir)
        } else {
            0
        },
        crate::utils::display_path(&tools_dir)
    );

    let plugins_dir = default_plugins_dir();
    let plugins_present = if plugins_dir.exists() {
        ""
    } else {
        "  (missing — run `setup --plugins`)"
    };
    println!(
        "  · plugins: {} entries at {}{plugins_present}",
        if plugins_dir.exists() {
            count_dir_entries(&plugins_dir)
        } else {
            0
        },
        crate::utils::display_path(&plugins_dir)
    );

    let sandbox = crate::sandbox::get_platform_sandbox();
    match sandbox {
        Some(kind) => println!(
            "  {} sandbox: {kind}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        ),
        None => println!(
            "  {} sandbox: unavailable (commands run best-effort)",
            "!".truecolor(sky_r, sky_g, sky_b)
        ),
    }

    println!("  {} {}", "·".dimmed(), dotenv_status_line(workspace));

    println!();
    println!("Run `mimo doctor --json` for a machine-readable check.");
    Ok(())
}

pub(crate) fn dotenv_status_line(workspace: &Path) -> String {
    let dotenv = workspace.join(".env");
    if dotenv.exists() {
        return format!(".env present at {}", dotenv.display());
    }

    if workspace.join(".env.example").exists() {
        return ".env not present in workspace (run `cp .env.example .env` and edit)".to_string();
    }

    ".env not present in workspace".to_string()
}

pub(crate) fn run_setup_clean(checkpoints_dir: &Path, force: bool) -> Result<()> {
    use colored::Colorize;

    if !checkpoints_dir.exists() {
        println!(
            "Nothing to clean — checkpoints dir does not exist: {}",
            checkpoints_dir.display()
        );
        return Ok(());
    }

    let plan = collect_clean_targets(checkpoints_dir);
    if plan.targets.is_empty() {
        println!(
            "Nothing to clean — no checkpoint files in {}",
            checkpoints_dir.display()
        );
        return Ok(());
    }

    if !force {
        println!(
            "Would remove {} checkpoint file(s) (use --force to apply):",
            plan.targets.len()
        );
        for path in &plan.targets {
            println!("  · {}", path.display());
        }
        return Ok(());
    }

    let removed = execute_clean_plan(&plan)?;
    println!("{}", "Cleaned checkpoints:".bold());
    for path in &removed {
        println!("  ✓ {}", path.display());
    }
    Ok(())
}

/// Initialize a new project with AGENTS.md
pub(crate) fn init_project() -> Result<()> {
    use crate::palette;
    use crate::project_context::create_default_agents_md;
    use colored::Colorize;

    let (sky_r, sky_g, sky_b) = palette::MIMOFAN_SKY_RGB;
    let (aqua_r, aqua_g, aqua_b) = palette::MIMOFAN_SKY_RGB;
    let (red_r, red_g, red_b) = palette::MIMOFAN_RED_RGB;

    let workspace = std::env::current_dir()?;
    let agents_path = workspace.join("AGENTS.md");

    if agents_path.exists() {
        println!(
            "{} AGENTS.md already exists at {}",
            "!".truecolor(sky_r, sky_g, sky_b),
            agents_path.display()
        );
        return Ok(());
    }

    match create_default_agents_md(&workspace) {
        Ok(path) => {
            println!(
                "{} Created {}",
                "✓".truecolor(aqua_r, aqua_g, aqua_b),
                path.display()
            );
            println!();
            println!("Edit this file to customize how the AI agent works with your project.");
            println!("The instructions will be loaded automatically when you run mimo.");
        }
        Err(e) => {
            println!(
                "{} Failed to create AGENTS.md: {}",
                "✗".truecolor(red_r, red_g, red_b),
                e
            );
        }
    }

    Ok(())
}
