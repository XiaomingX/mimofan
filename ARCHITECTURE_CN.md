# mimofan 架构说明

> 面向中国开发者的架构文档。说人话，不堆术语。
>
> 配套文档：`ARCHITECTURE_IMPROVEMENT_PLAN.md`（DDD 分析与改进待办）、`ARCHITECTURE_STABILITY.md`（稳定性/性能/可扩展性）、`USER_GUIDE_CN.md`（使用说明）、`EVOLUTION_CRAWLER.md`（百亿级分布式爬虫演进路线）。
>
> 最后更新：2026-09-03（以当前 `main` 代码为准复核：crate 数 15→**19**、补充 edit_core/goal_core/staticanalysis/telemetry 四个新 crate、更正"TUI 不依赖 core"的失真表述、新增 §5 安全检测能力）。

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

整个项目拆成 **19 个 Rust crate**（可以理解为 19 个模块），每个 crate 负责一件事：

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

**注意一个关键点（已更正）：** `mimofan`（TUI+CLI）**依赖** `mimofan-core`。TUI 既自实现了一套交互式引擎（`crate::core::Engine`，管流式/轮次/终端暂停），又通过 `mimofan_core::Runtime`（无界面 API 核心）复用会话编排、任务调度等底层能力（见 `crates/tui/src/runtime_threads/mod.rs:44`、`crates/tui/src/lib.rs:708` 等）。两者是 DDD 下两个正确的限界上下文（交互 UI 循环 vs 无界面 API 核心），通过共享内核（protocol/tools/execpolicy/state/config）协同，而非互斥。

### 19 个 Crate 干什么

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
| `mimofan-memory` | 记忆系统 | 向量记忆（已默认编译，经 `vector_memory` 接入主流程作语义召回互补层；运行时按 `MIMOFAN_MEMORY_API_KEY` 优雅降级，仍 experimental） |
| `mimofan-release` | 发布工具 | 版本管理和发布 |
| `mimofan-localization` | 国际化文本层 | 100+ UI 调用点的 `tr(MessageId)`，当前仅内置简体中文（`localization` 已是独立 crate，精简为 ~100 行 stub） |
| `mimofan-edit-core` | 编辑正确性逻辑 | 下沉自 `tools/file.rs` 的纯逻辑：字节区间→行号映射、锚点（内容哈希）定位、模糊匹配、读前必读守卫（`ReadState`），零依赖，可独立单测 |
| `mimofan-goal-core` | 目标管理状态机 | 下沉自 `tools/goal.rs` 的纯逻辑目标机：`GoalState`/`GoalQueue`、token 预算护栏、依赖 DAG 与环检测、快照恢复，**不依赖 TUI** |
| `mimofan-staticanalysis` | 静态分析（SAST）地基 | tree-sitter 抽象语法树分析：调用图、污点/数据流、跨过程污点、访问控制、攻击面枚举、gadget 链发现、SARIF、SCA/OSV。`crates/staticanalysis/src/lib.rs` 是其入口 |
| `mimofan-telemetry` | 可观测性 | feature-gated 的 OpenTelemetry 桥 + 无依赖的进程内 Prometheus 指标记录器；默认 `otlp` feature 关闭，主二进制默认惰性（`OtelHandle::Disabled`） |

**关于 `tui` crate 的现状（重要）：** 它是最大的一块，约 **26.6 万行**（2026-09-03 实测 `266,340` 行），占全仓（`313,501` 行）约 **85%**。其中 5 个最大的单文件（subagent、ui、shell、engine、config）已在 2026-08-04～05 按内聚性拆成子模块并合并（PR #567 / issue #566），文件变小了、可读性好了，但**所有子域仍在一个 crate 里**。"按领域拆成独立 crate"属于战略级重构，不在本次范围内（详见 `ARCHITECTURE_IMPROVEMENT_PLAN.md` §4.8 的 DDD 重构总纲）。

**一句话理解分层（说人话版）：**
- **你摸得到的**：终端界面（TUI）、命令行（CLI）、HTTP 接口（给外部系统调）——这层是对外契约，用法恒定。
- **大脑**：两套"应用核心"——`Engine`（管交互式对话循环）和 `Runtime`（管无界面 API 编排）。两者是 DDD 下两个正确的限界上下文，共享底层能力。
- **手脚**：工具、模型网关、执行策略、记忆、持久化等子域，各管一摊。
- **地基**：协议类型、SQLite、密钥、配置等基础设施。

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

