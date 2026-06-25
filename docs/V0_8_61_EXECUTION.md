# v0.8.61 Execution Plan — getting all 84 issues to "fix-or-idea-on-a-branch"

> Companion to `docs/V0_8_61_ISSUE_COVERAGE.md` (per-issue dispositions + plans from the
> ultracode triage) and `docs/V0_8_61_RELEASE_TRIAGE.md` (freeze/overlap/goal-mode roadmap).

## 0. Critical scope — read this first

The v0.8.61 milestone has **84 open issues**. The triage classified them:

| Disposition | Count | What it means |
|---|---|---|
| already-done | 16 | branch implements it → close with cited evidence |
| quick-fix | 3 | small + safe → land directly (#3012 landed; #3068, #3208 are docs) |
| design | 52 | real work; needs a plan/epic before code |
| defer | 13 | too large or wrong blast radius for 0.8.61 |

**Being critical:** "land code for all 84 tonight" is neither feasible nor wise. 52 of these are
design-class — several are multi-week epics (goal mode, fleet control plane, model registry,
provider adapter contracts, permission profiles). Fanning out 52 parallel code-writing agents
would (a) contend on one cargo target dir or pay N cold builds, and (b) pile dozens of unreviewed,
cross-cutting diffs onto the release branch — the exact failure mode that breaks a release. So the
plan is **multi-pass**, and each pass merges only **verified, green** branches.

What "work on every issue" actually delivers:
- **16 already-done** → close-with-evidence (no code).
- **3 quick-fix** → land now.
- **52 design** → each gets (this pass) a documented plan in the coverage matrix + a cluster
  design, then (subsequent passes) an implemented, verified branch per the cluster sequencing below.
- **13 defer** → documented rationale; revisit post-0.8.61 unless promoted.

## 1. The architectural spine: WhaleFlow = ultracode, for CodeWhale's many models

A large share of the design issues are facets of **one** architecture. The orchestration pattern
this very triage used — an **orchestrator that fans out specialized workers, hands off via
structured contracts, verifies adversarially, and merges methodically** ("ultracode") — is exactly
what **WhaleFlow** should be *inside* CodeWhale. The CodeWhale-specific twist, and the reason it is
not just a Claude-Code clone: **CodeWhale's workers are heterogeneous model types.** A flash model
scouts/triages cheaply; a pro model synthesizes/implements; per-role model routes are first-class,
not an afterthought.

```
        WhaleFlow (in-product) ≈ ultracode (this session)
   ┌────────────────────────────────────────────────────────────┐
   │  Goal loop (orchestrator)            #3215 #891 #1976 #2058  │  persistent objective
   │     │  fans out / steers / verifies                          │
   │     ▼                                                        │
   │  Durable worker runtime              #3216 #3096 #2211 #1806 │  nonblocking, retrying
   │     │  one headless runtime, three launchers                 │
   │     ▼                                                        │
   │  Fleet control plane                 #3154 #3166 #3167       │  ledger, lease, SSH
   │     │  each worker = codewhale exec                           │
   │     ▼                                                        │
   │  Per-role profiles + MODEL ROUTES    #3217 #2027 #1768 #3205 │  heterogeneous models
   │     │  permissions intersect parent  #414 #426 #1186 #3211   │  + capability gates
   │     ▼                                                        │
   │  /swarm gated until the above is real #3218                  │
   └────────────────────────────────────────────────────────────┘
```

Design implication: build the **model-route-per-role** seam (#3217/#2027/#1768) and the
**permission-intersection** seam (#414/#426/#1186/#3211) as the substrate, then goal mode (#3215)
and the durable fanout (#3216) ride on top, and `/swarm` (#3218) is the honest UX over it. This is
the through-line that turns ~25 scattered issues into one coherent epic.

## 2. Workstreams (clusters) — the unit of agent work

Each cluster is a coherent branch a single worker-agent owns end-to-end (worktree-isolated), sized
to compile + test independently. Model-tier column reflects the WhaleFlow framing (which model a
WhaleFlow worker would use for that role).

| # | Cluster | Issues | Worker role / model tier | Depends on |
|---|---|---|---|---|
| A | Durable worker runtime + nonblocking fanout | #3216 #3096 #2211 #1806 #1679 #2487 #1786 #1737 | implementer / pro | — |
| B | Goal mode persistent loop | #891 #1976 #2058 #2029 #3215 | implementer / pro | A |
| C | Permissions + worker profiles | #414 #426 #1186 #3211 #3217 #3213 #2475 | implementer / pro | A |
| D | Model registry / catalog / routing | #3071 #3072 #3073 #3075 #3076 #3205 #2027 #1768 | implementer / pro | — |
| E | Provider adapters / pricing / conformance | #3083 #3084 #3085 #2984 #2629 #3024 #3004 | implementer / pro | D |
| F | Context / telemetry / budget surfaces | #3086 #2666 #3190 #3016 #3025 | implementer / flash→pro | D |
| G | Cancellation + shell robustness | #1541 #3212 #1737 #1786 #1812 | implementer / pro | A |
| H | TUI surfaces | #2982 #3146 #3188 #3194 #2054 #3074 #3203 | implementer / flash | — |
| I | Prompt / constitution tooling | #3015 #719 | implementer / flash | — |
| J | Packaging / distribution | #1067 #3207 #3208 #2917 #2924 | implementer / flash | — |
| K | Localization / docs | #3087 #3090 #3091 #3092 #3093 #3068 | writer / flash | — |
| L | ACP / integrations / hygiene | #3192 #3214 #3206 | implementer / flash | — |

(Already-done issues are folded into the relevant cluster as "close with evidence"; see coverage matrix.)

## 3. Execution method (the ultracode loop, applied to ourselves)

For each pass:
1. **Pick a wave** of independent clusters (no shared files, deps satisfied).
2. **One worker-agent per cluster**, launched with `isolation: worktree` so parallel edits never
   collide. Each agent: implements the smallest *shippable* slice (or a compiling scaffold +
   tests + design doc when the cluster is epic-scale), runs `cargo fmt` + the focused tests, and
   returns a structured result (branch name, files, test command + result, risks).
3. **Adversarial verify**: a reviewer pass per branch (compiles? tests green? scope creep? release-branch
   conflict?). Only branches that pass advance.
4. **Methodical merge**: cherry-pick/merge green branches into `codex/v0.8.61` one at a time,
   re-running the gate (`cargo test -p codewhale-tui --bins`, fmt, `git diff --check`) after each.
   A red merge is reverted, not patched-over.
5. **Re-triage** the milestone, repeat.

Sequencing: **H, I, J, K, L** (self-contained, low-risk) are the first code wave. **D** (model
registry) unblocks **E/F**. **A** (durable runtime) unblocks **B/C/G** and is the release's center
of gravity — it gets the most careful, most-verified treatment and is **not** rushed.

Build-cost note (critical): parallel worktrees must NOT share one `CARGO_TARGET_DIR` (corruption);
each cold build is minutes. So a code wave is ~3–5 clusters at a time, verified sequentially at the
merge step — not 12 at once.

## 4. Status

**Landed on `codex/v0.8.61` this effort (all verified together — 4702 tui tests green, all crate
gates green, version state OK):**
- 6 community PRs harvested with authorship: #3201, #3195, #3220, #3199, #3197, #3221.
- 3 prepared fixes: triage doc, prompt/cap honesty, freeze cancel-between-batches.
- Quick-fix #3012 (global instructions.md autoload).
- 29 issues retargeted into the milestone; #3013 verified done + credited.

**This pass (in progress):** coverage matrix (all 84) + this plan + remaining quick-fix docs
(#3068, #3208) + first wave of cluster design/scaffold branches via worker-agents.

**Next passes:** code waves per §3 sequencing, merging green branches methodically, until the
milestone is either implemented or explicitly deferred with rationale.
