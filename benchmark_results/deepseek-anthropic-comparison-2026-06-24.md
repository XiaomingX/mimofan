# DeepSeek Anthropic 兼容端点 — 对比报告与决策 (#2963)

- **Issue:** [#2963](https://github.com/XiaomingX/mimofan/issues/2963) — *v0.8.65: DeepSeek Anthropic 兼容端点线路协议验证*
- **发布分支:** v0.8.65
- **日期:** 2026-06-24
- **状态:** 实现已落地，标记为**实验性**。保留 vs 升级为首选的决策**待补充线上数据**（第4节）。
- **本文档范围:** 本文为*报告*，不修改任何 Rust 代码，不发起任何线上 API 调用 — 当前环境无 DeepSeek 凭证，下方所有线上数据留待人工填写。

> **不要重复实现该路由。** 它已存在于 `main` 分支（commit
> `5b8a5ac0b2c478261740f49756d29c4a7f83d89c`，PR
> [#3449](https://github.com/XiaomingX/mimofan/pull/3449)）。本文档
> 验证已落地的内容，从代码推导可得出的结论，并指定精确的线上验证流程以解决待定问题。

下方所有文件:行号引用均基于本报告对应的 commit
（已验证：`5b8a5ac0b` 是 `HEAD` 的祖先提交）。

---

## 1. 已落地的内容

使用 **Anthropic Messages** 线路协议的 DeepSeek 可选路由已端到端实现。**已合入 `main`，无需重复实现。**

### 1.1 Provider 描述符 / 路由选择

- `crates/config/src/provider.rs:140-178` — `DeepseekAnthropic` provider：
  - id `deepseek-anthropic`（`provider.rs:143-145`）
  - 显示名称 `DeepSeek (Anthropic-compatible)`（`provider.rs:151-153`）
  - 别名 `deepseek_anthropic`、`deepseek-claude`、`deepseek_claude`
    （`provider.rs:171-173`）
  - **线路格式 `WireFormat::AnthropicMessages`**（`provider.rs:175-177`）
  - API Key 环境变量：**仅 `DEEPSEEK_API_KEY`**（`provider.rs:163-165`）—
    **不会**回退到 `ANTHROPIC_API_KEY`。
- `crates/config/src/provider.rs:31-38` — `WireFormat` 枚举
  （`ChatCompletions` / `Responses` / `AnthropicMessages`）。
- 注册表绑定：静态条目 `provider.rs:544`，注册于
  `provider.rs:573`。
- 默认值（`crates/config/src/provider_defaults.rs`）：
  - 基础 URL `https://api.deepseek.com/anthropic`
    （`provider_defaults.rs:14`）
  - 默认模型 `deepseek-v4-pro` — `DEFAULT_DEEPSEEK_ANTHROPIC_MODEL`，
    别名 `DEFAULT_DEEPSEEK_MODEL`（`provider_defaults.rs:8-9`）
- 对比：Chat-Completions DeepSeek 路由默认基础 URL 为
  `https://api.deepseek.com/beta`（`provider_defaults.rs:13`），使用相同的
  默认模型 `deepseek-v4-pro`。

### 1.2 请求分发

- `crates/tui/src/client.rs:1331-1339`（`create_message`）和
  `client.rs:1341-1352`（`create_message_stream`）在
  `api_provider_uses_anthropic_messages(self.api_provider)` 为 true 时路由到 Anthropic 适配器。
- `client.rs:864-869` — `api_provider_uses_anthropic_messages` 对
  `ApiProvider::Anthropic | ApiProvider::DeepseekAnthropic` 返回 true。
- 请求载荷模式由路由决定，而非提示词：
  `crates/tui/src/config.rs:526-530` 为 `DeepseekAnthropic` 设置
  `RequestPayloadMode::AnthropicMessages`，否则为 `ChatCompletions`。

### 1.3 认证方言

- `crates/tui/src/client.rs:805-838` 构建请求头：
  - 为 Anthropic 线路 provider 注入 `anthropic-version: 2023-06-01`
    （`client.rs:808-815`）
  - 使用 **`x-api-key`**（而非 `Authorization: Bearer`）传递 API Key
    （`client.rs:817-819`，应用于 `client.rs:831-837`）
- `client.rs:846-862` 剥离调用方可能携带的 `Authorization` / `api-key` /
  `x-api-key` 额外头，防止过期的 OpenAI 风格认证头泄漏到 Anthropic 线路
  （`is_auth_dialect_header`，`client.rs:858-862`）。
- 测试：`deepseek_anthropic_uses_anthropic_header_dialect`
  （`client.rs:2216`+）断言 `x-api-key` + `anthropic-version` 存在，
  且 Bearer / MiMo 头不存在。

### 1.4 请求编码（Messages 请求体）

- `crates/tui/src/client/anthropic.rs:40-143` — `build_anthropic_body`：
  - `model` / `max_tokens` / `stream`（`anthropic.rs:41-45`）
  - `system` 支持纯文本或带缓存标记的块（`anthropic.rs:47-66`）
  - `messages` 通过 `message_to_anthropic` 转换（`anthropic.rs:68-74`，
    `anthropic.rs:291-301`）
  - `tools` 支持 `strict` + `cache_control`（`anthropic.rs:76-98`）
  - `tool_choice` 从 OpenAI 风格的字符串/对象映射到 Anthropic 对象形式
    （`anthropic.rs:100-102`，`anthropic.rs:279-287`）
  - reasoning → `thinking: {type: adaptive}` + `output_config.effort`
    （low/medium/high/max），受 `model_supports_reasoning` 门控
    （`anthropic.rs:104-128`）
  - 采样参数规则：temperature/top_p 至多发送一个，或对拒绝它们的模型都不发送
    （`anthropic.rs:130-139`，`anthropic.rs:269-275`）
  - `cache_control` 断点放置，上限为 4 个
    （`anthropic.rs:141`，`anthropic.rs:367-446`）
- 端点 URL 构建器容忍 `/v1` 后缀
  （`anthropic.rs:259-266`）；`https://api.deepseek.com/anthropic` →
  `…/anthropic/v1/messages`。

### 1.5 响应与流式解码

- 非流式：`anthropic.rs:240-254`（`handle_anthropic_message`）解析 JSON 响应体并规范化 `usage`。
- 流式：`anthropic.rs:170-237`（`handle_anthropic_stream`）为 SSE
  透传；`convert_anthropic_sse_data`（`anthropic.rs:450-494`）解码
  `message_start` / `content_block_*` / `message_delta` / `message_stop` /
  `ping` / `error`，容忍未知事件类型，并在
  `message_start` / `message_delta` 上规范化 usage。
- 发送/错误路径：`anthropic.rs:145-167`（`send_anthropic_request`）设置
  `Accept: text/event-stream`，将非 2xx 响应通过
  `parse_anthropic_error_envelope`（`anthropic.rs:528-548`）映射为类型化错误。

### 1.6 Usage / 缓存规范化（#2961 约定）

- `anthropic.rs:499-523`（`parse_anthropic_usage`）：
  - `prompt_cache_hit_tokens = cache_read_input_tokens`
  - `prompt_cache_miss_tokens = input_tokens + cache_creation_input_tokens`
  - 规范化 `input_tokens = input_tokens + cache_creation + cache_read`
    （总 prompt — DeepSeek 约定）

### 1.7 随路由添加的运维护栏

- 健康检查**跳过 `/anthropic/v1/models` 探测**（`client.rs:871-873`，
  `api_provider_skips_models_probe`）；测试
  `deepseek_anthropic_health_check_skips_models_probe`（`client.rs:2301`+）。
- **FIM 在此路由上不支持**，本地直接返回明确错误消息
  （`client.rs:1722-1727`）；测试 `deepseek_anthropic_fim_fails_without_http_request`
  （`client.rs:2314`+）。
- 基础 URL 环境变量覆盖感知路由：`MIMOFAN_BASE_URL` / `DEEPSEEK_BASE_URL`
  写入 `providers.deepseek_anthropic.base_url`
  （`crates/tui/src/config.rs:3928-3939`）。
- 翻译辅助功能使用 Messages 端点（`client.rs:974-977`）；测试
  `deepseek_anthropic_translate_uses_messages_endpoint`（`client.rs:2251`+）。

### 1.8 文档说明

- `docs/PROVIDERS.md:48-51`、`:81`、`:111-112`、`:237` 将该路由描述为
  **Anthropic *线路协议*兼容性**（而非 Anthropic 模型/provider 语义），
  列出别名，并说明"保留 `provider = "deepseek"` 以使用默认的 Chat Completions 路径。"

### 1.9 已有的测试覆盖（无线上调用）

在 `crates/tui/src/client/anthropic.rs` 的 `#[cfg(test)]` 中（从 `anthropic.rs:550` 开始）：
body cache-control 放置、reasoning→effort 映射、采样参数丢弃、
签名/非签名 thinking 重放、断点上限、完整 SSE fixture 解码
（text + thinking + signature + tool_use + usage）、错误/未知事件处理、
缺失缓存字段的 usage 映射、错误信封解析、URL `/v1` 容忍。
在 `crates/tui/src/client.rs` 中：auth-dialect、models-probe-skip、
translate-endpoint 和 FIM-unsupported 测试（如上引用）。

---

## 2. 从代码推导的结论（无需线上调用）

以下是从**当前代码**即可得出的行为事实，无需任何线上对比。这些是审查者最需要了解的差异。

### 2.1 服务器工具 / 网络搜索当前未通过此路由使用

`content_block_to_anthropic` 在编码时**丢弃**服务器工具块类型：

```
crates/tui/src/client/anthropic.rs:359-364
    // 服务器工具块类型是 DeepSeek/内部概念，
    // 在 Anthropic 客户端线路中无对应等价物。
    ContentBlock::ServerToolUse { .. }
    | ContentBlock::ToolSearchToolResult { .. }
    | ContentBlock::CodeExecutionToolResult { .. } => None,
```

结果：引擎持有的任何服务器工具 / 网络搜索内容在通过此路由发送前会被过滤掉。
同时也没有编码端路径将 Anthropic 风格的服务器工具定义（如 `web_search`
工具）注入到出站 body 中 — `build_anthropic_body` 仅转发调用方提供的客户端工具
（`anthropic.rs:76-98`）。因此，**DeepSeek Anthropic 路由当前不使用服务器端网络搜索 / 代码执行。**
DeepSeek 端点是否*接受*此类工具是另一个独立的、仍待验证的问题，只有线上测试（第4节 Test E）能回答；
代码既不提供也不依赖它。

### 2.2 Usage 遥测：与 Chat-Completions 路径相比有两个实际差异

对比两个 usage 解析器：

| 字段 | Anthropic 路由（`anthropic.rs:499-523`） | Chat-Completions 路由（`client.rs:1643-1711`） |
|---|---|---|
| `input_tokens`（规范化后） | `input + cache_creation + cache_read` | `input_tokens`/`prompt_tokens` 原值 |
| `prompt_cache_hit_tokens` | `cache_read_input_tokens` | `prompt_cache_hit_tokens`，否则 `prompt_tokens_details.cached_tokens` |
| `prompt_cache_miss_tokens` | `input + cache_creation` | `prompt_cache_miss_tokens`，否则 `input − hit` |
| `reasoning_tokens` | **始终为 `None`**（`anthropic.rs:519`） | 从 `completion_tokens_details.reasoning_tokens` 解析（`client.rs:1658-1685`） |
| `reasoning_replay_tokens` | `None`（`anthropic.rs:520`） | `None`（`client.rs:1708`） |
| `server_tool_use` | **始终为 `None`**（`anthropic.rs:521`） | 从 `server_tool_use.{code_execution,tool_search}_requests` 解析（`client.rs:1687-1700`） |
| `output_tokens` | Anthropic `output_tokens` | `output_tokens`/`completion_tokens`，回退到 reasoning 或 `total − input`（`client.rs:1648-1670`） |

需如实记录的两个具体差异：

1. **`reasoning_tokens` 在 Anthropic 路由上从不填充。** Reasoning
   *内容*仍然流通（thinking 块解码和签名块重放 —
   `anthropic.rs:315-330`，`anthropic.rs:822-868` fixture），但**计数**
   被丢弃。在 Chat-Completions 路由上，计数从
   `completion_tokens_details.reasoning_tokens` 读取。这遵循 #2961/#3085
   "不支持的字段显式返回 unknown/null" 规则，但意味着
   两条路由之间的 reasoning-token *计量对等性*无法保证 — 在 Test C 中确认。
2. **`server_tool_use` 在 Anthropic 路由上从不填充**（与 §2.1 一致：
   该路由不驱动服务器工具）。

### 2.3 Thinking / reasoning 请求整形存在设计差异

Anthropic 路由将 `reasoning_effort` 级别映射到
`thinking: {type: adaptive}` + `output_config.effort`
（`anthropic.rs:104-128`），受 `model_supports_reasoning` 门控。
Chat-Completions DeepSeek 路径使用自己的 reasoning-split / payload
约定（`config.rs:526-530` 选择载荷模式；DeepSeek 系列
reasoning 处理在 Chat 路径上）。测试的衡量标准是等效*输出*（第3/4节），
而非字节级相同的请求。

### 2.4 缓存模型形态不同

Anthropic 路由在前缀和最新用户轮次上放置显式 `cache_control` 断点（最多 4 个）
（`anthropic.rs:367-446`），并从 Anthropic 的 `cache_read` / `cache_creation` 字段报告缓存
命中/未命中。Chat-Completions 路由依赖 DeepSeek 的自动前缀缓存，读取
`prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`（或
`prompt_tokens_details.cached_tokens`）。两者都规范化为相同的 #2961
字段，因此缓存*遥测*可比较，尽管*机制*不同。

### 2.5 能力/运维差异（路由级，来自代码）

- **FIM**：Chat-Completions DeepSeek 支持；Anthropic 路由**不支持**
  （`client.rs:1722-1727`）。
- **模型探测**：Anthropic 路由跳过（`client.rs:871-873`）；Chat 路径探测 `/models`。
- **认证**：`x-api-key` + `anthropic-version`（Anthropic 路由）vs
  `Authorization: Bearer`（Chat 路由）— `client.rs:817-827`。
- **端点**：`…/anthropic/v1/messages` vs `…/beta` chat completions。

### 2.6 构建层面等价的部分

工具调用和工具结果映射、图片块、系统提示词和停止原因都有直接编码器
（`anthropic.rs:303-358`），SSE 解码器重建工具输入 JSON
（fixture `anthropic.rs:816-897`）。因此对于普通的"提示词 → 文本 / tool_use"
交换，两条路由预期功能等价；待定问题是*定量*方面的
（延迟、token 计数）和*服务器工具*方面。

---

## 3. 对比方法论

对比 DeepSeek 的 **Chat-Completions** 路由（`provider = "deepseek"`）与
其 **Anthropic-Messages** 路由（`provider = "deepseek-anthropic"`），使用
**相同模型**（`deepseek-v4-pro`，如有 `deepseek-v4-flash` 也一并测试）。保持其他条件不变（相同提示词、相同 `max_tokens`、相同
reasoning effort、相同 temperature（如果被接受））。

对比维度：

1. **正确性 / 输出等价性** — 相同提示词 → 语义等价的回答；相同工具选择和参数（工具使用提示词）；结构化提示词的有效 JSON。
2. **延迟** — 总挂钟时间和（流式场景）首 token 时间，每条路由 N≥5 次运行；报告中位数 + 离散度，而非单次采样。
3. **Token / Usage 计量对等性** — 对比 `input_tokens`（规范化后）、
   `output_tokens`、`prompt_cache_hit_tokens`、`prompt_cache_miss_tokens`、
   `reasoning_tokens`。**预期 `reasoning_tokens` 在 Anthropic 路由上为 null**（§2.2）— 记录它，不要当作 bug。
4. **遥测字段** — #2961/#3085 规范化字段中哪些在每条路由上被填充 vs 为 null；注意 `server_tool_use` 在 Anthropic 路由上按设计为 null。
5. **服务器工具 / 网络搜索支持** — DeepSeek 的 Anthropic 端点是否
   *接受*、*忽略*或*拒绝* Anthropic 风格的服务器工具（如
   `web_search`）？捕获原始请求/响应。（注意引擎当前不发送此类工具 — §2.1 — 因此这是使用手工构建请求的端点能力探测，而非 Mimofan 编码器的测试。）
6. **错误信封与速率限制** — 确认 4xx/5xx 映射清晰
   （`anthropic.rs:528-548`），且该路由遵循相同的重试/退避策略。

"可比较"的通过标准（Issue 验收标准）：冒烟任务上的等价正确性、
合理范围内的延迟，以及可映射到规范化字段的 usage 遥测
（不支持的字段显式返回 null）。

---

## 4. 可运行的线上检查清单（人工操作，需设置 `DEEPSEEK_API_KEY`）

所有命令可直接复制粘贴。假设在仓库根目录且已有 DeepSeek Key。
**当前环境无凭证；这些命令供人工运行。**

### 4.0 一次性设置

```bash
export DEEPSEEK_API_KEY="sk-..."           # 你的 DeepSeek Key
MODEL="deepseek-v4-pro"                      # 如有 deepseek-v4-flash 也重复测试
CHAT_BASE="https://api.deepseek.com"        # Chat Completions（OpenAI 兼容）
ANTH_BASE="https://api.deepseek.com/anthropic"  # Anthropic Messages
mkdir -p benchmark_results/2963-live && cd "$(git rev-parse --show-toplevel)"
```

### 测试 A — 正确性，单轮（文本）

Chat Completions：

```bash
curl -sS -w '\n[http %{http_code} | total %{time_total}s | ttfb %{time_starttransfer}s]\n' \
  -X POST "$CHAT_BASE/v1/chat/completions" \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":64,\"stream\":false,
       \"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly the word: PONG\"}]}" \
  | tee benchmark_results/2963-live/A_chat.json
```

Anthropic Messages（注意 `x-api-key` + `anthropic-version`，无 Bearer）：

```bash
curl -sS -w '\n[http %{http_code} | total %{time_total}s | ttfb %{time_starttransfer}s]\n' \
  -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":64,\"stream\":false,
       \"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly the word: PONG\"}]}" \
  | tee benchmark_results/2963-live/A_anthropic.json
```

记录：每个是否返回 "PONG"？HTTP 状态码、总耗时。

### 测试 B — Usage / Token 计量（读取两者的 `usage` 对象）

```bash
echo "Chat usage:";      jq '.usage'  benchmark_results/2963-live/A_chat.json
echo "Anthropic usage:"; jq '.usage'  benchmark_results/2963-live/A_anthropic.json
```

填写下表：

| 字段 | Chat Completions | Anthropic Messages |
|---|---|---|
| prompt/input tokens | | |
| completion/output tokens | | |
| 缓存命中（`prompt_cache_hit_tokens` / `cache_read_input_tokens`） | | |
| 缓存未命中（`prompt_cache_miss_tokens` / `cache_creation_input_tokens`） | | |
| reasoning tokens（`completion_tokens_details.reasoning_tokens`） | | （预期不存在） |

### 测试 C — Reasoning / Thinking

Chat Completions（DeepSeek 推理风格）：

```bash
curl -sS -X POST "$CHAT_BASE/v1/chat/completions" \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":512,\"stream\":false,
       \"messages\":[{\"role\":\"user\",\"content\":\"A bat and ball cost \$1.10. The bat costs \$1 more than the ball. How much is the ball? Think, then answer.\"}]}" \
  | tee benchmark_results/2963-live/C_chat.json | jq '{content:.choices[0].message, usage:.usage}'
```

Anthropic Messages with adaptive thinking：

```bash
curl -sS -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" -H "anthropic-version: 2023-06-01" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":512,\"stream\":false,
       \"thinking\":{\"type\":\"adaptive\"},\"output_config\":{\"effort\":\"high\"},
       \"messages\":[{\"role\":\"user\",\"content\":\"A bat and ball cost \$1.10. The bat costs \$1 more than the ball. How much is the ball? Think, then answer.\"}]}" \
  | tee benchmark_results/2963-live/C_anthropic.json | jq '{content:.content, usage:.usage}'
```

记录：两者应回答 **$0.05**。注意 Anthropic 路由是否返回 `thinking` 块，
以及 reasoning tokens 是否出现在任何位置。

### 测试 D — 工具使用（两条路由使用相同工具）

Chat Completions：

```bash
curl -sS -X POST "$CHAT_BASE/v1/chat/completions" \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":256,
       \"tools\":[{\"type\":\"function\",\"function\":{\"name\":\"get_weather\",
         \"description\":\"Get weather for a city\",
         \"parameters\":{\"type\":\"object\",\"properties\":{\"city\":{\"type\":\"string\"}},\"required\":[\"city\"]}}}],
       \"messages\":[{\"role\":\"user\",\"content\":\"What's the weather in Paris? Use the tool.\"}]}" \
  | tee benchmark_results/2963-live/D_chat.json | jq '.choices[0].message.tool_calls'
```

Anthropic Messages：

```bash
curl -sS -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" -H "anthropic-version: 2023-06-01" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":256,
       \"tools\":[{\"name\":\"get_weather\",\"description\":\"Get weather for a city\",
         \"input_schema\":{\"type\":\"object\",\"properties\":{\"city\":{\"type\":\"string\"}},\"required\":[\"city\"]}}],
       \"messages\":[{\"role\":\"user\",\"content\":\"What's the weather in Paris? Use the tool.\"}]}" \
  | tee benchmark_results/2963-live/D_anthropic.json | jq '.content'
```

记录：每个是否发出 `get_weather` 调用且 `city = "Paris"`？

### 测试 E — 服务器工具 / 网络搜索能力探测（待定问题）

发送 Anthropic 风格的服务器工具，**记录 DeepSeek 是接受、忽略还是拒绝**（捕获完整 body）。引擎当前不发送此内容（§2.1）；这是原始端点探测。

```bash
curl -sS -w '\n[http %{http_code}]\n' -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" -H "anthropic-version: 2023-06-01" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":256,
       \"tools\":[{\"type\":\"web_search_20250305\",\"name\":\"web_search\",\"max_uses\":2}],
       \"messages\":[{\"role\":\"user\",\"content\":\"Search the web: what is the latest stable Rust version? Cite a source.\"}]}" \
  | tee benchmark_results/2963-live/E_websearch.json
```

分类结果：
- **接受且生效** — 响应包含服务器工具使用 / 搜索结果。
- **忽略** — 200 OK，纯文本回答，无工具活动。
- **拒绝** — 4xx 错误信封（记录 `error.type` / message）。

### 测试 F — 流式冒烟测试（两条路由）

```bash
# Anthropic SSE
curl -N -sS -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" -H "anthropic-version: 2023-06-01" \
  -H "Accept: text/event-stream" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":64,\"stream\":true,
       \"messages\":[{\"role\":\"user\",\"content\":\"Count: one two three\"}]}" \
  | tee benchmark_results/2963-live/F_anthropic.sse | head -40
```

确认 `message_start` → `content_block_*` → `message_delta` → `message_stop`
到达（即 `convert_anthropic_sse_data` 解码的格式，`anthropic.rs:450-494`）。

### 测试 G — 通过 Mimofan 端到端测试（可选，使用真实适配器）

```bash
# 通过构建好的二进制文件使用 Anthropic 路由
cargo run -q -p mimofan -- --provider deepseek-anthropic --model "$MODEL" \
  --print "Reply with exactly: PONG"
# Chat 路由对比
cargo run -q -p mimofan -- --provider deepseek --model "$MODEL" \
  --print "Reply with exactly: PONG"
```

（根据项目的实际非交互式入口点调整二进制文件/标志名称；核心目的是让同一提示词分别通过两条路由运行。）

### 4.1 需填写的结果表

| 维度 | Chat Completions | Anthropic Messages | 结论 |
|---|---|---|---|
| 正确性（A/C/D） | | | |
| 延迟中位数（N=…） | | | |
| TTFT（流式） | | | |
| Token 计量（B） | | | |
| reasoning_tokens 是否存在 | | （预期无） | |
| 工具使用（D） | | | |
| 网络搜索（E） | 不适用 | 接受 / 忽略 / 拒绝 | |
| 流式（F） | | | |

---

## 5. 决策

**建议：保留为实验性（Experimental）。保留 vs 升级为首选的决策待补充第4节的线上数据。本报告因未进行线上调用，不给出"已验证"结论。**

基于代码的理由：

- **保留（不移除）：** 该路由已完整实现，通过可选 provider 选择隔离
  （`deepseek-anthropic` / `deepseek-claude`），有护栏保护
  （FIM 不支持消息、models-probe 跳过、auth-header 卫生），并有
  单元测试 + SSE fixture 测试覆盖。它不影响也不回退默认的
  Chat-Completions DeepSeek 路径（在 `client.rs:1331-1352` 独立分发；
  文档说明保留 `provider = "deepseek"` 作为默认）。代码中没有任何理由要移除它。
- **暂不升级：** Issue 的升级标准要求 Anthropic 路由在线上 A/B 测试中
  *至少可比较*，加上明确的服务器工具证据。本报告中不存在该证据。从代码推导的两个注意事项，
  升级时必须权衡：(a) `reasoning_tokens` 计量在此路由上被丢弃
  （§2.2 #1），(b) 服务器工具 / 网络搜索未通过它使用
  （§2.1）— 因此如果网络搜索是"首选"的必要条件，此路由当前
  不满足，无论 Test E 关于端点的结果如何。
- **翻转决策的门槛：** 完成第4节（特别是 Test A–E），
  填写 §4.1 表格，确认等价正确性 + 可比较延迟 +
  清晰的遥测映射。如果全部绿色且网络搜索不是障碍 →
  可作为 DeepSeek V4 的首选候选。否则 → 保持实验性，
  或如果遥测/延迟回退则拒绝*升级*（而非拒绝路由本身）。

### 建议的 Issue 备注（线上数据补充后）

> 实现已验证落地（#3449 / `5b8a5ac0b`）；参见
> `benchmark_results/deepseek-anthropic-comparison-2026-06-24.md`。线上 A/B
> 结果：[待填写]。服务器工具/网络搜索探测（Test E）：[接受/忽略/
> 拒绝 + 证据]。决策：[保持实验性 | 升级为首选]。

---

## 附录 — 引用索引

| 主题 | 位置 |
|---|---|
| `WireFormat` 枚举 | `crates/config/src/provider.rs:31-38` |
| `DeepseekAnthropic` 描述符 | `crates/config/src/provider.rs:140-178` |
| 注册表条目 | `crates/config/src/provider.rs:544`、`:573` |
| 基础 URL / 模型默认值 | `crates/config/src/provider_defaults.rs:8-9,13-14` |
| 分发到 Anthropic 适配器 | `crates/tui/src/client.rs:1331-1352` |
| `api_provider_uses_anthropic_messages` | `crates/tui/src/client.rs:864-869` |
| Auth 头构建（`x-api-key`/`anthropic-version`） | `crates/tui/src/client.rs:805-862` |
| Models-probe 跳过 | `crates/tui/src/client.rs:871-873` |
| FIM 不支持 | `crates/tui/src/client.rs:1722-1727` |
| Chat-Completions usage 解析器 | `crates/tui/src/client.rs:1643-1711` |
| 基础 URL 环境变量覆盖（路由感知） | `crates/tui/src/config.rs:3928-3939` |
| 载荷模式选择 | `crates/tui/src/config.rs:526-530` |
| `build_anthropic_body` | `crates/tui/src/client/anthropic.rs:40-143` |
| Messages URL 构建器 | `crates/tui/src/client/anthropic.rs:259-266` |
| **服务器工具块在编码时被丢弃** | `crates/tui/src/client/anthropic.rs:359-364` |
| Anthropic usage 规范化器 | `crates/tui/src/client/anthropic.rs:499-523` |
| 错误信封解析器 | `crates/tui/src/client/anthropic.rs:528-548` |
| 文档说明 | `docs/PROVIDERS.md:48-51,81,111-112,237` |
| 已落地的 commit / PR | `5b8a5ac0b2c478261740f49756d29c4a7f83d89c` / [#3449](https://github.com/XiaomingX/mimofan/pull/3449) |
