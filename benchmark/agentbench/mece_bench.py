#!/usr/bin/env python3
"""mece_bench.py — 1000 题 MECE 能力基准的分层评分引擎。

加载 samples/mece_1000/part_*.json（合计 1000 条），按 MECE_TAXONOMY.md 的
三层判定（T1 grep / T2 struct_assert / T3 exec）逐条执行，应用反作弊系数，
按域配额归一化到 100 分总分。

用法:
    python3 benchmark/agentbench/mece_bench.py [--repo PATH] [--json OUT]
                                               [--skip-exec] [--target-dir DIR]

设计约束（见 MECE_TAXONOMY.md 第二、六节）:
  - T1 条目单独最高只记 0.5 系数；同 assert_key 存在**通过的** T3 条目时升到 1.0
  - 每条 exec 强制超时（默认 180s），超时判失败并记 reason，不挂死
  - 相同 cmd 在一次运行内只执行一次并缓存（多条目共享同一次 cargo test）
  - 评分诚实：任何失败/异常/文件缺失一律记 0 分并留存 reason

纯标准库，无第三方依赖。
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_ENTRIES_DIR = Path(__file__).parent / "samples" / "mece_1000"

# 域配额（MECE_TAXONOMY.md 第三节），用于归一化到 100 分总分
DOMAIN_QUOTA: dict[str, int] = {
    "D01": 110, "D02": 90, "D03": 110, "D04": 60,
    "D05": 100, "D06": 80, "D07": 70, "D08": 80,
    "D09": 70, "D10": 70, "D11": 90, "D12": 70,
}

DOMAIN_NAME: dict[str, str] = {
    "D01": "文件读写与编辑保真",
    "D02": "命令执行与沙箱安全",
    "D03": "上下文压缩与预算",
    "D04": "Tokenizer 与成本核算",
    "D05": "长程记忆与召回",
    "D06": "任务规划与目标循环",
    "D07": "多 Agent 编排",
    "D08": "工具协议与 Schema",
    "D09": "代码理解与检索",
    "D10": "扩展性（MCP/hook/skill）",
    "D11": "可观测、错误恢复与韧性",
    "D12": "工程化与发布质量",
}

DEFAULT_EXEC_TIMEOUT = 180

# 强项 / 达标 / 短板 分界（第六节）
STRONG_THRESHOLD = 0.80
PASS_THRESHOLD = 0.60


# --------------------------------------------------------------------------
# 条目加载
# --------------------------------------------------------------------------

def load_entries(entries_dir: Path) -> list[dict]:
    """加载 part_*.json，合并为条目列表。每个 part 可以是数组或 {"entries": [...]}。"""
    if not entries_dir.exists():
        raise SystemExit(f"条目目录不存在: {entries_dir}")

    entries: list[dict] = []
    parts = sorted(entries_dir.glob("part_*.json"))
    if not parts:
        raise SystemExit(f"未找到 part_*.json: {entries_dir}")

    for part in parts:
        try:
            data = json.loads(part.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise SystemExit(f"{part.name} JSON 解析失败: {exc}") from exc
        if isinstance(data, dict):
            data = data.get("entries", [])
        if not isinstance(data, list):
            raise SystemExit(f"{part.name} 顶层既不是数组也不含 entries 数组")
        for item in data:
            item["_source"] = part.name
        entries.extend(data)
    return entries


# --------------------------------------------------------------------------
# 源码读取（带缓存，1000 条目会反复读同一批文件）
# --------------------------------------------------------------------------

class SourceCache:
    def __init__(self, repo: Path) -> None:
        self.repo = repo
        self._cache: dict[str, str | None] = {}

    def read(self, rel: str) -> str | None:
        """读取仓库内相对路径文件内容；不存在或读失败返回 None。"""
        if rel in self._cache:
            return self._cache[rel]
        path = self.repo / rel
        text: str | None = None
        try:
            if path.is_file():
                text = path.read_text(encoding="utf-8", errors="replace")
            elif path.is_dir():
                # 目录：拼接目录下所有 .rs/.toml/.md 文本，供 grep 类断言使用
                chunks = []
                for sub in sorted(path.rglob("*")):
                    if sub.is_file() and sub.suffix in (".rs", ".toml", ".md", ".json"):
                        try:
                            chunks.append(sub.read_text(encoding="utf-8", errors="replace"))
                        except OSError:
                            continue
                text = "\n".join(chunks) if chunks else None
        except OSError:
            text = None
        self._cache[rel] = text
        return text

    def read_many(self, rels: list[str]) -> list[tuple[str, str]]:
        """返回 [(rel, content)]，跳过不存在的文件。"""
        out = []
        for rel in rels:
            content = self.read(rel)
            if content is not None:
                out.append((rel, content))
        return out


def compile_pattern(pattern: str) -> re.Pattern | None:
    try:
        return re.compile(pattern, re.MULTILINE)
    except re.error:
        return None


# --------------------------------------------------------------------------
# T1: grep 执行器
# --------------------------------------------------------------------------

def run_grep(check: dict, src: SourceCache) -> tuple[bool, str]:
    """返回 (是否通过, 原因)。失败时原因说明为什么。"""
    patterns: list[str] = check.get("patterns") or []
    files: list[str] = check.get("files") or []
    require_all: bool = bool(check.get("require_all", False))
    must_not_match: list[str] = check.get("must_not_match") or []

    if not patterns and not must_not_match:
        return False, "check 缺少 patterns"
    if not files:
        return False, "check 缺少 files"

    loaded = src.read_many(files)
    if not loaded:
        return False, f"目标文件均不存在: {', '.join(files)}"

    missing = [f for f in files if src.read(f) is None]

    # must_not_match：命中即失败（旧实现残留）
    for pat in must_not_match:
        rx = compile_pattern(pat)
        if rx is None:
            return False, f"must_not_match 正则非法: {pat}"
        for rel, content in loaded:
            if rx.search(content):
                return False, f"must_not_match 命中残留实现 /{pat}/ @ {rel}"

    if not patterns:
        return True, "must_not_match 全部未命中（旧实现已清理）"

    hits, misses = [], []
    for pat in patterns:
        rx = compile_pattern(pat)
        if rx is None:
            misses.append(f"{pat}(正则非法)")
            continue
        if any(rx.search(content) for _, content in loaded):
            hits.append(pat)
        else:
            misses.append(pat)

    if require_all:
        if misses:
            note = f"（另有文件缺失: {', '.join(missing)}）" if missing else ""
            return False, f"require_all 未全中，缺: {', '.join(misses)}{note}"
        return True, f"全部 {len(hits)} 个 pattern 命中"

    if hits:
        return True, f"命中 {len(hits)}/{len(patterns)}: {hits[0]}"
    return False, f"无 pattern 命中: {', '.join(patterns[:3])}"


# --------------------------------------------------------------------------
# T2: struct_assert 执行器
# --------------------------------------------------------------------------

def _find_fn_signature(content: str, fn_name: str) -> list[str]:
    """定位 Rust 函数签名（可能跨行），返回所有匹配的签名文本（从 fn 名到 `{` 或 `;`）。

    比裸 grep 严格：只在 `fn <name>` 起始、到函数体左花括号（或 trait 声明的分号）
    之间的范围内取文本，参数命中必须落在这个范围内。
    """
    sigs: list[str] = []
    for m in re.finditer(rf"\bfn\s+{re.escape(fn_name)}\b", content):
        start = m.start()
        depth_paren = 0
        seen_paren = False
        i = m.end()
        end = None
        # 扫描到签名结束：参数括号闭合后，遇到 `{`（函数体）或 `;`（声明）
        while i < len(content):
            ch = content[i]
            if ch == "(":
                depth_paren += 1
                seen_paren = True
            elif ch == ")":
                depth_paren -= 1
            elif seen_paren and depth_paren == 0:
                if ch == "{" or ch == ";":
                    end = i
                    break
            i += 1
            # 防止病态输入无限扫描
            if i - start > 8000:
                break
        if end is not None:
            sigs.append(content[start:end])
    return sigs


def _find_fn_body(content: str, fn_name: str) -> list[str]:
    """提取 Rust 函数体（花括号配对），返回所有同名函数的函数体文本。"""
    bodies: list[str] = []
    for m in re.finditer(rf"\bfn\s+{re.escape(fn_name)}\b", content):
        # 先找到函数体起始的 `{`
        i = m.end()
        depth_paren = 0
        seen_paren = False
        brace_start = None
        while i < len(content):
            ch = content[i]
            if ch == "(":
                depth_paren += 1
                seen_paren = True
            elif ch == ")":
                depth_paren -= 1
            elif seen_paren and depth_paren == 0:
                if ch == "{":
                    brace_start = i
                    break
                if ch == ";":
                    break
            i += 1
            if i - m.end() > 8000:
                break
        if brace_start is None:
            continue
        # 花括号配对找函数体结束
        depth = 0
        j = brace_start
        while j < len(content):
            ch = content[j]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    bodies.append(content[brace_start : j + 1])
                    break
            j += 1
    return bodies


def _find_enum_body(content: str, enum_name: str) -> list[str]:
    """提取 Rust 枚举定义体（花括号配对）。"""
    bodies: list[str] = []
    for m in re.finditer(rf"\benum\s+{re.escape(enum_name)}\b", content):
        i = m.end()
        brace_start = None
        while i < len(content):
            if content[i] == "{":
                brace_start = i
                break
            if content[i] == ";":
                break
            i += 1
            if i - m.end() > 2000:
                break
        if brace_start is None:
            continue
        depth = 0
        j = brace_start
        while j < len(content):
            ch = content[j]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    bodies.append(content[brace_start : j + 1])
                    break
            j += 1
    return bodies


def run_struct_assert(check: dict, src: SourceCache) -> tuple[bool, str]:
    kind = check.get("assert")
    args: dict = check.get("args") or {}
    files: list[str] = check.get("files") or []

    if not kind:
        return False, "check 缺少 assert 字段"
    if not files:
        return False, "check 缺少 files"

    loaded = src.read_many(files)
    if not loaded:
        return False, f"目标文件均不存在: {', '.join(files)}"

    if kind == "fn_has_param":
        fn_name, param = args.get("fn"), args.get("param")
        if not fn_name or not param:
            return False, "fn_has_param 缺少 args.fn 或 args.param"
        found_fn = False
        param_rx = re.compile(rf"\b{re.escape(param)}\b")
        for rel, content in loaded:
            sigs = _find_fn_signature(content, fn_name)
            if sigs:
                found_fn = True
            for sig in sigs:
                if param_rx.search(sig):
                    return True, f"fn {fn_name} 签名含参数 {param} @ {rel}"
        if not found_fn:
            return False, f"未找到函数 {fn_name}"
        return False, f"函数 {fn_name} 存在但签名不含参数 {param}"

    if kind == "enum_has_variant":
        enum_name, variant = args.get("enum"), args.get("variant")
        if not enum_name or not variant:
            return False, "enum_has_variant 缺少 args.enum 或 args.variant"
        found_enum = False
        var_rx = re.compile(rf"\b{re.escape(variant)}\b")
        for rel, content in loaded:
            bodies = _find_enum_body(content, enum_name)
            if bodies:
                found_enum = True
            for body in bodies:
                if var_rx.search(body):
                    return True, f"enum {enum_name} 含 variant {variant} @ {rel}"
        if not found_enum:
            return False, f"未找到枚举 {enum_name}"
        return False, f"枚举 {enum_name} 存在但不含 variant {variant}"

    if kind == "calls_symbol":
        caller, callee = args.get("caller"), args.get("callee")
        if not caller or not callee:
            return False, "calls_symbol 缺少 args.caller 或 args.callee"
        found_caller = False
        callee_rx = compile_pattern(rf"\b{re.escape(callee)}\b")
        for rel, content in loaded:
            bodies = _find_fn_body(content, caller)
            if bodies:
                found_caller = True
            for body in bodies:
                if callee_rx.search(body):
                    return True, f"{caller} 函数体内调用 {callee} @ {rel}"
        if not found_caller:
            return False, f"未找到调用方函数 {caller}"
        return False, f"函数 {caller} 存在但体内未调用 {callee}"

    if kind == "count_at_least":
        pattern, n = args.get("pattern"), args.get("n")
        if pattern is None or n is None:
            return False, "count_at_least 缺少 args.pattern 或 args.n"
        rx = compile_pattern(pattern)
        if rx is None:
            return False, f"正则非法: {pattern}"
        total = sum(len(rx.findall(content)) for _, content in loaded)
        if total >= int(n):
            return True, f"/{pattern}/ 命中 {total} 次 ≥ {n}"
        return False, f"/{pattern}/ 仅命中 {total} 次 < {n}"

    if kind == "both_present":
        a, b = args.get("a"), args.get("b")
        if not a or not b:
            return False, "both_present 缺少 args.a 或 args.b"
        rx_a, rx_b = compile_pattern(a), compile_pattern(b)
        if rx_a is None or rx_b is None:
            return False, f"正则非法: {a if rx_a is None else b}"
        for rel, content in loaded:
            if rx_a.search(content) and rx_b.search(content):
                return True, f"/{a}/ 与 /{b}/ 同文件命中 @ {rel}"
        hit_a = any(rx_a.search(c) for _, c in loaded)
        hit_b = any(rx_b.search(c) for _, c in loaded)
        missing = []
        if not hit_a:
            missing.append(a)
        if not hit_b:
            missing.append(b)
        if missing:
            return False, f"未命中: {', '.join(missing)}"
        return False, f"/{a}/ 与 /{b}/ 分散在不同文件，未同文件共存"

    if kind == "absent":
        pattern = args.get("pattern")
        if not pattern:
            return False, "absent 缺少 args.pattern"
        rx = compile_pattern(pattern)
        if rx is None:
            return False, f"正则非法: {pattern}"
        for rel, content in loaded:
            m = rx.search(content)
            if m:
                line = content[: m.start()].count("\n") + 1
                return False, f"残留 /{pattern}/ @ {rel}:{line}"
        return True, f"/{pattern}/ 已不存在（旧实现已清理）"

    return False, f"未知 assert 类型: {kind}"


# --------------------------------------------------------------------------
# T3: exec 执行器（带命令级缓存）
# --------------------------------------------------------------------------

class ExecRunner:
    """执行外部命令，按 cmd 去重缓存。

    缓存键 = (repo, tuple(cmd))。同一 cmd 在一次运行内只真正执行一次，
    后续条目直接复用 (returncode, stdout, stderr, timed_out) 结果。
    这是性能关键：多条 T3 条目常依赖同一次 `cargo test` 的输出。
    """

    def __init__(self, repo: Path, target_dir: str | None, enabled: bool = True) -> None:
        self.repo = repo
        self.target_dir = target_dir
        self.enabled = enabled
        self._cache: dict[tuple[str, ...], dict] = {}
        self.stats = {"executed": 0, "cache_hits": 0, "timeouts": 0}

    def run(self, cmd: list[str], timeout: int) -> dict:
        key = tuple(cmd)
        if key in self._cache:
            self.stats["cache_hits"] += 1
            return self._cache[key]

        env = os.environ.copy()
        if self.target_dir:
            env["CARGO_TARGET_DIR"] = self.target_dir
        # 保证 cargo 输出可解析、不带 ANSI 色码
        env.setdefault("CARGO_TERM_COLOR", "never")

        result: dict
        try:
            proc = subprocess.run(
                cmd,
                cwd=str(self.repo),
                capture_output=True,
                text=True,
                timeout=timeout,
                env=env,
            )
            result = {
                "returncode": proc.returncode,
                "stdout": proc.stdout,
                "stderr": proc.stderr,
                "timed_out": False,
                "error": None,
            }
            self.stats["executed"] += 1
        except subprocess.TimeoutExpired:
            result = {
                "returncode": None, "stdout": "", "stderr": "",
                "timed_out": True, "error": f"超时 {timeout}s",
            }
            self.stats["executed"] += 1
            self.stats["timeouts"] += 1
        except (OSError, ValueError) as exc:
            result = {
                "returncode": None, "stdout": "", "stderr": "",
                "timed_out": False, "error": f"无法执行: {exc}",
            }
            self.stats["executed"] += 1

        self._cache[key] = result
        return result


TEST_RESULT_RX = re.compile(r"^test result:\s+(ok|FAILED)\.", re.MULTILINE)
# 抓每个测试二进制的通过数，用于识别「0 passed」假阳性（用例名写错时 libtest 仍输出 ok）
TEST_PASSED_RX = re.compile(r"(\d+) passed", re.MULTILINE)

# 「测试没跑起来」而非「测试失败」的特征。命中这些说明是工具链/构建/环境问题，
# 判失败的同时必须在 reason 里显式标注 INFRA，避免被当成能力缺失写进结论。
INFRA_FAILURE_PATTERNS: list[tuple[str, str]] = [
    (r"failed to run `rustc` to learn about target-specific information",
     "rustc 探测失败（工具链/RUSTFLAGS 配置问题）"),
    (r"Unrecognized option: '[^']+'", "rustc 不识别的 flag（多半来自 cargo config 的 rustflags）"),
    (r"error: could not compile", "编译失败，测试未执行"),
    (r"^error\[E\d+\]", "编译错误，测试未执行"),
    (r"error: linking with .* failed", "链接失败，测试未执行"),
    (r"error: no such command", "cargo 子命令不存在"),
    (r"error: package ID specification .* did not match", "指定的 package 不存在"),
    (r"error: no test target named", "指定的 test target 不存在"),
    (r"Blocking waiting for file lock", "cargo 锁竞争（建议用 --target-dir 隔离）"),
    (r"error: failed to acquire package cache lock", "cargo 包缓存锁竞争"),
]


def detect_infra_failure(output: str) -> str | None:
    """识别「构建/工具链没跑起来」类失败，返回人类可读原因；否则 None。"""
    for pat, label in INFRA_FAILURE_PATTERNS:
        if re.search(pat, output, re.MULTILINE):
            return label
    return None


def run_exec(check: dict, entry: dict, runner: ExecRunner) -> tuple[bool, str]:
    if not runner.enabled:
        return False, "SKIPPED: --skip-exec 已跳过 T3 执行"

    cmd = check.get("cmd")
    if not cmd or not isinstance(cmd, list):
        return False, "check 缺少 cmd 数组"

    timeout = int(entry.get("timeout") or check.get("timeout") or DEFAULT_EXEC_TIMEOUT)
    res = runner.run([str(c) for c in cmd], timeout)

    if res["timed_out"]:
        return False, f"命令超时（{timeout}s）: {' '.join(cmd)}"
    if res["error"]:
        return False, f"{res['error']}: {' '.join(cmd)}"

    expect = check.get("expect", "exit_zero")
    args = check.get("args") or {}
    combined = (res["stdout"] or "") + (res["stderr"] or "")

    if expect == "exit_zero":
        if res["returncode"] == 0:
            return True, "退出码 0"
        tail = combined.strip().splitlines()[-3:]
        return False, f"退出码 {res['returncode']}；末尾输出: {' | '.join(tail)}"

    if expect == "stdout_contains":
        text = args.get("text")
        if not text:
            return False, "stdout_contains 缺少 args.text"
        if text in combined:
            return True, f"输出含 {text!r}"
        return False, f"输出不含 {text!r}（退出码 {res['returncode']}）"

    if expect == "test_passes":
        marks = TEST_RESULT_RX.findall(combined)
        if not marks:
            tail = combined.strip().splitlines()[-3:]
            return False, f"未解析到 'test result:' 行（退出码 {res['returncode']}）; {' | '.join(tail)}"
        failed = [m for m in marks if m != "ok"]
        if failed:
            return False, f"{len(failed)}/{len(marks)} 个测试二进制 FAILED"
        # 防「0 passed」假阳性：用例名写错时 libtest 仍输出 ok 但 0 passed，
        # 退出码 0 且含 'test result:' 行，朴素判定会误判通过。必须确有用例执行过。
        passed_total = sum(int(n) for n in TEST_PASSED_RX.findall(combined))
        if passed_total < 1:
            return False, "test_passes 但 0 passed（用例名可能写错，无任何用例执行）"
        if res["returncode"] != 0:
            return False, f"test result 全 ok 但退出码为 {res['returncode']}"
        return True, f"{len(marks)} 个测试二进制全部 ok（共 {passed_total} 用例通过）"

    return False, f"未知 expect 类型: {expect}"


# --------------------------------------------------------------------------
# 主评分流程
# --------------------------------------------------------------------------

def evaluate_entry(entry: dict, src: SourceCache, runner: ExecRunner) -> tuple[bool, str]:
    check = entry.get("check")
    if not isinstance(check, dict):
        return False, "条目缺少 check 对象"
    kind = check.get("kind")
    try:
        if kind == "grep":
            return run_grep(check, src)
        if kind == "struct_assert":
            return run_struct_assert(check, src)
        if kind == "exec":
            return run_exec(check, entry, runner)
    except Exception as exc:  # 任何执行器异常都记失败，不让整轮评测崩掉
        return False, f"执行器异常 {type(exc).__name__}: {exc}"
    return False, f"未知 check.kind: {kind}"


def score(entries: list[dict], repo: Path, target_dir: str | None,
          skip_exec: bool) -> dict:
    src = SourceCache(repo)
    runner = ExecRunner(repo, target_dir, enabled=not skip_exec)

    # --- 第一遍：T3 优先执行，供反作弊升级判定使用 ---
    order = sorted(range(len(entries)), key=lambda i: 0 if entries[i].get("tier") == "T3" else 1)

    raw: list[dict] = [None] * len(entries)  # type: ignore[list-item]
    passed_t3_keys: set[str] = set()

    for idx in order:
        entry = entries[idx]
        ok, reason = evaluate_entry(entry, src, runner)
        raw[idx] = {"passed": ok, "reason": reason}
        if ok and entry.get("tier") == "T3":
            key = entry.get("assert_key")
            if key:
                passed_t3_keys.add(key)

    # --- 第二遍：应用反作弊系数并累加 ---
    domains: dict[str, dict] = {}
    results: list[dict] = []
    upgraded = 0

    for entry, r in zip(entries, raw):
        domain = entry.get("domain") or "UNKNOWN"
        tier = entry.get("tier") or "T1"
        weight = float(entry.get("weight", 1.0))
        assert_key = entry.get("assert_key") or ""

        # 反作弊：T1 单独最高 0.5；同 assert_key 有通过的 T3 才升到 1.0
        if tier == "T1":
            upgrade = assert_key in passed_t3_keys
            coeff = 1.0 if upgrade else 0.5
            if upgrade and r["passed"]:
                upgraded += 1
        else:
            coeff = 1.0

        earned = weight * coeff if r["passed"] else 0.0
        # 分母用条目满权重（不打折），使 T1-only 的域天然拿不到满分
        maximum = weight

        d = domains.setdefault(domain, {
            "id": domain,
            "name": DOMAIN_NAME.get(domain, domain),
            "quota": DOMAIN_QUOTA.get(domain, 0),
            "earned": 0.0, "max": 0.0,
            "n_total": 0, "n_passed": 0,
            "tier_counts": {"T1": 0, "T2": 0, "T3": 0},
            "tier_passed": {"T1": 0, "T2": 0, "T3": 0},
        })
        d["earned"] += earned
        d["max"] += maximum
        d["n_total"] += 1
        if tier in d["tier_counts"]:
            d["tier_counts"][tier] += 1
        if r["passed"]:
            d["n_passed"] += 1
            if tier in d["tier_passed"]:
                d["tier_passed"][tier] += 1

        results.append({
            "id": entry.get("id"),
            "domain": domain,
            "cluster": entry.get("cluster"),
            "assert_key": assert_key,
            "view": entry.get("view"),
            "tier": tier,
            "desc": entry.get("desc"),
            "passed": r["passed"],
            "coefficient": coeff,
            "earned": round(earned, 4),
            "max": round(maximum, 4),
            "reason": r["reason"],
        })

    # --- 域得分归一化到 100 分总分 ---
    total_quota = sum(DOMAIN_QUOTA.values())
    total_score = 0.0
    domains_out = []
    for did in sorted(set(list(DOMAIN_QUOTA.keys()) + list(domains.keys()))):
        d = domains.get(did)
        quota = DOMAIN_QUOTA.get(did, 0)
        domain_weight = quota / total_quota * 100 if total_quota else 0.0
        if d is None:
            # 域完全没有条目：诚实记 0 分，并标注原因
            domains_out.append({
                "id": did, "name": DOMAIN_NAME.get(did, did), "quota": quota,
                "earned": 0.0, "max": 0.0, "ratio": 0.0,
                "domain_weight": round(domain_weight, 2), "weighted_score": 0.0,
                "n_total": 0, "n_passed": 0,
                "tier_counts": {"T1": 0, "T2": 0, "T3": 0},
                "tier_passed": {"T1": 0, "T2": 0, "T3": 0},
                "note": "该域无条目，记 0 分",
            })
            continue
        ratio = d["earned"] / d["max"] if d["max"] else 0.0
        weighted = ratio * domain_weight
        total_score += weighted
        entry_out = dict(d)
        entry_out["earned"] = round(d["earned"], 3)
        entry_out["max"] = round(d["max"], 3)
        entry_out["ratio"] = round(ratio, 4)
        entry_out["domain_weight"] = round(domain_weight, 2)
        entry_out["weighted_score"] = round(weighted, 3)
        if d["n_total"] < quota * 0.9:
            entry_out["note"] = f"条目数 {d['n_total']} 低于配额 {quota} 的 90%"
        domains_out.append(entry_out)

    strong = [d["id"] for d in domains_out if d["ratio"] >= STRONG_THRESHOLD]
    ok_list = [d["id"] for d in domains_out if PASS_THRESHOLD <= d["ratio"] < STRONG_THRESHOLD]
    weak = [d["id"] for d in domains_out if d["ratio"] < PASS_THRESHOLD]

    failures = [r for r in results if not r["passed"]]

    return {
        "benchmark": "mece_1000",
        "repo": str(repo),
        "total_score": round(total_score, 2),
        "total_max": 100.0,
        "n_entries": len(entries),
        "n_passed": sum(1 for r in results if r["passed"]),
        "n_failed": len(failures),
        "skip_exec": skip_exec,
        "anti_cheat": {
            "t1_base_coefficient": 0.5,
            "t1_upgraded_count": upgraded,
            "passed_t3_assert_keys": sorted(passed_t3_keys),
        },
        "exec_stats": runner.stats,
        "classification": {
            "strong": strong, "acceptable": ok_list, "weak": weak,
            "strong_threshold": STRONG_THRESHOLD, "pass_threshold": PASS_THRESHOLD,
        },
        "domains": domains_out,
        "entries": results,
        "failures": failures,
    }


# --------------------------------------------------------------------------
# 终端报告
# --------------------------------------------------------------------------

def print_report(result: dict, max_failures: int = 20) -> None:
    print("=" * 78)
    print("  mimofan 1000 题 MECE 能力基准")
    print(f"  仓库: {result['repo']}")
    if result["skip_exec"]:
        print("  [!] --skip-exec 模式：T3 条目全部记 0，总分不可用于结论")
    print("=" * 78)

    for d in result["domains"]:
        pct = d["ratio"] * 100
        bar_len = int(pct / 5)
        bar = "#" * bar_len + "." * (20 - bar_len)
        flag = "强" if d["ratio"] >= STRONG_THRESHOLD else ("达" if d["ratio"] >= PASS_THRESHOLD else "弱")
        print(f"{d['id']} {d['name']:<22} [{bar}] {pct:>5.1f}%  "
              f"{d['weighted_score']:>5.2f}/{d['domain_weight']:<5.2f} "
              f"[{flag}] {d['n_passed']}/{d['n_total']}")
        tc, tp = d["tier_counts"], d["tier_passed"]
        print(f"       T1 {tp['T1']}/{tc['T1']}  T2 {tp['T2']}/{tc['T2']}  T3 {tp['T3']}/{tc['T3']}"
              + (f"   ! {d['note']}" if d.get("note") else ""))

    print("-" * 78)
    cls = result["classification"]
    print(f"  强项 (≥80%): {', '.join(cls['strong']) or '无'}")
    print(f"  达标 (60-80%): {', '.join(cls['acceptable']) or '无'}")
    print(f"  短板 (<60%): {', '.join(cls['weak']) or '无'}")
    print("-" * 78)
    ac = result["anti_cheat"]
    print(f"  反作弊: T1 基础系数 0.5，因同 assert_key 的 T3 通过而升级的 T1 条目 {ac['t1_upgraded_count']} 条")
    es = result["exec_stats"]
    print(f"  exec: 实际执行 {es['executed']} 次，缓存命中 {es['cache_hits']} 次，超时 {es['timeouts']} 次")
    print("-" * 78)

    failures = result["failures"]
    if failures:
        print(f"  失败条目样例（共 {len(failures)} 条，显示前 {min(max_failures, len(failures))} 条）:")
        for f in failures[:max_failures]:
            print(f"    [{f['tier']}] {f['id']:<12} {(f['desc'] or '')[:38]:<38} | {f['reason'][:60]}")
    print("-" * 78)
    print(f"  总分: {result['total_score']:.2f} / 100.00   "
          f"（通过 {result['n_passed']}/{result['n_entries']} 条）")
    print("=" * 78)


def main() -> int:
    ap = argparse.ArgumentParser(description="1000 题 MECE 能力基准分层评分引擎")
    ap.add_argument("--repo", default=str(Path(__file__).resolve().parents[2]),
                    help="被测仓库路径（可指向不同 commit 的 worktree 做前后对比）")
    ap.add_argument("--entries", default=str(DEFAULT_ENTRIES_DIR),
                    help="条目目录，需含 part_*.json")
    ap.add_argument("--json", dest="json_out", default=None, help="完整结果 JSON 输出路径")
    ap.add_argument("--skip-exec", action="store_true", help="跳过 T3 执行（快速调试模式）")
    ap.add_argument("--target-dir", default=None,
                    help="注入 CARGO_TARGET_DIR，避免多 agent 并行抢 cargo 锁")
    ap.add_argument("--max-failures", type=int, default=20, help="终端报告显示的失败条目数")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    repo = Path(args.repo).resolve()
    if not repo.exists():
        raise SystemExit(f"仓库路径不存在: {repo}")

    entries = load_entries(Path(args.entries))
    result = score(entries, repo, args.target_dir, args.skip_exec)

    if not args.quiet:
        print_report(result, args.max_failures)
    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"\n已写入: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
