# MY_PLAN_0818.md — 能力实证核实（第二轮，2026-08-18）

> 继 MY_PLAN_0817.md 之后，对新一轮提交的 `my-plan.md`（9 域 + 升级规划）与
> `横向评测与 Benchmark 报告`（Before/After）+ `升级规划`+`待办方案` 做**代码级实证核实**。
> 方法：4 个并行 Explore agent 逐条 grep/读源码/查测试，结论以磁盘证据为准。
> 重要：本轮**推翻了上一轮（#870/#871）里我自己的多处误判**——凡标 `[更正]` 处即此前记错。

## 〇、最高优先级真缺口（确缺失 + 值得做 + 报告却虚构"已实现"）

这些项被报告写成"本轮已实现 [x]"，但实测**根本不存在**——属于最危险的假完成，应优先立项：

- [ ] **细粒度工具错误码体系**：`error_taxonomy::tool_codes` 及 `EDIT_REQUIRES_PRIOR_READ` / `FILE_CHANGED_SINCE_READ` / `AMBIGUOUS_MATCH` / `TARGET_NOT_REGULAR_FILE` 等**全库零命中**（`file.rs:882` 多命中仅返回通用文本错误，无机器码）。[更正] #870 我曾记"已实现"，误。
- [ ] **验证停机闸门 verification_gate**：报告称"本轮新增已接线"，实测**文件/符号均不存在**，非"编辑前行为级闸门"。
- [ ] **协议违规兜底 nudge stop_intent_guard**：同上，全库零符号，虚构。
- [ ] **AGENTS.md @import 递归导入 resolve_at_imports**：AGENTS.md 加载真实存在（project_context/mod.rs），但 @import 递归子能力**虚构**（仅有父目录向上查找）。
- [ ] **tools/untrusted.rs 信任边界包裹**：文件不存在；真实信任边界以 `capability_policy.rs`(net 检查) + `fetch_url.rs` SSRF guard 形式存在（命名失真，能力非缺失）。
- [ ] **防漂移 benchmark 化 goal_drift_samples.json / goal_drift_bench_test.rs**：文件与测试**均不存在**，完全虚构。

## 一、确已实现（证据充分，[x] 属实）

### 长期记忆 / 压缩 / 规划
- [x] 向量语义记忆 vector_memory（hnsw+sled，默认开启）— `crates/memory/src/vector.rs`
- [x] 文件记忆 + turn 级零 LLM 自动挖掘 — `crates/tui/src/turn_memory.rs`（[更正]路径非 tools/ 下）
- [x] 记忆生命周期治理 decay/evict/rollup — `crates/memory/src/consolidation.rs`
- [x] 跨会话内容级召回 — `injector.rs` reassemble + turn_memory（非 StateStore::search_messages，该符号虚构）
- [x] 周期性记忆 nudge — `loop_guard/mod.rs:537 periodic_memory_skill_nudge`（[更正]非独立 memory_nudge.rs）
- [x] loop_guard 5 检测器（RepeatedCall/Alternating/NoProgress/StreamingRepetition/SemanticEcho）— `loop_guard/mod.rs`
- [x] todo 依赖图 / plan 审批闸门 / goal_loop 预算 / Spec Freeze / GoalGate / drift_check / 压缩三范式 / recover_context_overflow
- [x] **本轮新增**：Dreaming 三阶段（consolidation_stages.rs）、decision_log 独立压缩、Arena/Team 骨架、VFS 乐观锁、apply_patch_claude、provenance 四级 — 均已合入 main 并测试通过

### 多 Agent / 传统优势
- [x] subagent lifecycle + mailbox/bus/task_claim（已接线）、fleet 控制平面、worktree_gate(Tower merge)、adversarial_verify（库在）
- [x] 静态分析套件 crates/staticanalysis（tree-sitter/taint/callgraph/... 齐全）
- [x] 无人值守：resilience.rs + headless_gate + event_stream/replay（已接线）
- [x] 成本治理 CostBudget 双阈值（[更正]是"硬告警"非"硬停"，不阻断执行）
- [x] MCP client + server 双形态 + ACP；跨 backend 故障转移 + circuit_breaker
- [x] OTel/Prometheus（feature-gated）

### 性能 / 资源 / bug
- [x] 真实 BPE tokenizer（tiktoken cl100k/o200k，已统一接线）
- [x] BM25 工具延迟加载、大输出路由、tool_output_receipts、truncate 7 天自清理
- [x] prefix cache 软缝 + 新→旧裁剪（[更正] ephemeral cache_control 未注入，仅 monitor）
- [x] app-server RwLock 并发（[更正] 报告标 [ ] 是标反，已实现）
- [x] 记忆索引 decay+evict 回归测试（[更正] 报告标 [ ] 是标反，已具备）
- [x] Linux Landlock 真修复（[更正] 历史"仅注释未接线"记忆已过期，本轮确有 raw syscall 实现）
- [x] clippy 改善（[更正] 实测 97 warnings 非 114，未全绿但改善）

## 二、部分实现 / 孤立未接线（模型够不着，需补接线）

