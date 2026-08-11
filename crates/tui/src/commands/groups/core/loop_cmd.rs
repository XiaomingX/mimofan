//! `/loop` command — autonomous prompt repetition (Claude Code `/loop` parity).
//!
//! Reuses the existing `/goal` continuation pipeline (`goal_loop` +
//! `GoalState` + engine `goal_continuation_message_if_needed`): a `/loop` is a
//! goal whose stop condition is a user-written natural-language predicate and
//! whose round cap is explicit. The model self-judges the stop condition each
//! round (injected into the continuation prompt) and ends the loop by calling
//! `update_goal` with `status: "complete"`.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, HuntVerdict};
use crate::tools::goal::{GoalStatus, LoopConfig};
use crate::automation_manager::CreateAutomationRequest;

use super::CommandResult;

pub(in crate::commands) const LOOP_COMMAND_INFO: CommandInfo = CommandInfo {
    name: "loop",
    aliases: &["xunhuan"],
    usage: "/loop <prompt> [--until <condition>] [--max N] [--checkpoint] [--schedule HH:MM]",
    description_id: MessageId::CmdLoopDescription,
};

pub(in crate::commands) struct LoopCmd;

impl RegisterCommand for LoopCmd {
    fn info() -> &'static CommandInfo {
        &LOOP_COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        loop_cmd(app, arg)
    }
}

/// Dispatch a `/loop` invocation.
fn loop_cmd(app: &mut App, arg: Option<&str>) -> CommandResult {
    let arg = arg.unwrap_or("").trim();

    // Sub-commands reuse the `/goal` control surface (shared verdict mapping).
    match arg {
        "clear" | "reset" => return clear_loop(app),
        "done" | "complete" | "stop" => return close_loop(app, HuntVerdict::Hunted, GoalStatus::Complete),
        "pause" | "paused" => return close_loop(app, HuntVerdict::Wounded, GoalStatus::Paused),
        "resume" | "continue" => return resume_loop(app),
        "block" | "blocked" => return close_loop(app, HuntVerdict::Escaped, GoalStatus::Blocked),
        "" | "status" | "info" => return show_loop_status(app),
        _ => {}
    }

    // Parse the main form: /loop <prompt> [flags]
    let parsed = match parse_loop_args(arg) {
        Ok(p) => p,
        Err(err) => return CommandResult::error(err),
    };

    if parsed.prompt.is_empty() {
        return CommandResult::error(loop_usage());
    }

    // Stage UI-side loop state (engine truth lives in `GoalState`, written via
    // `SetGoalStatus{loop_config}` below).
    app.hunt.quarry = Some(parsed.prompt.clone());
    app.hunt.token_budget = None;
    app.hunt.tokens_used = 0;
    app.hunt.time_used_seconds = 0;
    app.hunt.continuation_count = 0;
    app.hunt.started_at = Some(std::time::Instant::now());
    app.hunt.verdict = HuntVerdict::Hunting;
    app.hunt.stop_condition = parsed.stop_condition.clone();
    app.hunt.max_rounds = parsed.max_rounds;
    app.hunt.checkpoint_each_round = parsed.checkpoint;
    app.hunt.is_loop = true;

    let loop_config = LoopConfig {
        stop_condition: parsed.stop_condition.clone(),
        max_rounds: parsed.max_rounds,
        checkpoint_each_round: parsed.checkpoint,
    };

    // `/loop --schedule HH:MM`: register a daily automation that enqueues the
    // loop prompt at the requested wall-clock time. The scheduler respects the
    // same recurring-engine path as `/night`; the loop itself starts when the
    // automation fires and the user resumes it (or it runs unattended with the
    // configured auto-approve policy).
    if let Some((hour, minute)) = parsed.schedule {
        let result = schedule_loop(app, &parsed.prompt, hour, minute);
        return match result {
            Ok(text) => CommandResult::message(text),
            Err(e) => CommandResult::error(format!("调度循环任务失败: {e}")),
        };
    }

    let mut detail = String::new();
    if let Some(cond) = &parsed.stop_condition {
        detail.push_str(&format!(" | stop: \"{cond}\""));
    }
    if let Some(max) = parsed.max_rounds {
        detail.push_str(&format!(" | max {max} rounds"));
    }
    if parsed.checkpoint {
        detail.push_str(" | checkpoint each round");
    }

    CommandResult::with_message_and_action(
        format!("Loop started: \"{}\"{detail} - repeating until the stop condition is met.", parsed.prompt),
        AppAction::SetGoalStatus {
            status: GoalStatus::Active,
            clear: false,
            loop_config: Some(loop_config),
        },
    )
}

/// Parsed `/loop` arguments.
struct ParsedLoop {
    prompt: String,
    stop_condition: Option<String>,
    max_rounds: Option<u32>,
    checkpoint: bool,
    schedule: Option<(u32, u32)>,
}

