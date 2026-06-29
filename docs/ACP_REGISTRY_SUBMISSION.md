# ACP Registry Submission Prep

Prepared for #3192. This is local maintainer prep only; do not open or push the
external `agentclientprotocol/registry` PR from this train.

## Upstream Registry Requirements

Checked against `agentclientprotocol/registry` on 2026-06-14:

- New entries live in a directory whose name matches the `id` field.
- Each entry needs `agent.json` plus a required `icon.svg`.
- `agent.json` requires `id`, `name`, `version`, `description`, and at least one
  `distribution` method.
- Supported distribution methods are `binary`, `npx`, and `uvx`.
- Package and binary versions must match the entry version, and `latest` is not
  allowed.
- Binary platform ids are `darwin-aarch64`, `darwin-x86_64`, `linux-aarch64`,
  `linux-x86_64`, `windows-aarch64`, and `windows-x86_64`.
- Icons must be 16x16 SVG, square, monochrome, and use `currentColor`.
- Registry CI runs an auth check: `initialize` must return at least one
  `authMethods` entry with `type: "agent"` or `type: "terminal"`.

Sources for the external PR author:

- https://github.com/agentclientprotocol/registry
- https://github.com/agentclientprotocol/registry/blob/main/FORMAT.md
- https://github.com/agentclientprotocol/registry/blob/main/CONTRIBUTING.md
- https://github.com/agentclientprotocol/registry/blob/main/AUTHENTICATION.md
- https://github.com/agentclientprotocol/registry/blob/main/agent.schema.json

## Local ACP Readiness Audit

mimo-tui already exposes ACP through `mimo-tui serve --acp`.

Implemented locally:

- `crates/tui/src/main.rs` accepts `serve --acp` and dispatches to the ACP
  server.
- `crates/tui/src/acp_server.rs` implements JSON-RPC 2.0 over newline-delimited
  stdio.
- `initialize` advertises:
  - `agentInfo.name = "mimo-tui"`
  - `agentInfo.title = "mimo-tui"`
  - `agentInfo.version = env!("CARGO_PKG_VERSION")`
  - `promptCapabilities.embeddedContext = true`
  - `loadSession = false`
  - `mcpCapabilities.http = false`
  - `mcpCapabilities.sse = false`
  - `authMethods` with terminal auth: `auth set --provider <provider>`
- `session/new` creates an in-memory session with a cwd.
- `session/prompt` accepts string prompts plus text/resource/resource_link
  blocks, routes through the configured mimo-tui client, emits one
  `session/update` agent message chunk, then returns `stopReason: "end_turn"`.
- `session/cancel` currently returns `null`.

Known limitations to state clearly:

- The adapter is baseline ACP, not the full interactive TUI/runtime surface.
- `session/cancel` is accepted but does not cancel an in-flight provider call
  yet.
- The response is emitted after the provider completes; it is not token
  streaming.
- ACP does not expose shell tools, file-write tools, checkpoint replay, session
  loading, or the HTTP/SSE runtime API.
- Registry submission should be gated on a local run of the upstream registry
  auth-check before opening the external PR.

Recommendation: submit an `npx` distribution first after the matching npm
version is published. It avoids direct release-asset URL churn and lets the npm
wrapper handle platform selection, checksums, mirrors, and glibc preflight.

## External Registry Files

Create this directory in `agentclientprotocol/registry`:

```text
mimo-tui/
  agent.json
  icon.svg
```

Replace `0.8.61` with the final published mimo-tui version. Do not use
`@latest`.

### `mimo-tui/agent.json`

```json
{
  "id": "mimo-tui",
  "name": "mimo-tui",
  "version": "0.8.61",
  "description": "Provider-agnostic terminal coding agent with first-class DeepSeek support.",
  "repository": "https://github.com/XiaomingX/mimo-tui",
  "website": "https://github.com/XiaomingX/mimo-tui/blob/main/docs/RUNTIME_API.md#acp-stdio-adapter-mimo-tui-serve---acp",
  "authors": ["Hunter Bown"],
  "license": "MIT",
  "distribution": {
    "npx": {
      "package": "mimo-tui@0.8.61",
      "args": ["serve", "--acp"]
    }
  }
}
```

### `mimo-tui/icon.svg`

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none">
  <path d="M2 9.5c0-3.3 2.7-6 6-6h4.5v2H8a4 4 0 0 0-4 4v.5h7.5a2.5 2.5 0 0 0 2.4-1.8l.6-2.2H16l-.7 2.7A4 4 0 0 1 11.5 12H4.2A3 3 0 0 1 2 9.5Z" fill="currentColor"/>
  <path d="M5 7h1.5v1.5H5V7Zm3 0h1.5v1.5H8V7Z" fill="currentColor"/>
</svg>
```

## External PR Draft

Title:

```text
Add mimo-tui ACP agent
```

Body:

```text
Adds mimo-tui to the ACP registry.

mimo-tui is a provider-agnostic terminal coding agent with first-class
DeepSeek support. The submitted distribution uses the published npm package and
runs `mimo-tui serve --acp`.

Local readiness checked in XiaomingX/mimo-tui:
- ACP stdio adapter exists at `mimo-tui serve --acp`.
- `initialize` returns terminal auth via `auth set --provider <provider>`.
- `session/new`, `session/prompt`, and `session/cancel` are implemented.
- The adapter is intentionally baseline: no ACP shell/file tools, no session
  load, and no provider-token streaming yet.

Version: 0.8.61
```

## Pre-Submission Checklist

- Confirm `mimo-tui@0.8.61` is published to npm, or switch the draft to
  versioned GitHub Release binary URLs that exist.
- Run the upstream registry validator:
  `python3 .github/workflows/verify_agents.py --auth-check --agent mimo-tui`
- Verify `npx mimo-tui@0.8.61 serve --acp` returns `authMethods` from
  `initialize` on a clean machine.
- Keep the external PR body explicit that ACP support is baseline and does not
  imply the full TUI/runtime API is available inside ACP.
