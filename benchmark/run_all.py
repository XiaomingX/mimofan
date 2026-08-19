#!/usr/bin/env python3
"""run_all.py — mimofan 评测/数据闭环统一入口。

把分散的发布前验收 shell 脚本、各子目录评分 harness、横向模型对比孤岛，
收敛为单一入口驱动，输出统一汇总到 benchmark/results/。

用法：
    python3 benchmark/run_all.py --fast    # accept + harness --selftest + model-cmp verify
    python3 benchmark/run_all.py --full    # 默认：accept + harness --limit/--skip-exec（不触真模型付费）
    python3 benchmark/run_all.py --live    # full + 真模型 harness 全量 + model-cmp run
    python3 benchmark/run_all.py --group accept|harness|model-cmp   # 只跑一组

设计原则：新增聚合层，不改动任何现有脚本内部逻辑（低风险）。
每个子进程失败不阻断其余，结尾打印失败清单并返回非 0。
"""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent          # benchmark/
PROJECT = ROOT.parent                            # 仓库根
RESULTS = ROOT / "results"
HARNESS_OUT = RESULTS / "harness"

# 顶层发布前验收 shell 脚本（测 API/CLI，不调用任何 harness）——归 accept 组。
ACCEPT_SCRIPTS = [
    "api_providers_test.sh",
    "capability_tests.sh",
    "cli_commands_test.sh",
    "commands_test.sh",
    "fleet_test.sh",
    "tools_coverage_test.sh",
    "tools_extended_test.sh",
    "tools_test.sh",
    "tui_commands_test.sh",
    "run_observability_bench.sh",
]

# 评分 harness：mode=python 时用 sys.executable 跑；每个含 full 档（真模型采样/离线静态）与 live 档（真模型全量）。
# 字段：name=输出 key；cmd=live 档命令模板；full_cmd=full 档；fast 档统一走 --selftest（支持的）或跳过。
# {out} 会被替换为 results/harness/<name>.json。
HARNESSES = [
    {
        "name": "mece_static",
        "script": "agentbench/mece_bench.py",
        "fast": None,                                   # 无 selftest，fast 跳过
        "full": ["--skip-exec", "--json", "{out}"],     # 离线静态，不触真模型
        "live": ["--json", "{out}"],
        "needs": [],
    },
    {
        "name": "dynamic_bench",
        "script": "agentbench/dynamic_bench.py",
        "fast": None,
        "full": ["--skip-build", "--json", "{out}"],    # 跳过 B1/B2/B3（cargo 耗时）
        "live": ["--json", "{out}"],
        "needs": [],
    },
    {
        "name": "p0_dynamic",
        "script": "agentbench/p0_dynamic.py",
        "fast": None,
        "full": ["--skip-mem", "--json", "{out}"],      # 省 token，仍触真模型（少量）
        "live": ["--json", "{out}"],
        "needs": ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"],
    },
    {
        "name": "longmemeval",
        "script": "agentbench/longmemeval_harness.py",
        "fast": None,                                   # 需 --binary，fast 跳过
        "full": ["--json", "{out}", "--limit", "5"],
        "live": ["--json", "{out}"],
        "needs": ["--binary"],                          # 需传入 mimofan 二进制，full/live 均需
    },
    {
        "name": "long_horizon",
        "script": "long_horizon/run_eval.py",
        "fast": ["--selftest"],
        "full": ["--limit", "5", "--json", "{out}"],
        "live": ["--json", "{out}"],
        "needs": ["ANTHROPIC_API_KEY"],
    },
    {
        "name": "rd_exec_eval",
        "script": "long_horizon/rd_exec_eval.py",
        "fast": ["--selftest"],
        "full": ["--limit", "5", "--json", "{out}"],
        "live": ["--json", "{out}"],
        "needs": ["ANTHROPIC_API_KEY"],
    },
    {
        "name": "lineage",
        "script": "lineage/run_eval.py",
        "fast": ["--selftest"],
        "full": ["--limit", "2", "--json", "{out}"],
        "live": ["--json", "{out}"],
        "needs": ["ANTHROPIC_API_KEY"],
    },
    {
        "name": "vuln_hunt",
        "script": "vuln_hunt/evaluate.py",
        "fast": ["--selftest"],
        "full": ["--results-root", str(RESULTS / "vuln_hunt")],
        "live": ["--results-root", str(RESULTS / "vuln_hunt")],
        "needs": [],
    },
]

# 横向模型对比（evals/ 孤岛）——mode=subcmd，fast 用 verify（离线），live 用 run（真模型）。
MODEL_CMP = {
    "script": "evals/cli.py",
    "fast": ["verify"],
    "full": ["verify"],
    "live": ["run", "--config", "evals/config.toml"],
}


def _python() -> str:
    return sys.executable or "python3"


