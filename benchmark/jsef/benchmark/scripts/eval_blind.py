#!/usr/bin/env python3
"""JSEF Benchmark — 盲化评测回连评分器（eval_blind.py）。

用途
----
将被测对象（LLM/SAST）在 ``benchmark/blinded/`` 盲化语料上产出的结果，
回连回真实 checkpoint（``benchmark/expectedresults.csv`` 事实源）并计分。

盲化评测语义
------------
- ``benchmark/scripts/blind.py`` 把 ``benchmark/cases/`` 下源码盲化为
  ``B0001.java``…，并把每个 ``// [CHECKPOINT ...]`` 注解行替换为独立注释
  ``/*ANCHOR_N*/``（行号保持为原 CHECKPOINT 行 = sink 上方一行），同时写出
  私有映射 manifest：
      {"files": {"B0001.java": "<原路径>", ...},
       "anchors": {"B0001.java:ANCHOR_1": "JSEF-XXX-001", ...}}
- 本脚本扫描盲化文件得到 ``盲化文件 -> {锚点行号: checkpoint_id}``，再把被测
  对象对盲化文件上报的每条 finding 按行号容差回连到锚点，从而把盲化结果的
  ``file/line`` 翻译成真实 checkpoint id，随后复用 scorecard 的 ``score_object``
  计算混淆矩阵与指标。

回连规则
--------
1. 被测 finding 的 file 取基名（如 ``B0001.java``），须匹配 ``B\\d+\\.java``。
2. 该文件须出现在锚点行映射中；否则忽略并警告（可能未被本工具盲化）。
3. finding 行号须落在某锚点行容差（``--line-tolerance``，默认 1）内；
   命中则关联到该 checkpoint，构造 ``{id, hit, file, line, cwe, message, elapsed_ms}``；
   容差外无法关联任何 checkpoint → 忽略并警告（盲化评测的 ground truth 就是锚点集合）。
4. 回连后的 findings 写临时 result.json（简化 JSON 列表），复用
   ``scorecard.score_object(..., line_tolerance=0)`` 计分——回连已按 id 精确对齐，
   行容差已在回连阶段消耗。

约束
----
纯标准库（json / os / re / argparse / tempfile / typing），仅 import 同目录
``scorecard.py`` 的 ``load_expected`` / ``_parse_sarif`` / ``score_object``。

示例
----
    python eval_blind.py --expected benchmark/expectedresults.csv \
        --manifest benchmark/blinded/manifest.json \
        --blinded-dir benchmark/blinded \
        --result /tmp/llm_result.json --name "MyLLM" --out /tmp/score.json

退出码
------
0 = 成功；1 = 发生错误。
"""

import argparse
import json
import os
import re
import sys
import tempfile
from typing import Any, Dict, List, Optional, Tuple

import scorecard

# ——————————————————————————————————————————————————————————————————
# 正则
# ——————————————————————————————————————————————————————————————————

# 盲化文件名：B0001.java … B9999.java
BLIND_FILE_RE = re.compile(r"^B\d+\.java$")

# 盲化文件中的中性锚点注释 /*ANCHOR_N*/
ANCHOR_RE = re.compile(r"/\*\s*ANCHOR_(\d+)\s*\*/")


# ——————————————————————————————————————————————————————————————————
# manifest 加载 + 锚点行扫描
# ——————————————————————————————————————————————————————————————————

def load_manifest(manifest_path: str) -> Tuple[Dict[str, str], Dict[str, Dict[str, str]]]:
    """加载盲化 manifest，返回文件映射与锚点→checkpoint 映射。

    Args:
        manifest_path: blind.py 生成的 manifest.json 路径。

    Returns:
        tuple: (files_map, blind_anchor_map)
            - files_map: {盲化文件名(如 B0001.java): 原始完整路径}
            - blind_anchor_map: {盲化文件名: {anchor 名(如 ANCHOR_1): checkpoint_id}}

    Raises:
        FileNotFoundError: 文件不存在。
        ValueError: JSON 解析失败或缺少必需键。
    """
    if not os.path.isfile(manifest_path):
        raise FileNotFoundError("找不到 manifest: %s" % manifest_path)
    with open(manifest_path, encoding="utf-8") as fh:
        manifest = json.load(fh)
    files_map = manifest.get("files")
    anchors = manifest.get("anchors")
    if not isinstance(files_map, dict) or not isinstance(anchors, dict):
        raise ValueError("manifest 缺少 files/anchors 字典结构: %s" % manifest_path)
    blind_anchor_map: Dict[str, Dict[str, str]] = {}
    for key, cid in anchors.items():
        blind_name, _, anchor = str(key).partition(":")
        if not blind_name or not anchor:
            continue
        blind_anchor_map.setdefault(blind_name, {})[anchor] = str(cid)
    return dict(files_map), blind_anchor_map