- [部分] **session_search 工具**：文件 + 测试齐全；2026-08-18 已在 `crates/tui/src/core/engine/tool_setup.rs:149` 注册 `with_session_search_tool()`（gate 在 `vector-memory` feature，因模块级 `#![cfg(feature="vector-memory")]` 耦合），**编译已验证通过、待 commit+push**（#874 进行中）。注：未开 vector-memory 时该工具不注册——与跨会话检索默认可用性仍有张力，列为后续观察项。
- [部分] **prompt_injection 扫描器**：`prompt_injection/mod.rs`(239行) 存在，grep 零消费方 → 未包裹 fetch_url/web_search 等外部内容。
- [部分] **codebase.rs 语义索引**：库在(1111行)，无 `codebase_search` agent 工具 → 模型够不着。
- [部分] **aggregator 结果聚合**：仅自身单测，无运行时消费（decomposer 已接线）。
- [部分] **adversarial_verify**：库在，未接 spawn 主链路。

## 三、确缺失（声明标 [ ] 正确，或明确虚构）

- [ ] 子智能体断点续跑 resume、异构外部 agent 后端（codex/claude-code）
- [ ] loop_guard 补全局重复计数 / 自适应调用上限 / thinking 复读（当前仅 5 局部检测器）
- [ ] 记忆索引**规模上限**回归测试（decay/evict *阈值* 测试已存在 vector.rs:981/1025/1039/1064，但"索引条目数量上限/规模护栏"维度缺失）
- [ ] 记忆注入冻结快照、情景记忆时间轴视图
- [ ] Trajectory 飞行记录仪（有 event_stream/replay 轻量替代）
- [ ] Code Mode（js_execution 已走 OS 沙箱，非裸执行）
- [ ] 会话树 TUI（session_manager 仍线性）
- [ ] 自学习/技能工坊（skill_state 仅 enabled/disabled 二态）
- [ ] cli 安全态势自检（doctor.rs 仅健康诊断）
- [ ] 技能自生产闭环、KV-cache 友好工具披露、custom modes、microcompaction/receipts 收敛

## 四、报告失真汇总（必须纠正的误报）

| 声明 | 实际 | 失真类型 |
|---|---|---|
| 密钥泄漏 10 处→环境变量已清零 | **仍 11 处真实 `sk-` 硬编码**（config.env 生效 fallback、benchmark/*.sh、report/TEST_RESULTS.md 等） | **虚构已清零** → 真缺口 |
| AMBIGUOUS_MATCH / TARGET_NOT_REGULAR_FILE 错误码已接线 | 全库 0 命中，file.rs 仅文本错误 | 虚构 |
| verification_gate / stop_intent_guard / resolve_at_imports / untrusted.rs 本轮新增已实现 | 符号/文件均不存在 | 虚构 |
| error_taxonomy::tool_codes 6 错误码 | 全库 0 命中 | 虚构 |
| goal_drift benchmark 化 | 文件/测试不存在 | 虚构 |
| engine.rs 3682→2800，拆出 context_management.rs/shell_dispatch.rs | 实际 3937 行，两模块不存在 | 行数+路径失真 |
| token 启发式"仅剩 2 处" | 实际 ≥6 处 | 数字失真 |
| codebase.rs 675/720 行 | 实际 1111 行 | 行数失真 |
| js_execution "裸 Node 片段" | 已走 OS 沙箱 | 描述失真 |
| CostBudget "双阈值硬停" | 实际双阈值硬告警，不阻断 | 语义失真 |
| session_search 已接线 | 孤立未接线 | [更正] 此前误标 |
| app-server Mutex 改并发 [ ] | 已改 RwLock | [ ] 标反 |
| 记忆索引回归测试 [ ] | 已具备 | [ ] 标反 |
| clippy 114 warnings / [ ] | 实测 97 | 数字失真 |

## 五、下一轮值得做的真实缺口（开 issue 用）

**P0（低成本高价值，且报告误报"已实现"）** — 均已开 issue 并**已实施合入 main**（2026-08-18）
1. 细粒度工具错误码体系（error_taxonomy::tool_codes）— 报告虚构已实现，实为真空白 → **#872** ✅ 已实施：`crates/tui/src/error_taxonomy/tool_codes.rs`（5 码 + `[CODE]` 前缀接线 file.rs/spec.rs），commit `d4a2c7b` 已 push
2. 清理仓库硬编码密钥（真实安全缺口）— 原报告虚假声明"已清零" → **#873** ✅ 已修复：config.env/benchmark/*.sh/evals/*.sh/README/TEST_RESULTS.md 改为环境变量引用+脱敏，commit `4774a79` 已 push，4 个真实 key 串全仓库零命中
3. session_search 接线 — 已实现 90% 只差注册 → **#874** ✅ 已接线 tool_setup.rs:149，commit `5795c60` 已 push

**P1（补齐部分实现 / 竞品对标）**
4. prompt_injection 接线包裹 fetch_url/web_search
5. codebase_search agent 工具（暴露语义索引）
6. verification_gate 真实现（编辑前行为级闸门）— 注意区别于 GoalGate
7. stop_intent_guard 真实现（协议违规 nudge）
8. loop_guard 补全局重复/thinking 复读检测
9. 子 agent resume / adversarial_verify 接 spawn 主链

**P2（工程/形态）**
10. godfile 拆分（engine.rs 3937 / turn_loop 3653 / ui_event_loop 3494 / widgets 3120 仍超限）
11. Trajectory 飞行记录仪、会话树、技能自生产闭环、cli 安全自检

## 六、结论

本轮报告整体**夸大完成度**：9 域 + 升级规划中大量 `[x]` 实为虚构或孤立未接线。
确证缺失且值得做的真实缺口集中在 **P0 三项**（错误码体系、密钥清理、session_search 接线）——
这三项若不做，项目在"工具错误可机读""密钥零泄漏""跨会话检索可用"上仍是缺口。
上一轮 #870/#871 我已误判其中两项（错误码、session_search），本次已纠正并应优先回填。
