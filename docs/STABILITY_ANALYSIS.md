# mimo-tui 稳定性与性能风险分析

> 从架构师视角审视性能、可扩展性、稳定性方面的风险和改进方向。
> 最后更新：2026-06-28

---

## 一、风险总览

| 风险类别 | 具体问题 | 严重度 | 可能后果 | 状态 |
|----------|----------|--------|----------|------|
| 文件过大 | `ui.rs` 11,412 行 / `main.rs` 9,231 行 | 中 | 可维护性差，改动易引入回归 | 待处理 |
| unsafe 使用 | `settings.rs` 50 处 unsafe 块 | 中 | 内存安全风险 | 待审查 |
| clone 热区 | 非测试代码 2,425 处 `.clone()` | 低-中 | 内存压力、GC 延迟 | 需 profile |
| 工具串行化 | `tool_exec_lock` 全局写锁 | 低 | 工具执行吞吐受限 | 有意设计 |
| 嵌套锁 | `rlm/session.rs` 双层锁 | 低 | 潜在死锁 | 已改善 |

---

## 二、详细分析

### 风险 1：超大文件（可维护性风险）

**问题**：两个文件远超合理规模：

| 文件 | 行数 | 内容 |
|------|------|------|
| `crates/tui/src/tui/ui.rs` | 11,412 | 渲染逻辑、事件处理、异步动作分发 |
| `crates/tui/src/main.rs` | 9,231 | 初始化、参数解析、模块接线 |

**影响**：
- 单文件改动容易引入回归（合并冲突概率高）
- 新贡献者理解成本高
- IDE 性能下降（语义分析、补全变慢）

**建议拆分方案**：

```
ui.rs (11,412行) 拆分为：
├── ui/chat.rs        # 对话渲染
├── ui/sidebar.rs     # 侧边栏（已有 tui/sidebar.rs，可合并）
├── ui/footer.rs      # 底部状态栏
├── ui/picker.rs      # 选择器/弹窗
├── ui/notifications.rs # 通知渲染（已有）
└── ui/mod.rs         # 公共接口 + 组装

main.rs (9,231行) 拆分为：
├── init.rs           # 初始化流程
├── args.rs           # 参数解析
├── wiring.rs         # 模块接线
└── main.rs           # 入口点（<200行）
```

**目标**：单文件不超过 1,000 行。

---

### 风险 2：unsafe 使用（内存安全风险）

**问题**：`settings.rs` 有 50 处 unsafe 块，是全项目最高的。

**排查重点**：
- 是否涉及裸指针解引用
- 是否有 FFI 调用（C 库绑定）
- 是否可用安全的 Rust 抽象替代（如 `std::slice::from_raw_parts` → `safe` 版本）

**其他 unsafe 热区**：

| 文件 | unsafe 数 | 用途 |
|------|-----------|------|
| `settings.rs` | 50 | 待审查 |
| `main.rs` | 34 | 可能是终端 raw mode 相关 |
| `secrets/src/lib.rs` | 29 | 密钥操作，可能合理 |
| `notifications.rs` | 24 | 待审查 |

**建议**：对每个 unsafe 块添加 `// SAFETY:` 注释说明安全性依据。这是 Rust 社区的最佳实践。

---

### 风险 3：clone() 热区（内存压力风险）

**问题**：非测试代码中有 2,425 处 `.clone()`，集中在以下文件：

| 文件 | clone 数 | 场景 |
|------|----------|------|
| `tools/subagent/mod.rs` | 247 | 子代理上下文传递 |
| `tui/ui.rs` | 245 | UI 状态渲染 |
| `core/engine.rs` | 173 | 引擎状态管理 |
| `runtime_threads.rs` | 148 | 运行时线程状态 |
| `config/src/lib.rs` | 135 | 配置解析 |
| `core/engine/turn_loop.rs` | 130 | 轮次循环 |
| `runtime_api.rs` | 110 | API 调用 |
| `main.rs` | 108 | 初始化 |

**影响分析**：
- 大部分 clone 是 `String`、`Vec<u8>` 等堆分配类型
- 在高频路径（如流式响应处理、UI 渲染循环）中，累积的内存分配可能导致延迟抖动
- 但 Rust 的分配器通常很快，**需要 profile 确认实际瓶颈**

**优化策略**（按优先级）：
1. **引用替代**：函数参数用 `&str` 代替 `String`，`&[T]` 代替 `Vec<T>`
2. **Arc 共享**：不可变大对象用 `Arc<T>` 共享所有权
3. **Cow 惰性克隆**：`Cow<'_, str>` 在不需要修改时避免分配
4. **Bytes 零拷贝**：LLM 响应等大 payload 用 `bytes::Bytes`

**注意**：不要盲目优化。先用 `cargo flamegraph` 或 `perf` 确认 clone 是否真的是瓶颈。

---

### 风险 4：工具执行串行化（吞吐风险）

**问题**：`Engine::tool_exec_lock: Arc<RwLock<()>>` 对所有工具调用加写锁，同一时刻只能执行一个工具。

**实际评估**：这是**有意设计**，防止并发工具执行导致文件冲突（两个工具同时写同一个文件）。

