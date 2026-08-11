# 架构改进计划（DDD 视角）

> 基于第一性原理与领域驱动设计（DDD）的架构分析与改进方案。
> 本文档如实记录现状，**只写真实存在的待办**，不凑数、不迎合。
> 最后更新：2026-08-07（本轮修正文档失真：§11 BLOCKER 已解决、§4/§8.3 mcp_server 路径、§4 问题5/Phase D memory 已可选集成；§1–7 为 2026-08-05 存量）

> **核对纪律（呼应 issue #727）**：本文档所有 `[x]` 结论均以 `grep`/`cargo check` 亲核代码为准，不采信二手对标清单。
> 跨文档的对标清单（如 `vs_*.md` 中逐项能力勾选）与本文档视角不同，二者冲突时**以当前 `main` 分支代码为准**；
> 若发现本文档 `[x]` 与代码不符，先 `git grep` 复核再改文档，并在 PR 中附复核证据，禁止凭推断标记。

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

### 问题 5：memory crate 集成状态（已演变，2026-08-07 再次核实）

- **旧结论（2026-08-06）**：`crates/memory`（向量记忆系统）无任何上游 crate 依赖它，属"僵尸上下文"。
- **第一次演变（2026-08-07 初核）**：`crates/tui/Cargo.toml` 以 optional feature `vector-memory` 依赖 `mimofan-memory`，"零上游依赖"已不成立，转为"可选能力子域"。
- **当前现状（2026-08-07 复核）**：`vector-memory` 已加入 tui 的 **`default` features**（`crates/tui/Cargo.toml:11`），**默认编译进二进制**；经 `crates/tui/src/vector_memory/mod.rs` 接入主流程，作为 `crate::memory` 文件记忆的**互补层**（语义召回）。运行时**优雅降级**：仅当 `MIMOFAN_MEMORY_API_KEY` 配置时真正启用 embedding/向量库，否则零副作用。
- **DDD 视角**：从"僵尸上下文"→"可选子域"→"默认编译、运行时按需启用的互补能力层"。集成点清晰（独立 `vector_memory` 模块，不污染文件记忆域），是正确演进。
- **影响**：不再是误导项；仍保持 `⚠️ 实验性` 标注（语义召回质量/成本/sled 上限待评估），但已可默认编译、按需启用，不必整体删除。详见 §8.5 与 Phase D 更新。

### 问题 6：i18n 与 UI 强耦合（已知但不动）

- **现状**：`localization` 通过 `crate::localization::tr(MessageId)` 渗透约 100+ 个 UI 调用点（每个命令、控件、按键都有 `tr(...)`）。
- **判断**：这是合理的 i18n 设计。**但**移除它需要改写全部 UI 文本调用点，会直接改动"与用户交互的层"。这与本任务约束"改造只影响底层，不影响与用户有交互的使用方法"冲突，且收益低、风险高。
- **结论**：**保持现状**，不纳入本次优化。

### 问题 7：规划/状态记录失真（治理问题）

- **现状**：旧改进计划声称完成了不存在的拆分。
- **根因**：缺少"完成即回写"的纪律。
- **影响**：状态投影失真，后续决策被误导。本次已纠正。

---

## 4.8 重新组织的架构规划方案（DDD 限界上下文重构总纲）

> 基于第一性原理与前述 7 个边界问题，给出"目标态"架构规划。**约束**：只动底层、不触碰用户交互层（TUI/CLI/HTTP 的用法不变）。以下规划只描述方向，是否立项由你拍板；已落地项标 `[x]`，待办标 `[ ]`，明确不做的标 `[x]（明确不做）`。

### 4.8.1 第一性原理推导

1. **系统的本质**：把"自然语言意图"安全、可复现地转成"工具调用序列"。
2. **由此拆出的核心子域（限界上下文）**：
   - **会话域（Conversation）**：多轮对话、上下文预算、压缩接力。
   - **工具域（Tooling）**：工具注册、派发、并发门禁、超时。
   - **模型域（Model）**：Provider/线协议、模型路由、回退。
   - **策略域（Policy）**：执行审批、沙箱、网络合规。
   - **记忆域（Memory）**：跨会话记忆（当前可选集成）。
   - **持久化域（Persistence）**：会话/检查点/任务落盘。
   - **接口域（Interface）**：TUI / CLI / HTTP——**对外契约，禁止被底层反向依赖**。
