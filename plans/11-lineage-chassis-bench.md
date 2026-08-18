# 计划：多代理实验编排（chassis）谱系管理评测样本

> 由 `/make-plan` 生成。目标：规划并沉淀一套评测样本，验收「多代理实验编排（chassis）中，让智能体可遍历谱系高效查询状态、审计、分支、干净删除，无需全历史回放」这一能力。
>
> **第一性原理出发点**：该能力的本质是「实验谱系（lineage）是一棵有向无环树，agent 必须能 (a) 遍历树查询任意节点状态、(b) 审计节点间变更、(c) 从分支点长出新分支、(d) 按子树级联干净删除（含影响闭包审计），且全程无需重放全历史」。评测的不是"跑实验"，而是"管理实验的元状态"。

---

## 0. 调研结论（Phase 0：Documentation Discovery）

### 0.1 mimofan 现状（已 grep 全仓核实，confidence 高）

**该能力完全未实现**。相关但不同的真实符号：
- `crates/tui/src/fleet/manager.rs::FleetManager::status` / `run_status` — 单实体（一次 run）状态查询，用 ledger 增量重建，**不重放对话历史**，但**无谱系树/分支/删除子树**概念。
- `crates/tui/src/fleet/ledger.rs::FleetLedger::rebuild_state` — 增量事件重建（最接近"无需全历史回放查状态"）。
- `crates/tui/src/evolve/mod.rs::CandidateLineage` / `record_candidate` — 单链候选血统留痕（仅有 parent_id 追加写，**无遍历/分支/删除接口**）。
- `crates/tui/src/session_manager/mod.rs::mark_forked_from` — session 分叉轻量父指针（无分支树/遍历/按谱系删除）。
- `crates/tui/src/tools/subagent/lifecycle.rs::LifecycleTracker` — 单 agent 生命周期 O(1) 查询（无谱系维度）。
- `crates/tui/src/tools/workflow.rs::workflow` — 任务 DAG（节点是 sub-agent 调用，replay 跳过已完成节点，非实验谱系）。
- `crates/tui/src/purge/mod.rs::execute_purge_operations` — 消息级联清理（删 tool-call 级联删 result，**非实验谱系子树删除**）。
- **不存在**：`chassis` / `Experiment` / `ExperimentLineage::traverse` / `lineage::branch` / `experiment::delete_subtree` / `sweep` 等符号。

**结论**：本计划把该能力项作为**目标能力（gap）**规划——既定义期望能力的评测样本（验收标准），又用"缺口断言"形式落到 MECE 体系（指向最近邻真实符号，标注其余为未实现）。

### 0.2 公开借鉴源（confidence 见各源）

| 系统 | 借鉴点 | 具体 API/结构 |
|---|---|---|
| **DVC `exp`**（最佳原型） | 谱系树展示 + 从节点分支 + 删除实验 + 哈希状态查询（无需重放） | `dvc exp show` / `dvc exp branch <exp> <branch>` / `dvc exp remove` / `dvc exp clean` / `dvc.lock` 内容哈希 |
| **MLflow Tracking** | 父子关系 + 软删除 + 过滤查询 | `mlflow.parentRunId`（单指针）/ `lifecycle_stage: active\|deleted` / `search_runs(filter_string)` |
| **W&B Artifacts** | 二分 DAG 谱系（run × artifact） | `wandb.Artifact` / `log_artifact` / `use_artifact` |
| **Nix store** | 反向引用闭包（删除前审计影响范围） | `nix-store --query --referrers`（子树闭包语义） |

**关键 gap（公开 benchmark 基本空白）**：没有公开基准直接评测"agent 遍历谱系树 / 分支 / 级联删除"。本计划自建样本集，可成为 mimofan 差异化卖点。

### 0.3 样本 Schema 草案（借鉴映射）

采用：`parent_run_id`（MLflow）+ `lifeage_stage` 软删除（MLflow）+ `content_hash`（DVC/Nix）+ `branch_point`/`children`（DVC + 自补）+ `inputs`/`outputs` artifacts（W&B）。

四任务类型对应四能力：
- `lineage_traversal_query` — 遍历谱系查询状态（含祖先链、无需重放）
- `lineage_audit` — 审计节点间字段级变更
- `lineage_branch` — 从 branch_point 长出新分支（保持 DAG 无环）
- `lineage_cascade_delete` — 先算影响闭包再软删除子树（参考 Nix referrers）

### 0.4 与 benchmark 分级体系（L-TAXONOMY.md）的关系

该能力评测属 **L5（跨会话/谱系评测）**：需真模型 + 外部持久状态/谱系树 + 复杂判定（遍历/闭包）。当前 `sample_registry.json` 显示 L5 样本为 **0**，印证这是评测盲区——本计划补上首个 L5 样本集。

---

## 1. 总体方案

