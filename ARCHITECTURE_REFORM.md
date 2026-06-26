# CodeWhale 架构改进计划

> 基于 DDD（领域驱动设计）理论，从第一性原理出发的架构分析与改进方案。
> 改造范围：仅影响底层实现，不改变用户可见的交互方式和外部 API 接口。

---

## 一、系统本质分析

CodeWhale 是一个 **AI 编码代理平台**，核心价值链：

```
用户输入 → LLM 推理 → 工具调用 → 结果反馈 → 循环
```

用一句话概括：**把大模型的"想法"转化为对代码仓库的实际操作，并在安全可控的前提下自动化编码工作流。**

### 核心领域能力

| 能力 | 说明 |
|------|------|
| 多模型接入 | DeepSeek、OpenAI、Anthropic、小米 MiMo 等 20+ 模型提供商 |
| 工具执行 | Shell、文件读写、Git、搜索、代码执行等内置工具 |
| 上下文管理 | 会话持久化、上下文压缩、崩溃恢复、离线队列 |
| 安全策略 | 沙箱隔离、执行审批、权限控制 |
| 扩展机制 | MCP 协议、Skills 技能系统、Hooks 钩子 |
| 协作模式 | 子代理派生、RLM 持久会话、后台任务队列 |

---

## 二、现有架构精妙之处

### 1. 零依赖叶子 Crate 设计

```
Layer 0（无内部依赖）:
  protocol  mcp  secrets  state  release  whaleflow
```

6 个叶子 crate 完全独立，可单独编译、测试、复用。这是 Rust workspace 的最佳实践。

### 2. 统一工具抽象（ToolHandler Trait）

```rust
// crates/tools/src/lib.rs
#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn kind(&self) -> ToolKind;
    fn is_mutating(&self) -> bool { false }
    async fn handle(&self, invocation: ToolInvocation)
        -> Result<ToolOutput, FunctionCallError>;
}
```

所有工具（内置、MCP、Shell）通过统一 trait 注册到 `ToolRegistry`，调度层无需关心具体实现。这是一个干净的**策略模式**应用。

### 3. 分层审批策略

```
ExecPolicyEngine → AskForApproval → ReviewDecision
     (规则引擎)       (策略枚举)       (用户决策)
```

从"自动放行"到"每次审批"，再到"会话级记住"，策略粒度清晰。

### 4. 宪法式提示词体系

```
constitution.md (Tier 1 - 宪法)
  ↓
statutes (Tier 2 - 法规)
  ↓
regulations (Tier 3 - 规章)
  ↓
evidence (Tier 6 - 证据/工具手册)
```

优先级从高到低，冲突时高层级覆盖低层级。这是**分层治理**的经典模式。

### 5. 流式优先架构

所有 LLM 响应通过 `EventFrame` 流式推送，支持增量渲染、实时反馈。`ResponseDelta` 事件让 TUI 可以逐字显示。

---

## 三、架构边界问题诊断

### 问题 1：`tui` 是一个隐性上帝模块

```
codewhale-tui 依赖: config, execpolicy, protocol, release, secrets, tools
codewhale-tui 不依赖: core, agent, hooks, mcp, state
```

`tui` 没有使用 `core` crate，而是自己构建了一套完整的运行时集成路径。这意味着：
- `core` 中的线程管理、会话管理、工具编排逻辑在 TUI 模式下**被绕过**
- `tui` 内部存在与 `core` **平行的重复实现**
- 两个入口（CLI 和 TUI）走的是不同的代码路径

**DDD 诊断**：领域逻辑泄漏到了 UI 层，违反了"UI 层只做展示和交互"的原则。

### 问题 2：`core` 职责过重

`core` 依赖 8 个 crate（agent, config, execpolicy, hooks, mcp, protocol, state, tools），承担了：
- 会话管理（NewThread, InitialHistory）
- 线程管理（ThreadRequest/Response 处理）
- 工具编排（ToolRegistry 交互）
- MCP 启动管理
- 钩子调度
- 后台任务（JobStatus, JobRetryMetadata）
- 目标管理（ThreadGoal）

**DDD 诊断**：这是一个**聚合根过多**的"上帝聚合"，违反了单一职责原则。