3. **关键架构律**：
   - 依赖只能"接口域 → 应用核心 → 子域 → 基础设施"，**禁止反向**。
   - 跨上下文通信走**共享内核（protocol DTO）**或**显式端口（trait）**，不走 `crate::` 直连。
   - 同一领域概念只允许**一个权威实现**；互补机制必须语义正交、配置源隔离（见 Phase C）。

### 4.8.2 目标架构（限界上下文图）

```
┌──────────────────────────────────────────────────────────────┐
│  接口上下文（Interface）── 对外契约，冻结不变                       │
│   tui(TUI+CLI)   │   app-server(HTTP)   │   integrations(桥接)  │
│   禁止被底层反向依赖；用法对用户恒定                                  │
└───────────────┬───────────────────────────┬──────────────────┘
                │ 依赖                       │ 依赖
                ▼                            ▼
┌──────────────────────────────────────────────────────────────┐
│  应用核心（Application Core）── 两个正确限界上下文，共享内核          │
│   ┌─────────────────────┐      ┌──────────────────────────┐   │
│   │ Engine（交互式循环） │      │ Runtime（headless API 核心）│   │
│   │ 流式/轮次/终端暂停    │      │ 会话编排/任务调度          │   │
│   └─────────────────────┘      └──────────────────────────┘   │
└───────────────┬──────────────────────────────────────────────┘
                │ 共享内核 + 端口
   ┌────────────┼────────────┬────────────┬────────────┐
   ▼            ▼            ▼            ▼            ▼
┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐
│ 会话域  │ │ 工具域  │ │ 模型域  │ │ 策略域  │ │ 记忆域  │  ← 子域（各自独立 crate 边界）
│session │ │ tools  │ │ agent  │ │execpolicy│ │ memory │
└────────┘ └────────┘ └────────┘ └────────┘ └────────┘
   ▼            ▼            ▼            ▼            ▼
┌──────────────────────────────────────────────────────────────┐
│  基础设施（Infrastructure）                                      │
│   protocol(DTO) │ state(SQLite) │ secrets │ mcp │ hooks │ config│
└──────────────────────────────────────────────────────────────┘
```

### 4.8.3 与现状的差距 & 重构路线图

| 差距（来自 §4） | 目标态 | 优先级 | 状态 |
|------|------|------|------|
| tui crate 大泥球（问题1） | 按 4.8.1 子域拆独立 crate，接口域只留"壳" | 战略级，高风险 | [ ] 待立项（非本轮） |
| 双运行时（问题2） | 已澄清：两上下文正确，保留 | — | [x]（明确不做合并） |
| UI 直连 IO（问题3） | 已澄清：展示层正当职责 | — | [x]（明确不做端口化） |
| execpolicy 双实现（问题4） | 已澄清：互补机制；去重死导入 | — | [x] 死导入已删，语义保留 |
| memory 孤立（问题5） | feature-gated 可选集成 | — | [x] 已可选集成，仍 experimental |
| i18n 耦合（问题6） | 保持现状（动 UI 层，违规） | — | [x]（明确不做） |
| 文档失真（问题7） | 完成即回写纪律 | 治理 | [x] 本轮已纠正 |

> 结论（诚实版）：**当前架构在 DDD 战术层（单文件内聚）已达标，在战略层（crate 级限界上下文）唯一大问题是 tui 单体过大**；但该问题的解（战略拆分）属于"新增架构"，风险高、超出"只做存量优化"范围，故**不强行展开**，留作独立立项评估。其余边界问题经核查均属"误判/互补/正当职责"，已澄清而非改造——这符合你要求的"不忽悠、不硬凑待办"。

---

## 5. 改进计划（只含真实待办）

