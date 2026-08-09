# mimofan vs hermes-agent 能力对标

> 对标对象：[NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent)
> 本地快照：`/tmp/mimofan-bench/competitors/hermes-agent`
> 本文所有「缺失」结论均已用 Grep/Glob 在 mimofan 代码库验证符号确实不存在。

## 一、hermes-agent 定位判断

**结论：hermes-agent 不是 code agent，而是「个人级通用 AI 助理 + 模型训练数据工厂」的双形态项目。**

判断依据：

| 观察点 | 证据 |
| --- | --- |
| 自我定位 | README 首句："The self-improving AI agent built by Nous Research"，卖点是 learning loop 而非编码 |
| 主入口 | `hermes` 进入通用对话，而非 `cd <repo> && agent`；工作目录只是可选上下文 |
| 交互面 | Telegram / Discord / Slack / WhatsApp / Signal / Email 六个 IM 平台（`gateway/platforms`），CLI 只是其中之一 |
| 工具集偏向 | TTS、语音唤醒词（`tools/wakewords`）、图像/视频生成、Home Assistant 智能家居、Spotify、Google Meet、飞书文档 —— 生活助理属性远强于编码 |
| 研究向组件 | `batch_runner.py`（批量轨迹生成）、`trajectory_compressor.py`（训练数据压缩）、`toolset_distributions.py`（工具集采样分布）、`mini_swe_runner.py` —— 明确服务于「训练下一代 tool-calling 模型」 |
| 编码能力位置 | 编码只是众多 skill 分类之一（`skills/software-development`），且 `agent/coding_context.py`、`agent/verification_evidence.py` 是后加的补强 |

**实现形态**：Python，`cli.py` 单文件 86 万字符、`run_agent.py` 37 万字符、`hermes_state.py` 46 万字符——单体巨文件风格，与 mimofan 的 Rust 多 crate 分层架构差异极大。

**本次对标的适用范围**：

- ✅ **适用**：agent 循环范式、自我改进机制、记忆架构、上下文压缩策略、工具抽象、子智能体编排、可观测性、评测/数据生产。
- ⚠️ **部分适用**：多平台 IM 接入、云端执行后端 —— 理念可借鉴，但 mimofan 的终端定位决定了优先级低。
- ❌ **不适用**：TTS/语音唤醒、图像视频生成、智能家居、社交媒体工具 —— 属于生活助理形态，与 code agent 无关，本文不作为「缺失」记录。

---

## 总览

| # | 能力域 | 已具备 | 待补齐 | 一句话判断 |
| --- | --- | :---: | :---: | --- |
| 二 | 自我改进闭环 | 0 | 6 | **差距最集中处。** hermes 唯一真正原创的一层；mimofan 技能体系是纯消费型，模型无法生产/策展技能 |
| 三 | 记忆与上下文架构 | 3 | 5 | 存储层 mimofan 反超（向量/知识/压缩三范式齐全），**检索层落后**——跨会话只能按标题搜 |
| 四 | Agent 循环与停机判据 | 4 | 3 | 结构化程度 mimofan 反超（状态机/决策闸/目标循环），缺的是**回合结束前的强制校验闸门** |
| 五 | 工具抽象与子智能体 | 5 | 1 | 子智能体编排 mimofan 显著更强；唯一硬缺口是 PTC（脚本回调工具、零中间上下文） |
| 六 | 研究向（数据生产/评测） | 1 | 3 | 整体缺失，但**大部分与训练诉求绑定**；真正该补的只有端到端评测基准 |
| 七 | 执行环境与工程基建 | 5 | 3 | 沙箱/hook/MCP/定时/成本核算均已齐备；缺标准可观测性导出与跨会话用量分析 |
| **合计** | | **18** | **21** | |

> 注：`[x]` 与 `[ ]` 的数量比不代表能力差距比例——mimofan 有大量 hermes 完全没有的能力（`lsp/`、`execpolicy/`、`prefix_cache/`、`symbol_index/`、`fleet/`、`rlm/`、`slop_ledger/` 等）未纳入本表，因为本表只统计**hermes 侧存在的能力点**。

## 二、自我改进闭环（hermes 的核心差异化）

这是 hermes 最具原创性的一层，也是 mimofan 差距最集中的地方。

- [ ] **回合后台自评分叉（background review fork）** — 每个 turn 结束后 fork 一个受限 agent，用工具白名单（只允许 memory/skill 类工具）重放本轮对话，自问「这轮有什么值得沉淀成技能或记忆的？」，直接写入 memory/skill 存储，**完全不污染主会话与 prompt cache**。
  - hermes 落点：`agent/background_review.py`
  - mimofan 验证：`grep -r "background_review|BackgroundReview|self_improv"` → 无匹配。`crates/memory/` 有 injector/compressor，但注入是被动检索，没有「回合后自动反思并沉淀」的主动写入分叉。
  - 价值：把「用户显式让我记住」升级为「agent 自己判断该记什么」。对长期使用的 code agent，这是经验复利的关键。特别值得注意其工程细节——分叉继承父进程的 provider/model/凭证以复用同一 prefix cache，成本几乎只是 cache read。