**没有用任何官方 SDK**。项目自己实现了 OpenAI / Anthropic / Gemini 的 wire format 适配，依赖面小、可控。这是刻意的设计决策。

### 3.7 新增强领域组件

| 组件 | 用途 | 所在 crate |
|------|------|-----------|
| tree-sitter | 语法树解析，静态分析地基 | `staticanalysis` |
| semgrep（外部 CLI） | 真实安全审计扫描（离线调用） | 通过 `SandboxBackend` 调起 |
| hnsw_rs + sled | 向量检索 + 嵌入式 KV（memory 实验性能力） | `memory` |
| tiktoken-rs | 真实 BPE 分词（cl100k_base / o200k_base），替代 bytes/4 启发式估算 | workspace |
| opentelemetry（可选 feature） | 可观测性链路（`otlp` feature 启用） | `telemetry` |

> 说明：以上多为**可选/实验性**能力，不进入默认编译链路（telemetry 默认 `Disabled`，memory 按 `MIMOFAN_MEMORY_API_KEY` 优雅降级）。

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
| 加内置工具 | `crates/tools/src/lib.rs` | 实现 `ToolHandler` trait（`crates/tools/src/lib.rs:385`），注册进 `ToolRegistry` |
| 桥接 MCP server | `crates/mcp/src/lib.rs` | 配置 `~/.mimofan/mcp.json` |
| 加生命周期钩子 | `crates/hooks/src/lib.rs` | 实现 `HookSink` trait（`crates/hooks/src/lib.rs:198`） |
| 修改执行策略 | `crates/execpolicy/src/lib.rs` | 修改 `ExecPolicyEngine` 规则（`ExecPolicyEngine::check`） |
| 加 sandbox 后端 | `crates/tui/src/sandbox/` | 实现 `SandboxBackend` trait（`crates/tui/src/sandbox/backend.rs:70`） |
| 加 slash 命令 | `crates/tui/src/commands/groups/<group>/` | 注册 `CommandGroup` |
| 加静态分析引擎 | `crates/staticanalysis/src/` | 在 `lib.rs` 暴露新分析模块（污点/调用图/访问控制等） |

### 5.1 新增强能力速览（已并入 `main`，非待办）

> 这些能力均在 2026-08 之后陆续合并进 `main`（如 v0.0.22），是**已实现**状态。详见 `docs/NEW_CAPABILITIES_GUIDE.md` 与 `plans/13-*`。

| 能力 | 入口 | 一句话 |
|------|------|--------|
| **网络安全检测** | `crates/tui/src/tools/security_audit_tool.rs` 等 | 把离线静态分析引擎（semgrep/攻击面/协议/访问控制/gadget 链）包装成模型可调用的安全工具，`crates/tui/src/core/engine/tool_setup.rs:97-103` 接线；还随 release 内置了 `vuln-hunt` / `security-audit` skill |
| **静态分析地基** | `crates/staticanalysis/src/lib.rs` | tree-sitter AST：调用图、污点/数据流、跨过程污点（`interproc.rs`）、访问控制（`access_control.rs`）、攻击面（`attack_surface.rs`）、gadget 链（`auto_gadget.rs`）、SARIF、SCA/OSV |
| **长程任务轨迹** | `crates/tui/src/core/engine/trace.rs` | 追加式 JSONL 会话轨迹（`trajectory.jsonl`），默认开启（opt-out，`config/notifications.rs:134-143`），供标注/分析/训练数据源；工具输出截断 16 KiB 并脱敏 |
| **评测/数据闭环** | `crates/tui/src/eval/mod.rs` + `benchmark/vuln_hunt/evaluate.py` | 离线 harness 用 Mock LLM 驱动真实 `ToolRegistry`，记录轨迹并持久化产物，供三维校验器（一致性/追踪/复现）评分，回灌得分闭环 |
| **安全审核工具族** | `crates/tui/src/tools/security_audit.rs` 等 | 真实调用 `semgrep` 命令、解析 SARIF、映射为 `ReviewIssue` 的安全审计工具 |
| **研究运维斜杠指令** | `/evolve` `/repro` `/artifact` `/reviewer` | 可机评优化回路、可复现性纪律、研究成果物汇总、独立评审者（详见 `docs/NEW_CAPABILITIES_GUIDE.md`） |

