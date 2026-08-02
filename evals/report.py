"""report.py — CSV + HTML report generation for model comparison.

Outputs are fully offline / self-contained (no external JS or CSS).
"""
from __future__ import annotations

import csv
import html
import statistics
from typing import Optional

METRIC_ROWS = [
    ("平均延迟 (s)", "avg_latency_s"),
    ("P50 延迟 (s)", "p50_latency_s"),
    ("P95 延迟 (s)", "p95_latency_s"),
    ("平均 TTFT (s)", "avg_ttft_s"),
    ("平均吞吐 (tok/s)", "avg_throughput_tps"),
    ("质量·启发式 (0-1)", "quality_heuristic"),
    ("参考准确率 (0-1)", "ref_accuracy"),
    ("质量·裁判 (1-10)", "quality_judge"),
    ("自一致性 (0-1)", "self_consistency"),
    ("错误数", "n_errors"),
]


def _fmt(v: Optional[float]) -> str:
    if v is None:
        return "—"
    return f"{v:.3f}" if isinstance(v, float) else str(v)


def write_csv_summary(path: str, models: list[str], metrics: dict) -> None:
    with open(path, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["metric"] + models)
        for label, key in METRIC_ROWS:
            row = [label]
            for m in models:
                row.append(_fmt(metrics.get(m, {}).get(key)))
            w.writerow(row)


