#!/usr/bin/env python3
"""dynamic_bench.py — 动态运行指标评分器（B 类，40 分）。

采集项（见 EVAL_METRICS.md）:
  B1 构建健康度（零 warning）        6 分
  B2 单元/集成测试通过率              10 分
  B3 冷启动时间                      5 分
  B4 Tokenizer 计数精度              7 分
  B5 压缩保真度                      7 分
  B6 记忆跨会话召回                  5 分

B1/B2/B3 需要 cargo 工具链，耗时较长，可用 --skip-build 跳过（跳过项按 None 记录，
不计入总分，报告中标注为「未采集」，避免虚高）。
B4 通过运行 Rust 侧 token 计数器与 samples/tokenizer_samples.json 的真值比对。

用法:
    python3 benchmark/agentbench/dynamic_bench.py --json results/baseline_dynamic.json
    python3 benchmark/agentbench/dynamic_bench.py --skip-build   # 只跑 B4~B6
"""
from __future__ import annotations

import argparse
import json
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SAMPLES = Path(__file__).parent / "samples"


def find_probe(name: str) -> Path | None:
    """定位 cargo 编译出的探针可执行文件。

    example 产物落在 <target>/{release,debug}/examples/<name>，优先 release。
    除了默认的 ./target，还会看 CARGO_TARGET_DIR —— 多个 cargo 进程并行时
    共享 target 会互相抢锁，实践中常用独立 target 目录编译探针。

    返回 None 表示未编译，调用方按「未采集」处理——绝不用估算值顶替，
    否则分数会失真。
    """
    roots = [REPO / "target"]
    if env_dir := os.environ.get("CARGO_TARGET_DIR"):
        roots.insert(0, Path(env_dir))
    for root in roots:
        for profile in ("release", "debug"):
            p = root / profile / "examples" / name
            if p.exists():
                return p
    return None


# ── 工具 ──────────────────────────────────────────────────────────────────────


def run(cmd: list[str], cwd: Path = REPO, timeout: int = 3600) -> tuple[int, str, str]:
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return -1, "", "timeout"
    except OSError as e:
        return -1, "", str(e)


# ── B1 构建健康度 ─────────────────────────────────────────────────────────────


def bench_build_health() -> dict:
    """cargo clippy warning 数，0 warning 得满分。"""
    code, out, err = run(
        ["cargo", "clippy", "--workspace", "--all-targets", "--message-format=short"],
        timeout=5400,
    )
    text = out + err
    warnings = len(re.findall(r":\s*warning:", text))
    errors = len(re.findall(r":\s*error(\[|:)", text))
    if errors:
        score = 0.0
    elif warnings == 0:
        score = 6.0
    else:
        score = max(0.0, 6.0 - warnings * 0.25)
    return {
        "id": "B1",
        "name": "构建健康度（零 warning）",
        "max": 6,
        "score": round(score, 2),
        "warnings": warnings,
        "errors": errors,
        "ok": code == 0,
    }


# ── B2 测试通过率 ─────────────────────────────────────────────────────────────


def bench_tests() -> dict:
    code, out, err = run(["cargo", "test", "--workspace", "--no-fail-fast"], timeout=7200)
    text = out + err
    passed = sum(int(m) for m in re.findall(r"(\d+) passed", text))
    failed = sum(int(m) for m in re.findall(r"(\d+) failed", text))
    total = passed + failed
    rate = passed / total if total else 0.0
    return {
        "id": "B2",
        "name": "单元/集成测试通过率",
        "max": 10,
        "score": round(rate * 10, 2),
        "passed": passed,
        "failed": failed,
        "total": total,
        "pass_rate": round(rate * 100, 1),
    }


# ── B3 冷启动 ─────────────────────────────────────────────────────────────────


def bench_startup() -> dict:
    binary = REPO / "target" / "release" / "mimofan"
    if not binary.exists():
        binary = REPO / "target" / "debug" / "mimofan"
    if not binary.exists():
        return {"id": "B3", "name": "冷启动时间", "max": 5, "score": None, "note": "未找到构建产物"}

    samples = []
    for _ in range(10):
        t0 = time.perf_counter()
        run([str(binary), "--version"], timeout=60)
        samples.append((time.perf_counter() - t0) * 1000)
    p50 = statistics.median(samples)
    # 100ms 内满分，线性衰减到 500ms 得 0
    score = 5.0 if p50 <= 100 else max(0.0, 5.0 * (500 - p50) / 400)
    return {
        "id": "B3",
        "name": "冷启动时间",
        "max": 5,
        "score": round(score, 2),
        "p50_ms": round(p50, 1),
        "min_ms": round(min(samples), 1),
    }


