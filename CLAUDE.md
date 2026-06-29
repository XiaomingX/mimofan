# Claude Repository Guidance

Read `AGENTS.md` first for full agent work conventions.

## Project Overview

CodeWhale is a Rust-based terminal AI programming assistant (对标 opencode / claude code),
supporting multiple LLM providers (DeepSeek, OpenAI, Anthropic, Zai, etc.) with a
TUI interface, sub-agent system, and MCP tool integration.

## Workspace Crates (15 members)

```
codewhale-cli          CLI entry point, clap argument parsing
codewhale-app-server   HTTP application server (axum)
codewhale-tui          TUI interface (ratatui), runtime API, task management,
                       tool execution loop, model/provider picker
codewhale-core         Core engine: turn loop, session, events
codewhale-config       Configuration: providers, routes, model inventory
codewhale-protocol     Protocol definitions: tools, message formats
codewhale-agent        Sub-agent system
codewhale-tools        Built-in tool implementations
codewhale-mcp          MCP server integration
codewhale-hooks        Pre/post tool hooks
codewhale-execpolicy   Execution policy (security sandbox)
codewhale-secrets      Secret/key management
codewhale-state        State persistence (SQLite via rusqlite)
codewhale-whaleflow    Workflow engine
codewhale-release      Release tooling
```

default-members: `cli`, `app-server`, `tui`

### Crate Dependency Flow (simplified)

```
cli / app-server / tui   (binary crates, top-level entry points)
  └─ core                 (engine, turn loop, session)
       ├─ config          (provider, route, model config)
       ├─ protocol        (tool & message types)
       ├─ agent           (sub-agent orchestration)
       ├─ tools           (builtin tools)
       ├─ mcp             (MCP integration)
       ├─ hooks           (pre/post hooks)
       ├─ execpolicy      (sandbox)
       ├─ secrets         (key management)
       ├─ state           (SQLite persistence)
       └─ whaleflow       (workflow engine)
```

## Tech Stack

- Rust 2024 edition, rust-version = "1.88" (requires `let_chains` feature)
- tokio (full features) async runtime
- ratatui TUI framework
- clap CLI parsing
- serde / serde_json / toml serialization
- reqwest HTTP client (rustls)
- rusqlite SQLite (bundled)
- axum HTTP framework
- tracing logging

## Build / Test / Format Quick Reference

```bash
cargo fmt                                         # Format all code
cargo test -p codewhale-tui --locked              # TUI tests
cargo test -p codewhale-config                    # Config tests
cargo test -p codewhale-protocol                  # Protocol tests
cargo test --workspace                            # Full workspace tests
cargo build --release -p codewhale-cli \
                        -p codewhale-tui          # Release build
```

### Known Test Papercuts (pre-existing, not regressions)

- `config_command_allow_shell_*` fails when `~/.codewhale/settings.toml` has
  `default_mode = "yolo"` (tests are not hermetic)
- `run_verifiers_background_*` is flaky under full-suite parallelism but passes
  in isolation

## Code Style Conventions

- Rust standard style; run `cargo fmt` before every commit
- No `any`-equivalent: avoid `Box<dyn Any>` where a trait or enum suffices
- Prefer `thiserror` for library error types, `anyhow` only in binary crates
- Async everywhere via tokio; avoid `spawn_blocking` unless I/O-bound and measured
- One concern per commit; write a real commit body
- Commit as **WIP** unless behavior is actually verified (built binary, ran test,
  reproduced fix)

## Provider System

CodeWhale supports cloud API providers only. Local inference providers (Ollama,
Vllm, Sglang) have been removed. Provider configuration lives in
`codewhale-config` with route resolution in `config/src/route/`.

See `docs/PROVIDERS.md` for provider-specific details.

## Key Design Constraints

1. **Agent-only surface**: The model-facing sub-agent tool is **`agent` only**.
   There is no `agent_open` / `agent_eval` / `agent_close` / `delegate_to_agent`.
2. **No lifecycle / coherence system**: Do not introduce capacity / coherence /
   runtime-tag systems or lifecycle tools.
