# mimofan 架构说明（ARCHITECTURE）

> 本文档面向开发者，用大白话讲清楚 mimofan 的定位、架构设计、模块边界、依赖项、提示词工程、扩展点，以及后续可落地的优化方向。
>
> **本仓库 `mimofan` 已经完成架构收敛：唯一内置 LLM 提供方是 Xiaomi MiMo，其他模型（包括 DeepSeek、Claude、GPT、Kimi、GLM 等）均通过 `Custom`（OpenAI 兼容协议）走同一套适配路径。** 历史 README 中提到的"多 provider"已收敛为二元 `ProviderKind`，这一点在阅读下面的 DDD 分析时请先记住。

---

## 0. 一句话说清楚 mimofan 是什么

mimofan 是一个 **跑在你本机终端的 AI 编码助手**：你用自然语言下指令，它调用大模型思考，再用工具（读文件、改代码、跑命令、查 MCP）把活干完。整个工作流是"模型决策 → 工具执行 → 结果回灌 → 再决策"的闭环，直到任务完成或被中止。

它对标的产品是 Claude Code / OpenCode，差异点是：
- 完全 Rust + 本地优先（不上传代码）
- MIT 协议
- 唯一的内置 provider 是 Xiaomi MiMo（其他 provider 走 Custom 协议适配）

---

## 1. 架构分层视图

### 1.1 全局调用链（sequence）

```
┌──────────┐         ┌───────────────┐         ┌────────────────┐
│  用户      │ ──输入─→ │ 接口适配层         │ ──事件──→ │ 核心引擎         │
│（终端/API）│ ←─渲染─ │ TUI / CLI / HTTP │ ←─状态── │ (Runtime +      │
└──────────┘         └───────────────┘         │  Turn Loop)    │
                                                 └──────┬─────────┘
                                                        │ 决策
                                                        ▼
                                                ┌────────────────┐
                                                │ 模型网关        │
                                                │ ModelRegistry  │
                                                │ Provider       │
                                                └──────┬─────────┘
                                                       │ HTTP/SSE
                                                       ▼
                                                ┌────────────────┐
                                                │ 大模型          │
                                                │（Xiaomi MiMo / │
                                                │ Custom OpenAI  │
                                                │ 兼容端点）       │
                                                └────────────────┘
```

### 1.2 Crate 物理分层（依赖图）

```
                ┌────────────────────────────┐
                │ 接口适配层 (binary crates)   │
                │                            │
                │  mimofan-tui   (TUI 入口)   │
                │  mimofan-cli   (CLI 入口)   │
                │  mimofan-app-server (HTTP)  │
                └──────┬──────────┬──────────┘
                       │          │
                       ▼          ▼
                ┌────────────────────────────┐
                │ 核心域 (core)                │
                │  mimofan-core              │
                │  Runtime / ThreadManager / │
                │  JobManager / Turn Loop    │
                └──────┬─────────────────────┘
                       │
        ┌──────────┬───┴────┬──────────┬────────────┐
        ▼          ▼        ▼          ▼            ▼
   ┌─────────┐ ┌────────┐ ┌──────┐ ┌──────────┐ ┌──────────┐
   │ config  │ │ agent  │ │tools │ │   mcp    │ │  hooks   │
   │配置+路由 │ │模型注册 │ │工具集 │ │外部工具协议│ │生命周期钩子│
   └────┬────┘ └───┬────┘ └──┬───┘ └────┬─────┘ └────┬─────┘
        │          │         │          │            │
        │          │         │          │            │
        ▼          ▼         ▼          ▼            ▼
   ┌─────────┐ ┌────────┐ ┌──────────────────────────┐
   │protocol │ │ exec   │ │ state (SQLite) / secrets │
   │  DTO    │ │policy  │ │ 持久化 / 密钥              │
   └─────────┘ └────────┘ └──────────────────────────┘

   还有：mimofan-release（版本检查工具）— 与核心域无耦合
```

### 1.3 限界上下文（Bounded Context）

