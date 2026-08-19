#!/usr/bin/env python3
"""build_sample_registry.py — 扫描 benchmark/ 样本，生成带 L 分级的注册表。

产出 benchmark/sample_registry.json：
  {
    "version": 1,
    "taxonomy": "benchmark/L_TAXONOMY.md",
    "samples": [
      {"path": "...", "category": "...", "l_level": N, "count": K, "note": "...",
       "score": {"percent": 91.7, "source": "mece_static", "updated": "..."} },  # 可选，回灌写入
      ...
    ]
  }

分级口径见 benchmark/L_TAXONOMY.md。本脚本内置「文件 → (category, L)」映射，
对 mece_1000 按 part 文件估算 L0/L1/L2 占比（基于 tier 统计，可选）。

**回灌闭环**：生成 samples 后读取 benchmark/results/summary.json（由
run_all.py + merge_results.py 产出），把每个样本对应 harness 的最新得分写入
samples[i].score。对尚无对应得分的样本，保留上一次已写入的 score（合并而非
覆盖），避免每跑一次 build 就把历史得分清空。这样「跑评测 → 出汇总 → 重建
registry」形成数据闭环。

运行：
    python3 benchmark/build_sample_registry.py
"""
from __future__ import annotations

import json
import os
import time
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
    # 长程执行 + R&D 自动化质量（l_level=4）：MettleBench/LHTB 风格有序检查表前缀
    # 进度 + 执行恢复轴（AgentRewind）；PaperBench 风格 rubric 树 + ResearchCodeBench
    # 风格 target_signature 判定（R&D 自动化）。评测脚本统一在 rd_exec_eval.py。
    "long_horizon/samples/long_exec.json": ("long-horizon-exec", 4, "长程执行：有序检查表前缀进度 + 执行恢复轴（AgentRewind/MettleBench 思想）"),
    "long_horizon/samples/rd_automation.json": ("rd-automation", 4, "R&D 自动化：rubric 树（PaperBench）+ target_signature（ResearchCodeBench）"),
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


# 样本 category → 负责评分的 harness 输出 key（与 merge_results.HARNESS_CATEGORY 反向）。
# 用于把 summary.json 中各 harness 得分回灌到对应 category 的每个样本。
CATEGORY_HARNESS = {
    "static-capability": "mece_static",
    "dynamic-metric": "dynamic_bench",
    "long-term-memory": "longmemeval",
    "long-horizon": "long_horizon",
    "long-horizon-exec": "rd_exec_eval",
    "lineage": "lineage",
    "vuln-hunt": "vuln_hunt",
    "p0-e2e": "p0_dynamic",
}


def _load_summary_harnesses() -> dict:
    """读取 results/summary.json 的 harnesses 层（{key: {percent, ...}}），缺失返回 {}。"""
    p = ROOT / "results" / "summary.json"
    if not p.exists():
        return {}
    try:
        data = json.loads(p.read_text(encoding="utf-8"))
        return data.get("harnesses", {})
    except Exception:
        return {}


def _load_old_scores() -> dict:
    """读现有 sample_registry.json 的 samples 路径 → score 映射（用于合并保留旧得分）。"""
    p = ROOT / "sample_registry.json"
    if not p.exists():
        return {}
    try:
        data = json.loads(p.read_text(encoding="utf-8"))
    except Exception:
        return {}
    return {s.get("path"): s.get("score") for s in data.get("samples", []) if isinstance(s, dict)}


def _backfill_scores(samples: list[dict]) -> list[dict]:
    """把 summary.json 的 harness 得分回灌到 samples[i].score（合并而非覆盖）。

    对 category 有对应 harness 且该 harness 有得分的样本，写入新 score；
    否则保留旧 registry 里已写入的 score。
    """
    harnesses = _load_summary_harnesses()
    old_scores = _load_old_scores()
    now = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    for s in samples:
        path = s.get("path")
        cat = s.get("category")
        hname = CATEGORY_HARNESS.get(cat)
        h = harnesses.get(hname) if hname else None
        if h and isinstance(h.get("percent"), (int, float)):
            s["score"] = {
                "percent": h["percent"],
                "source": hname,
                "updated": now,
            }
        else:
            # 本次无对应得分：合并保留历史 score
            old = old_scores.get(path)
            if old is not None:
                s["score"] = old
    return samples


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

    # 回灌得分（闭环）：读 summary.json 写 score，合并保留旧得分
    samples = _backfill_scores(samples)

    # 统计
    from collections import Counter
    by_l = Counter(s["l_level"] for s in samples)
    by_cat = Counter(s["category"] for s in samples)
    scored = sum(1 for s in samples if s.get("score") and s["score"].get("percent") is not None)

    registry = {
        "version": 1,
        "taxonomy": "benchmark/L_TAXONOMY.md",
        "summary": {
            "total_samples": len(samples),
            "by_l_level": {f"L{k}": v for k, v in sorted(by_l.items())},
            "by_category": dict(by_cat),
            "coverage": {
                "scored": scored,
                "total": len(samples),
                "percent": round(scored / len(samples) * 100, 1) if samples else 0.0,
            },
        },
        "samples": samples,
    }
    out = ROOT / "sample_registry.json"
    out.write_text(json.dumps(registry, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"[registry] 写入 {out}")
    print(f"  样本文件数: {len(samples)}")
    print(f"  by L-level : {registry['summary']['by_l_level']}")
    print(f"  by category: {registry['summary']['by_category']}")
    print(f"  score 回灌: {scored}/{len(samples)} ({registry['summary']['coverage']['percent']}%)")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