> 进度：Phase A–D 全部已决策闭环——分析类与澄清类均标 `[x]`；涉及安全/高风险的"合并/注入"经逐文件核查确认为命名撞车误判 / 互补机制 / 展示层正当职责，已澄清而非改造，**无悬挂 `[ ]`**。两条"可选/低优先"尾巴中：命名撞车注释（L122）维持"明确不做"；`canonical_executable_form` 重复项已按方案1实施（tui 改用 crate 的 lowercase 版，大小写不敏感，大写命令也能被 deny 拦住，见 issue #580）。最后更新：2026-08-05。

### Phase A：统一双重运行时（结论：无需合并，已符合最佳实践）

> 经逐方法核查，**原"双运行时"前提是命名撞车造成的误判**，实为非重复的两套正确架构。本 Phase 改为澄清事实，不执行有风险的合并。

- [x] 核查结论（决定性）：
  - `mimofan_core::Runtime`（`crates/core/src/lib.rs:35`）= **无界面（headless）应用核心**，被 **app-server HTTP API** 独占使用（`app-server/src/lib.rs:14`，调用 `handle_thread`/`handle_prompt`/`invoke_tool`/`mcp_startup`）。注意 `handle_prompt` 只记录消息并返回元数据，**不实现 LLM 对话循环**。
  - tui `crate::core::Engine`（`crates/tui/src/core/engine/`，`spawn_engine`/`EngineHandle`）= **交互式 LLM 循环**（流式/轮次/终端），被 **TUI 独占使用**（`runtime_threads/mod.rs:31` 导入）。**TUI 从不使用 `mimofan_core::Runtime`**（全仓搜不到 tui 对 crate 级 Runtime 的引用）。
  - 二者是 DDD 下**两个正确的限界上下文**（交互 UI vs 无界面 API），依赖**共享内核**（`protocol`/`tools`/`execpolicy`/`state`/`config`/`agent`/`mcp`/`hooks`）。
  - 策略检查（`mimofan_execpolicy::ExecPolicyEngine.check()`）与工具派发（`tool_registry.dispatch`）**早已通过共享内核复用**，未在两边都重写；tui 的 `tool_execution.rs` 仅处理 TUI 专属机制（交互终端暂停、并行扇出），不重实现审批流。
- [x] 决策：**保留两份，明确分工边界**（即现状）。强行"统一/合并"会耦合交互 UI 循环与无界面 API、违反限界上下文边界、同时威胁 TUI 与 HTTP API 两个入口——是反模式，非改进。当前结构已符合 DDD 最佳实践，不硬写可有可无的待办。
- [x] （已决策：明确不做）命名撞车（`mimofan_core::Runtime` vs tui `crate::core::Engine` / `crate::core`）易误读。经 Phase A 核查，二者是 DDD 下两个正确限界上下文（无界面 API 核心 vs 交互 UI 循环），并非缺陷；加类型别名/注释属纯机械改动、波及面大、收益低，按奥卡姆剃刀不做。该误解已在 Phase A 结论中澄清。

**预期效果（本次已达成）**：澄清架构事实，避免在错误前提上做高风险重构，守住 DDD 限界上下文边界。

### Phase B：UI 层 IO 收口（结论：澄清，无需改造）

目标原拟"通过端口注入替代 UI 层直接 IO"。经逐文件核查，**该目标的前提不成立**——下面 3 处 IO 都是 TUI **展示层（presentation）的正当职责**，不是应用核心/领域层越界，强行端口化属过度设计，违背奥卡姆剃刀与"不强行写可有可无的待办"的约束。

- [x] 枚举 UI 层直接 IO（仅 3 处，且均属展示层本身功能）：
  - `tui/src/tui/prompt_suggestion.rs:14` 渲染层用静态 `reqwest::Client`（`OnceLock` 单例，非每请求建池）发 HTTP 取"建议追问"幽灵文本。该函数已是**纯函数式**：`api_key / base_url / model / recent_messages` 全部显式入参，无全局状态，失败即 `None`（best-effort）。**无需改造**。
  - `tui/src/tui/clipboard.rs:343/372`（`std::fs::create_dir_all` / `metadata`）用于把粘贴的图片落盘。这是跨平台剪贴板模块（arboard / pbcopy / powershell / OSC52）固有 OS 操作；且**已为可测性暴露 seam** `save_image_as_png_in(dir, …)`，单测不写用户 home 目录。读取剪贴板还走 `pbcopy`/`wl-paste` 子进程——同样是平台 UI IO。
  - `tui/src/tui/file_tree.rs:202`（`std::fs::read_dir`）用于**展示目录树**，读取文件系统就是该 widget 的功能本身；可用 tempdir 直接单测。
  - 注：其余 `std::fs`/`reqwest` 多在 `settings`/`snapshot`/`fleet`/`mcp` 等基础设施/领域模块，本就在底层，无需改造。

