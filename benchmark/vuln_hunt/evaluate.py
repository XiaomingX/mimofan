#!/usr/bin/env python3
"""vuln-hunt verifier benchmark (W5, issues #838/#839).

This is the VERIFIER half of the vuln-hunt long-horizon harness. It scores a
single task run across THREE structural dimensions (each 0.0..1.0) and prints a
scorecard. It deliberately does NOT substring-match the word "success" — it
checks the three structural conditions below.

Dimensions
----------
1. Consistency (axis B — 推理严谨性)
   Parse the hypothesis store (`hypotheses.json`) produced for the task. Assert
   every hypothesis with `status == "confirmed"` carries >= 1 evidence entry,
   i.e. the "先举证后结论" (evidence-before-verdict) gate held. The Rust
   `hypothesis` tool enforces this at resolve time, but the verifier re-checks
   the artifact independently.
   Score = (#confirmed hypotheses that are evidence-backed) / (#confirmed
   hypotheses). Open/refuted/inconclusive hypotheses do not count.

2. Trace (static traceability axis, #790)
   Parse the `gadget_chain_trace` output (or its KB). Assert the `satisfied`
   chain ids include every id in `expected.expected_gadgets`.
   Score = (#expected chains satisfied) / (#expected chains).

3. Reproduce (axis C — reproducibility, #833)
   Assert `run_poc` `realized == true` against `expected.expected_poc_expect`
   (the verifier confirms the artifact's `realized` flag AND that the matched
   expect equals the expected substring).
   Score = 1.0 if realized else 0.0.

Running
-------
Two modes:

  a) Self-test (offline, proves the scoring logic):
       python3 evaluate.py --selftest
     Runs the verifier over ./fixtures/{good,bad} and asserts:
       - good scores 1.0 on all three dims + mean 1.0
       - bad scores consistency < 1.0 (confirmed hypothesis with no evidence)
         and reproduce 0.0.

  b) Real task run:
       python3 evaluate.py --artifacts-dir <dir> --task <task_id>
     where <dir> contains hypotheses.json, gadget_chain.json, run_poc.json.
     Or point it at a live agent run by passing the path to the artifacts
     collected for that task (the Rust Engine driver is W4's EvalHarness; this
     script consumes its outputs).

  c) Batch over all tasks (drives evaluate.py over sample tasks):
       bash run.sh
     For each benchmark/vuln_hunt/tasks/<task_id>/task.json it looks for a
     matching artifacts dir (default: ./artifacts/<task_id>) and writes
     results/<task_id>.json + prints a summary scorecard.

The verifier is the reusable primitive: it never drives the agent itself.
"""

