//! `/night` and `/time` commands for scheduled task execution.

use crate::automation_manager::{AutomationManager, AutomationStatus, CreateAutomationRequest};
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::{MessageId, tr};
use crate::tui::app::App;

use super::CommandResult;

/// Run an async automation-manager operation on the TUI thread without
/// blocking the runtime. Mirrors the `block_in_place` pattern used by
/// `commands/groups/core/issue_monitor.rs` and other sync slash-command
/// handlers that need to touch the durable automation store.
fn with_automations<F, T>(app: &App, op: F) -> Result<T, String>
where
    F: FnOnce(&AutomationManager) -> anyhow::Result<T>,
{
    let automations = app
        .runtime_services
        .automations
        .clone()
        .ok_or_else(|| {
            "Automation manager is not available in this session. Restart the TUI to enable scheduled tasks.".to_string()
        })?;
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let guard = automations.lock().await;
            op(&guard).map_err(|e| e.to_string())
        })
    })
}

/// Resolve an automation id from a user-supplied reference (full id or any
/// unambiguous prefix), since generated ids are far too long to retype.
fn resolve_automation_id(app: &App, needle: &str) -> Result<String, String> {
    let automations = app
        .runtime_services
        .automations
        .clone()
        .ok_or_else(|| "Automation manager is not available in this session.".to_string())?;
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let guard = automations.lock().await;
            if guard.get_automation(needle).is_ok() {
                return Ok(needle.to_string());
            }
            let all = guard.list_automations().map_err(|e| e.to_string())?;
            let matches: Vec<String> = all
                .into_iter()
                .filter(|a| a.id.starts_with(needle) || a.name == needle)
                .map(|a| a.id)
                .collect();
            match matches.len() {
                1 => Ok(matches.into_iter().next().unwrap()),
                0 => Err(format!("未找到 automation: {needle}")),
                n => Err(format!(
                    "{needle} 匹配到 {n} 个 automation，请使用更长的 id 前缀"
                )),
            }
        })
    })
}

pub(in crate::commands) const NIGHT_COMMAND_INFO: CommandInfo = CommandInfo {
    name: "night",
    aliases: &["yejian"],
    usage: "/night <prompt> --schedule <HH:MM>",
    description_id: MessageId::CmdNightDescription,
};

pub(in crate::commands) const TIME_COMMAND_INFO: CommandInfo = CommandInfo {
    name: "time",
    aliases: &["dingshi"],
    usage: "/time [list|cancel <id>]",
    description_id: MessageId::CmdTimeDescription,
};

pub(in crate::commands) struct NightCmd;

impl RegisterCommand for NightCmd {
    fn info() -> &'static CommandInfo {
        &NIGHT_COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        night(app, arg)
    }
}

pub(in crate::commands) struct TimeCmd;

impl RegisterCommand for TimeCmd {
    fn info() -> &'static CommandInfo {
        &TIME_COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        time(app, arg)
    }
}

/// Queue a prompt for execution at a specific time (night mode).
///
/// Usage: `/night <prompt> --schedule <HH:MM>`
///
/// This command queues a message to be sent at the specified time,
/// allowing users to take advantage of off-peak API hours.
fn night(app: &mut App, arg: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;
    let arg = arg.unwrap_or("").trim();

    if arg.is_empty() {
        return CommandResult::error(tr(locale, MessageId::CmdNightUsage));
    }

    // Parse the schedule time from the argument
    let (prompt, schedule_time) = match parse_night_args(arg) {
        Ok(result) => result,
        Err(err) => return CommandResult::error(err),
    };

    // Validate the schedule time
    let hour = schedule_time.0;
    let minute = schedule_time.1;
    if hour > 23 || minute > 59 {
        return CommandResult::error(tr(locale, MessageId::CmdNightInvalidTime));
    }

    // Schedule the prompt as a daily automation (FREQ=DAILY;BYHOUR;BYMINUTE):
    // the background scheduler enqueues it as a normal background task at the
    // requested wall-clock time every day. The confirmation surfaces the
    // generated automation id so `/time list` / `/time cancel <id>` can
    // manage it afterwards.
    let rrule = format!("FREQ=DAILY;BYHOUR={hour};BYMINUTE={minute}");
    let result = with_automations(app, |mgr| {
        mgr.create_automation(CreateAutomationRequest {
            name: format!("night:{}", truncate_preview(&prompt, 40)),
            prompt: prompt.clone(),
            rrule: rrule.clone(),
            cwds: vec![app.workspace.clone()],
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: None,
            status: None,
        })
    });

    match result {
        Ok(record) => {
            let mut text = "✅ 已创建每日定时任务\n\n".to_string();
            text.push_str(&format!("**ID**: {}\n", record.id));
            text.push_str(&format!("**时间**: 每天 {}:{:02}\n", hour, minute));
            text.push_str(&format!("**提示**: {}\n", truncate_preview(&prompt, 80)));
            text.push_str("\n使用 `/time list` 查看全部，`/time cancel <id>` 取消。");
            CommandResult::message(text)
        }
        Err(e) => CommandResult::error(format!("创建定时任务失败: {e}")),
    }
}

