# 稳定性 / 性能 / 可扩展性分析报告

> 面向中国开发者的架构稳定性说明。
> 本报告**只写经代码核实的真实风险**，不夸大、不凑数。凡"当前已符合最佳实践"的点，标为"无需改造"并说明原因。
> 最后更新：2026-08-06

---

## 0. 先说结论（不吓人版）

我把仓库里所有 `Mutex` / `RwLock` / `tokio::sync` / `spawn` / 长生命周期对象都过了一遍。**没有发现正在发生的死锁或内存泄漏**。代码在并发安全上整体是专业的：

- 工具并发门禁 `ToolCallRuntime` 用的是 `tokio::sync::RwLock` + 自有守卫 + `task_local` 重入保护（见 §2），是**教科书级正确写法**。
- 之前被怀疑的 `goal.rs` 和 `mcp_server.rs` 里的 `std::sync::Mutex`，经核实**都在同步代码块内使用、不会跨 `.await`**（见 §3），所以**不是活死锁**，只是"风格隐患 + 未来改动的脚枪"。
- 真正的、**值得修**的问题只有一个：`app-server` 用一把 `Arc<Mutex<Runtime>>` 把**所有 HTTP 请求串行化**了（见 §1），这是服务器吞吐量的天花板，属于可扩展性问题，不是崩溃问题。

---

## 1. 真正要修的：app-server 单锁串行化（可扩展性风险）

**位置**：`crates/app-server/src/lib.rs:71`

```rust
pub runtime: Arc<Mutex<Runtime>>,   // tokio::sync::Mutex
```

**现象**：每一个 HTTP 处理器（`thread_handler` `:243`、`prompt_handler` `:267`、`tool_handler` `:283`、`jobs` `:306`、`:311`、`:547`、`:558`、`:988`……）开头都 `state.runtime.lock().await`。因为 `Runtime` 是**一把大锁**，`app-server` 其实是**单线程串行处理**所有外部请求的：一个慢工具（比如跑 30 秒的 shell 命令）会**阻塞后面所有** `/thread`、`/prompt`、`/tool` 请求。

**为什么是风险**：
- 对外提供 API 时，这是典型的"队头阻塞（Head-of-Line Blocking）"，吞吐量被单一长任务卡死。
- `tokio::sync::Mutex` 本身是 async 安全的（不会死锁），问题纯粹是**粒度过粗**。

**影响评级**：中（性能 / 可扩展性），不致命。

**改进方向（只动底层，不动用户交互层）**：

- [ ] **方案 A（推荐，低风险）**：把 `Runtime` 拆成"可并发读"的部分与"需独占写"的部分。例如会话索引/配置用 `Arc<RwLock<>>`，工具执行走 `ToolCallRuntime` 已有的读写锁模型，而不是整体 `Mutex<Runtime>`。
- [ ] **方案 B（中风险）**：让 `handle_prompt` 等接口内部用 `spawn_blocking` / 独立任务 + 消息通道驱动 `Runtime`，把"持锁时间"压缩到只做状态变更，长耗时动作交出去。
- [ ] **验收**：写并发压测——同时发 10 个请求，其中 1 个故意慢，确认其余 9 个不排队等待。`cargo test -p mimofan-app-server` 通过。

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

- [ ] **建议**：把 `SharedGoalState` 从 `std::sync::Mutex` 换成 `tokio::sync::Mutex`（零行为变化，纯安全加固），或保持 `std` 但在代码注释里写明"不可跨 await"。属**低风险美化项**，不做也不影响运行。

### 3.2 mcp_server.rs 的 `Arc<std::sync::Mutex<HashMap>>`

**位置**：`crates/tui/src/mcp_server.rs:85`（`threads` 字段），`:380`、`:431` 用 `self.threads.lock().unwrap_or_else(|e| e.into_inner())`。

**核实结果**：`handle_api_call` 是 **同步 `fn`**（`:330`），`threads.lock()` 在同步逻辑内使用，LLM 调用在别处 await。同样**不跨 await**，当前安全。
**亮点**：用了 `unwrap_or_else(|e| e.into_inner())` 做**中毒（poison）恢复**，这是好习惯。

- [ ] **可选**：统一成 `parking_lot::Mutex` 或 `tokio::sync::Mutex`，让全仓锁类型一致。纯风格，不急。

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

## 5. 僵尸上下文（与稳定性间接相关）

`crates/memory`（2736 行向量/embedding 系统）**全仓零上游依赖**（已用 `grep` 确认：没有任何 crate 的 `Cargo.toml` 引用 `mimofan-memory`）。

- 它不是 bug，但**会误导**：让人以为"记忆功能可用"。
- 已在 `crates/memory/src/lib.rs` 顶部标注实验性警告 + `Cargo.toml` 标 `(EXPERIMENTAL: not integrated)`。
- [x] 决策：**保留但明确 experimental**，接入与否待评估。若评估不接，应整体删除本 crate（减少维护面与误用风险）。

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
| app-server 单 `Mutex<Runtime>` 串行化 | 真实可扩展性风险 | **要动**（§1） |
| ToolCallRuntime 读写锁 | 教科书级正确 | 不动 |
| goal.rs / mcp_server.rs 的 `std` Mutex | 当前安全，脚枪 | 可选美化（§3） |
| 内存增长防护 | 已做 | 不动 |
| memory 僵尸 crate | 误导性 | 标 experimental，待删 |
| 依赖纪律 | 已优 | 不动 |

> 本报告刻意不夸大：没有死锁、没有内存泄漏。唯一值得投入的是 §1 的 app-server 并发粒度。其余要么已达标，要么是无行为变化的低风险美化。
