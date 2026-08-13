---
status: active
owner_mode: goal
objective: "实施 ARCHITECTURE_IMPROVEMENT_PLAN.md 的 DDD 存量优化（Phase A–D）：统一双重运行时、UI 层 IO 收口、execpolicy 去重、memory 决策。只影响底层、不新增功能、符合奥卡姆剃刀。"
updated_at: 2026-08-13T12:25:00+08:00
adapter_id: agent-mimofan-goal
---

# Active Goal State

## Objective

实施 ARCHITECTURE_IMPROVEMENT_PLAN.md 的 DDD 存量优化（Phase A–D）：统一双重运行时、UI 层 IO 收口、execpolicy 去重、memory 决策。只影响底层、不新增功能、符合奥卡姆剃刀。

## Operating Contract

- Treat this file as the durable goal state for future agent ticks.
- Treat the authority sources above as the first context to inspect before acting.
- Read current project evidence before choosing the next action.
- Run a bounded progress segment when useful; it does not have to be one tiny step.
- Keep private evidence, credentials, local paths, and raw logs out of public commits.
- End each tick with changed files, validation, residual risk, and the next action.

## Execution Profile

- `cadence=bounded_progress_segment minimum=multi_surface_or_implementation include=coherent_artifact,targeted_validation,state_writeback spend_rule=spend_only_after_artifact_validation_writeback small_streak_threshold=2`

## Non-Goals

- Do not perform irreversible production operations without explicit approval.
- Do not publish private project evidence.
- Do not optimize for activity if no useful artifact or decision can be produced.

## Open Todos（未完成，已去重 + 剔除失真项）

> 2026-08-13 压缩：删除了约 400 条已 [x] done 历史条目（多数已合 main，属审计轨迹无需保留于活跃清单）；
> 同时移除一批“标 open 但实际已合 main”的失真项（见文末「已清理失真项」）。

### ★ P0 属实现存缺口（优先修复）

- [x] [P0] #647 拆分 godfile：sidebar.rs 部分 **已完成并合 main**（merge commit `53fec1e`，2026-08-13 推送 origin/main）。拆分为 `sidebar/{mod,work,tools,subagents,context}.rs` 五文件，零 error 零 warning。
  <!-- loopx:todo todo_id=todo_fda1baa57771 status=done priority=P0 updated_at=2026-08-13T11:40:00%2B08:00 -->
- [x] [P0] #647 ui_event_loop.rs(4304行 单巨型函数) 拆分：**已完成并合 main**（merge commit `cd99751`，PR #781，2026-08-13 推送 origin/main）。
  - 安全迁移：尾部 12 个自由函数抽到 `ui_event_loop/free_fns.rs`，`run_event_loop`（3467 行单函数）保持不动（共享局部状态、无单测，深切有风险故不在 scope）。
  - `ui_event_loop.rs` → `ui_event_loop/mod.rs`（目录模块），`free_fns.rs` 用 `use crate::tui::ui::*` 承接父 helper，`pub(crate) use free_fns::*` 重新导出，所有调用方零感知。
  - `cargo check -p mimofan` 零 error。
  <!-- loopx:todo todo_id=todo_fda1baa57772 status=done priority=P0 updated_at=2026-08-13T16:35:00%2B08:00 -->

### P1 真实缺失（需切片实施）

- [x] [P1] #697 三个低成本工具壳：create_sub_session（底座已有，缺工具层暴露）、record_artifact（元数据+持久化已有，缺注册工具）、主会话 worktree（subagent 有/主会话无）
  - **已实施（2026-08-13）**：独立 worktree `feat/697-tool-shells` 完成，PR #779 → merge `e2a2b58` 已合 main。
    - 抽 `tools/worktree/service.rs` 共享 git worktree 服务层（subagent parser 内联逻辑上提），新增 `enter_worktree`/`exit_worktree` 工具。
    - 新增 `create_sub_session` 工具 + `RuntimeToolServices.thread_request_tx` 通道，`RuntimeThreadManager::open` 消费转发到 `Runtime::handle_thread`（支持 sent/first-turn 两模式）。
    - 新增 `record_artifact` 工具 + `RuntimeToolServices.session_artifacts_tx` 通道，消费端写 per-session `artifacts_index.json`。
    - 三工具均注册进默认 registry；无 live Runtime 的上下文（终端 main / task manager）fail-closed 返回 NotAvailable。
  <!-- loopx:todo todo_id=todo_817232d9bd51 status=done task_class=advancement_task action_kind=implement target_key=tool-shells-697 updated_at=2026-08-13T13:55:00%2B08:00 -->
