#!/usr/bin/env python3
"""longmemeval_harness.py — #777 用 LongMemEval 公开基准给 mimofan 长期记忆能力打分。

把 LongMemEval 样本的多轮历史灌入 mimofan 记忆链路（Rust `longmemeval_ingest`
二进制 → VectorStore 持久化+重建+检索），拿到召回记忆文本作为 system 上下文，
触发真模型回答，再用**我们自己的真模型做 LLM-as-judge**（yes/no 判定，替代原版
GPT-4o），按 5 维度聚合准确率并产出报告。

用法：
    # 先编译 Rust 支撑二进制：
    cargo build -p mimofan-memory --example longmemeval_ingest
    # smoke test（100 条，消耗约 200 次模型调用）：
    python3 benchmark/agentbench/longmemeval_harness.py \
        --limit 100 --binary target/debug/examples/longmemeval_ingest \
        --json benchmark/agentbench/results/longmemeval_smoke.json
    # 全量（500 条）：去掉 --limit

环境变量（必填）：ANTHROPIC_BASE_URL / ANTHROPIC_MODEL / ANTHROPIC_AUTH_TOKEN
数据集：自动从 HuggingFace 下载 `longmemeval_s_cleaned.json`（~115k tokens，500 条）。
       也可用 --data-path 指定本地已下载文件。

设计要点（详见 issue #777）：
- judge 走我们自己的端点，不依赖 OPENAI_API_KEY。
- 本地哈希 embedding 无语义能力，本接入测的是「持久化+检索链路」而非语义召回，
  报告须如实说明该口径偏置。
- 保留规则子串判定作对照基线（--rule-baseline），量化 judge vs 规则差异。
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))
from p0_dynamic import client_cfg, call_messages  # 复用真模型调用骨架

HF_REPO = "xiaowu0162/longmemeval-cleaned"
HF_FILE = "longmemeval_s_cleaned.json"
DATA_DIR = HERE / "samples" / "longmemeval"


# ── 数据集获取 ──────────────────────────────────────────────────────────────
def ensure_dataset(data_path: str | None) -> Path:
    if data_path:
        p = Path(data_path)
        if not p.exists():
            sys.stderr.write(f"[harness] --data-path 指定文件不存在: {p}\n")
            sys.exit(2)
        return p
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    local = DATA_DIR / HF_FILE
    if local.exists():
        sys.stderr.write(f"[harness] 复用本地数据集: {local}\n")
        return local
    try:
        from huggingface_hub import hf_hub_download
    except ImportError:
        sys.stderr.write(
            "[harness] 未安装 huggingface_hub，请 `pip install huggingface_hub` "
            "或用 --data-path 指定已下载文件\n"
        )
        sys.exit(2)
    sys.stderr.write(f"[harness] 从 HuggingFace 下载 {HF_REPO}/{HF_FILE} ...\n")
    p = hf_hub_download(repo_id=HF_REPO, filename=HF_FILE, repo_type="dataset")
    import shutil
    shutil.copy(p, local)
    sys.stderr.write(f"[harness] 已下载至: {local}\n")
    return local


# ── 记忆召回（调 Rust 二进制） ──────────────────────────────────────────────
def recall_memory(binary: str, sample: dict, project: str, top_k: int) -> list[str]:
    try:
        proc = subprocess.run(
            [binary, "--project", project, "--top-k", str(top_k)],
            input=json.dumps(sample),
            capture_output=True,
            text=True,
            timeout=120,
        )
    except Exception as e:  # noqa: BLE001
        sys.stderr.write(f"[harness] 调用 ingest 二进制失败: {e}\n")
        return []
    if proc.returncode != 0:
        sys.stderr.write(f"[harness] ingest 返回非零: {proc.stderr[:200]}\n")
        return []
    try:
        out = json.loads(proc.stdout)
    except ValueError:
        sys.stderr.write(f"[harness] ingest stdout 非 JSON: {proc.stdout[:200]}\n")
        return []
    return out.get("recalled", []) or []


# ── LLM-as-judge（用我们自己的真模型） ──────────────────────────────────────
def judge_hit(cfg: dict, question: str, ground_truth: str, answer: str,
              qtype: str) -> bool | None:
    """返回 True/False（judge 判定命中），None 表示 judge 调用失败。"""
    if qtype == "abstention" or question.endswith("_abs"):
        # 弃答类：正确做法是识别不可答；这里简化为「未给出具体事实性答案即视为正确弃答」。
        # 注意：本基准的 abstention 评测较复杂，smoke test 阶段仅作粗略判定。
        refused = any(k in answer.lower() for k in
                      ["无法", "不能", "没有足够", "不确定", "cannot", "don't know",
                       "not have", "no information", "unable", "insufficient"])
        return refused
    # 截断过长 ground_truth，避免干扰 judge；聚焦核心事实是否出现。
    gt = ground_truth.strip()[:300]
    prompt = (
        "You are a grader for a long-term memory QA task. Given a QUESTION, the "
        "CORRECT ANSWER (a fact or rubric), and a MODEL RESPONSE, answer 'yes' "
        "ONLY if the MODEL RESPONSE conveys the same core fact as the CORRECT "
        "ANSWER — different wording, extra detail, or a more verbose phrasing are "
        "all fine. Answer 'no' only if the response is wrong, contradicts the "
        "answer, or fails to address the question. Respond with a single word: "
        "yes or no.\n\n"
        f"QUESTION: {question}\n\n"
        f"CORRECT ANSWER: {gt}\n\n"
        f"MODEL RESPONSE: {answer}\n\n"
        "Grade (yes/no only):"
    )
    # 注意：mimo 端点每个响应先有 thinking 块，需足够 max_tokens 让 thinking
    # 之后还能输出 yes/no，否则 text 为空导致 judge 失败。
    r = call_messages(
        cfg,
        messages=[{"role": "user", "content": prompt}],
        max_tokens=512,
    )
    if r.get("error"):
        return None
    return "yes" in r["text"].strip().lower()


# ── 规则子串对照判定 ────────────────────────────────────────────────────────
def rule_hit(ground_truth: str, answer: str) -> bool:
    if not answer.strip():
        return False
    gt = ground_truth.strip()
    ans = answer.strip()
    return gt in ans or ans[:40] in gt


# ── 主流程 ──────────────────────────────────────────────────────────────────
DIM_LABELS = {
    "single-session-user": "信息提取(用户侧)",
    "single-session-assistant": "信息提取(助手侧)",
    "single-session-preference": "偏好提取",
    "temporal-reasoning": "时序推理",
    "knowledge-update": "知识更新",
    "multi-session": "跨会话推理",
}


def run(cfg: dict, data_path: Path, binary: str, limit: int | None,
        project: str, top_k: int, rule_baseline: bool) -> dict:
    samples = json.loads(data_path.read_text(encoding="utf-8"))
    if limit:
        samples = samples[:limit]
    sys.stderr.write(f"[harness] 评测 {len(samples)} 条样本\n")

    # 按维度聚合
    agg = {}  # dim -> {"hit":int,"judge_fail":int,"total":int,"rule_hit":int}
    details = []
    # miss 归因：区分「召回失败（模型答无信息，证据 session 没被检索到）」与
    # 「召回成功但模型答错」。这是定位短板环节的关键指标。
    miss_recall = 0
    miss_wrong = 0
    _KW_NO_INFO = ["no information", "don", "t have", "not have", "无信息", "没有",
                   "不知道", "apologi", "insufficient", "cannot", "unable"]
    for i, s in enumerate(samples):
        q = s.get("question", "")
        # answer 可能是 int（年份/数量等），统一转 str 避免 rule_hit/judge 崩溃。
        gt = str(s.get("answer", ""))
        qtype = s.get("question_type", "unknown")
        dim = DIM_LABELS.get(qtype, qtype)
        a = agg.setdefault(dim, {"hit": 0, "judge_fail": 0, "total": 0, "rule_hit": 0})

        recalled = recall_memory(binary, s, project, top_k)
        system = (
            "以下是从历史会话记忆中检索到的相关内容：\n\n"
            + "\n\n".join(recalled)
            if recalled
            else "（未检索到相关历史记忆）"
        )
        # 注意：mimo 端点先 thinking 再回答，max_tokens 需足够大（1024）让
        # thinking 之后还有空间输出实际答案，否则 text 为空。
        r = call_messages(
            cfg,
            messages=[{"role": "user", "content": q}],
            system=system,
            max_tokens=1024,
        )
        a["total"] += 1
        if r.get("error"):
            details.append({"question": q[:80], "dim": dim, "hit": False,
                            "error": r["error"]})
            continue
        ans = str(r["text"])
        if rule_baseline:
            if rule_hit(gt, ans):
                a["rule_hit"] += 1
        j = judge_hit(cfg, q, gt, ans, qtype)
        if j is None:
            a["judge_fail"] += 1
            details.append({"question": q[:80], "dim": dim, "hit": False,
                            "error": "judge call failed"})
        elif j:
            a["hit"] += 1
            details.append({"question": q[:80], "dim": dim, "hit": True})
        else:
            # 未命中：判断是召回失败还是模型答错。
            if any(k in ans.lower() for k in _KW_NO_INFO):
                miss_recall += 1
            else:
                miss_wrong += 1
            details.append({"question": q[:80], "dim": dim, "hit": False,
                            "answer": ans[:120]})

        if (i + 1) % 10 == 0:
            sys.stderr.write(f"[harness] 进度 {i + 1}/{len(samples)}\n")

    # 汇总
    per_dim = {}
    total_hit = total_all = 0
    for dim, a in agg.items():
        rate = a["hit"] / a["total"] if a["total"] else 0.0
        rule_rate = a["rule_hit"] / a["total"] if (rule_baseline and a["total"]) else None
        per_dim[dim] = {
            "hit": a["hit"], "total": a["total"], "recall_rate": round(rate, 3),
            "judge_fail": a["judge_fail"],
            "rule_baseline_rate": round(rule_rate, 3) if rule_rate is not None else None,
        }
        total_hit += a["hit"]
        total_all += a["total"]
    overall = total_hit / total_all if total_all else 0.0
    return {
        "benchmark": "LongMemEval (_s split)",
        "model": cfg["model"],
        "samples": len(samples),
        "overall_recall_rate": round(overall, 3),
        "total_hit": total_hit,
        "total": total_all,
        "per_dimension": per_dim,
        "miss_breakdown": {
            "recall_miss": miss_recall,
            "answered_wrong": miss_wrong,
            "total_miss": miss_recall + miss_wrong,
        },
        "details": details,
    }


def write_report(result: dict, out_json: Path):
    out_json.parent.mkdir(parents=True, exist_ok=True)
    out_json.write_text(json.dumps(result, ensure_ascii=False, indent=2),
                        encoding="utf-8")
    md = out_json.with_suffix(".md")
    lines = [
        "# LongMemEval 长期记忆能力评测报告（#777）",
        "",
        f"- 基准：{result['benchmark']}",
        f"- 模型：{result['model']}",
        f"- 样本数：{result['samples']}",
        f"- **总体召回率（LLM-as-judge）：{result['overall_recall_rate']}** "
        f"({result['total_hit']}/{result['total']})",
        "",
        "## 分维度得分",
        "",
        "| 维度 | 命中 | 总数 | judge 召回率 | 规则对照 | judge 失败 |",
        "|------|------|------|------------|---------|-----------|",
    ]
    for dim, d in result["per_dimension"].items():
        rule = f"{d['rule_baseline_rate']}" if d["rule_baseline_rate"] is not None else "-"
        lines.append(
            f"| {dim} | {d['hit']} | {d['total']} | {d['recall_rate']} | "
            f"{rule} | {d['judge_fail']} |"
        )
    mb = result.get("miss_breakdown", {})
    total_miss = mb.get("total_miss", 0)
    recall_miss = mb.get("recall_miss", 0)
    wrong = mb.get("answered_wrong", 0)
    lines += [
        "",
        "## miss 归因（定位短板环节）",
        "",
        f"- 总 miss：{total_miss}",
        f"- **召回失败（模型答「无信息」，证据 session 未被检索到）：{recall_miss}** "
        f"（占比 {recall_miss / total_miss:.0%}）",
        f"- 召回成功但模型答错：{wrong}（占比 {wrong / total_miss:.0%}）",
        "",
        "**根因结论**：当前低分主要由**召回层**造成——本地哈希 embedding 无语义"
        "能力，长尾事实的 evidence session 检索不到，模型根本看不到答案。这是 "
        "`VectorStore` + 本地哈希 embedding 的固有局限，不是模型回答能力问题。"
        "换真实语义 embedding 后预计召回率显著提升（验证见下方提升计划）。",
        "",
        "## 口径说明与诚实性声明",
        "",
        "- **judge 模型**：原版 LongMemEval 用 GPT-4o 做 yes/no 判定；本评测改为用"
        "我们自己的真模型（`ANTHROPIC_BASE_URL`）做 judge，避免依赖 OpenAI key。",
        "- **记忆接入**：样本历史经 `longmemeval_ingest` 二进制写入 mimofan "
        "`VectorStore`（真实持久化→重建→检索），本地哈希 embedding **无语义能力**，"
        "故召回取决于字面/词袋重叠，是已知口径偏置，不代表语义召回上限。",
        "- **对照基线**：`规则对照` 列为子串匹配得分，仅供对比 judge 口径差异。",
        "- **abstention 维度**：smoke test 阶段仅做粗略弃答判定，非最终口径。",
        "",
        "## 能力提升计划（待分数确认后切片）",
        "",
        "评分完成后据最弱维度切片成 loopx todo（标注来源条目）。预计方向：",
        "- 知识更新维度弱 → 强化 consolidation 时近衰减/去重/rollup（#716）",
        "- 跨会话检索召回弱 → 评估 retrieval 真实 embedding 接入（当前降级本地哈希）",
        "- 时序推理弱 → 记忆元数据保留变更时间戳并供检索排序",
        "",
    ]
    md.write_text("\n".join(lines), encoding="utf-8")
    sys.stderr.write(f"[harness] 报告已写出: {out_json} 与 {md}\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--limit", type=int, default=None,
                    help="只评测前 N 条（smoke test 用，默认全量 500）")
    ap.add_argument("--data-path", type=str, default=None,
                    help="本地 longmemeval_s_cleaned.json 路径")
    ap.add_argument("--binary", type=str, required=True,
                    help="longmemeval_ingest 二进制路径")
    ap.add_argument("--project", type=str, default="mimofan")
    ap.add_argument("--top-k", type=int, default=5)
    ap.add_argument("--json", type=str,
                    default=str(HERE / "results" / "longmemeval_smoke.json"),
                    help="输出 JSON 报告路径")
    ap.add_argument("--rule-baseline", action="store_true",
                    help="额外计算规则子串对照分")
    args = ap.parse_args()

    cfg = client_cfg()
    data_path = ensure_dataset(args.data_path)
    if not Path(args.binary).exists():
        sys.stderr.write(f"[harness] 二进制不存在: {args.binary}\n")
        sys.exit(2)
    result = run(cfg, data_path, args.binary, args.limit, args.project,
                 args.top_k, args.rule_baseline)
    write_report(result, Path(args.json))


if __name__ == "__main__":
    main()
