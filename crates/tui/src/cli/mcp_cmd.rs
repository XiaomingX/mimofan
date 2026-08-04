use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use crate::cli::McpCommand;
use crate::config::Config;
use crate::mcp::{McpConfig, McpPool, McpServerConfig, McpServerOAuthConfig};

use super::*;

pub(crate) async fn run_mcp_command(
    config: &Config,
    workspace: &Path,
    command: McpCommand,
) -> Result<()> {
    let config_path = config.mcp_config_path();
    match command {
        McpCommand::Init { force } => {
            let status = init_mcp_config(&config_path, force)?;
            match status {
                WriteStatus::Created => {
                    println!("Created MCP config at {}", config_path.display());
                }
                WriteStatus::Overwritten => {
                    println!("Overwrote MCP config at {}", config_path.display());
                }
                WriteStatus::SkippedExists => {
                    println!(
                        "MCP config already exists at {} (use --force to overwrite)",
                        config_path.display()
                    );
                }
            }
            println!("Edit the file, then run `mimo mcp list` or `mimo mcp tools`.");
            Ok(())
        }
        McpCommand::List => {
            let cfg = crate::mcp::load_config_with_workspace(&config_path, workspace)?;
            if cfg.servers.is_empty() {
                println!(
                    "No MCP servers configured in {} or {}",
                    config_path.display(),
                    crate::mcp::workspace_mcp_config_path(workspace).display()
                );
                return Ok(());
            }
            println!("MCP servers ({}):", cfg.servers.len());
            for (name, server) in cfg.servers {
                let status = if server.enabled && !server.disabled {
                    "enabled"
                } else {
                    "disabled"
                };
                let auth_status = crate::mcp::oauth::auth_status_for_server(&name, &server).await;
                let auth = if auth_status == crate::mcp::oauth::McpAuthStatus::Unsupported {
                    String::new()
                } else {
                    format!(
                        " auth={}",
                        auth_status
                            .to_string()
                            .to_ascii_lowercase()
                            .replace(' ', "-")
                    )
                };
                let args = if server.args.is_empty() {
                    "".to_string()
                } else {
                    format!(" {}", server.args.join(" "))
                };
                let cmd_str = if let Some(cmd) = server.command {
                    format!("{cmd}{args}")
                } else if let Some(url) = server.url {
                    url
                } else {
                    "unknown".to_string()
                };
                let required = if server.required { " required" } else { "" };
                println!("  - {name} [{status}{required}{auth}] {cmd_str}");
            }
            Ok(())
        }
        McpCommand::Connect { server } => {
            let mut pool = McpPool::from_config_path_with_workspace(&config_path, workspace)?;
            if let Some(name) = server {
                pool.get_or_connect(&name).await?;
                println!("Connected to MCP server: {name}");
            } else {
                let errors = pool.connect_all().await;
                if errors.is_empty() {
                    println!("Connected to all configured MCP servers.");
                } else {
                    for (name, err) in errors {
                        eprintln!("Failed to connect {name}: {err:#}");
                    }
                }
            }
            Ok(())
        }
        McpCommand::Tools { server } => {
            let mut pool = McpPool::from_config_path_with_workspace(&config_path, workspace)?;
            if let Some(name) = server {
                let conn = pool.get_or_connect(&name).await?;
                if conn.tools().is_empty() {
                    println!("No tools found for MCP server: {name}");
                } else {
                    println!("Tools for {name}:");
                    for tool in conn.tools() {
                        println!(
                            "  - {}{}",
                            tool.name,
                            tool.description
                                .as_ref()
                                .map_or(String::new(), |d| format!(": {d}"))
                        );
                    }
                }
            } else {
                let _ = pool.connect_all().await;
                let tools = pool.all_tools();
                if tools.is_empty() {
                    println!("No MCP tools discovered.");
                } else {
                    println!("MCP tools:");
                    for (name, tool) in tools {
                        println!(
                            "  - {}{}",
                            name,
                            tool.description
                                .as_ref()
                                .map_or(String::new(), |d| format!(": {d}"))
                        );
                    }
                }
            }
            Ok(())
        }
        McpCommand::Add {
            name,
            command,
            url,
            transport,
            bearer_token_env_var,
            oauth_client_id,
            oauth_resource,
            scopes,
            args,
        } => {
            if command.is_none() && url.is_none() {
                bail!("Provide either --command or --url for `mcp add`.");
            }
            if let Some(transport) = transport.as_deref()
                && !transport.trim().eq_ignore_ascii_case("sse")
            {
                bail!("Unsupported MCP transport '{transport}'. Supported values: sse");
            }
            let added_server = McpServerConfig {
                command,
                args,
                env: std::collections::HashMap::new(),
                cwd: None,
                url,
                transport,
                connect_timeout: None,
                execute_timeout: None,
                read_timeout: None,
                disabled: false,
                enabled: true,
                required: false,
                enabled_tools: Vec::new(),
                disabled_tools: Vec::new(),
                headers: std::collections::HashMap::new(),
                env_headers: std::collections::HashMap::new(),
                bearer_token_env_var,
                scopes,
                oauth: oauth_client_id.map(|client_id| McpServerOAuthConfig {
                    client_id: Some(client_id),
                }),
                oauth_resource,
            };
            let can_suggest_oauth = added_server.url.is_some()
                && added_server.bearer_token_env_var.is_none()
                && added_server
                    .headers
                    .keys()
                    .all(|key| !key.trim().eq_ignore_ascii_case("authorization"))
                && added_server
                    .env_headers
                    .keys()
                    .all(|key| !key.trim().eq_ignore_ascii_case("authorization"));
            let mut cfg = load_mcp_config(&config_path)?;
            cfg.servers.insert(name.clone(), added_server.clone());
            save_mcp_config(&config_path, &cfg)?;
            println!("Added MCP server '{name}' in {}", config_path.display());
            if can_suggest_oauth
                && crate::mcp::oauth::oauth_login_support(&added_server)
                    .await
                    .is_ok_and(|support| support.is_some())
            {
                println!(
                    "OAuth is available for '{name}'. Run `mimofan mcp login {name}` to authenticate."
                );
            }
            Ok(())
        }
        McpCommand::Login { name, scopes } => {
            let cfg = crate::mcp::load_config_with_workspace(&config_path, workspace)?;
            let server = cfg
                .servers
                .get(&name)
                .ok_or_else(|| anyhow!("MCP server '{name}' not found"))?;
            let explicit_scopes = (!scopes.is_empty()).then_some(scopes);
            crate::mcp::oauth::perform_oauth_login_for_server(
                &name,
                server,
                explicit_scopes,
                config.mcp_oauth_callback_port,
                config.mcp_oauth_callback_url.as_deref(),
            )
            .await?;
            println!("Stored OAuth credentials for MCP server '{name}'.");
            Ok(())
        }
        McpCommand::Logout { name } => {
            let cfg = crate::mcp::load_config_with_workspace(&config_path, workspace)?;
            let server = cfg
                .servers
                .get(&name)
                .ok_or_else(|| anyhow!("MCP server '{name}' not found"))?;
            if crate::mcp::oauth::delete_oauth_tokens_for_server(&name, server)? {
                println!("Deleted stored OAuth credentials for MCP server '{name}'.");
            } else {
                println!("No stored OAuth credentials found for MCP server '{name}'.");
            }
            Ok(())
        }
        McpCommand::Remove { name } => {
            let mut cfg = load_mcp_config(&config_path)?;
            if cfg.servers.remove(&name).is_none() {
                bail!("MCP server '{name}' not found");
            }
            save_mcp_config(&config_path, &cfg)?;
            println!("Removed MCP server '{name}'");
            Ok(())
        }
        McpCommand::Enable { name } => {
            let mut cfg = load_mcp_config(&config_path)?;
            let server = cfg
                .servers
                .get_mut(&name)
                .ok_or_else(|| anyhow!("MCP server '{name}' not found"))?;
            server.enabled = true;
            server.disabled = false;
            save_mcp_config(&config_path, &cfg)?;
            println!("Enabled MCP server '{name}'");
            Ok(())
        }
        McpCommand::Disable { name } => {
            let mut cfg = load_mcp_config(&config_path)?;
            let server = cfg
                .servers
                .get_mut(&name)
                .ok_or_else(|| anyhow!("MCP server '{name}' not found"))?;
            server.enabled = false;
            server.disabled = true;
            save_mcp_config(&config_path, &cfg)?;
            println!("Disabled MCP server '{name}'");
            Ok(())
        }
        McpCommand::Validate => {
            let mut pool = McpPool::from_config_path_with_workspace(&config_path, workspace)?;
            let errors = pool.connect_all().await;
            if errors.is_empty() {
                println!("MCP config is valid. All enabled servers connected.");
                return Ok(());
            }
            eprintln!("MCP validation failed:");
            for (name, err) in errors {
                eprintln!("  - {name}: {err:#}");
            }
            bail!("one or more MCP servers failed validation");
        }
        McpCommand::AddSelf { name, workspace } => {
            let exe_path = std::env::current_exe()
                .map_err(|e| anyhow!("Cannot resolve current binary path: {e}"))?;
            let exe_str = exe_path.to_string_lossy().to_string();

            let mut args = vec!["serve".to_string(), "--mcp".to_string()];
            if let Some(ref ws) = workspace {
                args.push("--workspace".to_string());
                args.push(ws.clone());
            }

            let mut cfg = load_mcp_config(&config_path)?;
            if cfg.servers.contains_key(&name) {
                bail!(
                    "MCP server '{name}' already exists in {}. Use `mimofan mcp remove {name}` first, or choose a different --name.",
                    config_path.display()
                );
            }
            cfg.servers.insert(
                name.clone(),
                McpServerConfig {
                    command: Some(exe_str.clone()),
                    args,
                    env: std::collections::HashMap::new(),
                    cwd: None,
                    url: None,
                    transport: None,
                    connect_timeout: None,
                    execute_timeout: None,
                    read_timeout: None,
                    disabled: false,
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
            save_mcp_config(&config_path, &cfg)?;
            println!(
                "Registered DeepSeek as MCP server '{name}' in {}",
                config_path.display()
            );
            println!("  command: {exe_str}");
            println!(
                "  args:    serve --mcp{}",
                workspace.map_or(String::new(), |ws| format!(" --workspace {ws}"))
            );
            println!();
            println!("Tip: Use `mimo mcp validate` to test the connection.");
            println!("     Use `mimo serve --http` for the HTTP/SSE runtime API instead.");
            Ok(())
        }
    }
}

pub(crate) fn load_mcp_config(path: &Path) -> Result<McpConfig> {
    if !path.exists() {
        return Ok(McpConfig::default());
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read MCP config {}: {}", path.display(), e))?;
    let cfg: McpConfig = serde_json::from_str(&contents)
        .map_err(|e| anyhow::anyhow!("Failed to parse MCP config: {e}"))?;
    Ok(cfg)
}

pub(crate) fn save_mcp_config(path: &Path, cfg: &McpConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create MCP config directory {}", parent.display())
        })?;
    }
    let rendered = serde_json::to_string_pretty(cfg)
        .map_err(|e| anyhow!("Failed to serialize MCP config: {e}"))?;
    crate::utils::write_atomic(path, rendered.as_bytes())
        .map_err(|e| anyhow!("Failed to write MCP config {}: {}", path.display(), e))?;
    Ok(())
}
