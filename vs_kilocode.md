# mimofan vs kilocode 能力对标

> 对标日期：2026-08-08
> mimofan：`/Users/a0000/mywork/commonLLM/opensource/nnnew/agent-mimofan`（Rust，约 24 万行，主体 `crates/tui/`）
> kilocode：`/tmp/mimofan-bench/competitors/kilocode`，commit 时点为 2026-08 抓取版本

## 0. 前置说明：kilocode 形态已发生重大变化

对标前必须澄清一个关键事实，否则整份结论会跑偏。

**kilocode 已不再是"单纯的 VS Code 扩展"。** 当前仓库是一个 bun workspace monorepo，核心运行时是 `packages/opencode/`（1553 个 TS 文件），这是 **opencode 的 fork**，kilocode 在其上以 `// kilocode_change` 注释的方式打了大量增强补丁。其形态矩阵为：

| 形态 | 落点 | 说明 |
|---|---|---|
| CLI / TUI | `packages/opencode/`、`packages/tui/`（214 文件，基于 `@opentui`） | 与 mimofan **同形态直接竞品** |
| VS Code 扩展 | `packages/kilo-vscode/`（1242 文件） | GUI 独有能力集中在此 |
| JetBrains | `packages/kilo-jetbrains/`（仅 2 文件，壳） | 尚未成型 |
| Web / Cloud | `packages/kilo-web-ui/`、`packages/server/`、`packages/kilo-gateway/` | 云端会话与网关 |

**这意味着"终端形态所以不适用"这个挡箭牌大幅收窄**：kilocode 的 CLI 与 mimofan 是正面竞争关系，`packages/opencode/` 里的能力不能用形态差异搪塞。真正只属于 VS Code 的，仅有依赖编辑器 API 的那部分（见第 9 节）。

本文件所有「缺失」结论均已用 Grep/Glob 在 mimofan 代码库验证过符号确实不存在，验证命令与证据在对应条目内注明。

---

## 1. 总览表

| 能力域 | mimofan 状态 | 关键差距摘要 |
|---|---|---|
| 上下文压缩与裁剪 | 领先 | 三套范式并存；缺 SWE-Pruner 式**工具输出级**自适应裁剪 |
| 会话生命周期 | 对等 | checkpoint/fork/revert 齐备 |
| 子智能体与并行 | 领先 | mailbox/bus/task_claim/worktree 隔离，kilocode 仅 task 工具 |
| 任务与依赖图 | 领先 | kilocode 只有扁平 todo，无依赖图 |
| 代码库语义索引 | **缺失（最大差距）** | kilocode 有完整 `kilo-indexing` + 托管 `codebase_search` |
| Mode / Agent 体系 | 部分具备 | mimofan 仅 3 个硬编码 AppMode，缺用户可定义 primary mode |
| 记忆 | 对等偏领先 | 双层（文件+向量），kilocode `kilo-memory` 侧重决策捕获 |
| 工具体系 | 对等 | 54 vs ~30 工具；mimofan 缺 notebook |
| Provider / 成本 | 对等 | 双方均有 models.dev + 成本追踪 |
| 沙箱与权限 | 对等 | 双方 seatbelt/bubblewrap + 策略引擎 |
| MCP | 对等（缺市场） | mimofan 有 client+server，缺 marketplace 发现安装 |
| 提示词工程辅助 | **缺失** | 缺 enhance-prompt、commit-message 生成 |
| 遥测与可观测 | 部分具备 | 有 audit/resource_telemetry，缺结构化产品遥测 |
| 多模态 | 部分具备 | 有 vision/OCR/speech，缺图像生成 |
| GUI 独有 | 不适用 | 见第 9 节 |

---

## 2. 上下文压缩与裁剪

mimofan 在这一域整体领先，kilocode 只有单一压缩范式，但有一个 mimofan 没有的正交机制。

### 已具备

- [x] **替换式摘要压缩** — `crates/tui/src/compaction/`。kilocode 对应 `packages/opencode/src/session/compaction.ts`（721 行，含 `PRUNE_MINIMUM=20_000`、`PRUNE_PROTECT=40_000`、turn 边界识别）。
- [x] **append-only 软缝压缩（护 prefix cache）** — `crates/tui/src/seam_manager/` + `crates/tui/src/prefix_cache/`。**kilocode 无对等物**：它的 `PruneReason = "normal" | "post-compaction" | "payload-limit"`（compaction.ts:66）说明它只能在"缓存已失效的边界"上做裁剪，属于被动规避；mimofan 的软缝是主动保 prefix。这是 mimofan 的净领先项。
- [x] **agent 驱动的外科式清理** — `crates/tui/src/purge/`。kilocode 无 agent 主动清理上下文的机制。
- [x] **PreCompact / PostCompact 生命周期 hook** — `crates/tui/src/hooks/mod.rs`（13 变体的用户可配生命周期 hook）。注意与 `crates/hooks/`（EventFrame 分发 sink，6 变体）是两套系统。kilocode 侧 compaction 事件走 `SessionCompactionEvent`。
- [x] **上下文预算管理** — `crates/tui/src/context_budget/`、`crates/tui/src/route_budget/`。kilocode 对应 `session/overflow.ts`（仅 36 行，`isOverflow`/`usable`），mimofan 更完整。
- [x] **大输出路由 + 溢出结果检索** — `crates/tui/src/tools/large_output_router.rs`（超阈值走 V4-Flash 子智能体综述，原文存 workshop 变量）、`crates/tui/src/tools/tool_result_retrieval.rs`（对已 spill 的输出做切片/摘要检索）、`crates/tui/src/tools/truncate.rs`。