def scan_anchor_lines(blinded_dir: str,
                      blind_anchor_map: Dict[str, Dict[str, str]],
                      warnings: List[str]) -> Dict[str, Dict[int, str]]:
    """扫描盲化目录，构建 盲化文件 -> {锚点行号: checkpoint_id} 映射。

    一行一个锚点；若同文件多个锚点出现在同一行（异常），以先出现者为准并告警。
    文件中出现但 manifest 中没有的锚点（孤儿锚点）同样告警。

    Args:
        blinded_dir: 盲化文件所在目录。
        blind_anchor_map: load_manifest 的 blind_anchor_map。
        warnings: 收集告警信息的列表（追加写）。

    Returns:
        dict: {盲化文件名: {行号: checkpoint_id}}。
    """
    line_map: Dict[str, Dict[int, str]] = {}
    if not os.path.isdir(blinded_dir):
        warnings.append("盲化目录不存在: %s" % blinded_dir)
        return line_map

    for fn in sorted(os.listdir(blinded_dir)):
        if not BLIND_FILE_RE.match(fn):
            continue
        path = os.path.join(blinded_dir, fn)
        try:
            with open(path, encoding="utf-8", errors="ignore") as fh:
                lines = fh.read().splitlines()
        except OSError as exc:
            warnings.append("无法读取盲化文件 %s: %s" % (path, exc))
            continue

        per_file: Dict[int, str] = {}
        for lineno, text in enumerate(lines, 1):
            m = ANCHOR_RE.search(text)
            if not m:
                continue
            anchor = "ANCHOR_%d" % int(m.group(1))
            cid = blind_anchor_map.get(fn, {}).get(anchor)
            if cid is None:
                warnings.append("%s:%d 的锚点 %s 不在 manifest 中（孤儿锚点）" % (fn, lineno, anchor))
                continue
            if lineno in per_file:
                warnings.append("%s:%d 存在多个锚点，以先出现者为准（%s）" % (fn, lineno, per_file[lineno]))
                continue
            per_file[lineno] = cid
        if per_file:
            line_map[fn] = per_file
    return line_map


# ——————————————————————————————————————————————————————————————————
# 被测对象结果解析（盲化视角：file 为 B*.java，不依赖真实 id）
# ——————————————————————————————————————————————————————————————————

def _norm_entry(item: Dict[str, Any]) -> Dict[str, Any]:
    """将单个简化 JSON 条目归一化为原始 finding。

    hit 缺省视为 True（盲化评测结果即"上报为漏洞"）；显式 false 则保留，
    使回连后的计分语义与 scorecard 一致（hit=false 计入 FN/TN）。
    """
    raw_line = item.get("line", -1)
    try:
        line = int(raw_line)
    except (TypeError, ValueError):
        line = -1
    hit_raw = item.get("hit")
    hit = True if hit_raw is None else bool(hit_raw)
    return {
        "file": str(item.get("file") or ""),
        "line": line,
        "hit": hit,
        "cwe": str(item.get("cwe") or ""),
        "message": str(item.get("message") or ""),
        "elapsed_ms": item.get("elapsed_ms"),
    }


def parse_raw_result(result_path: str) -> List[Dict[str, Any]]:
    """解析被测对象结果文件，返回原始 findings 列表。

    SARIF 复用 scorecard._parse_sarif；简化 JSON 自行解析。与
    scorecard._parse_simple_json 不同，这里容忍无 ``id`` 的条目——盲化结果里
    被测对象不知道真实 checkpoint id，靠 file/line 回连。

    Args:
        result_path: .sarif 或 .json 结果文件路径。

    Returns:
        list[dict]: 每条含 file/line/hit/cwe/message/elapsed_ms。

    Raises:
        FileNotFoundError: 文件不存在。
        ValueError: 无法解析的内容。
    """
    if not os.path.isfile(result_path):
        raise FileNotFoundError("找不到结果文件: %s" % result_path)
    with open(result_path, encoding="utf-8") as fh:
        raw = fh.read()
    ext = os.path.splitext(result_path)[1].lower()
    if ext == ".sarif":
        findings, _elapsed = scorecard._parse_sarif(raw)
        return list(findings.values())

    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ValueError("结果文件不是合法 JSON: %s (%s)" % (result_path, exc))

    if isinstance(data, list):
        return [_norm_entry(item) for item in data if isinstance(item, dict)]
    if isinstance(data, dict):
        out: List[Dict[str, Any]] = []
        for sid, val in data.items():
            if isinstance(val, dict):
                entry = dict(val)
                entry.setdefault("file", str(sid))
                out.append(_norm_entry(entry))
            elif isinstance(val, bool):
                out.append(_norm_entry({"file": str(sid), "hit": val}))
        return out
    raise ValueError("简化的 JSON 结果必须是列表或字典(id→命中信息)")