def write_csv_runs(path: str, results) -> None:
    with open(path, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(
            [
                "model",
                "prompt_id",
                "category",
                "repeat",
                "latency_s",
                "ttft_s",
                "prompt_tokens",
                "completion_tokens",
                "quality_heuristic",
                "error",
                "text",
            ]
        )
        for r in results:
            w.writerow(
                [
                    r.model,
                    r.prompt_id,
                    r.category,
                    r.repeat,
                    f"{r.latency_s:.3f}",
                    f"{r.ttft_s:.3f}" if r.ttft_s is not None else "",
                    r.prompt_tokens,
                    r.completion_tokens,
                    f"{r.quality_heuristic:.3f}",
                    r.error,
                    r.text,
                ]
            )


def _bar(value: Optional[float], vmax: float, higher_better: bool = True) -> str:
    if value is None or vmax <= 0:
        return ""
    pct = max(2, min(100, (value / vmax) * 100))
    # red for "bad" direction
    color = "#3fb950" if higher_better else "#f85149"
    return (
        f'<div class="bar" style="width:{pct:.1f}%;background:{color}"></div>'
        f'<span class="barval">{_fmt(value)}</span>'
    )


def write_html(path: str, models: list[str], metrics: dict, results, prompts, cross: dict) -> None:
    by_pid = {p.get("id"): p for p in prompts}

    # scales for bars
    lat_max = max([metrics[m].get("avg_latency_s", 0) or 0 for m in models] + [0.001])
    tput_vals = [metrics[m].get("avg_throughput_tps") for m in models]
    tput_max = max([v for v in tput_vals if v is not None] + [0.001])
    qual_max = max([metrics[m].get("quality_heuristic", 0) or 0 for m in models] + [0.001])
    cons_vals = [metrics[m].get("self_consistency") for m in models]
    cons_max = max([v for v in cons_vals if v is not None] + [0.001])

    # summary table rows
    sum_rows = ""
    for label, key in METRIC_ROWS:
        cells = "".join(f"<td>{_fmt(metrics.get(m, {}).get(key))}</td>" for m in models)
        sum_rows += f"<tr><th>{html.escape(label)}</th>{cells}</tr>\n"

    # chart blocks
    def chart(title, key, vmax, higher, fmt=_fmt):
        rows = ""
        for m in models:
            v = metrics.get(m, {}).get(key)
            rows += (
                f'<div class="crow"><span class="clabel">{html.escape(m)}</span>'
                f'<div class="ctrack">{_bar(v, vmax, higher)}</div></div>'
            )
        return f'<div class="card"><h3>{html.escape(title)}</h3>{rows}</div>'

    charts = (
        chart("平均延迟 (越低越好)", "avg_latency_s", lat_max, False)
        + chart("平均吞吐 tok/s (越高越好)", "avg_throughput_tps", tput_max, True)
        + chart("质量·启发式 (越高越好)", "quality_heuristic", qual_max, True)
        + chart("自一致性 (越高越好)", "self_consistency", cons_max, True)
    )

    # cross-model agreement
    ima = cross.get("inter_model_agreement")
    ima_txt = _fmt(ima) if ima is not None else "无可分类样本"
    cross_rows = ""
    for pp in cross.get("per_prompt", []):
        cross_rows += (
            f'<tr><td>{html.escape(str(pp["prompt_id"]))}</td>'
            f'<td>{pp["n_models"]}</td><td>{pp["agreement"]:.3f}</td></tr>'
        )

    # raw responses
    raw_rows = ""
    for m in models:
        for r in results:
            if r.model != m:
                continue
            raw_rows += (
                f"<tr><td>{html.escape(m)}</td><td>{html.escape(r.prompt_id)}</td>"
                f"<td>r{r.repeat}</td>"
                f'<td class="{"err" if r.error else ""}">{html.escape(r.error or r.text[:600])}</td></tr>'
            )

    ts = __import__("datetime").datetime.now().strftime("%Y-%m-%d %H:%M:%S")

    doc = f"""<!doctype html>
<html lang="zh"><head><meta charset="utf-8">
<title>模型横向对比报告</title>
<style>
 body{{font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;
   margin:0;background:#0d1117;color:#e6edf3;padding:24px}}
 h1{{font-size:20px}} h2{{font-size:16px;margin-top:28px;border-left:3px solid #58a6ff;padding-left:8px}}
 h3{{font-size:13px;margin:0 0 8px}}
 .meta{{color:#8b949e;font-size:12px}}
 table{{border-collapse:collapse;width:100%;font-size:13px;margin-top:8px}}
 th,td{{border:1px solid #30363d;padding:6px 8px;text-align:left}}
 th{{background:#161b22}}
 .grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:14px;margin-top:10px}}
 .card{{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:12px}}
 .crow{{display:flex;align-items:center;margin:6px 0}}
 .clabel{{width:120px;font-size:12px;color:#8b949e;flex:none}}
 .ctrack{{flex:1;background:#21262d;border-radius:4px;position:relative;min-height:18px}}
 .bar{{height:14px;border-radius:4px}}
 .barval{{position:absolute;right:6px;top:0;font-size:11px;color:#e6edf3}}
 .err{{color:#f85149}}
 details{{margin-top:16px}} summary{{cursor:pointer;color:#58a6ff}}
</style></head><body>
<h1>模型横向对比报告</h1>
<div class="meta">生成时间：{ts} ｜ 模型数：{len(models)} ｜ 提示词数：{len(prompts)} ｜ 总运行数：{len(results)}</div>

<h2>指标总览（横向对比参数表）</h2>
<table><thead><tr><th>指标</th>{''.join(f'<th>{html.escape(m)}</th>' for m in models)}</tr></thead>
<tbody>{sum_rows}</tbody></table>

<h2>可视化对比</h2>
<div class="grid">{charts}</div>

<h2>跨模型一致性</h2>
<p class="meta">分类/短答/抽取类样本上的多数投票一致率：<b>{ima_txt}</b></p>
<table><thead><tr><th>prompt</th><th>模型数</th><th>一致率</th></tr></thead>
<tbody>{cross_rows or '<tr><td colspan="3">—</td></tr>'}</tbody></table>

<details><summary>原始回复（点击展开 {len(results)} 条）</summary>
<table><thead><tr><th>模型</th><th>prompt</th><th>轮次</th><th>输出/错误</th></tr></thead>
<tbody>{raw_rows}</tbody></table></details>
</body></html>"""
    with open(path, "w", encoding="utf-8") as f:
        f.write(doc)
