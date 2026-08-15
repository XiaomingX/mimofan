name: herdr-runtime-self-control
description: >-
  Agent self-coordination runtime primitives for herdr-style agent loops: pause
  and resume sibling agents, inspect lifecycle/health state, and escalate to a
  human. Only activates when the runtime is explicitly enabled via HERDR_ENV
  (or MIMOFAN_RUNTIME_ENV); otherwise it is inert and never advertised to the
  model.
---

# herdr runtime self-control

This skill gives the agent a small, explicit vocabulary for coordinating its
**own** runtime when it is operating inside a herdr-style agent loop (multiple
sibling agents / a supervisor runtime). It is gated behind an environment
variable so that it is only ever offered in runtimes that actually support the
underlying control plane — in every other deployment it stays completely inert.

## Activation guard

The skill is loaded **only** when one of these environment variables is set to a
non-empty value:

- `HERDR_ENV`
- `MIMOFAN_RUNTIME_ENV`

If neither is set, `herdr_skill_if_enabled()` returns `None` and the runtime
never sees this skill. See `crates/tui/src/skills/herdr.rs`.

## Primitive commands

Use these as explicit instructions to the runtime control plane. Prefer the most
narrow primitive that accomplishes the goal.

### `pause_agent <agent_id>`

Suspend a sibling agent's scheduled work without terminating it. Use when one
agent's output would conflict with another in flight, or when you need a stable
snapshot of shared state before proceeding.

- Arguments: `agent_id` — the identifier of the sibling agent (e.g. a worker
  handle, task id, or slot name).
- Effect: the agent stops picking up new turns; already-running synchronous work
  may finish or be checkpointed by the runtime.
- Resume with `resume_agent`.

### `resume_agent <agent_id>`

Resume a previously paused sibling agent. Use once the conflicting condition is
resolved.

### `lifecycle_state [<agent_id>]`

Query the runtime for lifecycle/health state of an agent (or the whole loop when
`agent_id` is omitted). Reports one of: `idle`, `running`, `paused`, `blocked`,
`terminated`, plus last-error / last-heartbeat if tracked.

- Use this before `pause_agent` / `resume_agent` to confirm current state rather
  than assuming it.

### `escalate_to_human <summary>`

Hand control back to the human operator with a short, decision-ready summary:
what is blocked, the options considered, and the specific decision needed. Use
when:

- the task is ambiguous and the cost of guessing is high,
- a sibling agent is stuck in a `blocked` state that the loop cannot resolve,
- or a safety/permission boundary would be crossed by continuing autonomously.

Keep `<summary>` under ~280 characters; include the concrete question, not just
context.

## Operating rules

1. **Prefer inspection over action.** Call `lifecycle_state` before pausing or
   resuming so you act on real state, not assumptions.
2. **Pause narrowly, resume promptly.** Only pause the specific agent that
   conflicts; resume it as soon as the condition clears so throughput recovers.
3. **Escalate early on safety.** When autonomy would cross a permission or
   safety boundary, `escalate_to_human` instead of guessing.
4. **Never assume the guard is on.** If you are unsure whether the runtime is
   herdr-enabled, check for the guard; the skill will simply be unavailable
   otherwise.
