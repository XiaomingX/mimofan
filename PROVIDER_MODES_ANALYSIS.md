# Provider / 线协议模式分类分析报告

> 目标：分析 mimofan 当前支持哪些「使用模式」，评估是否收敛为
> **OpenAI Compatible / Anthropic Compatible / Gemini Compatible** 三种，
> 并列出 deepseek / mimo 等是否仍为独立模式，供你决定是否删除/简化。
>
> 约束：符合 **MECE 原则**（互斥且完备）与 **奥卡姆剃刀原则**（如无必要，勿增实体）。
> 所有结论均经代码核实（2026-08-07）。**本报告 §6 的简化动作已落地执行。**
>
> 最后更新：2026-08-07（本轮已去除历史别名，仅保留纯 MECE 三种）

---

## 0. 一句话结论（说人话）

**系统现在只有纯 MECE 的三种模式：`OpenAiCompatible` / `AnthropicCompatible` /
`GeminiCompatible`。** 历史产品别名（`openai` / `custom` / `xiaomi-mimo` / `mimo` /
`anthropic` / `gemini` / `google` 等）已在 2026-08-07 全部移除（不考虑向下兼容）。
配置与 CLI 现在只认规范 kebab-case 名：`openai-compatible` / `anthropic-compatible` /
`gemini-compatible`。

> 注意：`deepseek` / `mimo` 作为**模型名 / 路由厂商标识**仍存在（如 `deepseek-v4-pro`、
> `mimo-v2.5-pro`），但那是模型维度，不是协议模式维度——两者正交，不冲突。

---

## 1. 第一性原理：什么是"模式"

先厘清一个关键区分，否则分类会混乱：

| 维度 | 是什么 | 例子 | 是否算"模式" |
|------|--------|------|-------------|
| **线协议（Wire Protocol）** | LLM 网关"说哪种 HTTP 接口格式" | OpenAI `/v1/chat/completions`、Anthropic `/v1/messages`、Gemini `generativelanguage` | **是，这就是模式** |
| **模型（Model）** | 同一协议下具体用哪个模型 | deepseek-v4-pro、mimo-v2.5-pro、gpt-4 | **否，只是参数** |

**核心判断**：mimofan 的设计哲学是「只关心网关说哪种线协议，不绑定任何产品」。
所以"模式"= 线协议种类；deepseek / mimo 是**模型名**，不是模式。把它们当模式是
概念混淆，正是奥卡姆剃刀要剃掉的。

---

## 2. 现状核实（代码证据）

### 2.1 线协议枚举已是干净三分（已去除历史别名）

`crates/config/src/provider.rs:12` 的 `WireFormat`：

```rust
pub enum WireFormat {
    OpenAiCompatible,       // OpenAI /v1/chat/completions
    AnthropicCompatible,    // Anthropic /v1/messages
    GeminiCompatible,       // Google Gemini generativelanguage
}
```

`crates/config/src/provider_kind.rs:19` 的 `ProviderKind`（用户配置的"模式"）：
三种变体，**互相互斥且完备（MECE）**，**无 `serde(alias)`**，仅靠
`#[serde(rename_all = "kebab-case")]` 序列化为规范名：

```rust
#[derive(..., Serialize, Deserialize, ...)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    #[default]
    OpenAiCompatible,        // 解析为 "openai-compatible"
    AnthropicCompatible,     // 解析为 "anthropic-compatible"
    GeminiCompatible,        // 解析为 "gemini-compatible"
}
```

> **已落地（2026-08-07）**：历史产品别名 `openai` / `openai-compatible` /
> `custom` / `xiaomi-mimo` / `mimo`（OpenAI 侧），`anthropic` / `anthropic-compatible`
> （Anthropic 侧），`gemini` / `gemini-compatible` / `google`（Gemini 侧）**已全部移除**。
> 配置与 CLI 现在**只认规范 kebab-case 名**。不考虑向下兼容。
> 同步清理点：`provider.rs` 的 `provider!` 宏 `aliases: []`；CLI `ProviderArg` 的
> `#[value(alias)]`；tui `subagent_provider_key_matches` 的产品别名字符串匹配。

### 2.2 协议分发不依赖产品名

`crates/tui/src/client.rs:1420-1445` 的 `create_message` 分发逻辑：

