# 更新日志

本项目的所有重要变更都记录在此。版本遵循[语义化版本控制](https://semver.org/)，从工作区根目录（`Cargo.toml` → `[workspace.package] version`）递增。

## [0.0.17] - 2026-08-11

本轮（LoopX 续作）补齐研究编排范式全链路命令壳与 GitHub 共同作者署名。

### Added
- **可机评优化回路 `/evolve`（[#751](https://github.com/XiaomingX/mimofan/issues/751)）**：新增 `/evolve <goal>` 命令（别名 `/optimize`），进入 Agent 模式运行优化回路——锁定 baseline + evaluator（`evolve::lock_baseline`，拒绝覆盖、防篡改）、派发候选、由外部 evaluator 程序裁决 `valid && improved`（`evolve::run_evaluator_on` / `EvaluatorOutput::is_winner`）、胜出者经 `evolve::record_candidate` 留痕并作为下一轮父本。evaluator 拥有正确性，代理不自我报分。
- **可复现性纪律 `/repro`（[#754](https://github.com/XiaomingX/mimofan/issues/754)）**：新增 `/repro <brief>` 命令（别名 `/reproducibility` `/brief`），固化 `BRIEF.md` 唯一事实源 + `env_snapshot.json` 环境快照（rust/python 版本与依赖锁哈希）+ `provenance.jsonl` 起点留痕（`repro::write_brief` / `snapshot_env` / `record_provenance`），默认零行为变更。
- **研究成果物汇总 `/artifact`（[#750](https://github.com/XiaomingX/mimofan/issues/750)）**：新增 `/artifact <initiative_id> [--publish]` 命令（别名 `/publish`），进入 Agent 模式调用 `research_artifact::ArtifactInput::build` 汇总到 `initiatives/<id>/`（README.md + provenance.json）。`--publish` 走研究副作用闸门（[#753](https://github.com/XiaomingX/mimofan/issues/753)），`PublishRemote` 默认需显式授权，Auto 不自动推远程。
- **独立评审者 `/reviewer`（[#752](https://github.com/XiaomingX/mimofan/issues/752)）**：新增 `/reviewer [<initiative_id>]` 命令（别名 `/review`），只读审核 claim，调用 `reviewer::review` / `accepted_only` 下 Accepted/Rejected/Weak 判定（被反驳直接 Rejected；Strong 且未反驳 / Medium 有复现步骤且未反驳 → Accepted），作为 `/artifact` 公开章节的前置门。
- **GitHub 共同作者署名（对标 Claude Code `includeCoAuthoredBy`）**：`git_commit` 工具默认在 commit message 末尾追加 mimofan 共同作者 trailer（`🤖 Generated with [mimofan]` + `Co-Authored-By: mimofan <noreply@xiaoming.com>`），新增 `co_authored_by` 参数（默认 `true`，可关闭），并防重复追加（amend 安全）。GitHub 据此把 mimofan 显示为 co-author，真实 committer 不变。

### Docs
- 新增 `docs/NEW_CAPABILITIES_GUIDE.md`，介绍 `/evolve` `/repro` `/artifact` `/reviewer` 与 GitHub 共同作者署名的使用方法与配置方式。

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