- [ ] **技能生命周期与 Curator（自动策展）** — 技能有 `active / stale / archived / pinned` 四态，按 `stale_after_days`、`archive_after_days` 自动流转；空闲时触发 Curator 后台 agent，对 **agent 自建**技能做合并/归档/打补丁。严格不变量：只动 agent 自建技能、永不删除只归档、pinned 豁免。
  - hermes 落点：`agent/curator.py`、`tools/skill_usage.py`（sidecar `.usage.json` 遥测）、`tools/skill_provenance.py`（ContextVar 区分前台/后台写入来源）
  - mimofan 验证：`grep -r "curator|skill_usage|use_count|stale_after|archive_after"` → 无匹配。`crates/tui/src/skill_state/mod.rs` 只有 enable/disable 二态开关（TOML 里一个 `disabled` 数组），没有使用计数、时间戳、生命周期流转。
  - 价值：技能库会随使用膨胀并腐化。没有策展机制，技能索引最终会挤占 system prompt 预算且充斥失效内容。**provenance 隔离（用户写的技能永不被自动策展）是必须照抄的安全设计。**

- [ ] **agent 可写的技能管理工具（skill_manage）** — 模型能自己创建/编辑/归档技能，配合 `/learn` 把「一个目录、一篇 API 文档 URL、刚才那段对话」蒸馏成规范 SKILL.md。
  - hermes 落点：`tools/skills_tool.py`、`tools/skill_manager_tool.py`、`agent/learn_prompt.py`
  - mimofan 验证：`grep -r "skill_create|create_skill|skill_manage"` → 仅命中 `locales/zh-Hans.json` 文案。`crates/tui/src/tools/skill.rs` 只有 `load_skill`（只读加载）；`crates/tui/src/skills/install.rs` 只能从 GitHub/tarball 安装外部技能。**mimofan 的技能是纯消费型，模型无法生产技能。**
  - 价值：这是自我改进闭环的执行末端。没有它，前面的 review fork 即使判断出「该沉淀」也无处落笔。

- [ ] **技能自我改进（使用中修正）** — 技能在被使用时若发现描述过时或步骤有误，agent 可就地打补丁。
  - hermes 落点：`agent/skill_preprocessing.py`、`tools/skills_ast_audit.py`（技能内容静态审计）
  - mimofan 验证：同上，无技能写入路径。
  - 价值：技能从「写死的文档」变成「随环境演进的活资产」。

- [ ] **周期性记忆 nudge（提醒自己持久化知识）** — 每 N 个用户回合（默认 10，可配 `memory.nudge_interval`）向模型注入一次「回顾一下，本会话有什么值得写进记忆的」的提示。工程细节值得注意：nudge **只进模型看到的 message，不进 `original_user_message`**，所以转录、记忆查询与后续检索看到的都是干净的用户原话；计数器还会从历史会话补水（`prior_user_turns % interval`），会话恢复后不会从零重数。
  - hermes 落点：`agent/turn_context.py:597-605`（触发判定）、`agent/agent_init.py:1686`（默认值 10）、`agent/turn_context.py:549-557`（跨会话补水）
  - mimofan 验证：`grep -r "memory_nudge|turns_since_memory|periodic.*memory"` → 无匹配。mimofan 的 `crates/tui/src/turn_memory.rs` 是**正交**的另一条路——它用正则/关键词在本地挖掘信号并自动落盘（零 LLM、零网络），是「系统替模型记」；hermes 的 nudge 是「提醒模型自己记」。两者互补而非重复。
  - 价值：中高，且**实现成本极低**（一个回合计数器 + 一段注入文本）。mimofan 的 `turn_memory.rs` 启发式只能捞到符合正则的显式表述，捞不到「这个项目的构建必须先跑 codegen」这类需要理解才能提炼的知识——这正是 nudge 覆盖的盲区。

- [ ] **知识库型技能布局（大源码/大文档的渐进式披露）** — 当来源是一本书、一叠论文、一份大 spec 时，不塞进单个 SKILL.md 也不做有损摘要，而是产出「精简 SKILL.md 索引 + `references/` 分章文件」，索引常驻、章节按需用 `skill_view` 加载，**查询成本与答案规模成正比而非与来源规模成正比**。
  - hermes 落点：`agent/learn_prompt.py` 的 `_KNOWLEDGE_SKILL_STANDARDS`（约束）、`tools/skills_tool.py`（`references/`/`templates/`/`assets/` 目录约定）
  - mimofan 验证：`crates/tui/src/tools/skill.rs::collect_companion_files` **只列同目录平铺文件且显式跳过嵌套目录**（`is_file` 过滤），没有 `references/` 分层语义，也没有「按需加载某个章节」的二级入口——伴随文件只能靠模型自己 `read_file`。
  - 价值：中等。mimofan 已有 companion files 的雏形，补齐分层语义的增量成本不高。对「把一个大型内部框架文档变成技能」这类真实诉求，这是可用与不可用的分界。

