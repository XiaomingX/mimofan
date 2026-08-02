"""harness.py — core engine for horizontal model comparison.

Talks to any OpenAI-compatible `/chat/completions` endpoint and measures:
  * performance : latency (mean/p50/p95), time-to-first-token (TTFT),
                  throughput (tokens/s)
  * output quality : heuristic richness, optional reference-based accuracy,
                     optional LLM-as-judge score
  * consistency  : run-to-run self-consistency, cross-model agreement
  * time         : wall-clock per call

Pure standard library (Python 3.11+). No third-party dependencies.
"""
from __future__ import annotations

import json
import re
import statistics
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Optional

# --------------------------------------------------------------------------
# Results
# --------------------------------------------------------------------------


@dataclass
class RunResult:
    model: str
    prompt_id: str
    category: str
    repeat: int
    text: str = ""
    latency_s: float = 0.0
    ttft_s: Optional[float] = None
    prompt_tokens: int = 0
    completion_tokens: int = 0
    quality_heuristic: float = 0.0
    error: str = ""
    timestamp: str = ""


# --------------------------------------------------------------------------
# Client (OpenAI-compatible)
# --------------------------------------------------------------------------


class ModelClient:
    """Minimal OpenAI-compatible chat client with latency instrumentation."""

    def __init__(
        self,
        name: str,
        endpoint: str,
        api_key: str,
        model: str,
        timeout: int = 120,
        stream: bool = False,
    ) -> None:
        self.name = name
        self.endpoint = endpoint.rstrip("/")
        self.api_key = api_key
        self.model = model
        self.timeout = timeout
        self.stream = stream

    def complete(self, prompt: str, temperature: float = 0.0) -> RunResult:
        res = RunResult(model=self.name, prompt_id="", category="", repeat=0)
        url = self.endpoint + "/chat/completions"
        body = {
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": temperature,
            "stream": self.stream,
        }
        data = json.dumps(body).encode()
        req = urllib.request.Request(url, data=data, method="POST")
        req.add_header("Authorization", f"Bearer {self.api_key}")
        req.add_header("Content-Type", "application/json")

        t0 = time.perf_counter()
        try:
            if self.stream:
                text, usage, ttft = self._complete_stream(req, t0)
            else:
                with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                    raw = resp.read().decode()
                text, pt, ct = self._complete_nonstream(raw)
                usage = {"prompt_tokens": pt, "completion_tokens": ct}
                ttft = None
            latency = time.perf_counter() - t0
            res.text = text
            res.latency_s = latency
            res.ttft_s = ttft
            res.prompt_tokens = int(usage.get("prompt_tokens", 0) or 0)
            res.completion_tokens = int(usage.get("completion_tokens", 0) or 0)
        except urllib.error.HTTPError as e:
            res.latency_s = time.perf_counter() - t0
            detail = ""
            try:
                detail = e.read().decode()[:200]
            except Exception:
                pass
            res.error = f"HTTP {e.code}: {detail}"
        except Exception as e:  # network / timeout / json errors
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:240]}"
        return res

    # -- internals ---------------------------------------------------------

    def _complete_nonstream(self, raw: str):
        obj = json.loads(raw)
        text = obj["choices"][0]["message"]["content"]
        usage = obj.get("usage") or {}
        pt = int(usage.get("prompt_tokens", 0) or 0)
        ct = int(usage.get("completion_tokens", 0) or 0)
        return text, pt, ct

    def _complete_stream(self, req, t0):
        buf = b""
        ttft: Optional[float] = None
        pieces: list[str] = []
        last_usage: dict = {}
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                for chunk in resp:
                    buf += chunk
                    while b"\n" in buf:
                        line, buf = buf.split(b"\n", 1)
                        line = line.strip()
                        if not line.startswith(b"data:"):
                            continue
                        payload = line[5:].strip()
                        if payload == b"[DONE]":
                            continue
                        try:
                            obj = json.loads(payload)
                        except Exception:
                            continue
                        delta = obj.get("choices", [{}])[0].get("delta", {}).get("content")
                        if delta:
                            if ttft is None:
                                ttft = time.perf_counter() - t0
                            pieces.append(delta)
                        if obj.get("usage"):
                            last_usage = obj["usage"]
            text = "".join(pieces)
            pt = int(last_usage.get("prompt_tokens", 0) or 0)
            ct = int(last_usage.get("completion_tokens", 0) or 0)
            return text, {"prompt_tokens": pt, "completion_tokens": ct}, ttft
        except urllib.error.HTTPError as e:
            detail = ""
            try:
                detail = e.read().decode()[:200]
            except Exception:
                pass
            raise RuntimeError(f"HTTP {e.code}: {detail}")
        except Exception as e:
            raise


