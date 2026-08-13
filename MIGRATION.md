# Migration Guide

This document records recent structural refactors and the conventions that
follow from them, so downstream consumers and forks can adjust with minimal
churn. It is updated alongside each landed refactor on `main`.

## Provider convergence

The provider / model surface was consolidated into `mimofan-config`
(`config/src/route/`). Routing now resolves through a single
`ProviderCatalog` instead of ad-hoc per-crate service lists.

**What changed**

- Cloud-only providers. Local inference (`Ollama`) and redundant cloud
  providers (`HuggingFace`, `DeepInfra`, `Together`, `Arcee`, `Fireworks`,
  `Novita`, `WanjieArk`) were removed from the catalog.
- Service configuration lives in `mimofan-config`; the canonical example is
  `config/config.example.toml`.
- `rust-version` is `1.95` (true floor from `rusqlite` / `libsqlite3-sys`
  `cfg_select!`); the codebase itself only needs `1.88` (`let_chains`).

**Action for consumers**

- Stop referencing removed providers; migrate custom routing to the
  `route/` API.
- Build on a toolchain `>= 1.95`.

## Memory system

A persistent memory layer (`crates/memory`) was introduced for cross-session
context. State persistence continues to use `rusqlite` (`mimofan-state`).

**Action for consumers**

- Session/memory state is now read through `mimofan-memory`; do not reach
  into the SQLite schema directly. Behavior is gated by the auto-memory
  settings in `~/.mimofan/`.

## Worktree & Agent-Teams convention

Large refactors and multi-agent parallel work follow `AGENTS.md` /
`CODEBUDDY.md`:

- Big refactors use an isolated `git worktree`; each agent owns a disjoint
  set of files.
- Shared worktrees forbid naked `git reset --hard` / `git stash`
  push-pop (global stash can strand other agents' work).
- Commit promptly; merged feature branches are deleted (local + remote) and
  the worktree is removed.

**Action for contributors**

- Open a worktree for multi-file refactors; coordinate file ownership via the
  shared task list; never `git stash` in a shared worktree.

## Toolchain & quality gates (D12)

- `clippy.toml`, `rustfmt.toml`, `rust-toolchain.toml`, and `deny.toml`
  pin formatting, lint, and dependency-supply-chain policy at the repo root.
- CI runs `cargo clippy --workspace --all-features --locked -- -D warnings`.
- Run `cargo fmt` before every commit.
