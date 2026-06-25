# v0.8.65 Release Ledger

Updated: 2026-06-24, America/Los_Angeles.

This ledger tracks the live v0.8.65 milestone by concrete outcome: merged on
`main`, replaced by a clean PR, closed with evidence, or deliberately moved out
of the milestone. It is not a public release record. Tags, package publishing,
GitHub Releases, and website deploys remain separate maintainer actions.

## Current Verdict

v0.8.65 is through the PR-churn closeout: every non-ledger release PR is
merged, auto-closed by a replacement merge, or explicitly closed with evidence.

The milestone itself is not empty. Maintainer direction reinstated four
reporter/user-facing items into v0.8.65 after the PR queue was cleared:
#3461, #3205, #2300, and #1519. Those are the remaining release slice, and
`docs/V0865_REMAINING_AGENT_PROMPT.md` is the focused handoff prompt for the
next agent.

The release is not publicly shipped from this ledger alone. The latest public
ship state still has to be verified at release time by Hunter before any tag,
package publish, GitHub Release, or deploy.

## PR Queue

| PR | Status | Notes |
| --- | --- | --- |
| #3559 | Merged | Harvested @cy2311's zh-Hans JSON extraction, completed current localization coverage, moved the visible details shortcut to bare `v`, added the AUTHOR_MAP entry required by the harvest gate, and removed the stale internal `AltV` message id. Merge: `a7285ea5a2743d28d0c4bb4154526d0e727ac2fe`. |
| #3560 | Merged | Finished the remaining harness-profile split by moving built-in harness seeds and private matching helpers into `crates/config/src/harness.rs` while preserving crate-root exports. Merge: `ec29998cce511047f0a237f2be4d95e9c5108a05`. |
| #3561 | Merged | Harvested shared `integrations/bridge-core` helpers, patched review findings, and verified bridge-core plus Telegram, Feishu, WeCom, and Weixin checks/tests locally. Merge: `ead5165d433a3422625f55e0934443b50faad165`. |
| #3493 | Open ledger PR | This document plus the remaining-agent prompt. It is the only open v0.8.65 PR after the non-ledger PR closeout. |
| #3549 | Merged/auto-associated through #3559 | GitHub marked the original contributor PR merged by the replacement merge commit; final evidence comment posted. |
| #3506 | Closed by #3560 | Final evidence comment posted. |
| #3432 | Merged/auto-associated through #3561 | GitHub marked the original draft PR merged by the replacement merge commit; final evidence comment posted. |

## Landed Release Work

| Area | Evidence |
| --- | --- |
| Public security contact | #3558 updates `SECURITY.md`, release checks, and web footer to use `hmbown@gmail.com` after `security@codewhale.net` bounced. No vulnerability details are recorded here. |
| README/install end-cap | #3552 updates stale 0.8.64 install references to the workspace 0.8.65 line. |
| Provider route/readiness dashboard | #3458, #3485, #3521, #3544, #3555 wire the canonical route resolver and readiness dashboard path through `ReadyRouteCandidate` evidence. |
| Provider facts, catalog, pricing, and live cache | #3497, #3498, #3501, #3502, #3508, #3523, #3556 split provider/model/offering facts, carry route limits, project pricing, and add secret-free live catalog refresh coverage. |
| Usage telemetry | #3509 projects usage into canonical token/cache/reasoning classes; #3544 carries it through the release integration. |
| Fleet substrate, profiles, and runtime proof | #3469, #3511, #3512, #3513, #3516, #3518, #3520, #3525, #3536 land the profiled worker substrate, setup view, runtime API bridge, durable resume, and route-parity proof. |
| Reasoning stream styles | #3446 lands inline reasoning stream routing; #3544 carries the integration. |
| DeepSeek Anthropic-compatible route | #3449 lands the route spike and comparison evidence. |
| Provider/model UX polish | #3484, #3519, #3542, #3551, #3555 improve cross-provider search, picker navigation, MiniMax slug correctness, shortcut hints, and dashboard metadata. |
| YOLO / ask-rule / fallback hardening | #3479, #3531, #3553, #3554, #3547 land yolo/read-only tag behavior, review-intent policy, YOLO ask-rule bypass coverage, fallback privacy guardrails, and persisted file ask-rules. |
| TUI transcript polish | #3557 lands the calm transcript preset. |
| zh-Hans/i18n and bare-`v` detail shortcut | #3559 lands the zh-Hans JSON bundle and removes the visible Alt/Option-V detail shortcut copy. |
| Config module split cleanup | #3560 finishes the harness split for #3311. |
| Bridge integrations | #3561 lands shared bridge-core helpers and review fixes for Telegram/Feishu/WeCom/Weixin bridges. |
| Website/docs provenance | #3514, #3540, #3543 and related docs PRs land source-of-truth, docs structure, and release-credit data surfaces. |
| Community/contributor credit handling | #3517, #3533, #3535 and #3559 follow-up maintain harvest credit rules and manual credit surfaces. |

