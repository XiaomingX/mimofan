# CodeWhale 架构改进计划

> 基于 DDD（领域驱动设计）理论，从第一性原理出发的架构分析与改进方案。
> 改造范围：仅影响底层实现，不改变用户可见的交互方式和外部 API 接口。
> 最后更新：2026-06-26

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
| 多模型接入 | DeepSeek、OpenAI、Anthropic、小米 MiMo 等 25+ 模型提供商 |
| 工具执行 | Shell、文件读写、Git、搜索、代码执行等内置工具 |
| 上下文管理 | 会话持久化、上下文压缩、崩溃恢复、离线队列 |
| 安全策略 | 沙箱隔离（macOS Seatbelt / Linux Landlock）、执行审批、权限控制 |
| 扩展机制 | MCP 协议、Skills 技能系统、Hooks 钩子 |
| 协作模式 | 子代理派生（最多 20 并发）、后台任务队列 |

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
#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn kind(&self) -> ToolKind;
    fn is_mutating(&self) -> bool { false }
    async fn handle(&self, invocation: ToolInvocation)
        -> Result<ToolOutput, FunctionCallError>;
}
```

所有工具（内置、MCP、Shell）通过统一 trait 注册到 `ToolRegistry`，调度层无需关心具体实现。干净的**策略模式**。

### 3. 分层审批策略

```
ExecPolicyEngine → AskForApproval → ReviewDecision
     (规则引擎)       (策略枚举)       (用户决策)
```

三层规则集（BuiltinDefault → Agent → User），arity-aware 命令匹配，会话级审批记忆。策略粒度清晰。

### 4. 宪法式提示词体系

```
constitution.md (Tier 1 - 宪法，不可违反)
  ↓
statutes (Tier 2 - 法规：语言/格式/验证/执行纪律)
  ↓
regulations (Tier 3 - 规章：组合模式/子代理策略/资源管理)
  ↓