def _run(cmd: list[str], timeout: int, cwd: Path, name: str) -> dict:
    """跑一个子进程，容错（check=False），返回状态。"""
    status = {"name": name, "cmd": " ".join(str(c) for c in cmd), "status": "ok"}
    try:
        proc = subprocess.run(cmd, cwd=str(cwd), capture_output=True, text=True, timeout=timeout)
        status["rc"] = proc.returncode
        if proc.returncode != 0:
            status["status"] = "failed"
            status["stderr"] = proc.stderr[-2000:] if proc.stderr else ""
    except subprocess.TimeoutExpired:
        status["status"] = "failed"
        status["rc"] = "timeout"
        status["stderr"] = f"timeout after {timeout}s"
    except FileNotFoundError as e:
        status["status"] = "failed"
        status["rc"] = "notfound"
        status["stderr"] = str(e)
    return status


def _mk_out(name: str) -> str:
    HARNESS_OUT.mkdir(parents=True, exist_ok=True)
    return str(HARNESS_OUT / f"{name}.json")


def run_accept(mode: str) -> list[dict]:
    print("\n== accept 组：发布前验收 shell ==")
    results = []
    for script in ACCEPT_SCRIPTS:
        p = ROOT / script
        if not p.exists():
            results.append({"name": script, "status": "skipped", "rc": "missing"})
            print(f"  [skip] {script}: 不存在")
            continue
        print(f"  [run ] {script}", flush=True)
        results.append(_run(["bash", str(p)], timeout=600, cwd=ROOT, name=script))
    return results


def run_harness(mode: str) -> list[dict]:
    print(f"\n== harness 组：评分 harness（mode={mode}）==")
    results = []
    for h in HARNESSES:
        name = h["name"]
        # 模式对应的命令模板：fast/full/live 逐级选择，fallback 到更低档
        if mode == "live" and h["live"] is not None:
            tpl = h["live"]
        elif mode == "full" and h["full"] is not None:
            tpl = h["full"]
        else:
            tpl = h["fast"]
        if tpl is None:
            results.append({"name": name, "status": "skipped", "rc": "no-mode"})
            print(f"  [skip] {name}: {mode} 档无命令")
            continue
        script = str(ROOT / h["script"])
        out = _mk_out(name)
        args = [t.replace("{out}", out) for t in tpl]
        cmd = [_python(), script] + args
        # vuln_hunt 没有 --json 参数，用 --results-root；其余 harness 若带 {out} 走 --json
        print(f"  [run ] {name}", flush=True)
        results.append(_run(cmd, timeout=1800, cwd=PROJECT, name=name))
    return results


def run_model_cmp(mode: str) -> list[dict]:
    print(f"\n== model-cmp 组：横向模型对比（mode={mode}）==")
    if mode == "live":
        sub = MODEL_CMP["live"]
    else:
        sub = MODEL_CMP["fast"]  # verify 离线 / full 也走 verify 不触真模型
    script = str(PROJECT / MODEL_CMP["script"])
    cmd = [_python(), script] + sub
    print(f"  [run ] evals/cli.py {' '.join(sub)}", flush=True)
    return [_run(cmd, timeout=1800, cwd=PROJECT, name="model_cmp")]


def merge(mode: str) -> None:
    """调 merge_results.py 聚合各 harness 输出到 summary.json。"""
    merge_script = ROOT / "merge_results.py"
    if not merge_script.exists():
        print("\n[merge] 跳过：merge_results.py 尚未创建")
        return
    subprocess.run([_python(), str(merge_script), "--mode", mode], cwd=str(ROOT))


def main() -> int:
    ap = argparse.ArgumentParser(description="mimofan 评测/数据闭环统一入口")
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--fast", action="store_true", help="accept + harness selftest + model-cmp verify")
    g.add_argument("--live", action="store_true", help="真模型全量")
    ap.add_argument("--group", choices=["accept", "harness", "model-cmp"], help="只跑一组")
    ap.add_argument("--timeout", type=int, default=1800, help="单 harness 超时（秒）")
    args = ap.parse_args()

    mode = "live" if args.live else ("fast" if args.fast else "full")

    t0 = time.time()
    all_results = []

    groups_to_run = [args.group] if args.group else ["accept", "harness", "model-cmp"]

    for grp in groups_to_run:
        if grp == "accept":
            all_results += run_accept(mode)
        elif grp == "harness":
            all_results += run_harness(mode)
        elif grp == "model-cmp":
            all_results += run_model_cmp(mode)

    # 汇总
    ok = [r for r in all_results if r.get("status") == "ok"]
    failed = [r for r in all_results if r.get("status") == "failed"]
    skipped = [r for r in all_results if r.get("status") == "skipped"]

    print(f"\n=== 汇总：ok={len(ok)} failed={len(failed)} skipped={len(skipped)} 耗时={time.time()-t0:.0f}s ===")
    for r in failed:
        print(f"  [FAIL] {r['name']} rc={r.get('rc')}")
        if r.get("stderr"):
            print(f"         {r['stderr'][-400:]}")
    if not args.group:
        merge(mode)

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
