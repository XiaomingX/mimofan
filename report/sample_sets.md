# Code Harness 验收样本集（含具体样本文件）

> 用途：汇总用于验收 code harness / coding agent 的公开评测样本集及其**具体样本路径**，供 `benchmark/` 本地验收框架引用。
> 分类：A 类 = 数据集（自带具体样本实例）→ 本文件；B 类 = 仅仓库、无自有样本文件的评测框架 → 见 `README.md` 附录。
> 说明：样本路径指仓库内目录或 HuggingFace 数据集地址，并非要求克隆全部数据；`benchmark/` 框架可据此按需拉取子集验收。

## A. 自带具体样本文件的数据集

### 1. Terminal-Bench
- **仓库**：https://github.com/laude-institute/terminal-bench
- **样本路径**：仓库内 `tasks/` 目录；每题结构 `tasks/<task>/{instruction.md, task.toml, environment/, tests/, solution/}`。题面/镜像也发布于 HuggingFace：`laude-institute/terminal-bench-tasks`、`laude-institute/terminal-bench-2`（2.0，89 题）。
- **说明**：把 coding agent 放进真实终端/容器环境完成任务，考察其能否编辑文件、跑命令、处理运行时并通过验证脚本拿到 reward。
- **评测维度**：终端交互（文件编辑 / shell / 运行时行为 / 测试执行）、多步任务完成度、隐藏测试脚本打分（reward）。

### 2. SWE-bench
- **仓库**：https://github.com/SWE-bench/SWE-bench
- **样本路径**：实例加载器在 `swebench/harness/`；数据集（HuggingFace）：`princeton-nlp/SWE-bench`（全量）、`princeton-nlp/SWE-bench_Verified`（500 题验证集）、`princeton-nlp/SWE-bench_Lite`。每条实例含 repo + issue + gold patch + FAIL_TO_PASS / PASS_TO_PASS 测试。
- **说明**：真实 GitHub issue → 生成 patch，考察 agent 在真实仓库中改多文件、跑测试、持续纠错的能力。
- **评测维度**：issue→patch 正确性（FAIL_TO_PASS 通过且 PASS_TO_PASS 保持）、长链路代码库理解。

### 3. SWE-Atlas
- **仓库**：https://github.com/scaleapi/SWE-Atlas
- **样本路径**：仓库内 `data/qa/`（Codebase QnA，124 题）、`data/tw/`（Test Writing，90 题）、`data/rf/`（Refactoring，70 题）；共 284 题，来自 18 个开源仓库。
- **说明**：超越“修 issue”的专业软件工程评测，覆盖 Q&A、写测试、重构三类互补能力。
- **评测维度**：代码库理解问答、测试编写（变异测试）、重构（行为保持 + 可维护性 rubric）；程序化检查 + 基于 rubric 的 LLM 评分。

### 4. SWE-Lancer
- **仓库**：https://github.com/openai/SWELancer-Benchmark
- **样本路径**：仓库内 `swelancer_tasks.csv`（全量，约 1.4k 真实自由职业 SWE 任务）、`swelancer_tasks_lite.csv`（174 题，单题 ≥ $1000）。
- **说明**：基于 Expensify 真实自由职业软件工程任务，分 IC SWE（实现）与 SWE Manager（规格/调试决策）两类。
- **评测维度**：端到端实现正确性、经理级规格/调试决策、按任务价值（$）计分的真实产出能力。

### 5. MLE-bench
- **仓库**：https://github.com/openai/mle-bench
- **样本路径**：仓库内 `mlebench/registry/`（75 个 Kaggle 竞赛任务）；数据需 Kaggle 账号下载；精简版 22 题。
- **说明**：考察 agent 在机器学习工程上的能力（数据准备、模型训练、实验执行），模拟真实数据科学竞赛。
- **评测维度**：ML 工程全流程、相对 Kaggle 排行榜的奖牌（铜/银/金）达成率（Any Medal %）。

### 6. DevEval
- **仓库**：https://github.com/zhaospei/DevEval_replication
- **样本路径**：仓库内 `data.jsonl`（1,825 条样本元数据）、`Source_Code/`（115 个真实仓库）、`Dependency_Data/`。
- **说明**：与真实代码仓库对齐的仓库级代码生成基准，由 13 位开发者标注。
- **评测维度**：仓库级代码生成 Pass@k、参考依赖召回 Recall@k；三种上下文设定（无上下文 / 本地文件补全 / 中间填充）。

### 7. EvoEval
- **仓库**：https://github.com/evo-eval/evoeval
- **样本路径**：通过 `evoeval.data.get_evo_eval("EvoEval_difficult")` 加载；7 个基准共 828 题；HuggingFace：`evo-eval/*`。
- **说明**：在 HumanEval 基础上演化出的代码生成评测套件，覆盖多语义维度以降低数据泄漏。
- **评测维度**：代码生成 Pass@1，跨 5 个语义改写（Difficult / Creative / Subtle / Combine / Tool Use）+ 2 个语义保持维度。

### 8. AppWorld
- **仓库**：https://github.com/stanfordnlp/appworld
- **样本路径**：仓库内 `appworld/` 包；任务定义于 `appworld/tasks/`；9 个应用、70 个复杂任务。
- **说明**：在模拟应用生态中考察 agent 的工具/API 调用与任务完成能力。
- **评测维度**：工具/API 调用、接地任务完成度、基于环境状态的自动评分。

### 9. Cybench
- **仓库**：https://github.com/andyzorigin/cybench
- **样本路径**：仓库内 `benchmark/<ctf-event>/<category>/<task>/`，每题含任务描述、starter files、evaluator；`task_list.txt` / `subtask_list.txt`。共 40 个 CTF 任务。
- **说明**：考察 LM agent 的网络安全能力，含无引导与子任务引导两种模式。
- **评测维度**：网络安全 CTF（密码学 / Web / 逆向 / 取证 / 漏洞利用 / 杂项）、无引导解决率、子任务引导解决率。
