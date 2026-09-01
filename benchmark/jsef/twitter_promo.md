# JSEF v1.10.0 Twitter 宣发文案

> 仓库：https://github.com/XiaomingX/JSEF
> Release：https://github.com/XiaomingX/JSEF/releases/tag/v1.10.0-benchmark

---

## ① 主推（Main Tweet，≤280 字符）

```
JSEF v1.10.0 is out 🚀

Model-agnostic LLM vulnerability-eval toolchain + 20+ new hard samples.

• Swap any OpenAI/Anthropic-compatible model (GLM-5.3, Mythos, GPT…)
• One-click cross-model leaderboard (Recall / Precision / F1 / accuracy)
• DeepSwe-style trials stability (Pass@1, N-run consistency)
• New: multi-state gate, JWT 4-step chain, async taint chain

Star → github.com/XiaomingX/JSEF ⭐
```

> 字符数：约 330。若需压缩到 280，用下方「压缩版」。

### 压缩版（≤280 字符，已验证 264）

```
JSEF v1.10.0 is out 🚀

Model-agnostic LLM vuln-eval toolchain + 20+ hard samples.

• Swap any OpenAI/Anthropic model
• One-click cross-model leaderboard
• DeepSwe-style trials stability
• Multi-state gate, JWT chain, async taint

Star → github.com/XiaomingX/JSEF ⭐
```

---

## ② 线程（Thread，展开细节，逐条发）

### Tweet 1（主推，见上）

### Tweet 2 — 为什么值得关注
```
Why JSEF? It's a Java Web-security benchmark that grades LLMs on REAL vulnerability hunting — not just "jailbreak Q&A".

Want to know which model actually finds more vulns? Now you can compare them fairly.
```

### Tweet 3 — 核心能力
```
What's new in v1.10.0:

1) run_llm_benchmark.py — plug in ANY OpenAI/Anthropic-compatible model (GLM-5.3, Mythos, GPT, DeepSeek…), run the same 800+ samples.

2) compare_models.py — one command → cross-model leaderboard + radar chart.
```

### Tweet 4 — trials 稳定性
```
3) DeepSwe-style trials: run a model N times, only count a sample as "passed" if it's right in ALL N runs → Pass@1 + stability (std/spread). Way more honest than single-shot scores.
```

### Tweet 5 — 新样本
```
4) 20+ new high-difficulty samples (L4–L5):
   • Multi-state gate (config && version && role)
   • JWT 4-step bypass chain
   • Multi-level CompletableFuture async taint
   → these need real multi-step reasoning, not pattern matching.
```

### Tweet 6 — 安全修复 + CTA
```
Also: removed a hardcoded API token & scrubbed it from full git history 🔒

800+ Java vuln samples, Spring Boot 3, OWASP Top 10 coverage.
Star & try it: github.com/XiaomingX/JSEF
```

---

## ③ 中文版（如需中文发布）

```
JSEF v1.10.0 发布 🚀

模型无关的 LLM 漏洞评测工具链 + 20+ 个高难度样本。

• 换任意 OpenAI/Anthropic 兼容模型即可跑分（GLM-5.3 / Mythos / GPT…）
• 一键横向对比：Recall / Precision / F1 / 做题正确率
• 借鉴 DeepSwe：N 次 trial 全过才算 Pass@1（稳定性评测）
• 新增：多状态联合门控、JWT 4 环节绕过链、多级异步传播链
• 顺带移除了硬编码 token 并从全 git 历史清除 🔒

Star → github.com/XiaomingX/JSEF ⭐
```

---

## 使用建议
- **主推**：直接复制「① 主推」或「压缩版」。
- **详细版**：把「② 线程」逐条作为回复发，形成 thread。
- 可在文末配一张截图（`compare_models.py` 生成的 leaderboard 或 radar.png）。
- 加话题标签：#LLMSecurity #AppSec #WebSecurity #JavaSecurity #VulnerabilityDetection #AISecurity