class MockClient:
    """Offline client for `verify` (acceptance). Deterministic, no network."""

    def __init__(self, name: str, endpoint: str, api_key: str, model: str, **_kw) -> None:
        self.name = name
        self.endpoint = endpoint
        self.api_key = api_key
        self.model = model

    def complete(self, prompt: str, temperature: float = 0.0) -> RunResult:
        h = hash((self.name, prompt)) % 1000
        text = (
            f"[mock:{self.name}] Answer to: {prompt[:60]}. "
            f"Step 1 reasoning. Step 2 reasoning. "
            f"Conclusion={'YES' if h % 2 == 0 else 'NO'} (seed {h})."
        )
        return RunResult(
            model=self.name,
            prompt_id="",
            category="",
            repeat=0,
            text=text,
            latency_s=round(0.2 + (h % 50) / 100.0, 3),
            ttft_s=round(0.05 + (h % 10) / 200.0, 3),
            prompt_tokens=len(prompt.split()),
            completion_tokens=len(text.split()),
            error="",
        )


# --------------------------------------------------------------------------
# Runner
# --------------------------------------------------------------------------


def run_all(models, prompts, repeat: int, timeout: int, stream: bool, verbose=True):
    """Run every prompt against every model `repeat` times. Returns RunResult list."""
    results: list[RunResult] = []
    total = len(models) * len(prompts) * repeat
    done = 0
    for m in models:
        client = m["_client"]
        for p in prompts:
            for r in range(repeat):
                res = client.complete(p["prompt"])
                res.prompt_id = p.get("id", "")
                res.category = p.get("category", "")
                res.repeat = r
                res.quality_heuristic = heuristic_quality(res.text)
                results.append(res)
                done += 1
                if verbose:
                    status = "ERR" if res.error else f"{res.latency_s:.2f}s"
                    print(
                        f"  [{done}/{total}] {m['name']:>14} | {res.prompt_id:<10} "
                        f"| r{r} | {status}",
                        flush=True,
                    )
    return results


# --------------------------------------------------------------------------
# Quality / similarity helpers
# --------------------------------------------------------------------------


def _norm(s: str) -> str:
    return re.sub(r"\s+", " ", s.strip().lower())


def exact_match(a: str, b: str) -> bool:
    return _norm(a) == _norm(b)


def token_jaccard(a: str, b: str) -> float:
    sa, sb = set(_norm(a).split()), set(_norm(b).split())
    if not sa and not sb:
        return 1.0
    if not sa or not sb:
        return 0.0
    return len(sa & sb) / len(sa | sb)


def rouge_l_f1(a: str, b: str) -> float:
    """ROUGE-L F1 on token level (LCS-based)."""
    x, y = _norm(a).split(), _norm(b).split()
    m, n = len(x), len(y)
    if m == 0 or n == 0:
        return 1.0 if m == n else 0.0
    # LCS length via DP
    dp = [[0] * (n + 1) for _ in range(m + 1)]
    for i in range(1, m + 1):
        for j in range(1, n + 1):
            if x[i - 1] == y[j - 1]:
                dp[i][j] = dp[i - 1][j - 1] + 1
            else:
                dp[i][j] = max(dp[i - 1][j], dp[i][j - 1])
    lcs = dp[m][n]
    prec = lcs / n
    rec = lcs / m
    if prec + rec == 0:
        return 0.0
    return 2 * prec * rec / (prec + rec)


def heuristic_quality(text: str) -> float:
    """Lightweight format-richness proxy (0-1). Honest: heuristic only."""
    if not text:
        return 0.0
    score = 0.0
    if "```" in text:
        score += 0.30
    if any(tok in text for tok in ("- ", "* ", "1. ", "• ")):
        score += 0.20
    if text.count("\n") >= 3:
        score += 0.10
    length = len(text)
    if 50 <= length <= 4000:
        score += 0.40
    elif length > 0:
        score += 0.20
    return round(min(score, 1.0), 3)


