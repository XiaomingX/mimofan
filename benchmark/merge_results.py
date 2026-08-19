#!/usr/bin/env python3
"""merge_results.py — 聚合各 harness 输出到统一 summary.json，并打时间戳快照。

输入：
  - benchmark/results/harness/*.json    各评分 harness 输出（run_all.py 转存）
  - benchmark/results/vuln_hunt/*.json  vuln-hunt 每个 task 的记分卡（evaluate.py 用 --results-root 写出）

输出：
  - benchmark/results/summary.json      本次统一汇总（最新）
  - benchmark/results/snapshots/<ts>.json  summary 时间戳快照（趋势对比用）

summary.json schema：
{
  "version": 1,
  "timestamp": "...",
  "mode": "full",
  "harnesses": { "<name>": {"percent": float|null, "total_score": float|null,
                             "source": "<file>", "status": "ok|no-data|failed"} },
  "by_category": { "<category>": {"scored": n, "total": n, "percent": float|null} },
  "by_l_level":  { "L0": {"scored": n, "total": n, "percent": float|null} },
  "coverage": {"scored": n, "total": n, "percent": float|null}
}

设计原则：聚合层，不改任何 harness 输出格式。提取逻辑健壮（优先 percent，其次
total_score/mean），对缺失或失败项标记 no-data，不虚构分数。
"""
from __future__ import annotations

import json
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RESULTS = ROOT / "results"
HARNESS_OUT = RESULTS / "harness"
VULN_OUT = RESULTS / "vuln_hunt"
SNAP_DIR = RESULTS / "snapshots"
REGISTRY = ROOT / "sample_registry.json"

# harness 输出 key → 该 harness 主要覆盖的样本 category（用于 by_category 聚合）。
# 这是近似归属，供覆盖度展示；harnesses 层展示的是实际提取的精确分数。
HARNESS_CATEGORY = {
    "mece_static": "static-capability",
    "dynamic_bench": "dynamic-metric",
    "longmemeval": "long-term-memory",
    "long_horizon": "long-horizon",
    "rd_exec_eval": "long-horizon-exec",
    "lineage": "lineage",
    "vuln_hunt": "vuln-hunt",
    "p0_dynamic": "p0-e2e",
}

# category → 代表 L（与 sample_registry 的 by_l_level 口径对齐，近似）
CATEGORY_L = {
    "static-capability": 1,
    "dynamic-metric": 1,
    "long-term-memory": 3,
    "long-horizon": 4,
    "long-horizon-exec": 4,
    "rd-automation": 4,
    "lineage": 5,
    "vuln-hunt": 4,
    "p0-e2e": 3,
}


def _extract_percent(payload: dict) -> float | None:
    """从一个 harness 输出 dict 中提取 0-100 百分比。优先 percent，其次 total_score/mean。"""
    if not isinstance(payload, dict):
        return None
    # 1) 显式 percent（0-100）
    pct = payload.get("percent")
    if isinstance(pct, (int, float)):
        return round(float(pct), 1)
    # 2) total_score：若分母是 100 则即为百分比；否则按 0-1 归一
    ts = payload.get("total_score")
    if isinstance(ts, (int, float)):
        ts = float(ts)
        if ts <= 1.0 and payload.get("mean") is None:
            # 可能是 0-1 归一分数（如某些子维度），保守转成百分比
            return round(ts * 100, 1)
        return round(ts, 1)
    # 3) mean（vuln-hunt 三维平均，0-1）
    mean = payload.get("mean")
    if isinstance(mean, (int, float)):
        return round(float(mean) * 100, 1)
    return None


def _load_registry():
    if REGISTRY.exists():
        try:
            return json.loads(REGISTRY.read_text(encoding="utf-8"))
        except Exception:
            pass
    return None


def _collect_vuln_hunt() -> dict:
    """vuln-hunt 结果在 results/vuln_hunt/<task_id>.json（evaluate.py --results-root 写出），聚合 mean。"""
    payload = {}
    percents = []
    files = sorted(VULN_OUT.glob("*.json")) if VULN_OUT.exists() else []
    for f in files:
        try:
            d = json.loads(f.read_text(encoding="utf-8"))
        except Exception:
            continue
        mean = d.get("mean")
        if isinstance(mean, (int, float)):
            percents.append(float(mean) * 100)
    if percents:
        payload["percent"] = round(sum(percents) / len(percents), 1)
        payload["total_score"] = payload["percent"]
        payload["n_tasks"] = len(percents)
        payload["status"] = "ok"
        payload["source"] = "results/vuln_hunt/*.json"
    else:
        payload["percent"] = None
        payload["status"] = "no-data"
        payload["source"] = "results/vuln_hunt/（无 task 结果）"
    return payload


