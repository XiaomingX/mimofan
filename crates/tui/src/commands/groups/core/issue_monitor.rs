//! /monitor 命令：管理 Issue → PR 受监控工作流
//!
//! 用法：
//!   /monitor create <name> --issues 1,2,3 --repo owner/repo --branch main
//!   /monitor list
//!   /monitor status <id>
//!   /monitor pause <id>
//!   /monitor resume <id>
//!   /monitor delete <id>

use std::path::PathBuf;

use chrono::Utc;

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::issue_monitor::{IssueMonitor, MonitorStatus, MonitorStore, RouteConstraint};
use crate::localization::MessageId;
use crate::tui::app::App;

use super::CommandResult;

/// Directory holding persisted monitors. Sits alongside the memory store so
/// monitors survive restarts in the same per-workspace state tree.
fn monitor_dir(app: &App) -> PathBuf {
    app.memory_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| app.memory_dir.clone())
        .join("issue_monitors")
}

/// Bridge a sync slash-command handler into the async persistence layer.
/// We are on the TUI thread of a multi-threaded runtime, so `block_in_place`
/// keeps the runtime healthy while we block (mirrors the pattern in
/// `commands/groups/skills/skills.rs` and `groups/memory/vmemory.rs`).
fn run_async<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

/// Resolve a user-supplied monitor reference to a stored monitor. Accepts a
/// full UUID or any unambiguous prefix, since the generated ids are far too
/// long to retype from the `list` output.
async fn resolve(store: &MonitorStore, needle: &str) -> Result<IssueMonitor, String> {
    if let Ok(found) = store.load(needle).await {
        return Ok(found);
    }

    let all = store
        .load_all()
        .await
        .map_err(|e| format!("读取监控列表失败: {e}"))?;
    let mut matches: Vec<IssueMonitor> = all
        .into_iter()
        .filter(|m| m.id.starts_with(needle) || m.name == needle)
        .collect();

    match matches.len() {
        0 => Err(format!("未找到 monitor: {needle}")),
        1 => Ok(matches.remove(0)),
        n => Err(format!(
            "{needle} 匹配到 {n} 个 monitor，请使用更长的 id 前缀"
        )),
    }
}

fn status_label(status: MonitorStatus) -> &'static str {
    match status {
        MonitorStatus::Active => "active",
        MonitorStatus::Paused => "paused",
        MonitorStatus::Completed => "completed",
    }
}

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

