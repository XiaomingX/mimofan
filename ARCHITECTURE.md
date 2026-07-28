# mimofan 架构说明文档

> 面向开发者的架构文档。说人话，不堆砌术语。

---

## 1. 系统定位

mimofan 是一个**跑在本地的 AI 编码助手**。用户用自然语言下指令，它调用大模型思考，再用工具（读文件、改代码、跑命令）把活干完。工作流是**"模型决策 → 工具执行 → 结果回灌 → 再决策"**的闭环。

**对标：** Claude Code / OpenCode

**差异：** Rust 实现 · 本地优先 · MIT 协议 · 默认 Xiaomi MiMo

---

## 2. 架构分层视图

```
┌─────────────────────────────────────────────────────────────────────┐
│                         用户交互层                                   │
│    ┌─────────────┐    ┌─────────────┐    ┌─────────────────────┐    │
│    │   TUI 终端   │    │  CLI 命令行  │    │   HTTP/JSON-RPC    │    │
│    │  (ratatui)  │    │   (clap)    │    │    (axum)          │    │
│    └──────┬──────┘    └──────┬──────┘    └──────────┬─────────┘    │
└───────────┼──────────────────┼─────────────────────┼───────────────┘
            │                  │                     │
            └──────────────────┼─────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          核心引擎层                                  │
│    ┌─────────────────────────────────────────────────────────────┐  │
│    │                    Runtime (聚合根)                          │  │
│    │   Turn Loop  │  ThreadManager  │  JobManager               │  │
│    └─────────────────────────────────────────────────────────────┘  │
│                               │                                     │
│         ┌──────────┬──────────┼──────────┬──────────┬─────────┐    │
│         ▼          ▼          ▼          ▼          ▼         ▼    │
│    ┌─────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────┐ │
│    │ config  │ │ agent  │ │ tools  │ │  mcp   │ │ hooks  │ │state│ │
│    │  配置    │ │ 模型   │ │ 工具集  │ │ 外部协议 │ │ 生命周期 │ │持久化│ │
│    └─────────┘ └────────┘ └────────┘ └────────┘ └────────┘ └────┘ │
│         │          │          │          │          │         │    │
│         └──────────┴──────────┴──────────┴──────────┴─────────┘    │
│                               │                                     │
│                               ▼                                     │
│                    ┌─────────────────────┐                          │
│                    │  protocol (DTO)      │                          │
│                    │  secrets (密钥管理)   │                          │
│                    └─────────────────────┘                          │
└─────────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          LLM 服务层                                 │
│                    ┌─────────────────────┐                          │
│                    │   ModelRegistry      │                          │
│                    │  XiaomiMimo / Custom │                          │
│                    └─────────────────────┘                          │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 3. Crate 依赖关系

```
                     ┌────────────────────────────────┐
                     │    接口适配层 (binary crates)     │
                     │  mimofan       (TUI+CLI 入口)   │
                     │  mimofan-app-server (HTTP)      │
                     └──────┬──────────┬──────────────┘
                            │          │
                            ▼          ▼
                     ┌────────────────────────────────┐
                     │       核心域 (core)             │
                     │  Runtime / Turn Loop /         │
                     │  ThreadManager / JobManager    │
                     └──────┬─────────────────────────┘
                            │
        ┌──────────┬────────┴────────┬──────────┬────────────┐
        ▼          ▼               ▼          ▼            ▼
   ┌─────────┐ ┌────────┐     ┌────────┐ ┌─────────┐  ┌────────┐
   │ config  │ │ agent  │     │ tools  │ │   mcp   │  │ hooks  │
   │配置+路由 │ │模型注册 │     │工具集   │ │外部工具协议│  │生命周期│
   └────┬────┘ └───┬────┘     └───┬────┘ └────┬────┘  └────┬───┘
        │          │               │          │            │
        ▼          ▼               ▼          ▼            ▼
   ┌─────────┐ ┌────────┐    ┌──────────────────────────────────┐
   │protocol │ │ exec   │    │  state (SQLite) / secrets       │
   │  DTO    │ │policy  │    │  持久化 / 密钥                   │
   └─────────┘ └────────┘    └──────────────────────────────────┘