---

## 三、记忆与上下文架构

- [x] **分层记忆存储（向量 / 知识 / 压缩）** — mimofan 已具备且更工整。
  - mimofan 落点：`crates/memory/src/{vector,knowledge,embedding,compressor,injector,optimization}.rs`、`crates/tui/src/tools/remember.rs`、`remember_vector.rs`、`crates/tui/src/vector_memory/`
  - hermes 对应：`agent/memory_manager.py`（provider 编排）+ `agent/memory_provider.py`。hermes 主记忆其实是朴素的 `MEMORY.md` / `USER.md` markdown 文件，向量能力靠外部 plugin。**此项 mimofan 反超。**

- [x] **上下文压缩** — mimofan 三范式并存，覆盖面强于 hermes 单一压缩器。
  - mimofan 落点：`crates/tui/src/compaction/`、`seam_manager/`、`purge/`；含 cache-aligned summary（`should_use_cache_aligned_summary`，`compaction/mod.rs:1287`）
  - hermes 对应：`agent/context_compressor.py`（头尾保护 + 中段摘要 + 迭代式摘要更新）

- [x] **可插拔上下文引擎抽象** — 双方都有，形态不同。
  - hermes：`agent/context_engine.py` 定义 ABC，配置 `context.engine` 选择实现，第三方可放进 `plugins/context_engine/`
  - mimofan：`crates/tui/src/compaction/` + `context_budget/` + `prompt_zones/` 各司其职，虽非插件式 ABC 但职责分离更清晰。**形态差异，不算缺失。**

- [ ] **微压缩（micro-compaction，压缩成本摊销）** — 不等阈值触发大压缩，而是每个 turn 结束后把**最老的一次未吸收交换**折进一个滚动摘要，把一次长停顿摊成每轮的小额支出。
  - hermes 落点：`docs/micro-compaction.md`、`compression.micro_compact` 配置、`finalize_turn` 调用点
  - mimofan 验证：`grep -r "micro_compact|MicroCompact|incremental_compact"` → 无匹配。mimofan 三范式都是**阈值触发的批量压缩**。
  - 价值：消除长会话中突兀的压缩停顿，上下文占用保持平稳而非锯齿状。
  - ⚠️ 但要照抄其文档里的诚实警告：微压缩**每轮都重写已发送历史，破坏 provider prompt-cache 前缀**，hermes 自己默认关闭。对重度依赖 `prefix_cache/` 的 mimofan，这个代价可能超过收益——**建议作为可选项而非默认**。

- [ ] **压缩模型可行性启动探针** — 启动时检查配置的辅助压缩模型上下文窗口能否容纳主模型的压缩阈值，装不下就自动下调会话阈值，低于最小值直接硬拒绝。
  - hermes 落点：`agent/conversation_compression.py::check_compression_model_feasibility`
  - mimofan 验证：`crates/tui/src/compaction/mod.rs` 有 `summary_input_limits_for_model`（限制输入长度），但没有「启动时校验 aux 模型可行性并回调阈值」的前置探针。
  - 价值：低成本的配置防呆。用户把压缩模型配成小窗口模型时，当前会在压缩时才失败，而非启动即告警。

- [ ] **跨会话内容检索（FTS5 + LLM 摘要）** — 对历史会话全文建 FTS5 索引，agent 可用 `session_search` 工具搜自己的过往对话，命中后用 LLM 摘要回注。
  - hermes 落点：`tools/session_search_tool.py`、`hermes_state_search.py`（10 万字符的检索层）
  - mimofan 验证：`crates/tui/src/session_manager/mod.rs:605` 的 `search_sessions` **只对 `s.title` 做 lowercase substring 匹配**，不检索消息内容；`grep -r "fts5|FTS5|cross_session"` → 无匹配。
  - 价值：「上次我们怎么解决这个问题的」是 code agent 高频诉求。当前 mimofan 只能按标题找会话，等于没有跨会话记忆检索。