| 上下文 | 解决什么业务问题 | 主要 crate / 模块 | 关键类型 |
|------|----------------|----------------|---------|
| **配置上下文** | 加载 TOML、解析 profile、决定 provider/route/model | `mimofan-config` | `ConfigToml`、`ResolvedRuntimeOptions`、`ProviderKind` |
| **模型网关上下文** | 把用户输入的"模型名"解析为具体 provider/model ID，处理 fallback 链 | `mimofan-agent` | `ModelRegistry`、`ModelResolution`、`ModelInfo` |
| **对话上下文** | 会话/消息生命周期、checkpoint、持久化 | `mimofan-core` 的 `ThreadManager`、`JobManager` + `mimofan-state` | `Runtime`、`Thread`、`Message` |
| **工具执行上下文** | 注册工具、桥接 MCP、执行前策略评估、钩子调度 | `mimofan-tools` + `mimofan-mcp` + `mimofan-execpolicy` + `mimofan-hooks` | `ToolRegistry`、`McpManager`、`ExecPolicyEngine`、`HookDispatcher` |
| **密钥 / 持久化上下文** | API key 存储（系统 keyring + 文件回退）、SQLite 状态 | `mimofan-secrets` + `mimofan-state` | `Secrets`、`StateStore` |
| **线缆协议上下文** | 客户端↔服务端的应用层 JSON DTO | `mimofan-protocol` | `EventFrame`、`ThreadRequest`、`AppResponse` |
| **接口适配上下文** | 把上面的领域对象渲染成不同形态（终端/CLI/HTTP/IM 桥） | `mimofan-tui` + `mimofan-cli` + `mimofan-app-server` + `integrations/*` | — |

---

## 2. 依赖的三方组件（按层）

### 2.1 基础设施层
- **tokio** (`1.50`) — 异步运行时，所有 I/O 路径都基于 tokio。
- **reqwest** (`0.13` + rustls-no-provider) — LLM HTTP 客户端，禁用 OpenSSL 依赖。
- **rusqlite** (`0.32`, bundled) — SQLite 状态持久化。
- **axum** (`0.8`) — `mimofan-app-server` 的 HTTP 框架。
- **tower-http** — CORS 等中间件。
- **clap** (`4.5`) — CLI 参数解析 + `clap_complete` 生成补全脚本。

### 2.2 数据 / 序列化
- **serde / serde_json** — 配置、协议 DTO 的序列化基石。
- **toml / toml_edit** — 配置文件读写，`toml_edit` 用于就地编辑保留注释。
- **chrono / uuid / semver** — 时间戳、会话 ID、版本比较。
- **sha2** — 完整性校验（如 npm 包 SHA-256）。

### 2.3 可观测性 / 错误
- **tracing / tracing-subscriber / tracing-appender** — 结构化日志。
- **anyhow / thiserror** — 错误处理，核心库用 `thiserror`、边界处用 `anyhow`。

### 2.4 安全 / 隔离
- **rustls** — TLS 终结。
- **Landlock（Linux）/ Bubblewrap / Seatbelt（macOS）/ Seccomp / Job Object（Windows）** — sandbox 后端，`crates/tui/src/sandbox/` 集中实现。
- **OpenSandbox 协议**（HTTP）— 可选的远程 sandbox 后端。

### 2.5 用户态 / TUI
- **ratatui / crossterm** — TUI 渲染（实际依赖在 `crates/tui/Cargo.toml`）。
- **dotenvy** — 启动时加载 `.env`。
- **tempfile / wait-timeout** — 子进程管理。

### 2.6 LLM 适配
- 没有依赖 `openai-rs`、`anthropic-rs` 这类官方 SDK —— 因为 mimofan **自己实现 wire format 适配**（`crates/tui/src/client/`）。这一点是设计取舍：减少三方依赖、避免被上游绑死。

---

## 3. 核心能力常用方法入口

> 以下入口都在用户接口层（CLI/TUI/HTTP）背后，对应"想加个能力从哪儿下手"。

