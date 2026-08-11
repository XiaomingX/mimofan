//! `/evolve` command — 可机评优化回路（#751，对标 open-discovery/program-evolution）。
//!
//! 用户给出 goal + baseline 程序 + evaluator 脚本；**evaluator 拥有正确性**，
//! agent 只提候选，外部程序裁决每个候选是否「正确且更优」，agent 不自己报分。
//!
//! 本命令是编排层：进入 Agent 模式，把 `crate::evolve` 逻辑层的 API
//! （`lock_baseline` / `run_evaluator_on` / `record_candidate`）交给模型按
//! 工作流调用。纯逻辑与编排解耦，便于在 CI 中单测验收。

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, AppMode};

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "evolve",
    aliases: &["evolve", "optimize"],
    usage: "/evolve <goal>",
    description_id: MessageId::CmdEvolveDescription,
};

pub(in crate::commands) struct EvolveCmd;

impl RegisterCommand for EvolveCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        evolve(app, arg)
    }
}

/// 派发一个 `/evolve <goal>` 调用。
fn evolve(app: &mut App, arg: Option<&str>) -> CommandResult {
    let goal = match arg.map(str::trim).filter(|s| !s.is_empty()) {
        Some(g) => g,
        None => {
            return CommandResult::error(
                "Usage: /evolve <goal>\n\n\
                 可机评优化回路：锁定 baseline + evaluator，由外部程序裁决候选优劣。\n\
                 先给出目标描述，例如：/evolve 降低 tokenizer 编码延迟",
            );
        }
    };

    // evolve 回路在 Agent 模式下运行：模型读 baseline/evaluator、调用
    // crate::evolve 逻辑层 API、迭代候选。
    app.set_mode(AppMode::Agent);

    let prompt = format!(
        "你正在运行一个可机评优化回路（/evolve）。\n\n\
         目标（goal）：{goal}\n\n\
         工作流（严格遵循，evaluator 拥有正确性，你只提候选）：\n\
         1. 先确认本次回路的 baseline 程序路径与 evaluator 脚本路径。\n\
         2. 调用 crate::evolve::lock_baseline(baseline, evaluator, goal, out) 锁定\n\
            baseline（拷贝+哈希+求值，写 lock.json；已存在则拒绝覆盖，不可改 baseline）。\n\
         3. 提出一个候选改动，写到 evolution/candidates/<id>/ 下。\n\
         4. 调用 crate::evolve::run_evaluator_on(evaluator, candidate) 求值；用\n\
            EvaluatorOutput::is_winner() 判定 valid && improved。\n\
         5. 若胜出，调用 crate::evolve::record_candidate(evolution_dir, lineage) 留痕\n\
            （含 parent_id / patch_summary），并把它作为下一轮 parent。\n\
         6. 重复 3–5 直到预算用尽或不再有改进；绝不自己报分，裁决权在 evaluator。\n\n\
         完成后给出简洁总结：best candidate 路径、objective 改进比例、落败原因分布。",
        goal = goal
    );

    CommandResult::with_message_and_action(
        format!("已进入 Agent 模式，启动可机评优化回路：\"{goal}\""),
        AppAction::SendMessage(prompt),
    )
}
