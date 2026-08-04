//! `/plugins` slash command — list and inspect script plugin tools.

use std::path::PathBuf;

use crate::commands::CommandResult;
use crate::config::Config;
use crate::localization::{MessageId, tr};
use crate::tools::plugin::scan_plugin_dir;
use crate::tui::app::App;

/// List discovered plugins, or show details for a named plugin.
pub fn plugins(app: &mut App, arg: Option<&str>) -> CommandResult {
    let Some(plugin_dir) = plugin_dir_for(app) else {
        return CommandResult::error(
            "Could not resolve plugin directory. Set [tools].plugin_dir in config.toml or ensure ~/.mimofan/tools exists.".to_string(),
        );
    };

    if !plugin_dir.exists() {
        return CommandResult::message(format!(
            "No plugin directory found at {}",
            plugin_dir.display()
        ));
    }

    let discovered = scan_plugin_dir(&plugin_dir);

    if let Some(name) = arg.map(str::trim).filter(|s| !s.is_empty()) {
        show_plugin_detail(app, name, &discovered)
    } else {
        list_plugins(app, &plugin_dir, &discovered)
    }
}

fn list_plugins(
    app: &App,
    plugin_dir: &std::path::Path,
    discovered: &[(PathBuf, crate::tools::plugin::PluginMetadata)],
) -> CommandResult {
    if discovered.is_empty() {
        return CommandResult::message(
            tr(app.ui_locale, MessageId::CmdPluginNoneFound)
                .replace("{dir}", &plugin_dir.display().to_string()),
        );
    }

    let mut out = String::new();
    out.push_str(
        &tr(app.ui_locale, MessageId::CmdPluginListHeader)
            .replace("{count}", &discovered.len().to_string()),
    );
    out.push('\n');

    for (path, meta) in discovered {
        out.push_str(&format!(
            "• {} — {}\n  {}",
            meta.name,
            meta.description,
            path.display()
        ));
        out.push('\n');
    }

    CommandResult::message(out)
}

fn show_plugin_detail(
    app: &App,
    name: &str,
    discovered: &[(PathBuf, crate::tools::plugin::PluginMetadata)],
) -> CommandResult {
    let Some((path, meta)) = discovered.iter().find(|(_, m)| m.name == name) else {
        return CommandResult::error(
            tr(app.ui_locale, MessageId::CmdPluginNotFound).replace("{name}", name),
        );
    };

    let schema = serde_json::to_string_pretty(&meta.input_schema).unwrap_or_default();
    let approval = approval_label(meta.approval);

    let mut out = String::new();
    out.push_str(&format!("{}\n", meta.name));
    out.push_str(&format!("{:=<40}\n", ""));
    out.push_str(&format!(
        "{}\n",
        tr(app.ui_locale, MessageId::CmdPluginDetailDescription)
            .replace("{description}", &meta.description)
    ));
    out.push_str(&format!(
        "{}\n",
        tr(app.ui_locale, MessageId::CmdPluginDetailSchema).replace("{schema}", &schema)
    ));
    out.push_str(&format!(
        "{}\n",
        tr(app.ui_locale, MessageId::CmdPluginDetailApproval).replace("{approval}", approval)
    ));
    out.push_str(&format!(
        "{}\n",
        tr(app.ui_locale, MessageId::CmdPluginDetailPath)
            .replace("{path}", &path.display().to_string())
    ));

    CommandResult::message(out)
}

fn approval_label(approval: crate::tools::spec::ApprovalRequirement) -> &'static str {
    match approval {
        crate::tools::spec::ApprovalRequirement::Auto => "auto",
        crate::tools::spec::ApprovalRequirement::Suggest => "suggest",
        crate::tools::spec::ApprovalRequirement::Required => "required",
    }
}

/// Resolve the configured plugin directory, defaulting to `~/.mimofan/tools`.
fn plugin_dir_for(app: &App) -> Option<PathBuf> {
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
        .map(PathBuf::from)
        .or_else(default_mimofan_tools_dir)
}

fn default_mimofan_tools_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".mimofan").join("tools"))
}

