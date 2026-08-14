# Queue 队列能力规划（任务编排 / 工作流 / checkpoint / 多 agent 原子 claim / 次级模型）

> 本规划从 GitHub issue 中筛选「queue 队列 / 任务调度 / 长程任务」类 OPEN 待办，经源码权威核验（grep / 读文件）后细化，便于后续直接派工实施长程任务。
> **核对纪律（复用 `ARCHITECTURE_IMPROVEMENT_PLAN.md`）**：本文档所有「已落地」结论均以 `grep` / 读源码亲核 `main` 分支代码为准，不采信二手对标清单。若发现与代码不符，先 `git grep` 复核再改文档。

最后更新：2026-08-14

---

## 0. 已完成前提（不要再派工，仅作下游依赖底座）

- **#654 Goal 队列治理（完整落地）** — `crates/tui/src/tools/goal.rs`
  - `GoalQueue`（行 549）、`SharedGoalQueue = Arc<Mutex<GoalQueue>>`（行 570）
  - `enqueue()`（行 613）含 `priority: u8`（行 515/617）、依赖 `blocked_by`（行 519/625）
  - 状态机 `QueueStatus`：`Queued`/`Active`/`Paused`/`Done`（行 488-510），串行调度（行 476-479）
  - `promote`（行 784）、`pause`(863)/`resume`(879)/`cancel`(898)/`complete`(847)
  - 复用 todo DAG 依赖（`blocked_by` 即依赖边）
  - `goal_list` 工具（行 1277）可见队列与状态、预算消耗；单测覆盖调度（行 1804-1911）
  - **结论**：Goal 多目标排队/优先级/调度已完整，**不要重做**。
- **#653 统一次级模型抽象（OPEN，但相关）** — `secondary_model` 抽象是 T-Q5 的依赖，见下。

---

## 1. 能力地图（依赖关系）

```
[#654] Goal 队列(已落地) ──────────────→ 队列调度基础
        │                                │
        ↓                                ↓
[#700] 工作流编排 DAG (T-Q1, P1)    [#631] Plan 跨进程 checkpoint (T-Q2)
        │                                │
        ├─→ [#665] 跨 agent 原子 claim (T-Q3)
        ├─→ [#693] Goal 独立 LLM judge (T-Q4)
        └─→ [#724] 次级模型自动策略 (T-Q5, 依赖 #653)
```

依赖箭头自洽：**T-Q1 依赖 #654**；**T-Q2 依赖 state 持久化层**；**T-Q3 在既有 task_manager 持久化之上补**（勿重做持久化）；**T-Q5 依赖 #653**。

---

## 2. 可执行任务清单

### T-Q1. [#700] 可编程工作流编排（parallel / pipeline / DAG 原语） — P1，排最前

- **为什么**：当前 agent 工具单次仅能起一个子 agent，无 parallel/pipeline/DAG 原语（`parallel.rs` grep `workflow|pipeline|DAG|stall|journal` 零命中；`registry.rs` grep `workflow` 零命中）；长程/批量任务需要声明式编排。
- **落点文件**：新增 `crates/tui/src/tools/workflow.rs`（声明式 JSON/YAML DAG 引擎）+ `registry.rs` 注册
- **依赖**：#654（队列调度基础，已落地）
- **验收标准**（每项 grep / `cargo test` 可验证）：
  - [ ] 节点为子 agent 调用，支持 `parallel` / `sequential` / 条件分支
  - [ ] 复用已有 `subagent/manager.rs:57` `with_default_token_budget` 预算域继承、`helpers.rs:298-315` `budget_exhausted`
  - [ ] 复用 `parser.rs:585` `parse_optional_worktree_request` 工作树隔离
  - [ ] stall 重试（当前 `subagent/` 仅有心跳超时 `manager.rs:17`，无停滞→重试）
  - [ ] journal 续跑（当前 `subagent/persistence.rs` 零 `resume|journal|checkpoint`；Fleet `fleet/ledger.rs:283 rebuild_state` 仅供 Fleet，不服务 subagent 编排）
- **反模式**：
  - 不要重新造 `workflow-budget`（已真实接线，见 #700 核实表）
  - 不要启用 `tools/parallel.rs:1-9` 故意未注册的 `multi_tool_use.parallel`（注释明写 "intentionally no longer registered"）
  - `/swarm`（`commands/groups/core/swarm.rs:29`）被显式 gate 关闭，等 Fleet 落地，不要当成已有能力
  - 不必上 JS 沙箱（Lua/WASM 脚本沙箱作为后续可选增强）
- **易误判点**：`state/src/lib.rs:442-525` 的 `workflow_runs`/`control_node_runs` 等表是 **RL 训练 trace 持久化层**（配合 teacher_candidates），**非面向用户的工作流引擎**，不要误判为已落地。

### T-Q2. [#631] Plan 跨进程 checkpoint 持久化

