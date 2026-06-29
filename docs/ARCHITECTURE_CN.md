# mimofan 架构说明

> 本文档面向开发者，用大白话讲清楚 mimofan 的架构设计、模块职责、核心入口和扩展方式。
> 最后更新：2026-06-29

---

## 一句话说清楚

mimofan 是一个 **AI 编码助手**，你给它一句话（比如"帮我写个 REST API"），它调用大模型思考，然后用工具帮你改代码、跑命令、查文件，直到把活干完。

---

## 整体架构分层

```
┌──────────────────────────────────────────────────────────────────┐
│                        你（用户）                                  │
│                通过终端、命令行、HTTP API 交互                       │
└────────────────────────┬─────────────────────────────────────────┘
                         │
┌────────────────────────▼─────────────────────────────────────────┐
│                     用户交互层                                      │
│                                                                   │
│   ┌───────────────┐   ┌───────────────┐   ┌──────────────────┐  │
│   │  TUI 终端界面   │   │  CLI 命令行    │   │  HTTP/SSE 服务器  │  │
│   │  (ratatui)     │   │  (clap)       │   │  (axum)         │  │
│   │  crates/tui    │   │  crates/cli   │   │  crates/app-    │  │
│   │                │   │               │   │  server          │  │
│   └───────────────┘   └───────────────┘   └──────────────────┘  │
└────────────────────────┬─────────────────────────────────────────┘
                         │
┌────────────────────────▼─────────────────────────────────────────┐
│                     领域服务层                                      │
│                                                                   │
│   ┌───────────────┐   ┌───────────────┐   ┌──────────────────┐  │
│   │  核心引擎       │   │  模型注册表    │   │  配置管理         │  │
│   │  crates/core   │   │  crates/agent │   │  crates/config   │  │
│   │  (会话/线程/    │   │  (70+ 模型    │   │  (TOML 解析/     │  │
│   │   任务编排)     │   │   路由解析)    │   │   provider 配置)  │  │
│   └───────────────┘   └───────────────┘   └──────────────────┘  │
└────────────────────────┬─────────────────────────────────────────┘
                         │
┌────────────────────────▼─────────────────────────────────────────┐
│                     领域模型层                                      │
│                                                                   │
│   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│   │  工具注册  │  │ 执行策略   │  │  钩子系统 │  │  密钥管理      │  │
│   │  /调度    │  │  引擎     │  │          │  │               │  │
│   │  tools   │  │execpolicy │  │  hooks   │  │  secrets      │  │
│   └──────────┘  └──────────┘  └──────────┘  └───────────────┘  │
└────────────────────────┬─────────────────────────────────────────┘
                         │
┌────────────────────────▼─────────────────────────────────────────┐
│                     基础设施层                                      │
│                                                                   │
│   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│   │ 协议类型   │  │ MCP 客户端│  │ 状态持久化 │  │ 工作流引擎     │  │
│   │ protocol  │  │   mcp    │  │  state   │  │  whaleflow    │  │
│   └──────────┘  └──────────┘  └──────────┘  └───────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

### 各层职责（说人话）

- **用户交互层**：你看到的界面。TUI 是终端里的可视化界面（类似 vim），CLI 是命令行工具，HTTP 服务器让其他程序可以通过网络调用 mimofan。
- **领域服务层**：大脑。管理对话（会话/线程）、决定调用哪个模型、读取配置。
- **领域模型层**：手和脚。执行 shell 命令、读写文件、控制哪些操作需要你确认。
- **基础设施层**：地基。定义数据格式、连接外部 MCP 工具服务器、把数据存到 SQLite。

---

## Crate 依赖关系图

```
Layer 0 ─ 零依赖叶子（可独立编译测试）
┌─────────┬─────────┬─────────┬─────────┬─────────┬───────────┐
│protocol │  mcp    │ secrets │  state  │ release │ whaleflow │
└────┬────┴────┬────┴────┬────┴────┬────┴────┬────┴───────────┘
     │         │         │         │         │
Layer 1 ─ 仅依赖 protocol
┌────┴─────────┴─────────┴─────────┴─────────┐
│  tools    hooks    execpolicy               │
└────┬──────────────┬────────────────────────┘
     │              │
Layer 2 ─ 依赖 Layer 0-1
┌────┴──────────────┴────────────────────────┐
│  config ──→ execpolicy + secrets           │
│  agent  ──→ config                         │
└────┬───────────────────────────────────────┘
     │
