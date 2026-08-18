# benchmark/long_horizon — 长程任务 / 长期记忆 评测样本集

本目录沉淀从**公开评测集**筛选出的、与「长程任务（long-horizon / multi-step）」「长期记忆（long-term / cross-session）」「一致性」相关的代表性样本，供 mimofan 借鉴与评测。

> 设计原则（第一性原理）：评测集的价值不在数量，而在能否逼出 agent 在长程、跨步骤、跨会话下的真实失败模式。mimofan 的三大长程失败源（编辑错误、上下文丢失、记忆失效，见 `agentbench/samples/MECE_TAXONOMY.md`）决定了样本聚焦于「记忆召回」与「任务推进 / 防卡死」两条主线。

## 一、来源与筛选裁决

| 评测集 | 数据结构 | 长程多步契合度 | 长期记忆契合度 | 直接消费 | 本目录裁决 |
|---|---|---|---|---|---|
| **SciCode** | HF parquet：`problem_id` / `sub_steps[]`(`step_number`/`step_description_prompt`/`ground_truth_code`) / `general_tests` | ★★★★★（338 子步、显式分解、编号隐含依赖） | ★★★（子步依赖需跨步保持上下文） | 是 | **首选长程多步样本源** → `samples/scicode_long.json` |
| **Terminal-Bench** | 目录 + Python harness：`tasks/<name>/{instruction, run-tests.sh, solution.sh, task.yaml}` + `registry.json` | ★★★★（端到端真实任务：编译/训练/部署/debug） | ★★（单会话内多步） | 否（需解析） | 端到端长程任务 → `samples/terminal_bench_e2e.json` |
| **HLE** | parquet：`problem`/`answer`/`subject`/`task_type` | ★★★（含多步推理子集） | ★（单轮） | 是 | 长程推理对照子集 → `samples/hle_reasoning.json` |
| **GPQA** | CSV：`Question`/`Answer`/`Distractor1-4`/`Subject` | ★（单轮短答） | ★（单轮） | 是 | **不引入正文**，仅作知识回忆 / 幻觉负向对照说明 |
| **AA-Omniscience** | **无权威公开子集**（仅百科 / 二手综述，无官方仓库或数据集 URL） | — | — | 否 | **不纳入**（偏单轮知识边界，且无可信来源） |

**关键洞察**：五个公开集里**没有一个提供真正的「跨会话持久化记忆」样本**——全是单会话评测。因此：
- 「长期记忆（跨会话召回）」维度由 mimofan 自有 harness 覆盖：`agentbench/longmemeval_harness.py`(#777) + `agentbench/samples/memory_recall.json`(B5/B6) + MECE **D05**「长程记忆与召回」100 条。本目录**不重复造 D05 样本**。
- 「长程任务（多步 / 跨步骤 / 防卡死）」维度由本目录补强，并与 MECE **D06**「任务规划与目标循环」对照（见第三节）。

## 二、目录结构

```
benchmark/long_horizon/
├── README.md                  # 本文件
├── fetch_data.py              # 下载 / 抽取脚本（SciCode HF / Terminal-Bench / HLE）
├── run_eval.py                # 长程任务端到端评分 harness（三维：子步完成度 / 跨步一致性 / 防卡死）
├── long_horizon_mece.json     # 长程任务视角补充条目（MECE 风格，独立文件，不进 mece_1000/ 以免突破冻结骨架）
└── samples/
    ├── scicode_long.json      # SciCode 长程多步样本（sub_steps >= 3）
    ├── terminal_bench_e2e.json# Terminal-Bench 端到端长程任务
    └── hle_reasoning.json     # HLE 多步推理对照子集
```

## 三、与 MECE D06 的对照映射

MECE 骨架已冻结（不得在簇下超配额补充），故本目录提供**独立的长程执行视角条目**（`long_horizon_mece.json`），其锚点全部指向 D06 已核实的真实符号，作为 D06 的「跨会话 / 端到端」扩展：

| 本目录条目视角 | 对应 D06 簇 | 锚定真实符号（已 grep 核实存在） |
|---|---|---|
| 长程任务跨多回合不丢目标 | D06.3 目标循环推进 | `crates/tui/src/goal_loop/mod.rs::decide_continuation`、`::StopReason::NoProgress` |
| 长程任务重复工具调用被检测 | D06.4 循环与停滞检测 | `crates/tui/src/loop_guard/mod.rs::LoopGuard::observe`、`::LoopPattern` |
| 跨回合 LoopGuard 状态持久化 | D06.4 停滞检测 | `crates/tui/src/loop_guard/mod.rs::LoopGuardState::snapshot_state` / `::restore_state` |
| 长程任务依赖 DAG 阻塞推进 | D06.1 任务模型 | `crates/tui/src/task_manager/mod.rs::dependency_graph` / `::cycle_detection` |

## 四、运行方式

```bash
# 1. 拉取外部样本（需网络 / HuggingFace 访问）
python3 benchmark/long_horizon/fetch_data.py

# 2. 长程任务端到端评分（需 ANTHROPIC_API_KEY，真模型）
python3 benchmark/long_horizon/run_eval.py --limit 5 --json results/long_horizon.json

# 3. 仅校验评分逻辑（mock 轨迹，无需模型）
python3 benchmark/long_horizon/run_eval.py --selftest
```

## 五、License 合规

- **SciCode**：遵循其官方 license，样本仅存 `problem_id` + 抽取后的子步结构，不打包原始论文文本。
- **Terminal-Bench**：MIT，样本来自其公开 `registry.json`。
- **HLE**：要求不公开重传原文。`hle_reasoning.json` 只存 `id` + `subject` + `task_type` + 本地引用，**不存 `problem` / `answer` 全文**。
- **GPQA**：受 "do not reveal examples" 约束，本目录不引入其题目正文，仅作方法论对照。
