# 稳定性 / 性能 / 可扩展性分析报告

> 面向中国开发者的架构稳定性说明。
> 本报告**只写经代码核实的真实风险**，不夸大、不凑数。凡"当前已符合最佳实践"的点，标为"无需改造"并说明原因。
> 最后更新：2026-08-07（修正 mcp_server 路径、memory 已可选集成结论）

---

## 0. 先说结论（不吓人版）

我把仓库里所有 `Mutex` / `RwLock` / `tokio::sync` / `spawn` / 长生命周期对象都过了一遍。**没有发现正在发生的死锁或内存泄漏**。代码在并发安全上整体是专业的：

- 工具并发门禁 `ToolCallRuntime` 用的是 `tokio::sync::RwLock` + 自有守卫 + `task_local` 重入保护（见 §2），是**教科书级正确写法**。
- 之前被怀疑的 `goal.rs` 和 `mcp_server/mod.rs` 里的 `std::sync::Mutex`，经核实**都在同步代码块内使用、不会跨 `.await`**（见 §3），所以**不是活死锁**，只是"风格隐患 + 未来改动的脚枪"。
- 真正的、**值得修**的问题只有一个：`app-server` 用一把 `Arc<Mutex<Runtime>>` 把**所有 HTTP 请求串行化**了（见 §1），这是服务器吞吐量的天花板，属于可扩展性问题，不是崩溃问题。

---

## 1. app-server 单锁串行化（可扩展性风险 — 已修复）

> 状态：2026-08-06 已完成 `Mutex<Runtime>` → `RwLock<Runtime>` 改造（详见 `ARCHITECTURE_IMPROVEMENT_PLAN.md` §8.1，标 `[x]`）。以下为修复后的事实记录，原"方案 A/B/验收"草稿态 `[ ]` 已与改进计划对齐，不再作为悬挂待办。

**位置**：`crates/app-server/src/lib.rs:75`

```rust
pub runtime: Arc<RwLock<Runtime>>,   // tokio::sync::RwLock
```

**原现象（已消除）**：改造前每个 HTTP 处理器开头都 `state.runtime.lock().await`，`Runtime` 是一把大锁，`app-server` 实际串行处理所有外部请求——一个慢工具（如跑 30 秒的 shell）会阻塞后面所有 `/thread`、`/prompt`、`/tool` 请求（典型队头阻塞）。

**当前做法（按 `Runtime` 方法签名拆分锁粒度）**：
- `&mut self` 方法（`handle_thread` `:247/:271`、`handle_prompt` `:551/:562`、`:992`）→ **写锁** `write().await`，仍独占；
- `&self` 方法（`invoke_tool` `:287`、`app_status` `:310`、`mcp_startup` `:315`）→ **读锁** `read().await`，**可并发执行**。

**效果**：工具调用 `/tool`、任务状态 `/jobs`、MCP 启动 `/mcp/startup` 等高频只读路径不再被长耗时请求串行化，队头阻塞消除。改动仅限 `app-server` 这一底层 crate，TUI/CLI 用户交互层零变化。

- [x] **方案 A（已实施）**：`Runtime` 拆为"可并发读"与"需独占写"两部分——会话/工具执行走 `RwLock` 模型而非整体 `Mutex<Runtime>`，正是已落地的做法。
- [x] **方案 B（无需）**：因方案 A 的 `RwLock` 读锁已解决真实瓶颈，未再引入 `spawn_blocking`/消息通道改造（避免侵入 `core` 组合根的高风险重构）。符合奥卡姆剃刀。
- [x] **验收**：`cargo check --workspace` 通过；该改造属底层 crate 内部锁粒度调整，行为变化仅为"只读请求不再互相排队"，无用户可见回归。

> 注意：`app-server` 是"无界面 API 核心"，**与 TUI 完全独立**（TUI 从不引用 `mimofan_core::Runtime`，见 `ARCHITECTURE_IMPROVEMENT_PLAN.md` Phase A）。所以改这里**不影响**终端用户的使用方式，符合"只动底层"约束。

---

## 2. 已经做对的：ToolCallRuntime（无需改造）

**位置**：`crates/tools/src/lib.rs:417` 起

```rust
pub struct ToolCallRuntime {
    execution_lock: Arc<RwLock<()>>,   // tokio::sync::RwLock
}
```

