#!/usr/bin/env python3
"""coverage_report.py — 生成 benchmark 测试覆盖率报告（MECE 评测视角）。

本脚本复用 mece_bench.py 的评分结果，从「覆盖率」角度汇总 mimofan 能力评测对
代码面的覆盖情况，产出可复用的结构化报告，便于：
  - 持续追踪每个能力域 / 簇 / 视角 / tier 的覆盖与通过率
  - 识别覆盖薄弱点（视角缺失、T3 执行占比低、某簇通过率塌陷）
  - 跨 commit / 跨 worktree 复用同一报告口径做前后对比

覆盖率口径（four dimensions）：
  1. 域覆盖   —— 12 个 MECE 域是否都有条目、各自命中率
  2. 簇覆盖   —— 每个域下能力簇是否都有条目覆盖（不要求每簇满配额，但要求非零）
  3. 视角覆盖 —— existence / depth / negative / integration 四视角是否都在簇内出现
  4. tier 覆盖 —— T1 静态 / T2 结构 / T3 执行 三层判定占比与各自通过率

用法：
  # 直接跑评分并生成报告（会触发 cargo 编译 T3）
  python3 benchmark/agentbench/coverage_report.py --json results/coverage.json

  # 复用已有 mece_bench 产物，避免重复编译
  python3 benchmark/agentbench/coverage_report.py --from-json results/mece.json --json results/coverage.json

  # 只输出 Markdown 到终端
  python3 benchmark/agentbench/coverage_report.py --skip-exec --md

环境变量：无（--skip-exec 时不调真模型 / cargo test）。
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

# 复用 mece_bench 的评分与常量
from mece_bench import score, DEFAULT_ENTRIES_DIR, DOMAIN_NAME, DOMAIN_QUOTA, STRONG_THRESHOLD, PASS_THRESHOLD  # noqa: E402

VIEWS = ["existence", "depth", "negative", "integration"]
TIERS = ["T1", "T2", "T3"]


def build_coverage(result: dict) -> dict:
    """从 mece_bench 的 result 计算四维度覆盖率。"""
    results = result.get("entries", result.get("results", []))
    summary = result

    # 域维度
    domain_stats: dict[str, dict] = defaultdict(lambda: {
        "n_total": 0, "n_passed": 0,
        "clusters": set(), "views": set(),
        "tier": {"T1": [0, 0], "T2": [0, 0], "T3": [0, 0]},  # [total, passed]
    })
    for r in results:
        d = domain_stats[r["domain"]]
        d["n_total"] += 1
        d["clusters"].add(r["cluster"])
        d["views"].add(r["view"])
        if r["passed"]:
            d["n_passed"] += 1
        t = d["tier"][r["tier"]]
        t[0] += 1
        if r["passed"]:
            t[1] += 1

    domains_out = []
    for did in sorted(DOMAIN_QUOTA.keys()):
        d = domain_stats.get(did)
        if d is None:
            domains_out.append({
                "id": did, "name": DOMAIN_NAME.get(did, did), "quota": DOMAIN_QUOTA[did],
                "n_total": 0, "n_passed": 0, "pass_ratio": 0.0,
                "clusters_covered": 0, "views_covered": VIEWS,
                "tier_pass_ratio": {t: None for t in TIERS},
                "note": "无条目",
            })
            continue
        pr = d["n_passed"] / d["n_total"] if d["n_total"] else 0.0
        tier_pr = {}
        for t in TIERS:
            tot, pas = d["tier"][t]
            tier_pr[t] = round(pas / tot, 3) if tot else None
        domains_out.append({
            "id": did, "name": DOMAIN_NAME.get(did, did), "quota": DOMAIN_QUOTA[did],
            "n_total": d["n_total"], "n_passed": d["n_passed"],
            "pass_ratio": round(pr, 3),
            "clusters_covered": len(d["clusters"]),
            "views_covered": sorted(d["views"]),
            "tier_pass_ratio": tier_pr,
        })

    # 视角覆盖（全集合）
    view_totals = {v: [0, 0] for v in VIEWS}
    for r in results:
        if r["view"] in view_totals:
            view_totals[r["view"]][0] += 1
            if r["passed"]:
                view_totals[r["view"]][1] += 1
    view_coverage = {
        v: {"n_total": tot, "n_passed": pas,
            "pass_ratio": round(pas / tot, 3) if tot else 0.0}
        for v, (tot, pas) in view_totals.items()
    }

    # tier 覆盖（全集合）
    tier_totals = {t: [0, 0] for t in TIERS}
    for r in results:
        if r["tier"] in tier_totals:
            tier_totals[r["tier"]][0] += 1
            if r["passed"]:
                tier_totals[r["tier"]][1] += 1
    tier_coverage = {
        t: {"n_total": tot, "n_passed": pas,
            "pass_ratio": round(pas / tot, 3) if tot else 0.0,
            "share": round(tot / len(results), 3) if results else 0.0}
        for t, (tot, pas) in tier_totals.items()
    }

    # 弱点识别
    weak_domains = [d["id"] for d in domains_out if d["pass_ratio"] < PASS_THRESHOLD]
    missing_view_domains = [
        d["id"] for d in domains_out
        if d.get("n_total", 0) > 0 and len(d["views_covered"]) < len(VIEWS)
    ]

    return {
        "total_score": summary.get("total_score"),
        "n_entries": summary.get("n_entries"),
        "n_passed": summary.get("n_passed"),
        "domains": domains_out,
        "view_coverage": view_coverage,
        "tier_coverage": tier_coverage,
        "weak_domains": weak_domains,
        "missing_view_domains": missing_view_domains,
        "strong_threshold": STRONG_THRESHOLD,
        "pass_threshold": PASS_THRESHOLD,
    }


def render_markdown(cov: dict) -> str:
    lines = []
    lines.append("# Benchmark 测试覆盖率报告\n")
    lines.append(f"- 总分: **{cov['total_score']:.2f} / 100**")
    lines.append(f"- 条目: {cov['n_passed']}/{cov['n_entries']} 通过")
    lines.append(f"- 强项阈值 ≥ {cov['strong_threshold']} | 达标阈值 ≥ {cov['pass_threshold']}\n")

    lines.append("## 一、域覆盖（domain coverage）\n")
    lines.append("| 域 | 名称 | 配额 | 条目 | 通过率 | 簇覆盖 | 视角覆盖 | T3通过率 |")
    lines.append("|---|---|---|---|---|---|---|---|")
    for d in cov["domains"]:
        t3 = d["tier_pass_ratio"].get("T3")
        lines.append(
            f"| {d['id']} | {d['name']} | {d['quota']} | {d['n_passed']}/{d['n_total']} "
            f"({d['pass_ratio']:.0%}) | {d['clusters_covered']} | "
            f"{len(d['views_covered'])}/{len(VIEWS)} | {('%.0f%%' % (t3*100)) if t3 is not None else '—'} |"
        )

    lines.append("\n## 二、视角覆盖（four-view coverage）\n")
    lines.append("| 视角 | 条目 | 通过率 |")
    lines.append("|---|---|---|")
    for v, c in cov["view_coverage"].items():
        lines.append(f"| {v} | {c['n_passed']}/{c['n_total']} | {c['pass_ratio']:.0%} |")

    lines.append("\n## 三、Tier 覆盖（judgment-layer coverage）\n")
    lines.append("| Tier | 条目 | 占比 | 通过率 |")
    lines.append("|---|---|---|---|")
    for t, c in cov["tier_coverage"].items():
        lines.append(f"| {t} | {c['n_passed']}/{c['n_total']} | {c['share']:.0%} | {c['pass_ratio']:.0%} |")

    lines.append("\n## 四、薄弱点\n")
    lines.append(f"- 未达标域 (<{cov['pass_threshold']}): {cov['weak_domains'] or '无'}")
    lines.append(f"- 视角缺失域: {cov['missing_view_domains'] or '无'}")

    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description="生成 benchmark 测试覆盖率报告")
    ap.add_argument("--repo", default=str(Path(__file__).resolve().parents[2]))
    ap.add_argument("--entries", default=str(DEFAULT_ENTRIES_DIR))
    ap.add_argument("--from-json", default=None, help="复用已有 mece_bench --json 产物，跳过重跑")
    ap.add_argument("--json", dest="json_out", default=None, help="覆盖率报告 JSON 输出")
    ap.add_argument("--md", dest="md_out", default=None, help="覆盖率报告 Markdown 输出")
    ap.add_argument("--skip-exec", action="store_true", help="跳过 T3 执行（配合直接跑时使用）")
    ap.add_argument("--target-dir", default=None, help="注入 CARGO_TARGET_DIR")
    args = ap.parse_args()

    if args.from_json:
        result = json.loads(Path(args.from_json).read_text(encoding="utf-8"))
        print(f"[coverage] 复用 {args.from_json}")
    else:
        repo = Path(args.repo).resolve()
        entries = __import__("mece_bench").load_entries(Path(args.entries))
        result = score(entries, repo, args.target_dir, args.skip_exec)
        print("[coverage] 已跑 mece_bench 评分")

    cov = build_coverage(result)

    if args.json_out:
        Path(args.json_out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.json_out).write_text(json.dumps(cov, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"[coverage] JSON 报告: {args.json_out}")
    if args.md_out:
        Path(args.md_out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.md_out).write_text(render_markdown(cov), encoding="utf-8")
        print(f"[coverage] Markdown 报告: {args.md_out}")

    # 终端始终打印 Markdown 摘要
    print()
    print(render_markdown(cov))
    return 0


if __name__ == "__main__":
    sys.exit(main())
