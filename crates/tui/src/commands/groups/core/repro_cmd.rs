//! `/repro` command — 可复现性纪律（#754，对标 open-discovery 可复现性范式）。
//!
//! 把研究 initiative 的意图固化为 `BRIEF.md` 单一事实源，并采集环境快照
//! （rust/python 版本 + 依赖锁哈希）。默认只落盘证据、不触发任何行为变更；
//! provenance 留痕由 goal_loop / evolve 在运行时按需累积。

use std::path::PathBuf;

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::repro;
use crate::tui::app::App;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "repro",
    aliases: &["reproducibility", "brief"],
    usage: "/repro <brief>",
    description_id: MessageId::CmdReproDescription,
};

pub(in crate::commands) struct ReproCmd;

impl RegisterCommand for ReproCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        repro(app, arg)
    }
}

/// 派发一个 `/repro <brief>` 调用：写 BRIEF.md + 环境快照，零行为变更。
fn repro(_app: &mut App, arg: Option<&str>) -> CommandResult {
    let text = match arg.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => t,
        None => {
            return CommandResult::error(
                "Usage: /repro <brief>\n\n\
                 可复现性纪律：把研究意图固化为 BRIEF.md 单一事实源，并采集环境快照。\n\
                 例如：/repro 复现 paper-X 的 method-Y 并对比 baseline",
            );
        }
    };

    // 以当前工作目录为 initiative 根；纯落盘，不影响运行中的会话。
    let root: PathBuf = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let brief = repro::Brief::new(text, "cli");
    let brief_path = match repro::write_brief(&root, &brief) {
        Ok(p) => p,
        Err(e) => {
            return CommandResult::error(format!("写入 BRIEF.md 失败：{e}"));
        }
    };

    // 环境快照：尽力而为，任何步骤失败字段为 None，不整体失败。
    let dep_locks: Vec<PathBuf> = ["Cargo.lock"]
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();
    let snap = repro::snapshot_env(&dep_locks);
    let snap_path = match repro::write_env_snapshot(&root, &snap) {
        Ok(p) => p,
        Err(e) => {
            return CommandResult::error(format!("写入环境快照失败：{e}"));
        }
    };

    // 触发一次 provenance 起点留痕：本命令本身作为 turn-0。
    let _ = repro::record_provenance(
        &root,
        &repro::ProvenanceRecord {
            turn_id: "turn-0".into(),
            note: format!("repro 命令初始化 BRIEF：{}", brief.id),
            ..Default::default()
        },
    );

    CommandResult::message(format!(
        "已固化可复现性证据：\n  - {}\n  - {}\n后续 provenance 留痕由运行中的回路按需累积。",
        brief_path.display(),
        snap_path.display(),
    ))
}