### 待添加

- [ ] **SWE-Pruner：工具输出级自适应裁剪（模型自声明 focus question）**
  - kilocode 落点：`packages/opencode/src/kilocode/swe-pruner.ts`。机制是给 `read`/`grep`/`bash` 三个工具**额外挂一个可选参数** `context_focus_question`；模型调用工具时自己声明"我关心什么"，原始输出先由小模型按该问题逐行筛选，只保留相关行，其余标记省略。有 `MIN_LINES=50`/`MIN_CHARS=2_000`/`MAX_KEEP_RATIO=0.9`/`TIMEOUT_MS=15_000` 等护栏，任何失败回退全量输出。论文依据 arXiv:2601.16746。
  - mimofan 验证：`grep -rn "context_focus\|focus_question\|prune_output\|output_prune" crates --include='*.rs'` → **0 命中**。
  - 为什么值得做：mimofan 的 `large_output_router` 是**事后**的——输出产生后整体丢给子智能体综述，模型意图信息已丢失，综述只能猜"什么算重要"。SWE-Pruner 是**事前**的——模型在发起调用时就带上意图，裁剪有明确目标函数，且裁剪发生在进入上下文之前。两者正交、可叠加：mimofan 可在现有 `ToolSpec` 上加一个可选参数，复用已有的小模型路由（`model_routing/`）做筛选，护栏逻辑可直接照搬。这是本次对标中**投入产出比最高**的一项。


---

## 3. 代码库语义索引（本次对标最大差距）

这是 kilocode 相对 mimofan 最实质、最成体系的领先项，值得单列一章。

### kilocode 的两条并行路线

1. **本地索引引擎** `packages/kilo-indexing/`（119 文件，独立发包 `@kilocode/kilo-indexing`）：
   - 编排：`src/indexing/orchestrator.ts`、`manager.ts`、`state-manager.ts`（Indexed/Indexing 状态机）
   - 检索：`src/indexing/search-service.ts`（向量检索 + minScore/maxResults 过滤 + 目录前缀限定）
   - 分块：`src/tree-sitter/`（`languageParser.ts` + `queries/` 目录，**基于 tree-sitter AST 按语法结构分块**，而非按行切）
   - 向量库：`src/indexing/vector-store/`，支持 `lancedb-vector-store.ts` 与 `qdrant-client.ts` 双后端
   - Embedder：`src/indexing/embedders/` 共 10 家（openai / gemini / bedrock / mistral / ollama / voyage / openrouter / vercel-ai-gateway / kilo / openai-compatible）
   - 增量：`src/indexing/processors/file-watcher.ts` + `cache-manager.ts`（文件变更增量重建）
   - worktree 支持：`src/indexing/worktree-overlay.ts`（worktree 索引与主索引 overlay 合并）
2. **托管检索服务**：`packages/opencode/src/tool/warpgrep.ts` 暴露 `codebase_search` 工具，走 Morph WarpGrep（`@morphllm/morphsdk`），免 key 期间经 `https://api.kilo.ai/api/gateway` 代理。即用户零配置也能有语义代码检索。

### mimofan 现状（已逐项验证）

- [x] **本地向量库基础设施已存在** — `crates/memory/src/vector.rs`（`hnsw-rs` + `sled`）、`crates/memory/src/embedding.rs`（OpenAI 兼容 embedding API）、`crates/tui/src/vector_memory/mod.rs`（feature-gated `vector-memory`，默认开启，未配 key 时优雅降级）。**这点很重要：不是从零建设。**
- [x] **词法/符号检索** — `crates/tui/src/tools/search.rs`、`file_search.rs`、`crates/tui/src/symbol_index/`、`crates/tui/src/lsp/`。
- [x] **工具目录 BM25 检索** — `crates/tui/src/core/engine/tool_catalog.rs`（但检索对象是**工具**，不是代码文件）。

### 待添加