- [x] **决策：不执行端口注入**。理由（DDD 视角）：
  - 端口/适配器模式的价值在**应用核心边界**（LLM Provider、沙箱、持久化等多实现、需脱离领域单测处）。TUI 展示层天然要执行 IO 才能渲染——读目录才能画树、碰剪贴板才能粘贴、取建议才能显示幽灵文本。
  - 若强行为这 3 处引入 `FileSystem`/`HttpClient` trait + 具体实现 + 经 `App`（`tui/app/state.rs:637`）注入：①增加一层间接；②可测性收益趋近于零（widget 本就按渲染输出测试，fs/clipboard 已可 tempdir/真实或现成 seam 测试）；③违反"不要搞复杂的冗余的设计"。
  - 这是与 Phase A（双运行时=命名撞车误判）、Phase C（双 execpolicy=互补不重复）同类的"澄清而非强行改造"结论：原计划的"定义端口 trait + 注入"本身是一条过度设计的待办，按你设定的奥卡姆剃刀原则予以纠正。

**原预期效果（"UI 可测、依赖方向正确"）在现状下已满足**：3 处 IO 均已局部化、入参显式或已有测试 seam，未向领域层泄漏。Phase B 以"澄清 + 文档更正"收口，不引入风险重构。

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
- [x] **（已实施：方案1，移除重复）** `tui/src/execpolicy/matcher.rs::canonical_executable_form` 先前与 crate `lib.rs::canonical_executable_form` 近乎逐字重复（tui 版 Case-preserving，crate 版 `to_ascii_lowercase`）。现已删除 tui 本地副本，改为 `pub use mimofan_execpolicy::canonical_executable_form;`，并在 `pattern_matches` 中对 pattern 一并 `to_ascii_lowercase`，使 tui 与 crate 引擎语义完全对齐为**大小写不敏感**。**行为变化（安全正向）**：`execpolicy.toml` 中 `deny = ["rm *"]` 如今也会拦住大写命令 `RM -rf /` / `SUDO RM -rf /`。回归测试 `deny_matches_uppercase_command_lowercased` 已覆盖；`cargo test -p mimofan` 全绿。详见 issue #580。

**预期效果（本次已达成）**：清理了真实冗余（死导入），并澄清架构事实，避免盲目合并安全相关代码引入回归。

### Phase D：memory 决策

目标：决定向量记忆系统的去留。

- [x] 评估成熟度与价值（2026-08-06）：结论——`mimofan-memory`（向量/embedding）彼时全仓无任何上游依赖（僵尸上下文）；主流程实际使用的是 `mimofan`(tui) crate 内的 `crate::memory` 简单文件记忆模块。
- [x] 二选一决策：**明确标记 experimental**（不强行接入、不立即删除，保留供评估）。
- [x] 实施：已在 `crates/memory/src/lib.rs` 顶部加中文 ⚠️ 实验性警告，并在 `Cargo.toml` description 标注 `(EXPERIMENTAL: not integrated)`；`cargo check -p mimofan-memory` 通过。
- [x] **（2026-08-07 更新）集成路径已出现**：`crates/tui/Cargo.toml:20,40` 以 optional feature `vector-memory` 依赖本 crate（`crates/tui/src/vector_memory/mod.rs` 为集成点）。即由"僵尸上下文"演进为"可选能力子域"。
- [x] **（2026-08-07 复核）默认编译 + 运行时优雅降级**：`vector-memory` 已加入 tui `default` features，默认编译进二进制；仅当 `MIMOFAN_MEMORY_API_KEY` 配置才真正建立 embedding/向量库，否则 `enabled()==false`、零副作用。已从"默认关闭的可选子域"变为"默认编译、按需启用的互补能力层"。同时修正 `crates/memory/src/lib.rs` 顶部实验性警告中失真的"僵尸上下文/未集成"表述。
- [x] 决策维持：**仍标 experimental**（语义召回质量/成本/sled 上限待评估），但已可默认编译、按需启用，不再视为"未集成"。若后续评估决定不接入可整体移除本 crate；当前保留为文件记忆的互补语义召回层。

