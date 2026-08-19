#!/usr/bin/env python3
"""snapshot.py — 把最新 summary.json 复制为带时间戳的趋势快照。

供 `make snapshot` 手动打快照。merge_results.py 每次聚合也会自动落一份快照，
本脚本用于在需要保留某个特殊结果的时刻手动归档（例如某次 full/live 跑分）。

输出：benchmark/results/snapshots/summary_<ts>.json（git 不跟踪，趋势对比用）
"""
from __future__ import annotations

import json
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SUMMARY = ROOT / "results" / "summary.json"
SNAP_DIR = ROOT / "results" / "snapshots"


def main() -> int:
    if not SUMMARY.exists():
        print(f"[snapshot] 无 {SUMMARY}，先运行 run_all.py / merge_results.py")
        return 1
    SNAP_DIR.mkdir(parents=True, exist_ok=True)
    ts = time.strftime("%Y%m%d_%H%M%S")
    out = SNAP_DIR / f"summary_{ts}.json"
    data = json.loads(SUMMARY.read_text(encoding="utf-8"))
    data["snapshot_ts"] = ts
    out.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"[snapshot] 写入 {out}")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
