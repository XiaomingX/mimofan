# mimofan vs OpenClaw 能力对标

> 对标日期：2026-08-08
> mimofan：Rust 终端 AI 编码助手，约 24 万行，主体在 `crates/tui/`
> OpenClaw：TypeScript 个人 AI 助理（`/tmp/mimofan-bench/competitors/openclaw`），pnpm monorepo，含 164 个 extensions、22 个 packages、移动/桌面 companion apps

## 0. 先说定位差异（决定了哪些"缺失"值得补）

两者**不是同类产品**，这是读本文档时最重要的前提：

- **mimofan** 是**终端编码 agent**：单人、单机、工作区内、以代码任务为中心。
- **OpenClaw** 是**跨渠道个人助理网关**：以 Gateway 为控制面，把 assistant 接到 WhatsApp / Telegram / Slack / Discord / Signal / iMessage 等聊天渠道，外加 iOS/Android/macOS/Linux companion apps、Canvas、语音、摄像头、屏幕控制。编码只是它的一个使用面（且大量依赖 Codex / ACPX 外部 harness 代劳）。

因此 OpenClaw 有大量能力（channels、pairing、meeting-bot、realtime-transcription、music/video generation、nodes、Control UI）对 mimofan **不构成真实差距**——那是另一个产品品类。本文档对这类能力标注为「非目标域」，不计入建议补齐项。

真正值得对标的是两者重叠的部分：**agent runtime、工具体系、上下文工程、扩展性、安全模型、可观测性**。

---

## 1. 总览表

| 能力域 | OpenClaw | mimofan | 差距结论 |
| --- | --- | --- | --- |
| 工具体系规模 | 内置分类 12 类 + 164 extensions + 插件 SDK + MCP | 56+ 工具文件 + 插件脚本 + MCP 双向 | **基本持平**，mimofan 编码类工具更深，OpenClaw 泛能力更广 |
| 工具延迟加载 | Tool Search（bounded directory + describe + call） | `tool_catalog.rs` BM25/regex + 中途激活 | **持平** |
| Code Mode（沙箱内编程式调工具） | QuickJS-WASI worker，guest 侧 `tools.callValue` 编排隐藏目录 | 无（有 `js_execution` 但是裸 Node 跑片段，非工具编排桥） | **缺失，高价值** |
| 上下文压缩 | 单一 compaction + 溢出重试 | 三范式：`compaction/`+`seam_manager/`+`purge/` | **mimofan 更强** |
| Token 计量 | 真实 tokenizer | 字符启发式（3 chars/token） | **缺失，已知** |
| 记忆 | `memory/root-memory-files.ts`（文件级）+ active-memory 插件 | `crates/memory/` 全套 + 向量记忆默认开 | **mimofan 显著更强** |
| 自学习（经验→技能） | Self-learning + Skill Workshop（模型评审、提案、扫描、生命周期） | 只有技能发现/加载，无自动产出 | **缺失，高价值** |
| 循环检测 | 全局滚动 `(tool,args,result)` 三元组检测 + 压缩后守卫 | 有 goal-loop 断路器（回合粒度） | **部分缺失** |
| 可观测性 | Trajectory 飞行记录仪 + OTel/Prometheus extensions | `audit/` 单函数 + Markdown transcript 导出 | **缺失，高价值** |
| 提示注入防护 | `security/external-content.ts` 边界包裹 + 模式检测 | 无 | **缺失，高价值** |
| 沙箱/权限 | exec 5 档模式 + Docker/沙箱 + 审计 | seatbelt/opensandbox/policy/process_hardening + execpolicy + network_policy | **mimofan 更强（本机维度）** |
| 子智能体 | subagents + Swarm（Code Mode 内 fan-out） | `tools/subagent/` mailbox/bus/task_claim/worktree 隔离 | **持平，mimofan 隔离更硬** |
| 运行中引导 | `/steer`、`/queue steer` | `rx_steer` 通道，回合内注入 | **持平** |
| 浏览器自动化 | `browser` 工具（CDP/扩展/登录态） | 无 | **缺失，中价值** |
| 生命周期 hook | plugin hooks + agent-hooks | 13 变体 `hooks/mod.rs` + `crates/hooks/` EventFrame | **持平** |
| 多 Provider | 数十个 provider extensions | `model_registry`/`model_catalog`/`model_routing` | **持平** |
| IDE / 外部协议 | ACP + acp-core package | `acp_server/`、`mcp_server/`、`app-server` | **持平** |
| 聊天渠道 / 移动端 / 媒体生成 | 完整 | 无 | **非目标域** |

---

## 2. 工具能力