/// Test a local plugin script by parsing its frontmatter and executing it with test JSON.
pub fn plugin_test(_app: &mut App, args: Option<&str>) -> CommandResult {
    let raw = args.unwrap_or("").trim();
    if raw.is_empty() {
        return CommandResult::error("Usage: /plugin-test <script_path> [json_input]");
    }

    let mut parts = raw.splitn(2, char::is_whitespace);
    let script_path_str = parts.next().unwrap_or("").trim();
    let json_input_str = parts.next().unwrap_or("").trim();

    if script_path_str.is_empty() {
        return CommandResult::error("Usage: /plugin-test <script_path> [json_input]");
    }

    let path = std::path::Path::new(script_path_str);
    if !path.exists() {
        return CommandResult::error(format!("Plugin script not found at: {}", script_path_str));
    }

    // Read and parse frontmatter
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return CommandResult::error(format!("Failed to read script file: {e}")),
    };

    let meta = crate::tools::plugin::parse_frontmatter(&content);

    let mut output = String::new();
    output.push_str("=== Plugin Metadata Inspection ===\n");
    output.push_str(&format!("  Name: {}\n", meta.name));
    output.push_str(&format!("  Description: {}\n", meta.description));
    output.push_str(&format!("  Input Schema: {}\n", meta.input_schema));
    output.push_str(&format!("  Approval Level: {:?}\n\n", meta.approval));

    // Parse JSON input
    let json_val = if json_input_str.is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_str::<serde_json::Value>(json_input_str) {
            Ok(v) => v,
            Err(e) => {
                return CommandResult::error(format!(
                    "Invalid JSON input payload: {e} (got: '{json_input_str}')"
                ));
            }
        }
    };

    // Execute plugin script synchronously for debugging
    output.push_str("=== Execution Trial ===\n");

    // Resolve interpreter shebang
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return CommandResult::error(format!("Failed to open script: {e}")),
    };

    // Read shebang prefix
    use std::io::Read;
    let mut buf = vec![0u8; 256];
    let shebang = if let Ok(n) = (&file).take(256).read(&mut buf) {
        let text = String::from_utf8_lossy(&buf[..n]);
        if text.starts_with("#!") {
            let first_line = text.lines().next().unwrap_or("");
            let rest = first_line.strip_prefix("#!").unwrap_or("").trim();
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if !parts.is_empty() {
                let interpreter = parts[0].to_string();
                let shebang_args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
                Some((interpreter, shebang_args))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Extension Fallbacks
    let (interpreter, mut script_args) = if let Some((interp, shebang_args)) = shebang {
        let bin_name = interp.rsplit('/').next().unwrap_or(&interp);
        if bin_name == "env" && !shebang_args.is_empty() {
            (shebang_args[0].clone(), shebang_args[1..].to_vec())
        } else {
            (interp, shebang_args)
        }
    } else {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "py" => ("python3".to_string(), vec![]),
            "js" => ("node".to_string(), vec![]),
            "sh" | "bash" => ("bash".to_string(), vec![]),
            _ => (path.to_string_lossy().to_string(), vec![]),
        }
    };

    let script_path_arg = path.to_string_lossy().to_string();
    if interpreter != script_path_arg {
        script_args.push(script_path_arg);
    }

    let input_bytes = match serde_json::to_vec(&json_val) {
        Ok(b) => b,
        Err(e) => return CommandResult::error(format!("Failed to serialize input: {e}")),
    };

    output.push_str(&format!(
        "  Command: {} {}\n",
        interpreter,
        script_args.join(" ")
    ));
    output.push_str(&format!("  Input: {}\n\n", json_val));

    // Exec process
    let mut child = std::process::Command::new(&interpreter);
    child.args(&script_args);
    child.stdin(std::process::Stdio::piped());
    child.stdout(std::process::Stdio::piped());
    child.stderr(std::process::Stdio::piped());

    let mut spawned = match child.spawn() {
        Ok(s) => s,
        Err(e) => return CommandResult::error(format!("Failed to spawn child: {e}")),
    };

    if let Some(mut stdin) = spawned.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(&input_bytes);
        let _ = stdin.flush();
    }

    let output_res = match spawned.wait_with_output() {
        Ok(o) => o,
        Err(e) => return CommandResult::error(format!("Wait error: {e}")),
    };

    output.push_str("=== Output Result ===\n");
    output.push_str(&format!("  Exit Status: {}\n", output_res.status));

    let stdout_str = String::from_utf8_lossy(&output_res.stdout);
    let stderr_str = String::from_utf8_lossy(&output_res.stderr);

    output.push_str(&format!("  Stdout:\n{}\n", stdout_str.trim()));
    if !stderr_str.is_empty() {
        output.push_str(&format!("  Stderr:\n{}\n", stderr_str.trim()));
    }

    // Validate JSON format
    if output_res.status.success() {
        if serde_json::from_str::<serde_json::Value>(&stdout_str).is_ok() {
            output.push_str("\n✅ ToolResult validation succeeded (valid JSON format).\n");
        } else {
            output.push_str("\n⚠️  Output is not a valid JSON structure (will be treated as raw content string fallback).\n");
        }
    } else {
        output.push_str("\n❌ Execution failed (non-zero exit code).\n");
    }

    CommandResult::message(output)
}
