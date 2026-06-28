# DeepSeek 上下文硬盘缓存实现审计

审计日期：2026-06-27
审计目标：检查项目对 DeepSeek 上下文硬盘缓存（前缀缓存）的最佳实践符合度

## TL;DR

该项目对 DeepSeek 前缀缓存有**极其深入且深思熟虑**的支持。系统提示词按 "volatile-content-last" 排序、reasoning_content 确定性输出、append-only 消息纪律、前缀稳定性监控、缓存预热、缓存命中/未命中追踪等机制均已到位。**只有一个真正可操作的改进点**：缓存预热请求只发送系统提示词的静态层，而非完整提示词，导致预热覆盖的 token 比应覆盖的少了约 15–30%。

---

## 最佳实践检查清单

### ✅ 已正确实现

| 项目 | 状态 | 说明 |
|------|------|------|
| **系统提示词稳定性** | ✅ 极佳 | volatile-content-last 排序，compile-time 嵌入 constitution.md |
| **对话历史一致性** | ✅ 极佳 | append-only，绝不重排历史消息 |
| **reasoning_content 确定性** | ✅ 极佳 | 始终包含，绝不由后续轮次条件决定 |
| **缓存命中/未命中追踪** | ✅ 完整 | 三种 wire dialect 均解析 `prompt_cache_hit/miss_tokens` |
| **前缀稳定性监测** | ✅ 完整 | SHA-256 指纹 + drift 检测，TUI 芯片显示 |
| **缓存预热** | ✅ 实现 | `build_cache_warmup_request()` 专用路径 |
| **缓存成本核算** | ✅ 完整 | 不同定价模型区分 cache hit/miss token |
| **模型缓存意识** | ✅ 深入 | 系统提示词含 "Prompt-cache awareness" 章节指导模型行为 |
| **工具目录缓存** | ✅ 实现 | `ToolCatalogCache` LRU 避免重复序列化 |
| **Tool result 压缩去重** | ✅ 实现 | 减少历史体积，延长可缓存前缀 |
| **Anthropic cache_control 断点** | ✅ 实现 | `apply_anthropic_cache_breakpoints()` 最多4个标记 |
| **DeepSeek beta base URL** | ✅ 正确 | `https://api.deepseek.com/beta` 启用完整功能 |
| **消息压缩/清理** | ✅ 实现 | `/compact` + 手术式 purge |

### 🔍 可改进项

#### 改进 1：预热请求使用 `stable_system_prompt()` 而非完整系统提示词

**严重程度**：中

**现象**：

缓存预热请求 `build_cache_warmup_request()` 调用 `stable_system_prompt()` 过滤出**仅静态层**作为预热请求的系统提示词：

```rust
// chat.rs:667-697
fn build_cache_warmup_request(self) -> MessageRequest {
    let system = stable_system_prompt(self.system);  // ← 仅静态层
    // ...
}
```

这意味着：
- 预热发往 API：`[仅静态系统提示词] + [历史消息(不含最后一条用户)] + ["请只回复 OK"]`
- 真实请求发往 API：`[完整系统提示词=静态层+易变层] + [完整历史消息] + [用户实际提问]`

两者的公共前缀为 `[静态层] + [除去最后用户消息的公共历史]`。**易变层（Environment、Configured instructions、User memory、Goal、Handoff 等）完全不在预热覆盖范围内。**

**影响估算**：

以一次典型的多轮会话为例：
- 系统提示词总计：~5000 tokens（静态层 ~3500，易变层 ~1500）
- 历史消息：~2000 tokens
- 用户提问：~200 tokens

| 场景 | 预热覆盖 | 可命中缓存比例 |
|------|---------|--------------|
| **现状**（仅静态层） | 静态层 + 公共历史 | ~78%（3500+2000 / 5000+2000+200） |
| **若使用完整提示词** | 完整系统 + 公共历史 | ~94%（5000+2000 / 5000+2000+200） |

约 **16 个百分点**的缓存命中率提升空间。

**为什么不直接用完整提示词？**