**如果未来需要优化**：
- 按工具类型分锁：读操作（search、read_file）可并行，写操作（write_file、shell）串行
- 按资源分锁：不同文件的写操作可并行
- 引入依赖图：分析工具调用的资源依赖，并行无冲突的调用

**当前建议**：保持现状。并行化带来的复杂度和风险远大于当前的吞吐限制。

---

### 风险 5：嵌套锁模式（死锁风险）

**问题**：`rlm/session.rs` 原有双层嵌套 Mutex：

```rust
// 旧代码（已改善）
pub type SharedRlmSessionStore = Arc<Mutex<HashMap<String, Arc<Mutex<RlmSession>>>>>;
```

**当前状态**：已改为 `Arc<RwLock<HashMap<String, Arc<Mutex<RlmSession>>>>>`。

**改善点**：
- 外层改为 `RwLock`，读多写少场景下减少锁竞争
- 内层 `Mutex` 保护单个 session 的 mutation，粒度合理

**残余风险**：如果两个代码路径同时获取外层写锁和内层锁，仍有死锁可能。但当前代码中外层锁释放后才获取内层锁，实际风险低。

**建议**：添加注释说明锁获取顺序，防止未来修改引入死锁。

---

### 风险 6：tokio::spawn 使用（并发结构风险）

**问题**：全项目有 28 处 `tokio::spawn`，分布在 12 个文件中。

| 文件 | spawn 数 | 用途 |
|------|----------|------|
| `tui/ui.rs` | 7 | UI 异步操作 |
| `tools/dev_server_readiness.rs` | 7 | 开发服务器就绪检查 |
| `config_ui.rs` | 3 | 配置 UI 异步操作 |
| `utils.rs` | 2 | 工具函数 |
| `runtime_threads.rs` | 2 | 运行时线程 |
| `mcp.rs` | 2 | MCP 连接 |

**风险点**：
- 裸 `tokio::spawn` 的 task 如果 panic，错误会被静默吞掉
- 没有 `JoinHandle` 管理的 spawn，无法在关闭时等待完成

**好消息**：关键路径已使用 `spawn_supervised` 包装（12 个调用点），捕获 panic 并写入 crash dump。

**建议**：将剩余的裸 `tokio::spawn` 逐步迁移到 `spawn_supervised`，特别是 `ui.rs` 和 `dev_server_readiness.rs` 中的调用。

---

## 三、并发安全审查

### 锁使用模式

| 模式 | 数量 | 评估 |
|------|------|------|
| `RwLock` (tokio 异步) | 主要 | ✅ 正确，适合读多写少 |
| `Mutex` (tokio 异步) | 次要 | ✅ 正确，短临界区 |
| `try_lock()` | 31 处 | ✅ 有守卫，不会泄漏锁 |
| `RwLock::write().unwrap()` | 0 | ✅ 无此反模式 |
| `RwLock::read().unwrap()` | 0 | ✅ 无此反模式 |

**结论**：锁使用模式健康，无明显的死锁风险模式。

### Channel 使用模式

| 模式 | 用途 | 评估 |
|------|------|------|
| `mpsc` | Engine↔UI 通信 | ✅ 正确的 producer-consumer |
| `broadcast` | 事件扇出 | ✅ 正确的 fan-out |
| `oneshot` | 请求-响应 | ✅ 正确的 request-response |

**结论**：Channel 使用遵循结构化并发模式，无明显问题。

---

## 四、内存安全审查

### 已知安全措施

1. **spawn_supervised**：捕获子任务 panic，写入 crash dump
2. **CancellationToken**：管理优雅关闭，避免资源泄漏
3. **Drop trait**：关键资源（文件句柄、网络连接）实现 Drop 自动清理
4. **secrets crate**：密钥文件检查 0600 权限

### 潜在风险点

1. **大量 String clone**：频繁分配/释放可能导致内存碎片
2. **无界 channel**：如果生产者速度远超消费者，可能导致内存增长
3. **SQLite 连接**：单连接模式下，长事务可能阻塞其他操作

---

## 五、改进建议总结

### 立即行动（高收益低风险）

- [ ] 对 `settings.rs` 的 50 处 unsafe 块添加 `// SAFETY:` 注释
- [ ] 将 `ui.rs` 和 `dev_server_readiness.rs` 的裸 `tokio::spawn` 迁移到 `spawn_supervised`

### 短期改进（需评估收益）

- [ ] 拆分 `ui.rs`（11,412 行）为 5-6 个子模块
- [ ] 拆分 `main.rs`（9,231 行）为 3-4 个子模块
- [ ] 用 `cargo flamegraph` profile clone 热区的实际性能影响

### 长期优化（需架构评审）

- [ ] 评估工具执行锁按类型分拆的可行性
- [ ] 建立 CI 级别的 unsafe 审计和 clone 密度检查

### 不需要做的事

- ~~替换生产代码中的 unwrap~~ — 审计确认生产代码零 unwrap，全在测试代码中
- ~~消除所有 clone~~ — 大部分 clone 是合理的所有权转移，不需要优化
- ~~统一 TUI 和 core 的锁策略~~ — 两者的并发模型根本不同