- [x] **工具延迟加载 / 大目录检索**：mimofan `crates/tui/src/core/engine/tool_catalog.rs` 已实现 `tool_search` 工具（`TOOL_SEARCH_NAME`，wire type `tool_search_20251119`），支持 `bm25` 与 `regex` 两种匹配（第 251-253 行），`defer_loading` 工具中途激活并追加到目录尾部（第 277、293 行）。等价于 OpenClaw 的 Tool Search。
- [x] **文件读写与补丁**：`tools/file.rs`、`tools/apply_patch.rs`、`tools/fim.rs`、`tools/diff_format.rs`。OpenClaw 对应 `read`/`write`/`edit`/`apply_patch`。
- [x] **Shell 执行与进程管理**：`tools/shell.rs`、`tools/shell_tools.rs`、`tools/shell_output.rs`、`shell_dispatcher/`。
- [x] **人类介入询问**：`tools/user_input.rs`（对应 OpenClaw `ask_user`）。
- [x] **Web 搜索与抓取**：`tools/web_search.rs`、`tools/fetch_url.rs`、`tools/web_run.rs`。
- [x] **代码执行**：Python `code_execution` + JavaScript `tools/js_execution.rs`（经本机 Node 运行，由 `dependencies::resolve_node` 门控，Node 缺失则不注册）。
- [x] **TTS / 语音**：`tools/speech.rs`（MiMo v2.5 TTS，含 voiceclone / voicedesign）。OpenClaw 需装 elevenlabs / azure-speech / deepgram 等 extension。
- [x] **视觉 / OCR**：`vision/mod.rs`、`vision/tools.rs`、`tools/image_ocr.rs`。
- [x] **大输出治理**：`tools/large_output_router.rs`（超过 4096 token 阈值即走 V4-Flash 子智能体压缩，原文存入 workshop 变量 `last_tool_result` 供 `promote_to_context` 回捞）+ `tools/truncate.rs`（溢出落盘 `~/.mimofan/tool_outputs/`，7 天过期清理）。**对应 OpenClaw 需要额外安装的 Tokenjuice 插件，mimofan 是内建且更完整（OpenClaw 只压缩不留原文回捞路径）。**
- [x] **工具参数自修复**：`tools/arg_repair.rs`、`tools/schema_sanitize.rs`、`tools/schema_canonicalize.rs`。OpenClaw 对应 `packages/tool-call-repair`。

- [ ] **Code Mode：沙箱内编程式工具编排**
  - openclaw 落点：`docs/tools/code-mode.md`。模型不再看到全部工具 schema，只看到 `exec` + `wait`；模型写一段 JS/TS，在 **QuickJS-WASI worker** 里通过 `tools.callValue` 搜索/描述/调用隐藏工具目录。所有调用仍走正常执行路径（policy / approvals / hooks / telemetry 全部生效）。MCP 工具收敛到 `MCP` 命名空间。
  - 为什么值得做：这是与 mimofan 现有 `tool_search` **互补而非重复**的能力。`tool_search` 解决「schema 太多塞不下」，Code Mode 解决「多工具组合要多个回合」——一段程序里 `Promise.all` 并发调 5 个工具 + 条件分支，从 5-10 个模型回合压到 1 个。对 mimofan 这种长程编码任务，回合数直接等于 token 成本与延迟。mimofan 已有 `tools/parallel.rs` 与 `js_execution.rs`，但前者是固定并行原语、后者是裸跑脚本片段，**都没有「guest 侧回调宿主工具目录」的桥**，这是真正缺的那一块。

- [ ] **浏览器自动化**
  - openclaw 落点：`docs/tools/browser.md`、`extensions/browser/`，含 CDP 连接、Chrome 扩展、登录态复用、WSL2/Linux 排障文档。
  - 验证：mimofan grep `browser|cdp|playwright|webdriver` 仅命中 `mcp/oauth.rs:725` 的 `webbrowser::open`（OAuth 拉起系统浏览器）与若干无关词根，**无浏览器驱动能力**。
  - 为什么值得做：编码 agent 的验证闭环常需要「改完前端 → 打开页面 → 截图/取 DOM → 确认」。mimofan 已有 `vision/` 与 `tools/dev_server_readiness.rs`（探测开发服务器就绪），补一个最小 CDP 客户端即可把这条链路接通。优先级低于 Code Mode，因为纯后端/CLI 项目用不到。

---

## 3. 上下文与压缩

- [x] **多范式压缩（mimofan 明显领先）**：
  - `crates/tui/src/compaction/` — 替换式摘要压缩
  - `crates/tui/src/seam_manager/` — append-only 接缝管理，保护 prefix cache 不失效
  - `crates/tui/src/purge/` — 外科式精准清理
  - `core/engine.rs::recover_context_overflow` — 溢出恢复
  - OpenClaw 只有单一 compaction 路径（`src/agents/agent-compaction-constants.ts`、`agent-command.compaction-rotation.test.ts`）加一个溢出重试。**这一域 mimofan 是净优势。**
- [x] **上下文预算与分区**：`context_budget/`、`prompt_zones/`、`route_budget/`、`context_report/`。
- [x] **prefix cache 保护**：`prefix_cache/`（OpenClaw 侧只在 trajectory 里记录 provider 返回的 prompt-cache 元数据，无主动保护策略）。
- [x] **工作集管理**：`working_set/`、`project_context/`、`project_context_cache/`。

- [ ] **真实 tokenizer**
  - 现状：mimofan 全仓 `*.toml` grep `tiktoken|tokenizers|bpe|byte_pair` **零命中**；`tools/large_output_router.rs` 明确使用 `CHARS_PER_TOKEN_ESTIMATE = 3` 的保守启发式。
  - 为什么值得做：mimofan 的压缩三范式、`context_budget`、`route_budget`、大输出路由**全部**建立在 token 估算之上。启发式偏保守意味着**过早触发压缩**——本该留在上下文里的信息被摘要掉，长程任务的信息保真度直接受损。这是唯一一个「地基层」缺失：修它能同时提升四个已有子系统的精度，价值密度最高。建议接 `tokenizers` crate（HuggingFace 官方 Rust 实现），按 model_registry 里的模型族选对应 encoding，估算失败时回退到现有启发式。

---

## 4. 记忆与学习

