# 架构改进计划（DDD 视角）

> 基于第一性原理与领域驱动设计（DDD）的架构分析与改进方案。
> 本文档如实记录现状，**只写真实存在的待办**，不凑数、不迎合。
> 最后更新：2026-08-05

---

## 0. 说明：本文档与旧版的区别

仓库里曾有一份改进计划，声称已完成若干拆分（如 `tui/app.rs` 拆成 10 个子模块、`tools/` 移入独立 crate 等）。**经核查，这些 `[x]` 是失真的**：`app.rs` 根本不存在，`tools/`/`llm`/`prompts` 也未移出 `tui` crate。本文档以真实代码状态为准重写，避免误导后续决策。

---

## 1. 系统主要用途（第一性原理）

mimofan 是一个**跑在终端里的 AI 编程搭档**：用户用自然语言（默认中文）下指令 → 调大模型思考 → 用工具（读文件、改代码、跑命令、查资料）把活干完。本质是一个**本地优先、模型无关的智能体运行时**。

- 对标：Claude Code / OpenCode
- 差异化：Rust 实现 · MIT · 默认小米 MiMo 模型 · 多入口（TUI / CLI / HTTP）
- 第一性原理判断：它的核心价值是"把自然语言意图安全、可复现地转成工具调用序列"。所有架构都应服务这个目标，任何偏离这一点（为扩展性而扩展性）都是过度设计。

---

## 2. 架构精妙之处（值得保留）

| 维度 | 评价 | 为什么好 |
|------|------|---------|
| **Crate 依赖图** | ✅ | 15 个 crate 形成严格向下的 DAG，无环。这是 Rust workspace 工程纪律的体现，编译隔离清晰。 |
| **共享内核（protocol）** | ✅ | `protocol` crate 作为 DTO 层，多入口共享同一套消息/工具类型。典型的 DDD 共享内核模式，避免各上下文重复定义。 |
| **端口化设计** | ✅ | 22+ 个 trait 端口（`Tool`、`SandboxBackend`、`LlmClient`、`McpBackend`、`Hook` 等），扩展点清晰，符合"依赖倒置"。 |
| **自研 wire format** | ✅ | 不依赖任何官方 LLM SDK，自己实现 OpenAI/Anthropic 线协议。依赖面小、可控、易换模型。 |
| **提示词分层宪法** | ✅ | `constitution → statutes → regulations → project → memory → …` 的层级，硬约束不可被覆盖。是提示词工程的成熟做法。 |
| **Godfile 战术拆分（已完成）** | ✅ | 详见 §3，五大超大文件已按内聚性拆成子模块并合并（PR #567 / issue #566）。这是战术层（模块内聚）的正确优化。 |

---

## 3. 已经完成的拆分（[x] 真实状态）

五大 godfile 已在 2026-08-04～05 完成 DDD 战术拆分，并经 `cargo check` + 回归测试验收、合并入 `main`：

| 文件 | 原行数 → 现态 | 新增子模块 |
|------|--------------|-----------|
| `tui/src/tools/subagent/mod.rs` | 6265 → **2177** | `helpers` / `manager` / `runner` / `parser` / `tool` |
| `tui/src/tui/ui/mod.rs` | 5133 → **885** | `ui/ui_event_loop.rs`（4259） |
| `tui/src/tools/shell.rs` | 3172 → **2041** | `tools/shell_tools.rs`（1155） |
| `tui/src/core/engine.rs` | 3099 → **2782** | `core/engine/engine_messages.rs`（328） |
| `config/src/lib.rs` | 3193 → **2019** | 14 个配置子模块 |

- [x] 拆分 `subagent/mod.rs` 超大型模块（AgentTool + helpers/manager/runner/parser）
- [x] 拆分 `tui/ui/mod.rs` 的 `run_event_loop`
- [x] 拆分 `shell.rs` 的 tool 实现
- [x] 拆分 `engine.rs` 的消息/目标事件 helper
- [x] 拆分 `config/lib.rs` 为领域子模块
- [x] 验收：`cargo check` 全绿；95 个 lib 测试 + 48 个 tui 回归测试通过
- [x] 已合并：`PR #567` / `issue #566`