/// Parse `/loop` arguments of the form:
/// `<prompt> [--until <cond>] [--max N] [--checkpoint] [--schedule HH:MM]`
fn parse_loop_args(arg: &str) -> Result<ParsedLoop, String> {
    let mut prompt_parts: Vec<&str> = Vec::new();
    let mut stop_condition: Option<String> = None;
    let mut max_rounds: Option<u32> = None;
    let mut checkpoint = false;
    let mut schedule: Option<(u32, u32)> = None;

    let mut iter = arg.split_whitespace().peekable();
    while let Some(tok) = iter.next() {
        match tok {
            "--until" => {
                let next = iter.next().ok_or("Missing stop condition after --until")?;
                if let Some(stripped) = next.strip_prefix('"') {
                    // Quoted multi-word condition: collect until the closing quote.
                    let mut cond = stripped.to_string();
                    if let Some(end) = cond.find('"') {
                        cond.truncate(end);
                    } else {
                        for t in iter.by_ref() {
                            if let Some(end) = t.find('"') {
                                cond.push(' ');
                                cond.push_str(&t[..end]);
                                break;
                            } else {
                                cond.push(' ');
                                cond.push_str(t);
                            }
                        }
                    }
                    stop_condition = Some(cond);
                } else {
                    stop_condition = Some(next.to_string());
                }
            }
            "--max" => {
                let n = iter.next().ok_or("Missing number after --max")?;
                let n: u32 = n.parse().map_err(|_| format!("Invalid --max value: {n}"))?;
                max_rounds = Some(n);
            }
            "--checkpoint" => checkpoint = true,
            "--schedule" => {
                let t = iter.next().ok_or("Missing time after --schedule")?;
                schedule = Some(parse_hh_mm(t)?);
            }
            other => prompt_parts.push(other),
        }
    }

    let prompt = prompt_parts.join(" ").trim().to_string();
    Ok(ParsedLoop {
        prompt,
        stop_condition,
        max_rounds,
        checkpoint,
        schedule,
    })
}

/// Parse `HH:MM` into `(hour, minute)`.
fn parse_hh_mm(t: &str) -> Result<(u32, u32), String> {
    let parts: Vec<&str> = t.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err("Invalid time format. Use HH:MM (e.g., 00:30)".to_string());
    }
    let hour = parts[0]
        .parse::<u32>()
        .map_err(|_| "Invalid hour".to_string())?;
    let minute = parts[1]
        .parse::<u32>()
        .map_err(|_| "Invalid minute".to_string())?;
    if hour > 23 || minute > 59 {
        return Err("Time out of range. Use HH:MM (00-23 : 00-59)".to_string());
    }
    Ok((hour, minute))
}

/// Register a `/loop --schedule` prompt as a daily automation.
///
/// Reuses the durable `AutomationManager` (same backing store as `/night` and
/// `/time`) so the scheduler enqueues the loop prompt at the requested
/// wall-clock time. Returns a confirmation message with the automation id.
fn schedule_loop(app: &mut App, prompt: &str, hour: u32, minute: u32) -> Result<String, String> {
    let automations = app
        .runtime_services
        .automations
        .clone()
        .ok_or_else(|| {
            "Automation manager is not available in this session. Restart the TUI to enable scheduled loops.".to_string()
        })?;

    let rrule = format!("FREQ=DAILY;BYHOUR={hour};BYMINUTE={minute}");
    let record = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let guard = automations.lock().await;
            guard
                .create_automation(CreateAutomationRequest {
                    name: format!("loop:{}", truncate_preview(prompt, 40)),
                    prompt: prompt.to_string(),
                    rrule,
                    cwds: vec![app.workspace.clone()],
                    mode: None,
                    allow_shell: None,
                    trust_mode: None,
                    auto_approve: None,
                    status: None,
                })
                .map_err(|e| e.to_string())
        })
    })?;

    let mut text = "✅ 已创建每日循环调度\n\n".to_string();
    text.push_str(&format!("**ID**: {}\n", record.id));
    text.push_str(&format!("**时间**: 每天 {}:{:02}\n", hour, minute));
    text.push_str(&format!("**提示**: {}\n", truncate_preview(prompt, 80)));
    text.push_str("\n使用 `/time list` 查看全部，`/time cancel <id>` 取消。");
    Ok(text)
}

fn clear_loop(app: &mut App) -> CommandResult {
    app.hunt.quarry = None;
    app.hunt.token_budget = None;
    app.hunt.tokens_used = 0;
    app.hunt.time_used_seconds = 0;
    app.hunt.continuation_count = 0;
    app.hunt.started_at = None;
    app.hunt.verdict = HuntVerdict::default();
    app.hunt.stop_condition = None;
    app.hunt.max_rounds = None;
    app.hunt.checkpoint_each_round = false;
    app.hunt.is_loop = false;
    CommandResult::with_message_and_action(
        "Loop cleared.",
        AppAction::SetGoalStatus {
            status: GoalStatus::Active,
            clear: true,
            loop_config: None,
        },
    )
}

