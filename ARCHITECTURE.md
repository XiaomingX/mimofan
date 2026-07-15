# mimofan 架构说明文档

> 本文档面向开发者，介绍 mimofan 的定位、架构设计、模块边界、依赖项、提示词工程和扩展点。
>
> 改进计划详见 **[ARCHITECTURE_PLAN.md](ARCHITECTURE_PLAN.md)**。

---

## 1. 系统定位

mimofan 是一个**跑在本地的 AI 编码助手**。

用户用自然语言下指令，它调用大模型思考，再用工具（读文件、改代码、跑命令）把活干完。整个工作流是**"模型决策 → 工具执行 → 结果回灌 → 再决策"**的闭环。

**对标产品：** Claude Code / OpenCode

**差异点：**
- Rust 实现，本地优先（代码不上传）
- MIT 协议，完全开源
- 默认内置 Xiaomi MiMo，其他 provider 走 OpenAI 兼容协议

---

## 2. 架构分层视图

### 2.1 系统架构图

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
│    ┌─────────────────────────────────────────────────────────────┐  │
│    │                    ModelRegistry (模型注册表)                  │  │
│    └─────────────────────────────────────────────────────────────┘  │
│                               │                                     │
│                    ┌──────────┴──────────┐                          │
│                    ▼                      ▼                          │
│            ┌─────────────┐       ┌─────────────┐                    │
│            │ Xiaomi MiMo │       │   Custom    │                    │
│            │   (默认)     │       │ (OpenAI兼容) │                    │
│            └─────────────┘       └─────────────┘                    │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 Crate 依赖关系

