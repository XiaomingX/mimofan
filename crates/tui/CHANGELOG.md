# Changelog

All notable changes to the `mimofan` TUI crate are documented here.
Versions follow [Semantic Versioning](https://semver.org/) and are bumped
from the workspace root (`Cargo.toml` → `[workspace.package] version`).

## [0.0.3-rc.4] - 2026-07-05

### Fixed
- **`StartTurnRequest` now exposes `response_format`.** Added
  `response_format: Option<serde_json::Value>` to `StartTurnRequest`
  (`runtime_threads.rs`) and plumbed it through `Op::SendMessage`
  (`core/ops.rs`), `Session` (`core/session.rs`),
  `handle_send_message` (`core/engine.rs`), and `turn_loop.rs` so that
  the `MessageRequest` used for the turn carries the user-supplied JSON
  mode specification end-to-end. The two `Op::SendMessage` literal
  construction sites in `tui/ui.rs` and `main.rs` pass
  `response_format: None` (the TUI does not yet surface a JSON-mode
  control, but the app-server path now does).

### Verified — XiaomiMiMo API Capability Support

The following XiaomiMiMo capabilities have been confirmed functional
against the live API (`/v1/chat/completions` and `/anthropic/v1/messages`):

**OpenAI Chat Completions (`/v1/chat/completions`):**
- ✓ Basic call (non-stream and stream)
- ✓ Function calling / tools (`{"type":"function",...}`)
- ✓ Image input (`{"type":"image_url","image_url":{"url":"..."}}`)
- ✓ `response_format: {"type":"json_object"}` (structured JSON output)
- ✓ `thinking: {"type":"enabled"/"disabled"}` (deep reasoning)
- ✓ `reasoning_content` + `usage.completion_tokens_details.reasoning_tokens`
- ✗ Web search tool — mimofan uses its own internal `web_search` tool
  (DuckDuckGo / Baidu), not XiaomiMiMo's `{"type":"web_search",...}` API
  tool type
- ✗ Audio input (`{"type":"audio_url",...}`) — no `AudioUrl` variant in
  `ContentBlock` enum
- ✗ Video input — no `VideoUrl` variant in `ContentBlock` enum
- ✗ TTS output / speech synthesis — no API endpoint in mimofan client

**Anthropic Messages (`/anthropic/v1/messages`):**
- ✓ Basic call (non-stream and stream SSE with `message_start`,
  `content_block_delta`, `message_delta`, `message_stop`)
- ✓ Function calling (`content[].type:"tool_use"`)
- ✓ Image input (`content[].type:"image"`, `source.type:"url"`)
- ✓ Thinking (`thinking.type:"enabled"/"disabled"`, `content[].type:"thinking"`)
- ✗ Audio input — no `input_audio` / `audio` content block variant
- ✗ Video input — no `input_video` content block variant
- ✗ TTS output — no API endpoint in mimofan client
- ✗ ASR (audio transcription) — no API endpoint in mimofan client

**OpenAI Responses API (`/v1/responses`):**
- ✗ Not reachable for XiaomiMiMo — dispatch routes XiaomiMiMo to
  Chat Completions or Anthropic Messages; `responses.rs` is retained with
  `#[allow(dead_code)]` for a future OpenAI Codex provider entrypoint.

## [0.0.3-rc.3] - 2026-07-05

### Fixed
- **XiaomiMiMo OpenAI Chat-Completions routing.** The `create_message` /
  `create_message_stream` dispatch hard-coded `ApiProvider::XiaomiMimo`
  to the OpenAI Codex Responses API (`POST /v1/codex/responses`). The
  XiaomiMiMo gateway does not serve that path and returned 404, so any
  base URL other than `…/anthropic` (e.g. `https://api.xiaomimimo.com/v1`
  used for the OpenAI chat-completions dialect) failed before reaching
  the model. The branch is removed; XiaomiMiMo now falls through to the
  OpenAI Chat-Completions client (`/v1/chat/completions`), matching the
  gateway's actual surface. The Anthropic Messages path (driven by
  `base_url` ending in `/anthropic`) is unchanged. The Codex Responses
  helpers in `client/responses.rs` are retained with `#[allow(dead_code)]`
  for the future Codex provider entrypoint.

