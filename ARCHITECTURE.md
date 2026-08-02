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
| **接口适配上下文** | TUI / CLI / HTTP / IM 桥 | `mimofan` + `mimofan-app-server` + `integrations/*` |

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

## 8. 大文件索引

以下文件稳定且很少修改，已从 `.claudeignore` 排除以节省 token；行数会变动，不在此逐一标注。完整列表见 `.claudeignore` 的「超大稳定模块」段。

- `crates/tui/src/tui/ui.rs` — UI 渲染主循环
- `crates/tui/src/lib.rs` — 模块声明与 re-export
- `crates/tui/src/tools/subagent/mod.rs` — 子智能体工具
- `crates/tui/src/tui/app.rs` — TUI 应用状态机
- `crates/tui/src/config.rs` — TUI 配置管理
- `crates/tui/src/runtime_threads.rs` — 线程运行时
- `crates/tui/src/core/engine.rs` — 引擎核心
- `crates/tui/src/runtime_api.rs` — 运行时 API
- `crates/tui/src/tools/shell.rs` — Shell 工具
- `crates/tui/src/mcp.rs` — MCP 集成
- `crates/tui/src/prompts.rs` — 模式系统提示词
- `crates/tui/src/localization.rs` — TUI 字符串翻译（zh-Hans）
- `crates/tui/src/tui/widgets/mod.rs` — UI 组件实现
- `crates/tui/src/tui/views/mod.rs` — 模态框/对话框视图

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

---

## 12. DDD 架构分析与改进计划

> 视角：资深架构师 · 第一性原理 + 领域驱动设计（DDD）
> 约束：本次重构**只动底层**（领域/基础设施），**不改动**用户与外部系统的交互层（TUI / CLI / MCP / HTTP / ACP 的对外接口与行为保持不变）。

### 12.1 第一性原理：这个系统到底解决什么

把"在终端里跑一个 AI 编程搭档"拆到不可再分：

- **输入**：用户的自然语言指令 + 当前代码库上下文。
- **决策**：调用哪家大模型、用什么人格/模式/审批策略。
- **动作**：读写文件、跑命令、调外部工具（MCP）。
- **闭环**：模型输出 → 工具执行 → 结果回灌 → 再决策，直到任务完成。
- **状态**：会话/记忆需持久化，密钥需安全管理。

由此自然导出几个本质关注点（限界上下文）：**配置与路由、模型网关、对话与状态、工具与外部协议、密钥与安全**。mimofan 用 14 个 crate 把这些关注点分开——这是它架构上最正确的决定。

### 12.2 架构精妙之处（已经做对的）

1. **线协议先行（`protocol` crate）**：App/Thread/Prompt/Event 等跨进程 DTO 单独成 crate，TUI、app-server、MCP 都围着同一套类型说话。这是 DDD 中"共享内核"的干净实现，让"多前端共享同一内核"成为可能。
2. **领域运行时聚合根（`core::Runtime`）**：把 ThreadManager、JobManager、工具/MCP/hooks 编排收口到一个 Runtime，而不是让 UI 到处 new。方向正确。
3. **配置单一内核（`config` crate）**：服务商、路由、模型清单、定价集中管理，全仓以 `ProviderKind` 为新类型标识，避免"字符串满天飞"。
4. **提示词分层 + 编译期嵌入**：constitution / modes / personalities / approvals 分层，用 `include_str!` 内嵌，字节稳定、零运行时 IO。是"提示词即代码"的好范式。
5. **端口化扩展（trait）**：加工具 = 实现 `Tool` trait，加钩子 = `Hook` trait，加沙箱后端 = `SandboxBackend` trait。符合 DDD"通过端口适配外部系统"，扩展面清晰。
6. **零官方 LLM SDK**：自研 OpenAI/Anthropic wire format 适配，依赖面小、可控。

### 12.3 架构边界问题（需要修的）

以下均为代码探查得到的事实，非臆断：