def judge_score(judge_client, prompt: str, answer: str) -> Optional[int]:
    """LLM-as-judge (MT-Bench style): 1-10 quality score."""
    user = (
        "You are a strict evaluator. Rate the answer quality from 1 (poor) to "
        "10 (excellent) for the given prompt.\n\n"
        f"PROMPT:\n{prompt}\n\nANSWER:\n{answer}\n\n"
        "Reply with ONLY an integer from 1 to 10, then a short reason."
    )
    try:
        res = judge_client.complete(user)
        m = re.search(r"\b(10|[1-9])\b", res.text)
        return int(m.group(1)) if m else None
    except Exception:
        return None


# --------------------------------------------------------------------------
# Metrics aggregation
# --------------------------------------------------------------------------


def _pctl(xs: list[float], q: float) -> float:
    xs = sorted(xs)
    if not xs:
        return 0.0
    if len(xs) == 1:
        return xs[0]
    k = (len(xs) - 1) * q
    f = int(k)
    c = min(f + 1, len(xs) - 1)
    if f == c:
        return float(xs[f])
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def _group(results: list[RunResult], key):
    out: dict = {}
    for r in results:
        out.setdefault(key(r), []).append(r)
    return out


def compute_model_metrics(results: list[RunResult], prompts, judge_client=None):
    """Return {model_name: {metric: value, ...}, ...}."""
    by_model = _group(results, lambda r: r.model)
    by_pid = {p.get("id"): p for p in prompts}
    metrics: dict = {}
    for model, runs in by_model.items():
        ok = [r for r in runs if not r.error]
        lat = [r.latency_s for r in ok]
        ttft = [r.ttft_s for r in ok if r.ttft_s is not None]
        tps = [
            r.completion_tokens / r.latency_s
            for r in ok
            if r.latency_s > 0 and r.completion_tokens > 0
        ]
        heur = [r.quality_heuristic for r in ok]
        ref_acc = []
        judge_scores = []
        for r in ok:
            p = by_pid.get(r.prompt_id, {})
            ref = p.get("reference")
            if ref and p.get("type") in ("classify", "short", "extract"):
                ref_acc.append(1.0 if exact_match(r.text, ref) else rouge_l_f1(r.text, ref))
            if judge_client is not None:
                j = judge_score(judge_client, p.get("prompt", ""), r.text)
                if j is not None:
                    judge_scores.append(j)
        # self-consistency across repeats
        by_prompt = _group([r for r in ok if r.repeat >= 0], lambda r: r.prompt_id)
        consist = []
        for pid, grp in by_prompt.items():
            if len(grp) > 1:
                sims = []
                for i in range(len(grp)):
                    for j in range(i + 1, len(grp)):
                        sims.append(token_jaccard(grp[i].text, grp[j].text))
                if sims:
                    consist.append(statistics.mean(sims))
        metrics[model] = {
            "n_runs": len(runs),
            "n_errors": len(runs) - len(ok),
            "avg_latency_s": round(statistics.mean(lat), 3) if lat else 0.0,
            "p50_latency_s": round(_pctl(lat, 0.50), 3) if lat else 0.0,
            "p95_latency_s": round(_pctl(lat, 0.95), 3) if lat else 0.0,
            "avg_ttft_s": round(statistics.mean(ttft), 3) if ttft else None,
            "avg_throughput_tps": round(statistics.mean(tps), 2) if tps else None,
            "quality_heuristic": round(statistics.mean(heur), 3) if heur else 0.0,
            "ref_accuracy": round(statistics.mean(ref_acc), 3) if ref_acc else None,
            "quality_judge": round(statistics.mean(judge_scores), 2) if judge_scores else None,
            "self_consistency": round(statistics.mean(consist), 3) if consist else None,
        }
    return metrics


def compute_cross_model(results: list[RunResult], prompts):
    """Inter-model agreement on classifiable prompts (majority-vote)."""
    by_pid = {p.get("id"): p for p in prompts}
    by_prompt = _group(results, lambda r: r.prompt_id)
    agreements = []
    per_prompt = []
    for pid, grp in by_prompt.items():
        p = by_pid.get(pid, {})
        if p.get("type") not in ("classify", "short", "extract") and not p.get("reference"):
            continue
        answers = {}
        for r in grp:
            if r.error:
                continue
            answers.setdefault(r.model, _norm(r.text))
        if len(answers) < 2:
            continue
        vals = list(answers.values())
        majority = max(set(vals), key=vals.count)
        agree = sum(1 for v in vals if v == majority) / len(vals)
        agreements.append(agree)
        per_prompt.append({"prompt_id": pid, "agreement": round(agree, 3), "n_models": len(answers)})
    return {
        "inter_model_agreement": round(statistics.mean(agreements), 3) if agreements else None,
        "per_prompt": per_prompt,
    }