| 能力 | 代码入口 | 怎么扩展 |
|------|---------|---------|
| 加一种内置工具 | `crates/tools/src/lib.rs` 注册新 `Tool` | 实现 `Tool` trait，调用 `ToolRegistry::register()` |
| 桥接一个 MCP server | `crates/mcp/src/lib.rs` 已有 `McpManager` | 配置 `~/.mimofan/mcp.json`，stdio/HTTP transport 已实现 |
| 加一个生命周期钩子 | `crates/hooks/src/lib.rs` 的 `HookDispatcher` | 实现 `Hook` trait，注册到 dispatcher |
| 修改执行策略（如批准/拒绝规则） | `crates/execpolicy/src/lib.rs` | 实现 `ExecPolicyEngine` 的判定规则 |
| 加一种 sandbox 后端 | `crates/tui/src/sandbox/` | 实现 `SandboxBackend` trait，登记到 `BACKENDS` |
| 修改 CLI 子命令 | `crates/cli/src/lib.rs` 的 `Cli` 结构体 | 用 clap 的 `#[command(subcommand)]` 加新变体 |
| 修改 TUI 主题/快捷键 | `crates/tui/src/commands/` 和 `crates/tui/src/tui/ui.rs` | 通过 slash command 体系注册 |
| 加 TUI 内嵌命令（如 `/foo`） | `crates/tui/src/commands/groups/<group>/` | 注册新的 CommandGroup |

---

## 4. 提示词工程（Prompt Engineering）

### 4.1 提示词文件位置
所有发给 LLM 的 prompt 模板统一在 **`crates/tui/src/prompts/`**，编译期通过 `include_str!` 内嵌。修改 `.md` 后必须重新编译。

### 4.2 分层宪法（Tier 1-9，按优先级降序）

| Tier | 名称 | 文件 | 说明 |
|------|------|------|------|
| 1 | Constitution | `prompts/constitution.md` | 身份、行为准则、不可违反的硬约束 |
| 2 | Statutes | `prompts/approvals/*.md` | 权限 / 审批相关条款 |
| 3 | Regulations | `prompts/modes/*.md` | 模式（Plan / Agent / YOLO）规则 |
| 4 | Project Law | `.mimofan/constitution.json`（项目级） | 用户在项目里追加的硬约束 |
| 5 | Memory | `prompts/memory_guidance.md` | 长期记忆读取指引 |
| 6 | Live Evidence | 工具实时返回 | 当前对话上下文 |
| 7 | Handoffs | `prompts/compact.md` | 上下文压缩时使用的提示 |
| 8 | Personality | `prompts/personalities/*.md` | 角色语气 |
| 9 | Continuation | `prompts/continuation.md` | 长任务的续行衔接 |

### 4.3 特殊提示词文件

| 文件 | 用途 |
|------|------|
| `constitution.md` | 系统身份 + 不可覆盖的元规则 |
| `coding_assistant.md` | 编码场景下的默认角色 |
| `acp_coding_assistant.md` | ACP（外部客户端）场景下的角色 |
| `compaction_specialist.md` / `summarization_specialist.md` | 上下文压缩 |
| `synthesis_assistant.md` | 大工具输出的二次摘要（workshop） |
| `inventory_router_classifier.md` / `router_classifier.md` | 子 agent 路由 |
| `subagent_output_format.md` / `subagent_self_report_note.md` | 子 agent 输出格式 |
| `locale_preamble_zh_hans.md` / `locale_closer_zh_hans.md` | 中文语言环境的开场 / 收尾提示 |
| `memory_guidance.md` | 长期记忆读取规则 |
| `purge_instructions.md` | 对话清空后的恢复指令 |
| `v4_model_characteristics.md` / `generic_model_characteristics.md` | 针对不同模型家族的微调 |
| `voice_transcription.md` | 语音转写场景 |
| `translator.md` / `code_reviewer.md` | 专用角色 |
| `authority_recap.md` | 在系统 prompt 末尾追加的权限摘要，让模型"时刻记得" |

### 4.4 改提示词的注意点
1. **不要在 `constitution.md` 加任务级规则** —— 一旦硬编码，升级时难迁移。
2. **修改前先跑一遍** `cargo test -p mimofan-tui`：多数提示词有对应的 fixture 测试。
3. **分层宪法不可被项目级覆盖**（Tier 1-3 永远在 Tier 4 之上）。