import argparse
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def load_json(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


# ---------------------------------------------------------------------------
# Dimension 1: Consistency
# ---------------------------------------------------------------------------
def score_consistency(hyp_store):
    """Return (score, n_confirmed, n_backed).

    score = backed / confirmed. A confirmed hypothesis with zero evidence is a
    gate failure and lowers the score below 1.0.
    """
    hyps = hyp_store.get("hypotheses", []) if isinstance(hyp_store, dict) else []
    confirmed = [h for h in hyps if h.get("status") == "confirmed"]
    if not confirmed:
        # Nothing confirmed => nothing to violate the gate, but also no
        # productive claim. Score 1.0 (vacuously consistent) to avoid punishing
        # an agent that found nothing — the Trace/Reproduce dims cover depth.
        return 1.0, 0, 0
    backed = [h for h in confirmed if len(h.get("evidence", []) or []) >= 1]
    score = len(backed) / len(confirmed)
    return score, len(confirmed), len(backed)


# ---------------------------------------------------------------------------
# Dimension 2: Trace
# ---------------------------------------------------------------------------
def score_trace(gadget_out, expected_gadgets):
    """Return (score, satisfied_expected, total_expected).

    score = (#expected chains that are satisfied) / (#expected chains).
    """
    chains = gadget_out.get("chains", []) if isinstance(gadget_out, dict) else []
    satisfied_ids = {c["chain_id"] for c in chains if c.get("satisfied")}
    if not expected_gadgets:
        return 1.0, 0, 0
    matched = sum(1 for g in expected_gadgets if g in satisfied_ids)
    score = matched / len(expected_gadgets)
    return score, matched, len(expected_gadgets)


# ---------------------------------------------------------------------------
# Dimension 4: Auto-discovery (W2 / #788)
# ---------------------------------------------------------------------------
def score_auto_discovery(auto_json, expected_gadgets):
    """Return (score, discovered_expected, total_expected).

    Reads the artifact produced by the real `auto_gadget_discovery` tool
    (`auto_gadget.json`). A task's declared gadget classes (e.g.
    `runtime-exec`, `jndi-lookup`) are considered discovered iff the tool's
    `sinks_hit` (rule ids like `runtime-exec-sink`) or `pivots_observed`
    (rule ids like `runtime-exec`) contain a matching id. This proves the
    paradigm-level engine *automatically* surfaced the chain — it is never
    hardcoded.

    score = (#expected gadget classes discovered) / (#expected classes).
    When no artifact is present (the task did not ask for auto-discovery), the
    dimension is vacuously 1.0 so it does not penalize non-discovery tasks.
    """
    if auto_json is None:
        return 1.0, 0, 0
    if not expected_gadgets:
        return 1.0, 0, 0
    discovered = set()
    for key in ("sinks_hit", "pivots_observed"):
        for gid in (auto_json.get(key) or []):
            # Strip a trailing `-sink` so `runtime-exec-sink` matches the
            # expected class `runtime-exec`.
            discovered.add(gid.rsplit("-sink", 1)[0])
    matched = sum(1 for g in expected_gadgets if g in discovered)
    score = matched / len(expected_gadgets)
    return score, matched, len(expected_gadgets)


# ---------------------------------------------------------------------------
# Dimension 3: Reproduce
# ---------------------------------------------------------------------------
def score_reproduce(run_poc, expected_expect):
    """Return (score, realized).

    score = 1.0 iff realized is true AND the matched expect equals the expected
    substring (when the artifact recorded one).
    """
    realized = bool(run_poc.get("realized")) if isinstance(run_poc, dict) else False
    if not realized:
        return 0.0, False
    # Bonus structural check: if the artifact recorded which expect matched,
    # confirm it is the expected one (defends against a coincidental match).
    matched_expect = run_poc.get("matched_expect")
    if matched_expect is not None and expected_expect is not None:
        if matched_expect != expected_expect:
            return 0.0, True  # realized but for the wrong reason
    return 1.0, True


# ---------------------------------------------------------------------------
# Orchestration
# ---------------------------------------------------------------------------
def evaluate_task(artifacts_dir, expected):
    """Score one task from an artifacts dir. Returns a result dict."""
    expected_gadgets = expected.get("expected_gadgets", []) or []
    expected_poc = expected.get("expected_poc_expect")

    hyp_path = os.path.join(artifacts_dir, "hypotheses.json")
    chain_path = os.path.join(artifacts_dir, "gadget_chain.json")
    poc_path = os.path.join(artifacts_dir, "run_poc.json")

    # Missing artifacts => that dimension scores 0.0 (cannot demonstrate it).
    cons_score, n_conf, n_backed = (0.0, 0, 0)
    if os.path.exists(hyp_path):
        cons_score, n_conf, n_backed = score_consistency(load_json(hyp_path))

    trace_score, sat_expected, tot_expected = (0.0, 0, len(expected_gadgets))
    if os.path.exists(chain_path):
        trace_score, sat_expected, tot_expected = score_trace(
            load_json(chain_path), expected_gadgets
        )

    rep_score, realized = (0.0, False)
    if os.path.exists(poc_path):
        rep_score, realized = score_reproduce(load_json(poc_path), expected_poc)

    auto_path = os.path.join(artifacts_dir, "auto_gadget.json")
    auto_json = load_json(auto_path) if os.path.exists(auto_path) else None
    # Vacuous 1.0 when no auto-discovery artifact is present (the task did not
    # ask for it), so non-discovery tasks are not penalized and the selftest's
    # good/bad fixtures (which carry no auto_gadget.json) stay at mean 1.0/0.x.
    auto_score, auto_expected, auto_total = (1.0, 0, len(expected_gadgets))
    if auto_json is not None:
        auto_score, auto_expected, auto_total = score_auto_discovery(
            auto_json, expected_gadgets
        )

    mean = (cons_score + trace_score + rep_score + auto_score) / 4.0
    return {
        "consistency": {
            "score": round(cons_score, 4),
            "confirmed": n_conf,
            "evidence_backed": n_backed,
            "note": "fraction of confirmed hypotheses that are evidence-backed",
        },
        "trace": {
            "score": round(trace_score, 4),
            "expected_satisfied": sat_expected,
            "expected_total": tot_expected,
            "expected_gadgets": expected_gadgets,
            "note": "fraction of expected gadget chains satisfied",
        },
        "reproduce": {
            "score": round(rep_score, 4),
            "realized": realized,
            "expected_expect": expected_poc,
            "note": "1.0 iff run_poc realized against expected expect",
        },
        "auto_discovery": {
            "score": round(auto_score, 4),
            "expected_discovered": auto_expected,
            "expected_total": auto_total,
            "expected_gadgets": expected_gadgets,
            "note": "fraction of expected gadget classes auto-discovered by the paradigm engine (non-hardcoded)",
        },
        "mean": round(mean, 4),
    }


def print_scorecard(task_id, result):
    c = result["consistency"]
    t = result["trace"]
    r = result["reproduce"]
    a = result["auto_discovery"]
    print(f"  {task_id}")
    print(f"    consistency : {c['score']:.3f}  "
          f"({c['evidence_backed']}/{c['confirmed']} confirmed backed)")
    print(f"    trace       : {t['score']:.3f}  "
          f"({t['expected_satisfied']}/{t['expected_total']} expected chains)")
    print(f"    reproduce   : {r['score']:.3f}  "
          f"(realized={r['realized']}, expect={r['expected_expect']!r})")
    print(f"    auto-disc   : {a['score']:.3f}  "
          f"({a['expected_discovered']}/{a['expected_total']} expected classes)")
    print(f"    MEAN        : {result['mean']:.3f}")


def selftest():
    print("== vuln-hunt verifier selftest ==")
    ok = True

    # GOOD fixture: all three dims must be 1.0.
    good_expected = {
        "expected_gadgets": ["c3p0-log4shell"],
        "expected_poc_expect": "JNDI connection established",
    }
    good = evaluate_task(os.path.join(HERE, "fixtures", "good"), good_expected)
    print_scorecard("fixtures/good", good)
    for dim in ("consistency", "trace", "reproduce"):
        if good[dim]["score"] != 1.0:
            print(f"  FAIL: good.{dim} expected 1.0 got {good[dim]['score']}")
            ok = False
    if good["mean"] != 1.0:
        print(f"  FAIL: good.mean expected 1.0 got {good['mean']}")
        ok = False

    # BAD fixture: consistency must be < 1.0 (confirmed h2 has no evidence),
    # and reproduce must be 0.0 (run_poc did not realize).
    bad_expected = {
        "expected_gadgets": ["c3p0-log4shell"],
        "expected_poc_expect": "JNDI connection established",
    }
    bad = evaluate_task(os.path.join(HERE, "fixtures", "bad"), bad_expected)
    print_scorecard("fixtures/bad", bad)
    if not (bad["consistency"]["score"] < 1.0):
        print(f"  FAIL: bad.consistency expected <1.0 got {bad['consistency']['score']}")
        ok = False
    if bad["reproduce"]["score"] != 0.0:
        print(f"  FAIL: bad.reproduce expected 0.0 got {bad['reproduce']['score']}")
        ok = False

    print("SELFTEST", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def run_task(task_id, tasks_root, artifacts_root, results_root):
    task_path = os.path.join(tasks_root, task_id, "task.json")
    if not os.path.exists(task_path):
        print(f"skip {task_id}: no task.json")
        return None
    expected = load_json(task_path).get("expected", {})
    artifacts_dir = os.path.join(artifacts_root, task_id)
    if not os.path.isdir(artifacts_dir):
        print(f"skip {task_id}: no artifacts dir at {artifacts_dir}")
        return None
    result = evaluate_task(artifacts_dir, expected)
    os.makedirs(results_root, exist_ok=True)
    out_path = os.path.join(results_root, f"{task_id}.json")
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump({"task_id": task_id, **result}, f, indent=2)
    print_scorecard(task_id, result)
    return result


def main(argv=None):
    p = argparse.ArgumentParser(description="vuln-hunt verifier benchmark")
    p.add_argument("--selftest", action="store_true",
                   help="run the offline verifier self-test over ./fixtures")
    p.add_argument("--artifacts-dir", default=None,
                   help="directory containing hypotheses.json/gadget_chain.json/run_poc.json")
    p.add_argument("--task", default=None,
                   help="task id (used with --artifacts-dir and a task.json for expected)")
    p.add_argument("--tasks-root", default=os.path.join(HERE, "tasks"))
    p.add_argument("--artifacts-root", default=os.path.join(HERE, "artifacts"))
    p.add_argument("--results-root", default=os.path.join(HERE, "results"))
    args = p.parse_args(argv)

    if args.selftest:
        return selftest()

    if args.artifacts_dir and args.task:
        task_path = os.path.join(args.tasks_root, args.task, "task.json")
        expected = load_json(task_path).get("expected", {}) if os.path.exists(task_path) else {}
        result = evaluate_task(args.artifacts_dir, expected)
        print_scorecard(args.task, result)
        return 0

    # Batch mode: iterate over task dirs.
    if not os.path.isdir(args.tasks_root):
        print(f"no tasks root {args.tasks_root}")
        return 1
    task_ids = sorted(
        d for d in os.listdir(args.tasks_root)
        if os.path.isdir(os.path.join(args.tasks_root, d))
    )
    print(f"== vuln-hunt verifier: {len(task_ids)} tasks ==")
    results = []
    for tid in task_ids:
        r = run_task(tid, args.tasks_root, args.artifacts_root, args.results_root)
        if r is not None:
            results.append((tid, r))
    if results:
        mean_of_means = sum(r["mean"] for _, r in results) / len(results)
        print(f"OVERALL MEAN (of task means): {mean_of_means:.3f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