**预期效果**：不再误导——明确该 crate 已默认编译、经 `vector_memory` 模块集成主流程、作为文件记忆互补层、运行时优雅降级、仍 experimental。

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

---

## 8. Phase E：稳定性与并发风险（已核实，只写真实项）

> 背景：用户要求重点排查 内存泄漏 / 死锁 / 冲突并发 等稳定性隐患。
> 方法：逐文件核对所有 `Mutex` / `RwLock` / `spawn` / 长生命周期对象。结论——**当前无活死锁、无内存泄漏**，多数并发处理是规范的。详细报告见 `ARCHITECTURE_STABILITY.md`。

### 8.1 真正要修的（可扩展性风险）

- [x] **app-server 单锁串行化（已修复，2026-08-06）**：`crates/app-server/src/lib.rs` 原 `Arc<Mutex<Runtime>>` 改为 `Arc<RwLock<Runtime>>`。依据 `Runtime` 方法签名拆分锁粒度：
  - `&mut self` 方法（`handle_thread` / `handle_prompt`）→ **写锁**（`write().await`），仍独占；
  - `&self` 方法（`invoke_tool` / `app_status` / `mcp_startup`）→ **读锁**（`read().await`），**可并发执行**。
  - 效果：工具调用 `/tool`、任务状态 `/jobs`、MCP 启动 `/mcp/startup` 等高频只读路径不再被长耗时请求串行化，消除队头阻塞。改动仅限 `app-server` 这一底层 crate，TUI/CLI 用户交互层零变化。`cargo check --workspace` 通过。
  - 说明：未做"把 Runtime 内部拆成独立可并发子结构"的更深重构——那会侵入 `core` 组合根、风险高，且当前 RwLock 方案已解决真实瓶颈，符合奥卡姆剃刀。

### 8.2 已做对、不动的

- [x] **ToolCallRuntime**（`crates/tools/src/lib.rs:417`）：`tokio::sync::RwLock` + `OwnedRwLock*Guard` + `task_local` 重入保护，教科书级正确，保持现状。
- [x] **依赖纪律**：自研 LLM wire format、rusqlite bundled、reqwest+rustls、15 crate 严格 DAG——均有利稳定性，不变。

### 8.3 风格隐患（非活 Bug，可选美化）

> 经实施核查，这两项**保持 `std::sync::Mutex` 并加红线性注释**，而非盲目换成 `tokio::sync::Mutex`。原因：守卫在核实后**只在同步代码块内持有、绝不跨 `.await`**，`std` 锁在此场景是**正确且惯用**的选择；若强行换 `tokio::sync::Mutex`，会把 `.lock()` 变成 `.lock().await`，进而迫使 `engine_messages.rs` / `turn_loop.rs` / `goal.rs` 等 10+ 个**同步调用点**改签名、甚至把非 async 函数改成 async——这是高风险、无行为收益的改动，违背"只动底层、奥卡姆剃刀"。红线性注释已把脚枪风险锁死（见下）。

- [x] **goal.rs 的 `std::sync::Mutex`**（`crates/tui/src/tools/goal.rs:27/294`）：保留 `std` Mutex，在 `SharedGoalState` 类型别名与 `lock_goal_state` 处加注释，明确"守卫绝不跨 `.await`；若未来需长持锁整体换 `tokio::sync::Mutex` 并改 10+ 调用点"。当前安全。
- [x] **mcp_server/mod.rs 的 `std::sync::Mutex<HashMap>`**（`crates/tui/src/mcp_server/mod.rs:90/385/436`）：`handle_api_call` 是同步 `fn`，锁在同步段内、已做中毒恢复（`unwrap_or_else(|e| e.into_inner())`）。文件顶部 `threads` 字段已有红线性注释明确"守卫绝不跨 `.await`"。保留 `std` Mutex，当前安全。

