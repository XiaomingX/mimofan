# MY_PLAN_0819.md — 第三轮清单实证对照（make-plan，2026-08-18）

> 输入：`my-plan.md`（9 域 + 升级规划 + Benchmark 报告 Before/After）+ `mimofan_upgrade.md` + `草稿事实清单`。
> 这是**第三份**待核实清单，与前两份（MY_PLAN_0817 / 0818 已覆盖）大量重叠。
> 方法：复用前两轮 6 个 Explore agent 的实证结论 + 本轮对"新出现/说法有变"的落点亲自 Grep 核验。
> 结论以磁盘证据为准；凡与前两份清单**同一失真**处，本表标注 `[重复失真]` 不再重派 agent。

## 〇、本轮清单的"新声明"亲验结论（Grep 实测）

| # | 新清单声明 | 实测 | 判定 |
|---|---|---|---|
| N1 | `StateStore::search_messages` 本轮新增跨会话内容级召回 | `StateStore` 真实存在（`crates/state/src/lib.rs`），但**无 `search_messages` 方法**（grep 零命中） | **虚构落点** |
| N2 | 升级规划 P1 `recall` 工具（FTS5/向量检索暴露工具面） | `fn recall`/`with_recall_tool`/`RecallTool` 全库零命中 | **未实现**（标 `[ ]` 倒诚实，但与 my-plan.md 混谈 session_search 易误导） |
| N3 | 升级规划 1.3 `error_taxonomy::tool_codes` 6 码已接线 + 扩面 P1 | 同前轮：**全库零命中**（#872 已开 issue） | **虚构** `[重复失真]` |
| N4 | `verification_gate`/`stop_intent_guard`/`resolve_at_imports`/`untrusted.rs` 标 [x] 已实现 | 符号/文件全库零命中（shell_dispatcher 是另一回事） | **虚构** `[重复失真]` |
| N5 | godfile `engine.rs 3682→2800` 拆出 `context_management.rs`/`shell_dispatch.rs` | 实际 3937 行，两模块不存在（仅 `acceptance.rs` 测试函数名含 context_management） | **行数+路径失真** `[重复失真]` |
| N6 | `session_search` 标 [x] 已接线（升级规划 §2 / my-plan.md 域1） | 前轮 Explore 查 main 为"零调用点孤立未接线"；**本轮已派 agent 接线 `tool_setup.rs:149`，待编译验证 + commit** | **前误报，本轮纠正中** |
| N7 | `memory_nudge` 独立模块 / `protocol_violation` 模块 | `loop_guard/mod.rs:537 periodic_memory_skill_nudge` 真实存在（非独立模块）；`protocol_violation` 符号零命中 | **命名失真**：nudge 真但非独立模块；protocol_violation 虚构 |
| N8 | Benchmark 报告 D09 "探针找旧路径 symbol_index/，能力已迁 staticanalysis" | 与 Explore-4 一致：tui symbol_index 旧路径已删，staticanalysis 更强 | **属探针滞后非缺口** `[重复失真]` |

## 一、第三份清单中"确已实现"且与前轮一致（[x] 属实）

- [x] Linux Landlock 真修复（raw syscall 444/445/446，非注释）— `sandbox/landlock.rs` / `sandbox/mod.rs`
- [x] 真实 BPE tokenizer（tiktoken cl100k/o200k，LazyLock 单例）— `crates/tui/src/tokenizer/mod.rs`
- [x] BM25 延迟加载 / 大输出路由 / tool_output_receipts / truncate 7 天清理
- [x] loop_guard 五维检测（已接线 turn_loop，24 测试）
- [x] subagent lifecycle + fleet + worktree 隔离 + adversarial_verify（库在）
- [x] 压缩三范式 + objective 重注 + drift_check
- [x] app-server RwLock 并发（[更正] 报告误标 [ ]）
- [x] 记忆 decay/evict 回归测试（vector.rs:981/1025/1039/1064，[更正] 报告误标 [ ]）
- [x] prefix_cache 软缝（[更正] ephemeral cache_control 未注入，仅 monitor）
- [x] clippy 改善（实测 97 warning 非 114，未全绿）
- [x] `search_sessions_fulltext`（CLI/HTTP 层跨会话检索，真实存在但**非 agent 工具面**）

## 二、第三份清单中"标 [x] 但实为虚构/标反"的项（最高风险）

