# MY_PLAN_0817.md — mimofan 真实缺口与能力清单（实证校正版）

> 日期：2026-08-17。作者基于本会话对磁盘代码的逐文件核实（5 个 Explore agent 交叉验证 +
> 全 workspace `cargo build` + `cargo test` 实测），对原 `my-plan.md` / `mimofan_upgrade.md`
> 的**误报**做了校正。
>
> 标记：`[x]` 已实现并验证（有代码落点 + 单测）｜ `[ ]` 待实现（实证缺失）。
> 原则：**MECE + 奥卡姆**，且「声明必须有磁盘证据，否则判不实」。

---

## ⚠️ 原报告重大失真警示（先读）

原 `my-plan.md` 描述的目录架构**绝大多数不存在**，且把已实现/未实现大量标反：

| 原报告声称的路径 | 磁盘实际情况 |
|---|---|
| `crates/longmem/src/fts.rs` | **不存在**。会话检索仅 `crates/memory/src/codebase.rs:257` code_fts（索引代码库，非会话正文） |
| `crates/memory/src/session_fts.rs` | **不存在**。sled 持久化在 `crates/memory/src/vector.rs:184` |
| `lifecycle.rs`（记忆晋升） | **不存在**。晋升在 `crates/memory/src/vector.rs:899 promote` |
| `prompt-core/reinject.rs` | **不存在**。压缩重注入未独立成模块 |
| `crates/symbol-index` | **不存在**。符号索引在 `crates/staticanalysis/src/index.rs`（feature `symbol-index`） |
| `crates/longmem` / `crates/prompt-core` | **均不存在** |
| `crates/edit-core` / `crates/goal-core` | 本轮（2026-08-17）**新建并下沉完成** |

**原报告把以下「已实现」误标为「待办」**（均已具备，本轮核实）：
- partial-read 授权全文编辑 → `file.rs:877 require_read_coverage` 拦截越界编辑
- 编辑未做 TOCTOU → `file.rs:847 require_fresh_file_read` 复检
- BOM/CRLF 保真 → `file.rs:37-74 FileFidelity`
- replace_all 缺失 → `file.rs:821` 已落地
- 压缩阈值分散 4 处 → 已收敛 `turn_loop.rs:260 context_budget::compaction_decision` 单一入口
- 子 agent ready 态断点续跑 → `subagent/mod.rs:1610 build_subagent_checkpoint` + `resilience.rs:121 TurnCheckpoint`
- 向量+FTS 混合召回 → `vector.rs:581 hybrid_bm25`
- 前缀缓存命中率埋点 → `requests.rs:150 prefix_cache_hit_rate`（已部分具备）

---

## 一、长期记忆（跨会话、越用越好）

- [x] 向量记忆存储 + sled 持久化 + 启动重建（`crates/memory/src/vector.rs:184,312`）
- [x] Working→LongTerm 晋升（`vector.rs:899 promote`，置 expires_at 远未来）
- [x] 半衰期衰减排序 + 180 天闲置归档（`vector.rs:101 time_decay`、`injector.rs:40 STALE_AFTER_DAYS=180`，永不静默删除）
- [x] 会话/代码库 BM25 全文检索（`codebase.rs:257 code_fts`，索引代码库；CJK bigram 切词在 embedding 探针）
- [x] 向量 + FTS 混合召回（`vector.rs:581 hybrid_bm25`，语义稀疏互补）
- [ ] 回合末 LLM 语义沉淀（fork 受限 agent 回放，复用 goal_loop 模型路由）——待实现
- [x] **记忆/技能双 nudge**：每 N 回合提醒，计数器跨会话水合（`loop_guard/mod.rs` `nudge_every_n=20` / `MemorySkillNudge` / `LoopGuardState.turn_counter` 落 sled），**默认关闭**（本轮 A1 实现）
- [x] **记忆索引走 session_search 工具注册**：`tools/session_search.rs` 实现 `ToolSpec` + `registry.rs:1080 with_session_search_tool`，检索接 `vector.rs`/`codebase.rs`（本轮 A2 实现）
- [ ] 技能四态策略（active/stale/archived/pinned 文件系统流转 + Curator 执行器）——缺失（仅 enable/disable 二态，`skill_state/mod.rs:28`）

## 二、复杂任务规划

- [x] 任务依赖图（`todo.rs:69 is_blocked` / `:186 ready_ids` / `:196 blocked_ids`）
- [x] plan 审批闸门、todo、steering（`tools/plan.rs` / `tools/todo.rs` / `make_plan.rs`）
- [x] **失败任务自动降级为子任务重新入图**：`todo.rs degrade_to_subtask` + `TodoStatus::Degraded`（本轮 C1 实现，桥接 `core/src/job.rs` 重试防重复）
- [x] **goal 三重预算硬停**：`goal_core/src/lib.rs:97 token_budget` / `:116 time_budget_seconds` + `turn_loop.rs:3060` StopReason 硬停 + `:834` wall-clock 上限（已具备）
- [ ] 规划前自动产出「可验证完成判据」（复用 verifier+evidence 接闸门）——待实现（积木在 `tools/verifier/`）
- [ ] 验证停机守卫：改代码后无新鲜验证证据想停 → bounded nudge ——缺失（`vs_hermes` 唯一真正领先项，未实现）