```

**注意：** `mimofan`（TUI+CLI）不依赖 `mimofan-core`，它自己实现了运行时逻辑。

---

## 4. 限界上下文说明

| 上下文 | 解决什么问题 | 核心 crate |
|--------|-------------|------------|
| **配置上下文** | 加载 TOML、解析 profile、决定 provider/route | `mimofan-config` |
| **模型网关上下文** | 模型名解析、fallback 链 | `mimofan-agent` |
| **对话上下文** | 会话/消息生命周期、checkpoint、持久化 | `mimofan-core` + `mimofan-state` |
| **工具执行上下文** | 工具注册、MCP 桥接、执行策略 | `mimofan-tools` + `mimofan-mcp` + `mimofan-execpolicy` |
| **密钥上下文** | API key 存储（keyring + 文件） | `mimofan-secrets` |
| **协议上下文** | 客户端↔服务端 JSON DTO | `mimofan-protocol` |
| **接口适配上下文** | TUI / CLI / HTTP / IM 桥 | `mimofan` + `mimofan-cli`（库） + `mimofan-app-server` + `integrations/*` |

---

## 5. 依赖的三方组件

### 5.1 基础设施层

| 组件 | 版本 | 用途 |
|------|------|------|
| tokio | 1.50 | 异步运行时 |
| reqwest | 0.13 | LLM HTTP 客户端（rustls） |
| rusqlite | 0.32 | SQLite 持久化（bundled） |
| axum | 0.8 | HTTP 框架 |
| tower-http | 0.6 | CORS 等中间件 |
| clap | 4.5 | CLI 参数解析 |

### 5.2 数据 / 序列化

| 组件 | 版本 | 用途 |
|------|------|------|
| serde / serde_json | 1.0 / 1.0 | 配置、协议序列化 |
| toml / toml_edit | 1.0 / 0.23 | 配置文件读写 |
| chrono / uuid / semver | 0.4 / 1.11 / 1.0 | 时间戳、会话 ID、版本比较 |

### 5.3 可观测性 / 错误

| 组件 | 版本 | 用途 |
|------|------|------|
| tracing | 0.1 | 结构化日志 |
| anyhow / thiserror | 1.0 / 2.0 | 错误处理 |

### 5.4 安全 / 隔离

| 组件 | 用途 |
|------|------|
| rustls | TLS 终结 |
| Landlock / Bubblewrap / Seatbelt | sandbox 后端（Linux/macOS） |

### 5.5 用户态 / TUI

| 组件 | 用途 |
|------|------|
| ratatui / crossterm | TUI 渲染 |
| dotenvy | .env 加载 |

### 5.6 LLM 适配

**无官方 SDK 依赖** —— 自己实现 wire format 适配，减少三方依赖。

---

## 6. 提示词工程

### 6.1 文件位置

所有发给 LLM 的 prompt 模板在 **`crates/tui/src/prompts/`**，编译期通过 `include_str!` 内嵌。

### 6.2 分层宪法（Tier 1-9，优先级降序）

| Tier | 名称 | 文件 | 说明 |
|------|------|------|------|
| 1 | Constitution | `constitution.md` | 身份、行为准则、硬约束 |
| 2 | Statutes | `approvals/*.md` | 权限/审批相关 |
| 3 | Regulations | `modes/*.md` | 模式（Plan / Agent / YOLO）规则 |
| 4 | Project Law | `.mimofan/constitution.json` | 项目级追加硬约束 |
| 5 | Memory | `memory_guidance.md` | 长期记忆读取指引 |
| 6 | Live Evidence | 工具实时返回 | 当前对话上下文 |
| 7 | Handoffs | `compact.md` | 上下文压缩时使用 |
| 8 | Personality | `personalities/*.md` | 角色语气 |
| 9 | Continuation | `continuation.md` | 长任务续行衔接 |

### 6.3 改提示词的注意点

1. **不要在 `constitution.md` 加任务级规则**
2. **修改前先跑** `cargo test -p mimofan`
3. **分层宪法不可被项目级覆盖**（Tier 1-3 > Tier 4）

---

## 7. 核心能力入口

| 能力 | 代码入口 | 怎么扩展 |
|------|---------|---------|
| 加内置工具 | `crates/tools/src/lib.rs` | 实现 `Tool` trait |
| 桥接 MCP server | `crates/mcp/src/lib.rs` | 配置 `~/.mimofan/mcp.json` |
| 加生命周期钩子 | `crates/hooks/src/lib.rs` | 实现 `Hook` trait |
| 修改执行策略 | `crates/execpolicy/src/lib.rs` | 修改 `ExecPolicyEngine` 规则 |
| 加 sandbox 后端 | `crates/tui/src/sandbox/` | 实现 `SandboxBackend` trait |
| 加 slash 命令 | `crates/tui/src/commands/groups/<group>/` | 注册 `CommandGroup` |

---

## 8. 大文件摘要

以下文件超过 2000 行，已在 `.claudeignore` 中排除以节省 token。

| 文件 | 行数 | 用途 | 关键 API |
|------|------|------|----------|
| `localization.rs` | 4,698 | TUI 字符串翻译（zh-Hans） | `tr(MessageId)`、`Locale`、`MessageId` 枚举 |
| `prompts.rs` | 3,071 | 模式系统提示词 | `PromptSessionContext`、`build_system_prompt()` |
| `tui/widgets/mod.rs` | 2,986 | UI 组件实现 | `FooterWidget`、`HeaderWidget`、`AgentCard`、`ToolCard` |
| `tui/views/mod.rs` | 2,205 | 模态框/对话框视图系统 | `ModalKind` 枚举、`CommandPaletteAction`、视图渲染 |
| `tui/ui.rs` | 11,317 | UI 渲染主循环 | `render()`、`draw_*()` 系列函数 |
| `tui/lib.rs` | 6,827 | 模块声明与 re-export | 所有 pub 模块入口 |
| `tools/subagent/mod.rs` | 6,584 | 子智能体工具 | `SubagentTool`、`SubagentConfig` |
| `tui/app.rs` | 5,922 | TUI 应用状态机 | `App`、`AppEvent`、`handle_key()` |
| `config.rs` (tui) | 4,602 | TUI 配置管理 | `TuiConfig`、`load_tui_config()` |
| `runtime_threads.rs` | 3,943 | 线程运行时 | `RuntimeThread`、`ThreadHandle` |
| `core/engine.rs` | 3,678 | 引擎核心 | `Engine`、`TurnResult`、`dispatch_tool()` |
| `runtime_api.rs` | 3,444 | 运行时 API | `RuntimeApi`、`send_message()`、`cancel()` |
| `tools/shell.rs` | 3,413 | Shell 工具 | `ShellTool`、`ShellConfig`、`execute_command()` |
| `mcp.rs` (tui) | 3,261 | MCP 集成 | `McpManager`、`connect_server()`、`list_tools()` |

---

## 9. 常用函数和使用用例

### 9.1 启动 CLI

```rust
use mimofan::run_cli;

fn main() -> std::process::ExitCode {
    run_cli()
}
```

### 9.2 构造 Runtime（嵌入 Rust 程序）

```rust
use mimofan_core::Runtime;
use mimofan_config::{ConfigToml, load_config};
use mimofan_state::StateStore;
use mimofan_tools::ToolRegistry;
use std::sync::Arc;

let config: ConfigToml = load_config(None)?;
let state = StateStore::open("~/.mimofan/state.db")?;
let tools = Arc::new(ToolRegistry::with_builtins());

let runtime = Runtime::new(config, state, tools);
// ... 调用 runtime.thread_manager / runtime.jobs
```

### 9.3 发送用户消息

```rust
use mimofan_protocol::{PromptRequest, UserInputRequestEvent};

let req = PromptRequest {
    thread_id: thread.id.clone(),
    text: "帮我把 src/foo.rs 重构一下".into(),
    images: vec![],
};
let event: UserInputRequestEvent = req.into();
```

### 9.4 注册自定义工具

```rust
use mimofan_tools::{Tool, ToolRegistry, ToolCall, ToolResult};
use async_trait::async_trait;

struct MyTool;

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "我的自定义工具" }
    async fn invoke(&self, call: ToolCall) -> anyhow::Result<ToolResult> { todo!() }
}

let mut reg = ToolRegistry::with_builtins();
reg.register(Box::new(MyTool));
```

---

## 10. 快速定位指南

| 我想了解... | 去看... |
|------------|---------|
| CLI 命令解析 | `cli/src/lib.rs` |
| TUI 界面渲染 | `tui/src/tui/ui.rs`、`tui/src/tui/widgets/` |
| 对话轮次循环 | `tui/src/core/engine.rs` |
| LLM 配置 | `config/src/provider.rs`、`config/src/route/` |
| 工具执行 | `tools/src/lib.rs`、`protocol/src/tool.rs` |
| 子智能体管理 | `tui/src/tools/subagent/`、`tui/src/fleet/` |
| 安全策略 | `execpolicy/src/lib.rs` |
| 密钥存储 | `secrets/src/lib.rs` |
| 会话持久化 | `state/src/lib.rs` |
| MCP 集成 | `mcp/src/lib.rs`、`tui/src/mcp.rs` |
| 提示词构建 | `tui/src/prompts.rs`、`tui/src/prompts/` |

---

## 11. 扩展指南速查

| 你想做 | 看哪里 | 预计改动 |
|--------|--------|----------|
| 支持新的 LLM | `config.toml` 加 `[providers.<name>]` | 零代码 |
| 修改人格/风格 | `prompts/constitution.md` + `personalities/*.md` | 1-2 个 md |
| 加 slash 命令 | `commands/groups/` | ~100 行 |
| 加内置工具 | `tools/src/lib.rs` | ~150 行 |
| 桥接 MCP | `mcp/src/lib.rs` + `~/.mimofan/mcp.json` | 零代码 |
| 加 IM 桥 | `integrations/<bridge-name>/` | ~300-500 行 |
| 自定义主题 | `tui/src/` 主题相关 | ~50 行 |
| 改审批策略 | `execpolicy/src/lib.rs` | ~100 行 |