### 问题 3：`agent` crate 过于单薄

`agent` 只包含 `ModelRegistry`（模型注册表和解析逻辑），仅依赖 `config`。

**DDD 诊断**：本应是核心领域（Agent 聚合根）的 crate，退化成了一个数据查找表。LLM 客户端抽象、推理调度、上下文组装等 Agent 核心能力没有被抽取到这里。

### 问题 4：`protocol` 类型膨胀

`protocol/src/lib.rs` 超过 700 行，混杂了：
- 线程协议类型（Thread, ThreadRequest, ThreadResponse）
- 事件帧类型（EventFrame 及其 20+ 变体）
- 审批协议类型（ExecApprovalRequestEvent, ReviewDecision）
- 工具协议类型（ToolPayload, ToolOutput）
- 用户交互类型（UserInputRequestEvent, UserInputAnswerEvent）

**DDD 诊断**：共享内核（Shared Kernel）过大，应按限界上下文拆分。

### 问题 5：提示词系统分散

提示词分布在两个位置：
- `crates/tui/src/prompts/` — TUI 专属提示词（constitution.md, compact.md 等）
- `crates/core/src/` — core 内部的提示词逻辑（prompts.rs 引用）

**DDD 诊断**：提示词是"策略"层资源，不应绑定在 UI 或引擎中。

### 问题 6：跨层直接依赖

```
config → execpolicy, secrets（配置层依赖策略层和安全层）
tui → release（UI 层依赖发布工具）
cli → release（CLI 层依赖发布工具）
```

配置层不应直接依赖执行策略层；发布工具不应被 UI 层直接依赖。

### 问题 7：`whaleflow` 是完全孤立的 crate

`whaleflow` 无任何内部依赖，也不被任何其他 crate 依赖。它是一个 Starlark 工作流引擎，但与主系统完全脱节。

**DDD 诊断**：这是一个**未集成的限界上下文**，要么完成集成，要么移除。

### 问题 8：`agent` crate 命名与实际职责不符

`agent` crate 实际只包含 `ModelRegistry`（模型注册表和解析逻辑），名字暗示它应该是 Agent 系统的核心，但实际上只是一个数据查找表。

**DDD 诊断**：聚合根命名误导，应重命名为 `model_registry` 或扩展其职责。

### 问题 9：遗留文件未清理

`crates/tui/src/prompts/agent.txt` 是旧版遗留 prompt，已被 `constitution.md + overlays` 替代，但仍保留在代码库中。

**DDD 诊断**：技术债务，应清理。

---

## 四、改进方案（仅影响底层，不改变用户交互）

### 原则

1. **所有改动对用户透明** — CLI 命令、TUI 交互、HTTP API、配置文件格式不变
2. **渐进式重构** — 每个阶段可独立编译、测试、合并
3. **向后兼容** — 保留旧的 pub 接口，内部实现迁移

---

### 阶段 1：拆分 `protocol` 类型

**目标**：将 `protocol` 按限界上下文拆分为子模块，不改变外部 API。

**当前状态**：
- [x] `protocol/src/lib.rs` 包含所有协议类型（700+ 行）

**改进项**：
- [ ] 在 `protocol/src/` 下创建子模块：`thread.rs`, `event.rs`, `approval.rs`, `tool.rs`, `user_input.rs`
- [ ] 将对应类型从 `lib.rs` 移入子模块，`lib.rs` 做 `pub use` 重导出
- [ ] 保持所有外部 `use codewhale_protocol::*` 路径不变

**验证**：`cargo build --workspace` 通过，无新增编译警告。

---

### 阶段 2：抽取 `agent` 领域能力

**目标**：将 `agent` 从"模型查找表"升级为"Agent 领域核心"。

**当前状态**：
- [x] `agent` 仅包含 `ModelRegistry` 和 `ModelInfo`

**改进项**：
- [x] 将 `core` 中的 LLM 客户端 trait 抽取到 `agent` crate（如有 `LlmClient` trait）
- [x] 将模型解析、provider 路由逻辑保留在 `agent`
- [x] 将 `ModelFamily` 枚举及分类逻辑从 `agent/src/lib.rs` 移入独立的 `agent/src/family.rs`
- [x] 将 provider-specific 的 passthrough 逻辑（atlascloud_passthrough_model 等）移入 `agent/src/provider_resolver.rs`