- [ ] **记忆写入的「冻结快照」纪律** — 记忆文件被作为**会话启动时的冻结快照**注入 system prompt；会话中途的写入立刻落盘（持久性有保证），但**刻意不刷新 system prompt**，快照留到下个会话启动才更新。理由直白：中途改 system prompt 会让整个会话的 prefix cache 从改动点开始全部失效。工具返回值里回显最新状态，所以模型仍知道自己写成功了。
  - hermes 落点：`tools/memory_tool.py` 模块头「frozen snapshot pattern」、`MEMORY_BLOCK_HEADERS`（供压缩层检测残留空块）
  - mimofan 验证：`crates/tui/src/memory.rs` 的设计**已经部分收敛到同一答案，但走的是另一条路**——它让 `MEMORY.md` 索引保持极小（模块头明确写「It stays tiny, so prompt-prefix caching is unaffected」），分类文件根本不自动注入、由模型按需 Read。这规避了刷新问题，但也意味着**索引本身若在会话中途被 `remember` 刷新，仍会动到 prompt 前缀**；`compose_index_block` 在 `engine.rs:653` 与 `engine.rs:2660` 两处被调用，后者位于回合路径上。
  - 价值：中等，属**风险提示多于功能缺失**。mimofan 重度依赖 `prefix_cache/`，建议显式确认「会话中途 `remember` 写入是否会重建 system prompt」；若会，则照抄 hermes 的冻结快照语义（落盘即时、注入延后）是低成本的正确修法。

- [ ] **用户建模（USER.md / dialectic profile）** — 独立于任务记忆，持续积累「用户是谁、偏好什么」的画像，并作为 system prompt 的 volatile 层注入。
  - hermes 落点：`agent/system_prompt.py` 的 volatile 层、Honcho dialectic 集成
  - mimofan 验证：`grep -r "user_profile|UserProfile|USER\.md|dialectic"` → 仅命中一个测试文件名，无实现。
  - 价值：中等。mimofan 的 `crates/memory/` 可承载，但当前没有「用户画像」这一独立语义分类。

---

## 四、Agent 循环、推理与停机判据

- [x] **迭代预算 / 步数上限** — 双方都有。
  - hermes：`agent/iteration_budget.py`（父 500、子 agent 50，`execute_code` 的迭代会退还）
  - mimofan：`crates/tui/src/core/engine/turn_loop.rs`、`route_budget/`、`context_budget/`

- [x] **目标循环与状态机** — mimofan 更结构化。
  - mimofan 落点：`crates/tui/src/goal_loop/`、`state_machine/`、`decision_gate/`、`auto_reasoning/`
  - hermes 无对应的显式状态机，循环逻辑内嵌在 `run_agent.py` 巨文件中。**此项 mimofan 反超。**

- [x] **决策证据审计** — 双方都有，语义略有不同。
  - mimofan：`crates/tui/src/evidence/mod.rs`（`DecisionType`/`EvidenceOutcome`，面向决策漂移审计）
  - hermes：`agent/verification_evidence.py`（面向「编码时实际验证过什么」的命令结果账本）

- [x] **模型路由 / 大小模型切换** — 双方都有。
  - mimofan：`crates/tui/src/model_routing/mod.rs`（`RouterCandidates` big/cheap 配对）、`model_profile/`、`worker_profile/`
  - hermes：`agent/auxiliary_client.py`、`plugins/model-providers`

- [ ] **验证停机守卫（verify-on-stop nudge）** — 当模型在**编辑了代码之后**试图直接结束回合、且验证账本里没有新鲜证据时，注入一个有界的合成 nudge 让它先去验证，而不是放任「我改完了」草率收尾。且做了假阳性抑制：本轮只改了 `.md/.txt/.rst` 等非代码文件时不触发。
  - hermes 落点：`agent/verification_stop.py`、`agent/verification_evidence.py`、`agent/verify/{runner,recipes,environment}.py`
  - mimofan 验证：`grep -r "verify_on_stop|stop_guard|StopGuard|nudge"` → 仅命中 `tool_catalog.rs` 中无关内容。mimofan 有 `crates/tui/src/tools/verifier.rs`（`run_verifiers` 并行验证器合奏）和 `evidence/`，但**两者没有联动成「回合结束前的强制校验闸门」**——验证器要靠模型主动调用。
  - 价值：**这是对 code agent 最直接有用的一条。** mimofan 已有验证器和证据账本两块积木，只差把它们接成停机判据。改造成本低、收益明确。

- [ ] **协议违规兜底 nudge（kanban_stop 模式）** — 某些模型（GLM/Qwen 系）会「口头说下一步」然后以 `finish_reason=stop` 无工具调用地退出，被误判为正常完成。hermes 用策略层守卫检测这类协议违规并注入有界 nudge 续跑。
  - hermes 落点：`agent/kanban_stop.py`
  - mimofan 验证：同上无 nudge 机制。
  - 价值：mimofan 原生支持 MiMo/DeepSeek 等国产模型，**恰恰是这类协议遵循度问题的高发区**。这个模式的迁移价值被低估了。

