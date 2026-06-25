# Orchestration disposition and the Fleet substrate

Status: design note (forward-looking)
Related: constitution `## Orchestration` regulation; EPIC #3154; #3167; #3205

## Why this note exists

A CodeWhale agent, given a build larger than one context could hold,
spontaneously orchestrated it: it read the dependency shape, fanned the
independent work out to sub-agents across isolated git worktrees, kept its
own context for deciding and verifying, refused to trust any worker's
"done" until it checked the return, and kept a living backlog so the loop
never stalled. Nobody hardcoded that behavior. The model reached for the
delegation substrate as a *disposition*.

That is the thesis worth protecting: **metacognition belongs to the
model.** The judgment about when work has outgrown a single hand, how to
slice it, who should do each slice, and how to reassemble the pieces is
exactly the kind of decision we trust the model to make. We do not want to
re-encode it as a state machine.

This note separates the two halves of that observation so neither
swallows the other:

- The **disposition** — the orchestrator stance — is instilled by the
  constitution, not by machinery. It is judgment.
- **Fleet** — #3154 / #3167 / #3205 — is the **substrate** the disposition
  reaches for. It is mechanism: durable workers, profiled roles,
  preconfigured loadouts, receipts.

The constitution already draws this line in Article IV (*Legacy*):
"A principle may name the duty; mechanism carries it." Orchestration is the
principle. Fleet is the mechanism. They meet but do not merge.

## The disposition (the judgment)

The new `## Orchestration` regulation unifies four existing Tier-3
regulations into a single stance rather than adding new rules:

- *Composition Pattern for Multi-Step Work* — read the shape, plan before
  you dig.
- *Sub-Agent Strategy* — delegate the doing; brief tightly; keep
  architecture, integration, and verification in the parent.
- *Thinking Delegation* — deep reasoning on a sub-problem is a delegation
  signal, generalized here to all substantial doing, not only reasoning.
- *Keeping the Plan Honest* — the plan is the orchestrator's instrument; a
  stalled parent is the failure the stance exists to prevent.

The regulation deliberately encodes *only judgment*: when to shift from
builder to orchestrator, how to sequence vs. parallelize, how to match a
worker to its work and re-route an unfit one, how to isolate parallel
streams, and the verify-floor that nothing returned is trusted until it is
checked against ground truth (Article II). It carries the small-task
carve-out: you do not orchestrate a one-file change.

Crucially, it names *no* concrete mechanism — no tool names, no role
registry, no slot schema. That is what keeps it a disposition. The same
stance works whether the substrate underneath is short-lived `agent`
fanout today or durable Fleet workers tomorrow.

## The substrate (Fleet)

When an agent orchestrates today, it improvises the loadouts: it picks a
read-only scout for a lookup, a builder for a slice, a review panel for a
design, a verifier for a check — and it re-derives those choices every
turn, in prose, from scratch. Fleet is the layer that lets those choices
be *pre-set* so the agent reaches for a configured role instead of
re-improvising one.

The Fleet design (per the EPIC) keeps three layers distinct:

- **Profile layer (#3167)** — `FleetProfile`, `FleetRole`, `FleetSlot`,
  `FleetLoadout`. Roles already include `scout`, `builder`, `reviewer`,
  `verifier`, `synthesizer`, **`orchestrator`**, `release`, `security`.
  A slot says "when this role appears, use this persona, these tools, this
  permission envelope, this loadout — unless overridden."
- **Route layer (#3205)** — Fleet model classes (`strong` / `balanced` /
  `fast`) and semantic route roles, resolved through the shared route
  resolver. This is "match the worker to the work" made concrete and
  provider-agnostic.
- **Execution layer (#3154)** — durable worker lifecycle, leases,
  heartbeats, retries, receipts, artifacts, and synthesis hooks. This is
  the verify-floor given teeth: a receipt is a checkable record, not a
  worker's word.

The mapping from disposition to substrate is close to one-to-one:

- *read the dependency shape; sequence vs. parallelize* → the orchestrator
  composes a Fleet run of dependent and independent tasks.
- *match the worker to the work* → a `FleetSlot` binds a role to a tool
  profile and a model class (#3167 + #3205).
- *isolate parallel streams; serialize the shared surface* → per-worker
  workspace bounds and writable paths in the task spec.
- *never trust a worker's "done"* → Fleet receipts (`pass` / `fail` /
  `partial` / `verifier_failed`) instead of a transcript claim.
- *keep the loop alive* → durable supervision, planner wakeups on
  barriers, and resume-from-ledger so progress survives a dropped parent.

## What a Fleet profile buys the user

A `FleetProfile` lets a user pre-set the team an agent would otherwise
improvise: a `codex-coder` builder slot on a `balanced`/`strong` loadout,
a `claude-resolver` slot for conflict and merge work, a `design-panel`
reviewer slot that is read-only by default, and a `verifier` slot wired to
a deterministic scorer or a fresh-context verify pass. The disposition
still decides *whether and how* to delegate this turn; the profile decides
*what a delegated worker is made of* so that decision is cheap, repeatable,
and auditable rather than re-narrated every run.

This is the configurability payoff of trusting the model: because the
judgment lives in the model and the loadouts live in config, a user can
tune their team without touching the agent's reasoning, and the agent can
exercise its reasoning without re-litigating provider, model, and
permission policy every turn.

## What this note does not claim

- It does not propose new constitution machinery. The disposition is prose;
  the guarantees (ordering, budgets, permission narrowing, receipts) belong
  to Fleet mechanism per Article IV.
- It does not duplicate the Fleet schema. #3167 and #3205 own the profile
  and loadout shapes; this note only argues *why* they are the right home
  for the loadout choices the disposition currently improvises.
- It does not assert Fleet exists today. The disposition stands on its own
  on the current `agent` substrate; Fleet is where it lands durably. The
  regulation is intentionally substrate-agnostic so it does not rot when the
  substrate underneath it changes.

## Open questions

- How does an orchestrator-role worker (#3167) delegate further without
  unbounded recursive spawning? Delegation depth is a profile concern, but
  the disposition should know the ceiling exists.
- Should the constitution ever *name* Fleet, or stay substrate-agnostic
  forever? This note's position: stay agnostic. The moment the constitution
  names a mechanism, the mechanism becomes load-bearing in the place we
  reserved for judgment.
