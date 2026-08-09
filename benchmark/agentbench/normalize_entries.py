#!/usr/bin/env python3
"""normalize_entries.py — 条目集规范化器。

多个编写者独立产出条目时会出现两类系统性偏差，本脚本统一修正：

1. **assert_key 路径不完整**：写成 `tools/file.rs::X` 而非 `crates/tui/src/tools/file.rs::X`。
   反作弊升级靠 assert_key 逐字符相等匹配，路径写法不一致会导致 T1 永远停在 0.5 系数、
   总分被系统性压低。这里按 check.files 里的真实路径回填前缀。

2. **schema 方言差异**：部分条目用了 `pattern`(str) / `cmd`(str) / `expect:"pass"`，
   而引擎要求 `patterns`(list) / `cmd`(list) / `expect:"test_passes"`。

规范化是幂等的：已合规的条目原样保留。

用法:
    python3 normalize_entries.py [--dir DIR] [--dry-run]
"""
from __future__ import annotations

import argparse
import json
import shlex
import sys
from pathlib import Path

DEFAULT_DIR = Path(__file__).parent / "samples" / "mece_1000"

# expect 值的方言映射
EXPECT_ALIAS = {
    "pass": "test_passes",
    "passes": "test_passes",
    "ok": "test_passes",
    "success": "exit_zero",
    "zero": "exit_zero",
    "contains": "stdout_contains",
}


def resolve_key_prefix(key: str, files: list[str]) -> str:
    """用 check.files 里的真实路径补全 assert_key 的路径前缀。"""
    if "::" not in key:
        return key
    path_part, _, sym = key.partition("::")
    if path_part.startswith("crates/"):
        return key
    # 在 files 中找以该片段结尾的真实路径
    for f in files:
        if f.endswith(path_part) or path_part in f:
            return f"{f}::{sym}"
    return key


def normalize_entry(e: dict) -> tuple[dict, list[str]]:
    """返回 (规范化后的条目, 改动说明列表)。"""
    changes: list[str] = []
    check = e.get("check", {})
    files = check.get("files") or []

    # 1) assert_key 路径补全
    old_key = e.get("assert_key", "")
    new_key = resolve_key_prefix(old_key, files)
    if new_key != old_key:
        e["assert_key"] = new_key
        changes.append(f"assert_key: {old_key} -> {new_key}")

    kind = check.get("kind")

    # 2) grep: pattern(str) -> patterns(list)
    if kind == "grep":
        if "pattern" in check and "patterns" not in check:
            p = check.pop("pattern")
            check["patterns"] = [p] if isinstance(p, str) else list(p)
            changes.append("pattern -> patterns")
        elif isinstance(check.get("patterns"), str):
            check["patterns"] = [check["patterns"]]
            changes.append("patterns str -> list")

    # 3) exec: cmd(str) -> cmd(list)，expect 方言归一
    if kind == "exec":
        cmd = check.get("cmd")
        if isinstance(cmd, str):
            check["cmd"] = shlex.split(cmd)
            changes.append("cmd str -> list")
        exp = check.get("expect")
        if isinstance(exp, str) and exp in EXPECT_ALIAS:
            check["expect"] = EXPECT_ALIAS[exp]
            changes.append(f"expect: {exp} -> {check['expect']}")

    # 4) struct_assert: args.pattern 保持原样（引擎就用单数）
    return e, changes


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", default=str(DEFAULT_DIR))
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    d = Path(args.dir)
    files = sorted(d.glob("part_*.json"))
    if not files:
        print(f"未找到条目文件: {d}/part_*.json")
        return 1

    grand_total = 0
    grand_changed = 0
    for f in files:
        try:
            entries = json.loads(f.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            print(f"[跳过] {f.name}: JSON 解析失败 {exc}")
            continue

        changed = 0
        samples: list[str] = []
        for e in entries:
            _, ch = normalize_entry(e)
            if ch:
                changed += 1
                if len(samples) < 3:
                    samples.append(f"    {e['id']}: {'; '.join(ch)}")

        grand_total += len(entries)
        grand_changed += changed
        status = "(dry-run)" if args.dry_run else "已写回"
        print(f"{f.name}: {len(entries)} 条, 规范化 {changed} 条 {status}")
        for s in samples:
            print(s)

        if not args.dry_run and changed:
            f.write_text(
                json.dumps(entries, ensure_ascii=False, indent=1), encoding="utf-8"
            )

    print(f"\n合计: {grand_total} 条, 规范化 {grand_changed} 条")
    return 0


if __name__ == "__main__":
    sys.exit(main())
