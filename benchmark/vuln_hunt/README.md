# vuln-hunt verifier benchmark (W5 — issues #838 / #839)

This directory contains **workstream W5** of the modular-plugins plan: the
`SessionEvent` recording primitive (Rust, in `crates/tui/src/core/engine/trace.rs`)
plus a standalone **Python verifier harness** for the vuln-hunt long-horizon
task. W5 owns *recording* and *scoring*; it does **not** drive the agent (that
is W4's `EvalHarness`) nor implement the vuln-hunt tools (those are done in
`crates/tui/src/tools/{hypothesis,gadget_chain,run_poc}.rs`).

## Layout

```
benchmark/vuln_hunt/
├── evaluate.py          # the VERIFIER (3-dimension scorer)
├── run.sh               # drives selftest + batch over tasks
├── tasks/<task_id>/
│   └── task.json        # prompt + expected (expected_gadgets, expected_poc_expect)
├── fixtures/{good,bad}/ # example artifacts to exercise the verifier offline
│   ├── hypotheses.json  # from the `hypothesis` tool (resolve gate)
│   ├── gadget_chain.json# from the `gadget_chain_trace` tool
│   └── run_poc.json     # from the `run_poc` tool
└── README.md
```

## The three-score verifier

`evaluate.py` scores each task run across three **structural** dimensions
(0.0–1.0). It does NOT substring-match "success".

| Dim | Axis | What it checks | Score |
|-----|------|----------------|-------|
| **Consistency** | B 推理严谨性 | Parse `hypotheses.json`; every `status=="confirmed"` hypothesis must carry `>=1` evidence entry (the "先举证后结论" gate). | backed / confirmed |
| **Trace** | #790 static traceability | Parse `gadget_chain_trace` output; the `satisfied` chain ids must include `expected.expected_gadgets`. | expected-satisfied / expected |
| **Reproduce** | C 可复现 #833 | Assert `run_poc` `realized == true` against `expected.expected_poc_expect` (and the matched expect equals the expected one). | 1.0 iff realized else 0.0 |

Mean = average of the three. This maps directly to plan W5: a task is "solved"
only when the agent (1) argued rigorously, (2) traced the right gadget chain,
and (3) reproduced the bug with a real PoC.

## Run it

```bash
# Offline proof that the scoring logic works (good=1.0 on all dims,
# bad consistency<1.0, reproduce=0.0):
bash benchmark/vuln_hunt/run.sh
# or directly:
python3 benchmark/vuln_hunt/evaluate.py --selftest
```

### Pointing at a real agent run

The Rust Engine driver (W4 `EvalHarness`) should drop, per task, these three
artifacts:

- `<workspace>/.mimofan/hypotheses.json` (from the `hypothesis` tool)
- the `gadget_chain_trace` JSON output (from the `gadget_chain_trace` tool)
- the `run_poc` JSON output (from the `run_poc` tool)

Collect them under `benchmark/vuln_hunt/artifacts/<task_id>/` as
`hypotheses.json`, `gadget_chain.json`, `run_poc.json`, then run:

```bash
python3 benchmark/vuln_hunt/evaluate.py --artifacts-dir artifacts/<task_id> --task <task_id>
# or batch all tasks:
python3 benchmark/vuln_hunt/evaluate.py
```

Results are written to `benchmark/vuln_hunt/results/<task_id>.json` plus a
printed scorecard.

## Rust side

`SessionEvent` / `SessionEventSink` in `trace.rs` are the append-only recording
primitive: `SessionEventSink::open(task_id)` writes to
`~/.mimofan/tasks/<task_id>/session.jsonl`; `emit(&SessionEvent)` appends one
JSON line. A round-trip unit test (`session_event_round_trip`) lives in the same
file. No emit calls are wired into `turn_loop.rs` (W3 owns that); the sink is
the reusable primitive for later wiring.