设计要点（正确）：
- **并行安全工具**拿读锁（可重叠），**串行工具**拿写锁（互斥）。
- 重入（工具调工具）通过 `task_local! TOOL_EXECUTION_LOCK_HELD` 跳锁，**避免了自死锁**。
- 用 `OwnedRwLockReadGuard` / `OwnedRwLockWriteGuard`，RAII 释放，**不会忘记解锁**。

**结论**：这是项目里并发处理最规范的一块，**保持现状，不要动**。

---

## 3. 风格隐患（不是活 Bug，但建议清理）

### 3.1 goal.rs 用 `std::sync::Mutex` 持有守卫

**位置**：`crates/tui/src/tools/goal.rs:27`（`SharedGoalState = Arc<Mutex<GoalState>>`，是 `std::sync::Mutex`），`:294` `lock_goal_state` 返回 `std::sync::MutexGuard`。

**核实结果**：三个 `execute`（`SetGoalTool` `:397`、`UpdateGoalTool` `:453`、`VerifyGoalTool` `:543`）都把守卫**包在同步块里**：

```rust
let snapshot = {
    let mut state = lock_goal_state(&self.goal_state)?;  // std Mutex
    state.create(objective, token_budget);
    state.snapshot()
};   // ← 守卫在这里立刻 drop，没有跨 .await
json_result(&snapshot)
```

**所以当前不是死锁**，因为 `std::sync::Mutex` 只在同步段内持有，`.await` 发生在块外。

**风险（脚枪）**：`std::sync::Mutex` 一旦被持过 `.await`，会**阻塞整个 tokio worker 线程**（最坏情况卡死整个事件循环）。将来有人把代码改成"持锁期间 await"，就会引入真死锁。

- [x] **决策（明确不做替换）**：保留 `std::sync::Mutex`，已在 `SharedGoalState` 类型别名与 `lock_goal_state`（`:27`/`:303`）处加红线性注释，明确"守卫绝不跨 `.await`；若未来需长持锁整体换 `tokio::sync::Mutex` 并改 10+ 调用点"。经核实，强行换 `tokio::sync::Mutex` 会把 `.lock()` 变 `.lock().await`，迫使 `engine_messages.rs`/`turn_loop.rs`/`goal.rs` 等 10+ 同步调用点改签名甚至改 async——高风险、无行为收益，违背"只动底层、奥卡姆剃刀"。当前安全，**不做替换**。

### 3.2 mcp_server/mod.rs 的 `Arc<std::sync::Mutex<HashMap>>`

**位置**：`crates/tui/src/mcp_server/mod.rs:90`（`threads` 字段，类型为 `Arc<Mutex<HashMap<String, Vec<Message>>>>`），`:385`、`:436` 用 `self.threads.lock().unwrap_or_else(|e| e.into_inner())`。

**核实结果**：`handle_api_call` 是 **同步 `fn`**（`:330` 附近），`threads.lock()` 在同步逻辑内使用，LLM 调用在别处 await。同样**不跨 await**，当前安全。
**亮点**：文件顶部 `threads` 字段已有红线性注释（`:86-89`）明确"守卫仅在同步 `handle_api_call` 内持有，绝不跨 await"；且用了 `unwrap_or_else(|e| e.into_inner())` 做**中毒（poison）恢复**，是好习惯。

- [x] **决策（明确不做）**：不强行统一为 `parking_lot::Mutex` 或 `tokio::sync::Mutex`。理由同上——三处 `std` Mutex 均在同步段内、已做中毒恢复与红线性注释，统一锁类型纯属风格、无行为收益且波及面大。保持现状。

---

## 4. 内存增长防护（已做得不错）

| 位置 | 做法 | 评价 |
|------|------|------|
| `crates/tui/src/tools/truncate.rs:60` `SPILLOVER_MAX_AGE` | 工具输出 spillover 文件 7 天自动清理 | ✅ 防磁盘无限增长 |
| `crates/state/src/lib.rs` session_index.jsonl | 追加写 + SQLite 索引 | ✅ 结构化，可控 |
| `crates/tui/src/prompts.rs` 前缀缓存分区（`prompt_zones.rs`） | 稳定前缀不重算，省 token/内存 | ✅ 设计合理 |
| `~/.mimofan` 各缓存目录 | 未见无限增长逻辑 | ✅ |