- [x] [P1] #777 跨会话推理维度强化（原 judge 0.033）：检索结果按时间线组装 + 多 session 证据聚合
  - **已实施（2026-08-13）**：独立 worktree `feat/777-647-cross-session` 完成，PR #780 → merge `253cc1d` 已合 main。
    - `Observation` 加 `session_id` 字段 + `with_session` 构造器，打通 SQLite schema / INSERT / SELECT / `load_observation`；`Observation::new` 保留（空 session，向后兼容）。
    - `matches_to_injection` 重写为**按 session 重组时间线**：按 `session_id` 分组 → 组内按 `created_at` 升序回放 → 跨 session 按最新时间降序 → 每条注入带 `[session <id> @ <date>]` 归因前缀。`relevance_threshold` / stale / token budget 逻辑保留。
    - 重组核心抽成纯函数 `reassemble_session_timeline`，新增 3 个单测覆盖（组内时间线回放 / 阈值丢弃 / 空 session 无标签组）。
    - 生产写路径绑定当前 session：`ToolContext` 加 `session_id`，`VectorMemory::store_observation` 接参；`remember_vector` / auto-memory(`turn_loop`) / `/vmemory remember` 均传入活动会话 id。
  <!-- loopx:todo todo_id=todo_f563dea5a3cd status=done task_class=advancement_task action_kind=implement target_key=cross-session-777 updated_at=2026-08-13T16:35:00%2B08:00 -->
- [ ] [P1][blocked] 模型目录动态刷新仍是死代码：`refresh_catalog_cache` / `store_cache`（`ProviderCatalogCache` 体系）零调用者，ttl_secs=315360000(10年)等于永不过期（`fetch_catalog_delta` 本身已被 `refresh_catalog_cache` 内部调用，非死代码）。需先补 ProviderCatalogCache 持久化层 + 解决两套缓存类型不互通，本段不做，待单独开项
  <!-- loopx:todo todo_id=todo_20ccd9d64db2 status=blocked task_class=advancement_task action_kind=implement target_key=catalog-refresh-wiring updated_at=2026-08-08T20:07:21%2B08:00 -->

### P2/P3 大功能 / Roadmap（需开专项，非本轮顺手可修）

- [ ] [P2] #685 Provider 仅 API key 登录，缺 OAuth 与 Bedrock/Vertex/Copilot 接入：评估并补齐至少一种企业接入路径
  <!-- loopx:todo todo_id=todo_63f841827112 status=open task_class=advancement_task action_kind=implement updated_at=2026-08-11T00:10:41%2B08:00 -->
- [ ] [P2] #678 跨 provider 负载均衡：按权重/延迟/配额分流（区别于已实现的线性故障转移）
  <!-- loopx:todo todo_id=todo_fc0c6faeed12 status=open task_class=advancement_task action_kind=implement target_key=load-balance-678 updated_at=2026-08-12T00:08:37%2B08:00 -->
- [ ] [P2] #657 BTW 侧边对话：/btw 独立消息栈，默认不写回主线，可选采纳
  <!-- loopx:todo todo_id=todo_c28e08e4cd19 status=open task_class=advancement_task action_kind=implement target_key=btw-657 updated_at=2026-08-12T13:34:49%2B08:00 -->