**结论**：战术层（单文件内聚）已达标。**战略层（crate 级限界上下文）仍是核心问题**（见 §4）。

---

## 4. 架构边界问题（从 DDD 看）

### 问题 1：tui crate 仍是"大泥球"（最核心）

- **现状**：`tui` crate 约 20 万行，占全仓 85%+。Godfile 拆分后，文件变小了，但**所有子域仍挤在一个 crate 里**，通过 `crate::` 互相直连。
- **DDD 视角**：限界上下文边界缺失。"用户界面""对话运行时""模型网关编排""工具调度""提示词构建""配置加载"本应是不同生命周期、不同模型的子域，现在共享一个编译单元与命名空间。
- **影响**：编译慢（任一改动重编整个 tui）、耦合高、新人难上手。

### 问题 2：双重运行时

- **现状**：`core::Runtime`（2177 行）与 `tui` 自有的引擎（engine.rs + 运行时）并存，处理相似问题但 API 不同。
- **DDD 视角**：应用核心（Application Core）不唯一。DDD 要求一个明确的、可复用的应用层。
- **影响**：逻辑重复、维护成本翻倍。

### 问题 3：UI 层直接 IO

- **现状**：`prompt_suggestion.rs` 在渲染层直接发 HTTP；`file_tree.rs`/`clipboard.rs` 有同步 `std::fs` 调用。
- **DDD 视角**：界面层（接口层）不应直接依赖基础设施。应通过端口（gateway/repository）反转依赖。
- **影响**：难以测试、UI 与基础设施耦合。

### 问题 4：execpolicy 双实现

- **现状**：`tui/src/execpolicy/`（11 文件）与独立 `crates/execpolicy` 并存。
- **DDD 视角**：同一领域概念两套实现 = 上下文重复（duplicate model）。
- **影响**：维护成本、可能不一致。

### 问题 5：memory crate 孤立

- **现状**：`crates/memory`（向量记忆系统）无任何上游 crate 依赖它。
- **DDD 视角**：一个未被集成的子域 = "僵尸上下文"。
- **影响**：代码浪费、易误导（让人以为记忆功能可用）。

### 问题 6：i18n 与 UI 强耦合（已知但不动）

- **现状**：`localization` 通过 `crate::localization::tr(MessageId)` 渗透约 100+ 个 UI 调用点（每个命令、控件、按键都有 `tr(...)`）。
- **判断**：这是合理的 i18n 设计。**但**移除它需要改写全部 UI 文本调用点，会直接改动"与用户交互的层"。这与本任务约束"改造只影响底层，不影响与用户有交互的使用方法"冲突，且收益低、风险高。
- **结论**：**保持现状**，不纳入本次优化。

### 问题 7：规划/状态记录失真（治理问题）

- **现状**：旧改进计划声称完成了不存在的拆分。
- **根因**：缺少"完成即回写"的纪律。
- **影响**：状态投影失真，后续决策被误导。本次已纠正。

---

## 5. 改进计划（只含真实待办）

> 进度：分析类步骤已落实（标 `[x]`）；涉及安全/高风险的"合并/注入"步骤保留为 `[ ]`，需专门分支 + `cargo test` 验收后再合入（沿用 PR 流程）。最后更新：2026-08-05。

### Phase A：统一双重运行时（结论：无需合并，已符合最佳实践）

> 经逐方法核查，**原"双运行时"前提是命名撞车造成的误判**，实为非重复的两套正确架构。本 Phase 改为澄清事实，不执行有风险的合并。