新建 `benchmark/lineage/` 子目录（与 `long_horizon/` 并列，不污染现有 MECE 引擎），包含：
- 样本集（`samples/lineage_tasks.json`，多棵实验谱系树，分难度档）
- 评分 harness（`run_eval.py`，L5 级，真模型 + 谱系操作判定）
- MECE 缺口断言（`lineage_gap_mece.json`，指向最近邻真实符号）
- README（借鉴来源、L5 定级、与 chassis 现状对照）

不改动 `mece_1000/`（骨架冻结）。缺口断言以独立文件承载。

---

## 2. Phase A：实验谱系评测样本集

### What to implement
写 `benchmark/lineage/samples/lineage_tasks.json`，含 ≥3 棵谱系树（简单 4 节点 / 中等 8 节点 / 复杂含软删除与多分支点），每棵树挂 4 类任务（query/audit/branch/delete），按 L5 难度标注。

### Documentation references（照搬 schema 草案，见 §0.3）
- DVC `exp` 字段语义：`https://doc.dvc.org/command-reference/exp`
- MLflow `parentRunId` / `lifecycle_stage`：`https://mlflow.org/docs/latest/ml/tracking/tracking-api/`
- W&B Artifact DAG：`https://deepwiki.com/wandb/examples/5.1-artifacts-and-data-versioning`
- Nix referrers 闭包：`https://nixos.org/manual/nix/stable/command-ref/nix-store/query.html`

### 样本节点 schema（copy 自草案）
```json
{
  "run_id": "r1b1",
  "parent_run_id": "r1b",
  "name": "distill",
  "branch_point": false,
  "children": [],
  "status": "FINISHED",
  "lifecycle_stage": "active",
  "params": {"lr": 0.005, "batch": 128},
  "metrics": {"accuracy": 0.90},
  "content_hash": "sha256:9900...",
  "inputs": ["artifact:model-r1b"],
  "outputs": ["artifact:model-r1b1"]
}
```
任务 schema：
```json
{
  "task_id": "T1-query-state",
  "type": "lineage_traversal_query",
  "prompt": "查询 run r1b1 的祖先链...无需重跑任何实验",
  "expected": {"ancestors": ["r1b1","r1b","r1","r0"], "leaf_accuracy": 0.90},
  "constraint": "no_full_replay",
  "assert_key": "returns_ancestor_chain_in_order"
}
```

### Verification checklist
- [ ] JSON 合法，每棵树是合法 DAG（无环，parent 指针闭合）。
- [ ] 每棵树覆盖 4 类任务各 ≥1 条。
- [ ] `expected.affected_closure`（cascade delete）计算正确（参考 Nix referrers：从目标节点 DFS 所有 children）。
- [ ] 至少 1 棵树含 `lifecycle_stage: deleted` 节点，验证"软删除仍可审计查询"。

### Anti-pattern guards
- 不得把 `content_hash` 设计成需要重算实验——它只是状态查询的输入，harness 不许调用"重跑"接口（constraint `no_full_replay`）。
- 不得让 `children` 与 `parent_run_id` 矛盾（必须双向一致）。
- 不得引入 mimofan 不存在的符号作为 `assert_key`（本文件是样本，非 MECE 条目）。

---

## 3. Phase B：L5 评分 harness

### What to implement
写 `benchmark/lineage/run_eval.py`，参考 `benchmark/agentbench/longmemeval_harness.py` 与 `benchmark/long_horizon/run_eval.py` 的写法（复用 `p0_dynamic.client_cfg`/`call_messages`，LLM-as-judge yes/no），对样本做真模型评测。

四任务判定（不依赖重放，只验证"元操作"正确性）：
1. `lineage_traversal_query`：judge 判 ancestor 链完整有序、leaf 指标正确。
2. `lineage_audit`：judge 判字段级 diff 正确（r1 vs r0 / r1b1 vs r0）。
3. `lineage_branch`：结构化校验——新节点 `parent_run_id` 指向 branch_point，且树仍 DAG 无环（用代码算环，非 judge）。
4. `lineage_cascade_delete`：先校验 agent **先输出影响闭包**再软删除，且未误删兄弟子树（结构化校验闭包 + judge 软删除语义）。

### Documentation references（照搬现有 harness）
- `benchmark/agentbench/longmemeval_harness.py::judge_hit`（L102-136，yes/no judge）
- `benchmark/long_horizon/run_eval.py::score_cross_step_consistency`（跨步判定范式）
- `benchmark/agentbench/p0_dynamic.py::client_cfg` / `call_messages`（真模型调用）

### Verification checklist
- [ ] `python3 benchmark/lineage/run_eval.py --selftest` 用 mock 谱系树 + mock agent 响应，验证四任务判定逻辑（含 cascade delete 闭包计算、DAG 无环校验）。
- [ ] `--limit 2` 真模型跑通，输出四维分数 JSON。
- [ ] DAG 无环校验用代码实现（不靠模型），cascade 闭包计算正确。

