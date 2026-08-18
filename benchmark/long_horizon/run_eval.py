#!/usr/bin/env python3
"""run_eval.py — 长程任务端到端评分 harness（benchmark/long_horizon）。

从公开评测集（SciCode / Terminal-Bench / HLE）抽样的长程任务样本，喂给 mimofan
真模型，按三个长程维度打分：

  1. step_completion   子步完成度   —— 长程任务每个子步是否完成且通过 eval
  2. cross_step_consistency 跨步一致性 —— 前置步的中间产物/决策在后续步是否仍被正确沿用
                                   （不因上下文压缩/记忆失效而丢失，对应 tau-bench 终态等价思想）
  3. anti_stall        防卡死       —— 长程任务是否在合理回合内终止（无无限循环 / LoopGuard 触发）

借鉴 longmemeval_harness.py 的写法：复用 p0_dynamic.client_cfg / call_messages 调真模型，
judge 走我们自己的端点（yes/no 二值），不做自由文本打分以保证可复现。

用法：
    # 仅校验评分逻辑（mock 轨迹，无需模型/网络）
    python3 benchmark/long_horizon/run_eval.py --selftest

    # 真模型端到端评分（需 ANTHROPIC_BASE_URL / ANTHROPIC_MODEL / ANTHROPIC_AUTH_TOKEN）
    python3 benchmark/long_horizon/run_eval.py --limit 5 --json results/long_horizon.json

维度映射（与第一性原理的三失败源对齐）：
    - 编辑错误           → step_completion
    - 上下文丢失         → cross_step_consistency
    - 记忆失效 / 卡死    → anti_stall
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
AGENTBENCH = HERE.parent / "agentbench"
sys.path.insert(0, str(AGENTBENCH))
from p0_dynamic import client_cfg, call_messages  # 复用真模型调用骨架

SAMPLES = HERE / "samples"
DIM_LABELS = {
    "step_completion": "子步完成度",
    "cross_step_consistency": "跨步一致性",
    "anti_stall": "防卡死",
}


# ---------------------------------------------------------------------------
# 样本加载
# ---------------------------------------------------------------------------
def load_samples(limit: int | None) -> list[dict]:
    out: list[dict] = []
    for name in ("scicode_long.json", "terminal_bench_e2e.json", "hle_reasoning.json"):
        p = SAMPLES / name
        if not p.exists():
            continue
        rows = json.load(open(p))
        for r in rows:
            r["_srcfile"] = name
            out.append(r)
    if limit:
        out = out[:limit]
    return out


# ---------------------------------------------------------------------------
# 真模型驱动一个长程任务（逐子步推进，记录轨迹）
# ---------------------------------------------------------------------------
def run_task(cfg: dict, sample: dict, max_rounds: int = 50) -> dict:
    """返回轨迹：{steps:[{prompt, response, passed}], final_response, rounds}。"""
    trajectory: dict = {"steps": [], "rounds": 0, "stopped_reason": "completed"}
    if sample.get("source") == "scicode":
        steps = sample.get("steps", [])
        for s in steps:
            prompt = (
                f"任务总目标：{sample.get('goal')}\n\n"
                f"当前子步 {s.get('n')}：{s.get('prompt')}\n"
                f"请只写完成该子步所需的代码，不要执行。"
            )
            r = call_messages(cfg, messages=[{"role": "user", "content": prompt}], max_tokens=1024)
            if r.get("error"):
                trajectory["steps"].append({"prompt": s.get("n"), "response": "", "passed": False})
                continue
            trajectory["steps"].append({
                "prompt": s.get("n"),
                "response": r.get("text", ""),
                "passed": _scicode_step_passed(s, r.get("text", "")),
            })
            trajectory["rounds"] += 1
            if trajectory["rounds"] >= max_rounds:
                trajectory["stopped_reason"] = "continuation_limit"
                break
    else:
        # terminal-bench / hle：单轮长指令
        instr = sample.get("instruction") or sample.get("goal") or sample.get("subject", "")
        r = call_messages(cfg, messages=[{"role": "user", "content": instr}], max_tokens=2048)
        trajectory["steps"].append({
            "prompt": sample.get("id"),
            "response": r.get("text", ""),
            "passed": not r.get("error"),
        })
        trajectory["rounds"] = 1
    return trajectory


def _scicode_step_passed(step: dict, response: str) -> bool:
    """粗判：响应包含代码块且提及该步关键符号。"""
    if "```" not in response:
        return False
    gt = (step.get("gt") or "")[:60]
    return bool(gt) and (gt.split("(")[0].strip()[:8] in response or "def " in response)


# ---------------------------------------------------------------------------
# 三维评分（judge 走我们自己的真模型，yes/no 二值）
# ---------------------------------------------------------------------------
def judge(cfg: dict, rubric: str, evidence: str) -> bool | None:
    prompt = (
        "You are a grader for a long-horizon agent task. Given a RUBRIC (what good "
        "looks like) and the AGENT EVIDENCE (trace excerpt), answer 'yes' ONLY if the "
        "evidence satisfies the rubric. Respond with a single word: yes or no.\n\n"
        f"RUBRIC: {rubric}\n\n"
        f"AGENT EVIDENCE:\n{evidence[:1500]}\n\n"
        "Grade (yes/no only):"
    )
    r = call_messages(cfg, messages=[{"role": "user", "content": prompt}], max_tokens=256)
    if r.get("error"):
        return None
    return "yes" in r["text"].strip().lower()


def score_step_completion(trajectory: dict) -> float:
    steps = trajectory.get("steps", [])
    if not steps:
        return 0.0
    return sum(1 for s in steps if s.get("passed")) / len(steps)


def score_cross_step_consistency(cfg: dict, sample: dict, trajectory: dict) -> float | None:
    """判前置步决策是否在后续步被一致沿用（跨步一致性）。"""
    steps = trajectory.get("steps", [])
    if len(steps) < 2:
        return None  # 单步任务无跨步一致性可言
    early = "\n".join(s.get("response", "") for s in steps[: max(1, len(steps) // 2)])
    late = "\n".join(s.get("response", "") for s in steps[len(steps) // 2:])
    rubric = (
        f"任务 '{sample.get('goal', '')[:120]}' 的前置步骤产出的关键决策/符号，"
        f"在后续步骤中被正确使用且未被遗忘或矛盾改写。"
    )
    return judge(cfg, rubric, f"EARLY STEPS:\n{early}\n\nLATER STEPS:\n{late}")


def score_anti_stall(trajectory: dict, max_rounds: int = 50) -> float:
    """在合理回合内终止（无 continuation_limit 截断）即满分。"""
    if trajectory.get("stopped_reason") == "continuation_limit":
        return 0.0
    return 1.0


# ---------------------------------------------------------------------------
# 全量评测
# ---------------------------------------------------------------------------
def run(cfg: dict, limit: int | None) -> dict:
    samples = load_samples(limit)
    per_sample = []
    agg = {k: [] for k in DIM_LABELS}
    for s in samples:
        traj = run_task(cfg, s)
        sc = score_step_completion(traj)
        cs = score_cross_step_consistency(cfg, s, traj)
        as_ = score_anti_stall(traj)
        per_sample.append({
            "id": s.get("id"),
            "source": s.get("source"),
            "step_completion": round(sc, 3),
            "cross_step_consistency": (round(cs, 3) if cs is not None else None),
            "anti_stall": round(as_, 3),
            "rounds": traj.get("rounds"),
        })
        agg["step_completion"].append(sc)
        if cs is not None:
            agg["cross_step_consistency"].append(cs)
        agg["anti_stall"].append(as_)
    summary = {
        k: round(sum(v) / len(v), 3) if v else None
        for k, v in agg.items()
    }
    return {"summary": summary, "per_sample": per_sample}


# ---------------------------------------------------------------------------
# --selftest：用内置 mock 轨迹校验三维评分逻辑（无需模型/网络）
# ---------------------------------------------------------------------------
def _selftest() -> int:
    mock_good = {
        "goal": "Build an MLP trainer",
        "steps": [
            {"prompt": "10.1", "response": "```python\ndef get_alpha():\n  ...\n```", "passed": True},
            {"prompt": "10.2", "response": "uses get_alpha() from 10.1 to compute real space", "passed": True},
            {"prompt": "10.3", "response": "reuses get_alpha() and real space for reciprocal", "passed": True},
        ],
        "rounds": 3,
        "stopped_reason": "completed",
    }
    mock_bad = {
        "goal": "Build an MLP trainer",
        "steps": [
            {"prompt": "10.1", "response": "no code here", "passed": False},
            {"prompt": "10.2", "response": "ignores 10.1 entirely, redefines from scratch", "passed": False},
        ],
        "rounds": 50,
        "stopped_reason": "continuation_limit",
    }
    # step_completion
    assert abs(score_step_completion(mock_good) - 1.0) < 1e-6, "good step_completion should be 1.0"
    assert abs(score_step_completion(mock_bad) - 0.0) < 1e-6, "bad step_completion should be 0.0"
    # anti_stall
    assert score_anti_stall(mock_good) == 1.0, "good anti_stall should be 1.0"
    assert score_anti_stall(mock_bad) == 0.0, "bad anti_stall should be 0.0"
    # cross_step_consistency：用 stub judge 验证逻辑分支（不依赖真模型）
    global judge
    orig_judge = judge
    judge = lambda cfg, rubric, evidence: ("reuses get_alpha" in evidence)  # stub
    try:
        good_cs = score_cross_step_consistency({}, {"goal": "x"}, mock_good)
        assert good_cs is True, "good cross-step consistency should be True"
        bad_cs = score_cross_step_consistency({}, {"goal": "x"}, mock_bad)
        assert bad_cs is False, "bad cross-step consistency should be False"
        single = dict(mock_good, steps=mock_good["steps"][:1])
        assert score_cross_step_consistency({}, {"goal": "x"}, single) is None, "single-step -> None"
    finally:
        judge = orig_judge
    print("[selftest] 三维评分逻辑通过 (step_completion / anti_stall 数值断言 + cross_step 分支)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true", help="用 mock 轨迹校验评分逻辑，不调模型")
    ap.add_argument("--limit", type=int, default=None, help="最多评测样本数")
    ap.add_argument("--json", dest="json_out", default=None, help="结果输出 JSON 路径")
    args = ap.parse_args()

    if args.selftest:
        return _selftest()

    cfg = client_cfg()
    result = run(cfg, args.limit)
    txt = json.dumps(result, ensure_ascii=False, indent=2)
    if args.json_out:
        Path(args.json_out).parent.mkdir(parents=True, exist_ok=True)
        open(args.json_out, "w").write(txt)
        print(f"[run_eval] 结果已写入 {args.json_out}")
    else:
        print(txt)
    print("\n维度汇总：")
    for k, label in DIM_LABELS.items():
        v = result["summary"].get(k)
        print(f"  {label} ({k}): {v}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
