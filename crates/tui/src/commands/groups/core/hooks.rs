//! `/hooks` slash command — read-only listing of configured
//! lifecycle hooks (#460 MVP).
//!
//! The full picker / persisted enable-disable surface in #460 is
//! still M-sized. This MVP gives the user a no-typing view of what's
//! actually configured in `~/.mimofan/config.toml`'s `[hooks]`
//! table — the most-asked question once hooks start firing.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::hooks::HookEvent;
use crate::localization::MessageId;
use crate::tui::app::App;

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "hooks",
    aliases: &["hook", "gouzi"],
    usage: "/hooks [list|events]",
    description_id: MessageId::CmdHooksDescription,
};

pub(in crate::commands) struct HooksCmd;

impl RegisterCommand for HooksCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        hooks(app, arg)
    }
}

/// Top-level dispatch for `/hooks`. Subcommands:
///
/// * `/hooks`         — same as `/hooks list`.
/// * `/hooks list`    — show every configured hook grouped by event,
///   noting whether the global `[hooks].enabled` flag suppresses
///   them.
/// * `/hooks events`  — list every supported `HookEvent` value the
///   user can target in `[[hooks.hooks]]` entries. Useful for
///   discovery — without this, the only way to learn the event
///   names is to read source.
pub fn hooks(app: &App, arg: Option<&str>) -> CommandResult {
    let sub = arg.map(str::trim).unwrap_or("list").to_ascii_lowercase();
    match sub.as_str() {
        "" | "list" | "ls" | "show" => list(app),
        "events" | "event" | "list-events" => events(),
        other => CommandResult::error(format!(
            "unknown subcommand `{other}`. Try `/hooks list` or `/hooks events`."
        )),
    }
}

fn events() -> CommandResult {
    let mut out = String::new();
    out.push_str(
        "Available hook events (use one of these as `event = \"...\"` in your `[[hooks.hooks]]` entry):\n\n",
    );
    // Order matters — group lifecycle events first, then per-tool,
    // then situational. Stays stable across releases so users can
    // grep on it.
    let ordered = [
        (HookEvent::SessionStart, "fires once when the TUI launches"),
        (HookEvent::SessionEnd, "fires once on graceful shutdown"),
        (
            HookEvent::TurnEnd,
            "fires after a turn completes (observer-only)",
        ),
        (
            HookEvent::MessageSubmit,
            "fires before model dispatch; can transform or block submitted text",
        ),
        (
            HookEvent::ToolCallBefore,
            "fires before each tool call (read-only observer for now)",
        ),
        (
            HookEvent::ToolCallAfter,
            "fires after each tool call (read-only observer for now)",
        ),
        (
            HookEvent::ModeChange,
            "fires on Plan/Agent/Yolo transitions",
        ),
        (
            HookEvent::OnError,
            "fires on transport / capacity / tool errors",
        ),
        (
            HookEvent::SubagentSpawn,
            "fires when a sub-agent starts (observer-only)",
        ),
        (
            HookEvent::SubagentComplete,
            "fires when a sub-agent completes, fails, or is cancelled (observer-only)",
        ),
    ];
    for (event, desc) in ordered {
        out.push_str(&format!("  - `{}` — {desc}\n", event_label(event)));
    }
    CommandResult::message(out.trim_end().to_string())
}

fn list(app: &App) -> CommandResult {
    let config = app.hooks.config();
    if config.hooks.is_empty() {
        return CommandResult::message(
            "No hooks configured. Add a `[[hooks.hooks]]` entry to `~/.mimofan/config.toml` to define one.",
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "{} configured hook(s) (global enabled: {}):\n\n",
        config.hooks.len(),
        if config.enabled {
            "yes"
        } else {
            "no — all hooks suppressed"
        }
    ));

    let mut by_event: std::collections::BTreeMap<&str, Vec<&crate::hooks::Hook>> =
        std::collections::BTreeMap::new();
    for hook in &config.hooks {
        by_event
            .entry(event_label(hook.event))
            .or_default()
            .push(hook);
    }

    for (event, hooks) in by_event {
        out.push_str(&format!("### {event}\n"));
        for hook in hooks {
            let label = hook
                .name
                .as_deref()
                .filter(|n| !n.trim().is_empty())
                .map_or_else(|| "(unnamed)".to_string(), str::to_string);
            let bg = if hook.background { " [bg]" } else { "" };
            let timeout = format!("{}s", hook.timeout_secs);
            let condition = match &hook.condition {
                None | Some(crate::hooks::HookCondition::Always) => String::new(),
                Some(c) => format!(" if {}", condition_summary(c)),
            };
            let cmd_preview = preview_command(&hook.command, 60);
            out.push_str(&format!(
                "  - {label}{bg} (timeout {timeout}){condition}\n      $ {cmd_preview}\n",
            ));
        }
        out.push('\n');
    }

    if !config.enabled {
        out.push_str(
            "Hooks are globally disabled — set `[hooks].enabled = true` in `config.toml` to fire them.\n",
        );
    }

    CommandResult::message(out.trim_end().to_string())
}

fn event_label(event: HookEvent) -> &'static str {
    match event {
        HookEvent::SessionStart => "session_start",
        HookEvent::SessionEnd => "session_end",
        HookEvent::MessageSubmit => "message_submit",
        HookEvent::ToolCallBefore => "tool_call_before",
        HookEvent::ToolCallAfter => "tool_call_after",
        HookEvent::ModeChange => "mode_change",
        HookEvent::OnError => "on_error",
        HookEvent::TurnEnd => "turn_end",
        HookEvent::SubagentSpawn => "subagent_spawn",
        HookEvent::SubagentComplete => "subagent_complete",
        HookEvent::ShellEnv => "shell_env",
    }
}

fn condition_summary(condition: &crate::hooks::HookCondition) -> String {
    match condition {
        crate::hooks::HookCondition::Always => "always".to_string(),
        crate::hooks::HookCondition::ToolName { name } => format!("tool_name=`{name}`"),
        crate::hooks::HookCondition::ToolCategory { category } => {
            format!("tool_category=`{category}`")
        }
        crate::hooks::HookCondition::Mode { mode } => format!("mode=`{mode}`"),
        crate::hooks::HookCondition::ExitCode { code } => format!("exit_code={code}"),
        crate::hooks::HookCondition::All { conditions } => format!(
            "all of [{}]",
            conditions
                .iter()
                .map(condition_summary)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        crate::hooks::HookCondition::Any { conditions } => format!(
            "any of [{}]",
            conditions
                .iter()
                .map(condition_summary)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Single-line preview of the shell command, capped at `max_chars`.
fn preview_command(command: &str, max_chars: usize) -> String {
    let single_line: String = command.chars().filter(|c| *c != '\n').collect();
    if single_line.chars().count() <= max_chars {
        return single_line;
    }
    let mut out: String = single_line
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {}