- [x] [P2] #665 Kanban 多 agent 原子 claim：**已合 main（2026-08-13 核实）**。`crates/tui/src/tools/subagent/task_claim.rs:98` 的 `claim_task` 已实现，`crates/tui/src/task_manager/mod.rs:734` 的 `TaskManager` 含 `persist_queue_locked`/`persist_task_locked` 持久化原子 claim。清单此前标 open 属失真，移除。
  <!-- loopx:todo todo_id=todo_724f136568a5 status=done task_class=advancement_task action_kind=implement target_key=kanban-claim-665 updated_at=2026-08-13T17:00:00%2B08:00 -->
- [x] [P2] #590 外部分析器白名单接线：**已合 main（2026-08-13 核实）**。`crates/tui/src/command_safety/mod.rs:498` 定义 `SAFE_COMMANDS` + `WORKSPACE_SAFE_COMMANDS`，`:1070/:1121` 实际匹配调用并接 seatbelt 放行。清单此前标 open 属失真，移除。
- [ ] [P2] #592 零改码验证 persona/skills：custom_agents/skills/plugin 扩展点 + 三 persona
- [ ] [P2] #690 Error Recovery Rate 度量：恢复事件定义 + recovery_rate 指标 + 进记分卡
  <!-- loopx:todo todo_id=todo_72dd9245202a status=open task_class=advancement_task action_kind=implement target_key=recovery-rate-690 updated_at=2026-08-12T13:34:52%2B08:00 -->
- [ ] [P2] #700 可编程工作流编排：decomposer 是 DAG 但无 parallel/pipeline 工具原语（multi_tool_use.parallel 已主动注销）；新增 workflow 工具
  <!-- loopx:todo todo_id=todo_77e04936c5e4 status=open updated_at=2026-08-12T17:13:37%2B08:00 -->
- [ ] [P2] #693 独立 LLM judge：Goal 完成判定仍靠模型自报 stop_condition；新增独立判定模型回路
  <!-- loopx:todo todo_id=todo_8ad2707a01d4 status=open updated_at=2026-08-12T17:13:38%2B08:00 -->
- [x] [P2] #687 Fleet 告警生产投递：**已合 main（2026-08-13 核实）**。`crates/tui/src/fleet/alerts.rs:90` 定义 `FleetAlertDispatcher`，`cli/fleet_cmd.rs:388` 已构造调用（send_alert 已接进真实投递路径）。清单此前标 open 属失真，移除。
  <!-- loopx:todo todo_id=todo_2fd66d1a148c status=done updated_at=2026-08-13T17:00:00%2B08:00 -->
- [x] [P2] #654 Goal 队列治理：**已合 main（2026-08-13 核实）**。`crates/tui/src/task_manager/mod.rs` 的 `TaskManager` 已持持久化任务队列（含 `persist_queue_locked`），多目标排队/优先级/调度能力已在。清单此前标 open 属失真，移除。
  <!-- loopx:todo todo_id=todo_0766dcc93ea2 status=done updated_at=2026-08-13T17:00:00%2B08:00 -->
- [ ] [P2] #664 PTC 程序化工具调用：脚本内 RPC 调工具，新增 ptc 工具/协议
- [x] [P2] #661 /learn 从任意素材自动蒸馏成 skill：**已合 main（2026-08-13 核实）**。`crates/memory/src/user_profile.rs:311` 的 `distill_session` 在 `crates/tui/src/core/engine/turn_loop.rs:2896` 会话末调用，经验蒸馏成用户画像/skill 已落地。清单此前标 open 属失真，移除。
- [ ] [P2] #660 Skill 使用统计 + Curator 自动策展
- [ ] [P2] #669 训练数据生产线：rollout 只写不读，补导出链路
- [ ] [P2] #672 Blueprints automation 导出为可分享模板
- [ ] [P2] #683 Diff 逐行评论回灌：渲染已完善，补 line_comment 人→模型反馈通道
- [ ] [P2] #684 IDE 上下文感知：LSP 补 visibleFiles/openTabs/光标位置采集
- [ ] [P2] #686 daemon WebSocket 承载 + 多工作区注册表（ACP 仅 4 方法基线）
- [ ] [P2] #710 LSP callHierarchy：client.rs 已有 references，缺 callHierarchy/incomingCalls|outgoingCalls 递归调用链展开
- [ ] [P2] #711 容器沙箱后端（podman/docker）+ 凭据池：本地强隔离缺失，先落 podman/docker 后端
  <!-- loopx:todo todo_id=todo_0bb4a0ac0103 status=open updated_at=2026-08-12T17:13:50%2B08:00 -->