3. **No runtime prompt / tag injection**: `constitution.md` (via
   `~/.codewhale/constitution.json`) is the sole base prompt.
4. **Sub-agent depth is configurable**; no arbitrary new limits unless clearly
   needed and explained.
5. **Sub-agent TUI freeze resolved**: v0.8.61 cutover fixed it. Do not commit
   speculative `spawn_blocking` fixes.

## File Organization

```
docs/ARCHITECTURE_CN.md   Architecture doc (Chinese)
docs/CONFIGURATION.md     Configuration guide
docs/PROVIDERS.md         Provider guide
docs/SUBAGENTS.md         Sub-agent guide
docs/MCP.md               MCP integration guide
docs/MODES.md             Modes (plan/agent/yolo)
CHANGELOG.md              Changelog
config.example.toml       Example configuration
~/.codewhale/settings.toml          User settings
~/.codewhale/constitution.json      Constitution (base prompt)
```

## Detailed Docs

For deep dives, read the corresponding doc under `docs/`:
- Architecture and crate relationships: `docs/ARCHITECTURE_CN.md`
- Configuration and routing: `docs/CONFIGURATION.md`
- Provider setup: `docs/PROVIDERS.md`
- Sub-agent system: `docs/SUBAGENTS.md`
- MCP integration: `docs/MCP.md`
- Operating modes: `docs/MODES.md`

---

## Performance Best Practices

### Memory & Allocation

- **Avoid unnecessary `.clone()`**: The codebase has high `clone()` density in
  `subagent/mod.rs`, `ui.rs`, `engine.rs`, `runtime_threads.rs`. Prefer borrowing
  (`&str` over `String`, `&[T]` over `Vec<T>`) where lifetime permits. Use
  `Arc::clone()` for shared ownership instead of deep-cloning data.
- **Minimize `.to_string()` in hot paths**: UI rendering (`ui.rs` ~200+ calls) and
  test files are the worst offenders. Use `format!` only when concatenating;
  prefer `&str` references for static text. For `Display` types, use
  `write!` into a reused buffer instead of repeated `to_string()`.
- **Prefer `RwLock` over `Mutex` for read-heavy data**: Engine and runtime use
  `tokio::sync::RwLock` correctly for shared state. New code accessing
  config/session/registry should follow this pattern. `Mutex` is acceptable only
  for short critical sections with infrequent reads.
- **Use `Bytes` / `Cow<'static, str>` for large payloads**: LLM responses,
  streaming chunks, and tool outputs can be large. Avoid copying large strings
  across async boundaries; use `bytes::Bytes` or `Cow` where possible.

### Async & Concurrency

- **Structured concurrency**: Use `tokio::select!` with `CancellationToken` for
  cancellation (already used in engine). Avoid raw `tokio::spawn` without a
  handle — use `spawn_supervised` pattern from `utils.rs`.
- **Channel patterns**: Use `mpsc` for producer-consumer (engine→UI),
  `broadcast` for fan-out (events), `oneshot` for request-response. The codebase
  already follows this; new code must too.
- **Avoid blocking the runtime**: `spawn_blocking` is justified only for
  CPU-bound work (git rev-parse, file hashing). I/O-bound blocking (file reads)
  should use `tokio::fs`. The codebase uses `spawn_blocking` in ~24 places —
  verify necessity before adding new ones.
- **Batch operations**: Group related async operations with `FuturesUnordered`
  (already used in engine for sub-agent orchestration). Avoid sequential
  `await`s where parallelism is possible.

### HTTP & Network

- **Connection reuse**: `reqwest::Client` is shared via `Arc` (in `client.rs`).
  Never create a new client per request.
- **Streaming**: Use `reqwest` streaming for LLM responses (already in place).
  Buffer tool outputs before sending to avoid partial-message overhead.
- **Retry strategy**: Follow existing `with_retry` pattern in `llm_client/mod.rs`
  with exponential backoff + jitter. Respect `Retry-After` headers.

---

## Maintainability Best Practices

### File Size & Module Structure