fn handle_create(app: &mut App, args: &[&str]) -> CommandResult {
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

    let now = Utc::now();
    let monitor = IssueMonitor {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        issue_numbers: issues.clone(),
        repo: repo.clone(),
        target_branch: branch.clone(),
        status: MonitorStatus::Active,
        constraints: vec![
            RouteConstraint::Issues(issues.clone()),
            RouteConstraint::TargetBranch(branch.clone()),
        ],
        created_at: now,
        updated_at: now,
        last_wake_at: None,
        pr_number: None,
        wake_count: 0,
    };

    let store = MonitorStore::new(monitor_dir(app));
    if let Err(e) = run_async(store.save(&monitor)) {
        return CommandResult::message(format!("创建 monitor 失败: {e}"));
    }

    let mut text = "✅ Issue Monitor 已创建\n\n".to_string();
    text.push_str(&format!("**ID**: {}\n", monitor.id));
    text.push_str(&format!("**名称**: {}\n", monitor.name));
    text.push_str(&format!("**仓库**: {}\n", repo));
    text.push_str(&format!("**目标分支**: {}\n", branch));
    text.push_str(&format!(
        "**监控 Issues**: {}\n",
        issues
            .iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    text.push_str("\n使用 `/monitor list` 查看全部，`/monitor status <id>` 查看详情。");

    CommandResult::message(text)
}

fn handle_list(app: &mut App) -> CommandResult {
    let store = MonitorStore::new(monitor_dir(app));
    let mut monitors = match run_async(store.load_all()) {
        Ok(m) => m,
        Err(e) => return CommandResult::message(format!("读取监控列表失败: {e}")),
    };

    if monitors.is_empty() {
        return CommandResult::message(
            "暂无 Issue Monitor。使用 `/monitor create <name> --issues 1,2,3` 创建。".to_string(),
        );
    }

    monitors.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let mut text = format!("## Issue Monitors ({})\n\n", monitors.len());
    for m in &monitors {
        text.push_str(&format!(
            "- `{}` **{}** [{}] {} → {}\n",
            &m.id[..8.min(m.id.len())],
            m.name,
            status_label(m.status),
            m.issue_numbers
                .iter()
                .map(|n| format!("#{n}"))
                .collect::<Vec<_>>()
                .join(","),
            m.target_branch,
        ));
    }

    CommandResult::message(text)
}

fn handle_status(app: &mut App, args: &[&str]) -> CommandResult {
    if args.is_empty() {
        return CommandResult::message("用法: /monitor status <id>".to_string());
    }

    let store = MonitorStore::new(monitor_dir(app));
    let monitor = match run_async(resolve(&store, args[0])) {
        Ok(m) => m,
        Err(e) => return CommandResult::message(e),
    };

    let mut text = format!("## Monitor: {}\n\n", monitor.name);
    text.push_str(&format!("**ID**: {}\n", monitor.id));
    text.push_str(&format!("**状态**: {}\n", status_label(monitor.status)));
    text.push_str(&format!("**仓库**: {}\n", monitor.repo));
    text.push_str(&format!("**目标分支**: {}\n", monitor.target_branch));
    text.push_str(&format!(
        "**监控 Issues**: {}\n",
        monitor
            .issue_numbers
            .iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if let Some(pr) = monitor.pr_number {
        text.push_str(&format!("**关联 PR**: #{pr}\n"));
    }
    text.push_str(&format!("**唤醒次数**: {}\n", monitor.wake_count));
    if let Some(last) = monitor.last_wake_at {
        text.push_str(&format!(
            "**最后唤醒**: {}\n",
            last.format("%Y-%m-%d %H:%M:%S UTC")
        ));
    }
    text.push_str(&format!(
        "**创建于**: {}\n",
        monitor.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));

    CommandResult::message(text)
}

fn handle_pause(app: &mut App, args: &[&str]) -> CommandResult {
    set_status(app, args, MonitorStatus::Paused, "pause")
}

fn handle_resume(app: &mut App, args: &[&str]) -> CommandResult {
    set_status(app, args, MonitorStatus::Active, "resume")
}

fn set_status(
    app: &mut App,
    args: &[&str],
    status: MonitorStatus,
    verb: &str,
) -> CommandResult {
    if args.is_empty() {
        return CommandResult::message(format!("用法: /monitor {verb} <id>"));
    }

    let store = MonitorStore::new(monitor_dir(app));
    let result: Result<String, String> = run_async(async {
        let mut monitor = resolve(&store, args[0]).await?;
        if monitor.status == status {
            return Ok(format!(
                "Monitor `{}` 已处于 {} 状态",
                monitor.name,
                status_label(status)
            ));
        }
        monitor.status = status;
        monitor.updated_at = Utc::now();
        store
            .save(&monitor)
            .await
            .map_err(|e| format!("保存 monitor 失败: {e}"))?;
        Ok(format!(
            "✅ Monitor `{}` 状态已更新为 {}",
            monitor.name,
            status_label(status)
        ))
    });

    match result {
        Ok(msg) | Err(msg) => CommandResult::message(msg),
    }
}

fn handle_delete(app: &mut App, args: &[&str]) -> CommandResult {
    if args.is_empty() {
        return CommandResult::message("用法: /monitor delete <id>".to_string());
    }

    let store = MonitorStore::new(monitor_dir(app));
    let result: Result<String, String> = run_async(async {
        let monitor = resolve(&store, args[0]).await?;
        store
            .delete(&monitor.id)
            .await
            .map_err(|e| format!("删除 monitor 失败: {e}"))?;
        Ok(format!("✅ 已删除 Monitor `{}`", monitor.name))
    });

    match result {
        Ok(msg) | Err(msg) => CommandResult::message(msg),
    }
}
