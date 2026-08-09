#!/usr/bin/env python3
"""capability_probe.py — 静态能力矩阵评分器（A 类，60 分）。

按 samples/capability_matrix.json 定义的探针，在 mimofan 源码中 grep 验证符号存在性，
输出每个能力域的得分与总分。

用法:
    python3 benchmark/agentbench/capability_probe.py [--repo PATH] [--json OUT]

设计约束（见 EVAL_METRICS.md 反作弊约束）:
  - 探针命中 = 该能力"存在"，系数 1.0
  - 探针部分命中（require_all 时只中一部分）= 系数 0.5
  - 未命中 = 0
纯标准库，无第三方依赖。
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_MATRIX = Path(__file__).parent / "samples" / "capability_matrix.json"


def grep_exists(repo: Path, pattern: str, targets: list[str]) -> bool:
    """在给定文件/目录中搜索正则，命中返回 True。"""
    existing = [str(repo / t) for t in targets if (repo / t).exists()]
    if not existing:
        return False
    try:
        proc = subprocess.run(
            ["grep", "-rEl", "--", pattern, *existing],
            capture_output=True,
            text=True,
            timeout=60,
        )
        return proc.returncode == 0 and bool(proc.stdout.strip())
    except (subprocess.TimeoutExpired, OSError):
        return False


def run_probe(repo: Path, probe: dict) -> tuple[float, list[str]]:
    """执行单个探针，返回 (系数 0/0.5/1.0, 命中的 pattern 列表)。"""
    patterns: list[str] = probe["patterns"]
    files: list[str] = probe["files"]
    require_all: bool = probe.get("require_all", False)

    hits = [p for p in patterns if grep_exists(repo, p, files)]

    if not hits:
        return 0.0, []
    if require_all:
        return (1.0, hits) if len(hits) == len(patterns) else (0.5, hits)
    return 1.0, hits


def score_matrix(repo: Path, matrix: dict) -> dict:
    domains_out = []
    total_score = 0.0
    total_weight = 0.0

    for domain in matrix["domains"]:
        probes = domain["probes"]
        weight = float(domain["weight"])
        per_probe_weight = weight / len(probes) if probes else 0.0

        probe_results = []
        domain_score = 0.0
        for probe in probes:
            coeff, hits = run_probe(repo, probe)
            earned = per_probe_weight * coeff
            domain_score += earned
            probe_results.append(
                {
                    "id": probe["id"],
                    "desc": probe["desc"],
                    "coefficient": coeff,
                    "earned": round(earned, 3),
                    "max": round(per_probe_weight, 3),
                    "matched_patterns": hits,
                }
            )

        total_score += domain_score
        total_weight += weight
        domains_out.append(
            {
                "id": domain["id"],
                "name": domain["name"],
                "score": round(domain_score, 2),
                "weight": weight,
                "percent": round(domain_score / weight * 100, 1) if weight else 0.0,
                "probes": probe_results,
            }
        )

    return {
        "category": "A_static_capability",
        "total_score": round(total_score, 2),
        "total_weight": total_weight,
        "percent": round(total_score / total_weight * 100, 1) if total_weight else 0.0,
        "domains": domains_out,
    }


def print_report(result: dict) -> None:
    print("=" * 72)
    print("  静态能力矩阵评分（A 类）")
    print("=" * 72)
    for d in result["domains"]:
        bar_len = int(d["percent"] / 5)
        bar = "#" * bar_len + "." * (20 - bar_len)
        print(f"{d['id']:>4} {d['name']:<18} [{bar}] {d['score']:>5.2f}/{d['weight']:<4.0f} ({d['percent']:>5.1f}%)")
        for p in d["probes"]:
            mark = "x" if p["coefficient"] == 1.0 else ("~" if p["coefficient"] == 0.5 else " ")
            print(f"       [{mark}] {p['id']:<6} {p['desc']}")
    print("-" * 72)
    print(f"  A 类总分: {result['total_score']:.2f} / {result['total_weight']:.0f}  ({result['percent']:.1f}%)")
    print("=" * 72)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=str(Path(__file__).resolve().parents[2]))
    ap.add_argument("--matrix", default=str(DEFAULT_MATRIX))
    ap.add_argument("--json", dest="json_out", default=None)
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    repo = Path(args.repo).resolve()
    matrix = json.loads(Path(args.matrix).read_text(encoding="utf-8"))
    result = score_matrix(repo, matrix)

    if not args.quiet:
        print_report(result)
    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"\n已写入: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