- [ ] **Mixture-of-Agents（多模型参考聚合）** — `/moa` 把一个 turn 标记为 MoA 模式，每次模型迭代前先并行收集多个 reference model 的意见，聚合后再让主模型决策。附带 PII/密钥隐私过滤（advisor 输出可能回显对话中的敏感信息）。
  - hermes 落点：`agent/moa_loop.py`、`agent/moa_trace.py`
  - mimofan 验证：`grep -r "mixture_of_agents|MixtureOfAgents|moa|MoA"` → 无匹配。
  - 价值：中等偏低。对编码任务，多模型投票的收益不如「并行验证器」（mimofan 已有 `run_verifiers`）确定。但**其隐私过滤设计值得注意**——mimofan 若未来做多模型聚合需同步考虑。

---

## 五、工具抽象与子智能体

- [x] **工具延迟加载 / 渐进式披露** — 双方都有，mimofan 用 BM25，hermes 用分层预算。
  - mimofan 落点：`crates/tui/src/core/engine/tool_catalog.rs`（`tool_search` + BM25 语料检索 + `defer_loading` 标记）
  - hermes 落点：`tools/tool_search.py`（`tool_search`/`tool_describe`/`tool_call` 三桥接工具）
  - ⚠️ 但 hermes 的**分层降级策略**更细：Tier 0 全量直通 → Tier 1 名称+简述清单 → Tier 2（如 Cloudflare 3300 个工具、光名字就 32K token）退化为「每 server 一行摘要」。mimofan 当前是二元的 defer/不 defer，超大 MCP 目录场景下的清单本身就会爆预算。**这是一个值得补的边界情况。**

- [x] **子智能体编排** — mimofan 显著更强。
  - mimofan 落点：`crates/tui/src/tools/subagent/{mailbox,bus,task_claim,decomposer,aggregator,custom_agents}.rs`
  - hermes 落点：`tools/delegate_tool.py`、`tools/async_delegation.py`、`agent/subagent_lifecycle.py`、`agent/delegation_context.py`
  - hermes 只有单层 delegate + 独立迭代预算；mimofan 有 mailbox 通信、任务认领、分解器、聚合器。**此项 mimofan 反超。**

- [x] **任务管理与依赖图** — mimofan 已具备。
  - mimofan 落点：`crates/tui/src/task_manager/`、`tools/tasks.rs`、`tools/todo.rs`
  - hermes 对应：`tools/todo_tool.py` + `tools/kanban_tools.py`（看板式，带 dispatcher 派发 worker）

- [x] **交互式澄清提问** — 双方都有。
  - mimofan 落点：`crates/tui/src/tools/user_input.rs`（`UserInputQuestion`，含 `allow_free_text`、`multi_select`）
  - hermes 落点：`tools/clarify_tool.py`

- [x] **工作区快照与回滚** — mimofan 已具备且设计相当。
  - mimofan 落点：`crates/tui/src/snapshot/{mod,repo,prune,paths}.rs`（旁路 git 仓、pre/post-turn 快照、7 天保留）、`tools/revert_turn.rs`、`/restore`
  - hermes 落点：`tools/checkpoint_manager.py`（单一共享 shadow git store，跨项目对象去重）
  - ⚠️ 细节差异：hermes 用**单一共享 store** 让 git 对象在多个 worktree 间去重（其文档称多 worktree 场景可省 ~500MB）；mimofan 是 `<project_hash>/<worktree_hash>` 分仓。考虑到本项目 CODEBUDDY.md 明确鼓励 worktree 工作流，**这个存储优化对 mimofan 实际适用**。

- [ ] **编程式工具调用（PTC / execute_code）** — 让模型写一段 Python，脚本内通过 RPC 回调宿主的工具，把多步工具链塌缩成**单次推理回合**。关键收益：中间工具结果**从不进入上下文窗口**，只有脚本 stdout 回传。本地走 Unix domain socket，远程后端走文件式 RPC。
  - hermes 落点：`tools/code_execution_tool.py`
  - mimofan 验证：`grep -r "programmatic_tool|code_execution|execute_code|tool_rpc"` → 命中的是 `crates/tui/src/tools/js_execution.rs`（独立 JS 沙箱执行，**不能回调 mimofan 工具**）和 `tool_execution.rs`（常规分发器）。**PTC 语义确实缺失。**
  - 价值：**高。** 对「遍历 200 个文件各做一次检查」这类任务，常规工具循环要 200 个回合且每个结果都吃上下文；PTC 是 1 个回合、零中间上下文成本。mimofan 已有 `tools/parallel.rs` 和 js 沙箱两块基础，补齐 RPC 回调即可。
  - ⚠️ 若要实现，务必照抄其**收敛的能力面**：hermes 的 `SANDBOX_ALLOWED_TOOLS` 只放行 7 个工具（`web_search`/`web_extract`/`read_file`/`write_file`/`search_files`/`patch`/`terminal`），并与会话实际启用的工具取交集后才生成 stub；再叠加 `DEFAULT_MAX_TOOL_CALLS=50`、`MAX_STDOUT_BYTES=50_000` 等资源闸。一个能回调宿主全部工具的沙箱脚本，等于把工具审批链整个绕过去了。

