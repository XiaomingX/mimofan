# mimofan 架构说明

> 面向中国开发者的架构文档。说人话，不堆术语。

---

## 1. 这个项目到底是什么

mimofan 是一个**跑在你终端里的 AI 编程搭档**。你用中文下指令，它调用大模型思考，再用工具（读文件、改代码、跑命令）把活干完。工作流是一个闭环：

```
你说话 → 模型想 → 工具干 → 结果回来 → 模型继续想 → 直到活干完
```

**对标：** Claude Code / OpenCode

**差异：** Rust 写的 · 本地优先 · MIT 协议 · 默认小米 MiMo 模型

---

## 2. 架构长什么样

### 2.1 整体分层

```
┌─────────────────────────────────────────────────────────────────────┐
│                        你（用户）能摸到的层                             │
│    ┌─────────────┐    ┌─────────────┐    ┌─────────────────────┐    │
│    │  TUI 终端界面  │    │  CLI 命令行  │    │   HTTP 接口 (axum) │    │
│    │  (ratatui)  │    │   (clap)    │    │   给外部系统调用的     │    │
│    └──────┬──────┘    └──────┬──────┘    └──────────┬─────────┘    │
└───────────┼──────────────────┼─────────────────────┼───────────────┘
            │                  │                     │
            └──────────────────┼─────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          核心引擎层                                  │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    Runtime（大管家）                          │  │
│  │   Turn Loop（对话循环）  │  ThreadManager  │  JobManager    │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                               │                                     │
│         ┌──────────┬──────────┼──────────┬──────────┬─────────┐    │
│         ▼          ▼          ▼          ▼          ▼         ▼    │
│    ┌─────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────┐ │
│    │ config  │ │ agent  │ │ tools  │ │  mcp   │ │ hooks  │ │state│ │
│    │ 配置管理 │ │ 模型网关 │ │ 工具集  │ │ 外部协议 │ │ 钩子   │ │持久化│ │
│    └─────────┘ └────────┘ └────────┘ └────────┘ └────────┘ └────┘ │
│         │          │          │          │          │         │    │
│         └──────────┴──────────┴──────────┴──────────┴─────────┘    │
│                               │                                     │
│                               ▼                                     │
│                    ┌─────────────────────┐                          │
│                    │  protocol (数据类型)  │                          │
│                    │  secrets (密钥管理)   │                          │
│                    └─────────────────────┘                          │
└─────────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        LLM 服务层                                   │
│                  ┌─────────────────────┐                            │
│                  │   ModelRegistry      │                            │
│                  │  小米MiMo / 自定义    │                            │
│                  └─────────────────────┘                            │
└─────────────────────────────────────────────────────────────────────┘
```

**一句话总结：** 用户界面 → 核心引擎 → 各种能力模块 → 大模型。上层调下层，下层不反过来调上层。

### 2.2 Crate 依赖关系

整个项目拆成 15 个 Rust crate（可以理解为 15 个模块），每个 crate 负责一件事：

```
                     ┌────────────────────────────────┐
                     │    你用的两个程序（二进制 crate）    │
                     │  mimofan       (TUI+CLI 入口)   │
                     │  mimofan-app-server (HTTP 接口)  │
                     └──────┬──────────┬──────────────┘
                            │          │
                            ▼          ▼
                     ┌────────────────────────────────┐
                     │       核心引擎 (core)            │
                     │  Runtime / Turn Loop /          │
                     │  ThreadManager / JobManager     │
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
   │  数据类型│ │policy  │    │  持久化 / 密钥                   │
   └─────────┘ └────────┘    └──────────────────────────────────┘
```

**注意一个关键点：** `mimofan`（TUI+CLI）**不依赖** `mimofan-core`。TUI 自己实现了一套运行时逻辑，两者是并行的关系。

### 15 个 Crate 干什么

