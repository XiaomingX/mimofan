# Benchmark 样本分级体系（L-Taxonomy）

对 `benchmark/` 下的全部评测样本进行 **分类（category）、分级（tier 已存在于 MECE）、分层（L-level）** 的统一标注。本文件定义 L 分层口径，供 `sample_registry.json` 与各评测 harness 复用。

## 一、分类（category）

按评测对象的能力域归类：

| category | 含义 | 主要样本来源 |
|---|---|---|
| `static-capability` | 源码静态能力存在性 / 结构断言 | MECE A 类（`capability_matrix*.json`）、MECE `mece_1000/` |
| `dynamic-metric` | 运行指标（构建/测试/token/记忆） | MECE B 类（`dynamic_bench.py`）、`tokenizer_samples.json`、`memory_recall.json` |
| `long-horizon` | 长程多步 / 跨会话任务 | `long_horizon/samples/*`、`long_horizon_mece.json` |
| `long-term-memory` | 跨会话持久记忆召回 | `longmemeval_harness.py`、`memory_recall.json`(B5/B6) |
| `vuln-hunt` | 漏洞挖掘长程任务（多轴评分） | `vuln_hunt/tasks/*`、`vuln_hunt/fixtures/*` |
| `p0-e2e` | 真模型端到端（省 token/性能） | `p0/samples/*` |

## 二、分级（tier）—— MECE 已有，不复述

MECE 条目自带 `tier`：T1 静态存在性 / T2 结构断言 / T3 真实执行（见 `agentbench/samples/MECE_TAXONOMY.md` §二）。本 L 体系在 tier 之上做"评估难度"的额外分层。

## 三、分层（L-level）—— 数字越高越难评估

L 衡量的是**"评估这个样本本身有多难"**，而非样本考察的能力难度。维度包括：是否需要真模型、是否多轮/长程、是否依赖外部持久状态、判定是否需复杂推理。

| L | 名称 | 评估难度特征 | 典型样本 | 评估方式 |
|---|---|---|---|---|
| **L0** | 规则可判定 | 纯静态/grep/schema 校验，零模型、零执行 | MECE T1 条目、`capability_matrix.json` 的 existence 探针 | `grep` / JSON schema 校验 |
| **L1** | 结构可判定 | 解析源码结构 / JSON 字段，零模型 | MECE T2 条目、`tokenizer_samples.json` 真值比对 | AST/正则/字段比对 |
| **L2** | 执行可判定（deterministic） | 需跑测试/CLI/example 二进制，结果确定 | MECE T3 条目、`vuln_hunt` 的 `evaluate.py` 结构判定、`p0/perf_baseline` | `cargo test` / 二进制退出码 |
| **L3** | 单轮模型评测 | 需真模型单次调用 + LLM-as-judge | `memory_recall.json`(B5/B6) 单查询、`p0/token_budget` 省 token 判定 | 真模型 + yes/no judge |
| **L4** | 长程轨迹评测 | 需真模型多轮/长程轨迹 + 跨步一致性判定 | `long_horizon/samples/scicode_long.json`、`terminal_bench_e2e.json`、`vuln_hunt` 三维评测 | 真模型多轮 + 三维 judge |
| **L5** | 跨会话/谱系评测 | 需真模型 + 外部持久状态/跨会话/谱系遍历，且评估需复杂判定 | `longmemeval_harness.py`(跨会话)、实验谱系审计/分支/删除类（目标能力，见 `plans/`） | 真模型 + 持久化链路 + 复杂 judge |

**单调性**：L5 ⊃ L4 ⊃ L3 ⊃ L2 ⊃ L1 ⊃ L0。一个样本若同时满足多个特征，取**最高** L。

## 四、与各评测集的映射（summary）

| 样本集 | category | L-level | 说明 |
|---|---|---|---|
| `agentbench/samples/mece_1000/part_*.json` | static-capability | L0–L2（按 tier） | T1→L0, T2→L1, T3→L2 |
| `agentbench/samples/capability_matrix.json` | static-capability | L0 | 静态探针 |
| `agentbench/samples/capability_matrix_p0.json` | static-capability | L0–L1 | P0 专项静态/结构 |
| `agentbench/samples/tokenizer_samples.json` | dynamic-metric | L1 | 真值比对 |
| `agentbench/samples/memory_recall.json` | long-term-memory | L3 | 单查询 recall + judge |
| `agentbench/longmemeval_harness.py` | long-term-memory | L5 | 跨会话持久化 + judge |
| `long_horizon/samples/scicode_long.json` | long-horizon | L4 | 多步子任务 + 跨步一致性 |
| `long_horizon/samples/terminal_bench_e2e.json` | long-horizon | L4 | 端到端长程 + 测试脚本 |
| `long_horizon/samples/hle_reasoning.json` | long-horizon | L3 | 单轮长推理（仅对照索引） |
| `long_horizon/long_horizon_mece.json` | long-horizon | L0–L2（按 tier） | T1→L0, T2→L1, T3→L2 |
| `vuln_hunt/tasks/*.json` | vuln-hunt | L4 | 长程多轴评测 |
| `vuln_hunt/fixtures/*` | vuln-hunt | L2 | 结构判定（good/bad 对照） |
| `p0/samples/perf_baseline.json` | p0-e2e | L2 | 延迟基线（deterministic） |
| `p0/samples/token_budget.json` | p0-e2e | L3 | 省 token 判定（需真模型） |
| `p0/samples/prefix_cache.json` | p0-e2e | L3 | cache 收益判定（需真模型） |
| `lineage/samples/lineage_tasks.json` | lineage | L5 | 谱系树 query/audit/branch/cascade_delete（目标能力，当前未实现） |

## 五、用途

1. **覆盖报告**：`coverage_report.py` 可扩展按 L-level 统计各层样本数与通过率，识别"高难评估样本（L4/L5）是否被充分覆盖"。
2. **耗时/成本预估**：L3+ 消耗真模型调用，L4/L5 消耗最多；CI 可默认跳过 L4/L5，仅本地/夜间跑。
3. **缺口识别**：若某 category 在 L4/L5 层缺失样本，即为评测盲区（如本仓库当前无 L5 实验谱系样本，对应目标能力）。