evidence (Tier 6 - 证据：工具手册/选择指南)
```

优先级从高到低，冲突时高层级覆盖低层级。**分层治理**的经典模式。

### 5. 流式优先架构

所有 LLM 响应通过 `EventFrame` 流式推送（20+ 事件变体），支持增量渲染、实时反馈。

### 6. 生产级 panic 安全

`spawn_supervised` 包装 `tokio::spawn`，捕获 panic 并写入 crash dump。12 个生产调用点覆盖关键路径（引擎、子代理、LSP、持久化）。

### 7. Channel 驱动的 UI-Engine 解耦

Engine 与 UI 通过 mpsc channel 通信（`Op` → Engine, `Event` → UI），不共享内存。`CancellationToken` 管理取消，`CancelReason` 区分取消来源。

---

## 三、架构边界问题诊断

### 问题 1：`tui` 绕过 `core` 构建平行运行时

```
codewhale-tui 依赖: config, execpolicy, protocol, release, secrets, tools
codewhale-tui 不依赖: core, agent, hooks, mcp, state
```

`tui` 没有使用 `core` crate，而是自己构建了一套完整的运行时集成路径（RuntimeThreadManager、SubAgentRuntime 等）。`core` 中的线程管理、会话管理、工具编排逻辑在 TUI 模式下被绕过。

**DDD 诊断**：领域逻辑泄漏到了 UI 层。

**实际评估**：这是**有意为之**的设计。TUI 需要处理 ratatui 渲染循环、终端事件、子代理编排、MCP OAuth 等，这些远超 core 的 API 编排职责。强行统一会引入巨大风险。**保持现状。**

### 问题 2：`agent` crate 命名与实际职责不符

`agent` 实际只包含 `ModelRegistry`（模型注册表和解析逻辑），名字暗示它是 Agent 系统的核心。

**DDD 诊断**：聚合根命名误导。

**实际评估**：已在 Phase 2 中抽取了 `family.rs` 和 `provider_resolver.rs`，领域逻辑已清晰分离。重命名 crate 影响面大（所有依赖方、CI、发布流程），收益有限。**保持现状，文档明确说明。**

### 问题 3：`whaleflow` 完全孤立

`whaleflow` 无任何内部依赖，也不被任何其他 crate 依赖。Starlark 工作流引擎与主系统完全脱节。

**DDD 诊断**：未集成的限界上下文。

**实际评估**：这是一个实验性功能，设计了丰富的类型系统（BranchSet、TeacherReview、PromotionGate 等），但集成需要大量编排层工作。**保持现状，不强行集成。**

### 问题 4：遗留文件

- `crates/tui/src/prompts/agent.txt` — 旧版 prompt，已被 constitution.md 替代

**DDD 诊断**：技术债务。**应清理。**

---

## 四、稳定性与性能风险

### 风险 1：`unwrap()` 在生产代码中的使用

非测试代码中有 **513 处 `.unwrap()`**，热区：

| 文件 | 数量 | 风险等级 |
|------|------|----------|
| `fleet/manager.rs` | 117 | **高** — 舰队编排路径 |
| `snapshot/repo.rs` | 116 | **中** — 文件系统操作 |
| `working_set.rs` | 82 | **中** — 路径操作 |
| `commands/groups/session/session.rs` | 54 | **中** — 会话管理 |
| `tools/fetch_url.rs` | 41 | **中** — HTTP 响应解析 |

**好消息**：无 `RwLock::write().unwrap()` 或 `RwLock::read().unwrap()` 模式。锁获取全部使用 `.await`（异步锁）或 `try_lock()`（31 处，均有守卫）。

**改进项**：
- [ ] 将 `fleet/manager.rs` 的 unwrap 替换为 `?` 或 `.expect("context")`
- [ ] 将 `snapshot/repo.rs` 的 unwrap 替换为错误传播
- [ ] 将 `working_set.rs` 的路径 unwrap 替换为安全处理
- [ ] 其余文件逐步替换，优先处理用户交互路径

### 风险 2：`clone()` 热区

非测试代码中有 **2,425 处 `.clone()`**，热区：

| 文件 | 数量 | 影响 |
|------|------|------|
| `fleet/manager.rs` | 117 | 舰队扩展时内存/延迟增长 |
| `fleet/ledger.rs` | 42 | 账本记录克隆 |
| `tui/tab/manager.rs` | 38 | 标签页状态 |
| `client/chat.rs` | 35 | 每次 API 调用的请求构建 |
| `tools/plugin.rs` | 30 | 插件上下文 |

**改进项**：
- [ ] `client/chat.rs` — 审查请求构建路径，用引用替代不必要的 String clone
- [ ] `fleet/manager.rs` — 用 `Arc` 共享不可变状态，减少深拷贝
- [ ] 其余热区按需优化，需先 profile 确认瓶颈

### 风险 3：嵌套 Mutex 模式

`rlm/session.rs` 存在双层嵌套 Mutex：
```rust
pub type SharedRlmSessionStore = Arc<Mutex<HashMap<String, Arc<Mutex<RlmSession>>>>>;
```

**风险**：如果两个代码路径以不同顺序获取外层和内层锁，可能死锁。

**实际评估**：当前代码中内层锁仅在外层释放后获取，实际死锁风险低。但属于代码异味。

**改进项**：
- [ ] 重构为扁平结构（如 `Arc<Mutex<HashMap<String, RlmSession>>>`），或改用 `RwLock` 减少锁竞争

### 风险 4：`ui.rs` 文件过大（11,000+ 行）

渲染逻辑、事件处理、异步动作分发全在一个文件中。

**改进项**：
- [ ] 拆分为 `ui/chat.rs`（对话渲染）、`ui/sidebar.rs`（侧边栏）、`ui/footer.rs`（底部栏）、`ui/picker.rs`（选择器）等子模块
- [ ] 目标：单文件不超过 1,000 行

### 风险 5：工具执行完全串行化

`Engine::tool_exec_lock: Arc<RwLock<()>>` 对所有工具调用加写锁，意味着同一时刻只能执行一个工具。

**实际评估**：这是有意设计（防止并发工具执行导致文件冲突）。但如果未来需要并行工具执行，这是瓶颈。

**改进项**：
- [ ] 评估是否可以按工具类型分锁（读操作并行，写操作串行）

---

## 五、已完成的改进

| 阶段 | 改动 | 效果 | 状态 |
|------|------|------|------|
| Phase 1 | protocol 按限界上下文拆分为 8 个子模块 | lib.rs 714→17 行，代码可读性大幅提升 | [x] 已完成 |
| Phase 2 | agent 抽取 family.rs + provider_resolver.rs | 模型领域逻辑清晰分离 | [x] 已完成 |
| Phase 3 | core 拆分 job.rs + thread.rs | lib.rs 2767→1348 行，职责边界明确 | [x] 已完成 |
| 构建优化 | Cargo.toml 添加 profile 配置，启用 sccache | debug 构建减少 60-80% 空间，编译提速 55% | [x] 已完成 |
| 文档优化 | .claudeignore 补充 CLAUDE.local.md、scripts、assets 等 | 减少 AI 读取无关文件的 token 浪费 | [x] 已完成 |

---

## 六、待办改进清单

### 高优先级（影响稳定性）

- [ ] 替换 `fleet/manager.rs` 中 117 处 `unwrap()` 为错误传播
- [ ] 替换 `snapshot/repo.rs` 中 116 处 `unwrap()` 为错误传播
- [ ] 替换 `working_set.rs` 中 82 处 `unwrap()` 为安全处理
- [ ] 清理遗留文件 `crates/tui/src/prompts/agent.txt`

### 中优先级（影响可维护性）

- [ ] 拆分 `tui/ui.rs`（11,000+ 行）为子模块（chat、sidebar、footer、picker）
- [ ] 拆分 `tui/main.rs`（9,200+ 行）为初始化、参数解析、模块接线等独立文件
- [ ] 审查 `client/chat.rs` 的 35 处 clone，用引用替代不必要的 String 拷贝
- [ ] 重构 `rlm/session.rs` 的嵌套 Mutex 为扁平结构

### 低优先级（改善代码质量）

- [ ] 逐步替换其余文件中的 `unwrap()` 为 `?` 或 `.expect("context")`
- [ ] 评估 `fleet/manager.rs` 的 117 处 clone 是否可通过 `Arc` 优化
- [ ] 评估工具执行锁是否可以按类型分拆（读并行、写串行）

---

## 七、不做（经评估后明确排除）

以下改进经过深入分析后判断**收益不足以抵消风险或成本**：

| 提议 | 排除原因 |
|------|----------|
| 提示词系统独立为 crate | 提示词组装逻辑（35K tokens）深度耦合 TUI 类型（AppMode、ProjectContext），独立需大量接口抽象，收益有限 |
| 消除 config→execpolicy 依赖 | 配置域对策略类型的依赖是合理的领域依赖（重导出 ToolAskRule） |
| 统一 TUI 和 core 运行时 | TUI 运行时（ratatui 渲染、子代理编排、MCP OAuth）与 core 的 API 编排是根本不同的设计 |
| 重命名 agent crate | 影响所有依赖方、CI、发布流程，收益有限 |
| 集成 whaleflow | 实验性功能，集成需要大量编排层工作，当前保持独立更安全 |

---

## 八、关键设计决策记录

### 决策 1：保留双入口（CLI + TUI）

CLI 和 TUI 是不同的用户交互方式，但应共享同一套领域服务。不合并为单一入口。

### 决策 2：protocol 保持为共享内核

protocol 类型是多个限界上下文的共享语言，拆分为子模块但不拆分为独立 crate，避免依赖爆炸。

### 决策 3：提示词是配置而非代码

提示词应被视为"可配置的策略资源"，而非硬编码在 Rust 代码中。保持 `.md` 文件形式，通过加载器注入。

### 决策 4：不引入 DI 框架

Rust 生态不适合传统 DI 框架。通过 trait + 泛型实现依赖倒置，保持零成本抽象。

### 决策 5：TUI 独立运行时是有意设计

TUI 的运行时需求（终端渲染、事件循环、子代理编排）与 core 的 API 编排是根本不同的设计模式，不是重复代码。