- [x] **持久化记忆（mimofan 明显领先）**：`crates/memory/` 下有 `compressor` / `embedding` / `injector` / `knowledge` / `vector` / `optimization` 全套；工具侧 `tools/remember.rs`、`tools/remember_vector.rs`，**向量记忆默认开启**。OpenClaw 的 `src/memory/` 只有一个 `root-memory-files.ts`（文件级记忆），语义检索要靠 `extensions/active-memory` 外挂。
- [x] **会话管理与回溯**：`session_manager/`、checkpoint / fork / `tools/revert_turn.rs`、`snapshot/`。
- [x] **技能加载**：`skills/`（`discover`、`SkillRegistry`、多目录发现）+ `tools/skill.rs`（`load_skill`，一次调用取回 SKILL.md 正文 + 同目录伴生文件清单，省掉 `read_file`+`list_dir` 两步）。

- [ ] **自学习：把经验固化成技能**
  - openclaw 落点：`docs/tools/self-learning.md` + `src/skills/workshop/`。默认 `auto` 模式。触发条件很克制：前台回合已完成或被用户打断（但不是 provider 报错）、本回合用了 ≥10 次模型迭代、非 cron/heartbeat/subagent 等后台运行、系统静默 30 秒、无其他活跃 run。然后起一个**隔离的后台评审模型**去读真实证据，产出**至多一条** pending 提案：优先改已有 pending 提案 → 其次给现有技能打补丁（必须持有全文读取回执，hash 绑定，保证未触碰内容按构造存活）→ 都不匹配才新建技能。永远只写提案不直接改线上技能，且不能调用通用 agent 工具。
  - 验证：mimofan grep `skill_workshop|SelfLearn|learned_skill|skill_propos` **零命中**，`tools/skill.rs` 只有 `load_skill` 一个只读工具，`skills/` 下 `install.rs`/`system.rs`/`mod.rs` 均为发现与安装，无生成路径。
  - 为什么值得做：mimofan 已经把**所有原材料**备齐了——`hooks/mod.rs` 的 `TurnEnd`/`SessionEnd` 提供触发点，`crates/memory/` 提供存储与检索，`skills/SkillRegistry` 提供消费端，`tools/subagent/` 提供隔离评审 runner，`slop_ledger/` 甚至已经在记录质量信号。缺的只是把它们串起来的那条「回合结束 → 后台评审 → 技能提案」链路。这是**用现有零件拼装**的能力，投入产出比极高；而且它是唯一能让 agent 随使用变强的机制，其余能力都是一次性的。特别注意 OpenClaw 那套「打断的回合恰恰是最有价值的证据（走错的路 + 用户的纠正）」的设计洞察值得直接借鉴。

---

## 5. 扩展性

- [x] **MCP 双向**：`crates/tui/src/mcp/`（客户端，含 `oauth.rs`）+ `mcp_server/`（服务端）+ `mcp_server_backend/`。OpenClaw 侧 `src/mcp/` + `agents/agent-bundle-mcp-*`（约 20 个文件）。
- [x] **插件系统**：`tools/plugin.rs` — `~/.mimofan/tools/` 下放自描述脚本，frontmatter 声明 `name`/`description`/`schema`/`approval`，stdin 收 JSON、stdout 回 `ToolResult`，120s 超时。比 OpenClaw 的 npm/ClawHub 插件轻量得多，但**免安装、免构建**，对单机编码 agent 更合身。
- [x] **生命周期 hook**：`crates/tui/src/hooks/mod.rs` 13 个变体（`SessionStart`/`SessionEnd`/`MessageSubmit`/`ToolCallBefore`/`ToolCallAfter`/`ModeChange`/`OnError`/`TurnEnd`/`SubagentSpawn`/`SubagentComplete`/`ShellEnv`/`PreCompact`/`PostCompact`，共 1453 行）；另有 `crates/hooks/` 一套 EventFrame sink（6 变体）。**注意：只 grep `crates/hooks/` 会误判为能力不足。**
- [x] **自定义子智能体**：`tools/subagent/custom_agents.rs`。
- [x] **斜杠命令体系**：`crates/tui/src/commands/groups/`（core/session/project/config/debug 分组）。
- [x] **外部协议**：`acp_server/`（ACP，对应 OpenClaw `packages/acp-core` + `src/acp/`）、`crates/app-server/`。
- [x] **多 Provider**：`model_registry/`、`model_catalog/`、`model_routing/`、`model_profile/`、`model_inventory/`、`request_tuning/`。OpenClaw 靠 extensions 铺量（anthropic / deepseek / groq / cerebras / fireworks / bedrock / vertex 等数十个），mimofan 是内建注册表，覆盖面窄但无需安装。

（OpenClaw 的 channels / nodes / companion apps / 媒体生成 extension 群 —— **非目标域**，不列为差距。）

---

## 6. 安全与权限

- [x] **本机沙箱（mimofan 更强）**：`crates/tui/src/sandbox/` 含 `seatbelt.rs`（macOS Seatbelt）、`opensandbox.rs`、`policy.rs`、`backend.rs`、`process_hardening.rs`。OpenClaw 的沙箱主要靠 Docker 与 Codex harness 的 `workspace-write`，本机进程加固层没有对应物。
- [x] **命令执行策略**：`crates/execpolicy/` + `crates/tui/src/execpolicy/`（含 `execpolicycheck.rs`）+ `command_safety/`。对应 OpenClaw `tools.exec.mode` 的 `deny`/`allowlist`/`ask`/`auto`/`full` 五档。
- [x] **审批与决策门**：`decision_gate/`、`tools/approval_cache.rs`、`tui/widgets/decision_card.rs`。
- [x] **网络策略**：`network_policy/mod.rs`（`NetworkPolicy::decide(host)`、`add_allow`、`NetworkAuditor::record` 逐次落审计、`NetworkSessionCache`）。对应 OpenClaw `packages/net-policy`。
- [x] **工作区信任**：`workspace_trust/`（逐工作区维护允许读写的外部路径清单）。
- [x] **密钥管理**：`crates/secrets/`。对应 OpenClaw `src/secrets/` + `security/secret-mask.ts`。