| Crate | 干什么 | 一句话解释 |
|-------|--------|-----------|
| `mimofan` | TUI 界面 + CLI 入口 | 你直接用的东西，终端界面和命令行都在这里 |
| `mimofan-app-server` | HTTP 应用服务器 | 给外部系统调用的 API，用 axum 框架 |
| `mimofan-core` | 核心引擎 | 对话循环、会话管理、任务调度的大脑 |
| `mimofan-config` | 配置管理 | 读 TOML 配置、路由解析、服务商管理 |
| `mimofan-protocol` | 协议定义 | 工具、消息的数据类型定义，大家共用 |
| `mimofan-agent` | 子智能体系统 | 管理多个 AI 智能体的协作 |
| `mimofan-tools` | 内置工具 | 文件读写、Shell 命令等工具的实现 |
| `mimofan-mcp` | MCP 协议集成 | 连接外部工具服务器（Model Context Protocol） |
| `mimofan-hooks` | 生命周期钩子 | 工具执行前后的自动化动作 |
| `mimofan-execpolicy` | 执行策略 | 安全沙箱，控制哪些命令能跑 |
| `mimofan-secrets` | 密钥管理 | API Key 安全存储（keyring + 文件） |
| `mimofan-state` | 状态持久化 | SQLite 存会话历史和检查点 |
| `mimofan-memory` | 记忆系统 | 向量记忆（暂未集成到主流程） |
| `mimofan-release` | 发布工具 | 版本管理和发布 |
| `mimofan-localization` | 国际化文本层 | 100+ UI 调用点的 `tr(MessageId)`，当前仅内置简体中文 |

**关于 `tui` crate 的现状（重要）：** 它是最大的一块，约 20 万行，占全仓 85%+。其中 5 个最大的单文件（subagent、ui、shell、engine、config）已在 2026-08-04～05 按内聚性拆成子模块并合并（PR #567 / issue #566），文件变小了、可读性好了，但**所有子域仍在一个 crate 里**。更大的"按领域拆成独立 crate"属于战略级重构，不在本次范围内（详见 `ARCHITECTURE_IMPROVEMENT_PLAN.md`）。

---

## 3. 依赖的三方组件

### 3.1 基础设施

| 组件 | 用途 |
|------|------|
| tokio | 异步运行时，所有 I/O 都走这个 |
| reqwest | HTTP 客户端，调 LLM API 用的（rustls 加密） |
| rusqlite | SQLite 持久化（编译自带，不需要装系统库） |
| axum | HTTP 框架，app-server 用的 |
| clap | CLI 参数解析，命令行参数处理 |

### 3.2 数据序列化

| 组件 | 用途 |
|------|------|
| serde / serde_json | JSON 序列化，配置和协议数据都靠它 |
| toml / toml_edit | 配置文件读写 |
| chrono / uuid / semver | 时间戳、会话 ID、版本比较 |

### 3.3 日志与错误处理

| 组件 | 用途 |
|------|------|
| tracing | 结构化日志，排查问题用 |
| anyhow / thiserror | 错误处理（binary 用 anyhow，library 用 thiserror） |

### 3.4 安全与隔离

| 组件 | 用途 |
|------|------|
| rustls | TLS 加密连接 |
| Landlock / Bubblewrap / Seatbelt | 沙箱后端（Linux/macOS 分别用不同的） |

### 3.5 TUI 界面

| 组件 | 用途 |
|------|------|
| ratatui / crossterm | 终端界面渲染和键盘事件 |
| dotenvy | 加载 .env 环境变量 |

### 3.6 LLM 适配

**没有用任何官方 SDK**。项目自己实现了 OpenAI / Anthropic 的 wire format 适配，依赖面小、可控。这是刻意的设计决策。

---

## 4. 提示词工程

### 4.1 文件在哪

所有发给大模型的 prompt 模板在 **`crates/tui/src/prompts/`**，编译期通过 `include_str!` 直接嵌入二进制，运行时零 IO。

### 4.2 分层宪法（优先级从高到低）

| 层级 | 名称 | 文件 | 干什么 |
|------|------|------|--------|
| Tier 1 | Constitution | `constitution.md` | 身份、行为准则、硬约束（不可被覆盖） |
| Tier 2 | Statutes | `approvals/*.md` | 权限/审批规则 |
| Tier 3 | Regulations | `modes/*.md` | 模式规则（Plan / Agent / YOLO） |
| Tier 4 | Project Law | `.mimofan/constitution.json` | 项目级追加约束 |
| Tier 5 | Memory | `memory_guidance.md` | 长期记忆读取指引 |
| Tier 6 | Live Evidence | 工具实时返回 | 当前对话上下文 |
| Tier 7 | Handoffs | `compact.md` | 上下文压缩时使用 |
| Tier 8 | Personality | `personalities/*.md` | 角色语气 |
| Tier 9 | Continuation | `continuation.md` | 长任务续行衔接 |

**改提示词要注意：**
1. `constitution.md` 是最高优先级，不要往里面塞任务级规则
2. 改完必须跑 `cargo test -p mimofan` 验证
3. Tier 1-3 的内容不能被 Tier 4（项目级）覆盖