- [x] 核查结论（决定性）：
  - `mimofan_core::Runtime`（`crates/core/src/lib.rs:35`）= **无界面（headless）应用核心**，被 **app-server HTTP API** 独占使用（`app-server/src/lib.rs:14`，调用 `handle_thread`/`handle_prompt`/`invoke_tool`/`mcp_startup`）。注意 `handle_prompt` 只记录消息并返回元数据，**不实现 LLM 对话循环**。
  - tui `crate::core::Engine`（`crates/tui/src/core/engine/`，`spawn_engine`/`EngineHandle`）= **交互式 LLM 循环**（流式/轮次/终端），被 **TUI 独占使用**（`runtime_threads/mod.rs:31` 导入）。**TUI 从不使用 `mimofan_core::Runtime`**（全仓搜不到 tui 对 crate 级 Runtime 的引用）。
  - 二者是 DDD 下**两个正确的限界上下文**（交互 UI vs 无界面 API），依赖**共享内核**（`protocol`/`tools`/`execpolicy`/`state`/`config`/`agent`/`mcp`/`hooks`）。
  - 策略检查（`mimofan_execpolicy::ExecPolicyEngine.check()`）与工具派发（`tool_registry.dispatch`）**早已通过共享内核复用**，未在两边都重写；tui 的 `tool_execution.rs` 仅处理 TUI 专属机制（交互终端暂停、并行扇出），不重实现审批流。
- [x] 决策：**保留两份，明确分工边界**（即现状）。强行"统一/合并"会耦合交互 UI 循环与无界面 API、违反限界上下文边界、同时威胁 TUI 与 HTTP API 两个入口——是反模式，非改进。当前结构已符合 DDD 最佳实践，不硬写可有可无的待办。
- [ ] （可选/低优先，未执行）命名撞车（`mimofan_core::Runtime` vs tui `crate::core::Engine` / `crate::core`）易误读。若要做，仅在 app-server 侧加类型别名或注释澄清，属纯机械改名、波及面大，按奥卡姆剃刀暂不做。

**预期效果（本次已达成）**：澄清架构事实，避免在错误前提上做高风险重构，守住 DDD 限界上下文边界。

### Phase B：UI 层 IO 收口

目标：通过端口注入替代 UI 层直接 IO。

- [x] 枚举 UI 层直接 IO（已定位，仅 3 处，范围远小于"全 tui 的 fs"）：
  - `tui/src/tui/prompt_suggestion.rs:14` 渲染层直接用 `reqwest::Client` 发 HTTP
  - `tui/src/tui/clipboard.rs:343/372` 渲染层同步 `std::fs`
  - `tui/src/tui/file_tree.rs:202` 渲染层 `std::fs::read_dir`
  - 注：其余 `std::fs`/`reqwest` 多在 `settings`/`snapshot`/`fleet`/`mcp` 等基础设施/领域模块，本就在底层，无需改造。
- [ ] 定义端口 trait（`HttpClient` / `FileSystem`）
- [ ] 实现适配器并注入（仅针对上述 3 处渲染层 IO）
- [ ] 修改 UI 层改用端口
- [ ] `cargo test` 通过

**预期效果**：UI 可测、依赖方向正确。

### Phase C：execpolicy 去重

目标：消除执行策略的重复实现。**结论（已核实，诚实修正）**：所谓「双实现」并非同一逻辑的两份拷贝，而是**互补的两套机制**，读不同的配置源、职责不同，不能简单删其一。

- [x] 对比差异（落实关键结论）：
  - **crate `mimofan-execpolicy` = 运行时审批引擎（权威门禁）**：被 core/config/tui/protocol/app-server 依赖，提供 `ExecPolicyEngine` / `AskForApproval` / `ExecPolicyContext`。真正的执行门禁在 `core/engine/policy.rs:283` 调 `.check()`（`exec_shell` 审批由此决定）。
  - **tui 本地 `crate::execpolicy` = 文件策略覆盖 + CLI 诊断**，读**独立配置源** `~/.mimofan/execpolicy.toml`：
    - `shell_tools.rs`：`load_default_policy()` + `policy.evaluate()` 对 `exec_shell` 做**二次 deny 覆盖**（与主引擎叠加，非替代）。
    - `cli/mod.rs:867`：`ExecPolicyCheckCommand`（用户命令 `mimofan execpolicy check`）。
    - `matcher::pattern_matches` / `prefix_allow_matches`：被 `command_safety.rs` 复用。
  - 二者语义不同：crate 用**基数感知前缀**匹配；tui-local 用 **通配 `*` 正则**匹配（`execpolicy.toml` 格式 `[group] allow/deny`）。直接替换会改变用户的策略匹配行为。