- **Hotspot files requiring decomposition**:
  - `tui/ui.rs` (11,412 lines) — split into `ui/chat.rs`, `ui/sidebar.rs`,
    `ui/footer.rs`, `ui/picker.rs` etc.
  - `tui/main.rs` (9,231 lines) — extract initialization, argument parsing,
    and module wiring into separate files.
  - `tui/ui/tests.rs` (11,197 lines) — co-locate tests with their UI modules.
- **Target**: No source file should exceed 1,000 lines. Test files should
  mirror source structure and stay under 500 lines each.
- **Module depth**: The `tui/src/` directory has 71 top-level `.rs` files —
  consider grouping related modules into subdirectories (e.g., `tools/`, `core/`,
  `fleet/` already exist; `config_*`, `model_*`, `runtime_*` modules should be
  similarly grouped).

### Dependency Hygiene

- **Workspace dependencies**: All shared crates (`serde`, `tokio`, `anyhow`, etc.)
  are declared in `[workspace.dependencies]`. New dependencies MUST be added there
  first, then referenced with `workspace = true` in crate `Cargo.toml`.
- **Feature flags**: The `tui` crate has `tui`/`web`/`json`/`toml` features.
  Keep features minimal and avoid feature-gating core logic.
- **No duplicate versions**: Run `cargo tree -d` periodically to catch version
  drift. The CI `check-versions.sh` script enforces this.

### Error Handling

- **Library crates** (`config`, `protocol`, `secrets`, `execpolicy`, `tools`):
  Use `thiserror` for typed errors. Define `enum FooError` with `#[derive(Error)]`.
- **Binary crates** (`tui`, `cli`, `app-server`): Use `anyhow::Result` with
  `.context()` for rich error chains. Use `bail!` for early returns.
- **Anti-pattern: raw `unwrap()`**: The codebase has ~2,600 `unwrap()` calls.
  In production code, replace with `?` or `.expect("reason")`. In tests,
  `expect()` with a descriptive message is preferred over bare `unwrap()`.
  Hotspots: `mcp/tests.rs` (233), `fleet/manager.rs` (117), `snapshot/repo.rs` (116).
- **Anti-pattern: silent error swallowing**: Never `let _ = result` for
  fallible operations without a comment explaining why the error is safe to ignore.

### Testing Standards

- **Coverage**: 5,654 sync tests + 531 async tests. New features require tests.
- **Test naming**: Use `test_<module>_<scenario>_<expected>` pattern.
- **Async tests**: Use `#[tokio::test]` for async code. Prefer
  `#[tokio::test(start_paused = true)]` for time-sensitive tests.
- **Test isolation**: Tests must not depend on external state (`~/.codewhale/`,
  env vars). Use `tempfile` and env guards (`EnvGuard` in `config/tests.rs`).
- **Snapshot tests**: Use `insta` or `expect_test` for UI output verification
  where appropriate.
- **Integration tests**: Place in `tests/` directory with `support/` helpers.
  Use the PTY harness (`qa_harness/`) for terminal interaction tests.

---

## Code Quality Rules

### Clippy Configuration

CI runs clippy with:
```bash
cargo clippy --workspace --all-features --locked -- \
  -D warnings \
  -A clippy::uninlined_format_args \
  -A clippy::too_many_arguments \
  -A clippy::unnecessary_map_or \
  -A clippy::assertions_on_constants
```

New code must pass clippy without adding new `#[allow(clippy::...)]` attributes.
Currently 40 files have clippy allow attributes — reduce, don't increase.

### Formatting

- No `rustfmt.toml` — use default rustfmt settings.
- CI enforces `cargo fmt --all -- --check`.
- Always run `cargo fmt` before committing.

### Type Safety

- **No `Box<dyn Any>`**: Use enums or traits for polymorphism. The codebase
  has minimal `Box<dyn>` usage (~10 files) — keep it that way.
- **Newtype pattern**: Prefer `struct ProviderId(String)` over raw `String` for
  domain identifiers (already used in `route/ids.rs`).