- **为什么**：`PlanSnapshot` 仅会话内内存恢复（`engine.rs:100` 注入 prompt 文本），进程退出即丢；`grep "write.*plan|save.*snapshot|fs::write.*plan"` 在 `crates/tui/src/` 零命中，无落盘持久化。长 Plan 执行被中断（崩溃/重启）后无法从断点续跑。
- **落点文件**：`crates/tui/src/tools/plan.rs`（`PlanSnapshot` 行 171、`snapshot()` 行 437、`apply_snapshot()` 行 469）+ state 持久化层（SQLite/文件）
- **依赖**：state 持久化层（已有 SQLite 基础设施，复用之）
- **验收标准**：
  - [ ] `PlanSnapshot` 落 SQLite/文件（非仅内存 prompt），重启可恢复
  - [ ] 模拟进程退出后重启能从断点续跑（构造 fixture + 集成测试）
  - [ ] grep `fs::write.*plan` 或等价持久化调用有命中
- **反模式**：不要只做内存快照冒充持久化；不要重建 state 持久化层（复用现有 SQLite）。

### T-Q3. [#665] 跨 agent 原子 claim（Kanban 持久化工作板）

- **为什么**：`tools/todo.rs:338` 用 `SharedTodoList = Arc<Mutex<TodoList>>`，仅 session 内同步，**跨 agent 进程无共享**；无 column/swimlane/WIP 限额、无跨 agent 原子认领（`todo.rs:244` 仅有 advisory 注释）。多 agent 并行会重复派活。
- **落点文件**：在既有 `task_manager/mod.rs:777` `queue.json`、`:1527` `{id}.json`、`tools/subagent/persistence.rs` `state.json` 持久化之上，补跨进程原子 claim（SQLite 行锁 / 分布式锁）
- **依赖**：无（持久化层已存在，**勿重做**）
- **验收标准**：
  - [ ] 两 agent 并发 claim 同一 task 仅一个成功（SQLite 行锁 / 分布式锁保证原子性）
  - [ ] 新增 `claim|reserve|take` 方法（当前 todo.rs grep `claim|atomic|cross.*agent` 零命中）
  - [ ] 可选：column/swimlane/WIP 限额（看板语义）
- **反模式**：不要重做持久化层（queue.json + {id}.json 已落地）；不要只在 session 内存做 claim（跨进程无效）。

### T-Q4. [#693] Goal 独立 LLM judge

- **为什么**：`goal.rs:116` 完成判定由被判定的模型**自报**（`completion_verification` 由模型自己写入"完成证据"，行 103/339-348），无独立 LLM 二次判定，长程目标易过早/误判完成。
- **落点文件**：`crates/tui/src/tools/goal.rs`（完成判定逻辑）+ 复用 `crates/tui/src/reviewer/` 模块
- **依赖**：无（reviewer 模块已存在可复用）
- **验收标准**：
  - [ ] Goal 完成判定改由独立 LLM 二次判定（非自报）
  - [ ] grep `judge|Judge` 在 goal 上下文有独立 judge 调用（当前仅 reviewer 通用模块与 subagent `self_report`）
  - [ ] 长程目标不因自报而提前终止
- **反模式**：不要重复造 reviewer 模块；不要仍用模型自报当完成判定。

### T-Q5. [#724] 次级模型自动策略（子任务默认便宜模型）

- **为什么**：`subagent_model_overrides()`（`config.rs:1762`）需用户显式配置才有，无内置默认；`fast_mode`（`settings/mod.rs:247`）是用户手动 `/fast` 切换，非子任务自动选便宜模型。长程批量任务缺乏成本自动优化。
- **落点文件**：`crates/tui/src/config.rs` + `runtime_threads/mod.rs:1775`（override 读取点）+ 复用 `secondary_model` 抽象（#653）
- **依赖**：#653 统一次级模型抽象（OPEN，需先收敛 `seam_model`/`cheap_tier` 三处）
- **验收标准**：
  - [ ] 派发子任务时自动选便宜模型（无需用户手动配置）
  - [ ] grep `cheap|cheapest|default.*cheap` 在 config/runtime 有自动选择逻辑（当前零命中）
  - [ ] primary/secondary 覆盖机制仍可用
- **反模式**：不要重复造 `secondary_model` 抽象（#653 已在做收敛）；不要破坏现有 `subagent_model_overrides` 显式配置路径。

---

## 3. 风险与顺序建议

1. **T-Q1 工作流编排排最前** —— 它是长程任务规划与运行的核心原语，且依赖已落地的 #654 Goal 队列；stall 重试 + journal 续跑直接提升长程任务韧性。
2. **持久化层复用** —— T-Q2 / T-Q3 都依赖既有持久化（state SQLite、task_manager queue.json），只补缺口，勿重建。
3. **#653 阻塞 T-Q5** —— 次级模型抽象未统一前，T-Q5 的"默认便宜"策略无处挂接，需先推 #653。
4. **易误判点** —— `state` 的 `workflow_runs` 表是 RL trace 层非用户工作流引擎；`/swarm` 被 gate 关闭；`multi_tool_use.parallel` 故意未注册——三者都不能算已有能力，T-Q1 是从零建。
5. **跨 agent 并发安全** —— T-Q3 的原子 claim 是 Agent Teams 并行开发（见 `CODEBUDDY.md`）的前提，避免多成员重复派活，建议尽早做。