### 8.4 内存增长防护（已做，不动）

- [x] spillover 文件 7 天清理（`truncate.rs:60`）、SQLite 索引、前缀缓存分区——均防无限增长。

### 8.5 memory 上下文（已默认编译 + 按需启用）

- [x] `crates/memory` 经 2026-08-07 复核，已通过 tui 的 `vector-memory` feature（**已加入 `default` features，默认编译**）接入主流程 `crates/tui/src/vector_memory/mod.rs`，作文件记忆的语义召回互补层，运行时按 `MIMOFAN_MEMORY_API_KEY` 优雅降级；已标 experimental。若评估不接，应整体删除（详见 Phase F 阶段 3，memory 可作为语义去重归宿复用）。

**Phase E 结论**：§8.1 的 app-server 并发粒度已修复（真实瓶颈消除）。§8.3 的锁风格隐患经核实为"当前安全 + 正确选型"，以红线性注释锁定脚枪，不强行替换为 tokio 锁（避免 10+ 调用点高风险改动）。**不夸大、不硬凑 TODO**。

---

## 9. Phase F：演进为百亿级 URL 分布式爬虫 + 开源情报平台

> 用户愿景：未来演进成"百亿级 URL 管理 + 开源情报监测"的分布式爬虫，支持多模态清洗/去重/标准化/结构化解析。
> 约束：只动底层，TUI/CLI/HTTP 用户交互层用法不变。完整分步路线见 `EVOLUTION_CRAWLER.md`。

### 9.1 复用底座（已具备，无需新建）

- [x] 工具框架 `ToolHandler`/`ToolRegistry`、并发门禁 `ToolCallRuntime`、任务管理 `JobManager`、会话 `ThreadManager`、策略 `ExecPolicyEngine`、模型路由、提示词分层体系、协议 DTO、HTTP 服务——均可直接承载"爬虫=工具""情报调研=thread"。

### 9.2 演进阶段（每步可独立交付）

- [x] **阶段 0 并发底座（部分完成，2026-08-06）**：app-server 单锁串行化已在 §8.1 修复。StateRepository 抽象明确推迟到阶段 4（见上）。
- [x] **阶段 1 单机爬虫工具（已具备，2026-08-06 核实）**：**无需新建 `crates/fetcher`**——仓库已有等价能力：`crates/tui/src/tools/fetch_url.rs`（`FetchUrlTool`，已注册于 `registry.rs:716`）与 `web_search.rs`（`WebSearchTool`）。二者均实现 `ToolSpec`（即本项目的工具端口），且已内建：
  - **合规/频率门禁**：`NetworkPolicyDecider` 网络策略（`fetch_url.rs:394` `validate_network_policy`），等价于规划里的 `ExecPolicyEngine` 网络版；
  - **SSRF 防护 + DNS pinning + 重定向上限 + 超时**（`fetch_url.rs:330` 起）；
  - **HTML→可读文本**轻量解析（`html_to_text`，`fetch_url.rs:501`，已覆盖阶段 2 的部分意图）。
  新建并行 crate 属**重复造轮子**，违背奥卡姆剃刀与"不写可有可无的待办"纪律。阶段 1 标记为已完成（复用现有）。
- [~] **阶段 2 网页解析结构化（部分具备）**：`fetch_url` 已做 HTML→text；但"强/弱模型路由做字段级结构化抽取（如抽取融资事件 JSON）"的**抽取算子**尚未作为独立模块存在，需新增 `crates/parser` 或在 `fetch_url` 上扩展。属演进项，非必须新建 crate。
- [ ] **阶段 3 多模态清洗去重标准化**：新增 `crates/multimodal` + `crates/dedup`；语义去重复用 `crates/memory`（让它从僵尸变有用）；精确去重用 sha2 哈希。**需立项**，不在本仓库一次落地范围。
- [ ] **阶段 4 百亿 URL 调度**：新增 `crates/crawl-scheduler`，URL 队列用 Kafka/NATS，按域名哈希分片，布隆过滤器去重 Frontier；`StateRepository` 换分布式实现。**依赖外部中间件，需立项**。
- [ ] **阶段 5 开源情报监测**：监测规则引擎 + 变化检测 + 复用 `crates/hooks` 告警。**依赖阶段 3–4，需立项**。
- [ ] **阶段 6 集群化**：节点无状态化 + 服务发现 + 分布式锁 + `tracing`/Prometheus。**依赖阶段 4，需立项**。

