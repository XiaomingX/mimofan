# benchmark/lineage — 实验谱系（lineage）评测样本

本目录沉淀「多代理实验编排 chassis 中遍历谱系查询状态 / 审计 / 分支 / 干净删除，无需全历史回放」的评测样本（目标能力 / L5 级）。

## 借鉴来源表

| 来源 | 借鉴的能力 | 备注 |
|---|---|---|
| DVC `exp` | 谱系树 + 分支 + 删除 + 哈希状态查询 | 最佳原型：实验以哈希 DAG 组织，可 `exp show` 查询状态、`exp branch` 开分支、`exp remove` 删除、`exp list` 按哈希定位 |
| MLflow `parentRunId` + 软删除 `lifecycle_stage` | 父子 run 链式谱系 + 软删除审计 | run 通过 `parentRunId` 串成树；删除置 `lifecycle_stage=deleted`（软删除），保留审计痕迹 |
| W&B Artifacts | 二分 DAG | artifact 版本以 content hash 二分 DAG 组织，支持别名/分支与不可变版本遍历 |
| Nix `referrers` 闭包 | 删除前审计影响范围 | `nix store referrers` 列出反向引用闭包，删除前先审计谁依赖它，避免悬挂引用 |

## L5 定级说明

本目录样本定为 **L5（跨会话 / 谱系评测）**。依据 `benchmark/L_TAXONOMY.md` §三：

> L5 = 跨会话/谱系评测：需真模型 + 外部持久状态 / 跨会话 / 谱系遍历，且评估需复杂判定。
> 评估方式：真模型 + 持久化链路 + 复杂 judge。

谱系树遍历、分支、按子树级联软删除均满足「依赖外部持久状态 + 谱系遍历 + 复杂判定」，故属 L5。

## 与 mimofan 现状对照（缺口）

- 当前 **无 chassis / 实验谱系能力**，谱系树遍历 / 分支 / 按子树级联软删除均为未实现目标能力。
- 最近邻真实符号：
  - `FleetManager::status` —— 单实体状态查询（非谱系树遍历）。
  - `evolve::CandidateLineage` —— 单链血统（非分支树）。
  - `purge::execute_purge_operations` —— 消息级联清理（非谱系子树级联软删除）。

## 目录结构

```
benchmark/lineage/
├── README.md                         # 本文件
├── lineage_gap_mece.json             # 缺口 → MECE 条目映射
├── run_eval.py                       # 评测 harness（--selftest / 真模型）
└── samples/
    └── lineage_tasks.json            # 评测样本（query/audit/branch/cascade_delete）
```

## 运行命令

```bash
python3 benchmark/lineage/run_eval.py --selftest          # 验证评分逻辑（无需模型）
python3 benchmark/lineage/run_eval.py --limit 2 --json results/lineage.json   # 真模型评测（需 ANTHROPIC_* 环境变量）
```

## 说明

当前跑真模型 harness 会因目标能力未实现而**全 fail**，这是诚实缺口暴露，并非 harness 错误。先合入缺口断言（L5 样本），待 chassis 实现后回填样本使其通过。