### 4.5 本地化（i18n）
- UI 字符串走 `crates/tui/locales/zh-Hans.json`（仅中文一档），运行时按 `LC_ALL`/`LC_MESSAGES`/`LANG` 选取。
- **它只影响 TUI 标签文本，不影响模型输出语言**。要让模型用中文输出，请在用户指令中明确，或使用 `locale_preamble_zh_hans.md` 触发的提示词路径。

---

## 5. 常用函数和使用用例

### 5.1 编程式启动 mimofan（embed）

```rust
use mimofan_cli::run_cli;

fn main() -> std::process::ExitCode {
    mimofan_cli::run_cli()
}
```

### 5.2 直接构造 Runtime（嵌入到另一个 Rust 程序）

```rust
use mimofan_core::Runtime;
use mimofan_agent::ModelRegistry;
use mimofan_config::{ConfigToml, load_config};
use mimofan_execpolicy::ExecPolicyEngine;
use mimofan_hooks::HookDispatcher;
use mimofan_mcp::McpManager;
use mimofan_state::StateStore;
use mimofan_tools::ToolRegistry;
use std::sync::Arc;

let config: ConfigToml = load_config(None)?;
let registry = ModelRegistry::default();
let state = StateStore::open("~/.mimofan/state.db")?;
let tools = Arc::new(ToolRegistry::with_builtins());
let mcp = Arc::new(McpManager::start(&config).await?);
let exec = ExecPolicyEngine::from_config(&config);
let hooks = HookDispatcher::new();

let runtime = Runtime::new(config, registry, state, tools, mcp, exec, hooks);
// ... 调用 runtime.thread_manager / runtime.jobs 等
```

### 5.3 启动一个 Turn（发一条用户消息）

```rust
use mimofan_protocol::{PromptRequest, UserInputRequestEvent};

let req = PromptRequest {
    thread_id: thread.id.clone(),
    text: "帮我把 src/foo.rs 重构一下".into(),
    images: vec![],
    // ...
};
let event: UserInputRequestEvent = req.into();
// 交给 turn loop / app-server 路由
```

### 5.4 注册自定义工具

```rust
use mimofan_tools::{Tool, ToolRegistry, ToolCall, ToolResult};
use async_trait::async_trait;

struct MyTool;

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "我的自定义工具" }
    async fn invoke(&self, call: ToolCall) -> anyhow::Result<ToolResult> { /* ... */ }
}

let mut reg = ToolRegistry::with_builtins();
reg.register(Box::new(MyTool));
```

### 5.5 MCP server（让别的程序调用 mimofan 的工具）

```bash
mimofan mcp serve     # stdio 模式（推荐）
mimofan mcp serve --http  # HTTP 模式
```

对方用 stdio JSON-RPC 或 HTTP JSON-RPC 连入即可（详见 `docs/MCP.md`）。

### 5.6 启动 app-server（HTTP API）

```bash
mimofan app-server --bind 127.0.0.1:8787
# 或 stdio JSON-RPC
mimofan app-server --stdio
```

---

## 6. DDD 视角：架构精妙之处

1. **Provider 二元化是最大的架构胜利**
   - 把历史上 25+ 个 provider 收敛成 `XiaomiMimo` + `Custom` 两个值，所有 fallback / 默认 URL / 默认模型 / 环境变量候选都通过 `provider!` 宏静态展开。
   - 结果：新增一个 LLM 服务商只需要在配置里加 `[providers.foo] kind="openai-compatible"`，**代码零改动**。
   - 这是 Eric Evans 反复强调的"用限界上下文收口业务复杂度"的典型落地。

2. **`mimofan-protocol` 是纯 DTO 包**（17 行 `lib.rs`）
   - 只放 `EventFrame` / `PromptRequest` / `AppResponse` 等结构体，不带业务逻辑。
   - 客户端和服务端可以独立演进协议字段，互不污染领域模型。