- [ ] **对代码文件建立语义索引并提供 `codebase_search` 工具**
  - mimofan 验证：`grep -rn "index_codebase|code_index|CodeIndex|semantic_search|codebase_search|index_repository" crates --include='*.rs'` → 仅命中 `tool_catalog.rs:1058/1062` 的**单元测试夹具字符串**（`tool("semantic_search", ...)` 是构造测试数据）与 `config/src/harness.rs` 的 `prefer_codebase_search` 配置项；后者读注释 `harness.rs:78` 为 "Prefer search-based/on-demand context over always-on documentation"，语义是"优先 grep 而非常驻文档"，**与向量检索无关**。确认无真实实现。
  - 现有向量能力的作用域验证：`crates/tui/src/vector_memory/mod.rs:1-5` 明确写明用途是"记忆"——存的是 `Observation`（`crates/memory/src/vector.rs:14-33`，字段为 content/kind/project/files_read/files_modified/concepts），**索引对象是会话观察记录，不是代码文件内容**。
  - 为什么值得做：这是 mimofan 唯一一处"竞品有完整体系、自己连入口都没有"的能力域。影响的是最高频场景——大仓库里回答"这个功能在哪实现的"。当前 mimofan 只能靠 grep 关键词，对"描述语义但不知道确切标识符"的查询（如"处理支付重试的逻辑在哪"）无能为力，而 `symbol_index` 是 104 行的行首前缀匹配器（`crates/tui/src/symbol_index/mod.rs:65-103` 的 `parse_line_symbol` 用 `line.strip_prefix("fn ")` 这类硬编码前缀表），召回能力很弱。
  - 落地建议：**复用已有资产，不要重建**。`hnsw-rs`+`sled` 向量库、`EmbeddingService` 都已就绪，缺的是（a）代码分块器、（b）文件监听增量索引、（c）`codebase_search` 工具注册。分块可先用"函数/类边界正则 + 滑动窗口"起步（mimofan 无 tree-sitter 依赖，验证：`grep "tree.sitter|tree_sitter" **/*.toml` → 0 命中；引入 tree-sitter 是可选的后续增强）。注意 `VectorMemory` 非 `Send` 的既有约束（`vector_memory/mod.rs:18-25` 有详细说明），代码索引需沿用同样的 `take_embedder()` 模式。


---

## 4. Mode / Agent 体系

这一域结论最微妙，容易误判两个方向，需要精确区分。

### kilocode 的设计

`packages/opencode/src/agent/agent.ts`（658 行）用**统一的 Agent 抽象**同时表达"主模式"和"子智能体"，靠 `mode: "subagent" | "primary" | "all"` 字段区分（agent.ts:53）。内置：

- `build`（primary，默认，agent.ts:184）、`plan`（primary，禁所有编辑工具但白名单放行 plan 目录写入，agent.ts:203-228）
- `general`（subagent，agent.ts:229）、`explore`（subagent，只读权限集，agent.ts:243）、`scout`（subagent，实验开关，可 clone 依赖仓库做调研，agent.ts:269）

关键在于 `Info` schema（agent.ts:47-74）：每个 agent 可独立配置 `permission` ruleset、`model`、`temperature`/`topP`、`prompt`、`tools`、`steps` 上限，且**用户可在配置文件里定义任意多个 primary mode**。另有 `generate` 能力（agent.ts:88）——用自然语言描述自动生成一个新 agent 定义。还有 `modes-migrator.ts` / `rules-migrator.ts` / `workflows-migrator.ts` 负责从旧版 `.kilocodemodes` 迁移。

### 已具备

- [x] **自定义子智能体（Markdown + YAML frontmatter 定义）** — `crates/tui/src/tools/subagent/custom_agents.rs`。扫描 `~/.mimofan/agents/` 与 `.mimofan/agents/`，`CustomAgentDef` 含 `name`/`description`/`tools`（工具白名单）/`model`（inherit|fast|具体 ID）/`prompt`。**与 kilocode 的 subagent 定义能力基本对等。**
- [x] **计划模式** — `crates/tui/src/tools/plan.rs`（`ExitPlanModeTool`、`exit_plan_mode_plan_text`）+ `AppMode::Plan`。
- [x] **模式切换 UI** — `crates/tui/src/tui/app/state.rs:56` 定义 `AppMode`，`crates/tui/src/commands/` 的 `switch_mode`（`crates/tui/src/tui/ui/view_dispatch.rs:560`），引擎侧 `crates/tui/src/core/engine.rs:371/812/1322/1603` 持有并切换 `current_mode`。
- [x] **子智能体权限/模型隔离** — `custom_agents.rs` 的 `tools` 白名单 + `model` 路由，配合 `crates/tui/src/tools/subagent/runner.rs`。
- [x] **子智能体编排（远超 kilocode）** — `crates/tui/src/tools/subagent/` 下 `mailbox.rs`/`bus.rs`/`task_claim.rs`/`decomposer.rs`/`aggregator.rs`/`worktree` 隔离。kilocode 仅有 `tool/task.ts` 一个简单派发工具，**无 mailbox、无消息总线、无任务认领、无 worktree 隔离**。这是 mimofan 的显著净领先项。

### 待添加

- [ ] **用户可定义的 primary mode（顶层模式），而非仅 3 个硬编码枚举**
  - mimofan 验证：`grep -rn "enum AppMode" -A 30 crates --include='*.rs'` → `crates/tui/src/tui/app/state.rs:56-60` 完整定义仅 `{ Agent, Yolo, Plan }` 三个变体，**是编译期固定的权限姿态枚举，用户无法新增**。而 `custom_agents.rs` 加载的自定义 agent 只能作为**子智能体**被派发，无法成为顶层对话模式（`CustomAgentDef` 无 `mode: primary|subagent` 概念）。
  - kilocode 落点：`agent.ts:53` 的 `mode: Schema.Literals(["subagent", "primary", "all"])` + `agent.ts:479-506` 的 `defaultAgent()` 解析（支持 `cfg.default_agent` 指定任意 agent 为默认主模式）。
  - 为什么值得做：mimofan 已经有了自定义 agent 的**全部零件**（frontmatter 解析、工具白名单、模型路由、prompt 覆盖），差的只是让这些定义能挂到顶层模式位上——即给 `CustomAgentDef` 加一个 `mode` 字段，并让 `AppMode` 从闭合枚举改为 `{ 内置三种 } ∪ { 自定义 }`。用户价值明确：想要一个常驻的 "Reviewer 模式"（只读+严格 prompt）或 "Architect 模式"（禁编辑+强制先出方案）时，目前只能靠 Plan 模式凑合。改造面集中在 `state.rs` 与 `engine.rs` 的 mode 处理，属于中等成本、高感知收益。

