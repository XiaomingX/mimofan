# mimo-tui 架构改进计划

> 从第一性原理出发，以 DDD 理论分析当前架构，制定改进方案。
> 最后更新：2026-06-29

---

## 一、系统核心用途（第一性原理分析）

### 1.1 系统本质

mimo-tui 的本质是一个 **AI 驱动的自动化执行引擎**，核心价值链：

```
用户意图 → LLM �解 → 工具执行 → 结果反馈 → 循环迭代
```

**第一性原理拆解**：
- **输入**：用户的自然语言指令
- **处理**：LLM 理解意图，生成工具调用计划
- **执行**：通过工具（shell、文件、API）实际操作
- **反馈**：执行结果回传 LLM，决定下一步
- **终止**：任务完成或用户中断

### 1.2 核心能力矩阵

| 能力域 | 具体能力 | 当前实现 |
|--------|---------|---------|
| 意图理解 | 自然语言 → 结构化指令 | LLM API 调用 |
| 工具执行 | Shell、文件、搜索、子代理 | ToolRegistry + ToolHandler |
| 上下文管理 | 会话历史、压缩、恢复 | ThreadManager + StateStore |
| 安全控制 | 审批策略、沙箱、密钥管理 | ExecPolicy + Secrets |
| 多模型支持 | 路由、降级、切换 | ModelRegistry + RouteResolver |

---

## 二、DDD 限界上下文分析

### 2.1 当前上下文映射

```
┌─────────────────────────────────────────────────────────────────┐
│                    mimo-tui 限界上下文映射                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐      │
│  │  用户交互上下文 │    │  模型路由上下文 │    │  工具执行上下文 │      │
│  │              │    │              │    │              │      │
│  │  - TUI 渲染   │    │  - 模型注册表  │    │  - 工具注册    │      │
│  │  - CLI 解析   │    │  - Provider   │    │  - 工具调度    │      │
│  │  - HTTP API  │    │  - 路由解析    │    │  - 执行策略    │      │
│  │              │    │  - 配置管理    │    │  - 钩子系统    │      │
│  └──────┬───────┘    └──────┬───────┘    └──────┬───────┘      │
│         │                   │                   │              │
│         └───────────────────┼───────────────────┘              │
│                             │                                  │
│                    ┌────────▼────────┐                         │
│                    │  会话编排上下文   │                         │
│                    │                 │                         │
│                    │  - 线程管理     │                         │
│                    │  - 任务调度     │                         │
│                    │  - 子代理编排   │                         │
│                    │  - 上下文压缩   │                         │
│                    └────────┬────────┘                         │
│                             │                                  │
│                    ┌────────▼────────┐                         │
│                    │  基础设施上下文   │                         │
│                    │                 │                         │
│                    │  - 协议定义     │                         │
│                    │  - 状态持久化   │                         │
│                    │  - MCP 客户端   │                         │
│                    │  - 密钥管理     │                         │
│                    └─────────────────┘                         │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 上下文边界问题

| 问题 | 当前状态 | 影响 |
|------|---------|------|
| **config 上下文过载** | 5172 行，包含 provider、model、route、pricing、UI 配置 | 职责不清，修改风险高 |
| **tui 上下文膨胀** | main.rs 9235 行，ui.rs 11412 行 | 难以维护，测试困难 |
| **agent 命名误导** | 实际是模型注册表，不是 Agent 系统 | 概念混淆 |
| **client 职责混合** | 4874 行，包含 HTTP、SSE、重试、模型特定逻辑 | 违反单一职责 |

---

## 三、架构精妙之处（已有的好设计）

### 3.1 协议层零依赖

`protocol` crate 作为纯数据定义层，零外部依赖，所有模块共用。这是正确的 DDD 实践：
- **共享内核模式**：所有上下文通过共享协议类型通信
- **编译隔离**：修改 protocol 不会触发其他 crate 重编译（除非接口变化）

### 3.2 TUI 与 core 的有意分离

```
core (HTTP 服务器后端)          tui (终端交互界面)
  ├── 请求-响应模式               ├── ratatui 渲染循环
  ├── ThreadManager              ├── RuntimeThreadManager
  └── API 编排                   └── 直接 UI 状态管理