3. **零大小类型 + 静态注册表**
   - `XiaomiMimo` / `Custom` 都是零大小 struct（ZST），`PROVIDER_REGISTRY` 是 `&'static [&'static dyn Provider; 2]`。
   - 编译期决定所有元数据，运行时无 hash 查找 / 无堆分配。

4. **聚合根边界清晰**
   - `Runtime` 是一个明确的"组合根（Composition Root）"，把 config + registry + thread + tools + mcp + exec + hooks 装配在一起。
   - 任何"启动 mimofan"的入口（CLI / TUI / app-server）都先组装 Runtime，再分发给具体的子系统。

5. **Persistence 与领域解耦**
   - `mimofan-state` 用 SQLite，CRUD 接口对 `ThreadManager` 透明。将来想换存储后端（如 sled、PostgreSQL），改这一个 crate 即可。

---

## 7. 架构边界存在的真问题

> 不堆砌凑数项；以下都是基于代码事实梳理的、确实存在的问题。

### 7.1 `mimofan-tui` 不是真正的库

`crates/tui/src/lib.rs` 是空文件（1 行），所有 6896 行业务代码堆在 `src/main.rs`。后果：

- **TUI 内部模块无法被其他入口复用**（CLI、app-server 想借用 UI 组件就动不了）。
- **`cargo doc` 失效**，IDE 跳转经常失效。
- **增量编译劣化**：任何一个小改动都触发整个 TUI 重新链接。
- **测试只能在 binary crate 里写**，集成测试覆盖受限。

> 注意：这是设计选择，不是 bug。把 TUI 当作"一个有几千个内部模块的二进制"也可以工作，只是损失了"lib 可复用"的灵活性。

### 7.2 `Runtime` 是"上帝聚合根"

`crates/core/src/lib.rs` 的 `Runtime` 结构体直接持有 8 个组件（config / registry / thread / tools / mcp / exec / hooks / jobs），调用方都依赖 `Runtime`。后果：

- 测试很难只测一个组件（必须先造一个 Runtime）。
- 任何组件签名变更都要回头改 Runtime。

更 DDD 的做法是：把 `Runtime` 拆成 `RuntimeServices`（应用服务集合）+ `RuntimeContext`（不可变快照）。`Runtime` 只保留最小装配入口。

### 7.3 `provider_resolver` 与 `ModelRegistry` 重复

- `crates/agent/src/provider_resolver.rs` 负责"用户写的模型名 → 真实模型 ID"的解析（含 fallback 链）。
- `crates/agent/src/lib.rs` 的 `ModelRegistry` 也做类似的事。

两套语义不同（一个按 provider 路由、一个按 model 解析），但都给"模型名解析"留了接口，未来容易互相踩。

### 7.4 TUI 内部目录过深 / 职责混合

`crates/tui/src/` 顶层有 90+ 个文件 + 12+ 个子目录（commands/、tools/、prompts/、lsp/、sandbox/、fleet/、runtime_api/、runtime_threads/、state_machine/、skill_state/、snapshots/、vision/…），UI 渲染、repl 状态机、运行时事件循环、提示词拼装、sandbox backend、fleet 调度全在一个二进制 crate 里混着。

新人想"加一个 slash 命令"得先搞清楚 7 个目录才能动手。



`prompts/*.md` 是发给 LLM 的内容（中文 prompt），`locales/zh-Hans.json` 是 TUI UI 字符串。`docs/PROMPTS.md` 同时提到两者会让新人误解为「提示词就是本地化」。实际上两者机制完全不同。

---

## 8. 优化计划（只动底层，不影响用户接口）

> 标 `[x]` 的项目已经在当前代码中成立；标 `[ ]` 的项目是建议改进，按优先级和代价排序。

### 8.1 高优先级（低成本高收益）

