//! MCP UI action handling and shell job management.

use crate::config::Config;
use crate::tui::mcp_routing::{add_mcp_message, open_mcp_manager_pager};
use crate::tui::shell_job_routing::{
    add_shell_job_message, format_shell_job_list, format_shell_poll, open_shell_job_pager,
};

use super::super::app::{App, McpUiAction, ShellJobAction};

pub(crate) async fn handle_mcp_ui_action(app: &mut App, config: &Config, action: McpUiAction) {
    use crate::mcp::{self, McpWriteStatus};

    let path = app.mcp_config_path.clone();
    let mut changed = false;
    let mut message = None;
    let discover = mcp_ui_action_refreshes_discovery(&action);

    let action_result = match action {
        McpUiAction::Show => Ok(()),
        McpUiAction::Init { force } => {
            changed = true;
            match mcp::init_config(&path, force) {
                Ok(McpWriteStatus::Created) => {
                    message = Some(format!("Created MCP config at {}", path.display()));
                    Ok(())
                }
                Ok(McpWriteStatus::Overwritten) => {
                    message = Some(format!("Overwrote MCP config at {}", path.display()));
                    Ok(())
                }
                Ok(McpWriteStatus::SkippedExists) => {
                    changed = false;
                    message = Some(format!(
                        "MCP config already exists at {} (use /mcp init --force to overwrite)",
                        path.display()
                    ));
                    Ok(())
                }
                Err(err) => Err(err),
            }
        }
        McpUiAction::AddStdio {
            name,
            command,
            args,
        } => {
            changed = true;
            mcp::add_server_config(&path, name.clone(), Some(command), None, args, None)
                .map(|()| message = Some(format!("Added MCP stdio server '{name}'")))
        }
        McpUiAction::AddHttp {
            name,
            url,
            transport,
        } => {
            changed = true;
            mcp::add_server_config(&path, name.clone(), None, Some(url), Vec::new(), transport)
                .map(|()| message = Some(format!("Added MCP HTTP/SSE server '{name}'")))
        }
        McpUiAction::Enable { name } => {
            changed = true;
            mcp::set_server_enabled(&path, &name, true)
                .map(|()| message = Some(format!("Enabled MCP server '{name}'")))
        }
        McpUiAction::Disable { name } => {
            changed = true;
            mcp::set_server_enabled(&path, &name, false)
                .map(|()| message = Some(format!("Disabled MCP server '{name}'")))
        }
        McpUiAction::Remove { name } => {
            changed = true;
            mcp::remove_server_config(&path, &name)
                .map(|()| message = Some(format!("Removed MCP server '{name}'")))
        }
        McpUiAction::Login { name, scopes } => {
            let result = async {
                let cfg = mcp::load_config_with_workspace(&path, &app.workspace)?;
                let server = cfg
                    .servers
                    .get(&name)
                    .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' not found"))?;
                let explicit_scopes = (!scopes.is_empty()).then_some(scopes);
                mcp::oauth::perform_oauth_login_for_server(
                    &name,
                    server,
                    explicit_scopes,
                    config.mcp_oauth_callback_port,
                    config.mcp_oauth_callback_url.as_deref(),
                )
                .await
            }
            .await;
            result.map(|()| {
                message = Some(format!(
                    "Stored OAuth credentials for MCP server '{name}'. Restart if the server was already connected."
                ));
            })
        }
        McpUiAction::Logout { name } => {
            let result = (|| {
                let cfg = mcp::load_config_with_workspace(&path, &app.workspace)?;
                let server = cfg
                    .servers
                    .get(&name)
                    .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' not found"))?;
                mcp::oauth::delete_oauth_tokens_for_server(&name, server)
            })();
            result.map(|deleted| {
                message = Some(if deleted {
                    format!("Deleted stored OAuth credentials for MCP server '{name}'.")
                } else {
                    format!("No stored OAuth credentials found for MCP server '{name}'.")
                });
            })
        }
        McpUiAction::Validate | McpUiAction::Reload => Ok(()),
    };

    if let Err(err) = action_result {
        add_mcp_message(app, format!("MCP action failed: {err}"));
        return;
    }

    if changed {
        app.mcp_restart_required = true;
    }
    if let Some(message) = message {
        add_mcp_message(app, message);
    }

    let snapshot_result = if discover {
        let network_policy = config.network.clone().map(|toml_cfg| {
            crate::network_policy::NetworkPolicyDecider::with_default_audit(toml_cfg.into_runtime())
        });
        mcp::discover_manager_snapshot_with_workspace(
            &path,
            &app.workspace,
            network_policy,
            app.mcp_restart_required,
        )
        .await
    } else {
        mcp::manager_snapshot_from_config_with_workspace(
            &path,
            &app.workspace,
            app.mcp_restart_required,
        )
    };

    match snapshot_result {
        Ok(snapshot) => {
            if discover {
                add_mcp_message(
                    app,
                    "MCP discovery refreshed for the UI. Restart the TUI after config edits to rebuild the model-visible MCP tool pool.".to_string(),
                );
            }
            // Keep the boot-time MCP-count chip in sync with the live
            // snapshot so footers and panels reflect post-/mcp edits
            // (#502).
            app.mcp_configured_count = snapshot.servers.len();
            app.mcp_snapshot = Some(snapshot.clone());
            open_mcp_manager_pager(app, &snapshot);
        }
        Err(err) => add_mcp_message(app, format!("MCP snapshot failed: {err}")),
    }
}

