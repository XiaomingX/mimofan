# DeepSeek Anthropic-Compatible Endpoint — Comparison Report & Decision (#2963)

- **Issue:** [#2963](https://github.com/Hmbown/CodeWhale/issues/2963) — *v0.8.65: DeepSeek Anthropic-compatible endpoint wire-protocol spike*
- **Release lane:** v0.8.65
- **Date:** 2026-06-24
- **Status:** Implementation **landed and Experimental**. Keep-vs-promote decision **PENDING live numbers** (Section 4).
- **Scope of this document:** A *report*. It changes no Rust code and makes no live API calls — no DeepSeek credentials are available in this environment, so all live figures below are left as a checklist for a human operator to fill in.

> Do **not** reimplement the route. It already exists on `main` (commit
> `5b8a5ac0b2c478261740f49756d29c4a7f83d89c`, PR
> [#3449](https://github.com/Hmbown/CodeWhale/pull/3449)). This document
> verifies what landed, derives what can be concluded from the code without a
> network, and specifies the exact live procedure to settle the open question.

All file:line citations below are against the tree at this report's commit
(verified ancestry: `5b8a5ac0b` is an ancestor of `HEAD`).

---

## 1. What's landed

The opt-in DeepSeek route that speaks the **Anthropic Messages** wire protocol
is implemented end to end. **It is already in `main`; do not re-implement it.**

### 1.1 Provider descriptor / route selection

- `crates/config/src/provider.rs:140-178` — `DeepseekAnthropic` provider:
  - id `deepseek-anthropic` (`provider.rs:143-145`)
  - display name `DeepSeek (Anthropic-compatible)` (`provider.rs:151-153`)
  - aliases `deepseek_anthropic`, `deepseek-claude`, `deepseek_claude`
    (`provider.rs:171-173`)
  - **wire format `WireFormat::AnthropicMessages`** (`provider.rs:175-177`)
  - API-key env var: **`DEEPSEEK_API_KEY` only** (`provider.rs:163-165`) — it
    does **not** fall back to `ANTHROPIC_API_KEY`.
- `crates/config/src/provider.rs:31-38` — `WireFormat` enum
  (`ChatCompletions` / `Responses` / `AnthropicMessages`).
- Registry wiring: static entry `provider.rs:544`, registered at
  `provider.rs:573`.
- Defaults (`crates/config/src/provider_defaults.rs`):
  - base URL `https://api.deepseek.com/anthropic`
    (`provider_defaults.rs:14`)
  - default model `deepseek-v4-pro` — `DEFAULT_DEEPSEEK_ANTHROPIC_MODEL`
    aliases `DEFAULT_DEEPSEEK_MODEL` (`provider_defaults.rs:8-9`)
- The Chat-Completions DeepSeek route, for contrast, defaults to base URL
  `https://api.deepseek.com/beta` (`provider_defaults.rs:13`) with the same
  default model `deepseek-v4-pro`.

### 1.2 Dispatch

- `crates/tui/src/client.rs:1331-1339` (`create_message`) and
  `client.rs:1341-1352` (`create_message_stream`) route to the Anthropic
  adapter when `api_provider_uses_anthropic_messages(self.api_provider)` is
  true.
- `client.rs:864-869` — `api_provider_uses_anthropic_messages` returns true for
  `ApiProvider::Anthropic | ApiProvider::DeepseekAnthropic`.
- Request payload mode is selected by route, not prompt:
  `crates/tui/src/config.rs:526-530` sets
  `RequestPayloadMode::AnthropicMessages` for `DeepseekAnthropic`, else
  `ChatCompletions`.

### 1.3 Auth dialect

- `crates/tui/src/client.rs:805-838` builds headers:
  - injects `anthropic-version: 2023-06-01` for Anthropic-wire providers
    (`client.rs:808-815`)
  - uses **`x-api-key`** (never `Authorization: Bearer`) for the API key
    (`client.rs:817-819`, applied `client.rs:831-837`)
- `client.rs:846-862` strips any caller-supplied `Authorization` / `api-key` /
  `x-api-key` extra headers so a stale OpenAI-style auth header cannot leak onto
  the Anthropic wire (`is_auth_dialect_header`, `client.rs:858-862`).
- Tests: `deepseek_anthropic_uses_anthropic_header_dialect`
  (`client.rs:2216`+) asserts `x-api-key` + `anthropic-version` are present and
  that Bearer / MiMo headers are absent.

### 1.4 Request encoding (Messages body)

- `crates/tui/src/client/anthropic.rs:40-143` — `build_anthropic_body`:
  - `model` / `max_tokens` / `stream` (`anthropic.rs:41-45`)
  - `system` as text or cache-aware blocks (`anthropic.rs:47-66`)
  - `messages` via `message_to_anthropic` (`anthropic.rs:68-74`,
    `anthropic.rs:291-301`)
  - `tools` with `strict` + `cache_control` (`anthropic.rs:76-98`)
  - `tool_choice` mapped from OpenAI-style string/object to Anthropic object
    form (`anthropic.rs:100-102`, `anthropic.rs:279-287`)
  - reasoning → `thinking: {type: adaptive}` + `output_config.effort`
    (low/medium/high/max), gated on `model_supports_reasoning`
    (`anthropic.rs:104-128`)
  - sampling-parameter rules: send at most one of temperature/top_p, or neither
    for models that reject them (`anthropic.rs:130-139`,
    `anthropic.rs:269-275`)
  - `cache_control` breakpoint placement, capped at 4
    (`anthropic.rs:141`, `anthropic.rs:367-446`)
- Endpoint URL builder tolerates a `/v1` suffix
  (`anthropic.rs:259-266`); `https://api.deepseek.com/anthropic` →
  `…/anthropic/v1/messages`.

### 1.5 Response & stream decoding

- Non-streaming: `anthropic.rs:240-254` (`handle_anthropic_message`) parses the
  JSON body and normalizes `usage`.
- Streaming: `anthropic.rs:170-237` (`handle_anthropic_stream`) is an SSE
  pass-through; `convert_anthropic_sse_data` (`anthropic.rs:450-494`) decodes
  `message_start` / `content_block_*` / `message_delta` / `message_stop` /
  `ping` / `error`, tolerates unknown event types, and normalizes usage on
  `message_start` / `message_delta`.
- Send/error path: `anthropic.rs:145-167` (`send_anthropic_request`) sets
  `Accept: text/event-stream`, maps non-2xx into a typed error via
  `parse_anthropic_error_envelope` (`anthropic.rs:528-548`).

### 1.6 Usage / cache normalization (#2961 convention)

- `anthropic.rs:499-523` (`parse_anthropic_usage`):
  - `prompt_cache_hit_tokens = cache_read_input_tokens`
  - `prompt_cache_miss_tokens = input_tokens + cache_creation_input_tokens`
  - normalized `input_tokens = input_tokens + cache_creation + cache_read`
    (total prompt — the DeepSeek convention)

### 1.7 Operational guardrails added with the route

- Health check **skips the `/anthropic/v1/models` probe** for this route
  (`client.rs:871-873`, `api_provider_skips_models_probe`); test
  `deepseek_anthropic_health_check_skips_models_probe` (`client.rs:2301`+).
- **FIM is unsupported** on this route and fails locally with a clear message
  (`client.rs:1722-1727`); test `deepseek_anthropic_fim_fails_without_http_request`
  (`client.rs:2314`+).
- Base-URL env override is route-aware: `CODEWHALE_BASE_URL` / `DEEPSEEK_BASE_URL`
  writes into `providers.deepseek_anthropic.base_url`
  (`crates/tui/src/config.rs:3928-3939`).
- Translation helper uses the Messages endpoint for this provider
  (`client.rs:974-977`); test
  `deepseek_anthropic_translate_uses_messages_endpoint` (`client.rs:2251`+).

### 1.8 Docs framing

- `docs/PROVIDERS.md:48-51`, `:81`, `:111-112`, `:237` document the route as
  **Anthropic *wire-protocol* compatibility** (not Anthropic model/provider
  semantics), list the aliases, and state "Keep `provider = "deepseek"` for the
  default Chat Completions path."

### 1.9 Test coverage already present (no live calls)

In `crates/tui/src/client/anthropic.rs` `#[cfg(test)]` (from `anthropic.rs:550`):
body cache-control placement, reasoning→effort mapping, sampling-param dropping,
signed/unsigned thinking replay, breakpoint cap, full SSE fixture decode
(text + thinking + signature + tool_use + usage), error/unknown-event handling,
usage mapping with missing cache fields, error-envelope parsing, URL `/v1`
tolerance. In `crates/tui/src/client.rs`: the auth-dialect, models-probe-skip,
translate-endpoint, and FIM-unsupported tests cited above.

---

## 2. Code-derived findings (no live calls needed)

These are behavioral facts that can be stated **from the code today**, before
any live comparison. They are the deltas a reviewer most needs to know.

### 2.1 Server tools / web search are NOT exercised via this route today

`content_block_to_anthropic` **drops** the server-tool block types on encode:

```
crates/tui/src/client/anthropic.rs:359-364
    // Server-tool block types are DeepSeek/internal concepts with no
    // Anthropic client-side wire equivalent.
    ContentBlock::ServerToolUse { .. }
    | ContentBlock::ToolSearchToolResult { .. }
    | ContentBlock::CodeExecutionToolResult { .. } => None,
```

Consequence: any server-tool / web-search content the engine holds is filtered
out before the request is sent on this route. There is also no encode-side path
that *injects* an Anthropic-style server-tool definition (e.g. a `web_search`
tool) into the outbound body — `build_anthropic_body` only forwards
caller-supplied client tools (`anthropic.rs:76-98`). So **server-side web
search / code execution is not exercised through the DeepSeek Anthropic route as
implemented.** Whether DeepSeek's endpoint would *accept* such a tool is a
separate, still-open question that only live testing (Section 4, Test E) can
answer; the code neither offers nor depends on it.

### 2.2 Usage telemetry: two real deltas vs the Chat-Completions path

Compare the two usage parsers:

| Field | Anthropic route (`anthropic.rs:499-523`) | Chat-Completions route (`client.rs:1643-1711`) |
|---|---|---|
| `input_tokens` (normalized) | `input + cache_creation + cache_read` | `input_tokens`/`prompt_tokens` as-is |
| `prompt_cache_hit_tokens` | `cache_read_input_tokens` | `prompt_cache_hit_tokens`, else `prompt_tokens_details.cached_tokens` |
| `prompt_cache_miss_tokens` | `input + cache_creation` | `prompt_cache_miss_tokens`, else `input − hit` |
| `reasoning_tokens` | **always `None`** (`anthropic.rs:519`) | parsed from `completion_tokens_details.reasoning_tokens` (`client.rs:1658-1685`) |
| `reasoning_replay_tokens` | `None` (`anthropic.rs:520`) | `None` (`client.rs:1708`) |
| `server_tool_use` | **always `None`** (`anthropic.rs:521`) | parsed from `server_tool_use.{code_execution,tool_search}_requests` (`client.rs:1687-1700`) |
| `output_tokens` | Anthropic `output_tokens` | `output_tokens`/`completion_tokens`, with fallbacks to reasoning or `total − input` (`client.rs:1648-1670`) |

Two concrete deltas to record honestly in any telemetry comparison:

1. **`reasoning_tokens` is never populated on the Anthropic route.** Reasoning
   *content* still flows (thinking blocks decode and signed blocks replay —
   `anthropic.rs:315-330`, `anthropic.rs:822-868` fixture), but the **count**
   is dropped. On the Chat-Completions route the count is read from
   `completion_tokens_details.reasoning_tokens`. This is per the #2961/#3085
   "explicit unknown/null for unsupported fields" rule, but it means
   reasoning-token *accounting parity* between the two routes cannot be
   expected — confirm in Test C.
2. **`server_tool_use` is never populated on the Anthropic route** (consistent
   with §2.1: the route doesn't drive server tools).

### 2.3 Thinking / reasoning request shaping differs by design

The Anthropic route maps `reasoning_effort` tiers to
`thinking: {type: adaptive}` + `output_config.effort`
(`anthropic.rs:104-128`), gated on `model_supports_reasoning`. The
Chat-Completions DeepSeek path uses its own reasoning-split / payload
conventions (`config.rs:526-530` selects the payload mode; DeepSeek-family
reasoning handling lives on the Chat path). Equivalent *output* is the bar to
test (Section 3/4), not byte-identical requests.

### 2.4 Caching model differs in shape

The Anthropic route places explicit `cache_control` breakpoints (max 4) on the
prefix and latest user turn (`anthropic.rs:367-446`) and reports cache
hit/miss from Anthropic's `cache_read` / `cache_creation` fields. The
Chat-Completions route relies on DeepSeek's automatic prefix caching and reads
`prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` (or
`prompt_tokens_details.cached_tokens`). Both normalize into the same #2961
fields, so cache *telemetry* is comparable even though the *mechanism* differs.

### 2.5 Capability/operational deltas (route-level, from code)

- **FIM**: supported on Chat-Completions DeepSeek; **unsupported** on the
  Anthropic route (`client.rs:1722-1727`).
- **Models probe**: skipped on the Anthropic route (`client.rs:871-873`); the
  Chat path probes `/models`.
- **Auth**: `x-api-key` + `anthropic-version` (Anthropic route) vs
  `Authorization: Bearer` (Chat route) — `client.rs:817-827`.
- **Endpoint**: `…/anthropic/v1/messages` vs `…/beta` chat completions.

### 2.6 What is *equivalent* by construction

Tool-call and tool-result mapping, image blocks, system prompt, and stop
reasons all have direct encoders (`anthropic.rs:303-358`) and the SSE decoder
reconstructs tool-use input JSON (fixture `anthropic.rs:816-897`). So for an
ordinary "prompt → text / tool_use" exchange, the two routes are expected to be
functionally equivalent; the open questions are the *quantitative* ones
(latency, token counts) and the *server-tool* one.

---

## 3. Comparison methodology

Compare DeepSeek's **Chat-Completions** route (`provider = "deepseek"`) against
its **Anthropic-Messages** route (`provider = "deepseek-anthropic"`) for the
**same model** (`deepseek-v4-pro`, and `deepseek-v4-flash` if the account has
it). Hold everything else constant (same prompt, same `max_tokens`, same
reasoning effort, same temperature where accepted).

Dimensions:

1. **Correctness / output equivalence** — same prompt → semantically equivalent
   answer; same tool selection and arguments for a tool-use prompt; valid JSON
   for a structured prompt.
2. **Latency** — wall-clock total and (for streaming) time-to-first-token, over
   N≥5 runs each; report median + spread, not a single sample.
3. **Token / usage accounting parity** — compare `input_tokens` (normalized),
   `output_tokens`, `prompt_cache_hit_tokens`, `prompt_cache_miss_tokens`,
   `reasoning_tokens`. **Expect `reasoning_tokens` to be null on the Anthropic
   route** (§2.2) — record it, don't treat it as a bug.
4. **Telemetry fields** — which of the #2961/#3085 normalized fields are
   populated vs null on each route; note `server_tool_use` is null on the
   Anthropic route by construction.
5. **Server-tool / web-search support** — does DeepSeek's Anthropic endpoint
   *accept*, *ignore*, or *reject* an Anthropic-style server tool (e.g.
   `web_search`)? Capture the raw request/response. (Recall the engine does not
   send such a tool today — §2.1 — so this is an endpoint-capability probe with
   a hand-built request, not a test of CodeWhale's encoder.)
6. **Error envelopes & rate limiting** — confirm 4xx/5xx map cleanly
   (`anthropic.rs:528-548`) and that the route honors the same retry/backoff.

Pass bar for "comparable" (issue Acceptance Criteria): equivalent correctness on
the smoke tasks, latency within a reasonable band, and usage telemetry that maps
into the normalized fields (with explicit nulls where unsupported).

---

## 4. Runnable live checklist (human, with `DEEPSEEK_API_KEY` set)

All commands are copy-pasteable. They assume the repo root and a DeepSeek key.
**No credentials exist in this environment; these are for a human to run.**

### 4.0 One-time setup

```bash
export DEEPSEEK_API_KEY="sk-..."           # your DeepSeek key
MODEL="deepseek-v4-pro"                      # also repeat with deepseek-v4-flash if available
CHAT_BASE="https://api.deepseek.com"        # Chat Completions (OpenAI-compatible)
ANTH_BASE="https://api.deepseek.com/anthropic"  # Anthropic Messages
mkdir -p benchmark_results/2963-live && cd "$(git rev-parse --show-toplevel)"
```

### Test A — correctness, single turn (text)

Chat Completions:

```bash
curl -sS -w '\n[http %{http_code} | total %{time_total}s | ttfb %{time_starttransfer}s]\n' \
  -X POST "$CHAT_BASE/v1/chat/completions" \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":64,\"stream\":false,
       \"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly the word: PONG\"}]}" \
  | tee benchmark_results/2963-live/A_chat.json
```

Anthropic Messages (note `x-api-key` + `anthropic-version`, no Bearer):

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

Record: does each return "PONG"? HTTP status, total time.

### Test B — usage / token accounting (read the `usage` object on both)

```bash
echo "Chat usage:";      jq '.usage'  benchmark_results/2963-live/A_chat.json
echo "Anthropic usage:"; jq '.usage'  benchmark_results/2963-live/A_anthropic.json
```

Fill in the table:

| Field | Chat Completions | Anthropic Messages |
|---|---|---|
| prompt/input tokens | | |
| completion/output tokens | | |
| cache hit (`prompt_cache_hit_tokens` / `cache_read_input_tokens`) | | |
| cache miss (`prompt_cache_miss_tokens` / `cache_creation_input_tokens`) | | |
| reasoning tokens (`completion_tokens_details.reasoning_tokens`) | | (expected absent) |

### Test C — reasoning / thinking

Chat Completions (DeepSeek reasoner-style):

```bash
curl -sS -X POST "$CHAT_BASE/v1/chat/completions" \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":512,\"stream\":false,
       \"messages\":[{\"role\":\"user\",\"content\":\"A bat and ball cost \$1.10. The bat costs \$1 more than the ball. How much is the ball? Think, then answer.\"}]}" \
  | tee benchmark_results/2963-live/C_chat.json | jq '{content:.choices[0].message, usage:.usage}'
```

Anthropic Messages with adaptive thinking:

```bash
curl -sS -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" -H "anthropic-version: 2023-06-01" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":512,\"stream\":false,
       \"thinking\":{\"type\":\"adaptive\"},\"output_config\":{\"effort\":\"high\"},
       \"messages\":[{\"role\":\"user\",\"content\":\"A bat and ball cost \$1.10. The bat costs \$1 more than the ball. How much is the ball? Think, then answer.\"}]}" \
  | tee benchmark_results/2963-live/C_anthropic.json | jq '{content:.content, usage:.usage}'
```

Record: both should answer **\$0.05**. Note whether a `thinking` block is
returned by the Anthropic route and whether reasoning tokens appear anywhere.

### Test D — tool use (same tool both routes)

Chat Completions:

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

Anthropic Messages:

```bash
curl -sS -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" -H "anthropic-version: 2023-06-01" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":256,
       \"tools\":[{\"name\":\"get_weather\",\"description\":\"Get weather for a city\",
         \"input_schema\":{\"type\":\"object\",\"properties\":{\"city\":{\"type\":\"string\"}},\"required\":[\"city\"]}}],
       \"messages\":[{\"role\":\"user\",\"content\":\"What's the weather in Paris? Use the tool.\"}]}" \
  | tee benchmark_results/2963-live/D_anthropic.json | jq '.content'
```

Record: does each emit a `get_weather` call with `city = "Paris"`?

### Test E — server-tool / web-search capability probe (the open question)

Send an Anthropic-style server tool and **record whether DeepSeek accepts,
ignores, or rejects it** (capture the full body). The engine does not send this
today (§2.1); this is a raw endpoint probe.

```bash
curl -sS -w '\n[http %{http_code}]\n' -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" -H "anthropic-version: 2023-06-01" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":256,
       \"tools\":[{\"type\":\"web_search_20250305\",\"name\":\"web_search\",\"max_uses\":2}],
       \"messages\":[{\"role\":\"user\",\"content\":\"Search the web: what is the latest stable Rust version? Cite a source.\"}]}" \
  | tee benchmark_results/2963-live/E_websearch.json
```

Classify the outcome:
- **Accepted + worked** — response contains server-tool-use / search results.
- **Ignored** — 200 OK, plain answer, no tool activity.
- **Rejected** — 4xx with an error envelope (record `error.type` / message).

### Test F — streaming smoke (both routes)

```bash
# Anthropic SSE
curl -N -sS -X POST "$ANTH_BASE/v1/messages" \
  -H "x-api-key: $DEEPSEEK_API_KEY" -H "anthropic-version: 2023-06-01" \
  -H "Accept: text/event-stream" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":64,\"stream\":true,
       \"messages\":[{\"role\":\"user\",\"content\":\"Count: one two three\"}]}" \
  | tee benchmark_results/2963-live/F_anthropic.sse | head -40
```

Confirm `message_start` → `content_block_*` → `message_delta` → `message_stop`
arrive (the shapes `convert_anthropic_sse_data` decodes, `anthropic.rs:450-494`).

### Test G — end-to-end through CodeWhale (optional, exercises the real adapter)

```bash
# Anthropic route through the built binary
cargo run -q -p codewhale -- --provider deepseek-anthropic --model "$MODEL" \
  --print "Reply with exactly: PONG"
# Chat route for comparison
cargo run -q -p codewhale -- --provider deepseek --model "$MODEL" \
  --print "Reply with exactly: PONG"
```

(Adjust the binary/flag names to the project's actual non-interactive entry
point if different; the point is to run one prompt through each resolved route.)

### 4.1 Results table to fill in

| Dimension | Chat Completions | Anthropic Messages | Verdict |
|---|---|---|---|
| Correctness (A/C/D) | | | |
| Latency median (N=…) | | | |
| TTFT (streaming) | | | |
| Token accounting (B) | | | |
| reasoning_tokens present | | (expected no) | |
| Tool use (D) | | | |
| Web search (E) | n/a | accept / ignore / reject | |
| Streaming (F) | | | |

---

## 5. Decision

**Recommendation: KEEP as Experimental. The keep-vs-promote decision is PENDING
the live numbers in Section 4. This report does not assert a "verified" verdict
because no live calls were made.**

Rationale, grounded in code:

- **Keep (not reject):** the route is fully implemented, isolated behind opt-in
  provider selection (`deepseek-anthropic` / `deepseek-claude`), guarded
  (FIM-unsupported message, models-probe skip, auth-header hygiene), and covered
  by unit + SSE-fixture tests. It does not touch or regress the default
  Chat-Completions DeepSeek path (separate dispatch at `client.rs:1331-1352`;
  docs say keep `provider = "deepseek"` for the default). Nothing in the code
  argues for ripping it out.
- **Do not promote yet:** the issue's promotion bar requires the Anthropic route
  to be *at least comparable* on a live A/B, plus explicit server-tool evidence.
  That evidence does not exist here. Two code-derived caveats that promotion
  must weigh: (a) `reasoning_tokens` accounting is dropped on this route
  (§2.2 #1), and (b) server tools / web search are not exercised through it
  (§2.1) — so if web search is a requirement for "preferred," this route does
  not satisfy it today regardless of what Test E shows about the endpoint.
- **Gate to flip the decision:** complete Section 4 (especially Tests A–E),
  fill the §4.1 table, and confirm equivalent correctness + comparable latency +
  clean telemetry mapping. If all green and web search is not a blocker →
  candidate to promote to preferred for DeepSeek V4. Otherwise → remain
  Experimental, or reject the *promotion* (not the route) if telemetry/latency
  regress.

### Suggested issue note (after live numbers are in)

> Implementation verified landed (#3449 / `5b8a5ac0b`); see
> `benchmark_results/deepseek-anthropic-comparison-2026-06-24.md`. Live A/B
> results: [fill in]. Server-tool/web-search probe (Test E): [accept/ignore/
> reject + evidence]. Decision: [keep experimental | promote to preferred].

---

## Appendix — citation index

| Topic | Location |
|---|---|
| `WireFormat` enum | `crates/config/src/provider.rs:31-38` |
| `DeepseekAnthropic` descriptor | `crates/config/src/provider.rs:140-178` |
| Registry entry | `crates/config/src/provider.rs:544`, `:573` |
| Base URL / model defaults | `crates/config/src/provider_defaults.rs:8-9,13-14` |
| Dispatch to Anthropic adapter | `crates/tui/src/client.rs:1331-1352` |
| `api_provider_uses_anthropic_messages` | `crates/tui/src/client.rs:864-869` |
| Auth header build (`x-api-key`/`anthropic-version`) | `crates/tui/src/client.rs:805-862` |
| Models-probe skip | `crates/tui/src/client.rs:871-873` |
| FIM unsupported | `crates/tui/src/client.rs:1722-1727` |
| Chat-Completions usage parser | `crates/tui/src/client.rs:1643-1711` |
| Base-URL env override (route-aware) | `crates/tui/src/config.rs:3928-3939` |
| Payload-mode selection | `crates/tui/src/config.rs:526-530` |
| `build_anthropic_body` | `crates/tui/src/client/anthropic.rs:40-143` |
| Messages URL builder | `crates/tui/src/client/anthropic.rs:259-266` |
| **Server-tool blocks dropped on encode** | `crates/tui/src/client/anthropic.rs:359-364` |
| Anthropic usage normalizer | `crates/tui/src/client/anthropic.rs:499-523` |
| Error-envelope parser | `crates/tui/src/client/anthropic.rs:528-548` |
| Docs framing | `docs/PROVIDERS.md:48-51,81,111-112,237` |
| Landed commit / PR | `5b8a5ac0b2c478261740f49756d29c4a7f83d89c` / [#3449](https://github.com/Hmbown/CodeWhale/pull/3449) |
