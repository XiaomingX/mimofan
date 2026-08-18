#!/usr/bin/env python3
"""build_sample_registry.py — 扫描 benchmark/ 样本，生成带 L 分级的注册表。

产出 benchmark/sample_registry.json：
  {
    "version": 1,
    "taxonomy": "benchmark/L_TAXONOMY.md",
    "samples": [
      {"path": "...", "category": "...", "l_level": N, "count": K, "note": "..."},
      ...
    ]
  }

分级口径见 benchmark/L_TAXONOMY.md。本脚本内置「文件 → (category, L)」映射，
对 mece_1000 按 part 文件估算 L0/L1/L2 占比（基于 tier 统计，可选）。

运行：
    python3 benchmark/build_sample_registry.py
"""
from __future__ import annotations

import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def _mece_part_l(path: Path) -> int:
    """mece part 文件按 tier 分布给一个代表 L（取最高占比的 tier 对应 L）。"""
    try:
        data = json.load(open(path))
    except Exception:
        return 0
    tiers = [e.get("tier") for e in data if isinstance(e, dict)]
    if not tiers:
        return 0
    from collections import Counter
    c = Counter(tiers)
    top = c.most_common(1)[0][0]
    return {"T1": 0, "T2": 1, "T3": 2}.get(top, 0)


# (相对 benchmark/ 目录路径, category, 固定 L, 备注) —— 非 mece 样本的手工映射
STATIC = {
    "agentbench/samples/capability_matrix.json": ("static-capability", 0, "静态能力探针"),
    "agentbench/samples/capability_matrix_p0.json": ("static-capability", 1, "P0 专项静态/结构"),
    "agentbench/samples/tokenizer_samples.json": ("dynamic-metric", 1, "token 真值比对"),
    "agentbench/samples/memory_recall.json": ("long-term-memory", 3, "单查询 recall + judge"),
    "long_horizon/samples/scicode_long.json": ("long-horizon", 4, "多步子任务 + 跨步一致性"),
    "long_horizon/samples/terminal_bench_e2e.json": ("long-horizon", 4, "端到端长程 + 测试脚本"),
    "long_horizon/samples/hle_reasoning.json": ("long-horizon", 3, "单轮长推理（对照索引）"),
    "long_horizon/long_horizon_mece.json": ("long-horizon", 2, "T1→L0/T2→L1/T3→L2，取代表 2"),
    "vuln_hunt/tasks/vh-fastjson-js/task.json": ("vuln-hunt", 4, "长程多轴评测"),
    "vuln_hunt/tasks/vh-auto-discovery/task.json": ("vuln-hunt", 4, "长程多轴评测"),
    "vuln_hunt/tasks/vh-log4shell-java/task.json": ("vuln-hunt", 4, "长程多轴评测"),
    "vuln_hunt/tasks/vh-commons-collections/task.json": ("vuln-hunt", 4, "长程多轴评测"),
    "vuln_hunt/fixtures/good/hypotheses.json": ("vuln-hunt", 2, "结构判定 good"),
    "vuln_hunt/fixtures/bad/hypotheses.json": ("vuln-hunt", 2, "结构判定 bad"),
    "vuln_hunt/fixtures/good/gadget_chain.json": ("vuln-hunt", 2, "结构判定 good"),
    "vuln_hunt/fixtures/bad/gadget_chain.json": ("vuln-hunt", 2, "结构判定 bad"),
    "vuln_hunt/fixtures/good/run_poc.json": ("vuln-hunt", 2, "结构判定 good"),
    "vuln_hunt/fixtures/bad/run_poc.json": ("vuln-hunt", 2, "结构判定 bad"),
    "p0/samples/perf_baseline.json": ("p0-e2e", 2, "延迟基线 deterministic"),
    "p0/samples/token_budget.json": ("p0-e2e", 3, "省 token 判定需真模型"),
    "p0/samples/prefix_cache.json": ("p0-e2e", 3, "cache 收益判定需真模型"),
    "lineage/samples/lineage_tasks.json": ("lineage", 5, "实验谱系树 query/audit/branch/cascade_delete（L5 跨会话/谱系评测）"),
}


def count_entries(path: Path) -> int:
    try:
        d = json.load(open(path))
    except Exception:
        return 0
    if isinstance(d, list):
        return len(d)
    if isinstance(d, dict) and "samples" in d:
        return len(d["samples"])
    return 1


def main() -> int:
    samples = []
    # 1) mece_1000 part 文件
    mece_dir = ROOT / "agentbench" / "samples" / "mece_1000"
    if mece_dir.exists():
        for p in sorted(mece_dir.glob("part_*.json")):
            rel = str(p.relative_to(ROOT))
            samples.append({
                "path": rel,
                "category": "static-capability",
                "l_level": _mece_part_l(p),
                "count": count_entries(p),
                "note": "MECE 条目（T1→L0/T2→L1/T3→L2，取代表 tier）",
            })
    # 2) 静态映射表
    for rel, (cat, lvl, note) in STATIC.items():
        p = ROOT / rel
        if p.exists():
            samples.append({
                "path": rel,
                "category": cat,
                "l_level": lvl,
                "count": count_entries(p),
                "note": note,
            })

    # 统计
    from collections import Counter
    by_l = Counter(s["l_level"] for s in samples)
    by_cat = Counter(s["category"] for s in samples)

    registry = {
        "version": 1,
        "taxonomy": "benchmark/L_TAXONOMY.md",
        "summary": {
            "total_samples": len(samples),
            "by_l_level": {f"L{k}": v for k, v in sorted(by_l.items())},
            "by_category": dict(by_cat),
        },
        "samples": samples,
    }
    out = ROOT / "sample_registry.json"
    out.write_text(json.dumps(registry, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"[registry] 写入 {out}")
    print(f"  样本文件数: {len(samples)}")
    print(f"  by L-level : {registry['summary']['by_l_level']}")
    print(f"  by category: {registry['summary']['by_category']}")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