- [ ] **用自然语言生成 agent 定义**
  - kilocode 落点：`agent.ts:88-101` 的 `generate` 接口 + `agent/generate.txt` 提示词，输入一句描述，产出 `{identifier, whenToUse, systemPrompt}`。
  - mimofan 验证：`crates/tui/src/tools/subagent/` 下有 `naming.rs`（仅命名）、`decomposer.rs`（任务分解，非 agent 定义生成），无从描述生成 agent 定义文件的能力。
  - 为什么值得做：降低自定义 agent 的上手门槛。优先级低于上一条，属于锦上添花。


---

## 5. 会话生命周期、快照与任务管理

### 已具备

- [x] **checkpoint / 快照** — `crates/tui/src/snapshot/`、`crates/tui/src/session_manager/`（`checkpoint` 关键字命中 40 个文件）。kilocode 对应 `packages/opencode/src/snapshot/index.ts`（1008 行）。
- [x] **会话 fork** — mimofan `fork` 命中 34 个文件。kilocode 对应 `kilocode/session/` 相关模块。
- [x] **回退某一轮** — `crates/tui/src/tools/revert_turn.rs`。kilocode 对应 `session/revert.ts`（214 行）。
- [x] **离线队列 / 恢复** — `crates/tui/src/session_manager/`。kilocode 对应 `kilocode/session/prompt-queue.ts`、`kilocode/session-resume/`、`kilocode/task-resume.ts`。
- [x] **任务管理与依赖图（领先）** — `crates/tui/src/task_manager/`、`crates/tui/src/tools/tasks.rs`、`crates/tui/src/tools/todo.rs`（含 `is_blocked`/`unmet_dependencies`/`ready_ids`/`blocked_ids`）。kilocode 只有 `tool/todo.ts` + `session/todo.ts` 的**扁平 todo 列表，无依赖关系建模**。mimofan 净领先。
- [x] **worktree 隔离** — mimofan `worktree` 命中 18 个文件，含子智能体 worktree 隔离。kilocode 对应 `packages/opencode/src/worktree/index.ts`、`kilocode/worktree-family.ts`、`kilocode/primary-worktree.ts`、`kilocode/worktree-cleanup.ts`。
- [x] **会话分享 / 导出** — `crates/tui/src/commands/groups/project/share.rs:19`。kilocode 对应 `share/session.ts`、`share/share-next.ts`、`kilocode/session-export/`、`kilocode/session-portability/`（kilocode 的云端分享更成体系，但 mimofan 有本地等价物）。
- [x] **代码评审** — `crates/tui/src/tools/review.rs`、`crates/tui/src/tui/auto_review.rs`。kilocode 对应 `kilocode/review/`（`review.ts`/`worktree-diff.ts`）。

### 待添加

- [ ] **后台长任务（detached background job）**
  - kilocode 落点：`packages/opencode/src/background/job.ts` + `packages/core/src/background-job.ts` + `kilocode/background-process/`。允许把长耗时任务放到后台执行，会话可继续交互，任务完成后回收结果。
  - mimofan 验证：`grep -rn "background_job\|BackgroundJob" crates --include='*.rs'` → **0 命中**。虽然 `crates/tui/src/tools/shell.rs`、`crates/tui/src/scheduler/`、`crates/tui/src/runtime_threads/` 有后台线程与调度设施，但没有"把一个 agent 任务整体 detach 到后台、稍后回收"的抽象。
  - 为什么值得做：终端场景下用户经常要跑长构建/长测试，当前只能占住会话等待。mimofan 已有 `scheduler/`、`fleet/`、子智能体 runtime 等零件，补一层 job 抽象成本不高。

---

## 6. 记忆系统

### 已具备

- [x] **文件型结构化记忆** — `crates/tui/src/tools/remember.rs` + `crates/tui/src/memory.rs`。
- [x] **向量语义记忆** — `crates/tui/src/tools/remember_vector.rs` + `crates/tui/src/vector_memory/` + `crates/memory/`（`compressor.rs`/`embedding.rs`/`injector.rs`/`knowledge.rs`/`vector.rs`/`optimization.rs`）。
- [x] **记忆自动注入系统提示** — `crates/memory/src/injector.rs`。
- [x] **记忆压缩与优化** — `crates/memory/src/compressor.rs`、`optimization.rs`。

kilocode 对应 `packages/kilo-memory/`（59 文件：`capture/`、`decisions.ts`、`recall/`、`marker-meta.ts`、`autosave-status.ts`）+ `opencode/src/tool/recall.ts` + `kilocode/memory/`（`turn.ts`/`marker.ts`/`ports.ts`）。