> 阶段 3–6 属**新建子系统 + 引入外部中间件（Kafka/ES/Redis/K8s 等）**的工程项目，依赖本仓库当前完全没有的基础设施，且会改动"用户交互层之外的底层基础设施"。按你"不忽悠"的要求：**阶段 1 已确认具备（复用现有工具）、阶段 2 部分具备；阶段 3–6 不假装一次干完**，按 `EVOLUTION_CRAWLER.md` 分步立项推进。强行新建空 crate 框架而不接真实中间件，是"凑待办"，已避免。

### 9.3 演进红线（来自稳定性文档 §7）

- [x] 集群**绝不**用"单 `Mutex<Runtime>`"模式 → 节点无状态、状态外置。
- [x] SQLite 仅做本地元数据 → 百亿 URL 必须分片 + 倒排索引。
- [x] 多进程下 `std::sync::Mutex` 无意义 → 全面转向消息通道 / 分布式锁。

**Phase F 结论**：阶段 0–1 低风险可立刻开工；阶段 4–6 需引入消息队列/分布式存储等新基建，应单独立项评估，不在本次强行铺开。不画大饼、不凑 TODO。

---

## 10. 文档索引（本次产出）

| 文档 | 内容 |
|------|------|
| `ARCHITECTURE_CN.md` | 当前架构说明（中文，分层图/依赖/提示词/入口/用例） |
| `ARCHITECTURE_IMPROVEMENT_PLAN.md` | 本文档：DDD 存量优化 + 稳定性(§8) + 演进(§9) |
| `ARCHITECTURE_STABILITY.md` | 稳定性/性能/可扩展性专项报告（核实版） |
| `EVOLUTION_CRAWLER.md` | 百亿级 URL 分布式爬虫 + OSINT 分步演进路线 |
| `USER_GUIDE_CN.md` | 用户使用说明（中文） |

---

## 11. 问题记录（历史 blocker，已闭环）

> 本节记录**与待办计划无关、但在 2026-08-06 实施核查中发现的仓库编译断裂问题**。按用户要求"有问题先记录"。现已核实已于后续合并解决，故标 `[x]`。

- [x] **[历史 BLOCKER，已解决] 仓库编译断裂（2026-08-06 发现，2026-08-07 核实已修复）**：原报告 `cargo check --workspace` 在 `crates/tui` 报 **15 处 E0609**，全部位于 `crates/tui/src/commands/groups/config/config.rs`，因直接访问 `SubagentsConfig` 已删除字段（`max_concurrent` / `launch_concurrency` / `api_timeout_secs` / `heartbeat_timeout_secs`）。
  - **当前核实（2026-08-07）**：`config.rs` 已改用间接 helper `subagents_config_display_value(&config, "max_concurrent")`（定义于 `config.rs:1103`）解析这些值，不再直接访问已删除的结构体字段；`SubagentsConfig`（`crates/tui/src/config.rs:178`）字段列表确已不含这些字段（并发/超时 knobs 已统一到 `LimitsConfig` / `config.limits.*`）。
  - **根因**：该断裂来自"把常量统一到 limits 模块"的重构，后续经 `refactor/compat-modes` 分支合并（`903610e`）及 limit 字段迁移补齐已解决。
  - **结论**：当前 `main` 分支工作区干净、`config.rs` 编译通过，该 blocker 已闭环，**无悬挂 `[ ]`**。不再阻塞任何 `cargo check` 验收。


> 注：本文档刻意**不列**任何"可有可无"的待办。若某子域当前已符合最佳实践，就标记完成或明确不做，而不是硬凑 TODO。
