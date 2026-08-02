"""cli.py — entry point for the horizontal model-comparison harness.

Commands:
  run     Run the real evaluation across configured models (blocking: waits
          for all results), then writes CSV + HTML reports.
  verify  Offline acceptance test (no network / no API keys needed).
  check   Validate config + sample file without calling any model.

Secrets: any value in config starting with "$" is read from the environment
(e.g. api_key = "$XIAOMI_MIMO_API_KEY").
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

import tomllib

import harness
import report


# --------------------------------------------------------------------------
# helpers
# --------------------------------------------------------------------------


def envsub(s: str) -> str:
    if isinstance(s, str) and s.startswith("$"):
        return os.environ.get(s[1:], "")
    return s


def resolve_path(p: str, start: str) -> str:
    """Resolve `p` relative to `start`, walking up the tree (so a path like
    'benchmark/model-comparison/prompts.jsonl' works from anywhere in repo)."""
    if os.path.isabs(p):
        return p
    cur = start
    for _ in range(8):
        cand = os.path.join(cur, p)
        if os.path.exists(cand):
            return cand
        parent = os.path.dirname(cur)
        if parent == cur:
            break
        cur = parent
    return os.path.join(start, p)


def load_config(path: str) -> dict:
    with open(path, "rb") as f:
        cfg = tomllib.load(f)
    models = []
    for m in cfg.get("models", []):
        models.append(
            {
                "name": m["name"],
                "endpoint": envsub(m.get("endpoint", "")),
                "api_key": envsub(m.get("api_key", "")),
                "model": envsub(m.get("model", "")),
            }
        )
    judge = cfg.get("judge")
    if judge and judge.get("endpoint"):
        judge = {
            "endpoint": envsub(judge.get("endpoint", "")),
            "api_key": envsub(judge.get("api_key", "")),
            "model": envsub(judge.get("model", "")),
        }
    else:
        judge = None
    settings = cfg.get("settings", {})
    return {
        "models": models,
        "judge": judge,
        "settings": settings,
        "prompts_path": settings.get("prompts_path", "benchmark/model-comparison/prompts.jsonl"),
        "output_dir": settings.get("output_dir", "out"),
    }


def load_prompts(path: str) -> list[dict]:
    out = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                out.append(json.loads(line))
    return out


def build_clients(models: list[dict], settings: dict, client_cls=harness.ModelClient):
    timeout = int(settings.get("timeout_seconds", 120))
    stream = bool(settings.get("stream", False))
    clients = []
    for m in models:
        c = dict(m)
        c["_client"] = client_cls(
            name=m["name"],
            endpoint=m["endpoint"],
            api_key=m["api_key"],
            model=m["model"],
            timeout=timeout,
            stream=stream,
        )
        clients.append(c)
    return clients


def print_summary(models: list[str], metrics: dict) -> None:
    print("\n=== 横向对比摘要 ===")
    hdr = f"{'model':>16} | {'lat(s)':>7} | {'tps':>7} | {'qual':>5} | {'ref':>5} | {'judge':>5} | {'self':>5} | {'err':>3}"
    print(hdr)
    for m in models:
        d = metrics.get(m, {})
        print(
            f"{m:>16} | {d.get('avg_latency_s',0):>7.2f} | "
            f"{(d.get('avg_throughput_tps') or 0):>7.1f} | "
            f"{(d.get('quality_heuristic') or 0):>5.2f} | "
            f"{(d.get('ref_accuracy') or 0):>5.2f} | "
            f"{(d.get('quality_judge') or 0):>5.2f} | "
            f"{(d.get('self_consistency') or 0):>5.2f} | "
            f"{d.get('n_errors',0):>3}"
        )


# --------------------------------------------------------------------------
# commands
# --------------------------------------------------------------------------


def cmd_run(args) -> int:
    cfg_path = args.config
    if not os.path.exists(cfg_path):
        print(f"[run] 配置不存在: {cfg_path}\n       请复制 config.example.toml 为 config.toml 并填写。", file=sys.stderr)
        return 2
    cfg = load_config(cfg_path)
    cfg_dir = os.path.dirname(os.path.abspath(cfg_path))
    prompts_path = resolve_path(cfg["prompts_path"], cfg_dir)
    if not os.path.exists(prompts_path):
        print(f"[run] 样本提示词文件不存在: {prompts_path}", file=sys.stderr)
        return 2
    prompts = load_prompts(prompts_path)
    clients = build_clients(cfg["models"], cfg["settings"])
    judge_client = None
    if cfg["judge"]:
        judge_client = harness.ModelClient(
            name="judge",
            endpoint=cfg["judge"]["endpoint"],
            api_key=cfg["judge"]["api_key"],
            model=cfg["judge"]["model"],
            timeout=int(cfg["settings"].get("timeout_seconds", 120)),
        )

    repeat = int(cfg["settings"].get("repeat", 1))
    print(
        f"[run] 模型={len(clients)} 提示词={len(prompts)} 重复={repeat} "
        f"样本={prompts_path}\n       开始评测（将等待全部结果）…"
    )
    results = harness.run_all(
        clients,
        prompts,
        repeat=repeat,
        timeout=int(cfg["settings"].get("timeout_seconds", 120)),
        stream=bool(cfg["settings"].get("stream", False)),
    )
    metrics = harness.compute_model_metrics(results, prompts, judge_client)
    cross = harness.compute_cross_model(results, prompts)

    out_dir = os.path.join(cfg_dir, cfg["output_dir"])
    os.makedirs(out_dir, exist_ok=True)
    summary_csv = os.path.join(out_dir, "comparison_summary.csv")
    runs_csv = os.path.join(out_dir, "comparison_runs.csv")
    html_path = os.path.join(out_dir, "comparison_report.html")
    report.write_csv_summary(summary_csv, [m["name"] for m in cfg["models"]], metrics)
    report.write_csv_runs(runs_csv, results)
    report.write_html(html_path, [m["name"] for m in cfg["models"]], metrics, results, prompts, cross)

    print_summary([m["name"] for m in cfg["models"]], metrics)
    print(f"\n[run] 完成。输出：\n  - {summary_csv}\n  - {runs_csv}\n  - {html_path}")
    return 0


def cmd_check(args) -> int:
    cfg_path = args.config
    if not os.path.exists(cfg_path):
        print(f"[check] 配置不存在: {cfg_path}", file=sys.stderr)
        return 2
    cfg = load_config(cfg_path)
    cfg_dir = os.path.dirname(os.path.abspath(cfg_path))
    ok = True
    if not cfg["models"]:
        print("[check] 未配置任何 [[models]] 项。")
        ok = False
    for m in cfg["models"]:
        miss = [k for k in ("endpoint", "api_key", "model") if not m.get(k)]
        if miss:
            print(f"[check] 模型 '{m['name']}' 缺少: {', '.join(miss)}")
            ok = False
        elif m["api_key"].startswith("$") and not os.environ.get(m["api_key"][1:]):
            print(f"[check] 模型 '{m['name']}' 的环境变量 {m['api_key']} 未设置")
            ok = False
    pp = resolve_path(cfg["prompts_path"], cfg_dir)
    if not os.path.exists(pp):
        print(f"[check] 样本提示词文件不存在: {pp}")
        ok = False
    else:
        n = len(load_prompts(pp))
        print(f"[check] 样本提示词: {pp} （{n} 条）")
    if cfg["judge"]:
        print(f"[check] 裁判模型已配置: {cfg['judge']['model']} @ {cfg['judge']['endpoint']}")
    print("[check]", "就绪 ✓" if ok else "存在问题 ✗")
    return 0 if ok else 1


def cmd_verify(args) -> int:
    """Offline acceptance: synthetic clients, tiny sample, assert outputs."""
    prompts = [
        {"id": "v1", "category": "math", "type": "short",
         "prompt": "What is 2+2? Reply with the number only.", "reference": "4"},
        {"id": "v2", "category": "classify", "type": "classify",
         "prompt": "Is the sky blue? Answer YES or NO.", "reference": "YES"},
    ]
    models = [
        {"name": "mock-a", "endpoint": "http://mock", "api_key": "x", "model": "m"},
        {"name": "mock-b", "endpoint": "http://mock", "api_key": "x", "model": "m"},
    ]
    clients = build_clients(models, {"repeat": 2}, client_cls=harness.MockClient)
    results = harness.run_all(clients, prompts, repeat=2, timeout=10, stream=False, verbose=False)
    metrics = harness.compute_model_metrics(results, prompts)
    cross = harness.compute_cross_model(results, prompts)

    out = tempfile.mkdtemp(prefix="mimofan_verify_")
    summary_csv = os.path.join(out, "comparison_summary.csv")
    runs_csv = os.path.join(out, "comparison_runs.csv")
    html_path = os.path.join(out, "comparison_report.html")
    report.write_csv_summary(summary_csv, [m["name"] for m in models], metrics)
    report.write_csv_runs(runs_csv, results)
    report.write_html(html_path, [m["name"] for m in models], metrics, results, prompts, cross)

    # assertions
    checks = []
    checks.append(("no run errors", all(not r.error for r in results)))
    checks.append(("summary csv exists", os.path.exists(summary_csv)))
    checks.append(("runs csv exists", os.path.exists(runs_csv)))
    checks.append(("html exists", os.path.exists(html_path)))
    with open(summary_csv, encoding="utf-8") as f:
        content = f.read()
    checks.append(("csv has model headers", "mock-a" in content and "mock-b" in content))
    with open(html_path, encoding="utf-8") as f:
        html_text = f.read()
    checks.append(("html has title", "模型横向对比报告" in html_text))
    checks.append(("html has metric table", "平均延迟" in html_text))
    checks.append(("metrics computed", "avg_latency_s" in metrics["mock-a"]))

    passed = all(ok for _, ok in checks)
    print("=== verify (acceptance) ===")
    for name, ok in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}")
    print(f"输出目录: {out}")
    print("RESULT:", "PASS ✓" if passed else "FAIL ✗")
    return 0 if passed else 1


# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------


def main(argv=None) -> int:
    here = Path(__file__).resolve().parent
    ap = argparse.ArgumentParser(description="mimofan 横向模型对比 harness")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_run = sub.add_parser("run", help="运行真实评测并生成报告")
    p_run.add_argument("--config", default=str(here / "config.toml"))
    p_run.set_defaults(func=cmd_run)

    p_check = sub.add_parser("check", help="校验配置与样本（不调用模型）")
    p_check.add_argument("--config", default=str(here / "config.toml"))
    p_check.set_defaults(func=cmd_check)

    p_verify = sub.add_parser("verify", help="离线验收测试（无需密钥）")
    p_verify.set_defaults(func=cmd_verify)

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