Layer 3 ─ 依赖 Layer 0-2
┌────┴──────────────────────────────────────────────────────┐
│  core ──→ agent + config + execpolicy + hooks             │
│           + mcp + protocol + state + tools                │
│                                                           │
│  tui  ──→ config + execpolicy + protocol + release        │
│           + secrets + tools（不依赖 core，独立运行时）       │
│                                                           │
│  app-server ──→ agent + config + core + execpolicy + hooks│
│                 + mcp + protocol + state + tools           │
└────┬──────────────────────────────────────────────────────┘
     │
Layer 4 ─ 顶层入口
┌────┴──────────────────────────────────────────┐
│  cli ──→ agent + app-server + config +        │
│          execpolicy + mcp + release +          │
│          secrets + state                       │
└───────────────────────────────────────────────┘
```

**简单理解**：
- `protocol` 是所有模块共用的"语言"（数据类型定义），零依赖
- `core` 是中枢，连接模型、工具、配置（供 HTTP 服务器使用）
- `tui` 有独立的运行时路径（因为终端 UI 需要特殊的事件循环和渲染逻辑）
- `cli` 和 `app-server` 是两个入口

### 为什么 TUI 不依赖 core？

这不是 bug，是有意设计：

| | core | tui |
|---|---|---|
| 用途 | HTTP API 服务器的后端引擎 | 终端交互式界面 |
| 事件循环 | 请求-响应模式 | ratatui 渲染循环 + 终端事件 |
| 子代理 | 通过 API 编排 | 直接管理（渲染进度、UI 状态） |
| 线程管理 | ThreadManager | RuntimeThreadManager |

两者解决的问题根本不同，强行统一会引入巨大复杂度。

---

## 核心模块详解

### 1. protocol — 协议类型（基础设施层）

**位置**：`crates/protocol/src/`

**作用**：定义所有模块之间传递的数据格式。就像一份"合同"，大家都按这个格式来。

**子模块**：
- `thread.rs` — 线程协议（Thread, ThreadRequest, ThreadResponse, Envelope）
- `event.rs` — 流式事件帧（EventFrame 及其 20+ 变体）
- `approval.rs` — 审批协议（AskForApproval, ReviewDecision）
- `tool.rs` — 工具协议（ToolKind, ToolPayload, ToolOutput）
- `app.rs` — 应用层请求/响应（AppRequest, AppResponse）
- `fleet.rs` — 舰队编排协议
- `workroom.rs` — 工作室协议
- `lib.rs` — 仅做 `pub use` 重导出

**特点**：零依赖，纯数据定义，任何模块都可以安全引用。

### 2. agent — 模型注册表（领域服务层）

**位置**：`crates/agent/src/`

**作用**：管理所有支持的 AI 模型。你告诉它"我要用 mimo-v2.5-pro"，它帮你找到正确的 provider 和端点。

> **注意**：虽然叫 `agent`，但它实际只包含模型注册表逻辑，不是 Agent 系统的核心。

**子模块**：
- `family.rs` — `ModelFamily` 枚举（MiMo、DeepSeek、OpenAI 等家族）
- `provider_resolver.rs` — 模型匹配、大小写保持、provider 特定逻辑
- `lib.rs` — `ModelRegistry`（70+ 模型配置）、`resolve()` 方法

**使用示例**：
```rust
use mimofan_agent::{ModelRegistry, model_family, ModelFamily};

let registry = ModelRegistry::default();
let result = registry.resolve(Some("mimo-v2.5-pro"), None);
// result.resolved.id == "mimo-v2.5-pro"
// result.resolved.provider == ProviderKind::XiaomiMimo

assert_eq!(model_family("mimo-v2.5-pro"), ModelFamily::MiMo);
```

### 3. core — 核心引擎（领域服务层）

**位置**：`crates/core/src/`

**作用**：编排一切。管理对话会话、调度工具调用、处理审批流程、启动 MCP 服务器。

**子模块**：
- `thread.rs` — `ThreadManager`（线程生命周期：创建/恢复/fork/归档）、`InitialHistory`
- `job.rs` — `JobManager`（后台任务：入队/运行/暂停/完成/失败/重试）
- `lib.rs` — `Runtime`（顶层编排器）

**核心入口**：
```rust
// 创建运行时
let runtime = Runtime::new(config, model_registry, state, tool_registry,
                           mcp_manager, exec_policy, hooks);

// 管理线程
let new_thread = runtime.thread_manager.spawn_thread_with_history(
    model_provider, cwd, InitialHistory::New, true
)?;