- **OpenAI `response_format` pass-through.** Added
  `MessageRequest::response_format: Option<serde_json::Value>` and
  forwarded it into the body of `create_message_chat` and
  `handle_chat_completion_stream`. Enables XiaomiMiMo's JSON mode
  (`{"type":"json_object"}`); the Anthropic Messages dialect ignores
  this field by design (use a JSON-only system prompt there). All 13
  internal `MessageRequest { ... }` literal sites were updated with
  `response_format: None`.

### Tests
- Added `client::tests::message_request_response_format_round_trips`
  and `client::tests::message_request_response_format_omitted_when_none`
  to lock in the new field's serde shape and the
  `skip_serializing_if = "Option::is_none"` invariant.
- Renamed `xiaomi_mimo_token_plan_base_url_keeps_responses_protocol` to
  `xiaomi_mimo_token_plan_base_url_uses_chat_completions_dialect` and
  updated its comment to reflect the new dispatch target.

## [0.0.3-rc.2] - 2026-07-05

### Fixed
- **Runtime API now honours per-provider `default_text_model`.** The
  `POST /v1/threads` (and `POST /v1/tasks`, plus the matching
  `start_thread_turn` path) handlers used to read the top-level
  `default_text_model` field and fall back to the hardcoded
  `DEFAULT_TEXT_MODEL` constant. With the new default provider being
  `XiaomiMiMo`, this meant a thread created without an explicit `model`
  field was being initialised with `deepseek-v4-pro` even when
  `[providers.xiaomi_mimo] default_text_model = "mimo-v2.5-pro"` was
  set. The five resolution sites (`runtime_api::create_task`,
  `runtime_api::create_thread`, `runtime_api::start_thread_turn`,
  `runtime_threads::create_thread`, `task_manager::TaskManagerConfig::from_runtime`)
  now route through `Config::default_model()`, which already implements
  the per-provider → top-level → provider-default resolution order.

- **Default text model updated to `mimo-v2.5-pro`.** The hardcoded
  `DEFAULT_TEXT_MODEL` constant is now `mimo-v2.5-pro` to match the
  default `ApiProvider::XiaomiMimo`. TUI surfaces (model label,
  `EngineConfig::default`, `CompactionConfig::default`, model inventory
  fallback) automatically pick up the new default; per-provider
  `default_text_model` in `config.toml` still wins.

### Tests
- Added `client::tests::xiaomi_mimo_anthropic_base_url_picks_messages_protocol`
  and three sibling tests in `client::tests` to lock in the
  base-url-shaped dispatch (Anthropic Messages vs Responses vs Chat
  Completions) that landed in `0.0.3-rc.1`.

## [0.0.3-rc.1] - 2026-07-04

> Pre-release candidate. Same fix as the planned `0.0.3.1` patch
> (Cargo rejects four-component versions, so this is published as a
> pre-release on top of `0.0.3`).

### Fixed
- **Anthropic / XiaomiMiMo Messages URL routing.** `anthropic_messages_url`
  now appends `/v1/messages` when the configured `base_url` ends in
  `/anthropic` (XiaomiMiMo provider), matching the real endpoint
  `https://api.xiaomimimo.com/anthropic/v1/messages`. Previously it
  produced `…/anthropic/messages` and 404'd against the gateway.

  Verified end-to-end with the `mimo-v2.5-pro` Anthropic-format example
  from the project guide (`POST /anthropic/v1/messages` returns a
  standard `Message` response with `text` + `thinking` content blocks
  and `usage.input_tokens` / `output_tokens`).

### Tests
- Added `xiaomimimo_live_response_decodes_to_message_response` using a
  fixture captured from the live XiaomiMiMo response to lock in the
  `MessageResponse` decoding path (text + thinking content blocks,
  usage normalization, model id preservation).
- Added `xiaomimimo_endpoint_url_for_anthropic_provider` using the
  `base_url` from `~/.mimofan/config.toml` (`providers.xiaomi_mimo`).
- Updated `url_xiaomimimo_anthropic_endpoint` and
  `url_xiaomimimo_anthropic_with_trailing_slash` to expect the corrected
  `/anthropic/v1/messages` URL.
