# mimofan 架构规划与改进方案

> 本文档从第一性原理出发，以 DDD 理论分析当前系统架构，制定存量优化计划。
> 改造原则：**只影响底层实现，不影响用户交互方式**。

---

## 1. 系统定位（第一性原理）

**mimofan 是什么？**

mimofan 是一个**跑在本地的 AI 编码助手**。用户用自然语言下指令，它调用大模型思考，再用工具（读文件、改代码、跑命令）把活干完。整个工作流是**"模型决策 → 工具执行 → 结果回灌 → 再决策"**的闭环。

**核心价值：**
- 本地优先，代码不上传
- Rust 实现，性能优异
- MIT 协议，完全开源
- 默认内置 Xiaomi MiMo，支持所有 OpenAI 兼容协议

---

## 2. DDD 架构分析

### 2.1 限界上下文划分

| 上下文 | 解决的问题 | 核心 crate | 关键类型 |
|--------|-----------|------------|----------|
| **配置上下文** | 加载配置、解析 profile、决定 provider | `mimofan-config` | `ConfigToml`, `ProviderKind` |
| **模型网关上下文** | 模型名解析、fallback 链 | `mimofan-agent` | `ModelRegistry`, `ProviderResolver` |
| **对话上下文** | 会话生命周期、消息持久化 | `mimofan-core` + `mimofan-state` | `Runtime`, `Thread`, `Message` |
| **工具执行上下文** | 工具注册、MCP 桥接、执行策略 | `mimofan-tools` + `mimofan-mcp` + `mimofan-execpolicy` | `ToolRegistry`, `ExecPolicyEngine` |
| **密钥上下文** | API key 存储、密钥管理 | `mimofan-secrets` | `Secrets` |
| **协议上下文** | 客户端↔服务端 JSON DTO | `mimofan-protocol` | `EventFrame`, `PromptRequest` |
| **接口适配上下文** | TUI / CLI / HTTP 不同呈现 | `mimofan-tui` + `mimofan-cli` + `mimofan-app-server` | — |

### 2.2 依赖调用链

```
┌──────────────────────────────────────────────────────────────┐
│                     接口适配层（三端入口）                      │
│              TUI 终端  /  CLI 命令行  /  HTTP 服务              │
└────────────────────────────┬─────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────┐
│                      核心引擎域                                │
│           Runtime / Turn Loop / ThreadManager                │
└────────────────────────────┬─────────────────────────────────┘
                             │
      ┌───────────┬───────────┼───────────┬───────────┬─────────┐
      ▼           ▼           ▼           ▼           ▼         ▼
┌─────────┐ ┌────────┐ ┌────────┐ ┌──────────┐ ┌────────┐ ┌────────┐
│ config  │ │ agent  │ │ tools  │ │   mcp    │ │ hooks  │ │ state  │
│ 配置路由  │ │模型注册 │ │工具集   │ │外部协议   │ │生命周期  │ │持久化   │
└─────────┘ └────────┘ └────────┘ └──────────┘ └────────┘ └────────┘
      │           │           │           │           │         │
      └───────────┴───────────┴───────────┴───────────┴─────────┘
                             │
                             ▼
                     ┌─────────────────┐
                     │  protocol (DTO) │
                     │  secrets        │
                     └─────────────────┘
```

---

## 3. 架构精妙之处（已实现的最佳实践）

### 3.1 Provider 二元化 —— 最大的架构胜利

把历史上 25+ 个 provider 收敛成 `XiaomiMimo` + `Custom` 两个值。所有 fallback / 默认 URL / 默认模型都通过 `provider!` 宏静态展开。

**效果：新增一个 LLM 服务商只需要在配置里加一行，代码零改动。**

这是 Eric Evans 强调的"用限界上下文收口业务复杂度"的典型落地。

### 3.2 纯 DTO 包 —— mimofan-protocol

`mimofan-protocol` 只有 349 字节的 `lib.rs`，只放 `EventFrame` / `PromptRequest` / `AppResponse` 等结构体，不带任何业务逻辑。

**效果：客户端和服务端可以独立演进，互不污染。**

### 3.3 零大小类型 + 静态注册表

`XiaomiMimo` / `Custom` 都是零大小 struct（ZST），`PROVIDER_REGISTRY` 是 `&'static [&'static dyn Provider; 2]`。

**效果：编译期决定所有元数据，运行时无堆分配、无 hash 查找。**

### 3.4 聚合根边界清晰

`Runtime` 是明确的"组合根"，把 config + registry + thread + tools + mcp + exec + hooks 装配在一起。任何入口（CLI / TUI / app-server）都先组装 Runtime，再分发给子系统。

### 3.5 本地化单一化

UI 字符串只保留 `locales/zh-Hans.json`（中文一档），不影响模型输出语言。

---

## 4. 架构边界存在的问题

> 以下问题基于代码事实梳理，不堆砌凑数项。

### 4.1 mimofan-tui 不是真正的库

`crates/tui/src/lib.rs` 是空文件，所有代码堆在 `src/main.rs`（249KB）。

