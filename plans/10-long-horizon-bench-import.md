# 计划：从公开评测集引入长程任务 / 长期记忆样本到 benchmark/

> 本计划由 `/make-plan` 生成。目标：从公开评测集（SciCode / Terminal-Bench / GPQA / HLE，以及曾被提及但**无权威来源故不纳入**的 AA-Omniscience）筛选与「长程任务」「长期记忆」「一致性」相关的代表性样本，迁移沉淀到本项目的 `benchmark/` 体系（重点并入现有 MECE 骨架 D05/D06 簇）。
>
> 第一性原理出发点：评测集的价值不在"数量"，而在"它能否逼出 agent 在长程、跨会话、跨步骤下的真实失败模式"。本项目的三大长程失败源（编辑错误、上下文丢失、记忆失效，见 MECE_TAXONOMY.md:99-100）决定了样本应落到"记忆召回"与"任务推进/防卡死"两条主线上。

---

## 0. 调研结论（Phase 0：Documentation Discovery）

### 0.1 五个公开评测集的真实结构与契合度

| 评测集 | 数据结构 | 长程多步契合度 | 长期记忆契合度 | 可直接消费 | 迁移裁决 |
|---|---|---|---|---|---|
| **SciCode** | HF 数据集(parquet)：`problem_id`/`sub_steps[]`(`step_number`/`step_description_prompt`/`ground_truth_code`)/`general_tests` | ★★★★★（338 子步、显式分解、编号隐含依赖） | ★★★（子步依赖需跨步保持上下文） | 是（`load_dataset("SciCode1/SciCode")`） | **首选长程多步样本源** |
| **Terminal-Bench** | 目录结构 + Python harness：`tasks/<name>/{instruction, run-tests.sh, solution.sh, task.yaml}` + `registry.json` | ★★★★（端到端真实任务：编译/训练/部署/debug） | ★★（单会话内多步，无跨会话） | 否（需自解析 `task.yaml`） | 终端 agent 长程任务，需写解析器抽取 |
| **HLE** | parquet：`problem`/`answer`/`subject`/`task_type`(含 multiple-choice/short-answer) | ★★★（含多步推理子集，需筛 `task_type`） | ★（单轮） | 是 | 长程推理子集改造源（筛 `task_type`） |
| **GPQA** | CSV：`Question`/`Answer`/`Distractor1-4`/`Subject`/`Difficulty` | ★（单轮短答） | ★（单轮） | 是 | 知识回忆/幻觉**负向对照**，非长程 |
| **AA-Omniscience** | **未找到权威公开子集**（仅百科/二手综述，无官方仓库/数据集 URL） | 不纳入 | 不纳入 | 否 | **不纳入**。它偏"单轮知识边界/幻觉"，与跨会话记忆概念不同，且无可信来源 |

**关键洞察（第一性原理）**：五个公开集里**没有一个提供真正的"跨会话持久化记忆"样本**——它们全是单会话评测。因此：
- "长期记忆（跨会话召回）"维度，本项目已自有成熟 harness（`longmemeval_harness.py` + `memory_recall.json` + D05 100 条），**无需从外部集引入新样本**，本计划只做"外部集如何**对照验证**现有 D05 覆盖度"的分析，不新增 D05 条目（避免重复计分）。
- "长程任务（多步 / 跨步骤 / 防卡死）"维度是真正的缺口，外部集（尤其 SciCode 的子步依赖 + Terminal-Bench 端到端）可补强 **D06**。

### 0.2 当前 benchmark 体系现状（已核实）