- **Serde hygiene**: Use `#[serde(deny_unknown_fields)]` on config structs.
  Use `#[serde(default)]` for optional fields with sensible defaults.

### Concurrency Safety

- **`Arc<Mutex<T>>` vs `Arc<RwLock<T>>`**: Use `RwLock` when reads dominate
  (config, session state). Use `Mutex` for write-heavy or short critical sections.
- **Avoid lock ordering bugs**: Document lock acquisition order in comments
  when multiple locks are held simultaneously.
- **`CancellationToken`**: Use for graceful shutdown of spawned tasks.
  The engine already uses this pattern.

### Security

- **No hardcoded secrets**: All keys come from `codewhale-secrets` or env vars.
- **File permissions**: The `secrets` crate checks 0600 on secret files.
- **Command execution**: All shell commands go through `execpolicy` sandbox.
  Never bypass with raw `Command::new()` in user-facing paths.
- **Input sanitization**: Tool names are sanitized via `to_api_tool_name()`.
  User input to LLMs must be properly escaped.

---

## Stewardship Defaults

- Treat community PRs and issues as maintainer evidence. Inspect code, tests,
  linked issues, comments, and CI before merging, harvesting, closing, or
  deferring work.
- Do not tag, publish, create a GitHub Release, or push release artifacts
  without Hunter's explicit approval.
- Keep CodeWhale branding while preserving first-class DeepSeek model/provider
  support and legacy migration care.
- Preserve contributor credit for harvested work with authorship,
  `Co-authored-by`, `Harvested from PR #N by @handle`, and changelog/release
  notes where applicable. Use canonical GitHub-noreply identities from
  `.github/AUTHOR_MAP`; never add bot/tool `Co-authored-by` trailers (Claude,
  codex, cursor) -- the `check-coauthor-trailers.py` CI gate rejects them.

## Scratch Integration Branches

- For release queues, create disposable local branches from the real landing
  branch, for example `scratch/vX.Y.Z-pr-train-YYYYMMDD`.
- Use the scratch branch to merge or cherry-pick candidate PR heads in batches
  and learn which conflicts, tests, and overlaps are real.
- Do not ship the scratch branch itself. It may contain noisy merge commits,
  partial conflict resolutions, and unrelated PR interactions.
- After the scratch experiment, move only the safe result back to the release
  branch as narrow commits or direct merges. Keep each final commit explainable
  and testable.
- A PR that is clean against `main` is not necessarily clean against a release
  branch. Test mergeability against the branch that will actually receive the
  work.
- For already approved PRs, treat approval as a strong priority signal. Still
  inspect diffs, comments, check results, and release-branch conflicts before
  landing.

## Current Release Work

- Confirm the active branch for the current release lane from the latest handoff
  and `git branch --show-current`; recent work has landed on `main` through small
  PRs rather than a long-lived `codex/...` integration branch. This repo lives on
  multiple devices, so do not hard-code a checkout path; work in whichever local
  checkout you have and confirm the branch before editing. Never commit directly
  to `main`.
- Read the workspace version from `Cargo.toml`; it advances per release lane. Do
  not tag, publish, create a GitHub Release, push release artifacts, or merge to
  `main` without Hunter's explicit approval.
- Base release triage on the current GitHub release milestone named in the active
  handoff (`gh issue list --repo XiaomingX/mimo-tui --milestone "<current>" --state open`)
  unless Hunter gives a newer branch/milestone.
- Work the queue in this order: release blockers, recently approved PRs, clean
  PRs with small scope, blocked PRs with obvious fixes, dirty PRs that can be
  harvested safely, then larger architecture issues.
- Prefer batching PR conflict discovery on scratch branches, then harvesting
  reviewed, credited, tested slices back into the release branch.
- Before claiming an issue is done, verify whether the branch already contains
  equivalent work. If it does, prepare the GitHub note/closure path instead of
  reimplementing it.
- See `AGENTS.md` -> "Where to work right now" for build/test commands, known
  suite papercuts, and the removed-machinery guardrails (agent-only surface,
  no lifecycle/coherence systems).