fn mcp_ui_action_refreshes_discovery(action: &McpUiAction) -> bool {
    matches!(
        action,
        McpUiAction::Show
            | McpUiAction::Validate
            | McpUiAction::Reload
            | McpUiAction::Login { .. }
            | McpUiAction::Logout { .. }
    )
}

pub(crate) fn handle_shell_job_action(app: &mut App, action: ShellJobAction) {
    let Some(shell_manager) = app.runtime_services.shell_manager.clone() else {
        add_shell_job_message(app, "Command center is not attached.".to_string());
        return;
    };

    let mut manager = match shell_manager.lock() {
        Ok(manager) => manager,
        Err(_) => {
            add_shell_job_message(app, "Command center lock is poisoned.".to_string());
            return;
        }
    };

    match action {
        ShellJobAction::List => {
            let jobs = manager.list_jobs();
            add_shell_job_message(app, format_shell_job_list(&jobs));
        }
        ShellJobAction::Show { id } => match manager.inspect_job(&id) {
            Ok(detail) => open_shell_job_pager(app, &detail),
            Err(err) => add_shell_job_message(app, format!("Command lookup failed: {err}")),
        },
        ShellJobAction::Poll { id, wait } => {
            match manager.poll_delta(&id, wait, if wait { 5_000 } else { 1_000 }) {
                Ok(delta) => add_shell_job_message(app, format_shell_poll(&delta.result)),
                Err(err) => add_shell_job_message(app, format!("Command poll failed: {err}")),
            }
        }
        ShellJobAction::SendStdin { id, input, close } => {
            match manager.write_stdin(&id, &input, close) {
                Ok(()) => match manager.poll_delta(&id, false, 1_000) {
                    Ok(delta) => add_shell_job_message(app, format_shell_poll(&delta.result)),
                    Err(err) => {
                        add_shell_job_message(
                            app,
                            format!("Command input sent; poll failed: {err}"),
                        );
                    }
                },
                Err(err) => add_shell_job_message(app, format!("Command input failed: {err}")),
            }
        }
        ShellJobAction::Cancel { id } => match manager.kill(&id) {
            Ok(result) => add_shell_job_message(app, format_shell_poll(&result)),
            Err(err) => add_shell_job_message(app, format!("Command cancel failed: {err}")),
        },
        ShellJobAction::CancelAll => match manager.kill_running() {
            Ok(results) => {
                let count = results.len();
                if count == 0 {
                    add_shell_job_message(app, "No running commands to cancel.".to_string());
                } else {
                    let tasks: Vec<String> = results
                        .iter()
                        .filter_map(|result| result.task_id.clone())
                        .collect();
                    add_shell_job_message(
                        app,
                        format!("Canceled {count} command(s): {}", tasks.join(", ")),
                    );
                }
            }
            Err(err) => add_shell_job_message(app, format!("Command cancel-all failed: {err}")),
        },
    }
}