- **长期记忆**：成熟。D05「长程记忆与召回」100 条（`part_b_d04_d06.json`），5 簇满配额；另有 `memory_recall.json`(B5/B6)、`longmemeval_harness.py`(#777)、`p0_dynamic.py` 的 MEM 评测。
- **长程任务**：雏形。D06「任务规划与目标循环」80 条（`part_b_d04_d06.json`），4 簇：①任务模型与状态机 ②计划生成与审批 ③目标循环推进 ④循环与停滞检测。
- **一致性 / 一贯性**：空白（仅 `benchmark/vuln_hunt/evaluate.py::score_consistency` 是单任务内"先举证后结论"的评测侧维度，非跨会话一贯性；Rust crate 内无跨会话一致性门符号）。

### 0.3 MECE 骨架约束（硬约束，来自 MECE_TAXONOMY.md:3-4）

> "骨架冻结后，编写条目阶段不得新增/重命名域与能力簇，只能在能力簇下补充断言条目。"

**因此本计划的迁移策略 = 向 D06 的 4 个现有簇补充条目（不新建域/簇），并向 `benchmark/` 引入外部样本数据文件 + 解析/运行脚本。** 若评估后认为必须新增"长程任务"专属域，则需先走 MECE 骨架变更流程（扩展 TAXONOMY + `validate_mece.py` 配额表），本计划将其列为**可选 Phase 4**，默认不执行。

### 0.4 真实符号清单（assert_key 必须指向这些，已逐行核实）

**长程任务 / 防卡死（用于 D06 条目）：**
- `crates/tui/src/loop_guard/mod.rs::LoopGuard`
- `crates/tui/src/loop_guard/mod.rs::LoopPattern`（变体 `RepeatedCall`/`Alternating`/`NoProgress`/`SemanticEcho`/`StreamingRepetition`/`MemorySkill`/`SelfCheck`）
- `crates/tui/src/loop_guard/mod.rs::LoopGuard::observe`
- `crates/tui/src/loop_guard/mod.rs::LoopGuardState`（跨回合持久化：`snapshot_state`/`restore_state`）
- `crates/tui/src/goal_loop/mod.rs::decide_continuation`
- `crates/tui/src/goal_loop/mod.rs::StopReason`（`Completed`/`Blocked`/`TokenBudget`/`TimeBudget`/`ContinuationLimit`/`NoProgress`/`RepeatedError`）
- `crates/goal_core/src/lib.rs::GoalStatus`（`Active`/`Paused`/`Complete`/`Blocked`）
- `crates/goal_core/src/lib.rs::GoalQueue` / `GoalEntry`（`blocked_by` 依赖 DAG）
- `crates/tui/src/tools/plan.rs::PlanState` / `PlanDeviation` / `exit_plan_mode_plan_text`

**长期记忆（仅对照，D05 已饱和）：**
- `crates/memory/src/vector.rs::VectorStore`（`::store_observation`/`::search`/`::promote`/`::prune`）
- `crates/memory/src/consolidation.rs::MemoryEntry::decay_importance`
- `crates/memory/src/injector.rs::MemoryInjector::generate_injection`

**易踩坑（来自历史 MECE 经验）：**
1. 两套记忆系统：`crates/memory`(向量，实验性) vs `crates/tui/src/memory.rs`(文件型用户记忆)——assert_key 必须精确到模块。
2. `LoopGuard` 默认 `enabled:false`；`goal_loop` 的 `no_progress_rounds` 默认 `None`——写条目时标注"能力存在但默认可能关闭"，用 `T2 struct_assert` / `T3 exec` 而非纯 `T1 grep` 更能反映真实可用性。
3. `goal_loop::StopReason::NoProgress`(跨回合 goal 级) 与 `loop_guard::LoopPattern::NoProgress`(回合内 tool-call 级) **粒度不同，勿合并**。
4. `score_consistency` 是 vuln_hunt **评测脚本**符号，非 agent 运行时能力，勿当 agent 一致性门。

---

## 1. 总体方案

本计划分 3 个强制 Phase + 1 个可选 Phase，每 Phase 自包含、可在新会话中独立执行：

- **Phase A**：引入外部样本数据（SciCode / Terminal-Bench / HLE 子集）到 `benchmark/long_horizon/` 新子目录，含数据下载脚本与解析器。
- **Phase B**：基于 SciCode 子步依赖 + Terminal-Bench 端到端任务，编写一批 **D06 簇补充 MECE 条目**（长程任务 / 防卡死视角），落到 `mece_1000/part_b_d04_d06.json` 的 D06 段（不超配额，仅补充断言密度）。
- **Phase C**：新增 `benchmark/long_horizon/run_eval.py`——一个**长程任务端到端评分 harness**（参考 `longmemeval_harness.py` 写法），把 SciCode/Terminal-Bench 样本喂给真模型，按"子步完成度 / 跨步状态一致性 / 防卡死"三维打分。
- **Phase D（可选）**：若 D06 配额不足以容纳新维度，走 MECE 骨架变更流程新增"长程任务执行"域。默认跳过。

**不触碰的目录**：`benchmark/` 之外的任何 crate 源码、`benchmark/agentbench/mece_1000/` 中 D01-D05/D07-D13 段、MECE 骨架（除非走 Phase D）。

---

## 2. Phase A：引入外部样本数据

### What to implement
在 `benchmark/` 下新建 `long_horizon/` 子目录，作为外部长程样本的统一落点（不污染现有 `agentbench/` 评测引擎）。

```
benchmark/long_horizon/
├── README.md                  # 说明来源、license、如何更新
├── fetch_data.py              # 下载/抽取脚本（SciCode HF / Terminal-Bench registry / HLE parquet）
├── samples/
│   ├── scicode_long.json      # 从 SciCode 抽取的"长程多步"样本（子步 ≥3 的主问题）
│   ├── terminal_bench_e2e.json# 从 Terminal-Bench 抽取的端到端长程任务
│   └── hle_reasoning.json     # 从 HLE 筛选的多步推理子集（task_type=short-answer 且长）
└── (解析中间产物)
```

### Documentation references（照搬，不改造）
- SciCode 字段结构：`https://huggingface.co/datasets/SciCode1/SciCode` → 用 `load_dataset` 读 `sub_steps`、`ground_truth_code`、`general_tests`。
- Terminal-Bench 任务格式：`https://www.tbench.ai/docs/task-format` → 解析 `task.yaml` + `run-tests.sh`。
- HLE 字段：`https://huggingface.co/datasets/cais/hle` → 筛 `task_type` / `subject`。

### 具体步骤
1. 写 `fetch_data.py`：
   - `fetch_scicode()`：用 `datasets` 库加载 `SciCode1/SciCode`，筛 `len(sub_steps) >= 3` 的主问题，输出 `scicode_long.json`，字段映射为统一 schema：
     ```json
     {"id":"SC-<problem_id>", "source":"scicode", "domain":"<subject>",
      "goal":"<problem_description_main>",
      "steps":[{"n":"10.1","prompt":"<step_description_prompt>","gt":"<ground_truth_code>"}],
      "eval":"<general_tests>"}
     ```
   - `fetch_terminal_bench()`：克隆/读取 Terminal-Bench `registry.json`，筛 medium/hard 且含多步特征的任务（如 debug/setup/train 类），解析 `task.yaml` 抽取 `instruction` + 测试脚本路径，输出 `terminal_bench_e2e.json`。
   - `fetch_hle()`：加载 `cais/hle`，筛 `task_type=="short-answer"` 且 `problem` 长度超阈值、且隐含多步的，输出 `hle_reasoning.json`（仅作推理对照，标注 `long_reasoning:true`）。
2. 写 `README.md`：标注 license 合规（HLE 要求不重传原文、GPQA 受 "do not reveal" 约束——**本计划不引入 GPQA 全文，仅引用其作为负向对照说明**）。
3. 运行 `fetch_data.py` 生成 `samples/*.json`，确认文件非空、schema 正确。

### Verification checklist
- [ ] `scicode_long.json` 含 ≥20 条 `steps>=3` 样本（SciCode 共 80 主问题，多数满足）。
- [ ] `terminal_bench_e2e.json` 含 ≥10 条端到端任务。
- [ ] `hle_reasoning.json` 含 ≥15 条多步推理子集。
- [ ] 三个文件 JSON 合法，字段与 schema 一致。

### Anti-pattern guards
- 不要把 GPQA 全文当长程样本引入（它是单轮，会污染"长程"定义）。
- 不要伪造 AA-Omniscience 样本（无权威来源）。
- Terminal-Bench 任务必须来自其 `registry.json` 真实条目，不要手写伪任务。

---

## 3. Phase B：编写 D06 簇补充 MECE 条目

### What to implement
向 `benchmark/agentbench/mece_1000/part_b_d04_d06.json` 的 **D06 段**补充条目，强化"长程任务执行"与"防卡死"视角的断言密度。**不新建域/簇**（遵守冻结约束）。

补充落点（在 D06 现有 4 簇内）：
- **D06.3 目标循环推进**：补"长程任务跨多回合不丢目标"条目（关联 `decide_continuation` / `StopReason::ContinuationLimit`）。
- **D06.4 循环与停滞检测**：补"长程任务中重复工具调用被检测"条目（关联 `LoopGuard::observe` / `LoopPattern::RepeatedCall`）、"跨回合 LoopGuard 状态持久化"条目（关联 `LoopGuardState::snapshot_state`/`restore_state`）。
- **D06.1 任务模型与状态机**：补"长程任务依赖 DAG 阻塞推进"条目（关联 `GoalEntry::blocked_by` / `promote_next_ready`）。

### Documentation references（照搬现有样本格式）
- 条目 schema：`MECE_TAXONOMY.md:211-236`（§五）。
- D06 簇定义：`MECE_TAXONOMY.md:153-159`。
- assert_key 规范：`MECE_TAXONOMY.md:42-78`。
- 现有 D06 条目范例：读 `benchmark/agentbench/mece_1000/part_b_d04_d06.json` 中 `domain=="D06"` 段，仿其 id 编号续写（如 `D06.4.081` 起）。

### 条目范例（可直接复制改字段）
```json
{
  "id": "D06.4.081",
  "domain": "D06",
  "cluster": 4,
  "assert_key": "crates/tui/src/loop_guard/mod.rs::LoopGuard::observe",
  "view": "integration",
  "tier": "T2",
  "desc": "长程任务中引擎每轮调用 LoopGuard::observe 检测重复/无进展，触发 nudge 而非无限循环",
  "weight": 1.0,
  "check": {
    "kind": "struct_assert",
    "files": ["crates/tui/src/core/engine/turn_loop.rs"],
    "assert": "calls_symbol",
    "args": {"fn": "load_shared_loop_guard", "symbol": "loop_guard.observe"}
  }
}
```

### Verification checklist
- [ ] 用 `python3 benchmark/agentbench/validate_mece.py --entries benchmark/agentbench/samples/mece_1000` 跑校验，新条目无 ERROR（assert_key+tier+desc 不重复）。
- [ ] 新条目 assert_key 全部指向 Phase 0.4 核实过的真实符号。
- [ ] D06 段条目数仍 ≤ 配额（80）+ 允许的小幅溢出（历史 D01 曾 116>110），不触发配额 ERROR（校验脚本核对 90% 下限，补充不破此限）。
- [ ] 跑 `python3 benchmark/agentbench/mece_bench.py --skip-exec` 确认新条目被加载、无解析异常。

### Anti-pattern guards
- assert_key 不得虚构（必须 Phase 0.4 清单内）。
- 不得把 `goal_loop::StopReason::NoProgress` 与 `loop_guard::LoopPattern::NoProgress` 写成同一条目。
- 不得新建 `domain` 字段值（如 "D14"）——那是骨架变更，走 Phase D。
- T1 grep 条目必须与同 key 的 T3 配对，否则停在 0.5 系数（TAXONOMY:68-69）。

---

## 4. Phase C：长程任务端到端评分 harness

### What to implement
新增 `benchmark/long_horizon/run_eval.py`，参考 `benchmark/agentbench/longmemeval_harness.py` 的写法（harness + LLM-as-judge + 维度聚合），对 Phase A 的样本做真模型端到端长程评分。

评分三维（对应第一性原理的三失败源）：
1. **子步完成度（Step Completion）**：SciCode 的每个 `steps[i]` 是否完成且 `general_tests` 通过。
2. **跨步状态一致性（Cross-step Consistency）**：长程任务中，前置步骤的中间产物/决策在后续步骤仍被正确使用（不因上下文压缩/记忆失效而丢失）——参考 tau-bench 的"最终状态哈希等价"思想，但这里用 LLM-as-judge 判中间状态是否被一致沿用。
3. **防卡死（Anti-stall）**：长程任务是否在合理回合内终止（无无限循环 / `LoopGuard` 触发）——参考 `StopReason` 枚举。

### Documentation references（照搬现有 harness）
- `benchmark/agentbench/longmemeval_harness.py`：`judge_hit()`(L102-136, LLM-as-judge yes/no)、`rule_hit()`(L140-145, 子串对照)、`DIM_LABELS`(L149-156, 维度聚合)、`main()`(L323)。
- `benchmark/agentbench/p0_dynamic.py`：真模型调用入口（需 `ANTHROPIC_*` env，L65-75）、MEM 评测范式。
- 样本格式：Phase A 生成的 `benchmark/long_horizon/samples/*.json`。

### 步骤
1. 写 `run_eval.py`：
   - `load_samples(path)` → 读 Phase A 的 json。
   - `run_task(sample, model)` → 调真模型（参照 p0_dynamic.py 的 `/v1/messages` 调用），逐子步推进，记录轨迹。
   - `judge_step(trajectory, step)` → LLM-as-judge 判该步完成度（yes/no，仿 `judge_hit`）。
   - `judge_consistency(trajectory)` → 抽关键中间决策，判后续是否一致沿用。
   - `judge_antistall(trajectory)` → 判是否在 `DEFAULT_MAX_CONTINUATIONS`(50) 内终止。
   - `aggregate()` → 按三维输出得分 + 与现有 D06 MECE 条目的相关性报告。
2. 写 `README.md` 的运行说明（需 `ANTHROPIC_API_KEY`，参照 p0_dynamic 的环境变量约定）。

### Verification checklist
- [ ] `python3 benchmark/long_horizon/run_eval.py --selftest` 用内置 mock 轨迹跑通三维度评分逻辑（仿 `vuln_hunt/evaluate.py --selftest`）。
- [ ] 用 `--limit 5` 跑 SciCode 子集，输出三维分数 JSON，无异常。
- [ ] 评分输出字段与 `longmemeval_harness.py` 的 `DIM_LABELS` 风格一致（便于团队横向对比）。

### Anti-pattern guards
- 不得依赖 `cargo test` 抢锁（多 agent 并行时务必 `--target-dir` 隔离，见 mece_bench.py:814-815 注释）。
- LLM-as-judge 必须限定 yes/no 二值（仿 `judge_hit`），不要自由文本打分导致不可复现。
- 不要把"单轮短答准确率"混入长程三维（GPQA/HLE 仅作对照说明，不进 run_eval 主路径）。

---

## 5. Phase D（可选）：MECE 骨架变更——新增"长程任务执行"域

> **默认不执行**。仅当评估发现 D06 簇无法容纳"外部长程样本"的新维度（如"跨会话一贯性"）时，才走此流程。

### What to implement
1. 在 `MECE_TAXONOMY.md` §三新增域（如 `D14 长程任务执行`），更新配额表与域总数（当前合计 1000，需重算 12→13 域的配额归一）。
2. 在 `validate_mece.py` 的 `DOMAIN_QUOTA` / `REQUIRED_VIEWS` 同步更新。
3. 新建 `mece_1000/part_f_d14.json` 承载新域条目。

### Anti-pattern guards
- 必须经 MECE 维护者确认骨架变更（冻结约束意味着这是破坏性操作）。
- 域按"能力对象"切分，不按"场景"切分（TAXONOMY:17-18），避免与 D06 重叠。

---

## 6. Final Verification（全计划收尾）

1. `python3 benchmark/agentbench/validate_mece.py --entries benchmark/agentbench/samples/mece_1000` → 全量零 ERROR。
2. `python3 benchmark/agentbench/mece_bench.py --skip-exec` → 全量条目加载成功，D06 段含新增条目。
3. `python3 benchmark/long_horizon/run_eval.py --selftest` → 三维度评分逻辑通过。
4. 反作弊核对：grep 新条目 assert_key，确认全部命中 Phase 0.4 真实符号清单（无虚构）。
5. 文档：更新 `benchmark/README.md` 增加 `long_horizon/` 子目录说明与运行入口。

---

## 7. 已知 gap / 风险

- **Terminal-Bench 样本需自解析**：其任务是目录结构非 JSON，Phase A 的 `fetch_terminal_bench()` 需实际克隆仓库解析 `task.yaml`，字段名未在源级确认（confidence 中），执行时需先 `ls` 验证一个真实任务目录结构再写解析器。
- **SciCode 子步依赖仅由编号隐含**：无显式 `depends_on`，Phase B/C 的"跨步一致性"判定需自行解析 `step_number`（如 `10.1`→`10.2`）。
- **HLE license 约束**：要求不公开重传原文，Phase A 的 `hle_reasoning.json` 只存 `id`+`subject`+`task_type`+本地引用，不存 `problem`/`answer` 全文。
- **AA-Omniscience 不纳入**：无权威来源，任何"跨会话知识边界"对照仅引用百科描述，不当数据。
- **长期记忆维度不新增条目**：D05 已饱和（100/100），外部集无跨会话样本，强行新增会重复计分。本计划仅用外部集做 D05 覆盖度对照分析（写进 `long_horizon/README.md` 的"对照说明"段），不写 D05 条目。
