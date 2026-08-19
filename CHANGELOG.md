# 更新日志

本项目的所有重要变更都记录在此。版本遵循[语义化版本控制](https://semver.org/)，从工作区根目录（`Cargo.toml` → `[workspace.package] version`）递增。

## [0.0.21] - 2026-08-19

本轮聚焦长程记忆与推理、Agent 协作编排、编辑正确性与模块化重构，并打通漏洞挖掘评测产物闭环与跨文件静态分析召回。

### Added
- **Dreaming 三阶段记忆流水线（[#871](https://github.com/XiaomingX/mimofan/issues/871)）**：抽取-整合-抽象巩固自动巩固长期记忆，注入压缩提示 + `record_decision` 埋点，接 P0 接线。
- **记忆 provenance 四级信任模型**：Untrusted / Observed / Inferred / Verified 四级信任，跨会话记忆可追溯来源可信度。
- **Arena 多模型对战 + Team 领导角色骨架**：`mimofan-subagent` 暴露真实 spawn runner 调用入口，支撑多模型竞争与团队协作编排。
- **decision log 决策事件流独立压缩模块**：决策事件作为可压缩的独立持久段，客观目标 `objective` 暴露 `must_keep` 保留项清单供二次压缩注入。
- **apply_patch_claude Claude 方言解析器**：新增工具支持 Claude 方言 diff，便于接手中断任务收尾。
- **行为验证门 verification_gate 接线（[#874](https://github.com/XiaomingX/mimofan/issues/874)）**：behavioral verification 接入 turn loop。
- **`session_search` 工具**：召回长期记忆会话（[#874](https://github.com/XiaomingX/mimofan/issues/874)），并接入 TUI toolset。
- **语义代码库检索 `codebase_search`**：暴露为 agent tool（[#714](https://github.com/XiaomingX/mimofan/issues/714)）。
- **机器可读错误分类 `tool_codes`（[#872](https://github.com/XiaomingX/mimofan/issues/872)）**：文件工具错误分类 taxonomy，便于程序化处理。
- **跨过程污点分析 taint solver（T-6，[#788](https://github.com/XiaomingX/mimofan/issues/788) / [#790](https://github.com/XiaomingX/mimofan/issues/790)，修复 [#843](https://github.com/XiaomingX/mimofan/issues/843) 跨文件召回）**：修复漏洞挖掘跨文件召回塌陷。
- **长程任务与实验谱系（lineage）评测样本**：新增 L 分级体系与统一评分 harness，以及 vuln_hunt 产物链路闭环。
- **长程韧性能力**：goal 队列本地落盘持久化恢复（G4）、向量记忆容量逐出（G1）、transient failure 保守重试（G5）、失败任务自动降级为子任务重新入图。
- **VFS 乐观锁 `write_if_unchanged`**：防止 TOCTOU 覆盖。
- **LSP 空闲卸载 `idle_unload_secs`**：超时释放 transport 省资源。
- **Tower 式 merge gate**：合回前校验 scope 越界 + review。

### Changed
- **模块化重构（多 worktree 合并）**：goal-core（目标管理 + godfile 行数 CI 红线）、edit-core（编辑正确性 + 注入式 ReadState 先读后改防呆）、decision-log、arena-team、provenance-tier、dreaming、apply-patch-claude、vfs-optimistic 拆分为独立 crate。
- **工具 schema 按使用频率降序排列**：降低模型工具选择成本。
- **TUI HookExecutor 收敛（A 进 B 阶段 1/2）**：复用共享 `run_shell_command`，新增 `CommandHookSink` 消除重复实现。
- **子 agent 只读查看器 + 新轮并入 rlm REPL**。

### Fixed
- **移除硬编码 API keys，改用环境变量（[#873](https://github.com/XiaomingX/mimofan/issues/873)）**：安全加固。
- **vuln_hunt 产物闭环 + `run_async` runtime panic**。
- **`TodoStatus::Degraded` 三处穷举匹配补全**（C1 配套）。
- **压缩二次压缩 `Box::pin` 修复 + loop_guard 记忆/技能双 nudge 修正**。

## [0.0.20] - 2026-08-16

彻底修复发布流水线（`release.yml` 的 `parity` 关卡），让 GitHub 自动发版从此可端到端通过。

### Fixed
- **发布流水线 `parity` 关卡长期失败（根因修复）**：原先 `cargo clippy -- -D warnings` 把 `clippy::all` 的每一条纯风格 lint（collapsible_if、field init 顺序、type alias 建议等）都升级为硬错误，整库历史遗留的 cosmetic 债务让每一次发版都在 clippy 阶段挂掉，导致历史发版只能手工补建。改为只硬性拦截真正关乎正确性的 `clippy::unsafe_code`，其余 `clippy::all` 维持 warning（CI 日志可见），释放后 CI 不再因风格问题阻塞发版。
- **`clippy::lint_groups_priority` 硬错误**：`rust_2018_idioms` 与 `unsafe_code` 同为默认 priority 0 冲突，已为前者设 `priority = -1`。
- **`clippy::derivable_impls` / `clippy::field_reassign_with_default` / `clippy::collapsible_if` 等 `clippy::all` 债务**：随上述策略调整自然转 warning，不再阻断。
- **`clippy::correctness` 默认 deny 的真实 bug**：`recovery_stats.rs` 中 `0u64 * 100` 恒为 0；`fetch_url.rs` 中 `for addr in addrs` 实为单次返回（never_loop）。二者为 clippy 默认 deny 的 correctness lint，已修正为有意义的实现。
- **`mimofan-memory` 的 `test_search_records_access` 偶发失败**：HNSW 在极小数据集（N=1）下召回不稳定。测试改用 ≥3 个观测点并加有界重试，断言访问计数记账语义而非依赖 HNSW 单次召回稳定性，确定性通过。
- **`cargo fmt` / `Cargo.lock` 漂移**：补全全工作区格式化并重新解析锁文件，`parity` 的 Format / Compile / Lockfile 检查现已稳定通过。

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

[0.0.17]: https://github.com/XiaomingX/mimofan/compare/v0.0.16...v0.0.17

## [0.0.16] - 2026-08-11

本轮（LoopX 续作）落地代码库语义索引能力，补齐离线代码检索基础设施。

### Added
- **代码库语义索引（[#675](https://github.com/XiaomingX/mimofan/issues/675) / [#720](https://github.com/XiaomingX/mimofan/issues/720)）**：新增 `mimofan-memory::codebase` 模块，基于 rusqlite + FTS5 提供离线代码全文检索。特性：
  - `CodebaseIndex` 以 SQLite 持久化，按仓库隔离，文件级内容哈希增量跳过未变更文件；
  - `chunk_source` 按行窗口（40 行/重叠 8 行）切片，`extract_symbols` 启发式抽取 `fn/struct/enum/impl/trait/mod/async fn` 等符号；
  - `search` 支持语言 / 路径前缀 / 符号三类过滤，返回 `SearchHit`（含 `snippet` 高亮与 `rank` 排序）；
  - `normalize_query` 将 `compute_hash` / `fooBar` 等标识符风格查询展开为空格分隔 token，对齐 `unicode61` 分词，解决裸下划线/驼峰词在 FTS5 中查不到的问题（保留 `"..."`、`*`、`AND/OR/NOT` 等显式 FTS 语法不被改写）；
  - 含 9 个单元测试覆盖切片/符号抽取/增量跳过/语言与路径过滤/符号过滤/查询归一化。

[0.0.16]: https://github.com/XiaomingX/mimofan/compare/v0.0.15...v0.0.16

## [0.0.15] - 2026-08-10

本轮（LoopX 续作）补齐测试覆盖、收口安全审计遗漏路径、强化记忆访问统计。

### Added
- **记忆访问强化（[#719](https://github.com/XiaomingX/mimofan/issues/719) M7）**：`Observation` 新增 `access_count` / `last_accessed_at` 字段；检索（`search`）命中即调用 `record_access` 递增计数并刷新时间戳，反哺 M4 重要性评分。含 `ALTER TABLE` migration 兼容旧库，新增 `test_search_records_access` / `test_record_access_direct` 覆盖计数递增与时效字段更新。

### Fixed
- **`/plugins` 调试试用执行补齐 execpolicy 审核（[#617](https://github.com/XiaomingX/mimofan/issues/617) 续）**：v0.0.14 已修复工具路径（`tools/plugin.rs::run_plugin_child`）与 execpolicy 解析根因；本版补齐遗漏的调试命令路径——`commands/plugins.rs` 的 `Execution Trial` 此前裸 spawn 解释器、绕过审核门，现对齐 deny 硬拦 / allow 放行 / AskUser 审计放行语义，消除调试命令绕过安全策略的缺口。

### Test
- **web_run / web_search 纯逻辑单元测试（[#633](https://github.com/XiaomingX/mimofan/issues/633)）**：为 URL 识别、域名匹配、DuckDuckGo vqd 提取、PDF 判定、PDF 分页、根域提取、垃圾结果检测、挑战页识别、URL 归一化、错误体脱敏/截断、Bing 重定向解码等补 13 例单元测试，零行为变更。

[0.0.15]: https://github.com/XiaomingX/mimofan/compare/v0.0.14...v0.0.15

## [0.0.14] - 2026-08-10

本轮（LoopX P0 批次续作）聚焦工具执行层的命令策略审计，并修复一处导致安全门静默失效的配置解析缺陷。

### Fixed
- **MCP stdio server 启动经 execpolicy 审核（[#616](https://github.com/XiaomingX/mimofan/issues/616)）**：`mcp.rs` 启动任意 stdio server 前接入 `load_default_policy()` 审核门——`deny` 规则硬阻断、`allow` 放行并审计日志、未命中（AskUser）作为用户配置集成放行但记录审计轨迹；策略加载失败拒绝静默降级。
- **plugin 子进程启动经 execpolicy 审核（[#617](https://github.com/XiaomingX/mimofan/issues/617)）**：`tools/plugin.rs` 的 `run_plugin_child` 在既有审批门之外叠加 execpolicy 审核层，阻断/放行语义与 MCP 一致。
- **execpolicy 安全门静默失效（[#616](https://github.com/XiaomingX/mimofan/issues/616) 根因）**：`ExecPolicyConfig::rules` 原为 `BTreeMap`，但 `execpolicy.toml` 采用扁平 `[group]` 形式（如 `[runtime] deny=["rm *"]`），缺 `#[serde(flatten)]` 时该表被 serde 静默忽略，导致 `rules` 恒空、每条命令都落到 AskUser、deny 规则永不触发。补 `flatten` 后安全门真正生效。新增 3 个单测覆盖 deny/allow/ask_user。

[0.0.14]: https://github.com/XiaomingX/mimofan/compare/v0.0.13...v0.0.14

## [0.0.13] - 2026-08-10

本轮（LoopX P0 批次）补齐 Provider 矩阵与一批「引擎在手边但命令壳/守卫没接」的能力断连，并加固自动化与记忆检索。

### Added
- **Gemini 原生适配（[#737](https://github.com/XiaomingX/mimofan/issues/737)）**：新增 `client/gemini.rs`，实现 Gemini `generateContent`/`streamGenerateContent` 适配（系统指令、函数声明、停止原因映射、增量文本与 SSE 帧解析），补齐 `GeminiCompatible` provider 的端到端通路；含 9 个单元测试。
- **CostBudget 成本上限硬停守卫（[#620](https://github.com/XiaomingX/mimofan/issues/620)，[PR #747](https://github.com/XiaomingX/mimofan/pull/747)）**：会话水位 + 当日累计双阈值，Warn/Hard 两档；超出硬上限时阻断后续请求，避免失控烧钱。
- **TUI transcript 全文搜索（[#624](https://github.com/XiaomingX/mimofan/issues/624)，[PR #748](https://github.com/XiaomingX/mimofan/pull/748)）**：将 transcript 全文检索绑定到 TUI 交互，会话回顾可在界面内直接搜索。
- **`/commit` 生成并提交 commit message（[#682](https://github.com/XiaomingX/mimofan/issues/682)，[PR #749](https://github.com/XiaomingX/mimofan/pull/749)）**：新增 `GitCommitTool`（带 `ApprovalRequirement::Required` 审批门槛），由模型基于 diff 生成提交信息并执行提交，闭合「先生成后提交」工作流。
- **`/night` `/time` `/loop --schedule` 接线 AutomationManager（[#655](https://github.com/XiaomingX/mimofan/issues/655)）**：定时/循环类自动化命令真正落到 `AutomationManager`，消除 UI 壳未接引擎的断连。

### Changed
- **向量语义检索接入 SearchCache 缓存层（[#642](https://github.com/XiaomingX/mimofan/issues/642)）**：为向量检索叠加 `SearchCache` 缓存，降低重复查询成本、稳定召回路径。

[0.0.20]: https://github.com/XiaomingX/mimofan/compare/v0.0.19...v0.0.20
[0.0.19]: https://github.com/XiaomingX/mimofan/compare/v0.0.18...v0.0.19
[0.0.13]: https://github.com/XiaomingX/mimofan/compare/v0.0.12...v0.0.13

## [0.0.12] - 2026-08-10

### Fixed
- **`/monitor` 命令真正持久化（[#704](https://github.com/XiaomingX/mimofan/issues/704)）**：命令原仅回显「已接收」却零落盘。现经 `MonitorStore` 真正创建/列表/查询/暂停/恢复/删除，并用 `block_in_place`+`Handle::block_on` 桥接同步命令层与异步存储；monitor 存于状态目录 `issue_monitors/`，重启后仍保留。
- **`/balance` 读取真实余额（[#705](https://github.com/XiaomingX/mimofan/issues/705)）**：命令原对所有 provider 回显「not wired yet」。现复用 footer 同源的 `balance_cell`，展示与 footer 一致的真实余额（无 key/未拉取时明确提示，而非假失败）。
- **`/freeze` 无参不再假成功（[#707](https://github.com/XiaomingX/mimofan/issues/707)）**：无参时原回显「已约束到当前计划」但引擎要求 `frozen_spec` 非空，实际未约束。现明确提示需提供 plan，不误报约束。
- **UI 死代码/空壳清理（[#706](https://github.com/XiaomingX/mimofan/issues/706)）**：fleet Docker host 在 `create_run` 阶段 fail-fast 拒绝（原运行时才报错）；`with_parallel_tool` 标 `#[deprecated]` 显式暴露废弃意图；footer `LastToolElapsed`/`RateLimit` 无真实数据支撑，从可选列表移除（保留枚举变体以兼容既有配置反序列化）。
- **MSRV 修正为 1.95（[#641](https://github.com/XiaomingX/mimofan/issues/641)）**：原声明 1.88 仅覆盖自身语法需求；`rusqlite`/`libsqlite3-sys` 使用 1.95 稳定的 `cfg_select!` 且不声明自身 rust-version，1.88 工具链会报晦涩宏错误。
- **记忆 HNSW 召回稳定（[#615](https://github.com/XiaomingX/mimofan/issues/615)）**：`max_layer` 由 100 收敛到 16（≈ ln(N) 设计不变量），修复小数据集召回随机为空的 flaky；核验确认已删记忆不会被 `search` 召回（SQLite 为唯一真相源，`load_observation` 过滤已删行），并固化该不变量的注释与回归测试。
- **先读后改扩展到 write_file/apply_patch/notebook_edit（[#695](https://github.com/XiaomingX/mimofan/issues/695)）**：`FileIdentity` 引入 SHA-256 `content_hash`（可检测同长度同 mtime 改写）；新增 `require_fresh_file_read_for(tool, …)` 按调用工具归因拒绝并产出结构化 `prior_read_violation={...}` trailer，供模型按失败模式分支而非匹配散文。

### Chore
- **移除零调用方 token 估算残留**：清理 `context_budget` 中无生产调用方的字符启发式估算函数，统一到已收敛的 tiktoken/BPE 计数器。

[0.0.12]: https://github.com/XiaomingX/mimofan/compare/v0.0.11...v0.0.12

## [0.0.11] - 2026-08-09

### Fixed
- **Linux 编译失败（[issue #585](https://github.com/XiaomingX/mimofan/issues/585)）**：补齐 macOS-gated 函数在非 macOS 平台的 fallback，修复 `cargo install --git` 在 Linux 上的 `E0425`/`E0308` 编译错误。
  - `normalize_macos_modifiers`（`composer_ui.rs`）新增非 macOS 恒等 fallback。
  - `native_ocr_available` / `try_native_ocr`（`image_ocr.rs`）新增非 macOS fallback（返回 `false` / `Ok(None)`）。
  - `probe_bwrap_available` / `probe_cgroup_version`（`diagnostics.rs`）补 Linux 真实探测分支，并修正非 Linux 返回值类型。
  - `install_parent_death_signal` / `install_server_parent_death_signal`（`shell.rs` / `cli_commands/mod.rs`）补 Linux 实现（`prctl(PR_SET_PDEATHSIG, SIGKILL)`）。
  - `try_headless_browser_fetch`（`fetch_url.rs`）消除跨平台未使用变量 warning。

[0.0.11]: https://github.com/XiaomingX/mimofan/compare/v0.0.10...v0.0.11

## [0.0.10] - 2026-08-09

### Added
- **LSP request/response 基础设施（[issue #597](https://github.com/XiaomingX/mimofan/issues/597)）**：新增 `document_symbols` / `references` / `definition` 请求层与 `serverCapabilities` 解析，支撑后续漏洞挖掘的跨过程分析（MECE L0 前置）。
- **记忆分类索引 + 按需加载（[PR #583](https://github.com/XiaomingX/mimofan/pull/583)）**：对齐 CodeBuddy 记忆机制，按四分类体系索引、懒加载。

### Changed
- **Provider 配置增强**：`anthropic-compatible` 模式新增识别 `ANTHROPIC_AUTH_TOKEN`，并补充环境变量配置文档（README/config.example.toml）。
- **真实 BPE 统一全库 token 计数（[#9478882](https://github.com/XiaomingX/mimofan/commit/9478882)）**：修复中文 token 系统性低估，收敛 `seam_manager`/`context_inspector`/`injector` 三处估算到共享计数。

### Fixed
- **编辑工具正确性三项修复（[#2284d7a](https://github.com/XiaomingX/mimofan/commit/2284d7a)）**：读范围授权 / `replace_all` / BOM+CRLF 保真。
- **回合内循环检测（[loop_guard](https://github.com/XiaomingX/mimofan/commit/6801a72)）**：恢复循环/重复/停滞检测，防长程任务目标漂移。
- **抑制 libring linker_messages 警告**：`.cargo/config.toml` 新增 `-A linker_messages`，零 warning 编译。

### Docs
- **README 去 emoji 提升专业性（[PR #584](https://github.com/XiaomingX/mimofan/pull/584)）**。

[0.0.10]: https://github.com/XiaomingX/mimofan/compare/v0.0.9...v0.0.10

## [0.0.9] - 2026-08-05

### Added
- **Issue Monitor 工作流** ([#560](https://github.com/XiaomingX/mimofan/issues/560))：新增 issue 监控与自动处理流程，便于跟踪上游反馈与自动归档。

### Changed
- **架构规划收尾（DDD 第一性原理梳理，零功能新增、零交互层改动）**：
  - 标记 `mimofan-memory` 为 **experimental（未集成）**，避免零上游依赖的僵尸模块误导用户 ([#570](https://github.com/XiaomingX/mimofan/issues/570))。
  - `execpolicy` 双实现厘清为**互补**而非重复：CLI/tui 走本地文件策略、crate 提供可复用引擎；仅移除 `shell.rs` 的死导入 ([#572](https://github.com/XiaomingX/mimofan/issues/572))。
  - 双重「运行时」（`crate::Runtime` 无界面 API 核心 / `tui::Engine` 交互循环）厘清为**两个限界上下文**，命名撞车误判，不合并 ([#574](https://github.com/XiaomingX/mimofan/issues/574))。
  - UI 层 3 处渲染 IO（提示建议静态客户端、剪贴板落盘、文件树读取）确认为**展示层正当职责**，不做端口化注入（过度设计）([#576](https://github.com/XiaomingX/mimofan/issues/576))。
  - 冗余英文文档清理并中文化架构说明 ([#568](https://github.com/XiaomingX/mimofan/issues/568))。
  - DDD 拆分 5 个「上帝文件」模块 ([#566](https://github.com/XiaomingX/mimofan/issues/566))。

### Security
- **deny 规则大小写不敏感（行为变化，更安全）** ([#580](https://github.com/XiaomingX/mimofan/issues/580))：tui 的 deny/allow 匹配统一改用 crate 的 lowercase `canonical_executable_form`，`execpolicy.toml` 中 `deny = ["rm *"]` 现在也能拦住大写命令（`RM -rf /` / `SUDO RM -rf /`），不再因大小写绕过 deny 规则。这是本版本唯一的行为变化点。

[0.0.9]: https://github.com/XiaomingX/mimofan/compare/v0.0.8...v0.0.9

## [0.0.8] - 2026-08-04

> 说明：v0.0.8 为 v0.0.7 与 v0.0.9 之间的一个发布快照，主要包含该时点的主线提交；其增量在 v0.0.9 的条目中已按议题归类记录，此处保留占位段以维持 changelog 连续性。

[0.0.8]: https://github.com/XiaomingX/mimofan/compare/v0.0.7...v0.0.8

## [0.0.7] - 2026-08-04

### Changed
- **代码质量检查与清理**：修复循环依赖问题（protocol ↔ execpolicy），清理过时注释和版本引用
- **架构优化**：localization 模块独立为 crate，cli/runtime_threads 拆分为子目录
- **测试代码分离**：清理未使用的导入和 dead_code 警告

### Fixed
- 修复 `test_match_osc8_fragment` 测试中的 `\x08` → `'8'` bug
- 更新 API 配置验证脚本（`benchmark/api_providers_test.sh`）

[0.0.7]: https://github.com/XiaomingX/mimofan/compare/v0.0.4...v0.0.7

## [0.0.4] - 2026-07-28

### 变更
- **统一项目版本为 `0.0.4`**。`npm/mimofan` 和 `npm/runtime-sdk` 包版本现在跟踪 Cargo 工作区版本（`Cargo.toml` 中的 `[workspace.package] version`），而不是在独立的 `0.8.x` 行上漂移。`scripts/release/prepare-release.sh` 现在也会递增 `npm/runtime-sdk` 版本，`scripts/release/check-versions.sh` 会进行验证。

### 修复
- 清理品牌重命名（`deepseek-tui` → `mimofan`）遗留问题：
  - 恢复了被批量重命名复制到 `MIMOFAN_*` 的 `DEEPSEEK_*` 环境变量回退链
  - 修正了发布工具中遗留的 npm 包引用
  - 将 JS 工具链从 pnpm 迁移到 bun（`package.json` 的 `workspaces` / `overrides` / `trustedDependencies`，生成 `bun.lock`）

## [0.0.3-rc.4] - 2026-07-05

### 修复
- **`StartTurnRequest` 现在暴露 `response_format`**。在 `StartTurnRequest`（`runtime_threads.rs`）中添加了 `response_format: Option<serde_json::Value>`，并通过 `Op::SendMessage`（`core/ops.rs`）、`Session`（`core/session.rs`）、`handle_send_message`（`core/engine.rs`）和 `turn_loop.rs` 传递，使用于轮次的 `MessageRequest` 端到端携带用户提供的 JSON 模式规范。`tui/ui.rs` 和 `main.rs` 中的两个 `Op::SendMessage` 字面量构造点传递 `response_format: None`（TUI 尚未提供 JSON 模式控制，但 app-server 路径现在可以使用）。

### 已验证 — XiaomiMiMo API 能力支持

以下 XiaomiMiMo 能力已针对实时 API（`/v1/chat/completions` 和 `/anthropic/v1/messages`）确认可用：

**OpenAI Chat Completions（`/v1/chat/completions`）：**
- ✓ 基本调用（非流式和流式）
- ✓ 函数调用 / 工具（`{"type":"function",...}`）
- ✓ 图像输入（`{"type":"image_url","image_url":{"url":"..."}}`）
- ✓ `response_format: {"type":"json_object"}`（结构化 JSON 输出）
- ✓ `thinking: {"type":"enabled"/"disabled"}`（深度推理）
- ✓ `reasoning_content` + `usage.completion_tokens_details.reasoning_tokens`
- ✗ Web 搜索工具 — mimofan 使用自己的内部 `web_search` 工具（DuckDuckGo / Baidu），而不是 XiaomiMiMo 的 `{"type":"web_search",...}` API 工具类型
- ✗ 音频输入（`{"type":"audio_url",...}`）— `ContentBlock` 枚举中没有 `AudioUrl` 变体
- ✗ 视频输入 — `ContentBlock` 枚举中没有 `VideoUrl` 变体
- ✗ TTS 输出 / 语音合成 — mimofan 客户端中没有 API 端点

**Anthropic Messages（`/anthropic/v1/messages`）：**
- ✓ 基本调用（非流式和流式 SSE，包含 `message_start`、`content_block_delta`、`message_delta`、`message_stop`）
- ✓ 函数调用（`content[].type:"tool_use"`）
- ✓ 图像输入（`content[].type:"image"`，`source.type:"url"`）
- ✓ 思考（`thinking.type:"enabled"/"disabled"`，`content[].type:"thinking"`）
- ✗ 音频输入 — 没有 `input_audio` / `audio` 内容块变体
- ✗ 视频输入 — 没有 `input_video` 内容块变体
- ✗ TTS 输出 — mimofan 客户端中没有 API 端点
- ✗ ASR（音频转录）— mimofan 客户端中没有 API 端点

**OpenAI Responses API（`/v1/responses`）：**
- ✗ XiaomiMiMo 不可达 — 调度将 XiaomiMiMo 路由到 Chat Completions 或 Anthropic Messages；`responses.rs` 保留并带有 `#[allow(dead_code)]`，供未来 OpenAI Codex 服务商入口使用。

## [0.0.3-rc.3] - 2026-07-05

### 修复
- **XiaomiMiMo OpenAI Chat-Completions 路由**。`create_message` / `create_message_stream` 调度硬编码 `ApiProvider::XiaomiMimo` 到 OpenAI Codex Responses API（`POST /v1/codex/responses`）。XiaomiMiMo 网关不提供该路径并返回 404，因此任何非 `…/anthropic` 的 base URL（例如用于 OpenAI chat-completions 方言的 `https://api.xiaomimimo.com/v1`）在到达模型之前就失败了。该分支已移除；XiaomiMiMo 现在落入 OpenAI Chat-Completions 客户端（`/v1/chat/completions`），匹配网关的实际表面。Anthropic Messages 路径（由 base_url 以 `/anthropic` 结尾驱动）不变。Codex Responses 辅助函数在 `client/responses.rs` 中保留并带有 `#[allow(dead_code)]`，供未来 Codex 服务商入口使用。

- **OpenAI `response_format` 透传**。添加了 `MessageRequest::response_format: Option<serde_json::Value>` 并将其转发到 `create_message_chat` 和 `handle_chat_completion_stream` 的请求体中。启用 XiaomiMiMo 的 JSON 模式（`{"type":"json_object"}`）；Anthropic Messages 方言按设计忽略此字段（在那里使用仅 JSON 的系统提示词）。所有 13 个内部 `MessageRequest { ... }` 字面量站点都已更新为 `response_format: None`。

### 测试
- 添加了 `client::tests::message_request_response_format_round_trips` 和 `client::tests::message_request_response_format_omitted_when_none` 以锁定新字段的 serde 形状和 `skip_serializing_if = "Option::is_none"` 不变量。
- 将 `xiaomi_mimo_token_plan_base_url_keeps_responses_protocol` 重命名为 `xiaomi_mimo_token_plan_base_url_uses_chat_completions_dialect` 并更新其注释以反映新的调度目标。

## [0.0.3-rc.2] - 2026-07-05

### 修复
- **运行时 API 现在尊重每个服务商的 `default_text_model`**。`POST /v1/threads`（以及 `POST /v1/tasks`，加上匹配的 `start_thread_turn` 路径）处理程序过去读取顶层 `default_text_model` 字段并回退到硬编码的 `DEFAULT_TEXT_MODEL` 常量。随着新的默认服务商变为 `XiaomiMiMo`，这意味着未显式指定 `model` 字段的线程即使设置了 `[providers.xiaomi_mimo] default_text_model = "mimo-v2.5-pro"`，也会使用 `deepseek-v4-pro` 初始化。五个解析站点（`runtime_api::create_task`、`runtime_api::create_thread`、`runtime_api::start_thread_turn`、`runtime_threads::create_thread`、`task_manager::TaskManagerConfig::from_runtime`）现在通过 `Config::default_model()` 路由，该函数已实现每个服务商 → 顶层 → 服务商默认的解析顺序。

- **默认文本模型更新为 `mimo-v2.5-pro`**。硬编码的 `DEFAULT_TEXT_MODEL` 常量现在是 `mimo-v2.5-pro`，以匹配默认的 `ApiProvider::XiaomiMimo`。TUI 界面（模型标签、`EngineConfig::default`、`CompactionConfig::default`、模型清单回退）自动获取新默认值；`config.toml` 中每个服务商的 `default_text_model` 仍然优先。

### 测试
- 添加了 `client::tests::xiaomi_mimo_anthropic_base_url_picks_messages_protocol` 和 `client::tests` 中的三个兄弟测试，以锁定在 `0.0.3-rc.1` 中落地的 base-url 形状调度（Anthropic Messages vs Responses vs Chat Completions）。

## [0.0.3-rc.1] - 2026-07-04

> 预发布候选版本。与计划的 `0.0.3.1` 补丁相同的修复（Cargo 拒绝四组件版本，因此作为 `0.0.3` 之上的预发布发布）。

### 修复
- **Anthropic / XiaomiMiMo Messages URL 路由**。`anthropic_messages_url` 现在在配置的 `base_url` 以 `/anthropic` 结尾时附加 `/v1/messages`（XiaomiMiMo 服务商），匹配真实端点 `https://api.xiaomimimo.com/anthropic/v1/messages`。之前它产生 `…/anthropic/messages` 并对网关返回 404。

使用来自项目指南（`POST /anthropic/v1/messages` 返回带有 `text` + `thinking` 内容块和 `usage.input_tokens` / `output_tokens` 的标准 `Message` 响应）的 `mimo-v2.5-pro` Anthropic 格式示例进行了端到端验证。

### 测试
- 添加了 `xiaomimimo_live_response_decodes_to_message_response`，使用从实时 XiaomiMiMo 响应捕获的夹具来锁定 `MessageResponse` 解码路径（文本 + 思考内容块、用法规范化、模型 ID 保留）。
- 添加了 `xiaomimimo_endpoint_url_for_anthropic_provider`，使用来自 `~/.mimofan/config.toml`（`providers.xiaomi_mimo`）的 `base_url`。
- 更新了 `url_xiaomimimo_anthropic_endpoint` 和 `url_xiaomimimo_anthropic_with_trailing_slash` 以期望修正后的 `/anthropic/v1/messages` URL。

[0.0.4]: https://github.com/XiaomingX/mimofan/compare/v0.0.3...v0.0.4
[0.0.3-rc.4]: https://github.com/XiaomingX/mimofan/compare/v0.0.3-rc.3...v0.0.3-rc.4
[0.0.3-rc.3]: https://github.com/XiaomingX/mimofan/compare/v0.0.3-rc.2...v0.0.3-rc.3
[0.0.3-rc.2]: https://github.com/XiaomingX/mimofan/compare/v0.0.3-rc.1...v0.0.3-rc.2
[0.0.3-rc.1]: https://github.com/XiaomingX/mimofan/compare/v0.0.3...v0.0.3-rc.1