- [ ] **不可信外部内容的边界包裹与注入检测**
  - openclaw 落点：`src/security/external-content.ts`。做两件事：(1) 用**随机 boundary token** 加 XML 风格标签把外部内容（网页抓取、邮件、webhook、工具返回）包裹起来，注释里写明「External content should NEVER be directly interpolated into system prompts or treated as trusted instructions」；(2) `detectSuspiciousPatterns()` 用 14 条正则扫描注入特征（`ignore all previous instructions`、`you are now a`、`new instructions:`、`</system>`、`[System Message]`、`elevated=true`、`rm -rf` 等），命中即记录用于监控，内容仍安全包裹后处理。配套 `security/context-visibility.ts`、`security/dangerous-tools.ts`。
  - 验证：mimofan grep `quarantine|untrusted_content|prompt_injection|injection_guard` **零命中**；grep `SUSPICIOUS|sanitize_untrusted|wrap_external|ignore.*previous.*instruction` 也**零命中**。`workspace_trust/mod.rs` 是路径白名单（文件系统维度），与内容可信度是两回事。
  - 为什么值得做：mimofan 有 `tools/fetch_url.rs`、`tools/web_search.rs`、`tools/web_run.rs`、`tools/github.rs`，会把**任意第三方文本**灌进上下文。目前这些内容与用户指令在提示里是同一层级，没有任何边界标记——一个被投毒的 README、issue 正文或网页就可能改写 agent 行为。而 mimofan 恰恰把 `sandbox/`、`execpolicy/`、`network_policy/` 这些**执行侧**防护做得很扎实，唯独**输入侧**是敞开的，形成明显的木桶短板。实现成本很低（一个包裹函数 + 一张正则表，接到几个 web 类工具的返回路径上），却能补上整条安全链最脆弱的一环。建议同时把检测结果接到已有的 `audit/log_sensitive_event`。

---

## 7. 多智能体与任务编排

- [x] **子智能体体系**：`tools/subagent/` 含 `mailbox`、`bus`、`task_claim`、`decomposer`、`aggregator`、`custom_agents`、`helpers`，并支持 **git worktree 隔离**。OpenClaw 的 subagents 无等价的工作区物理隔离，Swarm 靠配额（`maxConcurrent`/`maxChildrenPerGroup`/`maxTotalPerGroup`）兜底。
- [x] **任务与依赖图**：`task_manager/`、`tools/tasks.rs`、`tools/todo.rs`（`is_blocked` / `unmet_dependencies` / `ready_ids` / `blocked_ids`）。OpenClaw 明确说明「没有 graph DSL，程序即编排」，**这一域 mimofan 的结构化程度更高**。
- [x] **运行中引导（steering）**：`core/engine/turn_loop.rs` 的 `rx_steer` 通道（第 187、700、1266 行），支持回合中途与工具批次之间注入用户输入（`pending_steers`）。对应 OpenClaw `/steer` 与 `/queue steer`。
- [x] **目标循环与反漂移断路器**：`goal_loop/mod.rs` 的 `decide_continuation()`，`StopReason` 覆盖 `Completed`/`Blocked`/`ContinuationLimit`/`TokenBudget`/`TimeBudget`/`NoProgress`/`RepeatedError`；阈值来自 `tools/goal.rs::DEFAULT_NO_PROGRESS_ROUNDS` / `DEFAULT_REPEATED_ERROR_ROUNDS`。
- [x] **工具失败自愈提示**：`turn_loop.rs:58` 注入 `[Self-heal]` 提示，显式要求模型不要盲目重复同一次调用；另有「重复错误熔断器」（第 147 行注释）。
- [x] **调度与自动化**：`scheduler/mod.rs`、`automation_manager/mod.rs`、`tools/automation.rs`、`issue_monitor/`。对应 OpenClaw 的 `cron` / `heartbeat`。
- [x] **舰队并行**：`fleet/manager.rs`、`fleet/executor.rs`。

- [ ] **全局工具调用循环检测（细粒度）**
  - openclaw 落点：`docs/tools/loop-detection.md`。两道协作守卫：(1) **滚动历史检测器**（`tools.loopDetection.enabled`，默认关）——监视重复模式、同工具同入参的高频无结果循环、已知轮询工具的特定重复模式；(2) **压缩后守卫**（默认开）——每次压缩重试后武装，若 agent 在窗口内重复同一 `(tool, args, result)` 三元组即以 `compaction_loop_persisted` 中止，专门用来打破「上下文溢出 → 压缩 → 同样的循环」死环。
  - 验证与边界：mimofan **已有** `goal_loop` 断路器，但两者粒度不同——`goal_loop` 是 `/loop` 目标模式下的**回合级**统计（连续 N 轮无文件变更 / 同类错误重复 N 轮），而 OpenClaw 的是**每次工具调用**级别的 `(tool, args, result)` 三元组指纹匹配，且在**所有会话**生效而非仅目标模式。mimofan grep `loop_detection|RepeatDetect|args_hash|call_signature|dedup` 在工具调用路径上**零命中**。
  - 为什么值得做：缺的正是「压缩后守卫」这一段。mimofan 有 `recover_context_overflow` 溢出恢复和三套压缩，但**压缩完之后没有任何机制检查模型是否又走回了导致溢出的同一条路**——这恰恰是长程任务最烧钱的失败模式：溢出→压缩→重复同样的调用→再溢出。mimofan 已有 `PreCompact`/`PostCompact` hook 作为天然武装点，补一个工具调用指纹环形缓冲即可，改动面很小。