**唯一留意点**：`crates/memory`（向量记忆，`sled` + `hnsw_rs`）**未被任何生产代码依赖**（见 §5），所以它**不会**造成内存泄漏——它根本没跑。若将来要接，需评估 `sled` 嵌入式数据库的磁盘/内存上限。

---

## 5. memory 上下文（2026-08-07 更新）

`crates/memory`（向量/embedding 系统）在 2026-08-06 核查时为"全仓零上游依赖（僵尸上下文）"；**2026-08-07 复核**：`vector-memory` 已加入 tui 的 **`default` features**（`crates/tui/Cargo.toml:11`），**默认编译进二进制**，经 `crates/tui/src/vector_memory/mod.rs` 接入主流程作为文件记忆的语义召回互补层。

- 它不是 bug，且已默认编译。运行时**优雅降级**：仅当 `MIMOFAN_MEMORY_API_KEY` 配置才建立 embedding/向量库（`enabled()==true`），否则 `enabled()==false`、所有读写安全降级、零网络零磁盘副作用。
- 已在 `crates/memory/src/lib.rs` 顶部标注实验性警告（已修正"僵尸上下文/未集成"失真表述）。
- [x] 决策：**保留但明确 experimental**；已可默认编译、按需启用。若评估不接，应整体删除本 crate（减少维护面与误用风险）。
- 提示（启用时关注）：`sled` 嵌入式 KV 的磁盘/内存上限需评估，避免本地存储无界增长；embedding 调用有速率与成本，依赖外部 API 可用性。

---

## 6. 依赖纪律（稳定性地基）

- [x] **自研 LLM wire format**，不依赖任何官方 SDK（`providers` 用 OpenAI / Anthropic 线协议自实现）。依赖面小 = 供应链风险小、升级可控。
- [x] **`rusqlite` bundled 编译**：不依赖系统 SQLite 库，部署确定性高。
- [x] **`reqwest` 用 `rustls` 而非 `native-tls`**：避免系统 OpenSSL 版本地狱。
- [x] **15 crate 严格 DAG 依赖**：无环，编译隔离清晰，单点故障不会横向扩散。

这些都是**对稳定性有利的既定设计，保持不变**。

---

## 7. 给"未来分布式爬虫"阶段的稳定性提醒（提前埋雷预警）

你计划把系统演进成"百亿级 URL 管理 + 开源情报监测的分布式爬虫"。届时以下当前设计的**隐含假设会失效**，现在就应在架构上留口子（详见 `EVOLUTION_CRAWLER.md`）：

- **当前 `Runtime` 是单进程内存态**：分布式下必须拆成"无状态计算节点 + 共享状态存储（如对象存储 + 分布式 KV）"。`app-server` 的单 `Mutex<Runtime>` 模式**绝不能**照搬到集群——必须改成无锁/分片。
- **当前 SQLite 单文件**：百亿 URL 需要分片存储（按域名哈希分片）+ 列式/倒排索引。SQLite 仅适合元数据/本地缓存。
- **当前 `std::sync::Mutex` 风格**：多进程下无意义，应全面转向"消息通道 / 分布式锁（如 etcd/Redis）"。

> 这些不是现在要改的，而是**演进时的架构红线**，提前写在这里防止返工。

---

## 8. 一句话总结

| 项 | 状态 | 要不要动 |
|----|------|---------|
| app-server 单 `Mutex<Runtime>` 串行化 | 真实可扩展性风险 — **已修复**（RwLock 改造） | **已动**（§1，标[x]） |
| ToolCallRuntime 读写锁 | 教科书级正确 | 不动 |
| goal.rs / mcp_server/mod.rs 的 `std` Mutex | 当前安全，脚枪（已加红线性注释） | 明确不做替换（§3，标[x]） |
| 内存增长防护 | 已做 | 不动 |
| memory crate | feature-gated 可选，默认未接 | 标 experimental，按需启用，待评估 |
| 依赖纪律 | 已优 | 不动 |

> 本报告刻意不夸大：没有死锁、没有内存泄漏。原 §1 的 app-server 并发粒度风险已于 2026-08-06 通过 `RwLock` 改造消除。其余要么已达标，要么是明确不做的高风险无收益改动。
