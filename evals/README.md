# evals/ — 横向模型对比 Harness

用**同一套提示词**对比多个模型（不同 `endpoint` / `sk` / `model_name`）在
**性能、输出质量、一致性、耗时**上的表现，并产出 CSV 与 HTML 横向对比报告。

纯标准库实现（Python 3.11+，无需 `pip install`），离线可验收。

---

## 目录约定（与需求一致）

| 内容 | 位置 |
|------|------|
| 横向评估用的**样本提示词** | `benchmark/model-comparison/prompts.jsonl` |
| 横向验证的**代码与脚本** | `evals/`（`harness.py` / `report.py` / `cli.py`） |
| 你的**配置（含密钥引用）** | `evals/config.toml`（从 `config.example.toml` 复制） |
| 生成的**报告** | `evals/out/`（`comparison_summary.csv` / `comparison_runs.csv` / `comparison_report.html`） |

> 样本统一在 `benchmark/`，代码与脚本统一在 `evals/`，两者互不混入。

---

## 1. 存放配置（在哪里、怎么写）

```bash
cd evals
cp config.example.toml config.toml      # config.toml 不入库
```

编辑 `config.toml`：

```toml
[settings]
repeat = 1                              # >1 才会算「自一致性」
timeout_seconds = 120
stream = false                         # 开流可测 TTFT
prompts_path = "benchmark/model-comparison/prompts.jsonl"
output_dir = "out"

# 可选裁判模型（LLM-as-judge 质量评分 1-10）；留空则只用启发式/参考指标
[judge]
endpoint = "https://api.xiaomimimo.com/v1"
api_key = "$XIAOMI_MIMO_API_KEY"
model = "mimo-v2.5-pro"

[[models]]                              # 任意增加
name = "mimo-pro"
endpoint = "https://api.xiaomimimo.com/v1"
api_key = "$XIAOMI_MIMO_API_KEY"        # 密钥走环境变量，不落盘
model = "mimo-v2.5-pro"

[[models]]
name = "deepseek-chat"
endpoint = "https://api.deepseek.com/v1"
api_key = "$DEEPSEEK_API_KEY"
model = "deepseek-chat"
```

**密钥安全**：所有 `*_key` 若以 `$` 开头，运行时从环境变量读取
（如 `export XIAOMI_MIMO_API_KEY=...`）。不要把明文密钥写进 `config.toml`。

每个 `[[models]]` 是一家待对比模型；想加几家就加几段。协议为
OpenAI Chat Completions 兼容（`{endpoint}/chat/completions`）。

---

## 2. 运行（怎么跑、如何等结果）

```bash
# 先校验配置与样本是否就绪（不调用任何模型）
python cli.py check --config config.toml

# 运行真实评测：会逐条打印进度，并【阻塞等待全部结果】完成后写报告
python cli.py run --config config.toml
```

`run` 对「每个模型 × 每条提示词 × repeat」依次请求，终端实时输出
`[done/total] 模型 | prompt | 轮次 | 耗时/ERR`；全部完成后在
`evals/out/` 生成三份产物：

- `comparison_summary.csv` —— **横向对比参数表**（行=指标，列=模型）
- `comparison_runs.csv` —— 每次运行的明细（含耗时、token、质量、原文）
- `comparison_report.html` —— 自包含离线仪表盘（表 + 条形图 + 原始回复）

打开 HTML 即可横向查看：延迟/吞吐/质量/自一致性对比，以及跨模型一致率。

---

## 3. 验收（acceptance）

无需任何密钥、不联网，验证整条流水线可用：

```bash
python cli.py verify
```

它通过内置 mock 客户端跑一个最小样本，断言：无错误、CSV/HTML 均生成、
HTML 含指标表与标题、指标已计算。输出 `RESULT: PASS` 即验收通过。

---

## 4. 评测维度与「行业最佳实践」口径

| 维度 | 指标 | 口径 |
|------|------|------|
| 性能 | 平均/P50/P95 延迟、吞吐 (tok/s) | 每请求墙钟耗时；吞吐 = completion_tokens / 延迟 |
| 耗时 | TTFT（首 token 延迟） | 仅 `stream=true` 时可得 |
| 输出质量 | 启发式 (0-1)、参考准确率 (0-1)、裁判 (1-10) | 启发式=格式丰富度代理；参考类用精确匹配 + ROUGE-L；裁判=LLM-as-judge（MT-Bench 风格） |
| 一致性 | 自一致性 (0-1)、跨模型一致率 (0-1) | 自一致性=同模型重复间的 token Jaccard；跨模型=分类/短答样本的多数投票一致率 |

> 透明说明：启发式质量与 ROUGE-L 为可复现的代理指标；若要更贴近人类评判，
> 在 `[judge]` 配置裁判模型即可获得 1-10 的 LLM-as-judge 分数。语义级一致性
> 如需要可后续接入 embedding 端点（当前默认词级 Jaccard，零依赖）。

---

## 5. 自定义样本

编辑 `benchmark/model-comparison/prompts.jsonl`，每行一条 JSON：

```json
{"id": "math-1", "category": "math", "type": "short", "reference": "391",
 "prompt": "Compute 17 * 23. Reply with only the final number."}
```

`type` 取值：`open`（开放生成）、`short`/`classify`/`extract`（带 `reference`，
用于参考准确率与跨模型一致率计算）。