---

## 8. 可观测性与调试

- [x] **成本与用量**：`cost_status/`、`pricing/`、`route_budget/`、`resource_telemetry/`。
- [x] **日志与运行时**：`logging/`、`runtime_log/`、`error_taxonomy/`、`retry_status/`。
- [x] **评测**：`crates/tui/src/eval/`。
- [x] **质量账本**：`slop_ledger/`（OpenClaw 无对应物）。
- [x] **诊断工具**：`tools/diagnostics.rs`（工作区与工具链环境信息收集）。
- [x] **会话导出**：`commands/groups/session/export.rs`、`session.rs:307 export()` — 导出 Markdown 对话记录。
- [x] **符号索引**：`symbol_index/`；**LSP**：`lsp/registry.rs` + `core/engine/lsp_hooks.rs`。

- [ ] **Trajectory：可复盘的会话飞行记录仪**
  - openclaw 落点：`docs/tools/trajectory.md` + `src/trajectory/`。`/export-trajectory` 打包一份**脱敏**支持包，落在 `.openclaw/trajectory-exports/<session>-<timestamp>/`，内容包括：发给模型的 prompt、**system prompt 与工具 schema 全文**、哪些 transcript 消息与工具调用导向了最终答案、本次运行是超时/中止/压缩/还是 provider 错误、生效的模型与插件与技能与运行时设置、provider 返回的 usage 与 prompt-cache 元数据。因为可能含敏感信息，导出**强制走 exec 审批**，群聊场景只私发给 owner。
  - 验证：mimofan grep `trajectory|export_trajectory|support_bundle|flight_recorder|diagnostics_bundle` **零命中**。现有 `session.rs::export()` 逐条遍历 `app.history` 的 `HistoryCell` 渲染成 Markdown，是**面向人阅读的 UI 层文字记录**；grep 该文件 `system_prompt|redact` **零命中**——不含 system prompt、不含工具 schema、不含 usage/cache 元数据、无脱敏逻辑，与 trajectory 不是一回事。
  - 为什么值得做：mimofan 现在有**十几个**互相影响的上下文子系统（三套压缩 + seam + purge + prefix_cache + context_budget + route_budget + prompt_zones + tool_catalog 延迟加载 + model_routing）。当一次运行结果不对时，几乎无法回答「模型当时到底看到了什么」——而这正是调这类系统唯一有效的手段。项目记忆里「压缩阈值判断散落 4 处」这类技术债，也需要一份能看到实际生效值的快照才好收敛。这是**开发者自己受益最大**的一项：它不直接提升 agent 能力，但让上面所有能力都变得可诊断、可优化。

- [ ] **指标导出（OTel / Prometheus）**
  - openclaw 落点：`extensions/diagnostics-otel/`、`extensions/diagnostics-prometheus/`。
  - 验证：mimofan grep `otel|opentelemetry|prometheus|OTLP` 于全仓 `*.rs`/`*.toml` **零命中**；`audit/mod.rs` 全文只有 `log_sensitive_event()` 一个函数。
  - 为什么值得做：优先级低。单机终端工具的用户不会架 Prometheus。`resource_telemetry/` 与 `cost_status/` 已覆盖日常需要。仅当 mimofan 要走 `acp_server`/`app-server` 的长期服务化路线时才需要。

---

## 9. 建议优先补齐的 Top 5（按价值密度排序）

价值密度 = 收益 ÷ 改动成本，并优先考虑「能否复用 mimofan 已有零件」。

### 1. 真实 tokenizer（地基层，一改多受益）
- **为什么第一**：唯一一个被**四个已有子系统**共同依赖的基础设施。压缩三范式、`context_budget`、`route_budget`、`large_output_router` 现在全部基于 `CHARS_PER_TOKEN_ESTIMATE = 3` 的保守估算，导致**过早压缩、信息过早丢失**。修一处，四处精度同时提升。
- **成本**：小。引入 `tokenizers` crate，按 `model_registry` 的模型族选 encoding，做一层 `estimate_tokens()` 抽象，失败回退现有启发式。
- **落点**：新建 tokenizer 模块，替换 `tools/large_output_router.rs`、`context_budget/`、`compaction/` 中的估算调用。

### 2. 不可信外部内容包裹 + 注入检测（补最短的那块板）
- **为什么第二**：mimofan 执行侧防护（sandbox / execpolicy / network_policy / workspace_trust）做得很扎实，输入侧却完全敞开。`fetch_url` / `web_search` / `web_run` / `github` 把第三方文本与用户指令平级灌入上下文。安全链的强度取决于最弱环节。
- **成本**：很小。一个 boundary-token 包裹函数 + 一张正则表 + 接到几个 web 类工具的返回路径，检测结果送 `audit::log_sensitive_event`。
- **参考**：`src/security/external-content.ts`。

