//! /monitor 命令：管理 Issue → PR 受监控工作流
//!
//! 用法：
//!   /monitor create <name> --issues 1,2,3 --repo owner/repo --branch main
//!   /monitor list
//!   /monitor status <id>
//!   /monitor pause <id>
//!   /monitor resume <id>
//!   /monitor delete <id>

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::issue_monitor::RouteConstraint;
use crate::localization::MessageId;
use crate::tui::app::App;

use super::CommandResult;

pub(in crate::commands) const MONITOR_INFO: CommandInfo = CommandInfo {
    name: "monitor",
    aliases: &[],
    usage: "/monitor <create|list|status|pause|resume|delete> [args]",
    description_id: MessageId::CmdMonitorDescription,
};

pub(in crate::commands) struct MonitorCmd;

impl RegisterCommand for MonitorCmd {
    fn info() -> &'static CommandInfo {
        &MONITOR_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        let args = arg.unwrap_or("");
        let parts: Vec<&str> = args.split_whitespace().collect();

        if parts.is_empty() {
            return CommandResult::message(
                "用法: /monitor <create|list|status|pause|resume|delete> [args]".to_string(),
            );
        }

        let subcommand = parts[0];

        match subcommand {
            "create" => handle_create(app, &parts[1..]),
            "list" => handle_list(app),
            "status" => handle_status(app, &parts[1..]),
            "pause" => handle_pause(app, &parts[1..]),
            "resume" => handle_resume(app, &parts[1..]),
            "delete" => handle_delete(app, &parts[1..]),
            _ => CommandResult::message(format!("未知子命令: {}", subcommand)),
        }
    }
}

fn handle_create(_app: &mut App, args: &[&str]) -> CommandResult {
    // 解析参数
    let mut name = None;
    let mut issues = Vec::new();
    let mut repo = None;
    let mut branch = None;

    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--issues" => {
                i += 1;
                if i < args.len() {
                    issues = args[i]
                        .split(',')
                        .filter_map(|s| s.trim().parse::<u64>().ok())
                        .collect();
                }
            }
            "--repo" => {
                i += 1;
                if i < args.len() {
                    repo = Some(args[i].to_string());
                }
            }
            "--branch" => {
                i += 1;
                if i < args.len() {
                    branch = Some(args[i].to_string());
                }
            }
            _ => {
                if name.is_none() {
                    name = Some(args[i].to_string());
                }
            }
        }
        i += 1;
    }

    let name = match name {
        Some(n) => n,
        None => return CommandResult::message("请提供监控名称".to_string()),
    };

    if issues.is_empty() {
        return CommandResult::message("请通过 --issues 指定要监控的 issue 编号".to_string());
    }

    let repo = repo.unwrap_or_else(|| {
        // 尝试从 git remote 获取
        std::process::Command::new("git")
            .args(["remote", "get-url", "origin"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout).ok().and_then(|url| {
                    let url = url.trim();
                    if url.contains("github.com") {
                        let parts: Vec<&str> = url.split('/').collect();
                        if parts.len() >= 2 {
                            let owner = parts[parts.len() - 2];
                            let repo_name = parts[parts.len() - 1].trim_end_matches(".git");
                            return Some(format!("{}/{}", owner, repo_name));
                        }
                    }
                    None
                })
            })
            .unwrap_or_else(|| "unknown/unknown".to_string())
    });

    let branch = branch.unwrap_or_else(|| {
        std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            })
            .unwrap_or_else(|| "main".to_string())
    });

    // 创建路由约束
    let _constraints = vec![RouteConstraint::Issues(issues.clone())];

    // 注意：这里需要异步调用，但 RegisterCommand 是同步的
    // 实际实现中需要通过 AppAction 来触发异步操作
    // 这里先返回一个提示信息

    let mut text = format!("✅ Issue Monitor 创建请求已接收\n\n");
    text.push_str(&format!("**名称**: {}\n", name));
    text.push_str(&format!("**仓库**: {}\n", repo));
    text.push_str(&format!("**目标分支**: {}\n", branch));
    text.push_str(&format!("**监控 Issues**: {:?}\n", issues));
    text.push_str("\n*注意：持久化功能需要异步支持，将在后续版本完善*");

    CommandResult::message(text)
}

fn handle_list(_app: &mut App) -> CommandResult {
    // 暂时返回提示信息
    CommandResult::message("Issue Monitor 列表功能需要异步支持，将在后续版本完善".to_string())
}

fn handle_status(_app: &mut App, args: &[&str]) -> CommandResult {
    if args.is_empty() {
        return CommandResult::message("用法: /monitor status <id>".to_string());
    }
    CommandResult::message(format!("Monitor {} 状态查询功能需要异步支持", args[0]))
}

fn handle_pause(_app: &mut App, args: &[&str]) -> CommandResult {
    if args.is_empty() {
        return CommandResult::message("用法: /monitor pause <id>".to_string());
    }
    CommandResult::message(format!("Monitor {} 暂停功能需要异步支持", args[0]))
}

fn handle_resume(_app: &mut App, args: &[&str]) -> CommandResult {
    if args.is_empty() {
        return CommandResult::message("用法: /monitor resume <id>".to_string());
    }
    CommandResult::message(format!("Monitor {} 恢复功能需要异步支持", args[0]))
}

fn handle_delete(_app: &mut App, args: &[&str]) -> CommandResult {
    if args.is_empty() {
        return CommandResult::message("用法: /monitor delete <id>".to_string());
    }
    CommandResult::message(format!("Monitor {} 删除功能需要异步支持", args[0]))
}