1. **`tui` crate 过度膨胀（最严重）**：392 个 `.rs` 文件、约 20.7 万行，占全仓绝对主体。TUI 渲染、CLI 派发、MCP 客户端、LLM 客户端、提示词、本地化全挤在一个 crate。违反单一职责，导致编译慢、认知负荷高、修改易牵连。
2. **UI 层直连底层 IO**：`tui/ui.rs` 直接在视图模块里 `static BALANCE_CLIENT: LazyLock<reqwest::Client>` 拉余额（网络 IO 在视图层）；`file_tree.rs`/`sidebar.rs`/`clipboard.rs` 等多处直接用 `std::fs`/`std::process`（文件 IO 在视图层）。DDD 中 UI 应是纯展现，IO 应通过端口（Repository / HttpClient）注入。
3. **MCP 职责边界表述需校正（原「双实现」措辞不准确）**：经核查，`mimofan-mcp`（`crates/mcp`）的 `Cargo.toml` 仅依赖 `anyhow/serde/serde_json`，**不含 `rmcp`**——它并非连接外部 server 的客户端，而是两类能力：(a) **服务端框架** `run_stdio_server`（把 mimofan 自身工具经 JSON-RPC stdio 暴露给外部 MCP 客户端，自身仅用 `InMemoryMcpClient` 桩做自检）；(b) **注入式代理** `McpManager`（持有 `Box<dyn McpManagedClient>`，代理 `list_tools/call_tool`，目前唯一实现是测试用的 `InMemoryMcpClient`）。真正基于 **rmcp** 连接外部 server 的客户端**只**在 `tui/src/mcp.rs`（约 3197 行）。两者是**互补**（服务端框架 + 注入式代理 vs 真实外部客户端），**并非重复实现**。关于「统一」的真实路径与约束，见第 12.5 节「统一 MCP 客户端」待办。
4. **`lib.rs` 上帝文件**：`tui/src/lib.rs` 同时承载 clap 定义、全局 `run()`、panic/signal 处理、以及 Doctor/Models/Eval/Fleet 等多个子命令处理函数。单一文件承担"入口 + 编排 + 多用例"。
5. **`mcp_server.rs` 跨层穿透**：`tui/src/mcp_server.rs` 直接 `use crate::client::ApiClient; crate::llm_client::LlmClient; crate::session_manager; crate::tools`，边界适配器穿透到内部领域对象。
6. **`config` 依赖扇出大**：config 被 agent/app-server/core/state/tui 共 5 个 crate 直接依赖，是事实上的共享内核；改动面大，且易诱使上层直接读配置而非经由端口。

### 12.4 目标架构（DDD 分层重构方案）

按 DDD 经典四层，把现有 crate 重新归类，明确依赖方向（上层依赖下层，下层不反向依赖）：

```
┌──────────────────────────────────────────────────────────────────┐
│ 接口适配层 (Interface Adapters) —— 用户/外部系统交互层（本次不动）  │
│  TUI(ratatui) │ CLI(clap) │ app-server(axum HTTP/SSE/stdio)        │
│  MCP server │ ACP server │ integrations/(feishu/weixin, Node)      │
└───────────────────────────────────────┬──────────────────────────┘
                                         │ 调用
┌────────────────────────────────────────▼──────────────────────────┐
│ 应用层 (Application) —— 用例编排                                       │
│  core::Runtime  (Turn Loop / ThreadManager / JobManager)            │
└───────────────────────────────────────┬──────────────────────────┘
                                         │ 依赖领域
┌────────────────────────────────────────▼──────────────────────────┐
│ 领域层 (Domain) —— 纯业务逻辑，不依赖 IO 框架                         │
│  config(配置/路由) │ agent(模型网关) │ tools(工具定义)                │
│  protocol(线协议 DTO) │ memory(记忆)                                 │
└───────────────────────────────────────┬──────────────────────────┘
                                         │ 通过端口适配
┌────────────────────────────────────────▼──────────────────────────┐
│ 基础设施层 (Infrastructure) —— 端口的实现                             │
│  secrets(密钥) │ execpolicy(沙箱/审批) │ hooks(生命周期)             │
│  mcp(外部工具协议) │ state(SQLite 持久化)                            │
└──────────────────────────────────────────────────────────────────┘
```

关键约束（奥卡姆：少即是多）：

