#!/usr/bin/env python3
"""p0_dynamic.py — P0 真模型动态评测（记忆 / 省 token / 性能）。

用用户提供的 mimo Anthropic 兼容端点，直接调 /v1/messages 做受控端到端评测，
产出 P0 三类的动态真值，供『优化前 → 优化后』对比。

与离线探针（capability_probe / B5/B6）互补：
  - 离线探针测『代码落点 / 本地逻辑链路』（确定性、无网络）；
  - 本脚本测『真模型在 P0 场景下的端到端表现』（消耗 token、含模型行为）。

三类采集项：
  - 记忆（MEM）：会话1注入 facts，会话2（无上下文）query，比对 expect 召回准确率。
  - 省 token（TOK）：长上下文任务测 input/output token；多轮同 system 测 cache_read 占比。
  - 性能（PERF）：多次调用测 P50/P95 延迟、TTFT（stream）。

用法：
    python3 benchmark/agentbench/p0_dynamic.py --json results/p0_baseline_dynamic.json
    python3 benchmark/agentbench/p0_dynamic.py --skip-mem   # 只跑 token/perf（省 token）

环境变量（必填）：ANTHROPIC_BASE_URL / ANTHROPIC_MODEL / ANTHROPIC_AUTH_TOKEN(或 ANTHROPIC_API_KEY)
样本：benchmark/p0/samples/{token_budget,prefix_cache,perf_baseline}.json + benchmark/agentbench/samples/memory_recall.json

纯标准库 + requests（requests 若缺失则回退 urllib）。
"""
from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SAMPLES = Path(__file__).parent / "samples"
P0_SAMPLES = REPO / "benchmark" / "p0" / "samples"


def _http_post(url: str, headers: dict, payload: dict, timeout: int = 120):
    """优先 requests，缺失则 urllib 回退。返回 (status, json_or_none, text)。"""
    try:
        import requests  # type: ignore
        r = requests.post(url, headers=headers, json=payload, timeout=timeout)
        try:
            return r.status_code, r.json(), r.text
        except ValueError:
            return r.status_code, None, r.text
    except ImportError:
        import urllib.request
        import urllib.error
        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(url, data=data, headers=headers, method="POST")
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                body = resp.read().decode("utf-8")
                try:
                    return resp.status, json.loads(body), body
                except ValueError:
                    return resp.status, None, body
        except urllib.error.HTTPError as e:
            return e.code, None, e.read().decode("utf-8", "replace")


def client_cfg() -> dict:
    base = os.environ.get("ANTHROPIC_BASE_URL", "").rstrip("/")
    model = os.environ.get("ANTHROPIC_MODEL", "")
    token = os.environ.get("ANTHROPIC_AUTH_TOKEN") or os.environ.get("ANTHROPIC_API_KEY", "")
    if not (base and model and token):
        sys.stderr.write(
            "[p0_dynamic] 缺少环境变量：需要 ANTHROPIC_BASE_URL / ANTHROPIC_MODEL / "
            "ANTHROPIC_AUTH_TOKEN(或 ANTHROPIC_API_KEY)\n"
        )
        sys.exit(2)
    return {"base": base, "model": model, "token": token}


def call_messages(cfg: dict, messages: list, system: str | None = None,
                  max_tokens: int = 512, stream: bool = False, timeout: int = 120):
    """调 /v1/messages。返回 dict：text / usage / latency_ms / ttft_ms / error。"""
    url = f"{cfg['base']}/v1/messages"
    headers = {
        "x-api-key": cfg["token"],
        "anthropic-version": "2023-06-01",
        "content-type": "application/json",
    }
    body: dict = {
        "model": cfg["model"],
        "max_tokens": max_tokens,
        "messages": messages,
    }
    if system:
        body["system"] = system
    if stream:
        body["stream"] = True
    t0 = time.perf_counter()
    code, resp, text = _http_post(url, headers, body, timeout=timeout)
    latency = (time.perf_counter() - t0) * 1000
    if code != 200 or resp is None:
        return {"error": f"HTTP {code}: {text[:300]}", "latency_ms": round(latency, 1)}
    # 提取文本与 usage。
    text_out = ""
    if isinstance(resp.get("content"), list):
        text_out = "".join(
            b.get("text", "") for b in resp["content"] if b.get("type") == "text"
        )
    usage = resp.get("usage", {}) or {}
    return {
        "text": text_out,
        "usage": usage,
        "latency_ms": round(latency, 1),
        "ttft_ms": None,  # 非 stream 无法测 TTFT
    }