**问题：**
- TUI 内部模块无法被其他入口复用
- `cargo doc` 失效，IDE 跳转失效
- 增量编译劣化，任何小改动都触发整个 TUI 重新链接
- 测试只能在 binary crate 里写

**影响范围：** 仅影响开发者，不影响用户交互。

### 4.2 Runtime 是"上帝聚合根"

`Runtime` 直接持有 8 个组件，调用方都依赖 `Runtime`。

**问题：**
- 测试很难只测一个组件（必须先造一个 Runtime）
- 任何组件签名变更都要回头改 Runtime

**影响范围：** 仅影响开发者测试体验，不影响运行时行为。

### 4.3 provider_resolver 与 ModelRegistry 重复

两套语义不同的"模型名解析"接口，存在未来互相踩的风险。

**影响范围：** 仅影响开发者理解代码，不影响用户功能。

### 4.4 TUI 内部目录过深

`crates/tui/src/` 顶层有 90+ 个文件 + 12+ 个子目录，UI 渲染、repl 状态机、提示词拼装、sandbox backend 全混在一起。

**影响范围：** 仅影响开发者上手速度，不影响运行时。

---

## 5. 改进计划

> 标 `[x]` 的项目已完成；标 `[ ]` 的项目是建议改进。

### 5.1 高优先级（低成本高收益，已完成）

- [x] **Provider 二元化收敛**（`ProviderKind` 只剩 `XiaomiMimo` + `Custom`）
- [x] **本地化只保留中文一档**（`locales/zh-Hans.json`）
- [x] **TUI 提示词分层宪法落地**（Tier 1-9）
- [x] **聚合根 `Runtime` 明确化**
- [x] **统一中文架构与使用文档到根目录**（`ARCHITECTURE.md`、`USER_GUIDE.md`）

### 5.2 中优先级（需要评估再动手）

- [ ] **把 `mimofan-tui` 拆成 lib + bin**
  - 把 `src/main.rs` 的内容下沉到 `src/lib.rs`，按职责拆成 `app/`、`repl/`、`transport/` 等模块
  - 预期收益：增量编译变快、可被 app-server 复用部分 UI 组件
  - 前提：保证二进制入口不变（CLI 参数完全兼容）

- [ ] **合并 `provider_resolver` 与 `ModelRegistry`**
  - 把"模型名解析"集中到一个服务，提供统一的 fallback 链语义
  - 预期收益：消除双轨语义
  - 前提：全量回归测试

- [ ] **把 `Runtime` 拆为 `RuntimeServices` + `RuntimeContext`**
  - 纯应用服务集合 + 不可变快照
  - 预期收益：测试粒度更细
  - 前提：保留 re-export 兼容现有导入路径

### 5.3 低优先级（暂不动）

- [ ] **TUI 内部目录重组**：按 DDD 限界上下文重新切分（`ui/` / `application/` / `infrastructure/`）
- [ ] **替换 `mimofan-protocol` 为 trait-based IPC**：放弃 JSON DTO，改成 trait-based 强类型消息

### 5.4 明确不需要做的事

- **不要**把 `mimofan-state` 抽象成 trait 化存储 —— 当前 SQLite 单后端够用，抽象只会带来间接成本
- **不要**新增 provider enum 变体 —— 当前 `XiaomiMimo` + `Custom` 已覆盖所有需求
- **不要**把 LLM 客户端拆成独立 crate —— 它高度耦合提示词拼装，独立出来会让循环依赖更复杂

---

## 6. 文件清理清单

### 6.1 已删除的无用文件

- `docs/CLAUDE.local.md` — 局部开发笔记
- `crates/tui/locales/CLAUDE.local.md` — 局部开发笔记
- 所有 `**/CLAUDE.local.md` — 散落在各 crate 的局部开发笔记
- `.aiderignore` / `.codeiumignore` / `.cursorignore` — 与 .claudeignore 重复，已合并

### 6.2 保留的文件

| 文件 | 用途 |
|------|------|
| `ARCHITECTURE.md` | 架构说明文档（中文） |
| `USER_GUIDE.md` | 用户使用指南（中文） |
| `CLAUDE.md` | AI 开发者工具指南（英文，机器用） |
| `AGENTS.md` | 仓库 AI 代理指南（英文，机器用） |
| `README.md` | 项目简介（中文） |

---

## 7. 快速扩展指南

| 你想做 | 看哪里 | 改动量 |
|--------|--------|--------|
| 支持新的 LLM 服务商 | `config.toml` 加 `[providers.<name>]` | 零代码 |
| 修改系统人格/编码风格 | `crates/tui/src/prompts/constitution.md` | 1-2 个 md |
| 增加一个 slash 命令 | `crates/tui/src/commands/groups/` | ~100 行 |
| 增加一个内置工具 | `crates/tools/src/lib.rs` | ~150 行 |
| 桥接 MCP | `crates/mcp/src/lib.rs` + `~/.mimofan/mcp.json` | 零代码 |
| 加 IM 桥（飞书/微信） | `integrations/<bridge-name>/` | ~300-500 行 |
| 自定义 TUI 主题 | `crates/tui/src/` 主题相关 | ~50 行 |
| 改审批策略 | `crates/execpolicy/src/lib.rs` | ~100 行 |