### 3. 自学习 / 技能工坊（唯一让 agent 越用越强的机制）
- **为什么第三**：所有零件 mimofan 都有了——`hooks` 的 `TurnEnd`/`SessionEnd` 做触发、`tools/subagent/` 做隔离评审、`crates/memory/` 做存储、`skills/SkillRegistry` 做消费、`slop_ledger/` 做信号。缺的只是串联。其余所有能力都是一次性收益，只有这条会随时间复利。
- **成本**：中。但基本是装配而非从零建设。
- **务必借鉴**：OpenClaw 那套克制的触发条件（≥10 次模型迭代、静默 30 秒、排除后台运行、provider 报错不触发）和「只产出 pending 提案、绝不直写线上技能、打补丁需全文读取回执做 hash 绑定」的安全模型。**被用户打断的回合是最有价值的证据**——错误路径加上人类纠正。

### 4. Trajectory 飞行记录仪（让前三项可验证）
- **为什么第四**：mimofan 上下文子系统的复杂度已经超过「靠读代码推断行为」的极限。没有它，上面三项改完也无法确认真的生效。它是**元能力**——投资它等于给所有后续优化装上仪表盘。放在第四是因为它本身不提升 agent 能力。
- **成本**：中。需要在 turn_loop 里挂采集点，记录 system prompt / tool schema / 压缩事件 / usage 元数据，导出时脱敏。
- **提示**：可直接复用现有 `PreCompact`/`PostCompact`/`ToolCallBefore`/`ToolCallAfter` hook 作为采集埋点，不必改引擎主干。

### 5. 压缩后循环守卫（小改动，堵最烧钱的失败模式）
- **为什么第五**：mimofan 已有 `goal_loop` 回合级断路器，但缺「压缩之后模型是否又走回老路」这道检查。「溢出 → 压缩 → 重复同样调用 → 再溢出」是长程任务最贵的死环，而 mimofan 恰好有三套压缩机制，触发面更大。
- **成本**：很小。一个 `(tool, args, result)` 指纹环形缓冲，在 `PostCompact` hook 处武装，命中即中止。
- **注意**：这是对 `goal_loop` 的**补充而非替代**——粒度不同（工具调用 vs 回合）、作用域不同（全会话 vs 目标模式）。

---

### 明确不建议跟进

- **聊天渠道 / 移动端 / companion apps / 媒体生成 / Gateway 控制面**：属于 OpenClaw 的产品定位，与终端编码 agent 无关。
- **OTel / Prometheus 导出**：单机工具用户不会用；`resource_telemetry/` + `cost_status/` 已够。除非走服务化路线。
- **重型插件市场（ClawHub / npm 包分发）**：mimofan 的 `tools/plugin.rs` 脚本式插件免安装免构建，对单机场景更合适，不必换成重型方案。

---

## 附：本次核实中避免的误判

以下能力**曾疑似缺失，经 grep 验证实际已存在**，记录以防后续重复误判：

| 疑似缺失 | 实际落点 |
| --- | --- |
| Tool Search 工具延迟加载 | `core/engine/tool_catalog.rs:27` `TOOL_SEARCH_NAME`，bm25 + regex |
| 运行中 steering | `core/engine/turn_loop.rs:187,700,1266` `rx_steer` / `pending_steers` |
| 工具输出压缩（Tokenjuice 等价物） | `tools/large_output_router.rs` + `tools/truncate.rs`，内建且带原文回捞 |
| 反漂移 / 循环断路器 | `goal_loop/mod.rs` `StopReason::NoProgress` / `RepeatedError`（回合级；仅细粒度指纹检测缺失） |
| 生命周期 hook 数量不足 | `crates/tui/src/hooks/mod.rs` 13 变体 1453 行（只看 `crates/hooks/` 的 6 变体会误判） |
| TTS / 语音 | `tools/speech.rs`（MiMo v2.5，含 voiceclone / voicedesign） |
| JS 执行能力 | `tools/js_execution.rs`（存在；缺的是 Code Mode 的「guest 回调宿主工具目录」桥） |
| 任务依赖图 | `tools/todo.rs` / `tools/tasks.rs` `is_blocked` / `ready_ids` / `blocked_ids` |
| 网络访问控制 | `network_policy/mod.rs` `decide()` / `NetworkAuditor` |
| 会话导出 | `commands/groups/session/session.rs:307`（但**不等于** trajectory：无 system prompt / schema / 脱敏） |
| 定时任务 / cron | `scheduler/mod.rs`、`automation_manager/mod.rs`、`fleet/scheduler.rs`、`commands/groups/core/schedule.rs` |
| ACP 协议服务端 | `acp_server/mod.rs`（484 行；存在，仅深度不及 openclaw 的 ~1.5 万行） |
| 结构化输出 / JSON Schema | `model_profile/mod.rs`、`tools/review.rs`、`mcp_server/mod.rs`（`response_format` / `json_schema`） |
| SQLite 持久化 | `crates/state/`、`crates/memory/src/vector.rs`、`tui/src/vector_memory/`、`tui/src/turn_memory.rs` |
| 开放回路 / 未竟事项追踪 | `slop_ledger/mod.rs`（比 openclaw `src/commitments/` 的 `open_loop` 更贴合编码场景） |

---

## 附二：补充对标（第二轮独立核实，前文未覆盖的 5 项）

以下条目由第二位对标成员独立调研得出，与前文**无重叠**，均已 grep 验证。

### A. 会话历史结构：树 vs 线性