## Milestone Issues

Closed as v0.8.65 landed/evidenced:

| Issue | Evidence |
| --- | --- |
| #3494 Orchestration disposition | #3470 landed the Orchestration/Fleet disposition and RFC. |
| #3384 ReadyRouteCandidate switches | #3458, #3521, and #3544 establish and consume the canonical route resolver path. |
| #3367 Fleet personas/profile inputs | #3513, #3518, and #3525 land workspace agent profile loading and runtime resolution. |
| #3222 reasoning stream style overrides | #3446 lands the selected-route reasoning stream slice. |
| #3167 Fleet profiles/loadouts/delegation | #3469, #3512, #3513, #3518, and #3525 land profile/loadout plumbing. |
| #3166 Fleet route parity smoke/soak/handoff | #3511 and #3536 land smoke proof and durable route-parity proof. |
| #3154 Fleet execution substrate | #3469, #3516, #3520, #3525, and #3536 land the release substrate. |
| #3086 route context budget service | #3508 and #3523 carry route limits into context budgets. |
| #3085 PricingSku/usage engine | #3501, #3509, and #3544 land pricing provenance and usage projection. |
| #3084 provider descriptors/conformance | #3458 and #3502 land descriptors and conformance coverage. |
| #3075 cross-provider model search | #3484, #3521, and #3544 land search plus resolver-backed selection. |
| #2963 DeepSeek Anthropic-compatible spike | #3449 lands the route spike and evidence. |
| #2961 usage telemetry | #3509 and #3544 land canonical usage projection. |
| #2608 provider/model/offering separation epic | #3458, #3497, #3498, #3501, #3502, #3508, #3521, #3523, #3544, #3555, and #3556 land the release scope. |
| #3311 config module split | #3503, #3505, #3507, and #3560 land the provider defaults, provider kind, harness types, and final harness helper split. |

Open in v0.8.65 as of this update:

| Issue | Remaining scope |
| --- | --- |
| #3461 MCP duplicate server lifecycle/doctor coverage | Current-release MCP lifecycle/doctor work for the concrete Windows duplicate-process report. |
| #3205 Fleet model classes/loadout auto/semantic route roles | Deterministic role/tag/loadout selection so Fleet auto is real, audited, and provider-scoped. |
| #2300 multi-model compatibility + automatic Fleet loadout | User-facing acceptance fixture for the automatic selection half of #3205; provider docs/foundations are already covered. |
| #1519 custom provider endpoints/models/auth | Reporter-raised custom endpoint/model/auth readiness and `/provider`/`/model` custom-row polish. |

Moved out of v0.8.65:

| Issue | New target | Why |
| --- | --- | --- |
| #2984 OpenAI Codex/ChatGPT OAuth route verification + usage display | v0.8.66 | Route infrastructure and Responses docs exist; live-account OAuth verification and usage/quota display proof remain unfabricated follow-up evidence. |

## Required Closeout

1. Use `docs/V0865_REMAINING_AGENT_PROMPT.md` to hand the four remaining
   v0.8.65 issues to the next implementation agent.
2. Merge #3493 when its docs-only checks are green if the handoff ledger is
   useful on `main`.
3. After #3461, #3205, #2300, and #1519 are merged or clearly resolved, update
   this ledger again with the actual closeout evidence.
4. Hunter may perform the separate release-owner actions: final release
   verification, version/tag/package/GitHub Release/publish/deploy decisions.

## Verification Run During Closeout

- #3559: `cargo fmt --all --check`; exact CI clippy command; harvest credit
  checker; `localization::tests`; `tool_details_help_documents_bare_v_without_alt_v`;
  `open_tool_details_pager`.
- #3560: `cargo fmt --all --check`; `cargo test -p codewhale-config --locked harness`;
  `cargo test -p codewhale-config --locked`; `git diff --check`.
- #3561: `npm --prefix integrations/bridge-core run check && npm --prefix integrations/bridge-core test`;
  same `check && test` for Telegram, Feishu, WeCom, and Weixin bridges; `git diff --check`.
