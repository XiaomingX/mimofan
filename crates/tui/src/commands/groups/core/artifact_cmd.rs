//! `/artifact` command — 研究成果物汇总（#750，对标 open-discovery 的
//! Repository Artifact Builder）。
//!
//! 把一次 initiative（goal_loop / evolve 运行）的产物汇总为单一可复现目录：
//! 含 BRIEF 原文、可运行 setup、代表性正/负结果、provenance。
//! 默认只生成本地目录，不自动发布；`--publish` 走 #753 的研究副作用闸门，
//! 默认需显式授权（Auto 不自动推 remote）。
//!
//! 本命令是编排层：进入 Agent 模式，把 `crate::research_artifact` 逻辑层的
//! API（`ArtifactInput::build`）交给模型按工作流调用。纯逻辑与编排解耦。

use std::path::PathBuf;

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::research_ethics;
use crate::tui::app::{App, AppAction, AppMode};

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "artifact",
    aliases: &["artifact", "publish"],
    usage: "/artifact <initiative_id> [--publish]",
    description_id: MessageId::CmdArtifactDescription,
};

pub(in crate::commands) struct ArtifactCmd;

impl RegisterCommand for ArtifactCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        artifact(app, arg)
    }
}

/// 派发一个 `/artifact <initiative_id> [--publish]` 调用。
fn artifact(app: &mut App, arg: Option<&str>) -> CommandResult {
    let raw = match arg.map(str::trim).filter(|s| !s.is_empty()) {
        Some(a) => a,
        None => {
            return CommandResult::error(
                "Usage: /artifact <initiative_id> [--publish]\n\n\
                 研究成果物汇总：把 BRIEF / 结果 / provenance 汇总到 \
                 initiatives/<initiative_id>/。\n\
                 例如：/artifact paper-x-method-y\n\
                 加 --publish 会把目录初始化为 git 仓库并尝试推 remote（需授权）。",
            );
        }
    };

    // 解析 <initiative_id> 与可选 --publish 开关。
    let mut initiative_id: Option<String> = None;
    let mut publish = false;
    for tok in raw.split_whitespace() {
        match tok {
            "--publish" | "-p" => publish = true,
            other => {
                if initiative_id.is_none() {
                    initiative_id = Some(other.to_string());
                }
            }
        }
    }

    let initiative_id = match initiative_id {
        Some(id) => id,
        None => {
            return CommandResult::error(
                "Usage: /artifact <initiative_id> [--publish]\n\n\
                 缺少 initiative_id。先运行 /repro 或 /evolve 建立 initiative，\
                 再以其 id 汇总产物。",
            );
        }
    };

    // --publish 触发研究副作用闸门（#753）：PublishRemote 默认需显式授权，
    // Auto 模式下不自动推 remote。
    if publish && research_ethics::requires_explicit_authorization("--publish") {
        let (_policy, advice) = research_ethics::advice_for("--publish");
        return CommandResult::error(format!(
            "发布到远程仓库（--publish）被研究副作用闸门拦截。\n{advice}\n\
             （本地汇总不受影响：去掉 --publish 即可先生成 initiatives/ 目录。）",
        ));
    }

    // 汇总在 Agent 模式下运行：模型读取本次 initiative 的 BRIEF / 结果 / provenance，
    // 调用 crate::research_artifact::ArtifactInput::build 落地目录。
    app.set_mode(AppMode::Agent);

    let publish_section = if publish {
        "\n\
         你同时还被授权执行 --publish：汇总完成后，把 initiatives/{id} 初始化为 \
         git 仓库、创建 GitHub remote 并推送。**这一步是真实的外部副作用**，确认用户 \
         确实希望发布后再执行；若用户未明确授权则只保留本地目录。\n"
    } else {
        "\n默认只生成本地目录（不发布）；如需发布，带 --publish 重试（需授权）。\n"
    };

    let prompt = format!(
        "你正在汇总一次研究成果物（/artifact）。\n\n\
         initiative_id：{id}\n\n\
         工作流（严格遵循，evaluator / reviewer 拥有正确性，你只做汇总）：\n\
         1. 读取本次 initiative 的 BRIEF（BRIEF.md 或 crate::repro 留痕）作为 \
            ArtifactInput.brief。\n\
         2. 收集代表性结果：正结果（如 evaluator 通过的候选、测试通过）与负结果 \
            （落败/被反驳的候选），填入 ArtifactInput.results。\n\
         3. 收集已通过 #752 reviewer 审核、verdict=Accepted 的 claim，按 \
            crate::research_artifact::Claim 结构（title/body/strength/evidence）填入 \
            ArtifactInput.claims。**只收录 Accepted 的 claim**，未通过的不得进入公开章节。\n\
         4. 收集复现步骤（setup 命令，纯文本行）填入 ArtifactInput.setup_steps。\n\
         5. 收集 crate::repro 的 provenance 留痕填入 ArtifactInput.provenance。\n\
         6. 调用 crate::research_artifact::ArtifactInput::build(root, \"{id}\") 汇总到 \
            <root>/initiatives/{id}/，写 README.md 与 provenance.json。\n\
         7. 完成后给出简洁总结：产物目录路径、收录了多少条 Accepted claim、正负结果数。\
        {publish}\n\
         注意：provenance 与 claim 必须可核验（指向 evaluator 输出 / 测试名），\
         评审者会据此复核，禁止凭自述收录。",
        id = initiative_id,
        publish = publish_section
    );

    let msg = if publish {
        format!("已进入 Agent 模式，汇总并（授权后）发布 initiative \"{initiative_id}\"")
    } else {
        format!("已进入 Agent 模式，汇总 initiative \"{initiative_id}\"（本地，不发布）")
    };

    CommandResult::with_message_and_action(msg, AppAction::SendMessage(prompt))
}

/// 当前工作目录作为 initiative 根（与 /repro 对齐）。
#[allow(dead_code)]
fn current_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