def _collect_harness_files(mode: str) -> dict:
    harnesses = {}
    if HARNESS_OUT.exists():
        for f in sorted(HARNESS_OUT.glob("*.json")):
            name = f.stem
            try:
                payload = json.loads(f.read_text(encoding="utf-8"))
            except Exception:
                harnesses[name] = {"percent": None, "status": "failed", "source": str(f)}
                continue
            pct = _extract_percent(payload)
            harnesses[name] = {
                "percent": pct,
                "total_score": payload.get("total_score") if isinstance(payload, dict) else None,
                "status": "ok" if pct is not None else "no-data",
                "source": str(f.relative_to(ROOT)),
            }
    # vuln-hunt 单独（evaluate.py 不写 harness/<name>.json，写 results/vuln_hunt/*.json）
    harnesses["vuln_hunt"] = _collect_vuln_hunt()
    return harnesses


def _aggregate(harnesses: dict) -> dict:
    """按 category / L 聚合覆盖度。基于 HARNESS_CATEGORY 归属 + sample_registry 的总样本分布。"""
    reg = _load_registry()
    # 每类总样本数取自 sample_registry.summary.by_category（权威口径）
    by_cat_total = {}
    by_l_total = {}
    if reg:
        s = reg.get("summary", {})
        by_cat_total = s.get("by_category", {})
        by_l_total = {k: v for k, v in s.get("by_l_level", {}).items() if k.startswith("L")}

    by_category = {}
    by_l_level = {}
    scored_cats = set()
    for hname, h in harnesses.items():
        cat = HARNESS_CATEGORY.get(hname)
        if not cat or h.get("percent") is None:
            continue
        scored_cats.add(cat)
        by_category[cat] = {"scored": 1, "total": by_cat_total.get(cat, 1), "percent": h["percent"]}
        lvl = CATEGORY_L.get(cat)
        if lvl is not None:
            key = f"L{lvl}"
            entry = by_l_level.setdefault(key, {"scored": 0, "total": by_l_total.get(key, 1), "percent": 0.0})
            entry["scored"] += 1
            entry["percent"] = entry["percent"] or h["percent"]

    scored_n = len(scored_cats)
    total_n = len(by_cat_total) if by_cat_total else scored_n
    coverage = {
        "scored": scored_n,
        "total": total_n,
        "percent": round(scored_n / total_n * 100, 1) if total_n else 0.0,
    }
    return by_category, by_l_level, coverage


def main() -> int:
    mode = "live"
    import sys
    if "--mode" in sys.argv:
        mode = sys.argv[sys.argv.index("--mode") + 1]

    harnesses = _collect_harness_files(mode)
    by_category, by_l_level, coverage = _aggregate(harnesses)

    summary = {
        "version": 1,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "mode": mode,
        "harnesses": harnesses,
        "by_category": by_category,
        "by_l_level": by_l_level,
        "coverage": coverage,
    }

    RESULTS.mkdir(parents=True, exist_ok=True)
    out = RESULTS / "summary.json"
    out.write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"[merge] 写入 {out}")
    print(f"  覆盖 {coverage['scored']}/{coverage['total']} 类 ({coverage['percent']}%)")

    # 时间戳快照（git 不跟踪 snapshots/，避免噪音）
    SNAP_DIR.mkdir(parents=True, exist_ok=True)
    ts = time.strftime("%Y%m%d_%H%M%S")
    snap = SNAP_DIR / f"summary_{ts}.json"
    snap.write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"[snapshot] {snap}")

    # 打印各 harness 分数
    print("\n  harness 分数：")
    for hname, h in harnesses.items():
        pct = h.get("percent")
        print(f"    {hname:<16} {'ok' if pct is not None else 'no-data':<8} {pct if pct is not None else '-'}")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