---

## 六、研究向能力（数据生产与评测）

这是 hermes 作为 Nous Research 出品的特色层，mimofan 整体缺失，但**适用性需要分辨**。

- [ ] **批量轨迹生成（batch_runner）** — 从数据集并行跑 agent，多进程、带断点续跑、聚合工具使用统计，输出 ShareGPT 格式轨迹。
  - hermes 落点：`batch_runner.py`、`agent/trajectory.py`
  - mimofan 验证：`grep -r "trajectory|ShareGPT|batch_runner"` → 无匹配。
  - 价值：取决于 mimofan 是否有训练自有模型的诉求。**若无，则形态不适用**；若有（考虑到项目原生对标 Xiaomi MiMo），这是从产品反哺模型的通路。

- [ ] **轨迹压缩（面向训练信号保真）** — 与运行时上下文压缩**目标不同**：这是离线后处理，为把轨迹压进目标 token 预算同时保留训练信号——保护首轮与末 N 轮，只压中段，压到刚好达标为止。
  - hermes 落点：`trajectory_compressor.py`
  - mimofan 验证：同上无匹配。
  - 价值：同上，与训练诉求绑定。**注意不要与 mimofan 已有的运行时压缩混为一谈，二者优化目标正交。**

- [ ] **工具集采样分布（toolset distributions）** — 数据生成时按概率分布采样启用哪些工具集，产出工具使用多样性可控的轨迹。
  - hermes 落点：`toolset_distributions.py`
  - 价值：纯训练数据工程，**形态不适用**于 mimofan 当前定位。

- [x] **离线评测框架** — mimofan 有，但覆盖面窄。
  - mimofan 落点：`crates/tui/src/eval/mod.rs`（自包含、不联网的工具循环场景harness，覆盖 List/Read/Search/Edit/ApplyPatch/ExecShell）
  - hermes 落点：`mini_swe_runner.py`（SWE 任务 runner，可跑 Docker/Modal）
  - ⚠️ 差异：mimofan 的 eval **刻意不调 LLM**，只验工具循环机械正确性；hermes 是端到端跑真实模型解 SWE 任务。**mimofan 缺的是端到端能力回归基准**，这在持续改 agent 逻辑时是重要的防退化网。

---

## 七、执行环境、可观测性与工程基建

- [x] **沙箱与执行策略** — mimofan 已具备。
  - mimofan 落点：`crates/tui/src/sandbox/{seatbelt,opensandbox,policy,process_hardening}.rs`、`crates/execpolicy/`、`crates/tui/src/command_safety/`、`network_policy/`、`workspace_trust/`
  - hermes 落点：`tools/path_security.py`、`tools/approval.py`、`tools/threat_patterns.py`、`tools/tirith_security.py`

- [x] **生命周期 hook** — mimofan 有两套，覆盖更全。
  - mimofan 落点：`crates/tui/src/hooks/mod.rs`（13 变体，含 PreCompact/PostCompact）+ `crates/hooks/`（EventFrame sink）
  - hermes 落点：`gateway/builtin_hooks`、`agent/shell_hooks.py`、`agent/verify_hooks.py`

- [x] **MCP 集成** — 双方都有。
  - mimofan：`crates/mcp/`、`crates/tui/src/mcp/`、`mcp_server/`、`mcp_server_backend/`
  - hermes：`tools/mcp_tool.py`、`mcp_serve.py`、含 OAuth 管理与 schema 缓存

- [x] **定时自动化** — mimofan 已具备。
  - mimofan 落点：`crates/tui/src/automation_manager/mod.rs`（持久化定期任务，落 `~/.mimofan/automations`）、`scheduler/`、`commands/groups/core/schedule.rs`、`tools/automation.rs`
  - hermes 落点：`cron/`（含 `blueprint_catalog`、`suggestions` 主动建议）
  - ⚠️ hermes 多一层 `cron/suggestions.py`——**主动向用户建议该设哪些定时任务**。小但有趣的产品化点。

- [x] **成本与 token 核算** — mimofan 已具备。
  - mimofan 落点：`crates/tui/src/cost_status/mod.rs`（含后台 LLM 调用的成本侧信道，压缩/seam 重压缩的 token 不再漏计）、`pricing/`
  - hermes 落点：`agent/usage_pricing.py`、`agent/billing_usage.py`

