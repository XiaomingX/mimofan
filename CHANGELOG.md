# 更新日志

本项目的所有重要变更都记录在此。版本遵循[语义化版本控制](https://semver.org/)，从工作区根目录（`Cargo.toml` → `[workspace.package] version`）递增。

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
