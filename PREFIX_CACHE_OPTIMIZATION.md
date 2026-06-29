# 前缀缓存与能力对标优化方案

对标 Claude Code、Codex CLI、Antigravity、OpenCode、Hermes Agent、OpenClaw 等主流 Coding Agent，
从 **Token 成本**、**提示词管理**、**自动更新**、**开发者使用体验** 四个维度分析差距与优化路径。

---

## 一、代码库现状总览

### 已实现的核心能力

| 维度 | 模块 | 文件 | 状态 |
|------|------|------|------|
| Token 成本 | 三层前缀模型 | `prefix_cache.rs` | ✅ 不可变前缀 + 仅追加历史 + 最新用户消息 |
| Token 成本 | 前缀稳定性管理器 | `prefix_cache.rs` | ✅ SHA-256 指纹 + 漂移检测 + 工具目录缓存 |
| Token 成本 | 缓存预热请求 | `client/chat.rs:600-697` | ✅ 仅静态前缀 + `max_tokens=8` |
| Token 成本 | Anthropic 缓存断点 | `client/anthropic.rs:376-446` | ✅ 3 个 ephemeral 断点 |
| Token 成本 | 缓存命中监控 | `runtime_threads.rs` | ✅ 每轮 `prompt_cache_hit_tokens` + footer chip |
| Token 成本 | 推理内容重放 | `client/chat.rs:1932` | ✅ 跨轮保持 `reasoning_content` |
| Token 成本 | 上下文压缩 | `compaction.rs` | ✅ 800K token 阈值 + 保留 4 条近期消息 |
| Token 成本 | Cache Guard CI | `tests/cache_guard.rs` | ✅ 模拟前缀缓存 + 命中率阈值检查 |
| 提示词管理 | 系统提示分层 | `prompts.rs:813-826` | ✅ 静态→动态排序，volatile-content 边界 |
| 提示词管理 | Harness 压缩策略 | `config/harness.rs` | ✅ Default/PrefixCache/Aggressive 三档 |
| 提示词管理 | Constitution 基础提示 | `constitution.json` | ✅ 用户可自定义基础提示 |
| 提示词管理 | 记忆系统 | `tools/remember.rs` | ✅ 用户记忆文件 + remember 工具 |
| 开发者体验 | 子代理系统 | `agent` crate | ✅ 可配置深度 + fleet workers |
| 开发者体验 | MCP 集成 | `mcp` crate | ✅ 工具服务器支持 |
| 开发者体验 | 会话管理 | `session_manager.rs` | ✅ 恢复、分叉、保存 |
| 开发者体验 | 快照系统 | `snapshot/` | ✅ 每轮前后工作区快照 |
| 开发者体验 | 成本追踪 | `cost_status.rs` | ✅ USD/CNY + 每轮/每会话 |
| 开发者体验 | Git 工具集 | `tools/git*.rs` | ✅ status/diff/history/blame/show |
| 开发者体验 | 文件工具集 | `tools/file*.rs` | ✅ read/write/edit/search/apply_patch |
| 开发者体验 | Shell 工具 | `tools/shell.rs` | ✅ 命令执行 + 安全分类 |
| 开发者体验 | Web 工具 | `tools/web_*.rs` | ✅ 搜索/抓取/浏览 |
| 开发者体验 | LSP 集成 | `tools/apply_patch.rs` | ✅ patch 后自动诊断 |
| 开发者体验 | 图像 OCR | `tools/image_ocr.rs` | ✅ 图像文字识别 |
| 开发者体验 | Hooks 系统 | `hooks` crate | ✅ 工具前后钩子 |
| 开发者体验 | Plan 模式 | `tools/plan.rs` | ✅ 只读规划模式 |
| 开发者体验 | 回滚工具 | `tools/revert_turn.rs` | ✅ 撤销工作区变更 |
| 开发者体验 | 代码审查 | `tools/review.rs` | ✅ 文件/diff/PR 审查 |
| 开发者体验 | 目标系统 | `tools/goal.rs` | ✅ 运行时目标 + 续行提示 |
| 开发者体验 | Todo 工具 | `tools/todo.rs` | ✅ 任务清单管理 |

---

## 二、对标分析：缺失的高频刚需模块

### A. Token 成本维度

| # | 缺失能力 | 对标工具 | 优先级 | 说明 |
|---|---------|---------|--------|------|
| A1 | 压缩后自动缓存预热 | Claude Code | **P0** | 压缩后前缀剧变，下一次请求全量缓存未命中，浪费一次完整输入 token |
| A2 | 命中率下降自动响应 | Codex | **P1** | footer chip 变红（<40%）但无自动触发预热或建议 |
| A3 | PrefixCache 压缩策略差异化 | — | **P1** | harness 配置了 `PrefixCache`，需确认压缩逻辑是否有差异化行为 |
| A4 | Token 预算告警 | Claude Code | **P2** | 无单次会话 token 上限告警，用户可能无意消耗大量 token |
| A5 | 输出 token 精细控制 | Codex | **P2** | 无按工具类型动态调整 `max_tokens` 的机制 |