```

这是**防腐层**的好例子：两种交互模式的根本差异被显式隔离，而不是强行统一。

### 3.3 三层审批策略

```
BuiltinDefault → Agent → User
```

从内到外的策略覆盖，既保证安全基线，又允许灵活定制。

### 3.4 流式事件帧设计

`EventFrame` 的 20+ 变体覆盖了完整的流式生命周期，支持：
- 增量渲染（ResponseDelta）
- 工具调用（ToolCall）
- 审批请求（AskForApproval）
- 子代理事件（SubAgentEvent）

---

## 四、架构边界问题（需要改进的地方）

### 4.1 上帝文件问题

| 文件 | 行数 | 问题 |
|------|------|------|
| `tui/ui.rs` | 11,412 | 渲染逻辑、布局、状态判断全混在一起 |
| `tui/main.rs` | 9,235 | 初始化、配置、事件循环、命令处理全在一起 |
| `tui/ui/tests.rs` | 11,197 | 测试文件与源码耦合 |
| `tui/config.rs` | 5,172 | provider、model、route、pricing 全在一个文件 |
| `tui/client.rs` | 4,874 | HTTP、SSE、重试、模型特定逻辑混合 |

### 4.2 配置上下文职责过载

`config.rs` 当前包含：
- Provider 枚举和配置
- 模型名称常量和规范化
- 路由解析逻辑
- 定价配置
- UI 配置
- 子代理限制配置

这些应该拆分为独立的领域服务。

### 4.3 客户端逻辑耦合

`client.rs` 和 `client/chat.rs` 包含大量 provider 特定的分支逻辑：

```rust
// 当前：每个 provider 都有特殊处理
match provider {
    ApiProvider::XiaomiMimo => { /* 特殊逻辑 */ }
    ApiProvider::Custom => { /* 其他逻辑 */ }
    // ...
}
```

应该通过 trait 抽象 provider 差异。

### 4.4 unwrap() 泛滥

当前 `tui/src/*.rs` 中有 **356 个 unwrap()** 调用，主要集中在：
- 测试代码（可接受）
- 配置解析（应改为 `?` 或 `.expect()`）
- UI 渲染（应防御性处理）

### 4.5 clone() 过度使用

当前 `tui/src/*.rs` 中有 **792 个 clone()** 调用，主要因为：
- 字符串跨 async 边界传递
- 共享状态的读取方式不够优化
- 缺少 `Arc` 和 `Cow` 的使用

---

## 五、改进计划（Checklist）

### Phase 1: 配置上下文拆分（降低 config.rs 复杂度）

- [x] `protocol` 按限界上下文拆分（已完成）
- [x] `agent` 抽取 `family.rs` + `provider_resolver.rs`（已完成）
- [ ] 从 `config.rs` 抽取 `provider_config.rs`（Provider 配置独立）
- [ ] 从 `config.rs` 抽取 `model_config.rs`（模型名称常量独立）— 部分实现：已有 `config/models.rs` 模块（152 行），但主文件仍有 5172 行
- [ ] 从 `config.rs` 抽取 `route_config.rs`（路由解析独立）
- [ ] 从 `config.rs` 抽取 `pricing_config.rs`（定价配置独立）
- [ ] 从 `config.rs` 抽取 `ui_config.rs`（UI 配置独立）— 部分实现：已有 `config_ui.rs`（1318 行），但主文件仍有 5172 行

### Phase 2: 客户端逻辑抽象（降低 client.rs 耦合）

- [ ] 定义 `ProviderAdapter` trait，抽象 provider 特定逻辑
- [ ] 实现 `XiaomiMimoAdapter`（包含 thinking/reasoning 处理）
- [ ] 实现 `CustomAdapter`（通用 OpenAI 兼容逻辑）
- [ ] 将 HTTP 客户端逻辑抽取为 `HttpClient` 模块
- [ ] 将 SSE 流式解析抽取为 `SseParser` 模块

### Phase 3: UI 层拆分（降低 tui 复杂度）

- [ ] 从 `ui.rs` 抽取 `ui/chat.rs`（聊天区域渲染）
- [ ] 从 `ui.rs` 抽取 `ui/sidebar.rs`（侧边栏渲染）
- [ ] 从 `ui.rs` 抽取 `ui/footer.rs`（底部状态栏）
- [ ] 从 `ui.rs` 抽取 `ui/picker.rs`（选择器组件）
- [ ] 从 `main.rs` 抽取 `init.rs`（初始化逻辑）
- [ ] 从 `main.rs` 抽取 `event_loop.rs`（事件循环）

### Phase 4: 错误处理规范化（提升稳定性）

- [ ] 替换关键路径的 `unwrap()` 为 `?` 或 `.expect("reason")` — 当前仍有 356 次调用
- [x] 统一错误类型：library crate 用 `thiserror`，binary crate 用 `anyhow` — 已实现：`tools`、`config` 等 crate 使用 `thiserror`，`tui` 使用 `anyhow`
- [x] 添加错误上下文链（`.context("xxx")`）— 已实现：`config/src/lib.rs` 等多处使用 `.context()`

### Phase 5: 性能优化（降低资源消耗）

- [ ] 审计热路径的 `clone()` 调用，改用 `&str` 或 `Arc`
- [ ] 为大型数据（LLM 响应、工具输出）使用 `Bytes` 或 `Cow`
- [ ] 优化 UI 渲染的 `to_string()` 调用（~200+ 次）
- [ ] 验证 `RwLock` vs `Mutex` 使用场景

### Phase 6: 测试隔离（提升可靠性）

- [ ] 为 `config_command_allow_shell_*` 添加 hermetic 测试环境
- [ ] 为 `run_verifiers_background_*` 修复并行竞争问题
- [ ] 增加集成测试覆盖率

---

## 六、改造原则

### 6.1 不改动的部分

- **用户交互层**：TUI 界面、CLI 命令、HTTP API 接口不变
- **外部系统集成**：MCP 协议、工具调用格式不变
- **配置文件格式**：`config.toml`、`settings.toml` 结构不变
- **插件系统**：Skills、Hooks 接口不变

### 6.2 改动的部分

- **内部模块拆分**：大文件拆分为小模块
- **抽象层引入**：trait 抽象 provider 差异
- **错误处理**：规范化错误传播
- **性能优化**：减少不必要的内存分配

### 6.3 改造验证标准

- 所有现有测试通过（`cargo test --workspace`）
- 无新增 `unwrap()` 调用
- 无新增 `clone()` 调用（除非有明确理由）
- 编译时间不增加超过 10%

---

## 七、预期收益

| 维度 | 当前 | 改进后 |
|------|------|--------|
| 最大文件行数 | 11,412 | < 1,000 |
| config.rs 行数 | 5,172 | < 500（主文件） |
| unwrap() 数量 | 356 | < 50（仅测试） |
| clone() 数量 | 792 | < 200 |
| 新增 provider 成本 | 修改 10+ 文件 | 实现 1 个 trait |
| 测试隔离性 | 部分测试依赖外部状态 | 完全隔离 |
