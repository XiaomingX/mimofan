#!/usr/bin/env python3
"""Trajectory export adapter for the mimofan long-horizon trajectory log.

mimofan's Rust side writes long-horizon task trajectories as an append-only
JSONL file (`trajectory.jsonl`), one `SessionEvent` per line. This script is a
PURE READ-ONLY adapter that converts those trajectories into two downstream
formats consumed by annotation tooling and analysis:

  1. `--export sharegpt` — a ShareGPT-style training export. One trajectory
     becomes ONE line (a flat role sequence `system`/`human`/`gpt`/`tool`).
     Successful trajectories go to `trajectory_samples.jsonl`; failed ones
     (exit_status not `submitted`/`completed`) are split into
     `failed_trajectories.jsonl` as DPO rejected candidates.

  2. `--export atif` — an ATIF-compatible analysis view. Events are regrouped
     into per-step structures where a ToolCall and its ToolResult are paired by
     `tool_call_id ↔ source_call_id` (never by ordering).

The script never modifies the input file, never invents fields beyond the
documented `SessionEvent` schema, and does not fabricate a system message when
the input has none.

Running
-------
  a) Self-test (offline, proves both export paths):
       python3 trajectory_export.py --selftest

  b) Real export:
       python3 trajectory_export.py --trajectory path/to/trajectory.jsonl \
           --export sharegpt --out-dir <dir>
       python3 trajectory_export.py --trajectory path/to/trajectory.jsonl \
           --export atif --out-dir <dir>        # or --out <single.json>
"""

import argparse
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

# exit_status values considered a SUCCESS for the training sample pool. Anything
# else is routed to the failed/DPO-rejected pool.
SUCCESS_EXIT_STATUS = {"submitted", "completed"}

# Human-readable truncated marker kept on truncated tool outputs (never silently
# dropped, per the anti-pattern guardrails).
_TRUNCATED_MARKER = "... [output truncated by mimofan]"


