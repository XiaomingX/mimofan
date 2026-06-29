# EPIC-001 Hunter Replay Evidence

**Target branch:** `hunter/0.8.62-glm-subagents`
**Replay branch:** `feat/replay-epic-001-on-hunter`
**Related EPIC:** [#2870](https://github.com/XiaomingX/mimo-tui/issues/2870)
**Related issue:** [#2791](https://github.com/XiaomingX/mimo-tui/issues/2791)

This file is the working PR/issue evidence checklist for replaying EPIC-001
FEAT-001, FEAT-002, and FEAT-003 onto the Hunter branch.

## Replay Scope

| Feature | Hunter replay decision |
|---------|------------------------|
| FEAT-001 | No raw cherry-pick. Hunter already contains the newer group-owned command tree and trait-backed registry. |
| FEAT-002 | Replayed semantically as `user_registry.rs`, wired into dispatch, palette, and slash completion. Adapted to keep newer Hunter command-state reset behavior. |
| FEAT-003 | Replayed as public architecture and PR/issue evidence docs for the Hunter target. Old release-branch validation claims were not copied. |

## PR Summary Draft

```markdown
## Summary

Replays the completed EPIC-001 command-boundary work onto
`hunter/0.8.62-glm-subagents`.

## Changes

- Keep Hunter's existing trait-backed built-in command registry and nested
  group-owned command tree as the FEAT-001 result.
- Add a dedicated `UserCommandRegistry` boundary for markdown user commands.
- Route user command dispatch, command palette entries, and slash completion
  through the registry.
- Preserve Hunter's newer command-state reset behavior when a user command
  starts, including todos and plan state.
- Preserve empty `allowed-tools` semantics: an explicit empty value blocks all
  tools.
- Add public architecture and PR/issue evidence docs for the Hunter target.

## Validation

- `cargo fmt --all -- --check`
- `CARGO_TARGET_DIR=/tmp/mimo-tui-hunter-target cargo check -p mimo-tui`
- `CARGO_TARGET_DIR=/tmp/mimo-tui-hunter-target cargo test -p mimo-tui commands::`
- `CARGO_TARGET_DIR=/tmp/mimo-tui-hunter-target cargo test -p mimo-tui command_palette`
- `CARGO_TARGET_DIR=/tmp/mimo-tui-hunter-target cargo test -p mimo-tui slash_completion`
- `git diff --check`
```

## Issue #2870 Comment Draft

```markdown
EPIC-001 has been replayed onto the Hunter target as a semantic replay rather
than raw cherry-picks.

- FEAT-001: represented by Hunter's current trait-backed registry and
  group-owned command tree.
- FEAT-002: replayed as the user-command registry boundary, adapted to preserve
  current Hunter behavior.
- FEAT-003: replayed as public architecture and evidence docs for the Hunter
  target.

Validation evidence is included in the PR body.

Paulo Aboim Pinto
```

## Validation Results

Record live results here before opening or updating the PR.

| Check | Result |
|-------|--------|
| `cargo fmt --all -- --check` | Pass |
| `CARGO_TARGET_DIR=/tmp/mimo-tui-hunter-target cargo check -p mimo-tui` | Pass |
| `CARGO_TARGET_DIR=/tmp/mimo-tui-hunter-target cargo test -p mimo-tui commands::` | Pass: 456 command tests |
| `CARGO_TARGET_DIR=/tmp/mimo-tui-hunter-target cargo test -p mimo-tui command_palette` | Pass: 18 tests |
| `CARGO_TARGET_DIR=/tmp/mimo-tui-hunter-target cargo test -p mimo-tui slash_completion` | Pass: 17 tests |
| `git diff --check` | Pass |