**双方设计取向不同**：kilocode 侧重「决策捕获」（`decisions.ts`、按 turn 打 marker 自动存档），mimofan 侧重「分类记忆 + 向量召回」。mimofan 的向量层（hnsw+sled 本地库）比 kilocode 更自足——kilocode 的 recall 更多依赖云端。整体判定为**对等偏领先**，无需补齐项。

值得借鉴但不作为待办：kilocode 的 `autosave-status.ts` + `marker.ts` 让记忆写入对用户完全无感（按轮次自动落盘并在 UI 显示状态），mimofan 的 `remember` 目前更依赖模型主动调用。这属于策略调优而非能力缺失。


---

## 7. 工具体系、Provider 与基础设施

### 已具备

- [x] **工具延迟加载 / 按需激活** — `crates/tui/src/core/engine/tool_catalog.rs`（BM25 检索 + 中途激活）。kilocode 无对等物，其 `tool/registry.ts` 是一次性全量注册。**mimofan 领先。**
- [x] **文件读写/编辑/patch** — `crates/tui/src/tools/file.rs`、`apply_patch.rs`、`fim.rs`（DeepSeek FIM 端点）。kilocode 对应 `tool/read.ts`/`write.ts`/`edit.ts`/`apply_patch.ts`。
- [x] **搜索** — `crates/tui/src/tools/search.rs`、`file_search.rs`。kilocode 对应 `tool/grep.ts`/`glob.ts`（均基于 ripgrep）。
- [x] **Shell 执行与安全** — `crates/tui/src/tools/shell.rs`、`shell_tools.rs`、`crates/tui/src/command_safety/`、`crates/tui/src/shell_dispatcher/`、`crates/execpolicy/`。kilocode 对应 `tool/shell.ts`、`tool/shell/`、`kilocode/bash-hierarchy.ts`。
- [x] **LSP 集成** — `crates/tui/src/lsp/`（23 文件）。kilocode 对应 `tool/lsp.ts`、`src/lsp/`、`tool/diagnostics.ts`；mimofan 有 `tools/diagnostics.rs` 对应。
- [x] **Web 抓取与搜索** — `crates/tui/src/tools/fetch_url.rs`、`web_search.rs`、`web_run.rs`。kilocode 对应 `tool/webfetch.ts`、`websearch.ts`、`mcp-websearch.ts`。
- [x] **Skills 体系** — `crates/tui/src/skills/`、`crates/tui/src/skill_state/`、`crates/tui/src/tools/skill.rs`（`skills` 命中 48 个文件）。kilocode 对应 `src/skill/discovery.ts`、`tool/skill.ts`、`kilocode/skills/`。
- [x] **MCP 客户端 + 服务端** — `crates/mcp/`、`crates/tui/src/mcp/`、`crates/tui/src/mcp_server/`、`crates/tui/src/mcp_server_backend/`。kilocode 对应 `src/mcp/`（仅客户端 + `kilocode/mcp-oauth-callback.ts`）。**mimofan 额外提供 MCP server 形态，领先。**
- [x] **沙箱** — `crates/tui/src/sandbox/`（seatbelt / opensandbox / policy）。kilocode 对应 `packages/kilo-sandbox/`（`seatbelt.ts`/`bubblewrap.ts`/`network.ts`/`proxy.ts`）。kilocode 多了 bubblewrap（Linux）与网络代理层，mimofan 有 `crates/tui/src/network_policy/` 对应网络侧。判定对等。
- [x] **权限与审批** — `crates/tui/src/core/engine/approval.rs`、`crates/tui/src/tools/approval_cache.rs`、`crates/tui/src/decision_gate/`、`crates/tui/src/workspace_trust/`。kilocode 对应 `src/permission/`、`core/src/permission.ts`。
- [x] **Provider 抽象与模型目录** — `crates/tui/src/model_catalog/`、`model_registry/`、`model_inventory/`、`model_profile/`、`crates/config/src/models_dev.rs`（models.dev 数据源，与 kilocode `core/src/models-dev.ts:166` 同源）。
- [x] **模型路由** — `crates/tui/src/model_routing/`、`crates/tui/src/route_runtime/`、`crates/tui/src/request_tuning/`。kilocode 对应 `provider/provider.ts`、`provider/transform.ts`。
- [x] **成本追踪** — `crates/tui/src/pricing/`、`crates/tui/src/cost_status/`。kilocode 在 `session/*.ts` 多处计费 + `kilo-gateway/` 的 balance/`kilocode/balance-refresh.ts`。
- [x] **ACP / 编辑器协议** — `crates/tui/src/acp_server/`、`crates/app-server/`。kilocode 对应 `src/acp/`、`kilocode/acp/`。
- [x] **审计与资源遥测** — `crates/tui/src/audit/`、`crates/tui/src/resource_telemetry/`、`crates/tui/src/runtime_log/`。
- [x] **多模态输入** — `crates/tui/src/vision/`（40 文件）、`crates/tui/src/tools/image_ocr.rs`、`crates/tui/src/tools/speech.rs`。kilocode 对应 `src/image/`、`kilo-vscode/src/speech-to-text/`。
- [x] **用户提问工具** — `crates/tui/src/tools/user_input.rs`。kilocode 对应 `tool/question.ts`、`src/question/`。
- [x] **自定义命令 / 工作流** — `crates/tui/src/commands/`、`crates/tui/src/automation_manager/`、`crates/tui/src/tools/automation.rs`。kilocode 对应 `src/command/`、`kilocode/command-files.ts`、`kilocode/workflows-migrator.ts`。
- [x] **配置 UI** — `crates/tui/src/config_ui/`。kilocode 对应 `kilo-vscode/src/SettingsEditorProvider.ts`（GUI 形态）。