---

## 5. 核心能力入口

你想扩展某个功能，知道去哪找入口：

| 能力 | 代码入口 | 怎么扩展 |
|------|---------|---------|
| 加内置工具 | `crates/tools/src/lib.rs` | 实现 `Tool` trait |
| 桥接 MCP server | `crates/mcp/src/lib.rs` | 配置 `~/.mimofan/mcp.json` |
| 加生命周期钩子 | `crates/hooks/src/lib.rs` | 实现 `Hook` trait |
| 修改执行策略 | `crates/execpolicy/src/lib.rs` | 修改 `ExecPolicyEngine` 规则 |
| 加 sandbox 后端 | `crates/tui/src/sandbox/` | 实现 `SandboxBackend` trait |
| 加 slash 命令 | `crates/tui/src/commands/groups/<group>/` | 注册 `CommandGroup` |

---

## 6. 常用函数和使用用例

### 6.1 启动 CLI

```rust
use mimofan::run_cli;

fn main() -> std::process::ExitCode {
    run_cli()
}
```

### 6.2 嵌入到自己的 Rust 程序

```rust
use mimofan_core::Runtime;
use mimofan_config::{ConfigToml, load_config};
use mimofan_state::StateStore;
use mimofan_tools::ToolRegistry;
use std::sync::Arc;

// 加载配置
let config: ConfigToml = load_config(None)?;

// 打开持久化存储
let state = StateStore::open("~/.mimofan/state.db")?;

// 创建工具注册表（内置工具）
let tools = Arc::new(ToolRegistry::with_builtins());

// 创建运行时
let runtime = Runtime::new(config, state, tools);
// 接下来可以用 runtime.thread_manager / runtime.jobs 驱动对话
```

### 6.3 发送用户消息

```rust
use mimofan_protocol::{PromptRequest, UserInputRequestEvent};

let req = PromptRequest {
    thread_id: thread.id.clone(),
    text: "帮我把 src/foo.rs 重构一下".into(),
    images: vec![],
};
let event: UserInputRequestEvent = req.into();
```

### 6.4 注册自定义工具

```rust
use mimofan_tools::{Tool, ToolRegistry, ToolCall, ToolResult};
use async_trait::async_trait;

struct MyTool;

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "我的自定义工具" }

    async fn invoke(&self, call: ToolCall) -> anyhow::Result<ToolResult> {
        // 你的工具逻辑
        todo!()
    }
}

let mut reg = ToolRegistry::with_builtins();
reg.register(Box::new(MyTool));
```

---

## 7. 快速定位指南

| 我想了解... | 去看这个文件 |
|------------|------------|
| CLI 命令解析 | `tui/src/cli/mod.rs`（子命令在 `cli/` 各文件） |
| TUI 界面渲染 | `tui/src/tui/mod.rs`、`tui/src/tui/ui/mod.rs`、`tui/src/tui/ui/ui_event_loop.rs` |
| 对话轮次循环 | `tui/src/core/engine.rs`（消息 helper 在 `core/engine/`） |
| LLM 配置 | `config/src/provider.rs`、`config/src/route/` |
| 工具执行 | `tools/src/lib.rs`、`protocol/src/tool.rs` |
| 子智能体管理 | `tui/src/tools/subagent/`、`tui/src/fleet/` |
| 安全策略 | `execpolicy/src/lib.rs` |
| 密钥存储 | `secrets/src/lib.rs` |
| 会话持久化 | `state/src/lib.rs` |
| MCP 集成 | `mcp/src/lib.rs`、`tui/src/mcp.rs` |
| 提示词构建 | `tui/src/prompts.rs`、`tui/src/prompts/` |

---

## 8. 扩展速查

| 你想做 | 看哪里 | 大概改多少 |
|--------|--------|-----------|
| 支持新的 LLM | `config.toml` 加 `[providers.<name>]` | 零代码，改配置就行 |
| 修改 AI 人格 | `prompts/constitution.md` + `personalities/*.md` | 改 1-2 个 md 文件 |
| 加 slash 命令 | `commands/groups/` | ~100 行 |
| 加内置工具 | `tools/src/lib.rs` | ~150 行 |
| 桥接 MCP 工具 | `mcp/src/lib.rs` + `~/.mimofan/mcp.json` | 零代码，配置就行 |
| 自定义主题 | `tui/src/` 主题相关 | ~50 行 |
| 改审批策略 | `execpolicy/src/lib.rs` | ~100 行 |

---

> 最后更新：2026-08-05