> 注意：安全检测工具族（security_audit / attack_surface / protocol_check / access_control / gadget_chain / auto_gadget / run_poc）**并非全部默认常驻**在普通 Agent 路径；`with_agent_tools_policy`（`crates/tui/src/tools/registry.rs:1210`）默认只带 `with_hypothesis_tools()`，完整安全面经 `with_full_agent_surface`（`:1305`）与 `tool_setup.rs` 接线。写文档/演示时要注意区分，避免"以为默认开、其实要特定入口"。

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
use mimofan_config::{ConfigStore, ConfigToml};
use mimofan_state::StateStore;
use mimofan_tools::ToolRegistry;
use std::sync::Arc;

// 加载配置（入口是 ConfigStore::load，不是 load_config）
let config: ConfigToml = ConfigStore::load(None)?.config;

// 打开持久化存储
let state = StateStore::open(Some(PathBuf::from("~/.mimofan/state.db")))?;

// 创建工具注册表（内置工具）
let tools = Arc::new(ToolRegistry::with_builtins());

// 创建运行时
let runtime = Runtime::new(config, state, tools);
// 接下来可以用 runtime.thread_manager / runtime.jobs 驱动对话
```

> 说明：真实签名以代码为准。`ConfigStore` 定义在 `crates/config/src/lib.rs:820`，`ConfigStore::load` 在 `:821`；`StateStore` 在 `crates/state/src/lib.rs:272`；`Runtime` 在 `crates/core/src/lib.rs:35`。

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
use mimofan_tools::{ToolHandler, ToolRegistry, ToolSpec, ToolInvocation, ToolOutput, ToolKind};
use async_trait::async_trait;

struct MyTool;

#[async_trait]
impl ToolHandler for MyTool {
    fn kind(&self) -> ToolKind { ToolKind::ReadOnly }

    async fn handle(&self, call: ToolInvocation) -> std::result::Result<ToolOutput, FunctionCallError> {
        // 你的工具逻辑
        todo!()
    }
}

// 工具的名字/schema 放在 ToolSpec 里，注册时一并传入
let mut reg = ToolRegistry::new();
reg.register(ToolSpec { name: "my_tool".into(), /* ... */ }, Arc::new(MyTool));
```

> 说明：工具调用 trait 实际叫 **`ToolHandler`**（`crates/tools/src/lib.rs:385`），名字/schema 放在 `ToolSpec`（`:268`）而非 trait 上。此示例为示意，完整字段见 `crates/tools/src/lib.rs`。

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
| 安全检测工具 | `tui/src/tools/security_audit*.rs`、`staticanalysis/src/` |
| 长程任务轨迹 | `tui/src/core/engine/trace.rs`、`tui/src/config/notifications.rs` |
| 评测闭环 | `tui/src/eval/mod.rs`、`benchmark/vuln_hunt/` |
| 编辑正确性 | `edit_core/src/lib.rs` |
| 目标状态机 | `goal_core/src/lib.rs` |

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

## 9. 并发与稳定性（一句话版）

- **工具并发门禁** `ToolCallRuntime`（`crates/tools/src/lib.rs:416`）用 `tokio::sync::RwLock` + 重入保护，是规范写法，**不会死锁**。
- **app-server** 已把 `Arc<Mutex<Runtime>>` 改为 `Arc<RwLock<Runtime>>`（`crates/app-server/src/lib.rs:75`）：`&mut self` 方法走写锁，`&self` 方法（invoke_tool / app_status / mcp_startup）走读锁可并发，消除队头阻塞。详见 `ARCHITECTURE_STABILITY.md` §1。
- **没有发现**活死锁或内存泄漏；绝大多数 `std::sync::Mutex` 守卫都被限制在同步代码块内、不跨 `.await`。
- **已知一处真实内存增长隐患**：`mimofan-telemetry` 的 `PrometheusRecorder::histograms`（`crates/telemetry/src/lib.rs:146`）是 `Vec<f64>` 且按标签（含 model 名）无限追加、从不淘汰——长进程下会单调增长。当前默认 feature 关闭、影响面小，但作为归档风险记录在稳定性文档。
- 完整核实报告见 **`ARCHITECTURE_STABILITY.md`**；未来演进路线见 **`EVOLUTION_CRAWLER.md`**；DDD 优化清单见 **`ARCHITECTURE_IMPROVEMENT_PLAN.md`**。

---

> 最后更新：2026-09-03
