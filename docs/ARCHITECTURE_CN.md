# CodeWhale 架构说明

> 本文档用大白话讲清楚 CodeWhale 是什么、怎么组织的、怎么用。

---

## 一句话说清楚

CodeWhale 是一个 **AI 编码助手**，你给它一句话（比如"帮我写个 REST API"），它就会调用大模型思考，然后用工具帮你改代码、跑命令、查文件，直到把活干完。

---

## 整体架构分层

```
┌──────────────────────────────────────────────────────────────┐
│                      你（用户）                                │
│              通过终端、命令行、HTTP API 交互                     │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                   用户交互层                                    │
│                                                               │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐   │
│   │  TUI 终端界面  │  │  CLI 命令行   │  │  HTTP/SSE 服务器  │   │
│   │  (ratatui)    │  │  (clap)      │  │  (axum)         │   │
│   │  crates/tui   │  │  crates/cli  │  │  crates/app-    │   │
│   │               │  │              │  │  server          │   │
│   └──────────────┘  └──────────────┘  └──────────────────┘   │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                   核心引擎层                                    │
│                                                               │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐   │
│   │  核心引擎      │  │  模型注册表   │  │  配置管理         │   │
│   │  crates/core  │  │  crates/     │  │  crates/config   │   │
│   │  (会话/线程/   │  │  agent       │  │  (TOML解析/      │   │
│   │   任务编排)    │  │  (模型路由)   │  │   provider配置)   │   │
│   └──────────────┘  └──────────────┘  └──────────────────┘   │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                   工具与策略层                                  │
│                                                               │
│   ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────┐   │
│   │  工具注册  │ │ 执行策略  │ │  钩子系统 │ │  密钥管理     │   │
│   │  /调度    │ │  引擎    │ │          │ │              │   │
│   │  tools   │ │execpolicy│ │  hooks   │ │  secrets     │   │
│   └──────────┘ └──────────┘ └──────────┘ └──────────────┘   │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                   基础设施层                                    │
│                                                               │
│   ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────┐   │
│   │ 协议类型  │ │ MCP 客户端│ │ 状态持久化│ │ Starlark引擎 │   │
│   │protocol  │ │   mcp    │ │  state   │ │  whaleflow   │   │
│   └──────────┘ └──────────┘ └──────────┘ └──────────────┘   │
└──────────────────────────────────────────────────────────────┘
```

### 各层职责（说人话）

- **用户交互层**：你看到的界面。TUI 是终端里的可视化界面，CLI 是命令行工具，HTTP 服务器让其他程序可以通过网络调用 CodeWhale。
- **核心引擎层**：大脑。管理对话（会话/线程）、决定调用哪个模型、读取配置。
- **工具与策略层**：手和脚。执行 shell 命令、读写文件、控制哪些操作需要你确认。
- **基础设施层**：地基。定义数据格式、连接外部 MCP 工具服务器、把数据存到 SQLite。

---

## Crate 依赖关系图

```
                    codewhale-cli
                   /      |      \
                  /       |       \
        app-server    release   secrets
         /  |  \        |         |
        /   |   \       |         |
     core  agent  mcp   |     config
      /|\    |     |    |      / \
     / | \   |     |    |     /   \
    /  |  \  |     |    |    /     \
 tools hooks state |    | execpolicy
    \   |    /     |    |    /
     \  |   /      |    |   /
    protocol --------+---+--/
                     |
                  whaleflow（独立，无内部依赖）
```

**简单理解**：
- `protocol` 是所有模块共用的"语言"（数据类型定义）
- `core` 是中枢，连接模型、工具、配置
- `tui` 是独立的终端 UI，走自己的集成路径
- `cli` 和 `app-server` 是两个入口

---

## 核心模块详解

### 1. protocol — 协议类型（基础设施层）

**位置**：`crates/protocol/src/`

**作用**：定义所有模块之间传递的数据格式。就像一份"合同"，大家都按这个格式来。

**子模块结构**（按限界上下文拆分）：
- `thread.rs` — 线程协议类型（Thread, ThreadRequest, ThreadResponse, Envelope 等）
- `event.rs` — 流式事件帧（EventFrame 及其 20+ 变体、MCP 启动事件、用户输入事件）
- `approval.rs` — 审批协议类型（AskForApproval, ReviewDecision, ApprovalDecisionRequest）
- `tool.rs` — 工具协议类型（ToolKind, ToolPayload, ToolOutput, LocalShellParams）
- `app.rs` — 应用层请求/响应（AppRequest, AppResponse, PromptRequest, PromptResponse）
- `lib.rs` — 仅做 `pub use` 重导出，所有外部 `use codewhale_protocol::*` 路径不变