- **只收敛职责，不制造 crate 数量爆炸**：优先在 `tui` crate 内部按子目录归并（已有 `tools/` `core/` `fleet/` 先例），不盲目拆几十个新 crate。
- **IO 收口**：UI 层不再直接 `reqwest`/`std::fs`，改为依赖 core 暴露的端口（如 `BalanceProvider`、`WorkspaceFs`），由基础设施层实现。
- **MCP 客户端归一（范围需校正）**：原「明确 `mimofan-mcp` 为唯一规范客户端」的措辞基于「双实现」误述。校正值：真实统一路径是让 `tui/src/mcp.rs` 的 rmcp 外部客户端**实现 `crates/mcp` 的 `McpManagedClient` trait**，并注入 `McpManager` 作为唯一编排入口（`tui::mcp` 同时保留「服务端暴露工具给外部」职责，即 `mcp_server.rs` 那类）。但这会改动 TUI 连接/调用外部 MCP 工具的方式，属**用户可见行为**，与 12.6「不改用户可见行为」约束冲突，故**不在本次存量优化范围**，留作后续单独评估行为兼容。详见 issue #530。
- **提示词/本地化解耦**：`prompts/` 与 `localization.rs` 与 UI 渲染无关，宜归到独立模块（甚至独立 crate），由 core/tui 按需引用，而非塞在 tui crate。

### 12.5 改进计划 checklist

**已实现（[x]）：**

- [x] 统一 `.claudeignore` / `.cursorignore` / `.windsurfignore`，减少 AI 上下文 token 浪费
- [x] 合并冗余 AI 工作流文档（AGENTS.md 内容并入 CLAUDE.md，删除 AGENTS.md）
- [x] 清理 LEGACY_ / dead_code 早期遗留（前期会话）
- [x] 移除无关第三方端点 `DEFAULT_NVIDIA_NIM_BASE_URL` 及其遗留强制改写逻辑
- [x] 移除 MiMo AMS 区域端点与 `xiaomi-mimo-v2-5-omni` 模型别名（含底层 const 与解析分支，统一为 SGP 默认 + CN 可选；`api.xiaomimimo.com/v1` 网关保留）

**待办（[ ]，均来自 12.3 的真实问题，非可有可无）：**

- [ ] 拆分 `crates/tui/src/tui/ui.rs` 上帝文件（实测 11234 行；目标单文件 < 1000 行；按 chat/sidebar/footer/picker 拆子模块）
- [x] 抽离 `tui/lib.rs` 的 panic-signal 处理到独立 `signals.rs`（无跨模块引用，零行为变化；clap 定义已抽离到 `cli.rs`（见 #533），`run()` / 子命令处理仍待进一步拆分）
- [x] 继续拆分 `tui/lib.rs`：clap 定义段（`Cli`/`Commands`/`Args`/`FeatureToggles` 及自由函数）已抽离到 `crates/tui/src/cli.rs`（纯物理拆分，零行为变化，编译干净，172 测试通过；见 #533）；`run()` 巨型函数（约 5874 行）与子命令处理仍待后续子 PR
- [ ] 将 UI 层直连 IO 收口到端口（余额请求、`std::fs` 散落点改为经 core 端口）
- [x] 统一 MCP 客户端：经核查 `mimofan-mcp` 非客户端实现（无 rmcp 依赖），与 `tui/src/mcp.rs` 是互补而非重复；真实统一路径=让 tui 的 rmcp 客户端实现 `crates/mcp` 的 `McpManagedClient` 并注入 `McpManager`。该改动涉及 TUI 连外部 MCP 工具的用户可见行为，受 12.6 约束，已决策**不实施**（见 issue #530，由 #531 收口界定范围），故标记完成
- [x] 收敛 `mcp_server.rs` 跨层穿透：引入 `McpBackend` 端口（依赖反转），删除死导入 `LlmClient`；`mcp_server` 仅依赖抽象、由组合根（`run_mcp_server`）注入 `RealMcpBackend`，不再直接 `use client`/`llm_client`/`session_manager`/`config`；对外签名与协议不变（见 PR 关联 issue #528）
- [ ] 提示词资源（`prompts/`）与本地化（`localization.rs`）从 tui crate UI 层解耦为独立模块
- [ ] 收敛 `config` 依赖扇出（上层经由端口读配置，避免直接耦合共享内核）
- [ ] 持续消除裸 `unwrap()`（全仓实测 441 处，生产路径替换为 `?` / `expect`；分批推进，见 #537）

### 12.6 不在本次范围

- 不改动任何对外交互接口与用户可见行为（CLI 子命令、MCP 工具、HTTP 路由、TUI 操作、配置键名）。
- 不新增功能；只做存量优化与边界收敛。
- 不全面翻译 20 万行英文注释（风险高、收益低）；仅清理确属无用的版权 boilerplate 与过时注释。