- [ ] **会话洞察报告（/insights）** — 对历史会话库做聚合分析：token 消耗、成本估算、工具使用模式、活跃趋势、模型/平台分布。
  - hermes 落点：`agent/insights.py`（其注释直言灵感来自 Claude Code 的 `/insights`）
  - mimofan 验证：`grep -r "insights|cost_report|usage_report|token_analytics"` → 无匹配。mimofan 有实时 `cost_status` 但**没有跨会话的历史聚合分析**。
  - 价值：中等。对个人用户是「用量自省」，对项目是发现工具设计问题的数据来源（哪些工具从没被调用过、哪些工具高频失败）。

- [ ] **标准可观测性导出（OTLP / Langfuse）** — 把 agent 轨迹导出到标准可观测性后端。
  - hermes 落点：`agent/monitoring/otlp_exporter.py`、`agent/monitoring/{emitter,events,policy,redaction}.py`、`plugins/observability/{langfuse,nemo_relay}`
  - mimofan 验证：`grep -r "otlp|OTLP|opentelemetry|langfuse"` → 无匹配。mimofan 有 `logging/`、`runtime_log/`、`audit/`、`resource_telemetry/`，但都是自有格式，**无标准协议导出**。
  - 价值：中等。对个人 CLI 工具优先级不高；若 mimofan 面向团队/企业场景，这是接入现有 LLM 可观测性栈的门槛。注意其 `monitoring/redaction.py`——导出前的敏感信息脱敏是必需品而非可选项。

- [ ] **多云执行后端（serverless 持久化）** — 七种终端后端：local / Docker / SSH / Singularity / Modal / Daytona / Vercel Sandbox。其中 Daytona 与 Modal 提供**空闲休眠、按需唤醒**的 serverless 持久化环境。
  - hermes 落点：`tools/environments/{local,docker,ssh,singularity,modal,managed_modal,daytona,vercel_sandbox}.py`、`tools/environments/file_sync.py`
  - mimofan 验证：`grep -r "Daytona|daytona|Singularity|vercel_sandbox"` → 无匹配（"Modal" 的命中全部是 TUI 弹窗组件，非云后端）。mimofan 有 `crates/tui/src/remote_setup/`，但其模块头注释明确写着「Generate-only MVP，`--apply` 云端自动置备路径是 stub，从不执行任何东西」——**只生成部署包，不是可用的远程执行后端**。
  - 价值：偏低。与 mimofan「终端 code agent」定位有张力。但 `tools/environments/base.py` 的**统一环境抽象接口**值得借鉴——mimofan 若未来要支持容器内执行，有个干净的 Environment trait 会省很多事。

---

## 八、形态不适用但理念可借鉴

以下能力在 hermes 中存在，但因 mimofan 是终端 code agent 而非通用生活助理，**不应直接照搬**。仅记录其中的可迁移理念。

| hermes 能力 | 落点 | 为何形态不适用 | 可借鉴的理念 |
| --- | --- | --- | --- |
| 六平台 IM 网关 | `gateway/platforms`、`gateway/run.py` | mimofan 是终端工具，不需要 Telegram/WhatsApp 接入 | **会话与 UI 解耦**。hermes 的 agent 核心对「消息从哪来」无感知，靠 gateway 层适配。mimofan 已有 `acp_server/`、`runtime_api/`、`app-server`，方向一致，可参考其 `session_context`/`turn_lease` 的多入口并发仲裁设计 |
| TTS / 语音唤醒 / 语音模式 | `tools/tts_tool.py`、`tools/wake_word.py`、`tools/voice_mode.py` | 与编码工作流无关 | 无 |
| 图像/视频生成 | `tools/image_generation_tool.py`、`tools/video_generation_tool.py` | 非编码能力 | 无（mimofan 的 `vision/` 做图像**理解**，方向正确） |
| 智能家居 / Spotify / 社交媒体 | `tools/homeassistant_tool.py`、`plugins/spotify` | 生活助理属性 | 无 |
| Kanban 看板 + dispatcher 派发 | `tools/kanban_tools.py`、`docs/kanban/multi-gateway.md` | 依赖长驻 gateway 进程与多 profile 部署 | **单一 dispatcher 所有权 + 原子事件认领**防止多进程重复派发。mimofan 的 `fleet/` 与 `subagent/task_claim.rs` 若走向多进程，会遇到同样的竞争问题 |
| 桌面 GUI / Web 仪表盘 | `web/`、`apps/`、`website/` | 超出终端定位 | `agent/learning_graph.py` 的**「学习可视化」**理念有意思：把习得的技能与记忆渲染成关系图，让用户看见 agent 在如何成长。即便在 TUI 里也能做简化版 |
| Nous Portal 一站式订阅 | `agent/portal_tags.py`、`agent/subscription_view.py` | 商业模式绑定 | 无 |

---

## 九、建议优先补齐的 Top 6

按「对 code agent 的实际价值 ÷ 实现成本」排序。前三项共同构成一个自洽的闭环，建议成组推进。

