//! `/tools` command.

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::config::Config;
use crate::localization::MessageId;
use crate::tools::{ToolContext, ToolRegistryBuilder};
use crate::tui::app::App;
use crate::worker_profile::ShellPolicy;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "tools",
    aliases: &["tool-inspect"],
    usage: "/tools [name]",
    description_id: MessageId::CmdToolsInspectDescription,
};

pub(in crate::commands) struct ToolsInspectCmd;

impl RegisterCommand for ToolsInspectCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        tools_inspect(app, arg)
    }
}

pub fn tools_inspect(app: &mut App, args: Option<&str>) -> CommandResult {
    let target = args.unwrap_or("").trim();

    // 1. Build native & plugin tool registry
    let shell_policy = ShellPolicy::from_legacy_allow_shell(app.allow_shell);
    let builder = ToolRegistryBuilder::new()
        .with_agent_tools_policy(shell_policy)
        .with_user_input_tool();

    let tool_context = ToolContext::new(app.workspace.clone());
    let mut registry = builder.build(tool_context);

    // Load plugins if path resolved
    let plugin_dir = resolve_plugin_dir(app);
    if let Some(ref dir) = plugin_dir {
        registry.load_plugins(dir);
    }

    if target.is_empty() {
        // List all tools
        let mut output = String::new();
        output.push_str("=== Available Native and Plugin Tools ===\n");
        let mut native_names = registry.names();
        native_names.sort();
        for name in native_names {
            if let Some(tool) = registry.get(name) {
                output.push_str(&format!("• {} — {}\n", name, tool.description()));
            }
        }

        // List MCP tools from snapshot if connected
        if let Some(snapshot) = &app.mcp_snapshot {
            output.push_str("\n=== Connected MCP Tools ===\n");
            let mut count = 0;
            for server in &snapshot.servers {
                if server.connected && !server.tools.is_empty() {
                    output.push_str(&format!("[MCP Server: {}]\n", server.name));
                    for tool in &server.tools {
                        output.push_str(&format!(
                            "  • {} — {}\n",
                            tool.model_name,
                            tool.description.as_deref().unwrap_or("No description")
                        ));
                        count += 1;
                    }
                }
            }
            if count == 0 {
                output.push_str("No active MCP tools loaded.\n");
            }
        } else {
            output.push_str(
                "\n=== MCP Tools ===\nUse /mcp status to verify connected MCP servers.\n",
            );
        }

        CommandResult::message(output)
    } else {
        // Detailed info of a specific tool
        // Check native/plugins first
        if let Some(tool) = registry.get(target) {
            let schema = serde_json::to_string_pretty(&tool.input_schema()).unwrap_or_default();
            let mut output = String::new();
            output.push_str(&format!("Tool: {}\n", tool.name()));
            output.push_str("========================================\n");
            output.push_str(&format!("Description: {}\n", tool.description()));
            output.push_str(&format!(
                "Approval Requirement: {:?}\n",
                tool.approval_requirement()
            ));
            output.push_str(&format!("Capabilities: {:?}\n", tool.capabilities()));
            output.push_str(&format!("Input Schema:\n{}\n", schema));
            return CommandResult::message(output);
        }

        // Check MCP tools in snapshot
        if let Some(snapshot) = &app.mcp_snapshot {
            for server in &snapshot.servers {
                for tool in &server.tools {
                    if tool.name == target || tool.model_name == target {
                        let mut output = String::new();
                        output.push_str(&format!("MCP Tool: {}\n", tool.model_name));
                        output.push_str("========================================\n");
                        output.push_str(&format!("MCP Server: {}\n", server.name));
                        output.push_str(&format!(
                            "Description: {}\n",
                            tool.description.as_deref().unwrap_or("No description")
                        ));
                        return CommandResult::message(output);
                    }
                }
            }
        }

        CommandResult::error(format!(
            "Tool '{}' not found in active registry or MCP snapshots.",
            target
        ))
    }
}

fn resolve_plugin_dir(app: &App) -> Option<std::path::PathBuf> {
    let config = match &app.config_path {
        Some(path) => {
            Config::load(Some(path.clone()), app.config_profile.as_deref()).unwrap_or_default()
        }
        None => Config::default(),
    };
    config
        .tools
        .as_ref()
        .and_then(|tools| tools.plugin_dir.as_ref())
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".mimofan").join("tools")))
}
