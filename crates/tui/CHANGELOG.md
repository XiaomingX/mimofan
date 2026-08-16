# 更新日志

本项目的所有重要变更都记录在此。版本遵循[语义化版本控制](https://semver.org/)，从工作区根目录（`Cargo.toml` → `[workspace.package] version`）递增。

## [0.0.19] - 2026-08-16

本轮（第二批能力补齐闭环 #844–#869）补齐成本/智能/无人值守 + herdr 借鉴的多项能力，并交付终端/会话持久化运行时（#869 核心保证：会话脱离主进程可断电续跑/reattach）。

### Added
- **PTY 持久化运行时（[#869](https://github.com/XiaomingX/mimofan/issues/869)）**：新增 `mimofan daemon <id> <cmd...>` 与 `mimofan session {attach,list,kill}`。独立守护进程持有 `portable_pty` PTY + Unix 套接字 `~/.mimofan/run/<id>.sock`，客户端代理 stdin/stdout、支持 Ctrl-] 分离而不杀进程，可 reattach；PTY 输出经 `broadcast` 总线广播给所有已连接客户端。
- **成本优化：Batch API 离线通道 + Prompt Audit + Advisor 顾问策略（[#844](https://github.com/XiaomingX/mimofan/issues/844) / [#846](https://github.com/XiaomingX/mimofan/issues/846) / [#847](https://github.com/XiaomingX/mimofan/issues/847)）**：批量推理通道降本；请求级 prompt 审计与顾问式策略建议。
- **客观 verifier：GoalGate + 工具级 assert_key 运行时化（[#849](https://github.com/XiaomingX/mimofan/issues/849) / [#852](https://github.com/XiaomingX/mimofan/issues/852)）**：目标门禁以客观断言裁决，避免自我报分。
- **结构化事件流 jsonl + replay（[#850](https://github.com/XiaomingX/mimofan/issues/850)）**：事件落盘为 jsonl，可回放复盘。
- **无人值守子集 + headless 门禁 + ConsolidationScheduler 生产接线（[#853](https://github.com/XiaomingX/mimofan/issues/853) / [#863](https://github.com/XiaomingX/mimofan/issues/863) / [#855](https://github.com/XiaomingX/mimofan/issues/855)）**：`--unattended` 安全子集与 headless 准入，consolidation 调度器落地生产。
- **引擎韧性：失败升级重跑 + 预算倒计时 + turn 检查点 + 可序列化状态 + 进度持久化自动恢复（[#845](https://github.com/XiaomingX/mimofan/issues/845) / [#848](https://github.com/XiaomingX/mimofan/issues/848) / [#851](https://github.com/XiaomingX/mimofan/issues/851) / [#856](https://github.com/XiaomingX/mimofan/issues/856) / [#857](https://github.com/XiaomingX/mimofan/issues/857)）**：断点可持久化、崩溃后自动续跑。
- **子代理生命周期与协作（[#864](https://github.com/XiaomingX/mimofan/issues/864) / [#865](https://github.com/XiaomingX/mimofan/issues/865) / [#866](https://github.com/XiaomingX/mimofan/issues/866) / [#867](https://github.com/XiaomingX/mimofan/issues/867) / [#804](https://github.com/XiaomingX/mimofan/issues/804) / [#842](https://github.com/XiaomingX/mimofan/issues/842)）**：生命周期状态机、阻塞等待、停滞检测、通知原语、独立反驳者 `adversarial_verify`、多 agent 并发文件级冲突防护收口。
- **herdr 借鉴：运行时自协调技能 + 环境守卫 + 记忆多会话恢复样本（[#868](https://github.com/XiaomingX/mimofan/issues/868) / [#860](https://github.com/XiaomingX/mimofan/issues/860)）**。
- **工具级权限策略按 ToolCapability 裁决（[#854](https://github.com/XiaomingX/mimofan/issues/854)）**：`deny_capability` / 限网络等细粒度裁决。
- **exec `--json-schema` 终态约束 + 合成终结工具（[#824](https://github.com/XiaomingX/mimofan/issues/824)）**。
- **LSP callHierarchy 入/出调用递归展开（[#827](https://github.com/XiaomingX/mimofan/issues/827)）**。
- **请求级 trace_id 透传至工具执行与子 agent 派生（[#799](https://github.com/XiaomingX/mimofan/issues/799)）**。
- **记忆 + 可观测：周期合并触发 + `/metrics` 与 GenAI span + 用户画像免衰减（[#829](https://github.com/XiaomingX/mimofan/issues/829) / [#830](https://github.com/XiaomingX/mimofan/issues/830) / [#831](https://github.com/XiaomingX/mimofan/issues/831)）**。
- **compaction / 子代理：objective 回灌防漂移 + 文件级互斥划分器（[`6c5dac2`](https://github.com/XiaomingX/mimofan/commit/6c5dac2)）**。
- **验收样本（[#858](https://github.com/XiaomingX/mimofan/issues/858) / [#859](https://github.com/XiaomingX/mimofan/issues/859) / [#861](https://github.com/XiaomingX/mimofan/issues/861) / [#862](https://github.com/XiaomingX/mimofan/issues/862)）**：loop/stop、权限边界、崩溃恢复、验证信号的验收用例。

### Changed
- **`mimofan` 工具集中接线（[#846](https://github.com/XiaomingX/mimofan/issues/846) / [#847](https://github.com/XiaomingX/mimofan/issues/847) / [#850](https://github.com/XiaomingX/mimofan/issues/850) / [#868](https://github.com/XiaomingX/mimofan/issues/868)）**：prompt_audit / advisor / replay / event_stream / herdr skill 统一注册到 registry。

### Fixed
- **Provider CircuitBreaker 接入 fallback readiness 过滤（[#795](https://github.com/XiaomingX/mimofan/issues/795)）**：熔断状态纳入 fallback 就绪判定，避免选中不可用 provider。
- **PTY 守护进程 SHUTDOWN 终止路径（[#869](https://github.com/XiaomingX/mimofan/issues/869)）**：清理阶段用 `clone_killer()` 句柄 `kill` 子进程，确保收到 SHUTDOWN 后守护进程正常退出（此前会永久阻塞在 `child.wait()`）。

## [0.0.17] - 2026-08-14

本轮（LoopX 续作）补齐研究编排范式全链路命令壳、GitHub 共同作者署名，并大规模落地安全审计门、浏览器/反向 MCP/ACP 协议扩展、记忆混合检索与跨会话推理、子代理 DAG 编排与循环守卫持久化等能力。

### Added
- **可机评优化回路 `/evolve`（[#751](https://github.com/XiaomingX/mimofan/issues/751)）**：新增 `/evolve <goal>` 命令（别名 `/optimize`），进入 Agent 模式运行优化回路——锁定 baseline + evaluator（`evolve::lock_baseline`，拒绝覆盖、防篡改）、派发候选、由外部 evaluator 程序裁决 `valid && improved`（`evolve::run_evaluator_on` / `EvaluatorOutput::is_winner`）、胜出者经 `evolve::record_candidate` 留痕并作为下一轮父本。evaluator 拥有正确性，代理不自我报分。
- **可复现性纪律 `/repro`（[#754](https://github.com/XiaomingX/mimofan/issues/754)）**：新增 `/repro <brief>` 命令（别名 `/reproducibility` `/brief`），固化 `BRIEF.md` 唯一事实源 + `env_snapshot.json` 环境快照（rust/python 版本与依赖锁哈希）+ `provenance.jsonl` 起点留痕（`repro::write_brief` / `snapshot_env` / `record_provenance`），默认零行为变更。
- **研究成果物汇总 `/artifact`（[#750](https://github.com/XiaomingX/mimofan/issues/750)）**：新增 `/artifact <initiative_id> [--publish]` 命令（别名 `/publish`），进入 Agent 模式调用 `research_artifact::ArtifactInput::build` 汇总到 `initiatives/<id>/`（README.md + provenance.json）。`--publish` 走研究副作用闸门（[#753](https://github.com/XiaomingX/mimofan/issues/753)），`PublishRemote` 默认需显式授权，Auto 不自动推远程。
- **独立评审者 `/reviewer`（[#752](https://github.com/XiaomingX/mimofan/issues/752)）**：新增 `/reviewer [<initiative_id>]` 命令（别名 `/review`），只读审核 claim，调用 `reviewer::review` / `accepted_only` 下 Accepted/Rejected/Weak 判定（被反驳直接 Rejected；Strong 且未反驳 / Medium 有复现步骤且未反驳 → Accepted），作为 `/artifact` 公开章节的前置门。
- **GitHub 共同作者署名（对标 Claude Code `includeCoAuthoredBy`）**：`git_commit` 工具默认在 commit message 末尾追加 mimofan 共同作者 trailer（`🤖 Generated with [mimofan]` + `Co-Authored-By: mimofan <noreply@xiaoming.com>`），新增 `co_authored_by` 参数（默认 `true`，可关闭），并防重复追加（amend 安全）。GitHub 据此把 mimofan 显示为 co-author，真实 committer 不变。

### Docs
- 新增 `docs/NEW_CAPABILITIES_GUIDE.md`，介绍 `/evolve` `/repro` `/artifact` `/reviewer` 与 GitHub 共同作者署名的使用方法与配置方式。

### Security
- **AUTO 权限分类器两阶段 + fail-closed（[#730](https://github.com/XiaomingX/mimofan/issues/730)）**：新增 `auto_classifier`（`classify_tool_call` / `classify_with_timeout`），未知工具默认落入 mutating（拒绝）分支，验证见 `unknown_tools_fail_closed_to_mutating`。
- **输入侧提示注入扫描（[#723](https://github.com/XiaomingX/mimofan/issues/723)）**：新增 `prompt_injection` 模块，在工具调用/输入侧扫描提示注入。
- **skill 供应链 provenance + 隔离审计（[#731](https://github.com/XiaomingX/mimofan/issues/731)）**：`skills/provenance.rs` 提供 `build_provenance_lock`（来源→隔离级别）与 `audit`（第三方 skill 能力越权结构审计），含 `IsolationLevel` / `ProvenanceLock`。
- **内容密钥扫描 / 流式脱敏 / 路径穿越防护（[#718](https://github.com/XiaomingX/mimofan/issues/718) / [#680](https://github.com/XiaomingX/mimofan/issues/680) / [#648](https://github.com/XiaomingX/mimofan/issues/648)）**：新增 `crates/secrets`（`scanner.rs` 多类密钥识别 + `lib.rs` 流式 redaction 原语）与 `path_traversal` guard。
- **命令注入 fuzz 覆盖（[#640](https://github.com/XiaomingX/mimofan/issues/640)）**：`execpolicy` 新增 133 行命令注入 fuzz 测试。
- **execpolicy 路径安全内核 `path_guard`（[#681](https://github.com/XiaomingX/mimofan/issues/681)）**：`path_guard.rs` 词法判定 `PathVerdict`（InsideWorkspace/EscapesWorkspace/Sensitive/NotFound），敏感文件清单 + 穿越防护，`resolve_path` 委托内核。
- **trust_mode 与路径边界解耦（[#733](https://github.com/XiaomingX/mimofan/issues/733)）**：受信模式不再跳过工作区边界校验，仅跳过审批弹窗（回归测试 `trust_mode_still_enforces_workspace_boundary`）。
- **命令注入绕过 + SAST 死枝/空 prompt 修复（[#756](https://github.com/XiaomingX/mimofan/issues/756) / [#715](https://github.com/XiaomingX/mimofan/issues/715) / [#670](https://github.com/XiaomingX/mimofan/issues/670)）**：修复命令注入绕过与 SAST 死枝/空 prompt 路径。

### Tools & Protocol
- **浏览器自动化工具（[#743](https://github.com/XiaomingX/mimofan/issues/743)）**：新增 `tools/browser.rs`，支持 `navigate`/`click`/`type`/`screenshot`/`eval_js`，复用 SSRF guard。
- **反向 MCP server 接线到 CLI（[#746](https://github.com/XiaomingX/mimofan/issues/746)）**：`mcp_server/mod.rs` 提供 `run_mcp_server` + `expose_tools`，作为 CLI 子命令暴露工具面。
- **ACP 能力矩阵扩展（[#745](https://github.com/XiaomingX/mimofan/issues/745)）**：`acp_server/mod.rs` 新增 `session/list`、image injection、embedded_context 渲染与 MCP 工具代理。
- **用量 / 成本 / 工具分析洞察（[#744](https://github.com/XiaomingX/mimofan/issues/744)）**：`tools/insights.rs` 的 `InsightsAggregator` 按 tool/session/model 聚合 token 与成本，`InsightsTool` 暴露。
- **聚合工具调用度量（[#734](https://github.com/XiaomingX/mimofan/issues/734)）**：引擎新增 `tool_call_duration` 累计与 count 聚合。
- **有效上下文利用率度量（[#735](https://github.com/XiaomingX/mimofan/issues/735)）**：`context_budget` 新增 `effective_utilization` 与利用率分级枚举。
- **结构化输出工具 `syntheticOutput`（[#729](https://github.com/XiaomingX/mimofan/issues/729)）**：无依赖 JSON-Schema 子集校验（`validate_against_schema`），失败带 feedback 重试。
- **`/share --local` 本地导出（[#688](https://github.com/XiaomingX/mimofan/issues/688)）**：`share.rs` 支持 `-l` 将会话写本地 `.md`。

### Memory
- **用户建模层 `UserProfile`（[#732](https://github.com/XiaomingX/mimofan/issues/732)）**：`user_profile.rs` 数据结构 + JSON 持久化 + 会话提炼 `distill_session` + 注入渲染。
- **consolidation 模块 + 重要性评分（[#716](https://github.com/XiaomingX/mimofan/issues/716) 切片 A）**：`consolidation.rs` 的 `MemoryEntry.importance`、`record_access`（封顶 1.0）、`decay_importance`、`evict_to_budget`。
- **Embedder trait 抽象（[#712](https://github.com/XiaomingX/mimofan/issues/712)）**：`embedding.rs` 抽象 `pub trait Embedder`，`EmbeddingService` 持有 `Arc<dyn Embedder>`。
- **混合检索 RRF + score_breakdown（[#714](https://github.com/XiaomingX/mimofan/issues/714) 切片 A/B）**：`codebase.rs` 的 `fuse_retrieval_hits` + `RetrievalHit.score_breakdown`（多源贡献可解释），`RetrievalSource::Vector` 纳入四路融合，统一召回载体。
- **hybrid_bm25 关键词召回（[#777](https://github.com/XiaomingX/mimofan/issues/777) / [#778](https://github.com/XiaomingX/mimofan/issues/778)）**：`vector.rs` 的 `hybrid_bm25` 用 RRF 融合 lexical + vector，真正召回关键词命中项。
- **跨会话推理 `session_id`（[#777](https://github.com/XiaomingX/mimofan/issues/777)）**：Observation 打 `session_id` 标签，`reassemble_session_timeline` 按会话重组时间线，注入加 `[session X]` 来源标注。
- **记忆可观测快照 `MemoryStats` + `/status` 接线（[#628](https://github.com/XiaomingX/mimofan/issues/628)）**：`memory_stats.rs` 的 `compute_memory_stats` 接 `/status`。
- **LongMemEval 记忆评测 harness（[#777](https://github.com/XiaomingX/mimofan/issues/777)）**：`examples/longmemeval_ingest.rs` Rust 接入二进制 + Python 打分脚本。
- **edit/apply 首试成功率基准（[#689](https://github.com/XiaomingX/mimofan/issues/689)）**：polyglot 统一 diff 基准，断言首试成功率 ≥ 0.9（实测 8/8）。
- **compaction 事实保留率断言（[#629](https://github.com/XiaomingX/mimofan/issues/629)）**：`compaction/mod.rs` 的 `fact_retention_rate` 防静默丢事实。

### Subagent & Orchestration
- **DAG 任务编排 + 结构化 plan 维度（[#79f8501](https://github.com/XiaomingX/mimofan/commit/79f8501)）**：`tools/subagent/task_graph.rs` 的 `run_task_graph` 校验图后按 wave 并发执行，失败节点跳过下游依赖（失败传播）。
- **`fork_turns` 窗口裁剪（[#702](https://github.com/XiaomingX/mimofan/issues/702)）**：spawn 子代理时裁剪历史窗口。
- **`task_shell_stop`（[#776](https://github.com/XiaomingX/mimofan/issues/776)）**：对齐后台任务 start/wait 生命周期（对称契约测试）。
- **后台 shell 完成注入 `<task-notification>`（[#696](https://github.com/XiaomingX/mimofan/issues/696)）**：完成的后台 shell 包装为运行事件进入会话。
- **pre-turn 快照 fire-and-forget（[#643](https://github.com/XiaomingX/mimofan/issues/643)）**：消除回合起始阻塞。
- **多目标 `GoalQueue`（[#654](https://github.com/XiaomingX/mimofan/issues/654)）**：单例 `GoalState` 升级为 `SharedGoalQueue`。
- **sidebar 与 `ui_event_loop` 拆分（[#647](https://github.com/XiaomingX/mimofan/issues/647)）**：原 3008 行 `sidebar.rs` 拆为按面板子模块；事件循环安全迁移拆分为 free functions。
- **循环守卫跨回合持久化 + 崩溃恢复 + 日志脱敏（[#9b20ea3](https://github.com/XiaomingX/mimofan/commit/9b20ea3)）**：`LoopGuardSnapshot` 可序列化、跨回合累积 suspicion、持久化磁盘、日志脱敏。
- **循环守卫两维度（[#694](https://github.com/XiaomingX/mimofan/issues/694)）**：新增 `StreamingRepetition` + `SemanticEcho` 检测，接 `observe()` 带每模式 nudge 上限。
- **bus + `task_claims` 接线（[#699](https://github.com/XiaomingX/mimofan/issues/699)）**：`AgentBus` / `SharedTaskClaimManager` 实际 attach 到 `SubAgentRuntime`。

### Model & Routing
- **catalog 实时刷新 + 磁盘持久化（[#3385](https://github.com/XiaomingX/mimofan/issues/3385) / [#787](https://github.com/XiaomingX/mimofan/issues/787)）**：`ProviderCatalogCache` JSON round-trip，App 进程级缓存落 `~/.mimofan/catalog_cache.json`，`refresh_catalog_cache` 真实接线。
- **AUTO 路由分类器 token 成本上报（[#692](https://github.com/XiaomingX/mimofan/issues/692)）**：`inventory_auto_router` 调用后上报成本到 `cost_status`。
- **prefix-cache 命中率（[#646](https://github.com/XiaomingX/mimofan/issues/646)）**：`prefix_cache_hit_rate`（`cached_tokens/(input+cached)`）+ 累计命中。

### Platform & Observability
- **`mimofan-telemetry` crate，feature-gated OTel 桥接（[#726](https://github.com/XiaomingX/mimofan/issues/726) 切片 A）**：默认 inert（`init_otel` 返回 `Disabled`），`otlp` feature 才起 OTLP exporter 桥接 `tracing`。
- **调用图可达性骨架（[#598](https://github.com/XiaomingX/mimofan/issues/598) 切片 A）**：`staticanalysis/src/callgraph.rs` 的函数提取 + 同文件调用边 + worklist `reachable_from`。
- **自动化 Webhook 投递（[#671](https://github.com/XiaomingX/mimofan/issues/671) / [#775](https://github.com/XiaomingX/mimofan/issues/775)）**：`MIMOFAN_AUTOMATION_WEBHOOK_URL` 设置时 `WebhookHookSink` 接自动化完成事件。

### TUI / UX
- **首屏欢迎页 redesign + 美化（[#2074c35](https://github.com/XiaomingX/mimofan/commit/2074c35) / [#067c9b9](https://github.com/XiaomingX/mimofan/commit/067c9b9) / [#934321a](https://github.com/XiaomingX/mimofan/commit/934321a)）**：居中品牌卡、清理过时 TODO、修复 auth status provider 判定。
- **HNSW 删除残影修复（[#934321a](https://github.com/XiaomingX/mimofan/commit/934321a)）**：修复删除后残留 ghost 命中，清理废弃 API。

---

Older releases: [CHANGELOG.md](https://github.com/XiaomingX/mimofan/blob/main/CHANGELOG.md) and [docs/CHANGELOG_ARCHIVE.md](https://github.com/XiaomingX/mimofan/blob/main/docs/CHANGELOG_ARCHIVE.md).