fn close_loop(app: &mut App, verdict: HuntVerdict, status: GoalStatus) -> CommandResult {
    if app.hunt.quarry.as_deref().is_none_or(str::is_empty) {
        return CommandResult::error("No loop set. Use /loop <prompt> first.");
    }
    app.hunt.verdict = verdict;
    let label = match verdict {
        HuntVerdict::Hunted => "Loop complete.",
        HuntVerdict::Wounded => "Loop paused. Use /loop resume to continue.",
        HuntVerdict::Escaped => "Loop blocked.",
        HuntVerdict::Hunting => "Loop resumed.",
    };
    CommandResult::with_message_and_action(
        label,
        AppAction::SetGoalStatus {
            status,
            clear: false,
            loop_config: None,
        },
    )
}

fn resume_loop(app: &mut App) -> CommandResult {
    let Some(objective) = app.hunt.quarry.as_deref().map(str::trim).filter(|o| !o.is_empty()) else {
        return CommandResult::error("No paused loop set. Use /loop <prompt> first.");
    };
    app.hunt.verdict = HuntVerdict::Hunting;
    if app.hunt.started_at.is_none() {
        app.hunt.started_at = Some(std::time::Instant::now());
    }
    CommandResult::with_message_and_action(
        "Loop resumed.",
        AppAction::SendMessage(objective.to_string()),
    )
}

fn show_loop_status(app: &mut App) -> CommandResult {
    let kind = if app.hunt.is_loop { "Loop" } else { "Goal" };
    if let Some(ref obj) = app.hunt.quarry {
        let verdict_label = match app.hunt.verdict {
            HuntVerdict::Hunting => "[ACTIVE]",
            HuntVerdict::Hunted => "[COMPLETE]",
            HuntVerdict::Wounded => "[PAUSED]",
            HuntVerdict::Escaped => "[BLOCKED]",
        };
        let stop = app
            .hunt
            .stop_condition
            .as_ref()
            .map(|c| format!(" | stop: \"{c}\""))
            .unwrap_or_default();
        let max = app
            .hunt
            .max_rounds
            .map(|m| format!(" | max {m}"))
            .unwrap_or_default();
        let ckpt = if app.hunt.checkpoint_each_round {
            " | checkpoint: on"
        } else {
            ""
        };
        CommandResult::message(format!(
            "{kind} {verdict_label}: \"{obj}\"{stop}{max}{ckpt} | continuations: {}",
            app.hunt.continuation_count
        ))
    } else {
        CommandResult::message(loop_usage())
    }
}

fn loop_usage() -> &'static str {
    "No loop set. Use /loop <prompt> [--until <condition>] [--max N] [--checkpoint] [--schedule HH:MM].\n\
     /loop status - show loop status\n\
     /loop pause - pause the loop\n\
     /loop resume - resume the loop\n\
     /loop stop - stop (mark complete)\n\
     /loop clear - remove the loop."
}

/// Truncate text to a maximum length with ellipsis.
fn truncate_preview(text: &str, max_chars: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_until_max_checkpoint() {
        let p = parse_loop_args("optimize foo --until \"all tests pass\" --max 5 --checkpoint").unwrap();
        assert_eq!(p.prompt, "optimize foo");
        assert_eq!(p.stop_condition.as_deref(), Some("all tests pass"));
        assert_eq!(p.max_rounds, Some(5));
        assert!(p.checkpoint);
        assert!(p.schedule.is_none());
    }

    #[test]
    fn parses_unquoted_until_and_schedule() {
        let p = parse_loop_args("run checks --until stable --schedule 03:30").unwrap();
        assert_eq!(p.prompt, "run checks");
        assert_eq!(p.stop_condition.as_deref(), Some("stable"));
        assert_eq!(p.schedule, Some((3, 30)));
    }

    #[test]
    fn rejects_bad_max() {
        assert!(parse_loop_args("do thing --max abc").is_err());
    }

    #[test]
    fn rejects_bad_schedule() {
        assert!(parse_loop_args("do thing --schedule 99:99").is_err());
    }

    #[test]
    fn quoted_until_with_spaces_kept_intact() {
        // A quoted multi-word condition is captured whole and does not leak
        // into the prompt.
        let p = parse_loop_args("refactor module --until \"the build is green\" --max 3").unwrap();
        assert_eq!(p.prompt, "refactor module");
        assert_eq!(p.stop_condition.as_deref(), Some("the build is green"));
        assert_eq!(p.max_rounds, Some(3));
    }

    #[test]
    fn quoted_until_split_across_tokens() {
        // Whitespace inside the quotes is preserved even when it spans tokens.
        let p = parse_loop_args("work --until \"all cargo test pass\"").unwrap();
        assert_eq!(p.stop_condition.as_deref(), Some("all cargo test pass"));
        assert_eq!(p.prompt, "work");
    }
}