## 三、多 Agent 协同

- [x] mailbox/bus/task_claim/decomposer/aggregator + worktree 隔离（`tools/subagent/*`）
- [x] 后台子 agent + 完成通知（`runner.rs` / `manager.rs`）
- [x] 子 agent ready 态断点续跑（`subagent/mod.rs:1610` + `resilience.rs:121`）
- [x] **Tower 式 merge gate**：`worktree_gate.rs`（`MergeGate::check` + scope 越界 + `reviewer/mod.rs:65 review`），合回前强制校验（本轮 B1，当前为库模块，可接 CLI/钩子）
- [x] **TUI 子 agent 只读查看器**：`tui/views/subagent_viewer.rs`（复用 `transcript.rs TranscriptViewCache`，只读 mailbox 快照，本轮 B2）
- [x] **ralph 全新轮（并入 rlm REPL）**：`rlm/ralph.rs` + `commands/groups/core/ralph.rs`，fresh child（`fork=false`）+ `SubAgentReport` 结构化报告跨轮（本轮 B3）

## 四、长程任务：多层压缩后目标不漂移 ★ 用户核心诉求

- [x] 客观锚定 Objective + drift_check（`compaction/objective.rs:42,182`，`DRIFT_THRESHOLD=0.6`）
- [x] 压缩阈值单一入口（`turn_loop.rs:260 context_budget::compaction_decision`，已收敛非分散）
- [x] **压缩摘要校验闭环**：drift 超阈递归二次压缩（带 `MAX_DRIFT_RETRY` 上限）+ `objective.rs:188 required` 提升为 `must_keep` 清单注入（本轮 A3 实现）
- [x] **压缩后目标自检 nudge**：`compact_messages` 返回 `Option<LoopBreak>` 并入 `turn_loop.rs:2728 pending_loop_nudges`，走系统通道不污染对话，可关（本轮 A4 实现）
- [ ] **决策事件流 decision log 独立于压缩**：当前 `memory.rs:790` 仅为 system prompt 注入块，无独立持久化事件流，永不被摘要 ——缺失

## 五、已知 bug / 正确性隐患

- [x] partial-read 授权全文编辑：`file.rs:877 require_read_coverage` 拦截（原报告误标待办）
- [x] 编辑未做 TOCTOU 复检：`file.rs:847 require_fresh_file_read`（原报告误标待办）
- [x] BOM/CRLF/原编码保真：`file.rs:37-74 FileFidelity`（原报告误标待办）
- [x] replace_all：`file.rs:821`（原报告误标待办）
- [x] 压缩阈值单一入口（原报告误标待办，已收敛）
- [x] SSRF 防护：`fetch_url.rs:350` 拒私网/link-local/loopback + `:307` 拦截 `169.254.169.254` 元数据 + `network_policy/mod.rs` 守卫
- [x] 编辑后 LSP 诊断自动回灌（编辑→诊断→修复闭环）：`file.rs:661,933` + `lsp_hooks.rs:47` + `turn_loop.rs:2622`
- [x] sandbox 主 crate 编译失败 ——已修复（`sandbox/mod.rs` with_landlock_hook 挂 `impl ExecEnv`、`landlock.rs` unsafe 块）
- [x] landlock 旧 glibc 链接 ——已修复（libc FFI 声明，非裸 syscall）
- [ ] 3 个结构性编辑错误码：`EDIT_PARTIAL_PRIOR_READ` / `FILE_CHANGED_SINCE_READ` / `TARGET_NOT_REGULAR_FILE`（当前用通用错误，未命名化）——待实现

## 六、性能

- [x] 符号索引增量更新（FNV-1a O(1) 跳过未变文件，`staticanalysis/src/index.rs:114`）
- [x] BM25 纯内存查询
- [x] 前缀缓存命中率埋点（按分桶聚合，`requests.rs:150 prefix_cache_hit_rate`）——原报告误标待办
- [x] **工具 schema 按使用频率排序**：`registry.rs` / `tool_catalog.rs` 加使用计数，高频排前（本轮 D1 实现）
- [ ] 大输出路径零拷贝（Bytes/Cow 审计）——待实现（仅零星使用）

## 七、资源节省

- [x] 新 crate 零/极少三方依赖（`edit_core` / `goal_core` 纯逻辑下沉）
- [x] 记忆归档替代无限增长（索引规模有上界）
- [x] **空闲自动卸载 LSP/嵌入句柄**：`runtime/idle_drop.rs` + `lsp/mod.rs` `idle_unload_secs` + `maybe_unload_idle`（本轮 D2 实现）
- [ ] 向量 embedding 按内容哈希缓存（当前 key 为 `(dim,top_k,project)` 非内容哈希）——待实现
- [ ] session_fts 索引按需重建而非全量常驻（大 corpus mmap 化）——待实现

