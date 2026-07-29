# Mimofan 候选需求跟踪

> 分析日期：2026-07-29
> 来源：verse.md 187 个候选项分析
> 工作区版本：v0.8.68+

## 一、已修复（commit 169275e）

| 编号 | 标题 | 修复内容 |
|------|------|----------|
| T1 | system prompt 路径浪费 tokens | `project_context.rs`/`prompts.rs`/`skills/mod.rs` 中 `display()` → `file_name()` |
| J2 | 大上下文模型 compaction 永不触发 | `DEFAULT_AUTO_COMPACT_MAX_CONTEXT_WINDOW_TOKENS` 1M → 1.1M |
| H7 | Token 统计永久失效 | `pricing.rs` 中 `reasoning_tokens` → `reasoning_replay_tokens` |

## 二、确认不存在（mimofan 已防护）

| 编号 | 标题 | 防护机制 |
|------|------|----------|
| T3 | 自动 compaction 无限循环 | 紧急恢复上限 2 次 + API 重试上限 3 次 + 振荡保护 |
| T4 | compaction 重新加载 system prompt | `merge_compaction_summary()` 追加摘要不重建 + hash 短路 |
| T6 | Observer 历史无限增长 | Composer 上限 1000 条 + RLM 上限 20 条 + trim_oldest |
| T8 | Token 消耗无限循环 | LLM/子智能体/截断/上下文恢复/流重试全部有界 |
| T9 | 429 错误导致无限回退循环 | `max_retries=3` + 指数退避（小隐患：Retry-After 可绕过 max_delay） |
| T11 | /btw 侧问题发送百万 token | `/btw` 功能不存在于本代码库 |
| T12 | OOM 10-20GB：无界 summary.diffs | 无 `diffs` 字段 + MAX_WORKING_SET_PATHS=24 + SUMMARY_INPUT_MAX_CHARS |
| T14 | compaction 变异 thinking block | pinned 消息 clone 不修改 + DeepSeek sanitizer + Anthropic 过滤 |
| T15 | reasoning_effort: null 拒绝 | 三个 API 构建器 None 时 early return |
| T18 | 标题级样板观察绕过 content_hash | content_hash 仅用于 TUI 渲染缓存 |
| T21 | footer 显示累计 token 用量 | 设计意图（StatusItem::Tokens 为"Session tokens"） |
| U3 | glob/grep 工具无超时 | 使用不同快捷键方案 |
| U11 | 子智能体执行期间 TUI 冻结 | v0.8.61 通过 tokio::spawn! 修复 |
| J1 | macOS stdin 检测常量错误 | 使用 Rust stdlib `std::io::IsTerminal` trait |
| J3 | 长会话 TUI 输入滚动退化 | saturating_sub 边界检查 + 帧率控制 + 同步输出 |
| J4 | TUI 粘贴文本 panic | Bracketed Paste + paste-burst 检测 + oversized_paste |
| J6 | 信息组件滚动跳动闪烁 | "Scroll Demon" 已修复（v0.8.18） |
| J7 | macOS 菜单栏图标不显示 | 纯 TUI 应用，无 tray icon |
| J8 | API 429/5xx 重试不足 | 3 次重试 + 指数退避 + Retry-After + 进程级速率限制 |
| J10 | /rewind 恢复编辑文件 | `revert_turn` 基于 side git 仓库快照，安全恢复 |
| J11 | TUI 跟随终端 ANSI 配色 | 完整颜色处理管线 + ColorCompatBackend |
| S1 | Mac M4 多 session 系统冻结 | 单实例 TUI + DELETE 日志模式 + 无并发竞争 |
| S2 | SQLite WAL 文件增长 | 未配置 WAL 模式 |
| S3 | /model 切换后 SQLite 崩溃 | 仅更新 threads 表字段，无 schema 变更 |
| S4 | 子智能体工具调用静默中止 | 300s 超时 + CancellationToken + API 超时三重保护 |
| S5 | Checkpoint rebuild 无限循环 | 线性迁移 v0→v4 + compact_messages_safe 单次操作 |
| S6 | 请求拒绝后永久挂起 | request_timeout: 120s + max_retries=3 |
| S7 | Plan 模式执行 bash 命令 | ShellPolicy::None + 工具排除 + 引擎拦截 + 沙箱 ReadOnly |
| S11 | 事件驱动无限循环 | channel 驱动 + max_steps 约束 + 无忙等待 |
| S13 | macOS sleep/wake 后 100% CPU | SLEEP_GAP_THRESHOLD=10s 检测 + 透明重试 |
| H2 | /model 命令响应 7+ 分钟 | turn 开始时解析一次，无阻塞 |
| H3 | 模型静默回退无通知 | 无自动回退，重试用同一模型 |
| H4 | 子代理完成消息干扰 | mailbox + event + parent completion 三通道 + 哨兵标记 |
| H5 | 禁用未使用 skill 注入 | 无 skill 注入系统 |
| H6 | 模型列表含不存在模型 | 模型来自 catalog/用户配置，未知返回 None |
| H8 | 就地压缩导致 session 无法结束 | compact_messages_safe 单次操作 + should_compact 纯谓词 |
| H9 | 上下文压缩器耗尽 provider 池 | 使用同一 LLM 客户端 + 降级本地裁剪 |
| H11 | 限流时 turn 丢失不重试 | 429 重试 3 次 + 指数退避 + 进程级协调 |
| H12 | 多实例模型切换互相污染 | 单实例 TUI + DELETE 日志模式 |
| O3 | 模型切换 reasoning drop 翻转 | reasoning_effort 仅对 XiaomiMimo 生效，Custom 空操作 |
| O4 | compaction 超时远低于 deadline | 3 次重试 + 指数退避 + request_timeout: 120s |
| O6 | TUI 选择器原语 | 19 种 ModalKind 完整选择器系统 |
| O8 | 可滚动文件查看器 | PagerView vim 风格导航 |
| C1 | 按 Provider/模型配置上下文窗口 | context_window_for_model + 用户可配置 + 路由预算 |
| C4 | filter-map 单次遍历优化 | 36 个 .map().filter() 均为 Option 惯用模式 |
| C8 | 大段用户输入折叠显示 | MAX_COMPOSER_DISPLAY_CHARS=4000 + oversized_paste_full_text |
| L2 | 成本追踪示例 | OfferingPricing + estimate_cost() + Models.dev |
| L3 | 断路器防止无限循环 | 多层约束：max_steps + MAX_STREAM_DURATION + max_retries |
| L4 | 上下文管理 | 自动压缩 + 75% 触发 + 紧急压缩 + 本地裁剪回退 |
| L6 | 安全命令执行 | ShellDispatcher + ExecPolicyEngine + SandboxManager + child_env |
| L7 | 沙箱执行 | SandboxPolicy 三级 + macOS Seatbelt + Linux Landlock |
| L8 | 门控机制 | ShellPolicy 按模式门控 + 子智能体角色门控 + Launch gate |
| L9 | 参数验证 | 工具输入验证 + command_safety + execpolicy 规则匹配 |