// 管理任务
runtime.jobs.enqueue("my-job", payload)?;
```

### 4. config — 配置管理（领域服务层）

**位置**：`crates/config/src/`

**作用**：读取和解析配置文件，管理 provider 配置、模型设置、执行策略。

**配置文件位置**：
- `~/.mimofan/config.toml` — 主配置文件
- `~/.mimofan/settings.toml` — 运行时设置
- `/etc/deepseek/managed_config.toml` — 系统级默认配置

**核心类型**：
```rust
// 解析配置
let store = ConfigStore::load(Some("config.toml"))?;
let config: &ConfigToml = store.config();

// 路由解析
let resolver = RouteResolver::new(config);
let candidate = resolver.resolve(&request)?;
// candidate.model == "mimo-v2.5-pro"
// candidate.base_url == "https://api.xiaomi.com"
```

### 5. tools — 工具系统（领域模型层）

**位置**：`crates/tools/src/`

**作用**：定义工具的抽象接口和注册调度机制。

**如何注册一个新工具**：
```rust
use mimofan_tools::{ToolHandler, ToolSpec, ToolRegistry, ToolInvocation, ToolOutput};
use async_trait::async_trait;

struct MyTool;

#[async_trait]
impl ToolHandler for MyTool {
    fn kind(&self) -> ToolKind { ToolKind::Function }
    fn is_mutating(&self) -> bool { true }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        // 实现工具逻辑
        Ok(ToolOutput::Function { body: Some(json!({"ok": true})), success: true })
    }
}

// 注册
let mut registry = ToolRegistry::default();
registry.register(spec, Arc::new(MyTool))?;
```

### 6. execpolicy — 执行策略引擎（领域模型层）

**位置**：`crates/execpolicy/src/`

**作用**：决定一个工具调用是否需要用户审批。三层规则集（BuiltinDefault → Agent → User），arity-aware 命令匹配。

```rust
let engine = ExecPolicyEngine::new();
let decision = engine.check(&ExecPolicyContext {
    command: "rm -rf /tmp/test",
    cwd: "/workspace",
    tool: "shell",
    ask_for_approval: AskForApproval::OnRequest,
    sandbox_mode: SandboxMode::Seatbelt,
})?;
// decision.requires_approval == true（因为 rm -rf 匹配了危险命令规则）
```

### 7. mcp — MCP 客户端（基础设施层）

**位置**：`crates/mcp/src/`

**作用**：连接外部 MCP（Model Context Protocol）工具服务器。MCP 是一种让 AI 助手调用外部工具的标准协议。

```rust
let manager = McpManager::new();
manager.register_server(McpServerConfig {
    name: "my-tools".into(),
    command: "node".into(),
    args: vec!["server.js".into()],
    ..Default::default()
}).await?;
manager.start_all().await?;

let tools = manager.list_tools().await;
// tools 包含 MCP 服务器暴露的所有工具
```

### 8. state — 状态持久化（基础设施层）

**位置**：`crates/state/src/`

**作用**：用 SQLite 存储会话、线程、任务等持久化数据。4 次 schema 迁移（v0→v4）。

```rust
let store = StateStore::open("~/.mimofan/state.db")?;

// 创建线程
store.upsert_thread(&ThreadMetadata { id: "t1".into(), .. })?;

// 追加消息
store.append_message(&MessageRecord {
    thread_id: "t1".into(),
    role: "user".into(),
    content: "Hello".into(),
    ..Default::default()
})?;

// 恢复会话
let messages = store.list_messages("t1")?;
```

---

## 依赖的第三方核心组件

| 组件 | 用途 | 出现在哪个 crate |
|------|------|-----------------|
| `tokio` | 异步运行时（全功能） | 几乎所有 crate |
| `axum` | HTTP 服务器框架 | app-server, tui |
| `clap` | 命令行参数解析 | cli, tui |
| `reqwest` | HTTP 客户端（rustls） | hooks, mcp, app-server |
| `serde` / `serde_json` | 序列化/反序列化 | 几乎所有 crate |
| `rusqlite` | SQLite 数据库（bundled） | state |
| `ratatui` | 终端 UI 框架 | tui |
| `tracing` | 日志追踪 | 几乎所有 crate |
| `anyhow` / `thiserror` | 错误处理 | 几乎所有 crate |
| `async-trait` | 异步 trait 支持 | tools, hooks |
| `tower-http` | HTTP 中间件（CORS） | app-server |
| `toml` / `toml_edit` | TOML 配置解析 | config |
| `keyring` | 系统密钥环 | secrets |
| `starlark` | Starlark 脚本引擎 | whaleflow |
| `rustls` | TLS 实现 | reqwest |
| `chrono` | 时间处理 | 几乎所有 crate |
| `uuid` | UUID 生成 | 几乎所有 crate |
| `sha2` | SHA256 哈希 | state, tui |
| `dirs` | 系统目录路径 | config, secrets |

---

## 提示词工程体系

mimofan 的提示词采用**分层治理**架构，优先级从高到低：

```
Tier 1 — 宪法（constitution.md）
  ├── 不可违反的核心原则：诚实、验证、行动力、遗产责任
  └── 优先级最高，与任何其他指令冲突时宪法胜出