/// Manage scheduled tasks.
///
/// Usage: `/time [list|cancel <id>]`
///
/// This command shows scheduled tasks or cancels a specific task.
fn time(app: &mut App, arg: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;
    let arg = arg.unwrap_or("").trim();

    if arg.is_empty() || arg.eq_ignore_ascii_case("list") {
        return list_scheduled_tasks(app);
    }

    let mut parts = arg.split_whitespace();
    let action = parts.next().unwrap_or("").to_lowercase();

    match action.as_str() {
        "cancel" | "drop" | "remove" => cancel_scheduled_task(app, parts.next()),
        _ => CommandResult::error(tr(locale, MessageId::CmdTimeUsage)),
    }
}

/// List all scheduled tasks.
fn list_scheduled_tasks(app: &mut App) -> CommandResult {
    let locale = app.ui_locale;

    let result = with_automations(app, |mgr| mgr.list_automations());
    let automations = match result {
        Ok(list) => list,
        Err(e) => return CommandResult::message(format!("读取定时任务失败: {e}")),
    };

    if automations.is_empty() {
        return CommandResult::message(tr(locale, MessageId::CmdTimeListEmpty));
    }

    let mut text = format!("## 定时任务 ({})\n\n", automations.len());
    for a in &automations {
        text.push_str(&format!(
            "- `{}` **{}** [{}] {} `{}`\n",
            &a.id[..8.min(a.id.len())],
            a.name,
            match a.status {
                AutomationStatus::Active => "active",
                AutomationStatus::Paused => "paused",
            },
            a.rrule,
            a.next_run_at
                .map(|t| t
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string())
                .unwrap_or_else(|| "-".to_string()),
        ));
    }
    CommandResult::message(text)
}

/// Cancel a scheduled task by ID.
fn cancel_scheduled_task(app: &mut App, id: Option<&str>) -> CommandResult {
    let locale = app.ui_locale;

    let Some(raw_id) = id else {
        return CommandResult::error(tr(locale, MessageId::CmdTimeMissingId));
    };

    let resolved = match resolve_automation_id(app, raw_id) {
        Ok(id) => id,
        Err(e) => return CommandResult::message(e),
    };

    let result = with_automations(app, |mgr| mgr.delete_automation(&resolved));
    match result {
        Ok(record) => CommandResult::message(format!(
            "{} 已取消定时任务 `{}`",
            tr(locale, MessageId::CmdTimeCancelled),
            record.name
        )),
        Err(e) => CommandResult::error(format!("取消定时任务失败: {e}")),
    }
}

/// Parse arguments for the `/night` command.
///
/// Expected format: `<prompt> --schedule <HH:MM>`
/// Returns `(prompt, (hour, minute))` or an error message.
pub fn parse_night_args(args: &str) -> Result<(String, (u32, u32)), String> {
    let parts: Vec<&str> = args.splitn(2, "--schedule").collect();
    if parts.len() != 2 {
        return Err("Usage: /night <prompt> --schedule <HH:MM>".to_string());
    }

    let prompt = parts[0].trim().to_string();
    let time_str = parts[1].trim();

    let time_parts: Vec<&str> = time_str.splitn(2, ':').collect();
    if time_parts.len() != 2 {
        return Err("Invalid time format. Use HH:MM (e.g., 00:30)".to_string());
    }

    let hour = time_parts[0]
        .parse::<u32>()
        .map_err(|_| "Invalid hour".to_string())?;
    let minute = time_parts[1]
        .parse::<u32>()
        .map_err(|_| "Invalid minute".to_string())?;

    Ok((prompt, (hour, minute)))
}

/// Truncate text to a maximum length with ellipsis.
pub fn truncate_preview(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(max_chars.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}
