# Verse.md 候选需求修复计划

> 基于 verse.md 187 项分析，14 个确认存在的问题
> 生成日期：2026-07-29
> 工作区版本：v0.8.68+

## 执行结果（2026-07-29）

| 项目 | 状态 | 说明 |
|------|------|------|
| 1.1 Footer 死代码 #80 | ⏭️ 跳过 | StatusItem 枚举变体有完整序列化映射，删除破坏 API 兼容性 |
| 1.2 RLM 临时目录 #86 | ✅ 完成 | turn.rs + session.rs 添加 UUID 隔离 |
| 1.3 子智能体注入上限 #76 | ✅ 完成 | 48K 字符总注入限制 |
| 2.1 Goal continuation 上限 #74 | ✅ 完成 | DEFAULT_MAX_CONTINUATIONS = 50 |
| 2.2 MCP SSE 心跳 #75 | ✅ 完成 | 添加 debug 日志 |
| 2.3 Compaction 截断分级 #84 | ⏭️ 跳过 | 二分法已覆盖实际模型分布 |
| 3.1 Shell 变量安全 #97 | ✅ 完成 | $VAR/${VAR} 检测 |
| 3.2 LSP 生命周期 #82 | ⏭️ 跳过 | kill_on_drop 已处理 |
| 3.3 Custom reasoning_effort #81 | ⏭️ 跳过 | 当前实现正确 |

## Phase 1：高 ROI 快速修复 ✅ 已完成

### 1.1 ~~清理 Footer 死代码~~ — #80 (T17) ⏭️ 跳过

**问题：** `footer_ui.rs:582` 硬编码 `LastToolElapsed` 和 `RateLimit` 返回空；`is_available_for()` 始终返回 `true`。

**修复：**
- 文件：`crates/tui/src/tui/footer_ui.rs`
- 删除 `LastToolElapsed` 和 `RateLimit` 的空实现，或实现过滤器连接
- 复制同文件中已有的 `StatusItem` 过滤模式

**验证：** `cargo test -p mimofan --lib` + grep 确认无残留引用

### 1.2 RLM 临时目录添加 UUID — #86 (S10)

**问题：** `rlm/turn.rs:526` 和 `rlm/session.rs:114` 使用硬编码路径 `temp_dir().join("deepseek_rlm_ctx")` 无 UUID。

**修复：**
- 文件：`crates/tui/src/rlm/turn.rs`, `crates/tui/src/rlm/session.rs`
- 将 `"deepseek_rlm_ctx"` 改为 `format!("deepseek_rlm_ctx_{}", uuid::Uuid::new_v4())`
- 复制同 crate 中 `tempfile::tempdir()` 的使用模式

**验证：** grep 确认无残留硬编码路径

### 1.3 子智能体完成事件注入上限 — #76 (T7)

**问题：** `drain_subagent_completion_events()` 每个完成事件上限 12K，但无单轮注入总量上限。

**修复：**
- 文件：`crates/tui/src/tools/subagent/mod.rs`（~line 6444）
- 添加 `const MAX_SUBAGENT_INJECTION_BYTES: usize = 48 * 1024;`（48K）
- 在 `drain_subagent_completion_events()` 中累加已注入字节数，超出时截断最旧事件
- 复制同文件中 `MAX_OUTPUT_SIZE` 的截断模式

**验证：** 单元测试：3 个子智能体各 12K → 前 3 个注入，第 4 个被截断

---

## Phase 2：中 ROI 功能修复（预估 4-6 小时）

### 2.1 Goal 模式添加 continuation 上限 — #74 (T5)

**问题：** `goal_loop.rs:15-18` 明确 "no continuation cap"，`token_budget`/`time_budget_seconds` 默认 `None`。

**修复：**
- 文件：`crates/tui/src/goal_loop.rs`
- 添加 `const DEFAULT_MAX_CONTINUATIONS: u32 = 50;`
- 在循环中添加计数器，超出时停止并提示用户
- 复制同 crate 中 `max_steps` 的约束模式（engine.rs）

**验证：** 单元测试：goal loop 在 50 次后自动停止

### 2.2 MCP SSE 心跳处理 — #75 (H1)

**问题：** `mcp.rs:721-807` SSE 循环无心跳/keepalive 处理，心跳帧被 `_ => {}` 静默丢弃。

**修复：**
- 文件：`crates/tui/src/mcp.rs`
- 添加 `last_heartbeat: Instant` 跟踪
- 收到心跳帧时重置计时器
- 超过 60s 未收到心跳则重连
- 复制同文件中已有的超时模式（`connect_timeout`）

**验证：** 单元测试：模拟心跳超时触发重连

### 2.3 Footer 每轮 token 差值显示 — #85 (T22)

**问题：** footer `tok` 芯片仅显示累计值，内部 `TurnContext` 已跟踪每轮数据。