> 这些项在第三份清单里被写成"本轮已实现"，且**与前两份清单同一失真**——说明失真已三级叠加，必须硬性纠正。

1. `error_taxonomy::tool_codes` 6 码（N3）→ 真空白，#872
2. `verification_gate` / `stop_intent_guard` / `resolve_at_imports` / `untrusted.rs`（N4）→ 符号零命中
3. `StateStore::search_messages`（N1）→ 方法不存在
4. `AMBIGUOUS_MATCH` / `TARGET_NOT_REGULAR_FILE` 守卫（N3 子集）→ 零命中
5. godfile `engine.rs 3682→2800` 拆出两模块（N5）→ 实为 3937 行
6. 密钥泄漏"已清零"（Benchmark 报告 §5 / my-plan.md §5）→ **仍 10+ 处真实硬编码**，#873
7. `session_search` 标 [x] 已接线（N6）→ 前轮孤立未接线，本轮纠正中

## 三、第三份清单中"标 [ ] 正确 / 确缺失"的项（值得做）

**P1（补齐部分实现 / 竞品对标）**
- [ ] `recall` 工具暴露（N2，FTS5/向量检索工具面）——与 session_search 是两条路径，不重复
- [ ] prompt_injection 接线包裹 fetch_url/web_search（扫描器孤立）
- [ ] codebase_search agent 工具（codebase.rs 库在模型够不着）
- [ ] verification_gate 真实现（编辑前行为级闸门）
- [ ] stop_intent_guard 真实现（协议违规 nudge）
- [ ] loop_guard 补全局重复/thinking 复读检测
- [ ] 子 agent resume 断点续跑 + adversarial_verify 接 spawn 主链

**P2（工程/形态）**
- [ ] godfile 拆分（engine.rs 3937 / turn_loop 3653 / ui_event_loop 3494 / widgets 3120 仍超限）
- [ ] 记忆索引规模上限回归（decay/evict 阈值已有，规模护栏缺失）
- [ ] Trajectory 飞行记录仪、会话树、技能自生产闭环、cli 安全自检
- [ ] 2 个 flaky 测试根治（gadget_chain 并行竞态 / session daemon PTY echo）
- [ ] `probe_recall` example 同名输出冲突（memory 与 tui 两 crate）

## 四、与前两份清单的失真一致性结论

第三份清单（my-plan.md / mimofan_upgrade.md / Benchmark 报告）**没有引入任何新的真实能力**，只是在前两份已失真的基础上：
- 把前轮已证伪的 `verification_gate`/`stop_intent_guard`/`tool_codes`/`StateStore::search_messages` **再次写成 [x]**；
- 把已纠正的 `session_search 已接线`、`engine.rs 拆出两模块` **再次写成 [x]**；
- Benchmark 报告里的"密钥已清零""AMBIGUOUS_MATCH 已接线"同样失真。

**建议**：三份清单应合并为单一真相源（MY_PLAN_0818.md 已是），并明确标注"[x] 需以磁盘证据为准，凡本表标 `[重复失真]` 者一律视为未实现"。

## 五、本轮已落地 / 进行中的交付

- [进行中] **P0-3 session_search 接线**：agent-24862d49 已在 `crates/tui/src/core/engine/tool_setup.rs:149` 注册 `with_session_search_tool()`（gate 在 `vector-memory` feature），改动未 commit，编译验证中。
- [已开 issue] #872 工具错误码体系（虚构）、#873 硬编码密钥（安全缺口）、#874 session_search 接线。
- [已登记] loopx goal `agent-mimofan-goal`：11 个 P0/P1/P2 todo。

## 六、给用户的执行建议（make-plan 产出）

1. **不要信任三份清单中的 [x] 声明**，以 MY_PLAN_0818.md + 本表为准。
2. **立即合并 P0-3**（session_search 接线已验证编译后 commit + push）。
3. **P0-1/P0-2**（#872/#873）留作下一批真实缺口，已开 issue 跟踪。
4. **P1 项**（recall 工具 / prompt_injection 接线 / codebase_search / verification_gate 真实现 / stop_intent_guard 真实现）按奥卡姆逐个立项，避免再次"虚构已实现"。
5. **统一真相源**：将三份清单收敛为一份，删除重复的失真 [x]，避免后续轮次继续叠加误报。
