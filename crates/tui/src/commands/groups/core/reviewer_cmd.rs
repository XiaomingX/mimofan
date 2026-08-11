//! `/reviewer` command — 独立评审者（#752，对标 open-discovery 的
//! Scientific Reviewer）。
//!
//! 核心思想：执行者与评审者职责分离，评审者依据**可核验证据**（不是 agent 自述）
//! 下结论。评审者只读不写：它读本次 initiative 的 claim / 证据，调用
//! `crate::reviewer` 逻辑层的 `review()` / `accepted_only()` 给出
//! Accepted / Rejected / Weak 判定，作为 `/artifact` 公开章节的前置门。
//!
//! 本命令是编排层：进入 Agent 模式，把 `crate::reviewer` 逻辑层的 API 交给模型
//! 按工作流调用。纯逻辑与编排解耦，便于在 CI 中单测验收。

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, AppMode};

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "reviewer",
    aliases: &["reviewer", "review"],
    usage: "/reviewer [<initiative_id>]",
    description_id: MessageId::CmdReviewerDescription,
};

pub(in crate::commands) struct ReviewerCmd;

impl RegisterCommand for ReviewerCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        reviewer(app, arg)
    }
}

/// 派发一个 `/reviewer [<initiative_id>]` 调用：进入 Agent 模式做独立审核。
fn reviewer(app: &mut App, arg: Option<&str>) -> CommandResult {
    let initiative_id = arg
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("current")
        .to_string();

    // 评审者在 Agent 模式下运行：模型读取 claim / 证据，调用 crate::reviewer
    // 逻辑层判定。评审者只读不写。
    app.set_mode(AppMode::Agent);

    let prompt = format!(
        "你正在以独立评审者（Scientific Reviewer）身份审核一次研究 initiative（/reviewer）。\n\n\
         initiative_id：{id}\n\n\
         职责分离原则：**你只审核，不修改任何代码或产物**。执行者已产生若干 claim，\
         你依据可核验证据下结论，绝不凭自述放行。\n\n\
         工作流：\n\
         1. 读取本次 initiative 的候选 claim（来自 evolve 候选 / 会话结论），每条形如\n\
            crate::reviewer::ClaimForReview {{ title, strength, has_repro_steps, contradicted }}。\n\
         2. 证据强度分级：\n\
            - Strong：有外部 evaluator 通过 / 测试通过（如 #751 的 EvaluatorOutput.is_winner）。\n\
            - Medium：有复现步骤但未自动验证。\n\
            - Weak：仅自述。\n\
         3. 对每条 claim 调用 crate::reviewer::review(&claim) 判定：\n\
            - 被反驳（contradicted=true）→ 无论强度直接 Rejected。\n\
            - Strong 且未反驳 → Accepted。\n\
            - Medium 且有复现步骤且未反驳 → Accepted；否则 Weak。\n\
            - Weak → Weak（不进入公开章节）。\n\
         4. 调用 crate::reviewer::accepted_only(&claims) 过滤出可进入公开产物的 claim。\n\
         5. 输出审核报告：每条 claim 的 verdict（Accepted/Rejected/Weak）+ 理由，\
            并明确列出「仅以下 Accepted claim 可被 /artifact 收录」。\n\n\
         完成后给出简洁总结：审核了多少条、Accepted / Rejected / Weak 各多少。",
        id = initiative_id
    );

    CommandResult::with_message_and_action(
        format!("已进入 Agent 模式，以独立评审者身份审核 initiative \"{initiative_id}\"（只读）"),
        AppAction::SendMessage(prompt),
    )
}
