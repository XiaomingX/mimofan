# The mimo-tui Agent Runtime — one durable substrate, familiar launchers

This document explains how sub-agents, the headless `exec` path, and Agent Fleet
relate. It exists because these had drifted into *two* parallel "worker"
systems, and the fix is to make the **fleet-backed worker run** the durable
primitive. "Sub-agent" remains useful product vocabulary for a nested role, but
it must not imply a separate execution substrate with weaker lifecycle
semantics. It also answers the open direction question in #2972 ("how much
Claude Code convergence is right?").

## The core idea

There is exactly **one** thing that runs detached agent work: a **headless agent
runtime** wrapped in a durable worker lifecycle. It is a model loop with the
full (policy-gated) tool surface that can, in turn, delegate child work through
the same lifecycle. Everything else is just a different way to *launch* that one
runtime, or a different way to *observe* it.

```
                         ┌───────────────────────────────┐
                         │     headless agent runtime     │
                         │  (full tools + can sub-spawn)  │
                         └───────────────────────────────┘
                            ▲             ▲             ▲
            launches │              │              │ launches
                     │              │              │
        ┌────────────┴───┐  ┌───────┴────────┐  ┌──┴───────────────────┐
        │   TUI turn     │  │ `mimo-tui     │  │   Agent Fleet         │
        │  (interactive, │  │   exec`        │  │  (durable: ledger,    │
        │   in-process)  │  │  (headless CLI,│  │   scheduler, SSH,     │
        │                │  │   anyone/any-  │  │   alerts) — launches   │
        │                │  │   time)        │  │   `mimo-tui exec`     │
        └────────────────┘  └────────────────┘  │   per worker          │
                                                 └───────────────────────┘
```

- A **sub-agent** is the user-facing name for a *nested assignment* with a role
  (`explore`, `review`, `implementer`, `verifier`, ...). It should be backed by
  the same worker run lifecycle as fleet. `agent` is the model-facing launcher,
  not a second runtime.
- **`mimo-tui exec`** is the headless front door: usable by anyone at any time
  (CI, scripts, another agent), full tools, emits a `stream-json` event stream,
  and can spawn sub-agents. It is *the* runtime with a CLI on it.
- A **fleet worker** *is* a `mimo-tui exec` run that the fleet launches and
  tracks durably — locally as a subprocess, or remotely as
  `ssh host … mimo-tui exec …`. The fleet does not re-implement execution; it
  adds **orchestration** (durable ledger, scheduling/leasing/retry, host
  transport, alert escalation) *over* the one runtime.

So "fleet vs sub-agent" is not two categories. It is **the same headless run**:
Fleet is the durable control plane, while sub-agent is the role/UX vocabulary
for a nested worker.

## The cutover rule

If a detached `agent` child can fail on a one-off provider timeout with no
retry while an equivalent fleet worker would retry and preserve ledger evidence,
then the cutover is incomplete. Treat that as a mimo-tui runtime gap, not as
normal "sub-agent behavior".

The compatibility `agent` runtime now retries transient provider header,
stream, and timeout failures with backoff before marking a worker interrupted;
when retries are exhausted it preserves a checkpoint and returns a continuation
handle. The remaining convergence work is to keep that lifecycle durable across
process restarts, remote execution, and full fleet-ledger scheduling.

The target rule is:

- durable or long-running work goes through the fleet worker lifecycle;
- `agent` should enqueue
  or observe a fleet-backed worker run instead of owning an independent
  lifecycle;
- in-process children are allowed only as a small compatibility/latency
  optimization, and they must expose the same terminal states, retry semantics,
  receipts, and inspection handles as the fleet path.

In product language it is fine to say "open a sub-agent". In architecture
language that means "start a nested fleet worker with this role".

## Why this shape (and why it fixes the lag)

The motivating problem: spawning many in-process sub-agents made the TUI lag,
because each child cloned a heavy runtime and rebuilt the whole tool registry,
*and* the TUI rendered a full card/transcript per child.

Surveying Claude Code, Codex, and Kimi, the thing that keeps an orchestrator
light at high fanout is **not** a process boundary — all three run sub-agents
in-process. It is **isolation + a compact event stream**:

- a child's transcript **never** flows back into the parent — the parent gets a
  result summary and a small lifecycle event stream;
- the UI renders **counts** (`2 running / 3 done`), not a child session per
  worker;
- each worker's tool surface is built directly from a **role/capability
  profile**, not "build everything then filter".

"Headless" therefore means *the execution is not shaped like the UI* — it does
**not** mean fewer abilities. A headless worker keeps the full toolset and can
spawn sub-agents.

When the work also needs to be **durable** (survive the TUI closing, a laptop
sleeping) or **remote** (SSH), the fleet runs the worker out-of-process as
`mimo-tui exec`. The heavy construction then lives in another process entirely,
so the orchestrator stays smooth regardless of fanout, and the run survives
restarts — the day-scale autonomy goal of #3154.

## One recursion axis

A worker runs at `spawn_depth = 0` and may spawn children while
`spawn_depth + 1 ≤ max_spawn_depth`, so a budget of `N` affords `N` nested
delegation levels. Sub-agents and fleet workers share **one** axis, sourced from
`mimo-tui_config`:

- `DEFAULT_SPAWN_DEPTH = 3` — the default budget for both standalone sub-agents
  and fleet workers (so they cannot drift into "two moving targets");
- `MAX_SPAWN_DEPTH_CEILING = 8` — the opt-in cap that every configured value
  (fleet `max_spawn_depth`, `agent`'s `max_depth`) clamps to.

The root worker always runs even at budget 0; the budget gates *child*
delegation. The default affords at least three nested levels.

## Event vocabulary

The fleet ledger persists the worker's own event stream rather than a separate,
simulated taxonomy. `mimo-tui exec --output-format stream-json` emits
`{"type": "content" | "tool_use" | "tool_result" | "metadata" | "done" |
"error"}` lines, which map onto the fleet ledger's `FleetWorkerEventPayload`
(`RunningTool`, `Running`, `Completed`, `Failed`, …). One vocabulary, two
surfaces.

## Convergence with Claude Code (#2972)

mimo-tui should converge with Claude Code on **shape**, not on branding:

- **Adopt**: a headless runtime with a real CLI/SDK front door; sub-agents as
  isolated runs that return summaries (not transcripts); a compact, event-driven
  fanout projection; capability/role tool profiles; the skills ecosystem
  (#2743); structured run receipts.
- **Keep distinct**: mimo-tui branding and first-class DeepSeek/GLM/MiniMax/
  multi-provider support; the local-first **Agent Fleet** (durable, SSH-capable
  orchestration) as mimo-tui's own layer above the shared runtime; WhaleFlow as
  the orchestration overlay.
- **Do not** fork execution semantics per surface. The TUI, `agent`,
  `exec`, the Runtime API, and the fleet must all drive the *same* runtime and
  observe the *same* event stream — divergence there is what produced the "two
  moving targets" this document exists to prevent.

The litmus test for any new agent surface: *does it launch and observe the one
runtime, or does it invent a second one?* Only the former is allowed.