def parse_results(result_path: str) -> List[Dict[str, Any]]:
    """解析被测对象结果：支持单文件（.sarif/.json）或结果目录。

    ``run_llm_benchmark.py --mode blind`` 对每个盲化文件产出一份 ``.sarif``，
    放在同一结果目录下；本函数接受该目录，合并其下所有 ``*.sarif`` 与
    ``result.json`` 再统一回连评分（跳过 ``trial_*`` 子目录，trials 稳定性聚合
    由 compare_models.py 负责）。

    Args:
        result_path: 结果文件或结果目录路径。

    Returns:
        list[dict]: 合并后的原始 findings（每条含 file/line/hit/cwe/...）。

    Raises:
        FileNotFoundError: 路径不存在。
        ValueError: 目录下无结果文件，或单文件无法解析。
    """
    if not os.path.isdir(result_path):
        return parse_raw_result(result_path)

    files: List[str] = []
    for fn in sorted(os.listdir(result_path)):
        full = os.path.join(result_path, fn)
        if fn.startswith("trial_"):
            continue
        if os.path.isfile(full) and (fn.endswith(".sarif") or fn == "result.json"):
            files.append(full)
    if not files:
        raise ValueError("结果目录 %s 下没有 *.sarif 或 result.json" % result_path)

    merged: List[Dict[str, Any]] = []
    for f in files:
        try:
            merged.extend(parse_raw_result(f))
        except ValueError as exc:
            print("[警告] 跳过 %s: %s" % (f, exc), file=sys.stderr)
    return merged


# ——————————————————————————————————————————————————————————————————
# 回连：盲化 file/line → 真实 checkpoint id
# ——————————————————————————————————————————————————————————————————

def reconnect_findings(raw_findings: List[Dict[str, Any]],
                       line_map: Dict[str, Dict[int, str]],
                       files_map: Dict[str, str],
                       line_tolerance: int,
                       warnings: List[str]) -> List[Dict[str, Any]]:
    """把盲化结果 findings 回连到真实 checkpoint id。

    Args:
        raw_findings: parse_raw_result 的输出。
        line_map: scan_anchor_lines 的输出（盲化文件 -> {锚点行号: checkpoint_id}）。
        files_map: load_manifest 的 files_map（盲化文件名 -> 原始路径）。
        line_tolerance: 命中行号容差（|hit_line - anchor_line| <= 容差）。
        warnings: 收集告警信息的列表（追加写）。

    Returns:
        list[dict]: 回连后的 findings，每条含
        {id, hit, file(原始路径), line, cwe, message, elapsed_ms}，按 id 去重
        （先出现者为准）。无法关联锚点的 finding 不返回，改写入 warnings。
    """
    connected: List[Dict[str, Any]] = []
    seen_ids = set()

    for f in raw_findings:
        raw_file = str(f.get("file") or "")
        bfile = os.path.basename(raw_file.replace("\\", "/"))
        line = f.get("line")

        if not BLIND_FILE_RE.match(bfile):
            warnings.append("忽略 finding: 文件 %r 不是盲化文件（应为 B0001.java 形式）" % raw_file)
            continue
        if bfile not in line_map:
            warnings.append("忽略 finding: 盲化文件 %s 不在锚点映射（可能未被本工具盲化）" % bfile)
            continue
        if not isinstance(line, int) or line < 0:
            warnings.append("忽略 finding: %s 行号无效（%r）" % (bfile, line))
            continue

        best: Optional[Tuple[int, str]] = None  # (距离, checkpoint_id)
        for anchor_line, cid in line_map[bfile].items():
            if abs(line - anchor_line) <= line_tolerance:
                if best is None or abs(line - anchor_line) < best[0]:
                    best = (abs(line - anchor_line), cid)
        if best is None:
            warnings.append("忽略 finding: %s:%d 不在任何锚点容差（%d）内" % (bfile, line, line_tolerance))
            continue

        cid = best[1]
        if cid in seen_ids:
            continue  # 同一 checkpoint 已关联，去重（先出现者为准）
        seen_ids.add(cid)
        connected.append({
            "id": cid,
            "hit": bool(f.get("hit", True)),
            "file": files_map.get(bfile, raw_file),
            "line": line,
            "cwe": str(f.get("cwe") or ""),
            "message": str(f.get("message") or ""),
            "elapsed_ms": f.get("elapsed_ms"),
        })
    return connected