- [ ] **会话树（per-entry tree）与分支摘要**
  - openclaw 落点：`packages/agent-core/src/harness/types.ts:38-118` 定义 `SessionTreeEntry`，含 12 种节点类型——`message` / `compaction` / `branch_summary` / `reset` / `label` / `model_change` / `thinking_level_change` / `session_info` / `leaf` / `custom` 等，每个节点带 `parentEntryId`。`harness/compaction/branch-summarization.ts` 在此之上实现**分支摘要**：探索性分支收束时单独摘要，并把该分支读过/改过的文件清单（`readFiles` / `modifiedFiles`）合并进摘要，随 `CompactionEntry.details` 沿树继承（`compaction.ts:48-66` `extractFileOperations`）。
  - mimofan 现状：**线性**。`session_manager/mod.rs:126-129` 的代码注释自认——「current saved sessions are **linear JSON files, not per-entry trees**」，fork 只记录 `parent_session_id` + `forked_from_message_count`（粗粒度快照点）。
  - 为什么值得做：这是 mimofan 上下文体系里唯一的**结构性**短板，且会放大已有优势。mimofan 有三套压缩范式，但它们都作用在一条线性历史上；一旦有了树，`purge/` 的外科清理可以按分支整枝、`seam_manager/` 的软缝可以按分支落点、`revert_turn` 可以变成真正的树上跳转而非线性回退。**「文件读写清单随摘要继承」这个细节尤其值得抄**——压缩后模型最常见的退化就是忘了自己已经读过/改过哪些文件，导致重复读取（烧 token）或重复编辑（改坏）。这一点与前文 Top 5 第 5 项「压缩后循环守卫」是同一个失败模式的两种解法，可以合并设计。
  - 成本：中偏大（涉及会话持久化格式迁移），建议先做「摘要携带文件清单」这个低成本子集。

### B. 技能内容安全扫描

- [ ] **技能内容级安全扫描（非结构校验）**
  - openclaw 落点：`src/skills/security/scanner.ts` + `scan-evidence.ts`。对技能文件与 manifest 做**内容**扫描，输出分级 findings（`info` / `warn` / `critical`），带 `ruleId` / 行号 / evidence 片段，含字面量密钥检测规则（`LITERAL_SECRET_SKILL_CONTENT_RULE`）。扫描失败/阻断会触发 `security_scan_blocked` / `security_scan_failed` hook 事件。配套 `clawhub-verdicts.ts` 做第三方技能信誉裁决。
  - mimofan 现状：`skills/install.rs:1029` 的 `scan_tarball()` 只做**结构**校验——路径穿越（zip-slip）、大小上限、定位 `SKILL.md`。经 grep（`scan|audit|severity|finding` 在 `skills/` 下仅 15 处命中，全部属于 tarball 解包逻辑），**无任何内容层面的危险模式检测**。
  - 为什么值得做：与前文 Top 5 第 2 项（外部内容注入防护）是**同一条攻击面的两端**——第 2 项防的是「运行时读进来的不可信文本」，本项防的是「安装期落盘的不可信可执行指令」。而技能是**持久化**的：一个恶意 SKILL.md 会在之后每次会话被当作可信指令加载，危害远高于一次性的 web 抓取。mimofan 有 `skills/install.rs` 的远程安装路径（58420 字节，支持从远端拉取），这条路径目前是敞开的。
  - 成本：小。复用第 2 项的正则表基础设施，在 `scan_tarball()` 之后加一道内容 pass，命中 critical 则拒绝安装并写 `audit::log_sensitive_event`。**建议与 Top 5 第 2 项打包一起做**，边际成本接近零。

### C. 安全态势自检（security posture audit）

- [ ] **分级安全审计报告**
  - openclaw 落点：`src/security/audit.types.ts` 定义 `SecurityAuditFinding { checkId, severity, title, detail, remediation }` 与 `SecurityAuditReport { summary{critical,warn,info}, findings, suppressedFindings, deep }`；实现分散在 `audit-fs.ts`（文件权限）、`audit-gateway-config.ts`、`audit-plugins-trust.ts`（插件信任）、`audit-model-refs.ts`、`audit-deep-code-safety.ts` 等 20+ 个 check 模块，支持**抑制规则**（带 reason）与**深度探测**。`security/windows-acl.ts`、`secret-mask.ts`、`dangerous-config-flags.ts` 提供底层能力。
  - mimofan 现状：`cli/doctor.rs` 有 1611 行，但 grep `critical|severity|finding|remediation` **零命中**——它是健康/配置诊断（API key 来源、模型连通性、MCP 配置、Node 依赖），产出的是人读文本 + 机器可读 JSON 报告，**没有 severity 分级、没有 remediation 建议、没有安全 check 的概念**。`workspace_trust/mod.rs` 仅 167 行，`audit/mod.rs` 仅 45 行。
  - 为什么值得做：mimofan 的执行侧防护（sandbox / execpolicy / network_policy）单点都很强，但**用户无法知道自己当前的配置组合是否安全**。典型场景：用户为了跑通某个任务临时开了 `--dangerously-skip-permissions` 或放宽了 network policy，事后忘记收回。openclaw 的 `dangerous-config-flags.ts` 正是为此存在。价值中等但成本低——`doctor.rs` 的骨架已经在了，加一个 finding 模型 + 十来条 check 即可。
  - 成本：小。建议作为 `doctor --security` 子命令，复用现有配置读取路径。

### D. 跨 backend 故障转移