```
                     ┌────────────────────────────┐
                     │    接口适配层 (binary crates) │
                     │  mimofan-tui   (TUI 入口)   │
                     │  mimofan-cli   (CLI 入口)   │
                     │  mimofan-app-server (HTTP)  │
                     └──────┬──────────┬──────────┘
                            │          │
                            ▼          ▼
                     ┌────────────────────────────┐
                     │       核心域 (core)         │
                     │  Runtime / Turn Loop /    │
                     │  ThreadManager / JobManager│
                     └──────┬─────────────────────┘
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

### 2.3 限界上下文说明

| 上下文 | 解决什么问题 | 核心 crate |
|--------|-------------|------------|
| **配置上下文** | 加载 TOML、解析 profile、决定 provider/route | `mimofan-config` |
| **模型网关上下文** | 模型名解析、fallback 链 | `mimofan-agent` |
| **对话上下文** | 会话/消息生命周期、checkpoint、持久化 | `mimofan-core` + `mimofan-state` |
| **工具执行上下文** | 工具注册、MCP 桥接、执行策略 | `mimofan-tools` + `mimofan-mcp` + `mimofan-execpolicy` |
| **密钥上下文** | API key 存储（keyring + 文件） | `mimofan-secrets` |
| **协议上下文** | 客户端↔服务端 JSON DTO | `mimofan-protocol` |
| **接口适配上下文** | TUI / CLI / HTTP / IM 桥 | `mimofan-tui` + `mimofan-cli` + `mimofan-app-server` + `integrations/*` |

---

## 3. 依赖的三方组件

### 3.1 基础设施层

| 组件 | 版本 | 用途 |
|------|------|------|
| tokio | 1.50 | 异步运行时 |
| reqwest | 0.13 | LLM HTTP 客户端（rustls） |
| rusqlite | 0.32 | SQLite 持久化 |
| axum | 0.8 | HTTP 框架 |
| tower-http | 0.6 | CORS 等中间件 |
| clap | 4.5 | CLI 参数解析 |

### 3.2 数据 / 序列化

| 组件 | 版本 | 用途 |
|------|------|------|
| serde / serde_json | 1.0 / 1.0 | 配置、协议序列化 |
| toml / toml_edit | 1.0 / 0.23 | 配置文件读写 |
| chrono / uuid / semver | 0.4 / 1.11 / 1.0 | 时间戳、会话 ID、版本比较 |

### 3.3 可观测性 / 错误

| 组件 | 版本 | 用途 |
|------|------|------|
| tracing | 0.1 | 结构化日志 |
| anyhow / thiserror | 1.0 / 2.0 | 错误处理 |

### 3.4 安全 / 隔离

| 组件 | 用途 |
|------|------|
| rustls | TLS 终结 |
| Landlock / Bubblewrap / Seatbelt | sandbox 后端（Linux/macOS） |

### 3.5 用户态 / TUI

| 组件 | 用途 |
|------|------|
| ratatui / crossterm | TUI 渲染 |
| dotenvy | .env 加载 |

### 3.6 LLM 适配

**无官方 SDK 依赖** —— 自己实现 wire format 适配，减少三方依赖。

---

## 4. 提示词工程

### 4.1 提示词文件位置

所有发给 LLM 的 prompt 模板在 **`crates/tui/src/prompts/`**，编译期通过 `include_str!` 内嵌。

### 4.2 分层宪法（Tier 1-9，优先级降序）

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

### 4.3 特殊提示词

| 文件 | 用途 |
|------|------|
| `constitution.md` | 系统身份 + 元规则 |
| `coding_assistant.md` | 编码场景默认角色 |
| `compaction_specialist.md` | 上下文压缩 |
| `subagent_output_format.md` | 子 agent 输出格式 |
| `locale_preamble_zh_hans.md` | 中文语言环境开场 |
| `v4_model_characteristics.md` | 不同模型家族微调 |

### 4.4 改提示词的注意点

1. **不要在 `constitution.md` 加任务级规则**
2. **修改前先跑** `cargo test -p mimofan-tui`
3. **分层宪法不可被项目级覆盖**（Tier 1-3 > Tier 4）

---

## 5. 核心能力入口

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
use mimofan_cli::run_cli;

fn main() -> std::process::ExitCode {
    mimofan_cli::run_cli()
}
```

### 6.2 构造 Runtime（嵌入 Rust 程序）

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
    async fn invoke(&self, call: ToolCall) -> anyhow::Result<ToolResult> { todo!() }
}

let mut reg = ToolRegistry::with_builtins();
reg.register(Box::new(MyTool));
```

---

## 7. 技术栈总结

| 层级 | 技术 | 用途 |
|------|------|------|
| CLI | clap | 命令行参数解析 |
| HTTP | axum | REST API 服务器 |
| TUI | ratatui + crossterm | 终端界面渲染 |
| 异步运行时 | tokio | 全异步 I/O |
| 序列化 | serde + serde_json + toml | 数据序列化 |
| HTTP 客户端 | reqwest (rustls) | LLM API 调用 |
| 数据库 | rusqlite (bundled) | SQLite 状态持久化 |
| 错误处理 | thiserror + anyhow | 类型化错误 |
| 日志 | tracing | 结构化日志 |

---

## 8. 快速定位指南

| 我想了解... | 去看... |
|------------|---------|
| CLI 命令解析 | `cli/src/main.rs`、`cli/src/args.rs` |
| TUI 界面渲染 | `tui/src/ui/`、`tui/src/widgets/` |
| 对话轮次循环 | `core/src/engine.rs` |
| LLM 配置 | `config/src/provider/` |
| 工具执行 | `tools/src/`、`protocol/src/tool.rs` |
| 子智能体管理 | `agent/src/`、`tui/src/fleet/` |
| 安全策略 | `execpolicy/src/` |
| 密钥存储 | `secrets/src/` |
| 会话持久化 | `state/src/` |
| MCP 集成 | `mcp/src/` |
| 国际化 | `tui/src/localization.rs` |
| 提示词构建 | `tui/src/prompts.rs` |

---

## 9. 扩展指南速查

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

---

## 10. 进一步阅读

- [USER_GUIDE.md](USER_GUIDE.md) — 终端用户使用手册
- [ARCHITECTURE_PLAN.md](ARCHITECTURE_PLAN.md) — 架构改进计划（DDD 视角）
- [docs/INSTALL.md](docs/INSTALL.md) — 安装方式
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) — 配置文件字段参考
- [docs/PROMPTS.md](docs/PROMPTS.md) — 提示词分层与索引
- [docs/MODES.md](docs/MODES.md) — TUI 模式与审批
- [docs/MCP.md](docs/MCP.md) — MCP 桥接