### Anti-pattern guards
- judge 必须 yes/no 二值（仿 `judge_hit`），不自由文本打分。
- harness 注入 `no_full_replay` 约束：prompt 明确"只许查询状态/谱系，不许重跑实验"，评分时若 agent 调用重跑类工具应扣分（记录 audit 日志）。
- 不得依赖 `cargo test` 抢锁（多 agent 并行用 `--target-dir` 隔离，仿 mece_bench.py:814）。

---

## 4. Phase C：MECE 缺口断言（不突破冻结骨架）

### What to implement
写 `benchmark/lineage/lineage_gap_mece.json`（独立文件，非 `mece_1000/`），标注该能力为**目标/gap**，assert_key 指向最近邻真实符号，desc 注明"其余子能力未实现"。

条目示例（锚定 §0.1 真实符号）：
```json
{
  "id": "LHX.1.001",
  "domain": "LHX",
  "cluster": 1,
  "assert_key": "crates/tui/src/fleet/manager.rs::FleetManager::status",
  "view": "existence",
  "tier": "T1",
  "desc": "已具备单实体状态查询（ledger 增量重建，不重放对话）；谱系树遍历/分支/级联删除为未实现缺口",
  "weight": 1.0,
  "check": {"kind": "grep", "files": ["crates/tui/src/fleet/manager.rs"], "patterns": ["pub fn status"]}
}
```
另含 `evolve/mod.rs::CandidateLineage`（血统留痕最近邻）、`purge/mod.rs::execute_purge_operations`（级联清理最近邻）等，并在 desc 明确标注缺口。

### Documentation references
- `MECE_TAXONOMY.md` §二 assert_key 规范（不虚构符号，缺口用 `_absent` 例外或 desc 标注）。
- 现有 D07 条目（`part_c_d07_d09.json`）的 assert_key 风格。

### Verification checklist
- [ ] 所有 assert_key grep 命中真实符号（用 coverage 思路校验，不虚构）。
- [ ] JSON 合法，id 唯一。
- [ ] README 注明"该域为 gap，非达标能力"。

### Anti-pattern guards
- 不得写 `chassis`/`ExperimentLineage::traverse` 等不存在符号作 assert_key（违反反作弊）。
- 不得把 gap 条目塞入 `mece_1000/`（违反骨架冻结）。

---

## 5. Phase D：文档与注册表接入

### What to implement
1. `benchmark/lineage/README.md`：借鉴来源表（DVC/MLflow/W&B/Nix）、L5 定级、与 chassis 现状对照、运行命令。
2. 更新 `benchmark/sample_registry.json`（运行 `build_sample_registry.py` 或手动加 `lineage/samples/lineage_tasks.json` 条目：`category: lineage, l_level: 5`）。
3. 更新 `benchmark/L_TAXONOMY.md` §四映射表追加 lineage 样本（L5）。

### Verification checklist
- [ ] README 存在且说明 L5 与缺口性质。
- [ ] `sample_registry.json` 含 lineage 条目且 l_level=5，by L-level 统计出现 L5>0。
- [ ] `build_sample_registry.py` 可重跑不报错。

---

## 6. Final Verification

1. `python3 benchmark/lineage/run_eval.py --selftest` → 四任务判定 + DAG/闭包逻辑通过。
2. `python3 benchmark/build_sample_registry.py` → lineage 样本注册为 L5。
3. `python3 benchmark/agentbench/coverage_report.py --skip-exec` → 报告含 lineage gap 域（若接入 MECE）或 standalone 覆盖。
4. 反作弊：lineage_gap_mece.json 的 assert_key 全部 grep 命中真实符号（无 `chassis`/`Experiment` 伪造）。

---

## 7. 已知 gap / 风险

- **mimofan 无 chassis 能力**：本计划样本是"期望能力验收标准"，当前跑 harness 会全 fail（这是诚实的缺口暴露，非 bug）。建议先合入缺口断言（Phase C），再推动 chassis 实现后回填样本通过。
- **`no_full_replay` 约束无 chassis 接口支撑**：benchmark 只能靠 prompt 约束 + audit 日志记录 agent 是否调用重跑，无法在系统层强制禁止（真实架构 gap，已标注）。
- **DVC/MLflow 字段为借鉴命名**：样本 schema 用其命名是为了可读性与行业对齐，不代表 mimofan 实现了这些 API。
- **Nix referrers 闭包语义未逐字验证**：cascade delete 的"先算影响闭包"用标准 DFS children 闭包即可，不依赖 Nix 实现细节。
- **真模型成本**：L5 评测消耗多轮真模型调用，CI 应默认跳过，仅本地/夜间跑（见 L_TAXONOMY.md §五）。