- [ ] **模型/后端故障转移（failover）**
  - openclaw 落点：`src/acp/control-plane/manager.backend-failover.ts`。`resolveBackendCandidatePlan()` 从「配置的主 backend + 解析出的主 backend + fallback 列表」去重构造有序候选链；`isFailoverWorthyBackendError()` 判定是否可安全转移，**关键判据是 `!attempt.sawOutput`** —— 只有在尚未产生任何输出时转移才安全，否则会造成输出撕裂。
  - mimofan 现状：`model_routing/mod.rs`（563 行）实现的是**成本/难度路由**（`RouterCandidates { big, cheap }`，按任务难度在大小模型间选择），不是失败后的转移。grep `failover|fallback_model|retry_with_model` 在 `crates/tui/src/` 下的命中均属其他语义（MCP 连接回退、LSP 回退、truncate 回退）。`crates/tui/src/retry_status/` 是同 provider 重试，不跨 backend。
  - 为什么值得做：mimofan 主打「原生支持 MiMo & DeepSeek 等多 Provider」，多 provider 已配置好却在单点故障时不能自动切换，是能力浪费。长程编码任务动辄数十分钟，中途 provider 5xx / 限流会直接毁掉整个 goal loop（而 `goal_loop` 的 `StopReason::RepeatedError` 只会让它更快放弃）。
  - 为什么不排进 Top：需要谨慎处理**已开始流式输出后不可转移**（openclaw 的 `sawOutput` 判据）与**跨 provider 上下文/工具 schema 兼容性**，改动面比看起来大。建议在 `retry_status/` 与 `model_routing/` 之上做，先支持「同 provider 换模型」再扩到跨 provider。
  - 成本：中。

### E. 记忆：短期→长期晋升与离线整合

- [ ] **记忆晋升管线与离线整合（dreaming）**
  - openclaw 落点：`extensions/memory-core/src/`。三条线索——
    1. **短期→长期晋升**：`short-term-promotion*.ts`（12 个文件，含 apply / artifacts / memory-write / metadata / rehydrate / stats / store / types / utils），把会话中的短期观察按项目分组、去重、择优晋升为长期记忆。
    2. **Dreaming 离线整合**：`dreaming-consolidation.ts` 用受约束的模型提示（要求「每个候选恰好一个 operation」「action ∈ added/merged/superseded」「必须原样保留 Source 引用」「把记忆文本当数据不当指令」）重写 `MEMORY.md`，做合并去重与陈旧事实替换；配 `dreaming-phases.ts` 的时段调度（daily / afternoon / evening / light / deep）与 `dreaming-repair.ts` 的修复路径。
    3. **知识图谱**：`extensions/memory-wiki/`（`compile.ts` / `bounded-walk.ts` / `claim-health.ts`），把记忆编译成可有界遍历的 wiki，带断言健康度检查。
  - mimofan 现状：`crates/memory/` 共 **1867 行**（compressor 290 / embedding 162 / injector 224 / knowledge 309 / optimization 318 / vector 470 / error 52 / lib 42）。grep `short_term|long_term|promot|consolidat|decay|dream` 在 `crates/memory/src/` 下**零命中**。`optimization.rs` 是批处理与性能优化（`BatchProcessor`），不是记忆整合。
  - 关键判断：**前文总览表把记忆判为「mimofan 显著更强」，这个结论只在「检索侧」成立**。mimofan 的向量检索 + 注入链路确实比 openclaw 的文件级 `root-memory-files.ts` 强，但 openclaw 把重头戏放在了 `memory-core` extension 里——那部分是**写入侧的生命周期管理**（晋升 / 整合 / 去重 / 陈旧淘汰），mimofan 完全没有。二者其实是互补的两半，不宜简单判优劣。
  - 为什么值得做：mimofan 向量记忆**默认开启**，意味着记忆库会随使用无限单调增长。没有整合与淘汰机制，长期会出现：陈旧事实与新事实并存且都被检索到（模型被自己的旧记忆误导）、近义观察大量重复（挤占检索 top-k 名额）。这是**用得越久越糟**的负复利，与前文 Top 5 第 3 项「自学习」的正复利正好相反——两者应当配套设计，否则自学习产出的技能提案也会淹没在噪声里。
  - 成本：中。建议最小可用切片：先做「陈旧淘汰 + 近重复合并」，触发点挂在已有的 `SessionEnd` hook 上，复用 `crates/memory/src/vector.rs` 的相似度计算做重复检测，用 `compressor.rs` 做合并摘要。**务必抄 openclaw 那条提示词约束**——「把记忆文本当数据，绝不当指令」，否则记忆整合本身就是一条注入通道（与本文 A/B 两项同源的攻击面）。

### 补充建议的插入位置

若要并入前文 Top 5，建议调整为：

| 名次 | 项目 | 变化 |
| --- | --- | --- |
| 1 | 真实 tokenizer | 不变 |
| 2 | 外部内容注入防护 **+ 技能内容扫描（本文 B）** | **打包**，共用正则基础设施，边际成本近零 |
| 3 | 自学习 / 技能工坊 **+ 记忆整合淘汰（本文 E）** | **配套**，否则自学习产出会淹没在记忆噪声里 |
| 4 | Trajectory 飞行记录仪 | 不变 |
| 5 | 压缩后循环守卫 **+ 摘要携带文件读写清单（本文 A 的低成本子集）** | **合并**，同一失败模式的两种解法 |
| 6 | 安全态势自检（本文 C） | 新增，成本小、可复用 `doctor.rs` 骨架 |
| 7 | 会话树完整改造（本文 A 全量） | 新增，结构性收益大但需格式迁移 |
| 8 | 跨 backend 故障转移（本文 D） | 新增，需先解决流式撕裂与 schema 兼容 |