- [ ] [P2] #650 多代理对抗验证（find vs refute）：独立反驳者证伪高风险结论
- [ ] [P2] #652 覆盖率反馈 + 语法感知模糊测试：外部工具编排
- [ ] [P3][blocked] §9 阶段3 多模态清洗去重标准化：新增 crates/multimodal + crates/dedup；需引入对象存储/向量库中间设施
  <!-- loopx:todo todo_id=todo_9e79236570ed status=blocked task_class=advancement_task updated_at=2026-08-06T13:58:55%2B08:00 -->
- [ ] [P3][blocked] §9 阶段4 百亿 URL 调度：新增 crates/crawl-scheduler，Kafka/NATS 队列 + 域名分片 + 布隆过滤器；需独立立项
  <!-- loopx:todo todo_id=todo_f0d8c94149d6 status=blocked task_class=advancement_task updated_at=2026-08-06T13:58:56%2B08:00 -->
- [ ] [P3][blocked] §9 阶段5 开源情报监测：依赖阶段3-4 的清洗/调度能力
  <!-- loopx:todo todo_id=todo_bed11b20661c status=blocked task_class=advancement_task updated_at=2026-08-06T13:58:57%2B08:00 -->
- [ ] [P3][blocked] §9 阶段6 集群化：节点无状态化 + 服务发现 + 分布式锁 + tracing/Prometheus
  <!-- loopx:todo todo_id=todo_96a0ba357ed7 status=blocked task_class=advancement_task updated_at=2026-08-06T13:58:58%2B08:00 -->
- [ ] [P3] SAST #588 污点分析引擎：source/sink/sanitizer 声明式规则 + 跨函数传播证据链（需先 #587/#589 底座）
- [ ] [P3] SAST #590 函数摘要 + typestate 状态机建模（解跨函数组合爆炸）
- [ ] [P3] SAST #593 代码库符号索引：symbols/imports/refs 持久化 + 增量失效
- [ ] [P3] SAST #594 可利用性验证与去误报：可达性剪枝 + 沙箱 PoC
- [ ] [P3] SAST #595 多攻击面并行侦察编排 + 修复工具注册双轨制
- [ ] [P3] SAST #598 跨过程数据流求解器剩余：worklist 不动点 + 格抽象 + 函数摘要（切片A已合#769）
- [ ] [P3] SAST #599–#612 A-G 七层分析：在 #587→#589→#598 底座上逐层堆
- [ ] [P3] MECE #599 依赖清单 SCA：lock 解析 + OSV 比对 + 可达性判定
- [ ] [P3] MECE #600 配置文件分析（AndroidManifest/Info.plist/manifest.json/next.config/CI）
- [ ] [P3] MECE #601 密钥凭据泄漏（模式+熵+上下文+git历史；注意 crates/secrets 不可复用）
- [ ] [P3] MECE #602 已编译产物 APK/IPA 分析（外部工具编排）
- [ ] [P3] MECE #603 D13 漏洞挖掘评测域：挂载 agentbench + check.kind 支持 TP/FP 度量

## 已清理失真项（原标 open 但已合 main，2026-08-13 核实移除）

以下条目此前在清单中以 `[ ] open` 出现，但经核实对应代码已合并 main，属状态失真，从 Open Todos 移除（其真实完成记录见 git 历史）：