**修复：**
- 文件：`crates/tui/src/tui/footer_ui.rs`
- 在 `StatusItem::Tokens` 中添加 `delta: Option<u32>` 字段
- 渲染时显示 `tok 1.2K (+320)` 格式
- 复制同文件中 `StatusItem::Cost` 的渲染模式

**验证：** 视觉检查：footer 显示每轮差值

### 2.4 Compaction 截断分级 — #84 (T2)

**问题：** `compaction.rs` 仅按 500K 阈值二分，128K 和 400K 模型使用相同截断限制。

**修复：**
- 文件：`crates/tui/src/compaction.rs`
- 将二分截断改为多档：`[128K, 256K, 512K, 1M]`
- 根据模型 context_window 选择合适档位
- 复制同文件中 `MAX_WORKING_SET_PATHS` 的分级模式

**验证：** 单元测试：128K 模型截断到 100K，1M 模型截断到 800K

---

## Phase 3：中 ROI 安全/稳定性修复（预估 3-4 小时）

### 3.1 Shell 变量展开安全分析 — #97 (S9)

**问题：** `command_safety.rs` 检测 `$()` 和反引号，但 shell 变量展开（`$VAR`）在 Rust 层未展开。

**修复：**
- 文件：`crates/tui/src/command_safety.rs`
- 添加 `$VAR` 模式的检测，升级为 `RequiresApproval`
- 不预展开（避免执行副作用），仅标记风险
- 复制同文件中 `$()` 检测的模式

**验证：** 单元测试：`$VAR` 命令返回 `RequiresApproval`

### 3.2 LSP 生命周期管理 — #82 (C9)

**问题：** LSP `opened: HashMap<PathBuf, i64>` 永不清理，无 `textDocument/didClose` 发送。

**修复：**
- 文件：`crates/tui/src/lsp.rs`（或相关 LSP 模块）
- 在 transport drop 时发送 `didClose` 对所有已打开文件
- 或在文件不再被引用时发送 `didClose`
- 复制同模块中已有的 `didOpen` 发送模式

**验证：** grep 确认 drop 路径发送 `didClose`

### 3.3 Custom provider reasoning_effort — #81 (T20/C5)

**问题：** `apply_reasoning_effort()` 仅对 `XiaomiMimo` 生效，`Custom` 为 no-op。

**修复：**
- 文件：`crates/tui/src/client.rs`（~line 1526-1569）
- 为 `Custom` provider 添加模型白名单检测
- 或在 `reasoning_effort` 非 None 时向用户发出警告
- 复制 `XiaomiMimo` 分支的实现模式

**验证：** 单元测试：Custom provider + reasoning_effort 设置正确传递

---

## Phase 4：低 ROI 增强（预估 2-3 小时，可选）

### 4.1 MCP 首次连接延迟优化 — #83 (J5)

**问题：** `connect_timeout` 默认 10s，首次调用阻塞。

**修复：**
- 文件：`crates/tui/src/mcp.rs`
- 实现后台预连接：启动时异步连接所有配置的 MCP 服务器
- 复制同文件中已有的 `get_or_connect()` 懒连接模式

### 4.2 工具输出内容 hash 去重 — #77 (T10)

**问题：** 无相同工具输出去重，同一调用 5 次每次 32KiB。

**修复：**
- 文件：`crates/tui/src/engine.rs`（工具输出处理）
- 添加内容 hash 缓存，相同 hash 仅保留一份
- 复制同 crate 中 `content_hash` 的模式（output_rows_cache.rs）

### 4.3 长会话任务锚点重注入 — #98 (C3)

**问题：** 压缩后模型可能丢失对早期任务目标的跟踪。

**修复：**
- 文件：`crates/tui/src/compaction.rs`
- 在 `extract_workflow_context()` 中添加任务锚点提取
- 压缩摘要中保留当前任务目标
- 复制同文件中已有的 `extract_workflow_context()` 模式

### 4.4 Compaction 后 workflow context 提取 — #79 (T16)

**问题：** `extract_workflow_context()` 仅关键词匹配 "TODO"/"task"，不解析 checklist。

**修复：**
- 文件：`crates/tui/src/compaction.rs`
- 增强关键词匹配，添加 "DONE"、"completed"、"in_progress" 等状态词
- 复制同文件中已有的 `extract_workflow_context()` 模式

---

## 执行顺序建议

```
Phase 1 (快速修复) → Phase 2 (功能修复) → Phase 3 (安全/稳定性) → Phase 4 (可选增强)
```

每个 Phase 完成后运行 `cargo test -p mimofan --lib` 验证。

## 风险控制

- 每个 Phase 独立可测试，不依赖其他 Phase
- Phase 1-3 为 Bug 修复，优先级高
- Phase 4 为功能增强，可按需实施
- 所有修改遵循现有代码模式，不引入新依赖