## 三、排查属实（需修复/改进）

### 高优先级

| 编号 | 标题 | 问题描述 | 修复计划 |
|------|------|----------|----------|
| T5 | Goal 模式空操作持续消耗 token | `goal_loop.rs:15-18` 明确"no continuation cap"，`token_budget`/`time_budget_seconds` 默认 None | 添加默认 continuation 上限（建议 50 次）或 token/time 预算 | [#74](https://github.com/XiaomingX/mimofan/issues/74) |
| H1 | SSE 心跳帧导致 stale 检测器失效 | `mcp.rs:721-807` 心跳帧被 `_ => {}` 静默丢弃，无空闲超时 | 实现心跳计时器 + 空闲超时重连机制 | [#75](https://github.com/XiaomingX/mimofan/issues/75) |
| O2/T13 | Prompt cache 滑动窗口 ~0% 命中率 | `prompts.rs:177,931` 承认"prompt-prefix bytes drifting turn-over-turn" | 稳定化 system prompt 前缀，分离静态/动态内容 | ✅ [#78](https://github.com/XiaomingX/mimofan/issues/78) 已修复 |

### 中优先级

| 编号 | 标题 | 问题描述 | 修复计划 |
|------|------|----------|----------|
| T7 | 子智能体观察污染主会话上下文 | `drain_subagent_completion_events()` 无单轮注入总量上限（每个 12K） | 添加单轮注入总量上限（建议 48K） | [#76](https://github.com/XiaomingX/mimofan/issues/76) |
| T10 | 超大工具结果阻止去重 | 无相同工具输出去重，同一调用 5 次每次 32KiB | 实现工具输出内容 hash 去重 | [#77](https://github.com/XiaomingX/mimofan/issues/77) |
| T16 | compaction 后重新打开已完成任务 | `extract_workflow_context()` 仅关键词匹配，不解析 checklist | 增强 workflow context 提取逻辑 | [#79](https://github.com/XiaomingX/mimofan/issues/79) |
| T17 | 配置过滤选项未连接到查询 | `footer_ui.rs:582` 硬编码空返回 | 删除死代码或实现过滤器连接 | [#80](https://github.com/XiaomingX/mimofan/issues/80) |
| T20/C5 | 更多模型不支持推理强度参数 | `apply_reasoning_effort()` 仅对 XiaomiMimo 生效 | 为 Custom provider 添加模型白名单 | [#81](https://github.com/XiaomingX/mimofan/issues/81) |
| C9 | LSP 文件跟踪内存泄漏 | `opened: HashMap` 无清理/无 didClose | 实现 LSP didClose + 生命周期管理 | [#82](https://github.com/XiaomingX/mimofan/issues/82) |

### 低优先级

| 编号 | 标题 | 问题描述 | 修复计划 |
|------|------|----------|----------|
| T2 | 长会话 compaction 不适配免费模型 | 仅按 500K 阈值二分，128K 和 400K 模型相同限制 | 改为按模型 context window 多档分级 | [#84](https://github.com/XiaomingX/mimofan/issues/84) |
| T22 | 暴露每轮 token 用量 | footer tok 芯片仅显示累计值 | 在 footer 增加每轮 token 差值显示 | [#85](https://github.com/XiaomingX/mimofan/issues/85) |
| S10 | 后台 shell 临时目录泄漏 | rlm 硬编码路径无 UUID + hooks 绕过沙箱 | 为 rlm 临时目录添加 UUID | [#86](https://github.com/XiaomingX/mimofan/issues/86) |
| J5 | MCP 连接超时阻塞 10 秒 | 默认 connect_timeout 10s，首次调用阻塞 | 实现预连接或异步发现 | [#83](https://github.com/XiaomingX/mimofan/issues/83) |

### 功能缺失（非 Bug，列为未来需求）

| 编号 | 标题 | 描述 |
|------|------|------|
| J9 | @file 文件提及 frecency 排序 | SessionContextReference 无 frecency 排序 |
| C2 | push/pop 上下文栈 + /digest 蒸馏 | 无上下文栈，无蒸馏功能 |
| C3 | 长会话任务锚点重注入 | 压缩保留部分上下文但无显式锚点重注入 |
| C6 | /workflows 延迟执行 | 无 workflow 系统 |
| C7 | 任务不同阶段切换模型 | auto-routing 支持 big/cheap 但无任务阶段切换 |
| O1 | totalTokens ~2x 真实上下文 | 保守 token 估算（1.5x 系数），API 404 无法验证 |
| O5 | 支持 fast mode (2.5x 吞吐) | 无 fast mode 功能 |
| O7 | 插件渲染瞬态 TUI 状态 | 无瞬态 TUI 状态渲染 |
| L1 | maker/checker 分离验证 | 无正式 maker/checker 分离模式 |
| L5 | 并发 worktree 管理 | 无 worktree 管理系统 |

### 待深入分析

| 编号 | 标题 | 当前判断 | Issue |
|------|------|----------|-------|
| S8 | SSRF 漏洞 (CVSS 8.6) | HTTP 客户端仅用于 LLM API，OpenSandbox URL 非攻击者可控，风险低 | — |
| S9 | Shell 变量展开绕过检测 | 分析器看到未展开形式，sh -c 执行时才展开，存在漏判可能 | [#97](https://github.com/XiaomingX/mimofan/issues/97) |
| S12 | macOS 45GB 内存泄漏 | 子智能体 messages Vec 无截断，delivered_subagent_completion_ids 无清理，需监控 | — |
| U1-U44 | UX 候选项 | 子代理分析中（UX section） | — |
| T19 | 代理自提示超出范围 | 子智能体无代码级范围限制，仅靠文本指令约束（轻微） | — |
| H10 | 空 content 重试浪费 | 流级重试有上限，但空内容场景可能浪费时间 | — |
| C3 | 长会话任务锚点重注入 | 压缩保留部分上下文但无显式锚点重注入 | [#98](https://github.com/XiaomingX/mimofan/issues/98) |

## 四、修复计划（详细）

### Phase 1：高优先级修复

- [ ] **T5** — 为 Goal 模式添加默认 continuation 上限（50 次）或 token/time 预算
- [ ] **H1** — 实现 MCP SSE 心跳计时器 + 空闲超时重连
- [ ] **O2** — 稳定化 system prompt 前缀，分离静态/动态内容

### Phase 2：中优先级修复

- [ ] **T7** — 添加单轮子智能体完成事件注入总量上限（48K）
- [ ] **T10** — 实现工具输出内容 hash 去重
- [ ] **T13** — 保留原始 system prompt 前缀不变
- [ ] **T16** — 增强 workflow context 提取逻辑
- [ ] **T17** — 删除死代码或实现过滤器连接
- [ ] **T20** — 为 Custom provider 添加 reasoning_effort 模型白名单
- [ ] **C5** — 扩展 reasoning_effort 到 Custom provider（与 T20 合并）
- [ ] **C9** — 实现 LSP didClose + 生命周期管理

### Phase 3：低优先级修复

- [ ] **T2** — compaction 截断分级改为多档
- [ ] **T22** — footer 增加每轮 token 差值显示
- [ ] **S10** — rlm 临时目录添加 UUID
- [ ] **J5** — MCP 预连接或异步发现

### Phase 4：功能缺失（未来需求）

- [ ] **J9** — @file frecency 排序
- [ ] **C2** — push/pop 上下文栈 + /digest 蒸馏
- [ ] **C3** — 任务锚点重注入
- [ ] **C6** — /workflows 延迟执行
- [ ] **C7** — 任务阶段切换模型
- [ ] **O5** — fast mode 支持
- [ ] **O7** — 插件瞬态 TUI 状态
- [ ] **L1** — maker/checker 分离验证
- [ ] **L5** — 并发 worktree 管理

## 五、统计

| 状态 | 数量 |
|------|------|
| 已修复（commit 169275e） | 3 |
| 确认不存在 | 54 |
| 排查属实（需修复） | 20 |
| 功能缺失（未来需求） | 10 |
| 待深入分析 | 10 |
| UX 分析中 | 90 (U1-U44) |
| **合计** | **187** |

---

## 六、需求建议（来自 pinggu-mimofan-v2 能力对比，2026-07-29）

> 对比结论：agent-mimofan 在多数基础能力上已对标/超越 Python 版；以下仅记录**经必要性裁剪后**仍值得做的需求，重型 RAG 栈（向量/FTS5/bm25）判定为过度设计，不立项。

### 功能缺失 / 未来需求（必要性已裁剪）

| 编号 | 标题 | 必要性判断与范围 | 描述 |
|------|------|------------------|------|
| M1 | 长期记忆保留策略修正 + 轻量整合 | **部分必要（精简后）**：重语义检索栈不必要；但截断方向与无整合确实有问题 | 现状：`crates/tui/src/memory.rs` 为 flat markdown，`append_entry` 无限追加磁盘；`MAX_MEMORY_SIZE=100KB` 截断时保留文件**头部（最旧）**并丢弃**尾部（最新）**，>100KB 后模型看不到最新记忆；无去重/整合/摘要。问题：① 磁盘无限增长；② 截断方向反了，新记忆被静默丢弃；③ 重复/低价值条目堆积。建议（必要，但精简）：修正保留策略（超阈值保留最近 N 条/时间窗，而非文件头部）；定期去重 + 对陈旧条目做本地 LLM 摘要（借鉴 ether `MemoryManager.consolidate` 思路，但用 markdown，不引入向量/FTS5）。**本期不做**：向量检索 / FTS5 / bm25 —— 对个人记忆笔记属过度设计。工作量 S–M。 |

### 已确认非缺口（不重复立项）

| 编号 | 标题 | 结论 |
|------|------|------|
| — | 内置 cron / 周期任务引擎 | **已存在**：`AutomationManager`（`crates/tui/src/automation_manager.rs`）+ `automation_*` 工具（create/list/read/update/pause/resume/delete/run）+ `/schedule` 命令，支持 RRULE `HOURLY`/`WEEKLY`；为自包含内部调度器，非宿主平台桥接。原 `todo.md` P1/P2 项撤销。 |