- [x] **完成 Provider 二元化收敛**（`ProviderKind` 只剩 `XiaomiMimo` + `Custom`）—— 已在 `crates/config/src/provider_kind.rs` 落地。
- [x] **本地化只保留中文一档**（`locales/zh-Hans.json`）—— 已实现。
- [x] **TUI 提示词分层宪法落地（Tier 1-9）** —— 已在 `crates/tui/src/prompts/` 落地，`docs/PROMPTS.md` 维护索引。
- [x] **聚合根 `Runtime` 明确化** —— 已在 `crates/core/src/lib.rs` 落地。
- [x] **删除根目录过时 md 文档**（`MIMOFAN_GUIDE_CN.md`、`report.md`、旧的 `docs/ARCHITECTURE_CN.md` / `docs/USAGE_CN.md` / `docs/STABILITY_ANALYSIS_CN.md` / `docs/DEAD_CODE_*.md` / `docs/PROVIDERS.md`）。
- [x] **统一中文架构与使用文档到根目录**（`ARCHITECTURE.md`、`USER_GUIDE.md`），其余子文档保持现状或随 ARCHITECTURE.md 同步。

### 8.2 中优先级（需要评估再动手）

- [ ] **把 `mimofan-tui` 拆成 lib + bin**：把 `src/main.rs` 的内容下沉到 `src/lib.rs`，按职责拆成 `app/`、`repl/`、`transport/`、`prompts/` 等模块。预期收益：增量编译变快、可被 app-server 复用部分 UI 组件、`cargo doc` 恢复。代价：较大的重构工作量，且要保证二进制入口不变（CLI 参数完全兼容）。
- [ ] **合并 `provider_resolver` 与 `ModelRegistry`**：把"模型名解析"集中到一个服务里，提供统一的 fallback 链语义。预期收益：消除双轨语义。代价：可能影响现有 fallback 行为，需要全量回归。
- [ ] **把 `Runtime` 拆为 `RuntimeServices` + `RuntimeContext`**：纯应用服务集合 + 不可变快照。预期收益：测试粒度更细、组合更灵活。代价：破坏 `pub use Runtime` 的现有导入路径，需要在迁移期间保留 re-export。

### 8.3 低优先级（暂不动）

- [ ] **TUI 内部目录重组**：按 DDD 限界上下文重新切分（`ui/` / `application/` / `infrastructure/`）。预期收益：新人上手快。代价：跨 90+ 文件的大迁移，需要冻结一段时间不接受新功能。
- [ ] **替换 `mimofan-protocol` 为 trait-based IPC**：放弃 JSON DTO，改成 trait-based 强类型消息。预期收益：编译期检查 + 零序列化开销。代价：JSON 兼容的客户端（如 Node.js 集成）会失效。

### 8.4 不需要做的事（避免过度承诺）

- **不要**为了"统一风格"把 `mimofan-state` 抽象成 trait 化的 storage —— 当前 SQLite 单后端够用，抽象只会带来间接成本。
- **不要**新增 provider enum 变体。当前 `XiaomiMimo` + `Custom` 已经覆盖所有需求，再扩字段只会让收敛功亏一篑。
- **不要**把 LLM 客户端拆成独立 crate（`mimofan-llm`）。它在 TUI 内部存在是因为它高度耦合提示词拼装，独立出来会让循环依赖更复杂。

---

## 9. 扩展指南速查

| 你想做 | 看哪里 | 预计改动量 |
|------|------|---------|
| 让 mimofan 支持一个新的 LLM 服务商 | 在 `config.toml` 加 `[providers.<name>] kind="openai-compatible"`，填 base_url / model / api_key_env | 零代码 |
| 修改系统人格 / 编码风格 | `crates/tui/src/prompts/constitution.md` + `personalities/*.md` | 1-2 个 md 文件 |
| 增加一个 slash 命令 | `crates/tui/src/commands/groups/` 注册新 `CommandGroup` | ~100 行 Rust |
| 增加一个内置工具 | `crates/tools/src/lib.rs` 实现 `Tool` trait | ~150 行 Rust |
| 加 MCP 桥接 | `crates/mcp/src/lib.rs` 已支持 stdio + HTTP，配置 `~/.mimofan/mcp.json` 即可 | 零代码 |
| 加 IM 桥（飞书 / 微信 / Discord） | `integrations/<bridge-name>/`，参考 `integrations/feishu-bridge/` | ~300-500 行 Rust |
| 自定义 TUI 主题 | `crates/tui/src/deepseek_theme.rs` + `assets/` | ~50 行 + 配色文件 |
| 改审批策略 | `crates/execpolicy/src/lib.rs` 的 `ExecPolicyEngine` | ~100 行 Rust |