### 待添加

- [ ] **Notebook（.ipynb）读写支持**
  - kilocode 落点：`packages/opencode/src/kilocode/notebook/`（`service.ts` + `protocol.ts`），并在 `tool/registry.ts:34` 注册进工具表。
  - mimofan 验证：`grep -rn "notebook\|ipynb" crates --include='*.rs' -i` → 仅命中 `crates/tui/src/core/engine/turn_loop.rs:2580` 一处**字符串字面量** `"NotebookEdit"`（出现在写类工具名匹配列表中，属于对外部工具名的兼容枚举），**无任何 .ipynb 解析/读写实现**。
  - 为什么值得做：数据科学/ML 用户占比不低，`.ipynb` 是 JSON 结构，用通用 `read`/`edit` 工具处理会把整个 JSON（含 base64 输出、执行计数）灌进上下文，既污染又易改坏。需要一个 cell 级的读写工具。成本中等，收益取决于目标用户画像。

- [ ] **MCP 市场（发现 + 一键安装）**
  - kilocode 落点：`packages/kilo-vscode/src/services/marketplace/`（`api.ts`/`installer.ts`/`relevance.ts`/`detection.ts`/`notifier.ts`）+ `MarketplacePanelProvider.ts`。
  - mimofan 验证：`grep -rn "marketplace" crates --include='*.rs' -i` → 仅 1 处命中，且是 `crates/tui/src/tui/ui/version_check.rs:17` 的**中文注释**（"CodeBuddy 的 marketplace 自动更新用的是同一量级（24h）"），与 MCP 无关。确认无实现。
  - 为什么值得做：mimofan 已有完整 MCP 客户端，但用户得手工写配置才能装 server。市场能显著降低 MCP 生态的使用门槛。注意 kilocode 的市场面板是 VS Code webview，mimofan 需做**终端等价物**（TUI 列表 + 搜索 + 一键写入配置），`crates/tui/src/config_ui/` 与 `crates/tui/src/palette/` 可复用。属于中等优先级。


---

## 8. 提示词工程辅助与开发者体验

这一域 mimofan 有两处明确缺口，都属于"小而高频"的体验增强。

### 待添加

- [ ] **Enhance Prompt（草稿提示词自动改写）**
  - kilocode 落点：`packages/opencode/src/kilocode/enhance-prompt.ts`。用一段专用 system instruction（`INSTRUCTION`，enhance-prompt.ts:11-18）把用户的草稿提示词改写得更清晰，**关键设计是把用户输入严格当作"待改写的源文本"而非"要执行的指令"**（防注入），并有 `clean()` 剥离 markdown fence 与包裹引号。VS Code 侧有 `enhance-prompt-error.ts` 做错误提示。
  - mimofan 验证：`grep -rn "enhance_prompt\|improve_prompt\|rewrite_prompt" crates --include='*.rs'` → **0 命中**。
  - 为什么值得做：实现成本极低（一次无工具的 `generateText` 调用 + 一个快捷键），但对提示词写得含糊的用户帮助明显，是很多 agent 产品的标配。mimofan 已有 `crates/tui/src/prompts/`、`crates/tui/src/model_routing/`（可路由到 fast 模型）、`crates/tui/src/palette/`（挂入口），零件齐全。

- [ ] **Commit message 自动生成**
  - kilocode 落点：`packages/opencode/src/kilocode/commit-message/`（`generate.ts`/`git-context.ts`/`types.ts`），基于 staged diff 生成提交信息。
  - mimofan 验证：`grep -rn "commit_message\|generate_commit" crates --include='*.rs'` → **0 命中**。mimofan 有 `crates/tui/src/tools/git.rs`、`git_history.rs`，但无专门的 commit message 生成。
  - 为什么值得做：终端 agent 的天然高频场景，且 mimofan 已有 git 工具链和 diff 能力，接一个小模型调用即可。成本低。

### 已具备（此域）

- [x] **提示词分区与缓存** — `crates/tui/src/prompt_zones/`、`crates/tui/src/prefix_cache/`、`crates/tui/src/llm_response_cache/`。kilocode 有 `session/prompt/`、`session/system.ts` 但无分区缓存概念，mimofan 领先。
- [x] **项目上下文文档自动发现** — `crates/tui/src/project_doc/`、`crates/tui/src/project_context/`、`crates/tui/src/project_context_cache/`、`crates/tui/src/workspace_discovery/`。kilocode 对应 `core/src/instruction-context.ts`、`session/instruction.ts`、`kilocode/rules-migrator.ts`。
- [x] **FIM 内联补全** — `crates/tui/src/tools/fim.rs`。kilocode 对应 `kilo-gateway/src/autocomplete.ts`、`fim.ts`、`mistral-fim-endpoint.ts`（kilocode 支持 kilo/mistral/inception 三家补全 provider，覆盖面更广，但 mimofan 有等价能力）。
- [x] **系统提示词组装** — `crates/tui/src/prompts/`。kilocode 对应 `session/system.ts`、`kilocode/system-prompt.ts`。

