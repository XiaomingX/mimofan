# Code Agent 横向评估指标体系（EVAL_METRICS）

本文件定义 mimofan 与竞品（openclaw / kilocode / qwen-code / kimi-code / hermes-agent）横向对比时使用的
统一指标体系，同时作为 `benchmark/agentbench/` 评分脚本的权威依据。

> 设计原则：**能自动化测的优先自动化**；无法自动化的走「能力矩阵静态评分」，但必须给出代码落点作为证据，
> 不允许凭印象打分。

---

## 一、业界主流评估指标综述

学术界与工业界评估 code agent 时，常见的指标可归为四层：

### 1. 端到端任务完成类（最权威，但成本最高）
| Benchmark | 度量 | 说明 |
|---|---|---|
| SWE-bench / SWE-bench Verified | `% Resolved` | 真实 GitHub issue 修复率，业界事实标准 |
| SWE-bench Multimodal | `% Resolved` | 含截图/UI 的 issue |
| Terminal-Bench | `% Solved` | 纯终端任务完成率，最贴近 CLI agent 形态 |
| SWE-Lancer | `$ Earned` | 以任务报酬计价的完成度 |
| Aider Polyglot | `pass@1/pass@2` | 多语言编辑正确率，强调 diff 编辑格式遵循度 |
| HumanEval / MBPP | `pass@k` | 函数级代码生成（已趋饱和，参考价值下降） |
| LiveCodeBench | `pass@1` | 防污染的滚动更新题库 |
| CodeContests | `pass@k` | 竞赛级算法能力 |

### 2. Agent 行为质量类（harness 层最能体现差异）
| 指标 | 定义 | 为什么重要 |
|---|---|---|
| Tool-call Accuracy | 工具选择正确率 + 参数合法率 | harness 的 schema 设计直接决定该值 |
| Tool-call Efficiency | 完成任务所需工具调用次数 | 反映规划质量，越少越好 |
| Edit Apply Success Rate | diff/patch 一次成功应用率 | 编辑格式健壮性，Aider 生态核心指标 |
| Error Recovery Rate | 报错后自主恢复成功率 | 长程任务的关键 |
| Turn Efficiency | 完成任务的对话轮数 | 与成本直接挂钩 |
| Instruction Adherence | 是否遵守 rules/AGENTS.md 约束 | 工程化落地的前提 |
| Hallucination Rate (API/路径) | 引用不存在的文件/函数/API 的比例 | 直接影响可信度 |

### 3. 上下文与长程能力类（本轮重点）
| 指标 | 定义 |
|---|---|
| Effective Context Utilization | 有效上下文利用率 = 有用 token / 总 token |
| Compaction Fidelity | 压缩后关键信息保留率（压缩前后关键事实召回） |
| Prefix Cache Hit Rate | 前缀缓存命中率，直接决定成本与延迟 |
| Long-horizon Task Success | 需 ≥20 轮、跨多文件的任务完成率 |
| Cross-session Memory Recall | 跨会话记忆召回准确率（本轮新增重点） |
| Context Overflow Survival | 上下文溢出后能否不丢关键状态继续工作 |

### 4. 工程与运行时类
| 指标 | 定义 |
|---|---|
| TTFT / P50 / P95 Latency | 首 token 时间与端到端延迟分布 |
| Token Cost per Task | 每任务 token 成本（输入/输出/缓存分开计） |
| Startup Time / RSS | 冷启动时间与常驻内存 |
| Crash-free Rate | 长时间运行稳定性 |
| Sandbox Escape Resistance | 危险命令拦截率（安全） |
| Secret Leak Rate | 密钥外泄拦截率（安全） |

---

## 二、mimofan 本轮采用的评分体系（总分 100）

考虑到「不依赖真实 LLM 也要能跑出可复现分数」，本轮把指标拆成
**A. 静态能力矩阵（60 分，离线可测）** + **B. 动态运行指标（40 分，需构建产物/可选 API）**。

### A. 静态能力矩阵（60 分）

以「能力是否存在 + 实现深度」打分，每项 0/0.5/1 系数乘以权重。
证据必须是代码落点（文件 + 符号），由 `benchmark/agentbench/capability_probe.py` 自动 grep 校验。

| 编号 | 能力域 | 权重 | 关键探针 |
|---|---|---|---|
| A1 | 核心文件工具健壮性 | 8 | read/write/edit/apply_patch 的越界、编码、原子写、并发保护 |
| A2 | Shell 与执行安全 | 6 | 超时/取消/交互式/沙箱/危险命令拦截 |
| A3 | 上下文压缩与预算 | 10 | 压缩三范式、统一阈值入口、tokenizer 精度、溢出恢复 |
| A4 | 长程记忆 | 10 | 跨会话记忆、向量召回、记忆治理（陈旧性/遗忘）、自动注入 |
| A5 | 任务与规划 | 6 | todo 依赖图、plan 审批闸门、目标循环 |
| A6 | 多 Agent 编排 | 5 | subagent 生命周期、消息总线、worktree 隔离、结果聚合 |
| A7 | 扩展性 | 5 | MCP、skills、自定义命令、hook、插件 |
| A8 | 代码理解 | 5 | symbol_index、LSP、语义检索 |
| A9 | 可观测与成本 | 3 | token/成本统计、trace、审计日志 |
| A10 | 工程化质量 | 2 | 零 warning 构建、测试覆盖 |

### B. 动态运行指标（40 分）

| 编号 | 指标 | 权重 | 采集方式 |
|---|---|---|---|
| B1 | 构建健康度（零 warning） | 6 | `cargo clippy --all-targets` warning 数 |
| B2 | 单元/集成测试通过率 | 10 | `cargo test --workspace` 通过率 |
| B3 | 冷启动时间 | 5 | `mimofan --version` 十次取 P50 |
| B4 | Tokenizer 计数精度 | 7 | 中英文样本相对真实分词的误差率 |
| B5 | 压缩保真度 | 7 | 压缩前后关键事实召回率（离线可测） |
| B6 | 记忆跨会话召回 | 5 | 写入→重启→召回 的端到端准确率 |

> **B4/B5/B6 是本轮升级的核心受益项**，也是改进前后分差最能体现的地方。

---

## 三、评分与对比口径

1. **基线（Before）**：改动前 commit 上跑一次，产出 `benchmark/agentbench/results/baseline.json`。
2. **改进后（After）**：升级完成后同一套脚本再跑，产出 `after.json`。
3. **竞品**：竞品仅参与「A 静态能力矩阵」的人工/半自动评分（无法在本机跑通它们的完整运行时），
   在报告中明确标注为**静态评分**，不与 mimofan 的动态分混淆。
4. 所有分数保留一位小数，报告中给出 `Before → After (Δ)` 三元组。

---

## 四、反作弊约束

- 探针 grep 到符号存在**不等于**能力可用；A 类每项至少有 1 个对应的真实测试用例佐证，
  否则该项系数封顶 0.5。
- 不允许为了提分而写「只被测试调用」的死代码；`cargo clippy` 的 `dead_code` warning 计入 B1 扣分。
- Benchmark 样本集与实现代码由不同阶段产出，样本集一旦冻结（`samples/` 目录），
  实现阶段不得修改样本，只能修改被测代码。