---

## 10. 数据流概览

```
用户输入
  │
  ▼
┌─────┐    ┌─────┐    ┌─────┐
│ CLI │───▶│ TUI │───▶│ API │  (三种入口)
└─────┘    └─────┘    └─────┘
  │          │          │
  └──────────┼──────────┘
             ▼
        ┌─────────┐
        │  core   │  核心引擎
        │ engine  │  对话轮次循环
        └────┬────┘
             │
     ┌───────┼───────┐
     ▼       ▼       ▼
┌────────┐┌─────┐┌───────┐
│  LLM   ││tools││agent  │  工具执行 / 子智能体
│provider││     ││manager│
└────────┘└─────┘└───────┘
     │       │       │
     ▼       ▼       ▼
┌────────┐┌─────┐┌───────┐
│config  ││exec-││ state │  配置 / 安全 / 持久化
│provider││pol. ││  db   │
└────────┘└─────┘└───────┘
```

---

## 11. 技术栈总结

| 层级 | 技术 | 用途 |
|------|------|------|
| CLI | `clap` | 命令行参数解析 |
| HTTP | `axum` | REST API 服务器 |
| TUI | `ratatui` + `crossterm` | 终端界面渲染 |
| 异步运行时 | `tokio` | 全异步 I/O |
| 序列化 | `serde` + `serde_json` + `toml` | 数据序列化 |
| HTTP 客户端 | `reqwest` (rustls) | LLM API 调用 |
| 数据库 | `rusqlite` (bundled) | SQLite 状态持久化 |
| 错误处理 | `thiserror` + `anyhow` | 类型化错误 |
| 日志 | `tracing` | 结构化日志 |

---

## 12. 快速定位指南

| 我想了解... | 去看... |
|------------|---------|
| CLI 命令怎么解析的 | `cli/src/main.rs`、`cli/src/args.rs` |
| TUI 界面怎么渲染的 | `tui/src/ui/`、`tui/src/widgets/` |
| 对话轮次怎么循环的 | `core/src/engine.rs` |
| LLM 提供商怎么配置的 | `config/src/provider/` |
| 工具怎么执行的 | `tools/src/`、`protocol/src/tool.rs` |
| 子智能体怎么管理的 | `agent/src/`、`tui/src/fleet/` |
| 安全策略怎么控制的 | `execpolicy/src/` |
| 密钥怎么存储的 | `secrets/src/` |
| 会话状态怎么持久化的 | `state/src/` |
| MCP 工具怎么集成的 | `mcp/src/` |
| 国际化字符串在哪 | `tui/src/localization.rs` |
| 系统提示词怎么构建的 | `tui/src/prompts.rs` |

---

## 13. 关键设计约束

1. **仅支持 agent 工具**：子智能体工具只有 `agent`，不存在 `agent_open`/`agent_eval` 等变体
2. **无生命周期/一致性系统**：不引入 capacity/coherence/runtime-tag 系统
3. **无运行时提示注入**：`constitution.json` 是唯一的 base prompt
4. **子智能体深度可配置**：无硬编码嵌套限制
5. **执行策略强制**：所有 Shell 命令必须经过 `execpolicy` 沙箱

---

## 14. 进一步阅读

- `USER_GUIDE.md` — 终端用户使用手册
- `docs/INSTALL.md` — 安装方式
- `docs/CONFIGURATION.md` — 配置文件字段参考
- `docs/PROMPTS.md` — 提示词分层与索引
- `docs/MODES.md` — TUI 模式与审批
- `docs/MCP.md` — MCP 桥接
- `docs/SUBAGENTS.md` — 子 agent 用法
- `docs/KEYBINDINGS.md` — 快捷键
- `docs/DOCKER.md` — Docker 镜像