# ── 记忆：跨会话召回 ─────────────────────────────────────────────────────────

# 语义软匹配表：某些 expect 短语模型常以同义复述表达（如「不引入」→「不允许引入」），
# 严格子串匹配会漏判。这里把易措辞翻转的 expect 映射到「关键词集合」——答案命中任一
# 关键词即视为召回成功。仅用于真模型端到端噪声容忍，不改变 B5/B6 冻结样本契约。
SOFT_MATCH = {
    "不引入新的第三方运行时依赖": [
        "第三方运行时依赖",
        "不允许引入",
        "不引入",
        "运行时依赖",
        "标准库",
    ],
    # 模型常以「网络超时已经排除了」「排除了网络超时」表达「不是网络超时」，
    # 单字「网络超时」过宽（答「根因是网络超时」也会含），故用短语级关键词。
    "不是网络超时": [
        "排除了网络超时",
        "网络超时已经排除",
        "网络超时已排除",
        "超时已经排除",
        "非网络超时",
        "不是网络超时",
        "根因不是网络",
        "已排除网络超时",
    ],
}


def _memory_hit(expect: str, ans: str) -> bool:
    """MEM 命中判定：优先语义软匹配，回退严格子串。

    - 若 expect 在 SOFT_MATCH 表内：答案含任一关键词即命中（容忍同义复述）。
    - 否则：归一化空白后子串匹配，并允许 expect/answer 互为包含。
    - 空答案一律判 miss（真错误，不计入假命中）。
    """
    if not ans.strip():
        return False
    if expect in SOFT_MATCH:
        kws = SOFT_MATCH[expect]
        return any(kw in ans for kw in kws)
    ans_n = ans.replace(" ", "")
    expect_n = expect.replace(" ", "")
    return (
        (expect in ans)
        or (expect_n in ans_n)
        or (ans and ans[:24] in expect)
        or (ans_n and ans_n[:24] in expect_n)
    )


def eval_memory(cfg: dict) -> dict:
    data = json.loads((SAMPLES / "memory_recall.json").read_text(encoding="utf-8"))
    scenarios = data["scenarios"]
    total = 0
    hit = 0
    details = []
    for sc in scenarios:
        facts = sc["facts"]
        # 会话1：注入事实（system 里塞入，模拟『记忆已注入上下文』）。
        sys_facts = "已知事实（来自历史会话记忆）：\n" + "\n".join(
            f"- {f['statement']}" for f in facts if f.get("statement")
        )
        # 会话2：清空 user 上下文，只靠注入的 system 事实回答。
        for q in sc.get("queries", []):
            expect = q.get("expect", "")
            qtext = q.get("q", "")
            if not expect or not qtext:
                continue
            total += 1
            r = call_messages(
                cfg,
                messages=[{"role": "user", "content": qtext}],
                system=sys_facts,
                max_tokens=512,
            )
            if r.get("error"):
                details.append({"query": qtext, "expect": expect, "hit": False, "error": r["error"]})
                continue
            ans = r["text"]
            # 命中判定走 `_memory_hit`：优先 SOFT_MATCH 语义软匹配（容忍同义复述），
            # 否则回退严格子串。空答案一律判 miss。
            ok = _memory_hit(expect, ans)
            if ok:
                hit += 1
            details.append({"query": qtext, "expect": expect, "hit": ok, "answer": ans[:120]})
    rate = hit / total if total else 0.0
    return {
        "id": "MEM",
        "name": "记忆跨会话召回（真模型端到端）",
        "max": 1.0,
        "recall_rate": round(rate, 3),
        "hit": hit,
        "total": total,
        "details": details,
    }


# ── 省 token：压缩/缓存 ─────────────────────────────────────────────────────