### B. 提示词管理维度

| # | 缺失能力 | 对标工具 | 优先级 | 说明 |
|---|---------|---------|--------|------|
| B1 | 项目上下文自动注入 | Claude Code (CLAUDE.md) | **P0** | 已有 `AGENTS.md`/`constitution.json`，但缺少自动扫描 `CLAUDE.md`/`PROJECT.md` 等约定文件的机制 |
| B2 | 提示词版本管理 | OpenCode | **P2** | constitution.json 无版本追踪，修改后无法回溯 |
| B3 | 多语言提示词适配 | Hermes Agent | **P2** | 已有 locale 支持，但提示词模板未做多语言翻译 |

### C. 自动更新维度

| # | 缺失能力 | 对标工具 | 优先级 | 说明 |
|---|---------|---------|--------|------|
| C1 | 自更新机制 | Claude Code / Codex CLI | **P0** | 无 `mimofan update` 或后台检查更新的能力，用户需手动 `cargo install` |
| C2 | Git hooks 集成 | Claude Code | **P1** | 无 pre/post-commit hooks 自动触发检查的机制 |
| C3 | 自动提交建议 | Claude Code | **P1** | 成功编辑文件后无自动建议 `git commit` 的流程 |

### D. 开发者使用体验维度

| # | 缺失能力 | 对标工具 | 优先级 | 说明 |
|---|---------|---------|--------|------|
| D1 | TDD 工作流 | Claude Code / Codex | **P0** | 无内置"写测试→运行→写代码→验证"循环，需用户手动编排 |
| D2 | 批量文件编辑 | Claude Code | **P0** | `apply_patch` 是单次操作，无"搜索→批量替换"工作流 |
| D3 | 测试运行器集成 | Codex CLI | **P1** | `tools/test_runner.rs` 存在但未深度集成（如自动重跑失败测试） |
| D4 | 更好的错误恢复 | Antigravity | **P1** | 编译/测试失败后无结构化重试策略，依赖模型自行判断 |
| D5 | 交互式确认增强 | OpenClaw | **P2** | 高风险操作（如删除文件）的确认流程可更细粒度 |
| D6 | 会话搜索与标签 | OpenCode | **P2** | 会话管理无全文搜索和标签分类 |
| D7 | 使用分析仪表盘 | Hermes Agent | **P2** | 无 token 效率、成本趋势、工具使用频率的可视化分析 |

---

## 三、优化实施计划

### Phase 1: 压缩后自动缓存预热 (A1) — P0

**目标**: 压缩完成后立即发送 cache warmup 请求，避免下一次真实请求全量缓存未命中

**参考代码**:
- `crates/tui/src/client/chat.rs:600-697` — `build_cache_warmup_request()` 现有实现
- `crates/tui/src/runtime_threads.rs:2245` — `compact_thread()` 压缩流程
- `crates/tui/src/compaction.rs` — 压缩执行逻辑

**任务**:

1. 在 `compact_thread()` 压缩完成后，调用 `build_cache_warmup_request()` 发送预热请求
2. 使用压缩后的新消息列表构建预热请求
3. 添加日志记录预热结果

**验证**: 压缩后检查日志中是否有 warmup 请求发出

---

### Phase 2: 命中率监控与自动响应 (A2) — P1

**目标**: 当缓存命中率持续偏低时，自动触发缓存预热

**参考代码**:

- `crates/tui/src/runtime_threads.rs:1182` — `prompt_cache_hit_tokens` 追踪
- `crates/tui/src/prefix_cache.rs` — `PrefixStabilityManager` 漂移检测

**任务**:

1. 在 turn loop 中检测连续 N 轮命中率 < 40%
2. 检查 `PrefixStabilityManager` 是否报告前缀漂移
3. 若漂移且命中率低，自动发送 cache warmup 请求
4. 在 footer chip 中显示 "cache warming..." 状态

**验证**: 模拟前缀漂移场景，验证自动预热触发

---

### Phase 3: PrefixCache 压缩策略验证 (A3) — P1

**目标**: 确认 `HarnessCompactionStrategy::PrefixCache` 在压缩逻辑中有差异化行为

**参考代码**:

- `crates/config/src/harness.rs:42-47` — 策略定义
- `crates/tui/src/compaction.rs` — 压缩执行
- `crates/tui/src/seam_manager.rs:185` — `plan_compaction()` 调用

**任务**:

1. 搜索 `PrefixCache` 在压缩逻辑中的使用
2. 确认是否有差异化行为（如更保守的压缩阈值、保留更多近期消息）
3. 如缺失，实现 PrefixCache 策略的具体差异化逻辑