# ── B4 Tokenizer 精度 ────────────────────────────────────────────────────────


def _mimofan_token_count(text: str, mode: str) -> int:
    """复刻 mimofan 各处启发式，用于基线对照。"""
    if mode == "bytes/4":
        return len(text.encode("utf-8")) // 4
    if mode == "chars/3":
        return -(-len(text) // 3)
    if mode == "chars/4":
        return -(-len(text) // 4)
    raise ValueError(mode)


def bench_tokenizer(rust_counter: Path | None = None) -> dict:
    data = json.loads((SAMPLES / "tokenizer_samples.json").read_text(encoding="utf-8"))
    samples = data["samples"]

    # 优先使用 Rust 侧真实实现（若已提供 CLI 探针）
    counts: dict[str, int] = {}
    used_impl = "heuristic-baseline"
    if rust_counter and rust_counter.exists():
        payload = json.dumps([s["text"] for s in samples], ensure_ascii=False)
        code, out, _ = run([str(rust_counter)], timeout=120)
        if code == 0:
            try:
                counts = {s["id"]: c for s, c in zip(samples, json.loads(out))}
                used_impl = "mimofan-tokenizer"
            except (json.JSONDecodeError, TypeError):
                counts = {}

    per_sample = []
    errs: list[float] = []
    zh_errs: list[float] = []
    for s in samples:
        ref = s["reference_tokens"]
        if counts:
            got = counts[s["id"]]
        else:
            # 基线：用 compaction/mod.rs 的 bytes/4（消息主路径）
            got = _mimofan_token_count(s["text"], "bytes/4")
        rel = abs(got - ref) / ref if ref else 0.0
        errs.append(rel)
        if s["lang"] == "zh":
            zh_errs.append(rel)
        per_sample.append(
            {"id": s["id"], "lang": s["lang"], "reference": ref, "got": got, "rel_error": round(rel, 3)}
        )

    mean_err = sum(errs) / len(errs)
    zh_err = sum(zh_errs) / len(zh_errs) if zh_errs else 0.0
    # 误差 <=5% 满分，>=50% 得 0
    score = 7.0 if mean_err <= 0.05 else max(0.0, 7.0 * (0.50 - mean_err) / 0.45)
    return {
        "id": "B4",
        "name": "Tokenizer 计数精度",
        "max": 7,
        "score": round(score, 2),
        "implementation": used_impl,
        "mean_rel_error_pct": round(mean_err * 100, 1),
        "zh_rel_error_pct": round(zh_err * 100, 1),
        "samples": per_sample,
    }


# ── B5 压缩保真度 / B6 记忆召回 ───────────────────────────────────────────────


def bench_compaction_fidelity(probe: Path | None = None) -> dict:
    """压缩前后关键事实召回率。无 Rust 探针时标记为未采集。

    探针源码已就位为 `crates/tui/examples/probe_compaction.rs`，Cargo 自动
    发现为 example 目标。`mimofan` 在 lib.rs 中以 `#[doc(hidden)] pub mod
    compaction;` 暴露 compaction（仅放开评测所需的公开符号 KEEP_RECENT_MESSAGES
    / estimate_tokens / plan_compaction，不进文档），故 example 作为外部 crate
    可正常引用，且不会让全队 `--all-targets` 编译失败。

    运行前需先编译 example：
        CARGO_TARGET_DIR=/tmp/mimofan-target-probe \\
            cargo build -p mimofan --example probe_compaction
    `find_probe` 会在 target/{debug,release}/examples/ 下找到产物；也可用
    `--compaction-probe <path>` 显式指定。

    未编译（找不到产物）时如实返回「未采集」（score=None，不计入总分），
    而不是记 0 分——记 0 会被误读成「压缩把事实全丢了」，那是失真的结论。
    """
    if not (probe and probe.exists()):
        return {
            "id": "B5",
            "name": "压缩保真度",
            "max": 7,
            "score": None,
            "note": (
                "未采集：需先编译 example（见本函数 docstring），"
                "find_probe 未在 target/ 下找到 probe_compaction 产物"
            ),
        }
    code, out, err = run([str(probe), str(SAMPLES / "memory_recall.json")], timeout=600)
    try:
        res = json.loads(out)
        rate = float(res.get("recall_rate", 0.0))
    except (json.JSONDecodeError, ValueError):
        return {
            "id": "B5",
            "name": "压缩保真度",
            "max": 7,
            "score": None,
            "note": f"探针输出解析失败 (exit={code}): {err.strip()[-300:]}",
        }
    return {
        "id": "B5",
        "name": "压缩保真度",
        "max": 7,
        "score": round(rate * 7, 2),
        "recall_rate": round(rate * 100, 1),
        "detail": {k: v for k, v in res.items() if k != "details"},
    }


def bench_memory_recall(probe: Path | None = None) -> dict:
    if not (probe and probe.exists()):
        return {
            "id": "B6",
            "name": "记忆跨会话召回",
            "max": 5,
            "score": None,
            "note": "需要 Rust 侧记忆探针，未采集",
        }
    code, out, err = run([str(probe), str(SAMPLES / "memory_recall.json")], timeout=600)
    try:
        res = json.loads(out)
        rate = float(res.get("recall_rate", 0.0))
    except (json.JSONDecodeError, ValueError):
        return {
            "id": "B6",
            "name": "记忆跨会话召回",
            "max": 5,
            "score": None,
            "note": f"探针输出解析失败 (exit={code}): {err.strip()[-300:]}",
        }
    return {
        "id": "B6",
        "name": "记忆跨会话召回",
        "max": 5,
        "score": round(rate * 5, 2),
        "recall_rate": round(rate * 100, 1),
        "detail": {k: v for k, v in res.items() if k != "details"},
    }


# ── 主流程 ────────────────────────────────────────────────────────────────────


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-build", action="store_true", help="跳过 B1/B2/B3（cargo 相关，耗时长）")
    ap.add_argument("--json", dest="json_out", default=None)
    ap.add_argument("--tokenizer-probe", default=None)
    ap.add_argument("--compaction-probe", default=None)
    ap.add_argument("--memory-probe", default=None)
    args = ap.parse_args()

    metrics = []
    if args.skip_build:
        for mid, name, mx in (("B1", "构建健康度（零 warning）", 6), ("B2", "单元/集成测试通过率", 10), ("B3", "冷启动时间", 5)):
            metrics.append({"id": mid, "name": name, "max": mx, "score": None, "note": "--skip-build 跳过"})
    else:
        print("[B1] 运行 cargo clippy ...", flush=True)
        metrics.append(bench_build_health())
        print("[B2] 运行 cargo test ...", flush=True)
        metrics.append(bench_tests())
        print("[B3] 测量冷启动 ...", flush=True)
        metrics.append(bench_startup())

    print("[B4] 评估 tokenizer 精度 ...", flush=True)
    metrics.append(bench_tokenizer(Path(args.tokenizer_probe) if args.tokenizer_probe else None))
    # 未显式指定 --*-probe 时，自动在 target/ 下找编译好的 example 产物。
    compaction_probe = (
        Path(args.compaction_probe) if args.compaction_probe else find_probe("probe_compaction")
    )
    memory_probe = Path(args.memory_probe) if args.memory_probe else find_probe("probe_recall")

    print("[B5] 评估压缩保真度 ...", flush=True)
    metrics.append(bench_compaction_fidelity(compaction_probe))
    print("[B6] 评估记忆召回 ...", flush=True)
    metrics.append(bench_memory_recall(memory_probe))

    collected = [m for m in metrics if m.get("score") is not None]
    total = sum(m["score"] for m in collected)
    total_max = sum(m["max"] for m in collected)

    result = {
        "category": "B_dynamic",
        "total_score": round(total, 2),
        "collected_max": total_max,
        "declared_max": 40,
        "percent": round(total / total_max * 100, 1) if total_max else 0.0,
        "metrics": metrics,
    }

    print("\n" + "=" * 72)
    print("  动态运行指标（B 类）")
    print("=" * 72)
    for m in metrics:
        if m.get("score") is None:
            print(f"{m['id']:>4} {m['name']:<22} [未采集] {m.get('note', '')}")
        else:
            print(f"{m['id']:>4} {m['name']:<22} {m['score']:>5.2f}/{m['max']:<3}")
    print("-" * 72)
    print(f"  B 类得分: {total:.2f} / {total_max}（已采集项）")
    print("=" * 72)

    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"\n已写入: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
