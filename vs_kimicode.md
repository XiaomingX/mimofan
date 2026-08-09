# mimofan vs kimi-code 能力对标

> 对标对象：[MoonshotAI/kimi-code](https://github.com/MoonshotAI/kimi-code)（Kimi Code CLI）
> 本地快照：`/tmp/mimofan-bench/competitors/kimi-code`
> mimofan 侧代码落点均已用 Grep/Glob 实证核验。

---

## 0. 分析深度与局限说明

**仓库形态判断：kimi-code 是完整的 TypeScript 源码仓库，不是打包产物。** 这点先说清楚，因为它决定了本文结论的可信度。

- pnpm monorepo，Node ≥ 24.15.0，`apps/` + `packages/` + `plugins/` 三层。
- 排除 `node_modules`/`dist` 后约 **84 万行 TS/TSX**，其中约一半是测试（`packages/agent-core-v2` 单包 23.3 万行里 11.8 万行是 `*.test.ts`）。核心实现是可读源码，含大量设计意图注释。
- 主要包：`packages/agent-core`（16.9 万行，v1 统一引擎，**本文主要分析对象**）、`packages/agent-core-v2`（23.3 万行，DI × Scope 重写版，kap-server 后端）、`apps/kimi-code`（11.3 万行，CLI/TUI）、`packages/kosong`（LLM/Provider 抽象）、`packages/kaos`（执行环境抽象）。

**局限，请据此调整对结论的信任度：**

1. **唯一的真·打包产物是 Web UI**：`apps/kimi-code/dist-web` 是从另一个私有仓库 code-app 同步过来的预构建 bundle，源码不在此仓库（`AGENTS.md:18` 明确说明）。因此浏览器端 UI 的能力**无法分析**，本文不涉及。
2. **v1/v2 双引擎并存**：`agent-core`（v1）与 `agent-core-v2` 是同一套能力的两代实现，v2 是 DI/Scope 架构重写。本文能力条目主要取自 v1（更完整、更易读），v2 的架构差异未逐条展开。某个能力在 v1 存在但 v2 未必已移植，反之亦然。
3. **服务端/多端形态未展开**：`kap-server`（5.9 万行 REST+WS 服务）、`minidb`（3.9 万行嵌入式文档库）、`apps/vscode`、`apps/vis`、`apps/kimi-inspect` 属于 kimi-code 的「多端/服务化」布局，与 mimofan 当前的单机 TUI 形态不在同一个坐标系上，本文只在第 11 章做定性提及，不做逐条 checklist。
4. **未运行、未验证**：全部结论来自静态阅读源码，没有实际跑起来对比行为。
5. **已剔除死代码**：`agent-core/src/agent/compaction/micro.ts` 的 MicroCompaction 全文被注释禁用（`detect()`/`compact()` 直接 return，实验 flag 已从注册表移除）。**不计入 kimi-code 的能力**，本文特别标注以免误判。

---

## 1. 总览

| 能力域 | mimofan | kimi-code | 判断 |
| --- | --- | --- | --- |
| 文件/搜索/Shell 基础工具 | 完备 | 完备 | 持平 |
| 上下文压缩 | 三套范式并存（compaction/seam_manager/purge） | 单套 FullCompaction + 精细 handoff 选择规则 | **kimi 的单套更精 mimofan 的更多但分散** |
| 压缩交接质量（handoff） | 有摘要 + 字符级 head/tail | 一等公民：token 级 head/tail + 省略标记 + 一流的自述式 prompt | **kimi 领先** |
| 上游溢出恢复 | 无专门恢复链路 | overflow→compact→retry 有限次数闭环 | **kimi 领先** |
| 目标/预算（长程自治） | goal_loop + GoalBudget + 双 anti-drift 断路器 | goal + budget 工具 + 完成判据 | **mimofan 略领先** |
| 定时/周期任务 | scheduler 模块 | cron 全套工具（create/list/delete + 表达式 + jitter） | 大体持平，kimi 工具面更全 |
| 子智能体 | subagent 全套（mailbox/bus/decomposer/aggregator） | Agent + AgentSwarm（模板批量、最多 128、可 resume） | **各有所长** |
| 子智能体断点续跑（resume） | 无 | `resume` / `resume_agent_ids` | **kimi 领先** |
| 工具渐进披露 | tool_search（BM25 检索式） | select_tools（公告 diff + 精确加载，KV-cache 友好） | **机制不同，kimi 更省 cache** |
| 权限模型 | execpolicy 引擎（parser/matcher/rule） | 策略链 17 个 policy + 规则 DSL + 三态模式 | **各有所长，kimi 组合性更强** |
| 生命周期 Hook | 13 事件（用户可配） | 17 事件 | 接近，kimi 多 3 类 |
| 插件市场 | 无 | marketplace + GitHub 源 + 信任等级 | **kimi 领先** |
| MCP | mcp + mcp_server | 全传输 + OAuth + 对话式 `/mcp-config` | 接近，kimi 配置体验更好 |
| 多模态 | image_analyze（旁路视觉模型） | 原生图/视频入模（Kimi Files 上传） | **kimi 领先（视频）** |
| Kimi 模型特化 | 通用 Provider 抽象 | prompt_cache_key、能力注册表、Kimi 错误分类 | **kimi 领先（自家模型）** |
| ACP / IDE 集成 | acp_server | acp-server + acp-adapter + VSCode 扩展 | 接近 |
| 记忆系统 | crates/memory + remember + 向量检索 | 仅 AGENTS.md 静态注入，**无记忆工具** | **mimofan 明显领先** |
| 沙箱 | sandbox 模块 | 依赖 kaos 抽象 + 权限层 | mimofan 领先 |
| 代码智能 | lsp + symbol_index | 无 LSP；tree-sitter-bash 仅用于 bash 安全分析 | **mimofan 明显领先** |
| 检查点/回滚 | checkpoint/fork/revert_turn/snapshot | `/undo` + 压缩边界感知 | 接近，mimofan 粒度更细 |

**一句话结论**：mimofan 在**记忆、代码智能（LSP/符号索引）、沙箱、检查点粒度**上明显领先；kimi-code 在**压缩交接工艺、上游溢出恢复、子智能体 resume、插件生态、视频多模态、自家模型 KV-cache 优化**上领先。两者不是同一条演化路线——kimi-code 已在往「服务化 + 多端」走，mimofan 仍是深度单机 TUI。

---

## 2. 上下文与压缩

这是两者差异最有价值的一章。mimofan「范式更多」，kimi-code「单条链路更精」。

- [x] **自动压缩触发（阈值驱动）** — mimofan `crates/tui/src/compaction/mod.rs`；kimi 对应 `agent/compaction/strategy.ts` 的 `triggerRatio: 0.85`。
- [x] **压缩失败重试与退避** — mimofan `compaction/mod.rs`（搜 `Check if an error is transient`；区分 Network/RateLimit/Timeout 为可重试，其余不重试）；kimi `full.ts` `MAX_COMPACTION_RETRY_ATTEMPTS = 5`。
- [x] **PreCompact / PostCompact hook** — mimofan `crates/tui/src/hooks/mod.rs:60,66`，且 PreCompact 支持 exit code 2 否决压缩。
- [x] **多套压缩范式并存** — mimofan `compaction/`、`seam_manager/`、`purge/` 三套；kimi 只有 FullCompaction 一套（MicroCompaction 已禁用）。这项 mimofan 覆盖面更广，但也是已知技术债。
- [x] **压缩后保留部分原始用户消息 + 省略标记** — mimofan `compaction/mod.rs`（搜 `characters omitted before summary`）有 `{head}...[... N characters omitted before summary ...]...{tail}`。**注意：这条容易被误判为缺失，实际已实现**，只是粒度是「字符级、对单条文本切分」，而 kimi 是「token 级、跨消息池选择」。

### 2.1 待补齐

- [ ] **预留输出预算触发压缩（reservedContextSize）**
  - kimi 落点：`agent/compaction/strategy.ts:67-70` `shouldUseReservedContext()`，默认预留 50k tokens——不只看「用了多少百分比」，还看「剩下的够不够写完这次输出」。
  - mimofan 现状：已 grep `crates/tui/src/compaction/` 与 `context_budget/`，无等价的「为输出预留额度而提前压缩」判断。
  - 价值：长输出任务（写大文件、大 patch）最容易在「还没到 85% 但剩余额度不够写完」时被上游截断。这条是低成本、高收益的补丁。

- [ ] **上游 context-overflow 的闭环恢复**
  - kimi 落点：`full.ts` 捕获 `APIContextOverflowError` / `APIRequestTooLargeError`，触发 overflow→compact→retry，用 `maxOverflowCompactionAttempts: 3` 封顶，并用 `OVERFLOW_CONTEXT_SAFETY_RATIO = 0.85` / `OVERFLOW_STATUS_RECOVERY_RATIO = 0.5` 逐级收紧；还会用 `observedMaxContextTokensByModel` **记住模型真实窗口**（上游报错反推，纠正配置里写错的窗口值）。
  - mimofan 现状：grep `context_length_exceeded|ContextOverflow|context overflow` 在 `crates/tui/src/` 下**无匹配**（只有 snapshot 的「workspace too large」等无关命中）。确认缺失。
  - 价值：模型窗口配置写大了、或单轮塞进超大 tool result 时，mimofan 目前只能把错误抛给用户；kimi 能自愈。`observedMaxContextTokensByModel` 尤其巧妙，等于运行时自动校准配置。

- [ ] **token 级 head/tail 用户消息选择 + 结构化 origin 保留策略**
  - kimi 落点：`agent/compaction/handoff.ts`。两个点值得抄：
    1. `selectCompactionUserMessages()`：20k token 预算里给最老的用户消息硬留 2k（`COMPACT_USER_MESSAGE_HEAD_TOKENS`）——因为**最初那条消息通常是任务本体**，纯 tail 策略会把它整个丢掉。头部保留开头、尾部保留结尾，甚至能对同一条超长消息「留头 + 留尾」。
    2. `compactionUserMessageDisposition()`：按 `PromptOrigin.kind` 决定去留，用 `never` 穷尽检查强制新增 origin 类型时必须显式表态。injection/cron/hook_result/retry 等一律 drop（压缩后由 injector 重建），只有真实用户输入和 user-slash 技能激活 keep。
  - mimofan 现状：`compaction/mod.rs` 的 head/tail 是**对拼接后文本按字符切**，不是按消息池按 token 选；也没有等价的 origin 分类去留表。
  - 价值：第 1 点直接决定「压缩后 agent 还记不记得原始任务」；第 2 点避免注入内容被反复摘要、层层累积。

- [ ] **压缩摘要 prompt 的工艺**
  - kimi 落点：`agent/compaction/compaction-instruction.md`（约 60 行）。它不是「请总结上文」，而是要求模型写**第一人称、现在时的自我交接笔记**，并明确点名要保留：已解决的歧义、已定的决策 vs 未决的问题、**原样命令与文件路径及其成败**、**返回的具体值/错误文本/schema**（因为重跑代价高）、**还不知道什么**（引用过但没读的文件、假设过但没见过的 API）、以及「现在是投资 forward plan 的时刻，因为你此刻掌握的上下文是往后最多的一次」。还要求对「声称做完但没验证过」的事**如实标注为未验证**，并说明 TODO 列表会自动重附、不要转抄。
  - mimofan 现状：`compaction/mod.rs` 内的摘要指令相对常规。
  - 价值：**这是整份对标里迁移成本最低、收益最直接的一条**——纯 prompt 工程，不动架构。压缩后掉链子的主要原因就是摘要丢了关键事实或谎报进度。

---

## 3. 工具集

- [x] **文件读写/编辑/glob/grep** — mimofan `crates/tui/src/tools/`；kimi `tools/builtin/file/`。
- [x] **Shell 执行** — mimofan `tools/shell_tools.rs` + `execpolicy/`；kimi `tools/builtin/shell/bash.ts`。
- [x] **Web 抓取与搜索** — mimofan `tools/fetch_url.rs`、`tools/web_search.rs`；kimi `tools/builtin/web/`。
- [x] **TODO 列表** — mimofan `tools/todo.rs`；kimi `tools/builtin/state/todo-list.ts`。
- [x] **任务与依赖图** — mimofan `task_manager/`、`tools/tasks.rs`（`task_list` 等）。
- [x] **Plan mode 进出** — mimofan `tools/plan.rs:611` `EXIT_PLAN_MODE_NAME`，引擎级处理 + `tui/ui/plan_choice.rs` 交互。
- [x] **技能（Skills）** — mimofan `skills/` + `crates/tui/assets/skills/`；kimi `skill/` + `tools/builtin/collaboration/skill-tool.ts`。
- [x] **目标工具** — mimofan `tools/goal.rs`（`create_goal`/`get_goal`/`update_goal`，见 `tools/registry.rs:1034-1038`）。
- [x] **图像分析** — mimofan `vision/tools.rs` `image_analyze`。

### 3.1 待补齐

- [ ] **`ask_user` 交互提问工具**
  - kimi 落点：`tools/builtin/collaboration/ask-user.ts` + `ask-user.md`，让模型在需求含糊时**主动向用户发问**，并有配套权限策略 `auto-mode-ask-user-question-deny.ts`（自动模式下禁止提问，避免无人值守时卡死）。
  - mimofan 现状：grep `ask_user|AskUser` 在 `crates/tui/src/tools/` 下只命中 `shell_tools.rs:485` 的 `ExecPolicyDecision::AskUser`——那是**权限审批的决策枚举**，不是给模型调用的提问工具。确认缺失。
  - 价值：mimofan 有 goal_loop 长程自治，越自治越需要一个「不确定就问」的出口，否则模型只能猜着往下做。注意 kimi 那个「auto 模式下 deny 提问」的配套设计要一起抄。

- [ ] **后台任务工具三件套**
  - kimi 落点：`tools/background/` 的 `task-list.ts` / `task-output.ts` / `task-stop.ts`，模型可列出、读取、终止长跑后台进程。
  - mimofan 现状：`tools/tasks.rs:121` 的 `task_list` 是**任务管理（todo/依赖图）**语义，不是后台进程管理。两者同名不同物。
  - 价值：跑 dev server、长测试、watch 构建时，模型需要能轮询输出和喊停。

- [ ] **视频/媒体入模（read-media）**
  - kimi 落点：`tools/builtin/file/read-media.ts` + `packages/kosong/src/providers/kimi-files.ts`（走 Moonshot 文件服务上传，返回 `VideoURLPart`）。README 把「Video input」列为头号特性。
  - mimofan 现状：`commands/groups/utility/attachment.rs:58` 的 `/attach` **已识别** `mp4|mov|m4v|webm|avi|mkv` 为 `video`，`tui/file_mention.rs:776` 也提示可 attach video。但 grep `read_media|ReadMedia` 全仓库无匹配，且视觉能力是 `vision/tools.rs` 的**旁路 image_analyze**（把图片发给独立视觉模型拿文字描述），不是把媒体原生放进主模型上下文。
  - 价值：定位为「有入口、无原生链路」。是否补齐取决于目标模型是否支持视频；若主要对接 MiMo，应先确认模型侧能力再投入。

---

## 4. 工具渐进披露（KV cache 视角）

两边都在解「工具太多撑爆上下文」，但走了**两条不同的路**，值得单列一章。

- [x] **工具延迟加载 / 按需发现** — mimofan `core/engine/tool_catalog.rs`：BM25 检索式（`discover_tools_with_bm25_like:460`）+ regex 模式（`discover_tools_with_regex:420`），另有 `apply_native_tool_deferral:111`、`apply_mcp_tool_deferral:128`、`apply_tool_surface_budget:167`、`suggest_tool_names:561`（编辑距离纠错）。功能面其实比 kimi 更丰富。

### 4.1 待补齐

- [ ] **公告 diff + 精确名加载的披露协议（select_tools）**
  - kimi 落点：`tools/builtin/select-tools.ts` + `agent/context/dynamic-tools.ts`。关键设计不在「能不能按需加载」，而在**对 KV cache 的保护**：
    1. 顶层 `tools[]` **不可变**，动态 schema 从不写进去；`select_tools` 自身的 description 被要求**逐字节稳定**（源码注释明确：「it must stay byte-stable across the session. Anything that varies with the tool set (names, counts) belongs in the announcements, never here」）。
    2. 变化量以 `<tools_added>/<tools_removed>` **增量公告**追加在历史末尾，模型自己「折叠」出当前可用集——**只追加、不改写前缀**。
    3. 加载后的完整定义作为 `role: 'system'` 且带 `tools` 字段的消息追加。
    4. 两类消息用不同 `origin` 区分 undo 行为：schema 消息保留（属协议上下文），公告消息移除（下轮 diff 会自愈）。
    5. 压缩摘要输入时把这两类都剥掉（协议上下文不该被摘要）。
    6. `select_tools` 不声明 `accesses`，故与同批次所有工具串行——防止并发调用重复注入同一 schema。
  - mimofan 现状：`tool_catalog.rs` 的 `active_tools_for_step:312` 每步重算活跃工具集，是**重写请求前缀**的思路。grep `select_tools|tools_added|tools_removed` 全仓库无匹配，确认无此协议。
  - 价值：mimofan 的 BM25 检索在「找得准」上不输甚至更强，但**每次工具集变化都会让 KV cache 前缀失效**。kimi 的做法把工具集变化转化为纯追加，cache 命中率完全不同。这是长会话下的实打实成本差异，也是与第 9 章 `prompt_cache_key` 配套的一环。

---

## 5. Agent 循环、目标与长程自治

- [x] **目标对象 + 完成判据** — mimofan `tools/goal.rs`、`goal_loop/mod.rs`。
- [x] **预算限制（token / 时间 / 轮次）** — mimofan `goal_loop/mod.rs:75-88` `GoalBudget { token_budget, time_budget_seconds, max_continuations, ... }`。
- [x] **反漂移断路器** — mimofan `goal_loop/mod.rs:82-87`：`no_progress_rounds`（连续 N 轮无文件变更即停）、`repeated_error_rounds`（连续 N 次相同工具错误即停）。**kimi 侧未见等价机制，这是 mimofan 的净优势。**
- [x] **定时/周期任务** — mimofan `scheduler/mod.rs`；kimi `tools/cron/`（`cron-create/list/delete` + `cron-expr.ts` + `jitter.ts` + `clock.ts` + 持久化）。kimi 的工具面更完整（模型可自行增删定时任务），mimofan 的 scheduler 更偏内部调度，但**不算缺失**。
- [x] **子智能体体系** — mimofan `tools/subagent/`：`mailbox.rs`、`bus.rs`、`task_claim.rs`、`decomposer.rs`、`aggregator.rs`、`custom_agents.rs`、`persistence.rs`。消息总线 + 任务认领 + 分解聚合，这套比 kimi 的 Agent/AgentSwarm **更接近真正的多 agent 协作框架**。

### 5.1 待补齐

- [ ] **子智能体断点续跑（resume）**
  - kimi 落点：`tools/builtin/collaboration/agent.md` 明确「prefer resuming that agent (pass its `resume` id) over spawning a fresh instance — the resumed agent keeps its prior context」；`agent-swarm.ts` 的 `resume_agent_ids` 支持「恢复一批 + 新建一批」混合调用。子智能体固定 30 分钟超时，**超时后的标准动作就是 resume 而非重来**。
  - mimofan 现状：`tools/subagent/tool.rs:73` 的 action 枚举是 `start | status | peek | cancel`，grep `resume` 在整个 `tools/subagent/` 目录下**零匹配**。虽有 `persistence.rs` 持久化状态、`tool.rs:81` 支持查询 prior-session completed agents，但**没有把已完成/超时的子 agent 带着上下文重新拉起**的路径。确认缺失。
  - 价值：mimofan 有 decomposer/aggregator，跑的是大任务分解；子任务失败或超时只能整个重跑，前面烧掉的 token 全废。有 `persistence.rs` 打底，这条的实现成本应该不高。

- [ ] **模板化批量子智能体（AgentSwarm）**
  - kimi 落点：`tools/builtin/collaboration/agent-swarm.ts`。`prompt_template` 含 `{{item}}` 占位符 + `items` 数组，一次最多 128 个，自动排队。有**启动前的硬校验**：至少 2 个 item、模板必须含占位符、展开后的 prompt 必须互不相同（防止重复劳动），违反则在任何子 agent 启动前就拒绝。还规定「若调用 AgentSwarm，它必须是本轮唯一的工具调用」。
  - mimofan 现状：`commands/groups/core/swarm.rs` 的 `/swarm` 命令**存在但被显式 gate 掉**——源码注释：「Gate the old prompt-only swarm fanout until it can route through durable MimofanFlow/Fleet workers (#3218)」。所以是「已知待做」而非「没想到」。
  - 价值：mimofan 已有 subagent 基础设施，缺的是这层「同构任务批量扇出」的语法糖。kimi 那几条启动前校验规则很实用，值得连同抄走。

---

## 6. 权限与安全

- [x] **命令级执行策略引擎** — mimofan `execpolicy/`：`parser.rs`、`matcher.rs`、`rule.rs`、`rules.rs`、`policy.rs`、`decision.rs`、`amend.rs`、`execpolicycheck.rs`、`parser_ohos.rs`。**解析命令行结构后再判定**，比纯字符串 glob 匹配更难绕过。
- [x] **审批模式与交互审批** — mimofan `core/engine/approval.rs`、`tui/ui/approval.rs`、`tools/approval_cache.rs`。
- [x] **沙箱** — mimofan `sandbox/`。kimi 无独立沙箱模块，靠 `kaos` 执行抽象 + 权限层。
- [x] **Bash 命令安全分析** — mimofan execpolicy 自带解析器；kimi 用 `packages/tree-sitter-bash`（纯 TS，无 wasm，带 `timeoutMs`/`maxNodes` 确定性预算，解析失败必须降级为「无法分析」）。两者思路一致。
- [x] **敏感信息与路径访问策略** — mimofan execpolicy 规则体系；kimi `tools/policies/path-access.ts`、`sensitive.ts`。

### 6.1 待补齐

- [ ] **可组合的权限策略链**
  - kimi 落点：`agent/permission/policies/` 下 **17 个独立 policy** 依次求值，每个返回 `approve|deny|ask|undefined`（undefined = 不表态、交给下一个）：`deny-all`、`yolo-mode-approve`、`auto-mode-approve`、`plan-mode-guard-deny`、`plan-mode-tool-approve`、`exit-plan-mode-review-ask`、`goal-start-review-ask`、`file-access-ask`、`git-cwd-write-approve`、`user-configured-rules`、`session-approval-history`、`pre-tool-call-hook`、`agent-swarm-exclusive-deny`、`swarm-mode-agent-swarm-approve`、`auto-mode-ask-user-question-deny`、`default-tool-approve`、`fallback-ask`。
  - 配套的规则 DSL（`agent/permission/types.ts`）：`Read(/etc/**)`、`Bash(rm *)`、裸 `Write`；四级 scope（`turn-override` > `session-runtime` > `project` > `user`）；三态模式 `manual|yolo|auto`，且**deny 规则在任何模式下都生效**。`ask` 分支还能带 `resolveApproval`/`resolveError` 回调，把审批结果再喂回策略。
  - mimofan 现状：grep `allow_rules|deny_rules` 只命中 `tui/auto_review.rs:276` 和 `config.rs:516`——那是 **auto-review 的规则**，不是通用权限规则。execpolicy 在**单条命令的安全判定**上更强，但缺的是「多个正交策略按序组合 + 四级 scope 覆盖 + 会话内审批记忆」这层编排。
  - 价值：不是补能力，是补**结构**。mimofan 的模式判断目前散落各处（这与既有的「4 处阈值判断技术债」是同类问题）。策略链能把 plan mode 守卫、goal 启动审查、会话审批历史等统一成可注册单元。

- [ ] **权限相关 hook 事件（PermissionRequest / PermissionResult / Interrupt）**
  - kimi 落点：`session/hooks/types.ts` 的 `HOOK_EVENT_TYPES` 共 **17 个**。
  - mimofan 现状：`crates/tui/src/hooks/mod.rs:27-67` 有 **13 个**（SessionStart/SessionEnd/MessageSubmit/ToolCallBefore/ToolCallAfter/ModeChange/OnError/TurnEnd/SubagentSpawn/SubagentComplete/ShellEnv/PreCompact/PostCompact）。差集是 `PermissionRequest`、`PermissionResult`、`Interrupt`（kimi 的 `StopFailure`/`PostToolUseFailure` 在 mimofan 由 `OnError` 覆盖；mimofan 独有 `ShellEnv`、`ModeChange`，kimi 没有）。
  - 价值：审计场景需要「谁申请了什么权限、批了还是拒了」的完整轨迹。补齐成本低——事件枚举加三项 + 触发点接线。
  - **注意**：mimofan 有**两套 hook 系统**——`crates/hooks/`（EventFrame sink，6 变体，内部事件流）与 `crates/tui/src/hooks/mod.rs`（用户可配生命周期 hook，13 变体）。此处对标的是后者。只 grep 前者会得出错误结论。

---

## 7. 记忆与项目上下文

**本章 mimofan 是净赢家，且优势明显。**

- [x] **持久化记忆库** — mimofan `crates/memory/`、`tools/remember.rs`、`tools/remember_vector.rs`（向量检索）、`commands/groups/memory/`、`crates/tui/src/memory.rs`。
- [x] **项目规则文件注入** — mimofan 有 CLAUDE.md/CODEBUDDY.md 体系；kimi `profile/context.ts` 加载 AGENTS.md（用户级 → 项目级覆盖，`.kimi-code/AGENTS.md` 也支持）。
- [x] **符号索引** — mimofan `symbol_index/`。
- [x] **LSP 集成** — mimofan `lsp/`。**kimi-code 无 LSP**（其 tree-sitter-bash 仅服务于 bash 安全分析，非代码智能）。

**kimi-code 侧无对应能力**：grep `packages/agent-core/src/tools/builtin/` 下 `remember|Memory` **零匹配**，无任何记忆写入/检索工具。kimi 的「记忆」完全是静态 AGENTS.md 注入 + 会话持久化，没有跨会话的结构化记忆沉淀。

唯一值得借鉴的小点：

- [ ] **AGENTS.md 体积软预算与超限告警**
  - kimi 落点：`profile/context.ts:9,21,110` — 合并后超过推荐大小时产出 `AGENTS.md total N KB exceeds the recommended ...` 告警。
  - mimofan 现状：未见等价的规则文件体积告警。
  - 价值：小改进。项目规则文件无节制膨胀会静默吃掉每轮上下文，用户往往无感。

---

## 8. 扩展生态：MCP、技能、插件

- [x] **MCP 客户端（stdio/SSE/HTTP）** — mimofan `mcp/`；kimi `mcp/client-stdio.ts`、`client-sse.ts`、`client-http.ts`、`client-remote.ts`。
- [x] **MCP 服务端** — mimofan `mcp_server/`（把自己暴露为 MCP server）。
- [x] **MCP OAuth** — mimofan `mcp/`；kimi `mcp/oauth/`。
- [x] **技能系统** — mimofan `skills/` + `assets/skills/`（含 `plugin-creator` 技能）。
- [x] **脚本插件** — mimofan `commands/plugins.rs`（`/plugins` 列出与检视脚本插件工具）。
- [x] **ACP / IDE 集成** — mimofan `acp_server/`；kimi `packages/acp-server` + `packages/acp-adapter` + `apps/vscode`。

### 8.1 待补齐

- [ ] **插件市场与远程安装**
  - kimi 落点：`plugins/marketplace.json`、`agent-core/src/plugin/`（`manager.ts`、`manifest.ts`、`source.ts`、`github-resolver.ts`、`archive.ts`、`store.ts`、`commands.ts`）。可从市场或**任意 GitHub 仓库**安装技能/MCP server/数据源，安装前**显式展示信任等级**。
  - mimofan 现状：grep `marketplace|Marketplace` 全仓库只命中 `tui/ui/version_check.rs`（版本检查，无关）；`/plugins` 命令的注释是「list and inspect script plugin tools」，只有本地列举，无远程解析/下载/安装/信任提示。确认缺失。
  - 价值：生态问题。mimofan 已有 skills + 脚本插件的**运行时**，缺的是**分发层**。GitHub 直装 + 信任等级提示这套设计可直接参考。

- [ ] **对话式 MCP 配置（/mcp-config）**
  - kimi 落点：README 列为特性，「Add, edit, and authenticate MCP servers conversationally, without hand-editing JSON」；实现见 `mcp/config-loader.ts`、`global-config.ts`、`session-config.ts`、`auth-tool.ts`。
  - mimofan 现状：`tools/spec.rs:259` 走 `mcp.json` 文件配置，grep `mcp-config` 无匹配。属体验缺口而非能力缺口。
  - 价值：MCP 配置（尤其带 OAuth 的）手写 JSON 门槛高，是实际的上手阻力点。

---

## 9. 模型特化与 Provider 层

kimi-code 对自家 Kimi 模型做了专门优化，mimofan 对 MiMo 可做对称的事。

- [x] **多 Provider 抽象** — mimofan 原生支持 MiMo/DeepSeek 等；kimi `packages/kosong/src/providers/`（anthropic、google-genai、openai-responses、openai-legacy、kimi）。
- [x] **模型能力声明** — mimofan `model_routing/`、`context_budget/`；kimi `kosong/src/capability.ts` + `providers/capability-registry.ts`。
- [x] **Anthropic 风格 prompt cache 断点** — mimofan `crates/tui/tests/anthropic_test.rs:281-317` 验证了在最后一个 tool、最后一条 user message 末块、system 末块打 `cache_control`。这条**容易被误判为缺失**，实际已实现。

### 9.1 待补齐

- [ ] **显式 `prompt_cache_key`（OpenAI 兼容侧的缓存键）**
  - kimi 落点：`kosong/src/providers/kimi.ts:73` `prompt_cache_key?: string`——请求里带稳定缓存键，让服务端把同一会话的请求路由到同一缓存分片。
  - mimofan 现状：grep `prompt_cache_key` 全仓库**零匹配**（`prefix_cache/mod.rs` 是本地前缀缓存，与上游服务端缓存键不是一回事）。确认缺失。
  - 价值：与第 4 章的 select_tools 是一套组合拳——前缀稳定 + 缓存键稳定，长会话的 TTFT 和成本才真正下来。需确认 MiMo 服务端是否支持同类参数。

- [ ] **Provider 特有错误分类（配额/限流语义化）**
  - kimi 落点：`kosong/src/providers/kimi-errors.ts` 的 `classifyKimiQuotaError()`，把上游错误分类成配额/限流等可操作类别，配合 `loop/retry.ts` 的退避策略。
  - mimofan 现状：`llm_client` 有通用 `with_retry` 与 `sanitize_http_error_body`，`compaction/mod.rs` 也区分了 Network/RateLimit/Timeout，但未见 MiMo 特有的配额错误分类。
  - 价值：中等。区分「限流（该退避重试）」与「配额耗尽（重试无用，应立即告知用户）」能避免无谓等待。

- [ ] **副模型 / 小模型分工**
  - kimi 落点：`config/secondary-model.ts`、`config/env-model.ts`——摘要、分类等轻量任务可下放到更便宜的模型。
  - mimofan 现状：grep `secondary_model|small_model|fast_model` **零匹配**。有 `model_routing/` 与 `vision/` 独立视觉模型配置，但无「主模型 + 副模型」的通用分工。
  - 价值：压缩摘要是最典型的可下放任务，长会话里调用频繁且不需要顶配模型。

---

## 10. 会话、回滚与可观测性

- [x] **会话持久化与恢复** — mimofan `session_manager/`；kimi `session/` + `agent/records/`。
- [x] **检查点 / 分叉 / 回滚** — mimofan `checkpoint/`、`fork`、`tools/revert_turn.rs`、`snapshot/`（`repo.rs` 带工作区体积上限保护）。粒度比 kimi 的 `/undo` 更细。
- [x] **压缩感知的回滚边界** — kimi `undo.ts` 的 `UndoAvailability.stoppedAtCompaction` 会告诉用户「只能撤到压缩点为止」；mimofan 有 `revert_turn` + snapshot，语义相近。
- [x] **LLM 请求日志与回放** — mimofan 有 debug 命令组；kimi `agent/llm-request-logger.ts`、`llm-request-recorder.ts`、`agent/replay/`、`apps/vis`（可视化重放）。
- [x] **遥测** — mimofan 有；kimi `packages/telemetry`。

### 10.1 待补齐

- [ ] **会话重放的可视化工具**
  - kimi 落点：`apps/vis`（1.3 万行，session/replay 可视化调试）、`apps/kimi-inspect`（1.3 万行，workspace/session 浏览器 + transcript 聊天视图 + DI 单元检视）。
  - mimofan 现状：有 debug 命令组和结构化日志，但无独立可视化工具。
  - 价值：**优先级低**。这是团队规模化调试的工程投资，对当前 mimofan 的形态收益有限，列在此处仅为完整性。

---

## 11. 架构形态差异（不计入 checklist）

这一节不做「缺失」判断，因为**两者不在同一坐标系**，照搬是错的：

- **kimi-code 已服务化**：`kap-server`（REST + WebSocket + debug RPC 反射面）、`klient`（契约驱动客户端 SDK，zod 校验，ipc/memory 双传输）、`minidb`（嵌入式文档库 + WAL + 全文索引）、`transcript`（同构渲染数据层，L1~L4 分层 + turn-cursor 分页）。目标是一套引擎同时驱动 CLI / VSCode / Web / IDE(ACP)。
- **agent-core-v2 的 DI × Scope 架构**：四级 `LifecycleScope`（App / Workspace / Session / Agent）+ L3 单元层（Service/Fiber）+ 集合贡献点 + Feature 缝合层。
- **实验特性 flag 体系**：`KIMI_CODE_EXPERIMENTAL_<NAME>` 环境变量驱动、默认关闭、发布时翻转 default。v1 用中央注册表 `flags/registry.ts`，v2 用领域内 `registerFlagDefinition` 分散注册。

mimofan 是深度优化的**单机 TUI**，`crates/tui/` 占 21.7 万行/24 万行。这个选择本身没问题。但有两点可以**在不改变形态的前提下**借鉴：

1. **实验 flag 体系**：低成本，能让「三套压缩范式并存」这类技术债有秩序地收敛——新范式挂 flag 灰度，旧范式到期删除，而不是长期并存。
2. **契约层与渲染层解耦**：`packages/transcript` 是纯 TS、无引擎依赖、独占 transcript 契约类型。mimofan 若未来要做 Web/IDE 端，这层解耦是前置条件；即便不做，把渲染数据层从 TUI 里剥出来也有利于测试。

---

## 12. 建议优先补齐的 Top 8

按「收益 ÷ 成本」排序。前三条都是几天内可落地的。

| # | 事项 | 成本 | 收益 | 章节 |
| --- | --- | --- | --- | --- |
| 1 | **压缩摘要 prompt 重写为第一人称自我交接笔记** | 极低（纯 prompt） | 极高 | §2.1 |
| 2 | **预留输出预算触发压缩（reservedContextSize）** | 低 | 高 | §2.1 |
| 3 | **上游 context-overflow 闭环恢复 + 运行时校准模型窗口** | 中 | 高 | §2.1 |
| 4 | **子智能体 resume** | 中（persistence.rs 已打底） | 高 | §5.1 |
| 5 | **`ask_user` 提问工具（含 auto 模式禁用策略）** | 低 | 中高 | §3.1 |
| 6 | **token 级 head/tail 选择 + origin 去留表** | 中 | 中高 | §2.1 |
| 7 | **权限策略链重构（含 3 个权限 hook 事件）** | 中高 | 中高（顺带还阈值判断技术债） | §6.1 |
| 8 | **`prompt_cache_key` + select_tools 式披露协议** | 中高 | 中（取决于 MiMo 服务端支持） | §4.1 §9.1 |

### 逐条说明

1. **压缩摘要 prompt**：全文见 `agent/compaction/compaction-instruction.md`。三个最值钱的要求——保留**原样命令/路径/返回值**而非概述、**显式标注未验证的声称**、**在压缩点重投资 forward plan**（「你此刻掌握的上下文是往后最多的一次」）。改一个 prompt 文件即可。
2. **预留输出预算**：`shouldCompact()` 里加一条 `usedSize + reserved >= maxSize` 的或分支，默认预留 50k。直接消除「长输出写到一半被截断」这类失败。
3. **溢出恢复**：捕获上游 context-overflow 类错误 → 压缩 → 重试，3 次封顶；顺带把上游报错反推出的真实窗口记进 `observedMaxContextTokensByModel`，自动纠正配错的窗口值。
4. **子智能体 resume**：`tool.rs:73` 的 action 枚举加 `resume`，复用 `persistence.rs`。让 decomposer 分解出的子任务失败后不必整链重跑。
5. **`ask_user`**：给 goal_loop 长程自治配一个「不确定就问」的出口。务必连带抄 `auto-mode-ask-user-question-deny` —— 无人值守时必须禁止提问，否则会静默卡死。
6. **head/tail + origin 表**：把现有的字符级切分升级为消息池 token 级选择，给最老的用户消息硬留 2k 预算（保住原始任务），并用穷尽匹配的 origin 去留表防止注入内容被反复摘要。
7. **权限策略链**：把散落的模式判断收敛成可注册的正交 policy 序列 + 四级 scope。这条同时能缓解已知的「4 处阈值判断」技术债，属于结构性投资。
8. **cache 组合拳**：先确认 MiMo 服务端是否支持 `prompt_cache_key` 类参数；支持则连同 select_tools 式「只追加不改前缀」的披露协议一起做，长会话成本与 TTFT 会有实质改善。不支持则只做披露协议部分。

---

## 附：本文的验证方法

每条「缺失」结论都在 mimofan 仓库执行过 Grep/Glob 确认符号不存在。以下是**核验后推翻的初判**，记录下来供后续对标参考：

| 初判 | 核验结果 | 落点 |
| --- | --- | --- |
| 缺 goal/budget | **已有，且更强** | `goal_loop/mod.rs:75-88`，含双 anti-drift 断路器 |
| 缺定时任务 | **已有** | `scheduler/mod.rs` |
| 缺 plan mode | **已有** | `tools/plan.rs:611` |
| 缺压缩 head/tail 保留 | **已有**（字符级） | `compaction/mod.rs` 搜 `characters omitted before summary` |
| 缺 prompt cache | **已有**（Anthropic 风格断点） | `tests/anthropic_test.rs:281-317` |
| 缺视频支持 | **入口已有**（`/attach` 识别视频扩展名） | `commands/groups/utility/attachment.rs:58` |
| 缺工具延迟加载 | **已有**（BM25，功能面更广） | `core/engine/tool_catalog.rs:460` |
| 缺压缩重试 | **已有** | `compaction/mod.rs` 搜 `Check if an error is transient` |
| `/swarm` 缺失 | **已有但被显式 gate** | `commands/groups/core/swarm.rs`（#3218） |

反向核验（避免高估竞品）：kimi-code 的 MicroCompaction（`agent/compaction/micro.ts`）全文注释禁用、实验 flag 已从注册表移除，**不计入其能力**。