---

## 9. 因形态差异不适用（不计入待办）

以下能力依赖 VS Code / GUI 宿主 API，mimofan 作为终端 TUI 无需对标。**注意：仅限真正依赖编辑器 API 的部分**——kilocode 的 CLI 形态能力已在前文正常对标。

| kilocode 能力 | 落点 | 不适用理由 |
|---|---|---|
| 编辑器内联 diff 视图 | `kilo-vscode/src/diff/`、`DiffVirtualProvider.ts` | 依赖 VS Code `TextDocumentContentProvider`。mimofan 有终端等价物：`crates/tui/src/tools/diff_format.rs` + TUI 渲染 |
| Webview 设置面板 | `kilo-vscode/src/SettingsEditorProvider.ts`、`packages/kilo-ui/` | 依赖 webview。mimofan 终端等价物：`crates/tui/src/config_ui/` |
| 浏览器自动化 | `kilo-vscode/src/services/browser-automation/browser-automation-service.ts`（仅 166 行） | 依赖 VS Code 宿主管理 Chromium 生命周期，且体量很小、非核心。终端下若需要可用 MCP 的 playwright server 覆盖 |
| 图像生成 | `kilo-vscode/src/image-generation/` | 产物需 GUI 预览才有意义（`image-preview.ts`），终端展示价值低 |
| 语音输入 | `kilo-vscode/src/speech-to-text/` | 依赖宿主麦克风权限。mimofan 有 `crates/tui/src/tools/speech.rs` 部分覆盖 |
| 子智能体可视化面板 | `kilo-vscode/src/SubAgentViewerProvider.ts` | GUI 树形视图。mimofan 有 TUI 侧 fleet/subagent 视图（`crates/tui/src/tui/views/fleet_setup.rs`） |
| MCP 市场**面板** | `kilo-vscode/src/MarketplacePanelProvider.ts` | 仅"面板 UI"不适用；**市场能力本身已列入第 7 节待办**，需做终端等价物 |
| Anaconda / JetBrains 宿主集成 | `kilo-vscode/src/anaconda-desktop/`、`packages/kilo-jetbrains/` | 特定 IDE 宿主绑定 |
| 云端会话与网关 | `packages/kilo-gateway/`、`packages/server/`、`kilocode/cloud/` | 属于 kilocode 的商业化托管服务（500+ 模型代理、余额计费），非开源 agent 的通用能力；mimofan 走 BYOK 直连路线，是产品定位差异而非能力缺失 |

---

## 10. 遥测与可观测

- [x] **审计日志** — `crates/tui/src/audit/`。
- [x] **资源遥测** — `crates/tui/src/resource_telemetry/`、`crates/tui/src/runtime_log/`、`crates/tui/src/logging/`。
- [x] **成本/状态展示** — `crates/tui/src/cost_status/`、`crates/tui/src/status/`、`crates/tui/src/retry_status/`。
- [x] **错误分类** — `crates/tui/src/error_taxonomy/`。kilocode 对应 `session/message-error.ts`、`kilocode/kilo-errors.ts`。
- [x] **评测框架** — `crates/tui/src/eval/`。kilocode 无对等物（仅 `perf/`），mimofan 领先。

关于结构化产品遥测（kilocode `packages/kilo-telemetry/`，含 `identity.ts`/`events.ts`/`client.ts`，以及 `Telemetry.trackToolUsed(...)` 这类埋点，见 `tool/warpgrep.ts:35`）：mimofan 有 `telemetry` 关键字命中（`crates/core/src/lib.rs`、`crates/config/src/lib.rs` 等），但性质是**资源/运行时指标**，非产品行为埋点。这属于**产品策略选择而非能力缺失**——是否上报用户行为数据涉及隐私取向，不列为待办，仅记录差异。


---

## 11. 建议优先补齐的 Top 5

排序依据：**用户可感知收益 ÷ 实现成本**，并优先选择"mimofan 已有零件、只差组装"的项。

### 1. SWE-Pruner 式工具输出级自适应裁剪 —— 最高优先级

- **落点参考**：`kilocode/swe-pruner.ts`
- **理由**：这是全表中投入产出比最高的一项。上下文管理本是 mimofan 的强项（三套压缩范式），但现有 `large_output_router` 全部是**事后**处理——输出已产生才做综述，模型意图信息已经丢失。SWE-Pruner 让模型在**调用工具时**就声明关注点，裁剪有明确目标函数且发生在入上下文之前，与现有三套范式完全正交、可叠加。
- **成本估计**：低。只需给 `read`/`grep`/`shell` 三个 `ToolSpec` 加一个可选参数，复用 `model_routing/` 的 fast 模型做筛选，护栏常量与回退逻辑可直接照搬 kilocode 实现。
- **风险**：低。失败即回退全量输出，不改变现有行为。

