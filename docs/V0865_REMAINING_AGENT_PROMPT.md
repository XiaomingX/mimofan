# v0.8.65 Remaining Agent Prompt

Use this prompt for the next agent that picks up the v0.8.65 finish work.
This is the canonical v0.8.65 remaining-work handoff.

```text
You are working in Hmbown/CodeWhale. Read AGENTS.md first, then confirm the
current branch with `git branch --show-current` before editing. The source of
truth is the live GitHub milestone and the issue/PR comments, not historical
triage notes.

Start clean:

- Do not implement from `codex/v0.8.65-ledger-truth`, #3493's docs branch, or
  any dirty handoff branch.
- Fetch live state, then create a fresh implementation branch/worktree from
  `origin/main`, for example:
  `git fetch origin`
  `git worktree add ../.cw-worktrees/v0865-remaining-mcp -b codex/v0865-remaining-mcp origin/main`
- If you stay in an existing checkout instead, run
  `git switch -c codex/v0865-remaining-<topic> origin/main`.
- Before editing, `git status -sb` must be clean. If it is not clean, inspect
  the changes and do not stage or overwrite unrelated work.
- After creating the branch/worktree, refresh live scope with
  `gh issue list --repo Hmbown/CodeWhale --milestone v0.8.65 --state open` and
  `gh pr list --repo Hmbown/CodeWhale --state open`.

Goal: finish the remaining v0.8.65 milestone issues and get them merged or
clearly resolved:

- #3461 MCP duplicate server instance lifecycle + doctor coverage.
- #3205 Fleet model classes, loadout auto, and semantic route roles.
- #2300 multi-model compatibility/provider docs/automatic Fleet loadout
  selection.
- #1519 custom provider endpoints/models/auth.

Do not tag, publish, cut a GitHub Release, deploy, or bump versions. Keep
release docs and the ledger truthful: v0.8.65 has not been completed by this
handoff, and public release-owner actions are Hunter's.

Hard constraints:

- Never disclose vulnerability details. Public security contacts should be
  `hmbown@gmail.com`, not `security@codewhale.net`.
- Preserve human contributor authorship. Do not add Claude, codex, cursor, or
  other bot/tool co-author trailers.
- Do not fabricate versions, pricing, model ids, readiness, OAuth proof, or
  release status.
- Keep DeepSeek support first-class while using CodeWhale branding.
- Review live PRs with `gh pr list`, `gh pr checks`, comments, and diffs before
  merging. As of this handoff, the non-ledger v0.8.65 PR churn is complete and
  #3493 is the only open PR, but verify live state yourself.

Already done for v0.8.65:

- Security contact correction: #3558.
- README/install end-cap: #3552.
- Provider route/readiness dashboard and ReadyRouteCandidate foundations:
  #3458, #3485, #3521, #3544, #3555.
- Provider facts/catalog/pricing/live cache: #3497, #3498, #3501, #3502,
  #3508, #3523, #3556.
- Usage telemetry: #3509, #3544.
- Fleet substrate/profile/runtime proof: #3469, #3511, #3512, #3513, #3516,
  #3518, #3520, #3525, #3536.
- Reasoning stream styles: #3446, #3544.
- DeepSeek Anthropic-compatible route: #3449.
- Provider/model UX polish: #3484, #3519, #3542, #3551, #3555.
- YOLO/ask-rule/fallback hardening: #3479, #3531, #3553, #3554, #3547.
- Calm transcript preset: #3557.
- zh-Hans/i18n and bare `v` details shortcut: #3559.
- Config harness split: #3560.
- Bridge-core/Telegram/Feishu/WeCom/Weixin integration cleanup: #3561.

Recommended PR split:

1. PR A: #3461 MCP duplicate stdio server lifecycle + doctor/status coverage.
2. PR B: #1519 custom provider endpoint/model/auth readiness and custom rows.
3. PR C: #3205/#2300 deterministic Fleet loadout auto/semantic route tags plus
   docs once behavior is true.

Issue #3461 direction:

- Start from the live issue and comments. The user supplied a concrete Windows
  repro: one `mcp.json` stdio server entry can spawn two Python
  `mcp_server.py` processes on first tool use; one works, one is a small orphan
  with no logs, and killing either can break the pair.
- Do not open user-uploaded attachments unless Hunter explicitly confirms the
  sender/file is trusted. The text in the issue is enough to start.
- Investigate whether both `codewhale.exe` and `codewhale-tui.exe`, or the
  app-server/runtime/TUI split, can independently instantiate the same stdio
  MCP server.
- Make ownership explicit: one process per configured stdio MCP server entry
  per runtime owner, no duplicate spawns for the same entry on lazy load.
- Make working directory handling explicit for relative commands/args.
- Add doctor/status coverage so duplicate registrations or lifecycle ambiguity
  are visible before users discover broken tools.
- Likely seams to inspect first: `docs/MCP.md`, `crates/tui/src/mcp.rs`,
  `crates/tui/src/mcp_server.rs`, `crates/tui/src/mcp/oauth.rs`,
  `crates/tui/src/tui/mcp_routing.rs`,
  `crates/tui/src/commands/groups/utility/mcp.rs`, CLI passthrough in
  `crates/cli/src/lib.rs`, and the runtime/tool-manager paths that build the
  model-visible MCP pool.

Issue #1519 direction:

- This is the custom endpoint/model/auth fixture. Provider-scoped routing and
  custom base URL preservation already exist; finish the remaining dynamic
  custom provider readiness/model-row user surface.
- Preserve custom model ids exactly as provider-scoped wire ids. Do not infer a
  provider switch from prefixes like `deepseek/`, `deepseek-ai/`, `anthropic/`,
  or `openai/`.
- Support HTTP and HTTPS with explicit validation/warnings, including local or
  intentionally insecure custom endpoint semantics where appropriate.
- Keep auth source metadata provider-scoped and secret-free in UI, logs,
  doctor JSON, and persisted payloads.
- Resolve through the ReadyRouteCandidate path before mutating config, UI
  state, engine state, or run ledgers.
- Likely seams to inspect first:
  `crates/config/src/route/resolver.rs`,
  `crates/config/src/route/candidate.rs`,
  `crates/config/src/provider.rs`,
  `crates/config/src/provider_kind.rs`,
  `crates/tui/src/route_runtime.rs`,
  `crates/tui/src/tui/provider_picker.rs`,
  model picker code, and provider/config commands under
  `crates/tui/src/commands/groups`.
- Tests should cover HTTPS custom endpoint, HTTP custom endpoint warning,
  custom model id prefix preservation, missing auth/readiness, `/provider`
  custom row, `/model` custom row, and failed-route rollback.

Issues #3205 and #2300 direction:

- #2300's provider-docs half is already documented in `docs/PROVIDERS.md`.
  Do not claim automatic selection until #3205 behavior is real.
- Use the refined IA from #3205, not the rejected worker-role/profile split:
  users tag models with semantic role labels on the model stat-card; shipped
  defaults provide zero-config tags; user tags are overrides; routing is
  deterministic only.
- Bundle curated default role tags through
  `crates/config/assets/models_dev.bundled.json` and the existing
  `CatalogCompiler` precedence: bundled defaults, then live catalog, then user
  overrides.
- `auto` should mean the role supplies a default profile and resolves
  deterministically. No hidden prompt sniffing, no LLM/router model, and no
  fabricated model ids. If no matching role/tag exists, fall back to the
  provider/current default that satisfies hard requirements.
- Open IA fork: user tags should likely union with bundled defaults, with an
  explicit clear/remove mechanism. Confirm in the issue/PR if you choose
  differently.
- Collapse overlapping Fleet worker route fields. The current overlap is in
  `codewhale_protocol::fleet::FleetTaskWorkerProfile`, with resolution seams
  in `crates/tui/src/tools/subagent/mod.rs` and
  `crates/tui/src/fleet/worker_runtime.rs`.
- Fleet and subagent run/receipt records need provider, model, wire model,
  reasoning, route source, role/loadout/model-class, and deterministic source
  metadata without secrets.
- Likely seams to inspect first:
  `crates/config/src/catalog.rs`,
  `crates/config/src/models_dev.rs`,
  `crates/config/src/lib.rs` (`FleetLoadout`),
  `crates/protocol/src/fleet.rs`,
  `crates/tui/src/fleet/worker_runtime.rs`,
  `crates/tui/src/fleet/ledger.rs`,
  `crates/tui/src/tools/subagent/mod.rs`,
  `crates/tui/src/tools/subagent/tests.rs`, and
  `crates/tui/src/tui/views/fleet_setup.rs`.
- Acceptance tests should prove subagent-vs-Fleet parity for worker role,
  model class/loadout, semantic route tag, reasoning, tool support, and
  fallback behavior. Include a no-hidden-prompt-router regression: freeform
  prompt text alone cannot change provider/model/reasoning.

Verification expectations:

- Always run `cargo fmt --all`.
- Run targeted tests for the touched area, for example:
  `cargo test -p codewhale-config --locked <filter>`,
  `cargo test -p codewhale-protocol --locked <filter>`, and
  `cargo test -p codewhale-tui --bin codewhale-tui --locked <filter>`.
- For MCP work, include focused MCP lifecycle/doctor/status tests.
- For provider/Fleet work, include route resolver, provider picker/model picker,
  custom provider, Fleet worker runtime, and subagent parity tests as relevant.
- Run `cargo test --workspace` or explain the known pre-existing papercuts if
  a full suite is not practical.
- For release-significant code, run
  `cargo build --release -p codewhale-cli -p codewhale-tui` before calling it
  ready.

Closeout:

- Merge only checks-green PRs you have reviewed from diff, comments, and tests.
- After each issue lands or is clearly resolved, post an evidence comment on
  the issue with the merge commit/PR and the exact verification run.
- Keep `docs/V0865_RELEASE_LEDGER.md` current. It should never say the
  milestone is empty until the live milestone actually is empty.
```