```rust
if api_provider_uses_anthropic_messages(self.api_provider, &self.base_url) {
    return self.handle_anthropic_message(request).await;
}
// 非 Anthropic 的 MiMo base URL 走 OpenAI Chat 路径（注释举例，非分支）
self.create_message_chat(&request).await
```

分发**只看 `ApiProvider` 三种之一**，没有任何 `if model == "deepseek"` 或
`if provider == "mimo"` 的协议级特殊分支。MiMo 在注释里只是作"非 Anthropic 即走
OpenAI Chat"的举例。

### 2.3 模型名层有 deepseek 特殊处理，但不是模式

`crates/tui/src/config/provider.rs:257`：

```rust
if !normalized.starts_with("deepseek") && !normalized.contains("/deepseek") {
    return None;   // 仅对 deepseek 前缀做模型名合法性放行
}
```

这是**模型名字符串校验**（让 `deepseek-xxx` 透传），与协议模式无关。DeepSeek 模型
最终仍走 `OpenAiCompatible` 协议。该逻辑保留（模型维度，非模式维度）。

---

## 3. MECE 分类总表（当前真实状态，已去别名）

按"线协议"这一唯一分类轴，系统**完备且互斥**地分为三类。配置/CLI 只接受规范名：

| # | 模式（ProviderKind / WireFormat） | 配置写法 | 覆盖的网关（模型维度） | 协议端点 | 是否独立模式 |
|---|----------------------------------|---------|----------------------|---------|------------|
| 1 | `OpenAiCompatible` | `openai-compatible` | OpenAI、DeepSeek(`deepseek-v4-*`)、MiMo(`mimo-v2.5-*`)、Qwen、Kimi、GLM、MiniMax、任意自托管 | `/v1/chat/completions` | ✅ 是 |
| 2 | `AnthropicCompatible` | `anthropic-compatible` | Anthropic、及以 `/anthropic` 结尾的 base_url | `/v1/messages` | ✅ 是 |
| 3 | `GeminiCompatible` | `gemini-compatible` | Google Gemini | `generativelanguage...:generateContent` | ✅ 是 |

**MECE 校验**：
- **互斥**：任一网关必属且仅属一种协议。判断依据是 base_url 路径特征 + ApiProvider，不存在重叠。
- **完备**：任何"说这三种协议之一"的网关都能接入；不支持这三种协议外的网关（如私有协议）本身就不在此系统目标内。
- **无别名残留**：写 `provider = "mimo"` / `"custom"` / `"deepseek"` / `"anthropic"` 等**现在会解析失败**（已去除别名，不向下兼容）。必须写规范名。

> "其他候选模式"清单（均不存在，已被去除或本就非模式）：
> - **deepseek 模式**：❌ 不存在（仅模型名，走 OpenAiCompatible；`deepseek` 字符串是路由/catalog 厂商标识，独立于协议模式）
> - **mimo 模式**：❌ 不存在（仅模型前缀 `mimo-v2.5-*`，协议走 OpenAiCompatible）
> - **custom 模式**：❌ 已去除（原本就是 OpenAiCompatible 的别名）
> - **TTS 语音合成（MiMo TTS）**：⚠️ 非新模式，是 OpenAiCompatible 下的一个**子功能**（`client.rs:1127` `generate_speech`，走 OpenAI-compatible 端点）

**结论：纯 MECE 三种模式已落地，无第四种，无历史别名。**

---

## 4. 可简化/冗余点（奥卡姆剃刀视角，待你拍板）

虽然"模式分类"已达标，但代码里仍有**与模式无关**的两处冗余，按奥卡姆剃刀可优化。
**注意：这些不是"多余的模式"，而是"重复的抽象"，是否删除由你决定。**

### 4.1 [可选] `ApiProvider` 与 `ProviderKind` 双枚举重复

- **现状**：`crates/tui/src/config/provider.rs:10` 的 `ApiProvider` 与
  `crates/config/src/provider_kind.rs:19` 的 `ProviderKind` **变体完全一致**
  （都是三种），且 `ApiProvider` 通过 `from_kind`/`kind` 与 `ProviderKind` 一一互转。