# ——————————————————————————————————————————————————————————————————
# 主流程
# ——————————————————————————————————————————————————————————————————

def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="JSEF 盲化评测回连评分器：把被测对象在盲化语料上的结果回连到真实 checkpoint 并计分。\n"
                    "复用 scorecard.py 的 load_expected / _parse_sarif / score_object。",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--expected", required=True,
                        help="expectedresults.csv 路径（事实源）")
    parser.add_argument("--manifest", default="benchmark/blinded/manifest.json",
                        help="盲化 manifest.json 路径（默认 benchmark/blinded/manifest.json）")
    parser.add_argument("--blinded-dir", default="benchmark/blinded",
                        help="盲化文件所在目录（默认 benchmark/blinded）")
    parser.add_argument("--result", required=True,
                        help="被测对象对盲化语料的输出：.sarif / 简化 JSON / 结果目录（自动合并其下 *.sarif 与 result.json）")
    parser.add_argument("--name", default=None,
                        help="被测对象名（默认取结果文件基名）")
    parser.add_argument("--out", default=None,
                        help="写出结构化 scorecard.json（score_object 返回结构 + aligned）路径")
    parser.add_argument("--timeout-ms", type=int, default=120000,
                        help="单次样本超时阈值（ms），默认 120000")
    parser.add_argument("--line-tolerance", type=int, default=1,
                        help="锚点回连行号容差（默认 1：锚点在 sink 上方一行，LLM 常报 sink 行）")
    args = parser.parse_args(argv)

    warnings: List[str] = []

    # 1) 事实源
    try:
        samples = scorecard.load_expected(args.expected)
    except (FileNotFoundError, KeyError) as exc:
        print("[错误] %s" % exc, file=sys.stderr)
        return 1

    # 2) manifest + 锚点行扫描
    try:
        files_map, blind_anchor_map = load_manifest(args.manifest)
    except (FileNotFoundError, ValueError) as exc:
        print("[错误] %s" % exc, file=sys.stderr)
        return 1
    line_map = scan_anchor_lines(args.blinded_dir, blind_anchor_map, warnings)

    # 3) 解析被测对象结果（单文件或目录：目录合并 *.sarif / result.json）
    try:
        raw_findings = parse_results(args.result)
    except (FileNotFoundError, ValueError) as exc:
        print("[错误] %s" % exc, file=sys.stderr)
        return 1

    # 4) 回连
    connected = reconnect_findings(raw_findings, line_map, files_map,
                                   args.line_tolerance, warnings)
    for w in warnings:
        print("[警告] %s" % w, file=sys.stderr)

    print("盲化回连：原始 findings=%d，回连=%d，忽略=%d"
          % (len(raw_findings), len(connected), len(raw_findings) - len(connected)))

    # 5) 写临时 result.json 并复用 scorecard 计分（line_tolerance=0：回连已按 id 精确对齐）
    if not connected:
        print("[警告] 没有可回连的 findings，混淆矩阵将全为 FN/TN", file=sys.stderr)
    fd, temp_path = tempfile.mkstemp(suffix=".json", prefix="eval_blind_")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            json.dump(connected, fh, ensure_ascii=False, indent=2)
        try:
            report, aligned = scorecard.score_object(
                samples, temp_path, args.timeout_ms, line_tolerance=0)
        except (FileNotFoundError, ValueError) as exc:
            print("[错误] %s" % exc, file=sys.stderr)
            return 1
    finally:
        try:
            os.remove(temp_path)
        except OSError:
            pass

    name = args.name or os.path.splitext(os.path.basename(args.result))[0]
    report["name"] = name
    m = report["metrics"]

    # 6) stdout 摘要
    print("\nJSEF 盲化评测 — %s" % name)
    print("Recall=%.3f  Precision=%.3f  FPR=%.3f  Youden=%.1f" % (
        m["Recall"], m["Precision"], m["FPR"], m["Youden"]))
    print("TP=%d  FN=%d  FP=%d  TN=%d" % (m["TP"], m["FN"], m["FP"], m["TN"]))

    # 7) 可选结构化输出
    if args.out:
        out_report = {
            "name": name,
            "metrics": m,
            "timing": report["timing"],
            "simplicity": report["simplicity"],
            "completeness": {"total_completeness": report["completeness"]},
            "by_cwe": report["by_cwe"],
            "by_level": report["by_level"],
            "aligned": aligned,
        }
        try:
            with open(args.out, "w", encoding="utf-8") as fh:
                json.dump(out_report, fh, indent=2, ensure_ascii=False)
            print("\n[完成] 结构化报告已写出: %s" % args.out)
        except OSError as exc:
            print("[警告] 无法写出 --out: %s" % exc, file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