**验证**: 配置 PrefixCache 策略后，压缩行为与 Default 有可观察差异

---

### Phase 4: 自更新机制 (C1) — P0

**目标**: 支持 `mimofan update` 命令或后台检查更新

**对标**: Claude Code 自动检查新版本，Codex CLI 支持 `codex update`

**任务**:
1. 添加版本检查逻辑（对比 GitHub releases API）
2. 实现 `mimofan update` 子命令
3. 可选：启动时后台检查（非阻塞）

**验证**: `mimofan update` 能正确检测并提示新版本

---

### Phase 5: TDD 工作流集成 (D1) — P0

**目标**: 内置"写测试→运行→写代码→验证"循环

**对标**: Claude Code 的 `/tdd` 工作流，Codex 的 test-first 模式

**参考代码**:

- `crates/tui/src/tools/test_runner.rs` — 现有测试运行器
- `crates/tui/src/tools/verifier.rs` — 验证器

**任务**:

1. 定义 TDD 工作流状态机（red→green→refactor）
2. 集成 test_runner 自动重跑失败测试
3. 在 goal 系统中添加 TDD 目标模板

**验证**: 模型能自动执行写测试→失败→写代码→通过的循环

---

### Phase 6: 批量文件编辑 (D2) — P0

**目标**: 支持"搜索→批量替换"工作流

**对标**: Claude Code 的 MultiEdit 工具，Codex 的批量 apply

**参考代码**:

- `crates/tui/src/tools/apply_patch.rs` — 现有 patch 工具
- `crates/tui/src/tools/search.rs` — 搜索工具

**任务**:
1. 实现 `batch_edit` 工具：接收多个 (file, old, new) 对
2. 支持 dry-run 预览模式
3. 与 snapshot 系统集成，失败时自动回滚

**验证**: 单次调用完成多文件修改，失败时自动回滚

---

### Phase 7: Git hooks 集成 (C2) — P1

**目标**: 编辑文件后自动触发 lint/format/check

**对标**: Claude Code 编辑后自动运行 prettier/eslint

**任务**:
1. 在 `apply_patch` 成功后检测项目 git hooks
2. 自动运行 `pre-commit` 或 `pre-push` hooks
3. 将 hooks 结果反馈给模型

**验证**: 编辑 TypeScript 文件后自动运行 prettier

---

### Phase 8: Token 预算告警 (A4) — P2

**目标**: 单次会话 token 上限告警

**对标**: Claude Code 的 usage dashboard

**参考代码**:

- `crates/tui/src/cost_status.rs` — 现有成本追踪
- `crates/tui/src/session_manager.rs:120-170` — 会话成本快照

**任务**:

1. 在 settings.toml 中添加 `token_budget` 配置
2. 达到阈值时在 footer 显示警告
3. 超出阈值时要求用户确认继续

**验证**: 设置 token_budget=100000，接近时显示警告

---

## 四、优先级总结

| 优先级 | 编号 | 能力 | 预估工作量 |
|--------|------|------|-----------|
| **P0** | A1 | 压缩后自动缓存预热 | 0.5 天 |
| **P0** | C1 | 自更新机制 | 1 天 |
| **P0** | D1 | TDD 工作流集成 | 2 天 |
| **P0** | D2 | 批量文件编辑 | 1 天 |
| **P1** | A2 | 命中率下降自动响应 | 1 天 |
| **P1** | A3 | PrefixCache 压缩策略验证 | 0.5 天 |
| **P1** | C2 | Git hooks 集成 | 1 天 |
| **P1** | C3 | 自动提交建议 | 0.5 天 |
| **P1** | D3 | 测试运行器深度集成 | 1 天 |
| **P1** | D4 | 结构化错误恢复 | 1 天 |
| **P2** | A4 | Token 预算告警 | 0.5 天 |
| **P2** | A5 | 输出 token 精细控制 | 1 天 |
| **P2** | B1 | 项目上下文自动注入 | 0.5 天 |
| **P2** | B2 | 提示词版本管理 | 0.5 天 |
| **P2** | D5 | 交互式确认增强 | 0.5 天 |
| **P2** | D6 | 会话搜索与标签 | 1 天 |
| **P2** | D7 | 使用分析仪表盘 | 2 天 |

---

## 五、结论

代码库的前缀缓存设计已相当成熟（三层模型、指纹检测、分层提示、Anthropic 断点），**在 Token 成本维度已接近 Claude Code/Codex 水平**。

**最大差距在开发者体验维度**：缺少自更新、TDD 工作流、批量编辑、Git hooks 集成等"开箱即用"能力。这些是 Claude Code 和 Codex CLI 的核心卖点，也是用户选择工具时的关键决策因素。

建议按 P0→P1→P2 顺序实施，P0 项总工作量约 4.5 天，可在一周内完成。