`stable_system_prompt()` 的设计初衷是"只缓存真正不可变的部分"。但预热请求在真实请求**之前同步执行**（同一 async 上下文中），易变层在预热和真实请求之间不可能发生变化。当前实现过于保守。

**修复建议**：

将 `build_cache_warmup_request()` 中的系统提示词替换为完整提示词即可：

```rust
// 当前：
let system = stable_system_prompt(self.system);

// 改为（仅一行变更）：
let system = self.system.cloned();
```

需要保留 `stable_history_messages()` 对最后用户消息的剥离（隐私保护：不将实际用户问题发往预热请求）。

**风险**：
- 预热请求输入增加 15–30% token 消耗，成本略微上升
- 但预热带来的缓存命中收益远超此微末成本

**复杂度**：极低（改动 ≤2 行代码）

---

#### 改进 2（可选）：环境块稳定性分类微调

**严重程度**：极低（不直接影响缓存行为）

`is_static_base_layer()` 将 `"Environment"` 标记为 `PromptLayerStability::Static`，但 `system_prompt_for_mode_with_context_skills_session_and_approval()` 将其置于 volatile 分界线之下。这仅影响 `split_system_layers()` 的调试/检测输出中该层的分类标签，不影响 `PrefixStabilityManager`（它对完整系统提示词哈希，不使用该分类），也不影响预热行为（包含 Environment 反而是有益的）。

无需变更。记录在此仅为完整性。

---

## 结论

| 维度 | 评分 |
|------|------|
| **架构设计** | ★★★★★ — volatile-content-last 是教科书级的最佳实践 |
| **消息组装** | ★★★★★ — 确定性 reasoning，append-only，tool result 压缩 |
| **监测与追踪** | ★★★★☆ — 缓存命中/未命中追踪 + 前缀指纹 |
| **预热策略** | ★★★☆☆ — 已实现，但应使用完整系统提示词最大化效果 |
| **模型引导** | ★★★★★ — "Prompt-cache awareness" 系统指令非常详尽 |

**唯一必须处理的改进**：预热请求使用完整系统提示词，预计可将缓存命中率提升约 15–30%。

**不推荐的"改进"**（已评估并排除）：
- 添加 `cache_control` 标记到 Chat Completions 请求：DeepSeek 不支持，缓存完全服务器端自动处理
- 实现长周期缓存命中率统计：当前的按轮芯片已足够，过度设计无实质收益
- 对齐 token 边界到 128-token 缓存单元：客户端无法控制服务端 tokenization
- "对话前缀续写"（Beta）功能：这是不同的 API 功能，与硬盘缓存无关

---

## 附录：关键文件位置

| 关注点 | 文件 | 函数/结构 |
|--------|------|----------|
| 系统提示词组装 | `crates/tui/src/prompts.rs:1040` | `system_prompt_for_mode_with_*` |
| 系统提示词分层 | `crates/tui/src/client/chat.rs:1089` | `split_system_layers()` |
| 预热请求构建 | `crates/tui/src/client/chat.rs:667` | `build_cache_warmup_request()` |
| 静态层过滤 | `crates/tui/src/client/chat.rs:1154` | `stable_system_prompt()` |
| 历史消息过滤 | `crates/tui/src/client/chat.rs:1170` | `stable_history_messages()` |
| 消息组装 Chat | `crates/tui/src/client/chat.rs:1504` | `build_chat_messages_with_reasoning()` |
| 消息组装 Anthropic | `crates/tui/src/client/anthropic.rs:40` | `build_anthropic_body()` |
| Anthropic 缓存断点 | `crates/tui/src/client/anthropic.rs:376` | `apply_anthropic_cache_breakpoints()` |
| 用法解析（含缓存字段）| `crates/tui/src/client.rs:1718` | `parse_usage()` |
| 前缀稳定性管理器 | `crates/tui/src/prefix_cache.rs:172` | `PrefixStabilityManager` |
| 预热调度 | `crates/tui/src/tui/ui.rs:5061` | `run_cache_warmup()` |
| 预热结果展示 | `crates/tui/src/tui/format_helpers.rs:17` | `cache_warmup_result()` |