- [x] 安全去重（已实施）：移除 `crates/tui/src/tools/shell.rs` 中 `use crate::execpolicy::{ExecPolicyDecision, load_default_policy};` **死导入**（仅 import 从未使用）。`cargo check -p mimofan` 通过，无破坏。
- [x] 决策：**保留两份**（互补）。更深合并（让 crate 引擎也能消费 `execpolicy.toml`、删 tui-local 评估逻辑）会改变通配/基数匹配语义，属**用户可感知的行为变化**，需你显式拍板后再做，不在本次擅自执行。
- [ ] （可选/低优先，待定）`tui/src/execpolicy/matcher.rs::canonical_executable_form` 与 crate `lib.rs::canonical_executable_form` 近乎逐字重复（仅大小写处理不同）。因大小写敏感度差异影响 deny/allow 语义，**暂不合并**，仅标注为已知重复项。

**预期效果（本次已达成）**：清理了真实冗余（死导入），并澄清架构事实，避免盲目合并安全相关代码引入回归。

### Phase D：memory 决策

目标：决定向量记忆系统的去留。

- [x] 评估成熟度与价值：结论——`mimofan-memory`（2736 行，向量/embedding）**全仓无任何上游依赖**（僵尸上下文）；主流程实际使用的是 `mimofan`(tui) crate 内的 `crate::memory` 简单文件记忆模块（`lib.rs:71`）。该 crate 不成熟且未集成。
- [x] 二选一决策：**明确标记 experimental**（不强行接入、不立即删除，保留供评估）。
- [x] 实施：已在 `crates/memory/src/lib.rs` 顶部加中文 ⚠️ 实验性警告，并在 `Cargo.toml` description 标注 `(EXPERIMENTAL: not integrated)`；`cargo check -p mimofan-memory` 通过。若后续评估决定不接入，应整体移除本 crate。

**预期效果**：不再误导——明确该 crate 未集成、不可在生产路径依赖。

### 明确不做（及原因）

- [x] **不移除 i18n / `localization`**：违反"不动用户交互层"约束，且改写 100+ UI 调用点收益低、风险高。保持现状。
- [x] **不把 tui 子域提升为独立 crate（战略重构）**：属于"新增架构"，超出"只做存量优化"范围，且风险高。当前先做战术拆分（已完成），战略重构留作后续独立评估，不在本计划强行展开。

---

## 6. 实施纪律（防止再次失真）

1. **只影响底层**：改进不改动 TUI/CLI/HTTP 对用户的行为与入口。
2. **MECE**：每个 Phase 职责互斥、完全穷尽。
3. **奥卡姆剃刀**：不引入不必要的抽象；能不动就不动。
4. **不新增功能**：只做存量优化。
5. **完成即回写**：每完成一项，立即更新本文档 `[ ]` → `[x]` 并跑验收，不让状态再次失真。
6. **验收门槛**：每个 Phase 后必须 `cargo build --workspace` + `cargo test --workspace` 通过。

---

## 7. 风险评估

| 风险 | 影响 | 缓解 |
|------|------|------|
| 双重运行时合并引入 bug | 高 | 充分测试、灰度 |
| UI IO 收口影响性能 | 中 | 必要时保留同步快路径 |
| execpolicy 去重影响安全 | 高 | 保留沙箱能力、安全测试 |
| memory 接入不成熟 | 低 | 评估后决定，不强行 |

---

> 注：本文档刻意**不列**任何"可有可无"的待办。若某子域当前已符合最佳实践，就标记完成或明确不做，而不是硬凑 TODO。