**特点**：零依赖，纯数据定义，任何模块都可以安全引用。子模块拆分后保持完全向后兼容。

### 2. agent — 模型注册表（核心引擎层）

**位置**：`crates/agent/src/`

**作用**：管理所有支持的 AI 模型。你告诉它"我要用 deepseek-v4-pro"，它帮你找到正确的 provider 和端点。

**子模块结构**：
- `family.rs` — `ModelFamily` 枚举及 `model_family()` 分类函数（DeepSeek、OpenAI、Anthropic 等 11 个家族）
- `provider_resolver.rs` — 模型匹配、大小写保持、provider 特定的 passthrough 逻辑（Atlascloud、Arcee、小米 MiMo）
- `lib.rs` — `ModelRegistry`（60+ 模型配置）、`ModelInfo`、`ModelResolution` 及 `resolve()` 方法

**使用示例**：
```rust
use codewhale_agent::{ModelRegistry, model_family, ModelFamily};

let registry = ModelRegistry::default();
let result = registry.resolve(Some("deepseek-v4-pro"), None);
// result.resolved.id == "deepseek-v4-pro"
// result.resolved.provider == ProviderKind::Deepseek

assert_eq!(model_family("deepseek-v4-pro"), ModelFamily::DeepSeek);
```

### 3. tools — 工具系统（工具与策略层）

**位置**：`crates/tools/src/`

**作用**：定义工具的抽象接口和注册调度机制。所有工具（shell、文件操作、MCP 工具）都通过这里注册和调用。

**核心类型**：
- `ToolHandler` trait — 工具实现者必须实现的接口
- `ToolRegistry` — 工具注册表，管理所有已注册的工具
- `ToolSpec` — 工具规格描述（名称、输入 schema、超时等）
- `ToolResult` — 工具执行结果
- `ToolError` — 工具执行错误

**如何注册一个新工具**：
```rust
use codewhale_tools::{ToolHandler, ToolSpec, ToolRegistry, ToolInvocation, ToolOutput};
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
registry.register(spec, Arc::new(MyTool)).unwrap();
```

### 4. config — 配置管理（核心引擎层）

**位置**：`crates/config/src/`

**作用**：读取和解析 `~/.codewhale/config.toml` 配置文件，管理 provider 配置、模型设置、执行策略等。

**配置文件位置**：
- `~/.codewhale/config.toml` — 主配置文件
- `~/.codewhale/settings.toml` — 运行时设置
- `/etc/deepseek/managed_config.toml` — 系统级默认配置（Unix）

**核心类型**：
- `ConfigToml` — 顶层配置结构
- `ProvidersToml` — 所有 provider 的配置（DeepSeek、OpenAI、Anthropic 等）
- `ProviderConfigToml` — 单个 provider 的配置（API key、base URL、model）
- `ProviderKind` — provider 类型枚举

### 5. core — 核心引擎（核心引擎层）

**位置**：`crates/core/src/`

**作用**：编排一切。管理对话会话、调度工具调用、处理审批流程、启动 MCP 服务器。

**子模块结构**（按领域职责拆分）：
- `thread.rs` — `ThreadManager`（线程生命周期：创建/恢复/fork/归档）、`InitialHistory`、`NewThread`、线程目标管理
- `job.rs` — `JobManager`（后台任务：入队/运行/暂停/完成/失败/重试）、`JobStatus`、`JobRetryMetadata`、持久化编解码
- `lib.rs` — `Runtime`（顶层编排器，组合 config、model_registry、thread_manager、tool_registry、mcp_manager、exec_policy、hooks、jobs）

**核心类型**：
- `Runtime` — 顶层运行时，所有子系统的组合入口
- `InitialHistory` — 新对话的初始化方式（新建/fork/恢复）
- `NewThread` — 创建/恢复线程的结果
- `JobStatus` / `JobRetryMetadata` — 后台任务状态和重试逻辑
- `ThreadGoal` / `ThreadGoalStatus` — 对话目标管理

### 6. execpolicy — 执行策略引擎（工具与策略层）

**位置**：`crates/execpolicy/src/`

**作用**：决定一个工具调用是否需要用户审批。根据规则引擎匹配操作类型，返回"自动放行"、"建议审批"、"必须审批"或"拒绝"。

