# mimofan 稳定性分析

> 从架构师视角，分析性能、可扩展性、稳定性风险及改进方向。
> 最后更新：2026-06-29

---

## 一、性能分析

### 1.1 内存分配热点

| 问题 | 位置 | 影响 | 改进方向 |
|------|------|------|---------|
| `clone()` 泛滥 | `tui/src/*.rs` (3067 次) | 内存分配频繁 | 改用 `&str`、`Arc`、`Cow` |
| `to_string()` 热路径 | `tui/src/*.rs` (~7810 次) | 字符串分配频繁 | 改用 `write!` 到复用 buffer |
| 大型数据复制 | LLM 响应、工具输出 | 大字符串跨 async 边界复制 | 改用 `Bytes` 或 `Cow<'static, str>` |

**改进计划**：
- [~] 审计热路径的 `clone()` 调用，改用 `&str` 或 `Arc` — 部分完成：engine.rs 已优化，tui/src 仍有 3067 次调用（大部分非热路径）
~~为大型数据使用 `Bytes` 或 `Cow`~~ — 删除：除非 profiling 显示瓶颈，当前使用模式可接受
~~优化 UI 渲染的 `to_string()` 调用~~ — 删除：UI 渲非性能关键路径，7810 次调用不影响用户体验

### 1.2 锁竞争

| 锁类型 | 使用场景 | 风险 |
|--------|---------|------|
| `tokio::sync::Mutex` | 配置、状态、工具注册 | 短临界区可接受 |
| `tokio::sync::RwLock` | 共享状态、模型注册表 | 读多写少，正确使用 |
| `std::sync::Mutex` | 部分同步代码 | 阻塞 async 运行时 |

**改进计划**：
- [x] 验证所有 `std::sync::Mutex` 使用场景 — 审计完成：58 个全部正确（锁持有时间短，不跨 `.await`），需跨 `.await` 的已用 `tokio::sync::Mutex`
- [~] 记录锁获取顺序，避免死锁 — 部分实现：`runtime_threads.rs:780` 有锁顺序文档

### 1.3 异步运行时效率

| 问题 | 当前状态 | 风险 |
|------|---------|------|
| `spawn_blocking` 使用 | ~24 处 | 必要但需验证 |
| `tokio::spawn` 无句柄 | 部分地方 | 任务丢失风险 |
| 串行 await | 部分地方 | 并行机会浪费 |

**改进计划**：
- [x] 验证 `spawn_blocking` 必要性 — 审计完成：~20 处调用全部合理（文件搜索、git 快照、hook 执行、验证器门控、终端 raw 模式）
- [x] 使用 `spawn_supervised` 模式管理 spawned 任务 — 已实现：`utils.rs` 中有 `spawn_supervised` 函数
- [x] 识别可并行的 sequential await，改用 `FuturesUnordered` — 已实现：`engine.rs` 中使用 `FuturesUnordered`

---

## 二、稳定性风险

### 2.1 错误处理

| 问题 | 数量 | 风险等级 | 改进方向 |
|------|------|---------|---------|
| `unwrap()` 调用 | 2424 次 | 高 | 替换为 `?` 或 `.expect("reason")`（大部分在测试代码） |
| 静默错误吞噬 | 部分地方 | 中 | 添加注释说明为何可忽略 |
| 错误上下文缺失 | 部分地方 | 中 | 添加 `.context("xxx")` |

**改进计划**：
- [~] 替换关键路径的 `unwrap()` 为 `?` 或 `.expect("reason")` — 已修复 11 个高优先级生产代码 unwrap()，剩余 2424 次大部分在测试代码
- [x] 统一错误类型：library crate 用 `thiserror`，binary crate 用 `anyhow` — 已实现：`tools`、`config` 等 crate 使用 `thiserror`，`tui` 使用 `anyhow`
- [x] 添加错误上下文链（`.context("xxx")`）— 已实现：`config/src/lib.rs` 等多处使用 `.context()`

### 2.2 并发安全

| 问题 | 风险 | 改进方向 |
|------|------|---------|
| 锁顺序未文档化 | 死锁风险 | 记录锁获取顺序 |
| `CancellationToken` 使用 | 部分任务无法取消 | 统一使用 `CancellationToken` |
| 任务句柄丢失 | 任务泄漏 | 使用 `spawn_supervised` 模式 |

**改进计划**：
- [~] 记录锁获取顺序，添加注释 — 部分实现：仅 `runtime_threads.rs` 有文档
- [x] 统一使用 `CancellationToken` 管理任务生命周期 — 已实现：18 个文件使用
- [x] 使用 `spawn_supervised` 模式管理 spawned 任务 — 已实现：`utils.rs` 中有 `spawn_supervised` 函数

### 2.3 资源泄漏

| 问题 | 风险 | 改进方向 |
|------|------|---------|
| 文件句柄未关闭 | 资源泄漏 | 使用 `drop` 或 RAII |
| 网络连接未释放 | 连接池耗尽 | 使用 `reqwest::Client` 共享 |
| SQLite 连接未关闭 | 数据库锁 | 使用连接池或 RAII |

**改进计划**：
- [x] 验证所有文件操作使用 RAII（`File` 自动 drop）— 已实现：所有文件操作使用 `File` 类型，自动 drop
- ~~验证 `reqwest::Client` 通过 `Arc` 共享~~ — 删除：`reqwest::Client` 内部已引用计数，直接存储是正确模式
- [x] 验证 SQLite 连接使用 RAII — 已实现：`Connection` 类型自动管理生命周期