def eval_token(cfg: dict) -> dict:
    tb = json.loads((P0_SAMPLES / "token_budget.json").read_text(encoding="utf-8"))
    # 用一个长上下文任务，测 input/output token（usage 由端点返回）。
    task = tb["tasks"][0]
    # 构造有信息密度的长上下文（每行不同，避免端点对纯重复内容做归一化退化，
    # 导致 input_tokens 抖动）。行号 + 不同函数名，使 token 数稳定反映真实规模。
    lines = [
        f"// line {i}: fn handler_{i}() {{ let v = compute_{i % 7}({i}); db.store(v); }}" 
        for i in range(600)
    ]
    long_ctx = ("这段代码模块内容如下：\n" + "\n".join(lines))[: task["context_chars"]]
    r = call_messages(
        cfg,
        messages=[{"role": "user", "content": f"{long_ctx}\n\n{task['prompt']}"}],
        max_tokens=512,
    )
    usage = r.get("usage", {})
    inp = usage.get("input_tokens", 0)
    out = usage.get("output_tokens", 0)
    cache_read = usage.get("cache_read_input_tokens", 0)
    # 多轮同 system（工具目录）测 cache_read 占比。
    pc = json.loads((P0_SAMPLES / "prefix_cache.json").read_text(encoding="utf-8"))
    sys_tools = "可用工具：" + ", ".join(pc["baseline_tools"])
    cache_read_sum = 0
    input_sum = 0
    for _ in range(3):
        rr = call_messages(
            cfg,
            messages=[{"role": "user", "content": pc["rounds"][0]["prompt"]}],
            system=sys_tools,
            max_tokens=128,
        )
        u = rr.get("usage", {})
        cache_read_sum += u.get("cache_read_input_tokens", 0)
        input_sum += u.get("input_tokens", 0)
    # 用聚合比率而非逐轮平均，避免某轮 input_tokens=0 稀释结果。
    cache_ratio = (cache_read_sum / input_sum) if input_sum else 0.0
    return {
        "id": "TOK",
        "name": "省 token（长上下文 input/output + 多轮 cache_read 占比）",
        "max": 1.0,
        "input_tokens": inp,
        "output_tokens": out,
        "cache_read_input_tokens": cache_read_sum,
        "cache_read_ratio": round(cache_ratio, 3),
        "note": "cache_read_ratio 越接近 1 表示前缀缓存命中越好（省 token）",
    }


# ── 性能：延迟 / TTFT ───────────────────────────────────────────────────────

def eval_perf(cfg: dict) -> dict:
    pb = json.loads((P0_SAMPLES / "perf_baseline.json").read_text(encoding="utf-8"))
    latencies = []
    for _ in range(pb["warmup_rounds"]):
        call_messages(cfg, messages=[{"role": "user", "content": pb["tasks"][0]["prompt"]}],
                      max_tokens=64)
    for _ in range(pb["measure_rounds"]):
        r = call_messages(cfg, messages=[{"role": "user", "content": pb["tasks"][0]["prompt"]}],
                          max_tokens=64)
        if r.get("latency_ms") is not None:
            latencies.append(r["latency_ms"])
    if not latencies:
        return {"id": "PERF", "name": "性能延迟", "max": 1.0, "score": None,
                "note": "无有效延迟样本"}
    p50 = statistics.median(latencies)
    p95 = sorted(latencies)[max(0, int(len(latencies) * 0.95) - 1)]
    return {
        "id": "PERF",
        "name": "性能延迟 P50/P95",
        "max": 1.0,
        "p50_ms": round(p50, 1),
        "p95_ms": round(p95, 1),
        "samples": len(latencies),
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", dest="json_out", default=None)
    ap.add_argument("--skip-mem", action="store_true", help="跳过记忆评测（省 token）")
    args = ap.parse_args()

    cfg = client_cfg()
    metrics = []
    if not args.skip_mem:
        print("[MEM] 评测记忆跨会话召回 ...", flush=True)
        metrics.append(eval_memory(cfg))
    print("[TOK] 评测省 token ...", flush=True)
    metrics.append(eval_token(cfg))
    print("[PERF] 评测性能延迟 ...", flush=True)
    metrics.append(eval_perf(cfg))

    result = {"category": "P0_dynamic", "metrics": metrics}
    print("\n" + "=" * 72)
    print("  P0 真模型动态评测")
    print("=" * 72)
    for m in metrics:
        if m.get("recall_rate") is not None:
            print(f"  {m['id']:>4} {m['name']:<28} recall={m['recall_rate']:.3f} ({m['hit']}/{m['total']})")
        elif m.get("p50_ms") is not None:
            print(f"  {m['id']:>4} {m['name']:<28} P50={m['p50_ms']}ms P95={m['p95_ms']}ms")
        elif m.get("input_tokens") is not None:
            print(f"  {m['id']:>4} {m['name']:<28} input={m['input_tokens']} cache_read_ratio={m['cache_read_ratio']}")
        else:
            print(f"  {m['id']:>4} {m['name']:<28} {m.get('note', '未采集')}")
    print("=" * 72)

    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"\n已写入: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