**验证**：`cargo test -p codewhale-agent` 通过（48 tests passed）。

---

### 阶段 3：`core` 瘦身 — 提取领域服务

**目标**：将 `core` 从"上帝模块"拆分为多个领域服务。

**当前状态**：
- [x] `core` 包含会话管理、线程管理、工具编排、任务管理、MCP 管理等

**改进项**：
- [x] 在 `core/src/` 下创建子模块：`thread.rs`, `job.rs`
- [x] 将 `InitialHistory`, `NewThread`, `JobStatus`, `JobRetryMetadata` 等类型按职责归入子模块
- [x] 将 MCP 启动管理逻辑保留在 `core` Runtime（编排层职责）
- [x] 将钩子调度逻辑保留在 `core`（hooks 已是独立 crate，core 只做编排）

**验证**：`cargo test -p codewhale-core` 通过（41 tests passed），`cargo build --workspace` 零警告。

---

### 阶段 4：提示词系统独立

**目标**：将提示词从 `tui` 和 `core` 中解耦为独立资源。

**当前状态**：
- [x] 提示词文件在 `tui/src/prompts/`
- [x] 提示词逻辑分散在 `core` 和 `tui` 中

**改进项**：
- [ ] 创建 `crates/prompts/` crate（或在 `config` 下增加 `prompts` 模块）
- [ ] 将 `tui/src/prompts/*.md` 移入 `crates/prompts/assets/`
- [ ] 提供 `PromptLoader` trait，由 `tui` 和 `core` 通过 trait 获取提示词
- [ ] `constitution.md` 的解析和模板渲染逻辑集中在 prompts crate

**验证**：`cargo test -p codewhale-prompts` 通过（如新建 crate）。

> **状态**：评估后暂不实施。提示词组装逻辑（prompts.rs, 35K tokens）深度耦合 TUI 类型（AppMode、ProjectContext、SystemPrompt），独立为 crate 需大量接口抽象，收益有限。

---

### 阶段 5：消除跨层不当依赖

**目标**：修正依赖方向，使依赖图更符合 DDD 分层。

**改进项**：
- [ ] `tui` 对 `release` 的依赖改为可选 feature 或通过 `cli` 间接调用
- [ ] `config` 对 `execpolicy` 的依赖改为通过 trait 注入（依赖倒置）
- [ ] 确保依赖方向：UI → 应用服务 → 领域服务 → 领域模型 → 基础设施

**验证**：`cargo build --workspace` 通过，`cargo doc` 无循环依赖警告。

> **状态**：评估后暂不实施。`config→execpolicy` 是配置域对策略类型的合理依赖（重导出 ToolAskRule、使用 ExecPolicyEngine）；`tui→release` 仅用于版本检查（8 处调用），是 UI 层合理需求。强行依赖倒置增加复杂度而收益甚微。

---

### 阶段 6：统一运行时路径

**目标**：消除 `tui` 和 `core` 的平行实现，让 TUI 使用 core 的统一运行时。

**当前状态**：
- [x] TUI 绕过 core 构建自己的运行时
- [x] CLI 通过 app-server 使用 core

**改进项**：
- [ ] 识别 `tui` 中与 `core` 重复的逻辑（会话管理、工具调度等）
- [ ] 将重复逻辑迁移到 `core`，`tui` 通过 trait/接口调用
- [ ] 保持 TUI 的 ratatui 渲染层不变，只替换底层运行时调用

**验证**：TUI 启动、对话、工具调用、会话恢复等功能正常。

> **状态**：评估后暂不实施。TUI 运行时（RuntimeThreadManager、SubAgentRuntime、PythonRuntime、McpOAuthRuntime 等）与 core 的 Runtime 是根本不同的设计，不是简单的重复代码。TUI 需要处理 ratatui 渲染循环、终端事件、子代理编排、MCP OAuth 等，这些远超 core 的 API 编排职责。强行统一会引入巨大风险。

---

## 五、目标架构分层