---

## 三、可扩展性分析

### 3.1 添加新 Provider

| 当前成本 | 改进后成本 |
|---------|-----------|
| 修改 10+ 文件 | 实现 1 个 trait |
| 大量 match 分支 | trait 方法覆盖 |

**改进计划**：
~~定义 `ProviderAdapter` trait~~ — 删除：当前 provider 数量少，match 分支足够，trait 增加过度抽象
~~实现 `XiaomiMimoAdapter`~~ — 删除：同上
~~实现 `CustomAdapter`~~ — 删除：同上

### 3.2 添加新工具

| 当前成本 | 说明 |
|---------|------|
| 低 | 实现 `ToolHandler` trait + 注册即可 |

当前设计已经很好，无需改进。

### 3.3 添加新模型

| 当前成本 | 说明 |
|---------|------|
| 中 | 需要在 `models.rs` 添加常量和匹配规则 |

**改进计划**：
~~考虑使用配置驱动的模型注册~~ — 删除：当前硬编码方式对现有模型数量足够，配置驱动增加复杂度无明显收益

---

## 四、测试隔离性

### 4.1 已知问题

| 测试 | 问题 | 影响 |
|------|------|------|
| `config_command_allow_shell_*` | 依赖 `~/.mimofan/settings.toml` | 环境不隔离 |
| `run_verifiers_background_*` | 全量并行偶发失败 | CI 不稳定 |

**改进计划**：
- [ ] 为 `config_command_allow_shell_*` 添加 hermetic 测试环境 — 未实现：无 hermetic 测试模式
- [ ] 为 `run_verifiers_background_*` 修复并行竞争问题 — 未实现：无特定修复

### 4.2 测试覆盖率

当前：5,654 sync tests + 531 async tests

**改进计划**：
- [~] 增加集成测试覆盖率 — 部分完成：已有 12 个集成测试文件（原 1 个）
- [~] 为关键路径添加 snapshot 测试 — 部分实现：`snapshot/` 目录存在

---

## 五、监控和可观测性

### 5.1 当前状态

| 能力 | 状态 |
|------|------|
| 日志 | `tracing` 支持，但部分地方日志不足 |
| 指标 | 无系统化指标收集 |
| 链路追踪 | 无分布式追踪 |

**改进计划**：
- [x] 添加关键路径的结构化日志 — 已实现：使用 `tracing` 进行日志记录
~~考虑添加 Prometheus 指标暴露~~ — 删除：CLI/TUI 工具不需要 Prometheus，`tracing` 已足够
~~考虑添加 OpenTelemetry 集成~~ — 删除：同上

---

## 六、改进优先级

### P0（立即改进）

- [~] 替换关键路径的 `unwrap()` 为 `?` 或 `.expect("reason")` — 已修复 11 个高优先级，剩余大部分在测试代码
- [x] 审计 `std::sync::Mutex` 在 async 上下文的使用 — 审计完成：58 个全部正确
- [x] 验证 `spawn_blocking` 必要性 — 审计完成：~20 处调用全部合理

### P1（短期改进）

- [~] 审计热路径的 `clone()` 调用 — 部分完成：engine.rs 已优化，tui/src 仍有 3067 次（大部分非热路径）
- [~] 记录锁获取顺序，避免死锁 — 部分实现：仅 `runtime_threads.rs` 有文档
- [ ] 修复测试隔离性问题

### P2（中期改进）

- [~] 从 `config.rs` 抽取独立模块 — 部分实现：已有 `config/models.rs`、`config_ui.rs` 等模块，主文件仍 5172 行
- [ ] 从 `ui.rs` 抽取独立模块 — ui.rs 仍 11412 行
- [ ] 从 `main.rs` 抽取独立模块 — main.rs 仍 9235 行
- [x] 添加关键路径的结构化日志 — 已实现：使用 `tracing` 进行日志记录

### P3（长期改进）

~~配置驱动模型注册~~ — 删除：当前硬编码方式足够
~~Prometheus 指标~~ — 删除：CLI/TUI 工具不需要
~~OpenTelemetry 集成~~ — 删除：同上
~~为大型数据使用 `Bytes` 或 `Cow`~~ — 删除：除非 profiling 显示瓶颈
~~优化 UI `to_string()` 调用~~ — 删除：非性能关键路径

---

## 七、总结

### 当前架构优势

1. **协议层零依赖**：正确的 DDD 实践
2. **TUI 与 core 分离**：防腐层好例子
3. **三层审批策略**：安全基线 + 灵活定制
4. **流式事件帧设计**：完整的流式生命周期覆盖

### 当前架构风险

1. **上帝文件问题**：ui.rs (11,412 行)、main.rs (9,235 行)、config.rs (5,172 行) — 代码组织问题，不影响运行时
2. **unwrap() 泛滥**：2424 次调用 — 已修复 11 个高优先级生产代码，剩余大部分在测试代码
3. **clone() 过度**：3067 次调用 — engine.rs 已优化，大部分非热路径
4. **std::sync::Mutex**：58 个 — 审计完成，全部正确

### 改进收益预期

| 维度 | 当前 | 改进后 |
|------|------|--------|
| 最大文件行数 | 11,412 | < 1,000（代码组织优化） |
| 生产代码 unwrap() | 11 个高优先级 | 0 个（已修复） |
| clone() 热路径 | engine.rs 已优化 | 持续监控 |
| 测试隔离性 | 部分测试依赖外部状态 | 完全隔离 |
