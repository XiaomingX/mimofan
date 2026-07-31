# 仓库智能体指南

## 当前工作位置（首先阅读此节）

- **仓库**：`XiaomingX/mimofan`。此仓库存在于多设备，因此不要在此硬编码设备特定的检出路径——在你拥有的本地检出中工作，并且始终在编辑前**通过 `git branch --show-current` 确认**。
- **活动分支**：从实时事实开始，而非硬编码通道。从最新交接/目标文件和 `git branch --show-current` 确认当前修复/集成分支；最近的工作通过小 PR 在 `main` 上登陆，而非长期存在的 `codex/...` 集成分支，因此不要假设命名的集成分支仍然存在——在依赖前验证。
- **工作区版本**：从 `Cargo.toml` 读取（`[workspace.package] version`）；它随发布通道推进，因此不要相信记在此处的数字。不要机会性地提升版本；版本提升、标签、发布工件、发布和 GitHub Release 需要 Hunter 的明确批准。
- **里程碑路标**：使用活动交接中命名的当前 GitHub 发布里程碑并实时列出，例如 `gh issue list --repo XiaomingX/mimofan --milestone "<current milestone>" --state open`。
- **默认分支是 `main`**。永远不要直接提交到 `main`；在活动集成分支或新的 `codex/...` 分支/工作树上进行隔离更改。仅当工作单元可审查时才向 `main` 开 PR。
- **推送更改前始终运行**：`cargo fmt`，然后针对该区域的定向测试（`cargo test -p mimofan --bin mimofan-tui --locked <filter>`、`cargo test -p mimofan-config`、`cargo test -p mimofan-protocol`……）。完整门禁：`cargo test --workspace`。发布构建：`cargo build --release -p mimofan`。
- **已知套件问题（预先存在，非回归）**：`config_command_allow_shell_*` 在 `~/.mimofan/settings.toml` 设置 `default_mode = "yolo"` 的机器上失败（测试不是隔离的）；`run_verifiers_background_*` 在完整套件并行时不稳定但单独通过。不要将这些视为你的更改引起的。

## 持续智能体工作约定

- 每次提交只关注一个问题；写真实的提交信息。不要合并不相关的更改。
- 除非你实际验证了行为（构建了二进制、运行了测试、复现了修复），否则提交为 **WIP**。没有证据声称"已修复"比诚实的 WIP 更糟。
- 不要重新引入已移除的机制：面向模型的子智能体表面是 **`agent` 仅**（无 `agent_open`/`agent_eval`/`agent_close`/`delegate_to_agent`/等等）；无容量/一致性/运行时标签系统；无生命周期工具；无运行时提示词/标签注入。`constitution.md` 是唯一的基础提示词。
- 可配置的子智能体深度保持不变。除非明确需要并解释清楚，否则不要新增任意限制。
- 子智能体 **TUI 冻结在较早的交接中报告**已由 v0.8.61 切换解决（cap-20、persist-debounce、AgentProgress 重绘节流、ListSubAgents 合并、input-pump-off-render-thread）。领先的"阻塞 I/O 饥饿工作池"理论已被测量和**证伪**（`git rev-parse` ~10ms，18 核机器）。不要提交推测性的 `spawn_blocking` 修复冻结。

## Mimofan 管理

- 将社区贡献者视为合作伙伴。善意的 PR、问题报告、复现、日志、审查和验证评论是维护者证据，而非队列噪音。
- 保持门禁温暖和干运行，除非 Hunter 明确批准执行。门禁内容应清晰尊重地指导贡献者。
- 为每个实质性影响修复的收割 PR、问题报告或评论保留信誉。尽可能保留作者身份；否则使用 `.github/AUTHOR_MAP` 中可映射的 GitHub noreply `Co-authored-by` 尾部。
- 没有 Hunter 批准，不要打标签、发布、创建 GitHub Release 或推送发布工件。
- 保持 Mimofan 品牌同时保持对 DeepSeek 模型/服务商的一等支持。退役遗留 `mimofan` 名称绝不能读作弃用 DeepSeek 模型或服务商支持。
- 从代码、测试、链接的问题、评论和检查结果审查 PR。永远不要仅从标题或标签合并、关闭、收割或推迟社区工作。
- 尊重树中的并发工作。不要恢复或重写其他人或智能体的不相关编辑。

## 发布 PR 集成

- 当 triage 拥挤的发布队列时使用暂存集成分支。像 `scratch/v0.8.59-pr-train-YYYYMMDD` 这样的分支可以批量合并或挑选许多 PR 头以快速暴露冲突、缺失测试、重复工作和隐藏耦合。
- 将暂存分支视为证据，而非要发布的工件。不要从暂存列车打标签、发布或快进发布分支。将安全解析的块或提交以窄的、可审查的提交收割回发布分支。
- 仅当 PR 对实际登陆分支干净、检查可接受且不跨越信任边界表面时才优先直接 GitHub 合并。对 `main` 干净的 PR 仍可能与发布分支冲突；在称为合并就绪之前针对实际发布头测试可合并性。
- 对于已批准的 PR，从对发布分支的暂存合并开始，然后在直接合并、带冲突解决的挑选或署名收割之间决定。维护者批准是强优先级信号，而非跳过审查或测试的许可。
- 收割时，保留或添加机器可读信誉：尽可能保留原始作者，使用 `.github/AUTHOR_MAP` 或 GitHub 数字 noreply 身份添加 `Co-authored-by`，并在提交体中包含 `Harvested from PR #N by @handle`，以便自动关闭工作流在到达 `main` 后关闭 PR 并保留信誉。合并带此行的 PR 时使用 rebase 或合并提交，永远不要 squash：squash 可能重写体、丢弃 `Harvested from PR` 行并静默丢失机器可读信誉和自动关闭。
- 永远不要添加机器人/工具 `Co-authored-by` 尾部（Claude、codex、cursor、`noreply@anthropic.com`）：`scripts/check-coauthor-trailers.py` 在收割提交上拒绝它们——贡献者尾部是给人类的。还要刷新不会从尾部自动填充的手动信誉表面：`docs/CONTRIBUTORS.md` 和 `CHANGELOG.md`。
- 在验证登陆提交在相关分支上之后才关闭或更新问题和 PR。如果发布分支已包含等效行为，留下清晰的注释链接提交并描述任何剩余差异。
- 对于活动发布队列，从活动交接中命名的当前 GitHub 发布里程碑开始（`gh issue list --repo XiaomingX/mimofan --milestone "<current milestone>"`）并在行动前刷新状态。`docs/` 下较旧的按版本 triage 文档仅作为历史参考。