```
┌─────────────────────────────────────────────────────────────┐
│                     用户交互层（不变）                        │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────────┐  │
│  │  TUI (ratatui)│  │  CLI (clap)  │  │  HTTP/SSE API     │  │
│  └──────┬──────┘  └──────┬───────┘  └────────┬──────────┘  │
└─────────┼────────────────┼───────────────────┼──────────────┘
          │                │                   │
          ▼                ▼                   ▼
┌─────────────────────────────────────────────────────────────┐
│                    应用服务层                                 │
│  ┌──────────────┐  ┌──────────────┐                         │
│  │  app-server   │  │     cli      │                         │
│  └──────────────┘  └──────────────┘                         │
└─────────────────────────────────────────────────────────────┘
          │                │
          ▼                ▼
┌─────────────────────────────────────────────────────────────┐
│                    领域服务层                                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────┐   │
│  │   core    │ │  agent   │ │  config  │ │   prompts    │   │
│  │(引擎编排) │ │(模型领域) │ │(配置领域) │ │(提示词策略)  │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────────┘   │
└─────────────────────────────────────────────────────────────┘
          │                │               │
          ▼                ▼               ▼
┌─────────────────────────────────────────────────────────────┐
│                    领域模型层                                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────┐   │
│  │  tools   │ │execpolicy│ │  hooks   │ │   secrets    │   │
│  │(工具抽象) │ │(策略引擎) │ │(钩子调度) │ │(密钥管理)    │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────────┘   │
└─────────────────────────────────────────────────────────────┘
          │                │               │
          ▼                ▼               ▼
┌─────────────────────────────────────────────────────────────┐
│                    基础设施层                                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────┐   │
│  │ protocol │ │   mcp    │ │  state   │ │  whaleflow   │   │
│  │(协议类型) │ │(MCP客户端)│ │(持久化)  │ │(Starlark引擎)│   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### 依赖规则

1. **只能向下依赖**，不能反向
2. **同层不互相依赖**（通过上层编排）
3. **基础设施层**是纯技术组件，不包含业务逻辑
4. **领域模型层**定义核心抽象（trait），不依赖具体实现
5. **领域服务层**编排领域模型，实现业务用例
6. **应用服务层**处理协议转换、路由、认证等横切关注点
7. **用户交互层**只做展示和输入，不包含业务逻辑

---

## 六、改进优先级与风险评估

| 阶段 | 优先级 | 风险 | 预期收益 | 状态 |
|------|--------|------|----------|------|
| 1. 拆分 protocol | 高 | 低（纯重导出） | 代码可读性提升 | ✅ 已完成 |
| 2. 抽取 agent | 高 | 低 | Agent 领域清晰化 | ✅ 已完成 |
| 3. core 瘦身 | 中 | 中（需仔细测试） | 降低维护成本 | ✅ 已完成 |
| 4. 提示词独立 | 中 | 低 | 策略可配置化 | ⏸ 评估后暂缓（耦合过深） |
| 5. 消除跨层依赖 | 低 | 中 | 架构纯净度 | ⏸ 评估后暂缓（依赖合理） |
| 6. 统一运行时 | 低 | 高（影响面大） | 消除重复代码 | ⏸ 评估后暂缓（设计差异大） |
| 7. 集成 whaleflow | 低 | 低 | 消除孤立模块 | [ ] 待评估集成方案 |
| 8. 重命名 agent crate | 中 | 低 | 命名准确性 | [ ] 待评估影响范围 |
| 9. 清理遗留文件 | 高 | 极低 | 减少混淆 | [ ] 待执行 |

---

## 七、关键设计决策记录

### 决策 1：保留双入口（CLI + TUI）

CLI 和 TUI 是不同的用户交互方式，但应共享同一套领域服务。不合并为单一入口。

### 决策 2：protocol 保持为共享内核

protocol 类型是多个限界上下文的共享语言，拆分为子模块但不拆分为独立 crate，避免依赖爆炸。

### 决策 3：提示词是配置而非代码

提示词应被视为"可配置的策略资源"，而非硬编码在 Rust 代码中。保持 `.md` 文件形式，通过加载器注入。

### 决策 4：不引入 DI 框架

Rust 生态不适合传统 DI 框架。通过 trait + 泛型实现依赖倒置，保持零成本抽象。