### 1. 验证停机守卫（verify-on-stop）
**价值最高、成本最低。** mimofan 已有 `tools/verifier.rs`（并行验证器合奏）与 `evidence/mod.rs`（决策证据），缺的只是把两者接成回合结束前的闸门：模型改了代码却没有新鲜验证证据就想收尾 → 注入有界 nudge 让它先验证。
必须照抄 hermes 的假阳性抑制（`_NON_CODE_VERIFY_EXTENSIONS`）：本轮只改 `.md`/`.txt` 时不触发，否则改个 README 都被要求跑测试会非常恼人。
参考：`agent/verification_stop.py`、`agent/verification_evidence.py`

### 2. agent 可写的技能管理工具（skill_manage）
mimofan 的技能体系目前是**纯消费型**——`load_skill` 只读、`skills/install.rs` 只能装外部技能，模型无法生产技能。这是自我改进闭环的必要前置：不补这一条，第 3 项就无处落笔。
需同步引入 hermes 的 **provenance 隔离**（`tools/skill_provenance.py`）：用 ContextVar 标记写入来源，区分「用户让我写的技能」与「agent 自己沉淀的技能」，后者才允许被自动策展。
参考：`tools/skills_tool.py`、`agent/learn_prompt.py`

### 3. 回合后台自评分叉 + 技能生命周期策展
在 2 的基础上，每个 turn 后 fork 受限 agent（工具白名单只含 memory/skill）反思本轮是否值得沉淀；配合 `active/stale/archived/pinned` 四态与使用计数遥测，让技能库自动新陈代谢。
两条硬不变量必须保留：**永不自动删除（只归档，可恢复）**、**pinned 豁免一切自动流转**。
成本控制关键：分叉继承父进程 provider/model/凭证以复用同一 prefix cache。
参考：`agent/background_review.py`、`agent/curator.py`、`tools/skill_usage.py`

### 4. 编程式工具调用（PTC）
让模型写脚本、脚本通过 RPC 回调 mimofan 工具，中间结果不进上下文。对批量文件操作是数量级的上下文节省。
mimofan 已有 `tools/js_execution.rs`（沙箱）与 `tools/parallel.rs` 两块基础，主要工作是补 RPC 回调通道与 stub 生成。
参考：`tools/code_execution_tool.py`

### 5. 跨会话内容检索
当前 `session_manager/mod.rs:605` 的 `search_sessions` 只匹配标题，等于没有跨会话记忆。补全文索引（Rust 侧可用 tantivy，或直接 SQLite FTS5）并暴露为 agent 可调工具。
「上次这个 bug 我们怎么修的」是 code agent 的高频真实诉求，而 mimofan 已有的 `crates/memory/` 向量能力可以直接复用作语义召回层。

### 6. 协议违规兜底 nudge
检测「模型口头说下一步却以 `finish_reason=stop` 无工具调用退出」并注入有界 nudge 续跑。
**这条对 mimofan 的相关性高于 hermes 自身**——mimofan 原生支持 MiMo/DeepSeek 等国产模型，正是这类工具调用协议遵循度问题的高发区。实现极轻量（策略层判断 + 有界重试计数）。
参考：`agent/kanban_stop.py`

---

### 次优先（记录备查）

- **超大 MCP 目录的分层降级**（`tools/tool_search.py` Tier 0/1/2）：mimofan 的 defer 是二元的，接入 3000+ 工具的 MCP server 时清单本身会爆预算。
- **微压缩**（`docs/micro-compaction.md`）：消除压缩停顿，但破坏 prefix cache，**务必做成默认关闭的可选项**——mimofan 有 `prefix_cache/`，代价可能超过收益。
- **快照共享 store**（`tools/checkpoint_manager.py`）：单一 git store 跨 worktree 去重。本项目工作流明确鼓励 worktree，实际适用。
- **压缩模型可行性启动探针**（`agent/conversation_compression.py`）：低成本配置防呆。
- **端到端评测基准**：mimofan 的 `eval/mod.rs` 刻意不调 LLM，缺真实模型解题的防退化网。
- **周期性记忆 nudge**（`agent/turn_context.py:597`）：一个回合计数器 + 一段注入文本，成本近乎为零，覆盖 `turn_memory.rs` 正则启发式捞不到的「需要理解才能提炼」的知识。**性价比其实高于本节其他条目，可考虑提进 Top N。**
- **知识库型技能布局**（`agent/learn_prompt.py`）：`references/` 分章 + 按需加载。mimofan 的 `collect_companion_files` 显式跳过嵌套目录，补齐分层语义成本不高。
- **记忆注入的 prefix-cache 纪律**（`tools/memory_tool.py` frozen snapshot）：需先确认 mimofan 会话中途 `remember` 是否会重建 system prompt；若会，则属待修风险而非新功能。

---

*文档生成于 2026-08-08。所有 mimofan「缺失」判定均经 Grep 验证；标注 `[x]` 的条目均附实际代码落点。*