### 2. 代码库语义索引 + `codebase_search` 工具 —— 最大能力缺口

- **落点参考**：`packages/kilo-indexing/`（本地引擎）、`opencode/src/tool/warpgrep.ts`（工具接口形态）
- **理由**：唯一一处"竞品成体系、mimofan 连入口都没有"的能力域。影响最高频场景——大仓库里回答"这个功能在哪实现"。当前只能 grep 关键词，对语义化查询无能为力，而 `symbol_index` 仅 104 行行首前缀匹配，召回很弱。
- **成本估计**：中。**但远低于从零**——`hnsw-rs`+`sled` 向量库（`crates/memory/src/vector.rs`）、`EmbeddingService`（`crates/memory/src/embedding.rs`）、优雅降级机制都已就绪，缺的只是代码分块器 + 文件监听增量索引 + 工具注册三件。
- **落地建议**：分块先用"函数/类边界正则 + 滑动窗口"起步，不必一开始就引 tree-sitter（mimofan 当前无该依赖）。务必沿用 `vector_memory/mod.rs:18-25` 说明的 `take_embedder()` 模式规避 `!Send` 约束。
- **风险**：中。需要注意索引构建的性能与磁盘占用，建议默认对大仓库惰性/增量构建。

### 3. 用户可定义的 primary mode

- **落点参考**：`agent.ts:53` 的 `mode: subagent|primary|all`
- **理由**：mimofan 已有自定义 agent 的**全部零件**（frontmatter 解析、工具白名单、模型路由、prompt 覆盖，见 `custom_agents.rs`），唯独这些定义只能当子智能体用，无法挂到顶层。用户想要常驻的 "Reviewer 模式" / "Architect 模式" 时只能拿 Plan 模式凑合。
- **成本估计**：中。给 `CustomAgentDef` 加 `mode` 字段，把 `AppMode`（`state.rs:56`）从闭合枚举改为"内置 ∪ 自定义"，改造 `engine.rs` 的 mode 处理路径（:371/:812/:1322/:1603）。
- **风险**：中。`AppMode` 是 `Copy` 枚举且被多处模式匹配，改为开放集合需要留意穷尽性匹配与序列化兼容。

### 4. Enhance Prompt + Commit message 生成（打包做）

- **落点参考**：`kilocode/enhance-prompt.ts`、`kilocode/commit-message/`
- **理由**：两项都是"小而高频"的体验增强，各自都只是一次 fast 模型调用 + 一个入口，合并实现摊薄成本。commit message 尤其契合终端 agent 的工作流。
- **成本估计**：低。`prompts/`、`model_routing/`、`palette/`、`tools/git.rs` 零件齐全。
- **注意**：enhance-prompt 必须照抄 kilocode 的**防注入设计**——严格把用户输入当作待改写源文本，而非待执行指令（`enhance-prompt.ts:11-18`）。

### 5. MCP 市场（终端形态）

- **落点参考**：`kilo-vscode/src/services/marketplace/`（`api.ts`/`installer.ts`/`relevance.ts`）
- **理由**：mimofan 的 MCP 客户端已完整，但用户必须手写配置才能装 server，这是 MCP 生态的主要使用门槛。补上后能放大已有 MCP 投入的价值。
- **成本估计**：中。需要做**终端等价物**而非照搬 webview 面板——TUI 列表 + 搜索 + 一键写配置，可复用 `crates/tui/src/config_ui/` 与 `crates/tui/src/palette/`。
- **优先级说明**：排在末位是因为它依赖外部 registry 数据源的可用性，且对不使用 MCP 的用户无感。

### 未进入 Top 5 但已记录

- Notebook（.ipynb）支持 —— 收益高度依赖目标用户画像，若 ML/数据科学用户占比高应上调优先级。
- 后台长任务 detach —— 有价值，但 mimofan 现有 `scheduler/`/`fleet/` 已部分缓解。
- 自然语言生成 agent 定义 —— 锦上添花，建议在第 3 项落地后再做。
- 结构化产品遥测 —— 属隐私取向的产品决策，不作为技术待办。

---

## 12. 结论

**mimofan 的整体能力面不弱于 kilocode，在多个维度显著领先**：上下文压缩（三套范式 vs 单套，且独有 prefix-cache 友好的软缝）、子智能体编排（mailbox/bus/task_claim/worktree 隔离 vs 单个 task 工具）、任务依赖图（vs 扁平 todo）、工具延迟加载（BM25 按需激活，kilocode 无）、MCP 双形态（client+server）、评测框架。

**真正的差距集中且清晰**，只有一处结构性缺口（代码库语义索引）和若干可快速补齐的体验项。且即便是最大的那处缺口，mimofan 也已具备本地向量库与 embedding 服务，属于"组装"而非"从零建设"。

**一个必须记录的认知修正**：kilocode 当前已是 opencode fork 的多形态 monorepo，其 CLI/TUI 与 mimofan 是正面同形态竞品，不能再用"它是 VS Code 扩展所以不可比"来解释差距。真正不适用的仅剩第 9 节所列的编辑器 API 依赖项与商业化托管服务。
