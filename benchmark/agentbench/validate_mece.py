#!/usr/bin/env python3
"""validate_mece.py — 1000 题条目集的 MECE 合规校验器。

校验 samples/mece_1000/part_*.json 是否满足 MECE_TAXONOMY.md 的硬约束：
  1. 总数 1000
  2. 域配额 / 簇配额达标（下限 90%）
  3. assert_key + tier 组合唯一（互斥性硬约束）
  4. 每簇覆盖 existence/depth/negative/integration 四视角（穷尽性）
  5. tier 占比接近 T1 55% / T2 30% / T3 15%
  6. Schema 字段完整性与 check.kind 合法性

用法:
    python3 benchmark/agentbench/validate_mece.py [--entries DIR] [--json OUT]

退出码 0 = 无 ERROR 级违规；1 = 存在 ERROR。纯标准库。
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

DEFAULT_ENTRIES_DIR = Path(__file__).parent / "samples" / "mece_1000"

TOTAL_TARGET = 1000
QUOTA_FLOOR = 0.90  # 域/簇配额下限

# 域配额（MECE_TAXONOMY.md 第三节）
DOMAIN_QUOTA: dict[str, int] = {
    "D01": 110, "D02": 90, "D03": 110, "D04": 60,
    "D05": 100, "D06": 80, "D07": 70, "D08": 80,
    "D09": 70, "D10": 70, "D11": 90, "D12": 70,
}

# 簇配额（第四节），{domain: {cluster_no: quota}}
CLUSTER_QUOTA: dict[str, dict[int, int]] = {
    "D01": {1: 20, 2: 20, 3: 25, 4: 20, 5: 15, 6: 10},
    "D02": {1: 20, 2: 15, 3: 20, 4: 20, 5: 15},
    "D03": {1: 20, 2: 25, 3: 25, 4: 20, 5: 20},
    "D04": {1: 20, 2: 15, 3: 10, 4: 15},
    "D05": {1: 20, 2: 20, 3: 20, 4: 20, 5: 20},
    "D06": {1: 20, 2: 20, 3: 20, 4: 20},
    "D07": {1: 20, 2: 20, 3: 15, 4: 15},
    "D08": {1: 20, 2: 20, 3: 20, 4: 20},
    "D09": {1: 20, 2: 20, 3: 15, 4: 15},
    "D10": {1: 20, 2: 20, 3: 15, 4: 15},
    "D11": {1: 20, 2: 20, 3: 25, 4: 25},
    "D12": {1: 15, 2: 25, 3: 15, 4: 15},
}

TIER_TARGET = {"T1": 0.55, "T2": 0.30, "T3": 0.15}
TIER_TOLERANCE = 0.05  # 占比允许偏差 ±5 个百分点

VIEWS = {"existence", "depth", "negative", "integration"}
TIERS = {"T1", "T2", "T3"}
KINDS = {"grep", "struct_assert", "exec"}
ASSERTS = {"fn_has_param", "enum_has_variant", "calls_symbol",
           "count_at_least", "both_present", "absent"}
EXPECTS = {"exit_zero", "stdout_contains", "test_passes"}

ID_RX = re.compile(r"^D\d{2}\.\d+\.\d{3}$")

# tier 与 check.kind 的对应关系（第二、五节）
TIER_KIND = {"T1": "grep", "T2": "struct_assert", "T3": "exec"}


class Report:
    def __init__(self) -> None:
        self.violations: list[dict] = []

    def add(self, level: str, category: str, message: str, ref: str | None = None) -> None:
        self.violations.append({"level": level, "category": category,
                                "message": message, "ref": ref})

    def error(self, category: str, message: str, ref: str | None = None) -> None:
        self.add("ERROR", category, message, ref)

    def warn(self, category: str, message: str, ref: str | None = None) -> None:
        self.add("WARN", category, message, ref)

    @property
    def n_errors(self) -> int:
        return sum(1 for v in self.violations if v["level"] == "ERROR")

    @property
    def n_warns(self) -> int:
        return sum(1 for v in self.violations if v["level"] == "WARN")


def load_entries(entries_dir: Path, rep: Report) -> list[dict]:
    if not entries_dir.exists():
        rep.error("load", f"条目目录不存在: {entries_dir}")
        return []
    parts = sorted(entries_dir.glob("part_*.json"))
    if not parts:
        rep.error("load", f"未找到 part_*.json: {entries_dir}")
        return []
    entries: list[dict] = []
    for part in parts:
        try:
            data = json.loads(part.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            rep.error("load", f"{part.name} JSON 解析失败: {exc}", part.name)
            continue
        if isinstance(data, dict):
            data = data.get("entries", [])
        if not isinstance(data, list):
            rep.error("load", f"{part.name} 顶层既不是数组也不含 entries 数组", part.name)
            continue
        for item in data:
            if isinstance(item, dict):
                item["_source"] = part.name
                entries.append(item)
            else:
                rep.error("load", f"{part.name} 含非对象条目: {item!r}", part.name)
    return entries


# --------------------------------------------------------------------------
# 校验规则
# --------------------------------------------------------------------------

def check_schema(entries: list[dict], rep: Report) -> None:
    """Schema 字段完整性与合法性。"""
    required = ["id", "domain", "cluster", "assert_key", "view", "tier", "desc", "check"]
    for e in entries:
        ref = f"{e.get('_source', '?')}:{e.get('id', '<no-id>')}"
        for field in required:
            if field not in e or e[field] in (None, ""):
                rep.error("schema", f"缺少必填字段 `{field}`", ref)

        eid = e.get("id")
        if eid and not ID_RX.match(str(eid)):
            rep.error("schema", f"id 格式不符 D\\d\\d.\\d+.\\d\\d\\d: {eid}", ref)

        domain = e.get("domain")
        if domain and domain not in DOMAIN_QUOTA:
            rep.error("schema", f"未知 domain: {domain}", ref)
        if eid and domain and not str(eid).startswith(f"{domain}."):
            rep.error("schema", f"id 前缀与 domain 不一致: {eid} vs {domain}", ref)

        cluster = e.get("cluster")
        if domain in CLUSTER_QUOTA:
            try:
                cnum = int(cluster)
            except (TypeError, ValueError):
                rep.error("schema", f"cluster 非整数: {cluster!r}", ref)
            else:
                if cnum not in CLUSTER_QUOTA[domain]:
                    rep.error("schema", f"{domain} 无 cluster {cnum}（骨架未定义）", ref)
                elif eid and not str(eid).startswith(f"{domain}.{cnum}."):
                    rep.error("schema", f"id 与 cluster 不一致: {eid} vs cluster {cnum}", ref)

        view = e.get("view")
        if view and view not in VIEWS:
            rep.error("schema", f"未知 view: {view}（应为 {'/'.join(sorted(VIEWS))}）", ref)

        tier = e.get("tier")
        if tier and tier not in TIERS:
            rep.error("schema", f"未知 tier: {tier}", ref)

        weight = e.get("weight", 1.0)
        if not isinstance(weight, (int, float)) or weight <= 0:
            rep.error("schema", f"weight 必须为正数: {weight!r}", ref)

        check_schema_check(e, rep, ref)


def check_schema_check(e: dict, rep: Report, ref: str) -> None:
    check = e.get("check")
    if not isinstance(check, dict):
        rep.error("schema", "check 不是对象", ref)
        return
    kind = check.get("kind")
    if kind not in KINDS:
        rep.error("schema", f"非法 check.kind: {kind}（应为 {'/'.join(sorted(KINDS))}）", ref)
        return

    tier = e.get("tier")
    if tier in TIER_KIND and kind != TIER_KIND[tier]:
        rep.error("schema", f"tier {tier} 应搭配 check.kind={TIER_KIND[tier]}，实为 {kind}", ref)

    if kind == "grep":
        if not check.get("files"):
            rep.error("schema", "grep 缺少 files", ref)
        if not check.get("patterns") and not check.get("must_not_match"):
            rep.error("schema", "grep 需至少有 patterns 或 must_not_match", ref)
        for key in ("patterns", "must_not_match"):
            for pat in check.get(key) or []:
                try:
                    re.compile(pat)
                except re.error as exc:
                    rep.error("schema", f"{key} 正则非法 /{pat}/: {exc}", ref)

    elif kind == "struct_assert":
        if not check.get("files"):
            rep.error("schema", "struct_assert 缺少 files", ref)
        assert_kind = check.get("assert")
        if assert_kind not in ASSERTS:
            rep.error("schema", f"非法 assert: {assert_kind}", ref)
            return
        args = check.get("args") or {}
        need = {
            "fn_has_param": ["fn", "param"],
            "enum_has_variant": ["enum", "variant"],
            "calls_symbol": ["caller", "callee"],
            "count_at_least": ["pattern", "n"],
            "both_present": ["a", "b"],
            "absent": ["pattern"],
        }[assert_kind]
        for k in need:
            if k not in args or args[k] in (None, ""):
                rep.error("schema", f"{assert_kind} 缺少 args.{k}", ref)
        if assert_kind == "count_at_least" and "n" in args:
            try:
                if int(args["n"]) < 1:
                    rep.error("schema", f"count_at_least 的 n 必须 ≥1: {args['n']}", ref)
            except (TypeError, ValueError):
                rep.error("schema", f"count_at_least 的 n 非整数: {args['n']!r}", ref)
        for k in ("pattern", "a", "b"):
            if k in args and isinstance(args[k], str):
                try:
                    re.compile(args[k])
                except re.error as exc:
                    rep.error("schema", f"args.{k} 正则非法 /{args[k]}/: {exc}", ref)

    elif kind == "exec":
        cmd = check.get("cmd")
        if not cmd or not isinstance(cmd, list):
            rep.error("schema", "exec 的 cmd 必须为非空数组", ref)
        expect = check.get("expect", "exit_zero")
        if expect not in EXPECTS:
            rep.error("schema", f"非法 expect: {expect}", ref)
        if expect == "stdout_contains" and not (check.get("args") or {}).get("text"):
            rep.error("schema", "stdout_contains 缺少 args.text", ref)


def check_total(entries: list[dict], rep: Report) -> dict:
    n = len(entries)
    if n != TOTAL_TARGET:
        level = rep.error if abs(n - TOTAL_TARGET) > TOTAL_TARGET * 0.02 else rep.warn
        level("total", f"总条目数 {n} != {TOTAL_TARGET}（差 {n - TOTAL_TARGET}）")
    return {"count": n, "target": TOTAL_TARGET, "ok": n == TOTAL_TARGET}


def check_unique_id(entries: list[dict], rep: Report) -> None:
    seen: dict[str, str] = {}
    for e in entries:
        eid = e.get("id")
        if not eid:
            continue
        if eid in seen:
            rep.error("uniqueness", f"重复 id `{eid}`（另见 {seen[eid]}）",
                      f"{e.get('_source')}:{eid}")
        else:
            seen[eid] = e.get("_source", "?")


def check_assert_key_uniqueness(entries: list[dict], rep: Report) -> dict:
    """互斥性硬约束：assert_key + tier 组合全集唯一。"""
    groups: dict[tuple[str, str], list[str]] = defaultdict(list)
    for e in entries:
        key, tier = e.get("assert_key"), e.get("tier")
        if not key or not tier:
            continue
        groups[(key, tier)].append(f"{e.get('_source', '?')}:{e.get('id')}")
    dups = {f"{k}|{t}": refs for (k, t), refs in groups.items() if len(refs) > 1}
    for combo, refs in sorted(dups.items()):
        rep.error("uniqueness",
                  f"assert_key+tier 重复 {len(refs)} 次: {combo} → {', '.join(refs[:5])}")
    return {"n_unique_combos": len(groups), "n_duplicated": len(dups),
            "duplicates": dups}


def check_domain_quota(entries: list[dict], rep: Report) -> list[dict]:
    counts = Counter(e.get("domain") for e in entries)
    out = []
    for did, quota in DOMAIN_QUOTA.items():
        n = counts.get(did, 0)
        floor = quota * QUOTA_FLOOR
        ok = n >= floor
        if not ok:
            rep.error("quota", f"{did} 条目数 {n} 低于配额 {quota} 的 90%（{floor:.0f}）")
        elif n > quota * 1.15:
            rep.warn("quota", f"{did} 条目数 {n} 超配额 {quota} 逾 15%")
        out.append({"domain": did, "count": n, "quota": quota,
                    "floor": round(floor, 1), "ok": ok,
                    "ratio": round(n / quota, 3) if quota else 0.0})
    unknown = [d for d in counts if d not in DOMAIN_QUOTA]
    for d in unknown:
        rep.error("quota", f"骨架未定义的域 `{d}` 出现 {counts[d]} 条")
    return out


def check_cluster_quota(entries: list[dict], rep: Report) -> list[dict]:
    counts: Counter = Counter()
    for e in entries:
        try:
            counts[(e.get("domain"), int(e.get("cluster")))] += 1
        except (TypeError, ValueError):
            continue
    out = []
    for did, clusters in CLUSTER_QUOTA.items():
        for cnum, quota in clusters.items():
            n = counts.get((did, cnum), 0)
            floor = quota * QUOTA_FLOOR
            ok = n >= floor
            if not ok:
                rep.error("quota",
                          f"{did}.{cnum} 条目数 {n} 低于配额 {quota} 的 90%（{floor:.0f}）")
            out.append({"cluster": f"{did}.{cnum}", "count": n, "quota": quota, "ok": ok})
    return out


def check_view_coverage(entries: list[dict], rep: Report) -> list[dict]:
    """穷尽性：每簇必须覆盖 existence/depth/negative/integration 四视角。"""
    seen: dict[tuple[str, int], set] = defaultdict(set)
    for e in entries:
        try:
            key = (e.get("domain"), int(e.get("cluster")))
        except (TypeError, ValueError):
            continue
        if e.get("view") in VIEWS:
            seen[key].add(e["view"])
    out = []
    for did, clusters in CLUSTER_QUOTA.items():
        for cnum in clusters:
            got = seen.get((did, cnum), set())
            missing = sorted(VIEWS - got)
            if missing:
                rep.error("exhaustiveness",
                          f"{did}.{cnum} 缺视角: {', '.join(missing)}")
            out.append({"cluster": f"{did}.{cnum}", "views": sorted(got),
                        "missing": missing, "ok": not missing})
    return out


def check_tier_ratio(entries: list[dict], rep: Report) -> dict:
    counts = Counter(e.get("tier") for e in entries)
    total = sum(counts[t] for t in TIERS)
    out = {}
    for tier, target in TIER_TARGET.items():
        n = counts.get(tier, 0)
        ratio = n / total if total else 0.0
        delta = ratio - target
        ok = abs(delta) <= TIER_TOLERANCE
        if not ok:
            rep.warn("tier_ratio",
                     f"{tier} 占比 {ratio:.1%} 偏离目标 {target:.0%} 超过 ±{TIER_TOLERANCE:.0%}"
                     f"（{n}/{total} 条）")
        out[tier] = {"count": n, "ratio": round(ratio, 4),
                     "target": target, "delta": round(delta, 4), "ok": ok}
    unknown = [t for t in counts if t not in TIERS]
    for t in unknown:
        rep.error("tier_ratio", f"未知 tier `{t}` 出现 {counts[t]} 次")
    return {"total": total, "tiers": out}


def check_t1_anti_cheat_coverage(entries: list[dict], rep: Report) -> dict:
    """信息性检查：多少 T1 的 assert_key 有对应 T3 条目（决定反作弊升级上限）。"""
    t1_keys = {e.get("assert_key") for e in entries if e.get("tier") == "T1" and e.get("assert_key")}
    t3_keys = {e.get("assert_key") for e in entries if e.get("tier") == "T3" and e.get("assert_key")}
    upgradable = t1_keys & t3_keys
    ratio = len(upgradable) / len(t1_keys) if t1_keys else 0.0
    if t1_keys and ratio < 0.10:
        rep.warn("anti_cheat",
                 f"仅 {len(upgradable)}/{len(t1_keys)} ({ratio:.1%}) 个 T1 的 assert_key 有对应 T3，"
                 f"绝大多数 T1 将永远只记 0.5 系数")
    return {"n_t1_keys": len(t1_keys), "n_t3_keys": len(t3_keys),
            "n_upgradable": len(upgradable), "upgradable_ratio": round(ratio, 4)}


# --------------------------------------------------------------------------

def validate(entries_dir: Path) -> dict:
    rep = Report()
    entries = load_entries(entries_dir, rep)

    result = {
        "entries_dir": str(entries_dir),
        "total": check_total(entries, rep),
    }
    check_unique_id(entries, rep)
    check_schema(entries, rep)
    result["assert_key_uniqueness"] = check_assert_key_uniqueness(entries, rep)
    result["domain_quota"] = check_domain_quota(entries, rep)
    result["cluster_quota"] = check_cluster_quota(entries, rep)
    result["view_coverage"] = check_view_coverage(entries, rep)
    result["tier_ratio"] = check_tier_ratio(entries, rep)
    result["anti_cheat_coverage"] = check_t1_anti_cheat_coverage(entries, rep)

    result["violations"] = rep.violations
    result["n_errors"] = rep.n_errors
    result["n_warnings"] = rep.n_warns
    result["compliant"] = rep.n_errors == 0
    return result


def print_report(result: dict, max_per_category: int = 15) -> None:
    print("=" * 78)
    print("  1000 题条目集 MECE 合规校验")
    print(f"  条目目录: {result['entries_dir']}")
    print("=" * 78)

    t = result["total"]
    print(f"  总数: {t['count']} / {t['target']}  {'OK' if t['ok'] else '不符'}")

    ak = result["assert_key_uniqueness"]
    print(f"  assert_key+tier 唯一组合: {ak['n_unique_combos']}，重复组合: {ak['n_duplicated']}")

    tr = result["tier_ratio"]
    parts = [f"{k} {v['count']}({v['ratio']:.1%}/目标{v['target']:.0%}){'' if v['ok'] else ' !'}"
             for k, v in tr["tiers"].items()]
    print(f"  tier 分布: {'  '.join(parts)}")

    acc = result["anti_cheat_coverage"]
    print(f"  反作弊可升级: {acc['n_upgradable']}/{acc['n_t1_keys']} 个 T1 assert_key 有对应 T3 "
          f"({acc['upgradable_ratio']:.1%})")

    print("-" * 78)
    print("  域配额:")
    for d in result["domain_quota"]:
        mark = "OK" if d["ok"] else "!!"
        print(f"    [{mark}] {d['domain']}  {d['count']:>4} / {d['quota']:<4} "
              f"(下限 {d['floor']:.0f}, 达成 {d['ratio']:.0%})")

    bad_clusters = [c for c in result["cluster_quota"] if not c["ok"]]
    print(f"  簇配额: {len(result['cluster_quota']) - len(bad_clusters)}"
          f"/{len(result['cluster_quota'])} 达标")
    for c in bad_clusters[:max_per_category]:
        print(f"    [!!] {c['cluster']}  {c['count']} / {c['quota']}")

    bad_views = [v for v in result["view_coverage"] if not v["ok"]]
    print(f"  四视角覆盖: {len(result['view_coverage']) - len(bad_views)}"
          f"/{len(result['view_coverage'])} 簇完整")
    for v in bad_views[:max_per_category]:
        print(f"    [!!] {v['cluster']} 缺 {', '.join(v['missing'])}")

    print("-" * 78)
    by_cat: dict[str, list[dict]] = defaultdict(list)
    for v in result["violations"]:
        by_cat[v["category"]].append(v)
    if by_cat:
        print(f"  违规明细（ERROR {result['n_errors']}，WARN {result['n_warnings']}）:")
        for cat in sorted(by_cat):
            items = by_cat[cat]
            print(f"    [{cat}] {len(items)} 条")
            for v in items[:max_per_category]:
                ref = f" @ {v['ref']}" if v.get("ref") else ""
                print(f"      {v['level']}: {v['message']}{ref}")
            if len(items) > max_per_category:
                print(f"      ... 另有 {len(items) - max_per_category} 条")
    else:
        print("  无违规。")

    print("=" * 78)
    print(f"  结论: {'合规' if result['compliant'] else '不合规'} "
          f"(ERROR {result['n_errors']}, WARN {result['n_warnings']})")
    print("=" * 78)


def main() -> int:
    ap = argparse.ArgumentParser(description="1000 题条目集 MECE 合规校验器")
    ap.add_argument("--entries", default=str(DEFAULT_ENTRIES_DIR))
    ap.add_argument("--json", dest="json_out", default=None)
    ap.add_argument("--max-per-category", type=int, default=15)
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    result = validate(Path(args.entries))

    if not args.quiet:
        print_report(result, args.max_per_category)
    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"\n已写入: {out}")
    return 0 if result["compliant"] else 1


if __name__ == "__main__":
    sys.exit(main())