Tier 2 — 法规（statutes）
  ├── 语言规则：跟随用户语言，中英文自动切换
  ├── 输出格式：终端渲染，避免 markdown 表格
  ├── 验证原则：每个工具调用后必须验证结果
  ├── 执行纪律：必须用工具行动，不能只说不做
  └── 范围控制：只做用户要求的事，不擅自扩展

Tier 3 — 规章（regulations）
  ├── 组合模式：多步骤任务先列计划再执行
  ├── 子代理策略：何时派生子代理、如何编排
  ├── 资源管理：token 预算、子代理配额
  └── 上下文管理：何时压缩、何时清理

Tier 6 — 证据/工具手册（evidence）
  ├── 工具速查表：所有可用工具的描述
  ├── 工具选择指南：什么场景用什么工具
  └── 子代理完成事件处理协议
```

**提示词文件位置**：`crates/tui/src/prompts/`

| 文件 | 作用 |
|------|------|
| `constitution.md` | 核心宪法，定义 AI 的行为准则 |
| `compact.md` | 上下文压缩时的摘要模板 |
| `continuation.md` | 对话续接提示 |
| `memory_guidance.md` | 记忆系统引导 |
| `subagent_output_format.md` | 子代理输出格式要求 |
| `modes/agent.md` | Agent 模式提示词 |
| `modes/plan.md` | Plan 模式提示词 |
| `modes/yolo.md` | YOLO 模式提示词（跳过审批） |
| `approvals/*.md` | 不同审批策略的提示词 |
| `personalities/*.md` | 人格风格（calm/playful） |

**提示词模板变量**：提示词中使用 `{variable_name}` 占位符，运行时由引擎注入实际值（如 `{subagent_economics}`, `{context_window_note}` 等）。

---

## 核心数据流

### 一次完整的对话交互

```
你输入 "帮我写个 hello world"
    │
    ▼
TUI/CLI 接收输入
    │
    ▼
core 创建 Thread，组装 prompt
    │
    ▼
agent 根据配置解析出模型 (mimo-v2.5-pro)
    │
    ▼
LLM 客户端发送 HTTP 请求到小米 API
    │
    ▼
流式接收 EventFrame::ResponseDelta
    │
    ▼
TUI 逐字渲染回复 "好的，我来帮你..."
    │
    ▼
LLM 返回 ToolCall: write_file("main.rs", "fn main() {...}")
    │
    ▼
execpolicy 检查是否需要审批
    │
    ▼ (不需要审批)
tools::ToolRegistry::dispatch() 执行写文件
    │
    ▼
hooks 触发 post_tool_call 钩子
    │
    ▼
工具结果返回给 LLM
    │
    ▼
LLM 回复 "已完成，文件写入 main.rs"
    │
    ▼
TUI 显示最终回复
```

### 工具执行流程

```
LLM 请求调用工具
    │
    ▼
ToolRegistry 查找工具 handler
    │
    ▼
验证 payload 类型匹配
    │
    ▼
检查 is_mutating + allow_mutating
    │
    ▼
获取执行锁（并行/串行）
    │
    ▼
调用 handler.handle(invocation)
    │
    ▼
返回 ToolOutput
```

---

## 扩展点

### 添加新工具

1. 在 `crates/tools/src/` 或新文件中实现 `ToolHandler` trait
2. 创建 `ToolSpec`（定义名称、输入 schema、超时等）
3. 在工具注册处调用 `registry.register(spec, handler)`
4. LLM 就能自动发现并使用这个工具

### 添加 MCP 服务器

1. 编辑 `~/.mimofan/mcp.json`，添加服务器配置
2. mimofan 启动时自动连接并注册工具
3. 工具自动对 LLM 可见

### 添加 Skills 技能

1. 在 `~/.mimofan/skills/` 下创建目录
2. 添加 `SKILL.md` 文件定义技能提示词
3. 可选添加辅助脚本
4. 通过 `load_skill` 工具调用

### 添加 Hooks 钩子

在 `~/.mimofan/config.toml` 中配置：

```toml
[[hooks]]
event = "tool_call_before"
command = "echo 'Running tool: $TOOL_NAME'"
```

---

## 构建与测试

```bash
# 格式化
cargo fmt

# 编译（默认成员：cli, app-server, tui）
cargo build

# 运行测试
cargo test -p mimofan-config        # 配置测试
cargo test -p mimofan-protocol      # 协议测试
cargo test -p mimofan --locked  # TUI 测试
cargo test --workspace                # 全量测试

# 发布构建
cargo build --release -p mimofan-cli -p mimofan
```

### 已知测试问题（非回归）

- `config_command_allow_shell_*` 在 `~/.mimofan/settings.toml` 设置 `default_mode = "yolo"` 时失败（测试不隔离）
- `run_verifiers_background_*` 在全量并行测试时偶发失败，单独运行通过

---

## 配置文件说明

| 文件路径 | 说明 |
|---------|------|
| `~/.mimofan/config.toml` | 主配置（provider、模型、策略） |
| `~/.mimofan/settings.toml` | 运行时设置（模式、UI 偏好） |
| `~/.mimofan/mcp.json` | MCP 服务器配置 |
| `~/.mimofan/skills/` | 用户自定义技能目录 |
| `~/.mimofan/sessions/` | 会话历史 |
| `~/.mimofan/tasks/` | 后台任务记录 |
| `~/.mimofan/snapshots/` | 工作区快照（用于恢复） |
| `~/.mimofan/audit.log` | 审计日志 |
| `config.example.toml` | 配置文件示例（项目根目录） |

---

---

## 性能优化指南

### 内存优化

```rust
// ❌ 不好：频繁 clone
let data = large_string.clone();
process(data);

// ✅ 好：使用引用
process(&large_string);

// ✅ 好：使用 Arc 共享所有权
let data = Arc::new(large_string);
process(data.clone());  // Arc clone 只增加引用计数
```

### 异步优化

```rust
// ❌ 不好：顺序 await
let a = fetch_a().await;
let b = fetch_b().await;
let c = fetch_c().await;

// ✅ 好：并行 await
let (a, b, c) = tokio::join!(
    fetch_a(),
    fetch_b(),
    fetch_c()
);
```

### 锁优化

```rust
// ❌ 不好：长时间持有锁
let mut state = self.state.lock();
state.update();
state.validate();
state.persist();  // 这里持有锁太久

// ✅ 好：缩小锁范围
let data = {
    let state = self.state.lock();
    state.get_data()
};  // 锁在这里释放
self.persist(data);
```

---

## 稳定性最佳实践

### 错误处理

```rust
// ❌ 不好：unwrap 导致 panic
let config = load_config().unwrap();

// ✅ 好：使用 ? 传播错误
let config = load_config()
    .context("加载配置文件失败")?;

// ✅ 好：使用 expect 提供上下文
let config = load_config()
    .expect("配置文件必须存在且格式正确");
```

### 并发安全

```rust
// ❌ 不好：嵌套锁可能导致死锁
let a = self.a.lock();
let b = self.b.lock();  // 如果其他地方顺序相反，死锁

// ✅ 好：全局锁顺序
// 规则：总是先获取 a，再获取 b
let a = self.a.lock();
let b = self.b.lock();

// ✅ 好：使用 RwLock（读多写少）
let state = self.state.read();  // 多个读者可以并发
let mut state = self.state.write();  // 写者独占
```

---

## 附录：关键类型速查

| 类型 | 所在 crate | 用途 |
|------|-----------|------|
| `Runtime` | core | 顶层编排器 |
| `ThreadManager` | core | 线程生命周期管理 |
| `JobManager` | core | 后台任务管理 |
| `ModelRegistry` | agent | 模型注册表 |
| `ConfigStore` | config | 配置加载器 |
| `RouteResolver` | config | 模型路由解析 |
| `ToolRegistry` | tools | 工具注册调度 |
| `ToolHandler` | tools | 工具实现 trait |
| `ExecPolicyEngine` | execpolicy | 审批策略引擎 |
| `HookDispatcher` | hooks | 钩子分发器 |
| `McpManager` | mcp | MCP 服务器管理 |
| `StateStore` | state | SQLite 持久化 |
| `EventFrame` | protocol | 流式事件帧 |
| `Thread` | protocol | 线程数据结构 |
| `ToolPayload` | protocol | 工具调用载荷 |
| `AskForApproval` | protocol | 审批策略枚举 |