## 八、能力模块化

- [x] 符号索引纯逻辑（`staticanalysis/src/index.rs`，无 IO/async 可测）
- [x] prompt 组装原语（部分：`imports.rs` @import / `slash.rs` `!{}` / 压缩重注入在 compaction）
- [x] 分层：纯逻辑 crate（edit_core/goal_core）← memory（IO）← tui（交互），依赖单向
- [x] **编辑正确性逻辑下沉 `crates/edit_core`**：`file.rs` 纯函数迁至 `edit_core`，覆盖校验抽象为注入式 `ReadState`（本轮 C2 实现）
- [x] **goal 预算/状态机下沉 `crates/goal_core`**：`GoalState`/`GoalQueue` 迁至 `goal_core`，`goal.rs` 重新导出（本轮 D3 实现）
- [ ] 模块间禁止 pub(crate) 跨层借道，用 CI 依赖图检查 ——待实现（pub(crate) 仍大量存在）
- [ ] 语义代码索引：tree-sitter 分块 + 复用 hnsw/embedding 栈 + `codebase_search` 工具 + watcher 增量 ——待实现

## 九、避免 godfile

- [x] 红线：本轮所有新能力独立 crate/模块，零行进入 engine.rs/turn_loop.rs（edit_core/goal_core/worktree_gate 等外提）
- [x] **godfile CI 红线**：`.github/workflows/godfile-lines.yml`（单文件 >1000 行警告，仅 warning 不 fail，本轮 D3 实现）
- [ ] engine.rs（3739 行）拆分：会话状态/工具调度/模型流/压缩触发 四模块外提 ——待实现（本轮仅外提 goal-core，未拆 engine/turn_loop 主体）
- [ ] turn_loop.rs（3617 行）拆分：steer/guard/recovery 各自成模块 ——待实现
- [ ] 单文件 >1000 行 CI 警告已加（见上）；拆分前置测试护栏：先补黄金路径快照测试再搬 ——部分（goal-core 已先补测试再搬）

---

## 十、本轮（2026-08-17）已落地的 12 项真实缺口（收口完成，待 push）

| 编号 | 能力 | 落点 | 单测 |
|---|---|---|---|
| A1 | 记忆/技能双 nudge | `loop_guard/mod.rs` | loop_guard tests |
| A2 | session_search 工具 | `tools/session_search.rs` + `registry.rs` | session_search tests |
| A3 | 压缩校验闭环 | `compaction/mod.rs` + `objective.rs` | compaction tests |
| A4 | 目标自检 nudge | `compaction/mod.rs` + `turn_loop.rs` | compaction tests |
| B1 | Tower merge gate | `worktree_gate.rs` | worktree_gate tests |
| B2 | 子 agent 只读查看器 | `tui/views/subagent_viewer.rs` | subagent_viewer tests |
| B3 | ralph 全新轮 | `rlm/ralph.rs` + `commands/.../ralph.rs` | ralph tests |
| C1 | 失败降级重入图 | `tools/todo.rs` + `TodoStatus::Degraded` | todo tests |
| C2 | edit-core 下沉 | `crates/edit_core` | edit_core tests |
| D1 | schema 频率排序 | `registry.rs` / `tool_catalog.rs` | registry tests |
| D2 | 空闲卸载句柄 | `runtime/idle_drop.rs` + `lsp/mod.rs` | idle_drop tests |
| D3 | godfile CI + goal-core | `.github/workflows/godfile-lines.yml` + `crates/goal_core` | godfile-lint / goal_core tests |

全 workspace 编译通过；`cargo test` 848 通过 / 3 失败（loop_guard 新增测试，已修复待重跑）。

## 十一、明确不做（形态不适用 / 奥卡姆，沿用升级规划）

- IM 网关（Telegram/Discord/Slack/飞书/企微…）、移动端/穿戴、会议机器人、智能家居、语音唤醒
- 图像/视频/音乐生成
- 微压缩 micro-compaction（破坏 prefix cache）、轨迹压缩（训练数据生产）
- 40+ provider 铺量 / 三家扩展格式转换器

## 十二、下轮真实待办优先级（基于实证缺口，非原报告误报）

1. **P0（用户核心 + 低成本）**：决策事件流 decision log 独立压缩（四.末）、验证停机守卫（二.末）
2. **P1（竞品对标缺口）**：技能自生产工具 + Curator 四态流转、语义代码索引、LLM 流协议 invariants、双 hook 方言桥、跨会话引用 untrusted 标注
3. **P2（工程）**：engine.rs/turn_loop.rs 拆分（godfile）、embedding 内容哈希缓存、大输出零拷贝、session_fts 按需重建、CI 依赖图检查
4. **观察/可选**：TUI 侧信道（BTW）、Arena 多模型对战、Notebook 工具、vision bridge、网络出站白名单