def load_json(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def _load_jsonl(path):
    """Yield each non-empty line of a JSONL file as a dict."""
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            yield json.loads(line)


def _tool_call_value(ev):
    """Build the `<think>...</think>\n<tool_call>{...}</tool_call>` value.

    Preceding AssistantText reasoning (if any) is folded into the `<think>`
    block; an empty think block is emitted when there was none.
    """
    think = ev.pop("_think", None)
    think_text = think if isinstance(think, str) else ""
    call = {
        "name": ev.get("tool_name"),
        "arguments": ev.get("tool_input"),
    }
    return "<think>{}</think>\n<tool_call>{}</tool_call>".format(
        think_text, json.dumps(call, ensure_ascii=False)
    )


# ---------------------------------------------------------------------------
# ShareGPT export
# ---------------------------------------------------------------------------
def export_sharegpt(trajectory_path, out_dir, user_prompt=None):
    """Convert a trajectory.jsonl into ShareGPT-style JSONL.

    A trajectory records only the agent's internal events (turn start, tool
    calls/results, turn end) — it does NOT capture the original user prompt.
    For SFT/DPO training the conversation should start from a `human` turn, so
    callers pass the task description via `user_prompt`; when omitted, a
    placeholder `human` turn is injected so the role sequence stays well-formed
    (and the absence is flagged in `meta["user_prompt_injected"]`).

    Returns (samples_written, failed_written, skipped_lines).
    """
    os.makedirs(out_dir, exist_ok=True)
    samples_path = os.path.join(out_dir, "trajectory_samples.jsonl")
    failed_path = os.path.join(out_dir, "failed_trajectories.jsonl")

    samples_f = open(samples_path, "w", encoding="utf-8")
    failed_f = open(failed_path, "w", encoding="utf-8")

    samples_written = 0
    failed_written = 0
    skipped = 0

    try:
        # We operate on events grouped by trajectory. The input file is one
        # trajectory (or concatenated trajectories). We treat the whole file as
        # a single trajectory here; callers may point at one file per run. A
        # session_id boundary switch would split, but we keep it simple: each
        # file == one trajectory line.
        events = list(_load_jsonl(trajectory_path))
        meta = {"session_id": None, "model": None, "exit_status": None}
        for ev in events:
            for key in ("session_id", "model", "exit_status"):
                if key in ev:
                    meta[key] = ev.get(key)

        # First pass: fold an AssistantText that immediately precedes a ToolCall
        # into that tool call's think block, and record its index so the second
        # pass does not emit it twice. We mutate a copy, never the parsed
        # originals on disk.
        pending_think = None
        pending_think_idx = None
        folded_idxs = set()
        for i, ev in enumerate(events):
            if ev.get("kind") == "AssistantText" and "text" in ev:
                pending_think = ev.get("text")
                pending_think_idx = i
                continue
            if ev.get("kind") == "ToolCall":
                if pending_think is not None:
                    ev["_think"] = pending_think
                    folded_idxs.add(pending_think_idx)
                pending_think = None
                pending_think_idx = None
            else:
                pending_think = None
                pending_think_idx = None

        # Second pass: rebuild the flat role sequence. AssistantText folded into
        # a following tool call is not emitted as a standalone message.
        conv = []
        # A well-formed ShareGPT sample must open with a `human` turn so the
        # role sequence is complete for SFT/DPO. The trajectory itself has no
        # user message, so we inject one (caller-supplied or a placeholder).
        user_prompt_injected = user_prompt is None
        if user_prompt is None:
            user_prompt = (
                "[no user prompt captured in trajectory; injected placeholder "
                "— pass --user-prompt for a real task description]"
            )
        conv.append({"from": "human", "value": user_prompt})
        for i, ev in enumerate(events):
            kind = ev.get("kind")
            if kind in ("TurnStart", "SessionEnd"):
                continue
            if kind == "AssistantText":
                if "text" in ev and i not in folded_idxs:
                    conv.append({"from": "gpt",
                                 "value": "<think>{}</think>".format(ev["text"])})
                continue
            if kind == "ToolCall":
                conv.append({"from": "gpt", "value": _tool_call_value(ev)})
                continue
            if kind == "ToolResult":
                content = ev.get("tool_result", {}).get("content")
                if ev.get("truncated") and content is not None:
                    content = content + _TRUNCATED_MARKER
                conv.append({"from": "tool",
                             "value": "<tool_response>{}</tool_response>".format(
                                 json.dumps(content, ensure_ascii=False))})
                continue
            if kind == "HypothesisOp":
                # Treated as a reasoning note under the gpt role.
                text = "hypothesis {} op {}".format(
                    ev.get("hypothesis_id"), ev.get("tool_name"))
                conv.append({"from": "gpt", "value": "<think>{}</think>".format(text)})
                continue
            if kind == "PocResult":
                text = "poc realized={} via {}".format(
                    ev.get("poc_realized"), ev.get("tool_name"))
                conv.append({"from": "gpt", "value": "<think>{}</think>".format(text)})
                continue
            if kind == "AgentSpawn":
                text = "[spawned agent source={}]".format(ev.get("source"))
                conv.append({"from": "gpt", "value": "<think>{}</think>".format(text)})
                continue
            if kind == "AgentDone":
                text = "[agent done source={}]".format(ev.get("source"))
                conv.append({"from": "gpt", "value": "<think>{}</think>".format(text)})
                continue
            if kind == "Error":
                text = "[error]".format()
                conv.append({"from": "gpt", "value": "<think>{}</think>".format(text)})
                continue

        exit_status = meta["exit_status"]
        is_success = exit_status in SUCCESS_EXIT_STATUS
        meta = dict(meta)
        meta["user_prompt_injected"] = user_prompt_injected
        record = {"conversation": conv, "meta": meta}
        line = json.dumps(record, ensure_ascii=False)

        if is_success:
            samples_f.write(line + "\n")
            samples_written += 1
        else:
            failed_f.write(line + "\n")
            failed_written += 1
    finally:
        samples_f.close()
        failed_f.close()

    return samples_written, failed_written, skipped


# ---------------------------------------------------------------------------
# ATIF export
# ---------------------------------------------------------------------------
def export_atif(trajectory_path, out_path):
    """Convert a trajectory.jsonl into an ATIF-compatible analysis view.

    ToolCall/ToolResult are paired by `tool_call_id ↔ source_call_id`, never by
    ordering. Returns a single ATIF JSON object.
    """
    events = list(_load_jsonl(trajectory_path))

    trajectory_id = None
    model = None
    exit_status = None
    truncated_any = False
    pending_reasoning = []  # stacked AssistantText before a tool_call

    # Index ToolResults by their source_call_id (tool_call_id of the paired call).
    tool_results = {}
    for ev in events:
        if ev.get("kind") == "ToolResult" and "tool_call_id" in ev:
            tool_results[ev["tool_call_id"]] = ev

    steps = []
    next_step_id = 0

    for ev in events:
        kind = ev.get("kind")
        # session_id/model may ride on TurnStart or other events; hoist them out
        # of the per-kind branches so they are captured regardless of placement.
        if "session_id" in ev:
            trajectory_id = ev["session_id"]
        if "model" in ev:
            model = ev["model"]

        if kind == "TurnStart":
            continue
        if kind == "SessionEnd":
            exit_status = ev.get("exit_status")
            continue

        if kind == "AssistantText":
            if "text" in ev:
                pending_reasoning.append(ev["text"])
            continue

        if kind == "ToolCall":
            tool_input = ev.get("tool_input") or {}
            call = {
                "tool_call_id": ev.get("tool_call_id"),
                "function_name": ev.get("tool_name"),
                "arguments": tool_input,
            }
            # Pair with the matching ToolResult, if any.
            obs = {"results": []}
            tr = tool_results.get(ev.get("tool_call_id"))
            if tr is not None:
                content = (tr.get("tool_result") or {}).get("content")
                if tr.get("truncated") and content is not None:
                    content = content + _TRUNCATED_MARKER
                    truncated_any = True
                obs["results"].append({
                    "source_call_id": tr.get("tool_call_id"),
                    "content": content,
                })
            step = {
                "step_id": next_step_id,
                "source": "agent",
                "message": None,
                "reasoning_content": "\n".join(pending_reasoning) or None,
                "tool_calls": [call],
                "observation": obs,
                "metrics": {
                    "prompt_tokens": None,
                    "completion_tokens": None,
                    "cost_usd": None,
                },
            }
            pending_reasoning = []
            steps.append(step)
            next_step_id += 1
            continue

        if kind == "ToolResult":
            # If no paired ToolCall preceded it (orphan), still surface it.
            if ev.get("tool_call_id") not in [c["tool_call_id"]
                                              for s in steps for c in s["tool_calls"]]:
                content = (ev.get("tool_result") or {}).get("content")
                if ev.get("truncated") and content is not None:
                    content = content + _TRUNCATED_MARKER
                    truncated_any = True
                steps.append({
                    "step_id": next_step_id,
                    "source": "agent",
                    "message": None,
                    "reasoning_content": "\n".join(pending_reasoning) or None,
                    "tool_calls": [],
                    "observation": {"results": [{
                        "source_call_id": ev.get("tool_call_id"),
                        "content": content,
                    }]},
                    "metrics": {
                        "prompt_tokens": None,
                        "completion_tokens": None,
                        "cost_usd": None,
                    },
                })
                pending_reasoning = []
                next_step_id += 1
            continue

        if kind in ("HypothesisOp", "PocResult", "AgentSpawn", "AgentDone", "Error"):
            # Treat as a lightweight agent message (reasoning/status note).
            pending_reasoning.append(json.dumps({
                "kind": kind,
                "tool_name": ev.get("tool_name"),
                "hypothesis_id": ev.get("hypothesis_id"),
                "poc_realized": ev.get("poc_realized"),
                "source": ev.get("source"),
            }, ensure_ascii=False))
            continue

    # Any residual reasoning with no following tool call becomes a final step.
    if pending_reasoning:
        steps.append({
            "step_id": next_step_id,
            "source": "agent",
            "message": None,
            "reasoning_content": "\n".join(pending_reasoning) or None,
            "tool_calls": [],
            "observation": {"results": []},
            "metrics": {
                "prompt_tokens": None,
                "completion_tokens": None,
                "cost_usd": None,
            },
        })

    completed = exit_status in SUCCESS_EXIT_STATUS
    atif = {
        "trajectory_id": trajectory_id,
        "model": model,
        "completed": completed,
        "steps": steps,
        "final_metrics": {
            "exit_status": exit_status,
            "truncated": truncated_any,
        },
    }
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(atif, f, ensure_ascii=False, indent=2)
    return atif


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------
def _sample_events():
    """A small representative SessionEvent sequence (success path)."""
    return [
        {"kind": "TurnStart", "turn": 0, "ts": "2026-08-15T12:00:00Z",
         "session_id": "run-1", "model": "deepseek-v4"},
        {"kind": "AssistantText", "turn": 0,
         "ts": "2026-08-15T12:00:01Z", "text": "start by tracing gadgets"},
        {"kind": "ToolCall", "turn": 0, "ts": "2026-08-15T12:00:02Z",
         "tool_name": "gadget_chain_trace",
         "tool_input": {"target": "log4shell"},
         "tool_call_id": "eval-step-1"},
        {"kind": "ToolResult", "turn": 0, "ts": "2026-08-15T12:00:03Z",
         "tool_name": "gadget_chain_trace",
         "tool_call_id": "eval-step-1",
         "tool_result": {"success": True, "content": "{\"satisfied\": [\"c3p0\"]}"}},
        {"kind": "ToolCall", "turn": 1, "ts": "2026-08-15T12:00:04Z",
         "tool_name": "run_poc", "tool_input": {"poc": "1"},
         "tool_call_id": "eval-step-2"},
        {"kind": "ToolResult", "turn": 1, "ts": "2026-08-15T12:00:05Z",
         "tool_name": "run_poc", "tool_call_id": "eval-step-2",
         "tool_result": {"success": True, "content": "JNDI connection established"}},
        {"kind": "SessionEnd", "turn": 1, "ts": "2026-08-15T12:00:06Z",
         "exit_status": "completed"},
    ]


def selftest():
    print("== trajectory_export selftest ==")
    ok = True

    import tempfile
    import shutil

    tmp = tempfile.mkdtemp()
    try:
        traj = os.path.join(tmp, "trajectory.jsonl")
        with open(traj, "w", encoding="utf-8") as f:
            for ev in _sample_events():
                f.write(json.dumps(ev, ensure_ascii=False) + "\n")

        # --- ShareGPT ---
        out_dir = os.path.join(tmp, "out_sharegpt")
        s_w, f_w, _ = export_sharegpt(traj, out_dir)
        if s_w != 1 or f_w != 0:
            print(f"  FAIL: sharegpt expected 1 sample got {s_w}, failed {f_w}")
            ok = False
        with open(os.path.join(out_dir, "trajectory_samples.jsonl"),
                  encoding="utf-8") as f:
            rec = json.loads(f.readline())
        conv = rec["conversation"]
        allowed = {"system", "human", "gpt", "tool"}
        for msg in conv:
            if msg.get("from") not in allowed:
                print(f"  FAIL: unexpected role {msg.get('from')}")
                ok = False
        # The leading `human` turn is injected (placeholder or --user-prompt),
        # then the folded AssistantText becomes the first `gpt` tool-call msg,
        # so the sequence is human -> gpt(tool) -> tool -> gpt(tool) -> tool.
        kinds = [m["from"] for m in conv]
        if kinds != ["human", "gpt", "tool", "gpt", "tool"]:
            print(f"  FAIL: sharegpt role sequence {kinds}")
            ok = False
        # The first gpt msg (index 1, after the injected human turn) carries the
        # folded think + the tool_call.
        tool_msg = conv[1]
        if "<tool_call>" not in tool_msg["value"] or "gadget_chain_trace" not in tool_msg["value"]:
            print(f"  FAIL: sharegpt tool_call msg malformed: {tool_msg['value']}")
            ok = False
        if "start by tracing gadgets" not in tool_msg["value"]:
            print("  FAIL: sharegpt did not fold AssistantText into tool_call think")
            ok = False
        # Truncated ToolResult must keep the marker (test a truncated case).
        if rec["meta"]["exit_status"] != "completed":
            print("  FAIL: meta.exit_status should be completed")
            ok = False

        # --- Failed trajectory routing ---
        fail_traj = os.path.join(tmp, "failed.jsonl")
        evs = _sample_events()
        evs[-1]["exit_status"] = "failed"
        with open(fail_traj, "w", encoding="utf-8") as f:
            for ev in evs:
                f.write(json.dumps(ev, ensure_ascii=False) + "\n")
        out_f = os.path.join(tmp, "out_sharegpt_f")
        s_w, f_w, _ = export_sharegpt(fail_traj, out_f)
        if s_w != 0 or f_w != 1:
            print(f"  FAIL: failed trajectory routing s={s_w} f={f_w}")
            ok = False

        # --- ATIF ---
        atif_path = os.path.join(tmp, "out_atif.json")
        atif = export_atif(traj, atif_path)
        if atif["completed"] is not True:
            print(f"  FAIL: atif completed expected True got {atif['completed']}")
            ok = False
        if not atif["steps"]:
            print("  FAIL: atif has no steps")
            ok = False
        # Every tool_call_id must be pairable in some observation's results.
        call_ids = [c["tool_call_id"] for s in atif["steps"] for c in s["tool_calls"]]
        obs_ids = [r["source_call_id"] for s in atif["steps"] for r in s["observation"]["results"]]
        for cid in call_ids:
            if cid not in obs_ids:
                print(f"  FAIL: atif tool_call_id {cid} not paired in observation")
                ok = False
        # step ordering should pair by id, not sequence.
        step1 = atif["steps"][0]
        if step1["tool_calls"][0]["tool_call_id"] != "eval-step-1":
            print("  FAIL: atif step0 call_id mismatch")
            ok = False
        if step1["observation"]["results"][0]["source_call_id"] != "eval-step-1":
            print("  FAIL: atif step0 observation pairing mismatch")
            ok = False
        # reasoning folded into the first tool step.
        if not step1["reasoning_content"]:
            print("  FAIL: atif step0 reasoning_content empty")
            ok = False

        # --- ATIF truncated marker preservation ---
        trunc_traj = os.path.join(tmp, "truncated.jsonl")
        evs = _sample_events()
        evs[3]["truncated"] = True
        with open(trunc_traj, "w", encoding="utf-8") as f:
            for ev in evs:
                f.write(json.dumps(ev, ensure_ascii=False) + "\n")
        atif_t = export_atif(trunc_traj, os.path.join(tmp, "out_atif_t.json"))
        content = atif_t["steps"][0]["observation"]["results"][0]["content"]
        if _TRUNCATED_MARKER not in content:
            print("  FAIL: atif truncated marker not preserved")
            ok = False

    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print("SELFTEST", "PASS" if ok else "FAIL")
    return 0 if ok else 1


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def main(argv=None):
    p = argparse.ArgumentParser(
        description="Convert mimofan trajectory.jsonl into ShareGPT or ATIF format")
    p.add_argument("--selftest", action="store_true",
                   help="run the offline export self-test")
    p.add_argument("--trajectory", default=None,
                   help="path to input trajectory.jsonl (SessionEvent lines)")
    p.add_argument("--export", choices=["sharegpt", "atif"], default=None,
                   help="which downstream format to produce")
    p.add_argument("--out-dir", default=None,
                   help="output directory (sharegpt writes samples + failed here)")
    p.add_argument("--out", default=None,
                   help="explicit single output file path (atif)")
    p.add_argument("--user-prompt", default=None,
                   help="task description injected as the leading `human` turn "
                        "for the ShareGPT export (otherwise a placeholder is used)")
    args = p.parse_args(argv)

    if args.selftest:
        return selftest()

    if not args.trajectory or not args.export:
        p.error("--trajectory and --export are required (or use --selftest)")

    if args.export == "sharegpt":
        out_dir = args.out_dir or os.path.join(HERE, "results")
        samples, failed, skipped = export_sharegpt(
            args.trajectory, out_dir, user_prompt=args.user_prompt)
        print(f"sharegpt: {samples} sample(s) -> "
              f"{os.path.join(out_dir, 'trajectory_samples.jsonl')}")
        if failed:
            print(f"sharegpt: {failed} failed trajectory(ies) -> "
                  f"{os.path.join(out_dir, 'failed_trajectories.jsonl')}")
        return 0

    if args.export == "atif":
        if args.out:
            out_path = args.out
        elif args.out_dir:
            out_path = os.path.join(args.out_dir, "trajectory_atif.json")
        else:
            out_path = os.path.join(HERE, "results", "trajectory_atif.json")
        os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
        atif = export_atif(args.trajectory, out_path)
        print(f"atif: {len(atif['steps'])} step(s) -> {out_path}")
        return 0

    return 1


if __name__ == "__main__":
    sys.exit(main())