- **奥卡姆剃刀判断**：tui 层的 `ApiProvider` 是 config 层 `ProviderKind` 的**镜像副本**，
  多一套 enum + 互转函数 = 多一处维护面。理想解是直接用 `ProviderKind`，删掉 `ApiProvider`。
- **风险/代价**：`ApiProvider` 在 tui 内大量使用（display_name / credential_url /
  env_vars 等 UI 辅助方法），删除需改 tui 多处调用点，属中风险重构。
- **建议**：
  - [ ] **保留**（推荐，低风险）：现状已无功能危害，仅轻微冗余，不动。
  - [ ] **合并**：若追求极简，把 `ApiProvider` 的 UI 辅助方法迁到 `ProviderKind`
       或新 trait，删掉镜像 enum。

### 4.2 [可选] `RequestPayloadMode` 与 `WireFormat` 同构

- **现状**：`crates/tui/src/config/provider.rs:209` 的 `RequestPayloadMode`
  （ChatCompletions / AnthropicMessages / Gemini）与 `WireFormat` 变体**一一对应**，
  在 `provider_capability()` 里由 `ApiProvider` 直接映射。
- **奥卡姆剃刀判断**：两者语义完全同构，可统一为一种。但 `RequestPayloadMode`
  承载"请求体方言"语义、`WireFormat` 承载"线协议"语义，在 DDD 里算是**不同限界上下文的
  视图**，保留也有道理（接口清晰）。
- **建议**：
  - [ ] **保留**（推荐）：语义视角不同，分置合理，不构成冗余危害。

### 4.3 [已完成] 历史别名移除

- **已落地（2026-08-07）**：`ProviderKind` 的 `#[serde(alias)]`、`provider!` 宏的
  `aliases: []`、`ProviderArg` 的 `#[value(alias)]`、`subagent_provider_key_matches`
  的产品别名字符串匹配，**全部移除**。配置/CLI 仅认规范 kebab-case 名。
- 模型名层的 deepseek 前缀校验（§2.3）**保留**（模型维度，非模式维度，合理透传）。

---

## 5. 最终建议（不忽悠版）

1. **纯 MECE 三种模式已落地**：`openai-compatible` / `anthropic-compatible` /
   `gemini-compatible`，无历史别名，配置只认规范名。
2. **deepseek / mimo 不是模式**：它们是模型名/路由厂商标识（如 `deepseek-v4-pro`、
   `mimo-v2.5-pro`），协议走 OpenAiCompatible，与模式正交，保留。
3. **若你想进一步极简**，唯一仍可考虑的是 §4.1 合并 `ApiProvider` 双枚举
   （中风险重构，收益是少一套镜像类型）。是否值得由你定。
4. **已按你要求"不考虑向下兼容"**：历史别名已硬删。代价是旧配置文件里
   `provider = "mimo"` / `"custom"` / `"deepseek"` 等写法**现在会解析失败**，
   必须改为规范名（见本报告的 `USER_GUIDE_CN.md` / `docs/CONFIGURATION.md` 更新）。

---

## 6. 已执行的简化动作（2026-08-07 落地）

- [x] **移除 `ProviderKind` 历史 `#[serde(alias)]`**：`provider_kind.rs` 三变体仅保留
      kebab-case 规范名（依赖 `rename_all`）。
- [x] **移除 `provider!` 宏 `aliases: [...]`**：`provider.rs` 三处改为 `aliases: []`。
- [x] **移除 CLI `ProviderArg` 的 `#[value(alias)]`**：`cli_commands/mod.rs` 仅认规范名。
- [x] **清理 tui `subagent_provider_key_matches` 产品别名匹配**：仅认规范 kebab-case 名。
- [x] **编译验证**：`cargo check -p mimofan-config -p mimofan` 通过（2026-08-07）。
- [ ] **(可选，未做) 合并 `ApiProvider` → `ProviderKind`**：中风险重构，收益有限，
      本轮未执行，待你单独拍板。

> 文档同步：本报告的 §2/§3/§4.3/§5/§6 已更新；`USER_GUIDE_CN.md`、
> `docs/CONFIGURATION.md`、`docs/CONFIGURATION_GUIDE.md`、`README.md` 中所有
> `provider = "custom"` / `"xiaomi-mimo"` / `"mimo"` 示例已改为规范名
> `openai-compatible` / `anthropic-compatible` / `gemini-compatible`。