**核心类型**：
- `ExecPolicyEngine` — 策略引擎
- `AskForApproval` — 审批策略枚举（Never / OnRequest / OnFailure / UnlessTrusted）
- `ExecPolicyDecision` — 策略决策结果
- `ToolAskRule` — 工具审批规则

### 7. mcp — MCP 客户端（基础设施层）

**位置**：`crates/mcp/src/`

**作用**：连接外部 MCP（Model Context Protocol）工具服务器。MCP 是一种让 AI 助手调用外部工具的标准协议。

**使用方式**：在 `~/.codewhale/mcp.json` 中配置 MCP 服务器，CodeWhale 启动时自动发现并注册这些工具。

### 8. state — 状态持久化（基础设施层）

**位置**：`crates/state/src/`

**作用**：用 SQLite 存储会话、线程、任务等持久化数据。确保对话历史、任务状态在重启后不丢失。

---

## 依赖的第三方核心组件

| 组件 | 用途 | 出现在哪个 crate |
|------|------|-----------------|
| `tokio` | 异步运行时 | 几乎所有 crate |
| `axum` | HTTP 服务器框架 | app-server, tui |
| `clap` | 命令行参数解析 | cli, tui |
| `reqwest` | HTTP 客户端 | hooks, mcp, app-server |
| `serde` / `serde_json` | 序列化/反序列化 | 几乎所有 crate |
| `rusqlite` | SQLite 数据库 | state |
| `ratatui` | 终端 UI 框架 | tui |
| `tracing` | 日志追踪 | 几乎所有 crate |
| `anyhow` / `thiserror` | 错误处理 | 几乎所有 crate |
| `async-trait` | 异步 trait 支持 | tools, hooks |
| `tower-http` | HTTP 中间件 | app-server |
| `toml` / `toml_edit` | TOML 配置解析 | config |
| `keyring` | 系统密钥环 | secrets |
| `starlark` | Starlark 脚本引擎 | whaleflow |

---

## 提示词工程体系

CodeWhale 的提示词采用**分层治理**架构，优先级从高到低：

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
agent 根据配置解析出模型 (deepseek-v4-pro)
    │
    ▼
LLM 客户端发送 HTTP 请求到 DeepSeek API
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

1. 编辑 `~/.codewhale/mcp.json`，添加服务器配置
2. CodeWhale 启动时自动连接并注册工具
3. 工具自动对 LLM 可见

### 添加 Skills 技能

1. 在 `~/.codewhale/skills/` 下创建目录
2. 添加 `SKILL.md` 文件定义技能提示词
3. 可选添加辅助脚本
4. 通过 `load_skill` 工具调用

### 添加 Hooks 钩子

在 `~/.codewhale/config.toml` 中配置：

```toml
[[hooks]]
event = "tool_call_before"
command = "echo 'Running tool: $TOOL_NAME'"
```

---

## 配置文件说明

| 文件路径 | 说明 |
|---------|------|
| `~/.codewhale/config.toml` | 主配置（provider、模型、策略） |
| `~/.codewhale/settings.toml` | 运行时设置（模式、UI 偏好） |
| `~/.codewhale/mcp.json` | MCP 服务器配置 |
| `~/.codewhale/skills/` | 用户自定义技能目录 |
| `~/.codewhale/sessions/` | 会话历史 |
| `~/.codewhale/tasks/` | 后台任务记录 |
| `~/.codewhale/snapshots/` | 工作区快照（用于恢复） |
| `~/.codewhale/audit.log` | 审计日志 |

---

## 架构改进记录

基于 DDD 理论分析，已完成以下架构改进（详见 `ARCHITECTURE_REFORM.md`）：

### 已完成

| 阶段 | 改动 | 效果 |
|------|------|------|
| Phase 1 | protocol 按限界上下文拆分为 8 个子模块 | lib.rs 714→17 行，代码可读性大幅提升 |
| Phase 2 | agent 抽取 family.rs + provider_resolver.rs | 模型领域逻辑清晰分离 |
| Phase 3 | core 拆分 job.rs + thread.rs | lib.rs 2767→1348 行，职责边界明确 |

### 评估后暂缓

| 阶段 | 原因 |
|------|------|
| Phase 4 提示词独立 | 提示词组装逻辑（35K tokens）深度耦合 TUI 类型，独立收益有限 |
| Phase 5 消除跨层依赖 | config→execpolicy 和 tui→release 都是合理的领域依赖 |
| Phase 6 统一运行时 | TUI 运行时（ratatui 渲染、子代理、MCP OAuth）与 core 的 API 编排是根本不同的设计 |
