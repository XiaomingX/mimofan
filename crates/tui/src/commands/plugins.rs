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

#[cfg(test)]
mod tests {}