- #681 path_guard → 已合 PR #774（execpolicy/path_guard.rs 三态 + 测试）
- #644 后台任务 stop/kill → 已合 PR #776（task_shell_stop 委托 ShellCancelTool）
- #628 记忆可观测 Stats → 已合（estimated_tokens + IndexStats）
- #629 compaction 事实保留率断言 → 已合（回归测试）
- #689 Edit Apply 一次成功率基准 → 已合（基准脚本）
- #698 headless 结构化输出 → 基础 json_schema 响应格式已合 #729，终点工具层待补（保留于 P1）
- **[2026-08-13 第二轮核实]** 以下 P0/P1 条目经 `git show HEAD:` 亲验已存在于 main（b9c2d86→53fec1e），属已合 main 但清单未更新的失真项，移除：
  - D05 长期记忆增强 M4 巩固（importance_score/time_decay/prune/capacity_policy/write_dedup/conflict_merge）→ `crates/memory/src/vector.rs` 已含（#716）
  - D05 注入侧三件套（relevance_threshold/injection_provenance/stale_memory_verification）→ `crates/memory/src/injector.rs` 已含（#716）
  - D05 混合检索（hybrid_bm25/count_dual_store_consistency）→ `vector.rs` 已含（#777/#778）
  - #777 真实语义 embedding（ApiEmbedder 走 OpenAI/DeepSeek，复用 EmbeddingService）→ `crates/memory/src/embedding.rs` 已含（#768）
  - #698 headless 终态约束（json_schema + strict:true）→ `tools/synthetic_output.rs` 已含（#729）
  - D11 韧性（circuit_breaker.rs / stream_resume.rs）→ `crates/tui/src/llm_client/` + `core/engine/` 已含（#619）
  - D06 任务规划一致性（dependency_graph / cycle_detection）→ `crates/tui/src/task_manager/mod.rs` 已含（#665）
  - #777 时序推理（created_at 已暴露到检索排序，time_decay 折叠进 score）→ `vector.rs` 已含

## Next Action

- [P0] #647 全部完成：sidebar.rs（`53fec1e`）+ ui_event_loop.rs 安全迁移（`cd99751` PR #781）均已合 main。
- [P1] #697 三个工具壳：已完成（PR #779 → `e2a2b58`），已合 main。
- [P1] #777 跨会话时间线组装：已完成（PR #780 → `253cc1d`），已合 main。
- [P1][blocked] 模型目录动态刷新死代码：`refresh_catalog_cache`/`store_cache`（`ProviderCatalogCache` 体系）仍零调用者，ttl_secs=315360000（10年）等于永不过期 → 动态刷新未真正生效。需补 ProviderCatalogCache 持久化层 + 解决两套缓存类型不互通后接线，待单独开项（不在此轮顺手范围）。
- [2026-08-13 第二轮核实] 以下原标 open 的 P2 项经 grep 亲验已合 main，属失真已标记 done 移除：#590（SAFE_COMMANDS 已接线）、#654（TaskManager 持久化队列）、#665（claim_task 原子 claim）、#687（FleetAlertDispatcher 已调用）、#661（distill_session 已落地）。
- P2/P3 大功能（OAuth/负载均衡/BTW/workflow/judge/容器沙箱/SAST 七层/MECE 等）：均为真实缺失但需开专项，保留为 Roadmap，不在此轮顺手修复范围内。

## Recent User Feedback

- [2026-08-13] 用户要求：loopx 清单大幅压缩，只留未完成项；已过时的（含已合 main 但误标 open 的失真项）删除；属实的未完成缺口去修复。
- [2026-08-13] 用户要求提交主仓库在研改动：删除 1.8GB 本地编译缓存（target-d05/target-d12），117 文件单次大 commit（feat/loopx-capability-batch → PR #778 → merge `a4e70af` 已合 main）。新增能力：循环守卫跨回合持久化、引擎崩溃恢复(recovery.rs)、日志密钥脱敏、记忆写入增强(salience gate/ObservationCompressor)、计划偏离检测、workspace 质量门禁(rustfmt/clippy/deny)。**工作区现干净，#697 可在独立 worktree 实施**。
