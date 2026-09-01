# Plan 13 — 新增「验收 LLM 安全能力」评测指南章节（README）+ 补齐盲化评分闭环

> 目标：在根目录 `README.md` 新增一节，讲清楚"如何用本项目验收 LLM 安全能力（误报率、漏报率等指标评测）"；并补齐让「自动生成双盲样本 → 自动评分 → 快速生成报告」真正可行的缺失脚本。
>
> 状态：planning（本文件为可执行计划，供 `do` 技能分阶段执行）

---

## Phase 0 — 现状调研结论（已完成，证据见下）

### 已有工具链（成熟，无需重造）

| 环节 | 脚本 | 现状 |
|---|---|---|
| 双盲样本生成 | `benchmark/scripts/blind.py` | 已实现：去标签化生成 `benchmark/blinded/B*.java` + 私有 `manifest.json`（`files` / `anchors` 两张映射） |
| LLM 跑测 | `run_llm_benchmark.py`（顶层） | 模型无关（openai/anthropic 协议），`--mode identify|blind`、`--trials N`、`--resume`、`--require-complete`；identify 产简化 JSON、blind 产 SARIF |
| LLM 跑测（本地模型） | `run_mimofan_benchmark.py`（顶层） | 仅驱动本地 mimo 二进制，identify 语义，无 blind/trials |
| 评分卡 | `benchmark/scripts/scorecard.py` | TP/FN/FP/TN、Recall、Precision、**FPR（=误报率）**、Youden、F1、MCC、定位精度、CWE 精确度、trace 召回 |
| 横向对比 | `compare_models.py`（顶层） | 调 scorecard 交叉矩阵 → `compare.md` + `ranking.png` + `radar.png` + `trials_matrix.json` |
| OWASP 行业报告 | `benchmark/reports/generate_report.py` | `report.md` / `report.json` / `radar_data.json` / `ranking.png` |
| 双源校验门禁 | `benchmark/scripts/validate_checkpoints.py` | 校验 CSV↔CHECKPOINT 一致性，退出码非 0 即失败 |
| 一键半流程 | `benchmark/run_benchmark.sh` | 只串联 scorecard→compare_models，**不含盲化、不含 generate_report** |

### 关键断点 / 缺口（本次要补的）

1. **[HARD] 盲化评分闭环断裂**：`blind.py` 生成 `manifest.json`（`B0001.java:ANCHOR_1 → JSEF-XXX-001`），但 `scorecard.py` **不消费 blind manifest**（仅用 `expectedresults.csv` 的真实 file/line 或 id 对齐）。→ 盲化样本被 LLM 跑出的结果**无法自动回连真实 checkpoint 计分**。README 描述的"锚点回连"机制在代码里不存在。这是"自动生成双盲样本 → 快速报告"之间的断桥。
2. **[MED] 缺 `.env.example`**：项目没有任何环境变量集中文档；`OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `MIMOFAN_PROVIDER` / `ANTHROPIC_BASE_URL` / `ANTHROPIC_MODEL` / `ANTHROPIC_AUTH_TOKEN` 只散落在各脚本 docstring。
3. **[MED] 根 README 文档脱节**：`run_llm_benchmark.py` / `compare_models.py` / `generate_report.py` / `blind.py` 在根 README 完全未提及；误报率/漏报率公式未在根 README 明示。现有 benchmark 章节（L81–160）只讲了 scorecard 单对象评分。

### 根 README.md 结构（插入点）

```
L81  ## SAST 能力与多模型漏洞挖掘 Benchmark   ← 现有章节
L149 ### 如何运行与交叉对比
L160 详细设计见 benchmark/README.md
L163 ## 官方文档
```
**新节插入点**：`L160`（现有 benchmark 章节末尾）与 `L163`（官方文档）之间，新增 `## 验收 LLM 安全能力（误报率 / 漏报率评测）`。

---

## Phase 1 — 打通盲化评分闭环：新增 `benchmark/scripts/eval_blind.py`

### 做什么

新增一个独立脚本（**不改动 scorecard.py 核心**，符合"禁止强行重构整体架构"），把盲化评测补成闭环：

**输入**
- `--expected`（默认 `benchmark/expectedresults.csv`）
- `--manifest`（默认 `benchmark/blinded/manifest.json`）
- `--result`：被测对象对盲化语料的输出，支持 SARIF 或简化 JSON（`[{id?, file, line, ...}]`，file 为 `B*.java` 路径）
- `--name` / `--out` / `--timeout-ms` / `--line-tolerance`（透传给内部评分）

**逻辑（核心：锚点回连）**
1. `load_manifest()` 解析 `manifest.json`：`anchors`（`"B0001.java:ANCHOR_1" → "JSEF-XXX-001"`）与 `files`（`"B0001.java" → 原始路径`）。
2. `reconcile()`：把结果里每个命中 `(file=Blindfile.java, line=N)` 在 anchors 里找最近的行命中，映射回真实 checkpoint `id`（行容差用 `--line-tolerance`）。
3. 构造 scorecard 原生格式的简化 JSON 列表 `[{id, hit=true, line, file, cwe, message}]`，`import` scorecard 的 `load_expected` / `compute_metrics` / `score_object` 直接评分（复用不 fork）。
4. 输出与 `scorecard.py` 同构的 `scorecard.json`（含 TP/FN/FP/TN、Recall、**FPR**、Youden、F1、MCC、by_cwe、by_level）。

**验证清单**
- `python3 benchmark/scripts/blind.py` 生成 `benchmark/blinded/` + `manifest.json`
- 构造一份"模拟 LLM 结果"（对某几个 anchor 报命中），跑 `eval_blind.py`，确认 TP/FN 与手动核对一致
- `eval_blind.py --result <示例>.sarif` 也能解析

**反模式防护**
- 不修改 `scorecard.py` / `blind.py` 既有行为（只读导入）
- 不做"全自动挖洞"，只做"结果回连与评分"

---

## Phase 2 — 补 `.env.example` 与环境变量集中说明

### 做什么

新增根目录 `.env.example`，集中列出评测相关环境变量，并附注释：

```bash
# ── LLM 评测环境变量（配合 run_llm_benchmark.py / run_mimofan_benchmark.py）──
# 通用 HTTP 评测（run_llm_benchmark.py）
OPENAI_API_KEY=        # provider=openai 时使用（或 --api-key 直传）
ANTHROPIC_API_KEY=     # provider=anthropic 时使用
# 本地 mimo 评测（run_mimofan_benchmark.py，硬性要求）
MIMOFAN_PROVIDER=anthropic-compatible
ANTHROPIC_BASE_URL=https://api.xiaomimimo.com/anthropic
ANTHROPIC_MODEL=mimo-v2.5
ANTHROPIC_AUTH_TOKEN=  # 必填，缺省 FATAL
```

> 依据：`run_llm_benchmark.py:33-37`、`run_mimofan_benchmark.py:17-21,42-49`。`.env` 本身加入 `.gitignore`（先确认现有 `.gitignore` 是否已含）。

**验证清单**
- `git status` 确认 `.env.example` 新增、`.env` 不入库
- grep 各脚本环境变量名与 `.env.example` 一一对应

---

## Phase 3 — 更新根 `README.md`：新增「验收 LLM 安全能力」章节

### 做什么

在 `L160` 之后插入新 H2 章节（标题：`## 验收 LLM 安全能力（误报率 / 漏报率评测）`），内容大纲：

1. **评测指标与公式**（对接 scorecard 输出）
   - 混淆矩阵四元组：TP / FN / FP / TN（`expect=VULN` 报对=TP，漏报=FN；`expect=SAFE` 报错=FP，不报=TN）
   - **漏报率 = FN / (TP + FN) = 1 − Recall**
   - **误报率 (FPR) = FP / (FP + TN)**（scorecard `compute_metrics` 定义）
   - 派生：Precision、Youden = (Recall − FPR) × 100、F1、MCC、定位精确率、CWE 精确度、trace 召回（`--check-trace`）
2. **推荐流程（三步）**
   1. **生成双盲样本**：`blind.py` → `benchmark/blinded/B*.java` + 私有 `manifest.json`（防标签泄漏；评分方持 manifest 回连）
   2. **驱动 LLM 产出结果**：`run_llm_benchmark.py`（identify 直评 / blind 盲挖；`--trials N` 做稳定性）
   3. **评分 + 报告**：`scorecard.py` 或 `eval_blind.py`（盲化）→ `compare_models.py`（横向对比 compare.md/ranking.png/radar.png）→ `generate_report.py`（OWASP 报告）
3. **命令示例**（每种模式给一条可直接复制的命令，标注 `.env.example` 来源）
   - identify：`python3 run_llm_benchmark.py --provider openai --base-url ... --model ... --mode identify ...`
   - blind：`python3 run_llm_benchmark.py --mode blind ...` + `python3 benchmark/scripts/eval_blind.py --result <obj>/...`
   - trials：`--trials 5` → `compare_models.py` 自动聚合 `sample_pass@1`
4. **盲化评测注意点**：`--require-complete` 防漏跑样本被误判 FN；盲化语料不进 `expectedresults.csv` 的 file 列（它只是分发形式）。
5. **环境变量表**：指向 `.env.example`（表列出每个变量用途）。

### 反模式防护
- 不重写现有 benchmark 章节，只新增独立章节
- 所有命令参数必须与真实脚本 argparse 一致（已在 Phase 0 核实），不发明参数
- 不复制 benchmark/README.md 全量内容，只做"入口指引 + 指标定义 + 命令速查"，深链到 `benchmark/README.md` §5/§8

### 验证清单
- 每个出现的脚本名 `grep` 确认真实存在
- 每条命令 `--dry-run`（`run_llm_benchmark.py --dry-run`）跑通不报参数错
- `eval_blind.py` 的用法说明与实际 CLI 一致

---

## Final Phase — 验证

1. **脚本全链路冒烟**：
   - `python3 benchmark/scripts/validate_checkpoints.py --expected benchmark/expectedresults.csv --cases-dir benchmark/cases --src-dir src/main/java/com/freedom/securitysamples/vulnerability` 退出码 0（现有门禁不被破坏）
   - `eval_blind.py` 用示例盲化结果跑通，输出 TP/FN/FP/TN 合理
   - `run_llm_benchmark.py --dry-run ...` 不报参数错误
2. **README 一致性**：grep 新节所有脚本名/参数，与源码逐一对照
3. **不回归**：`git status` 确认改动面收敛（新增 `eval_blind.py`、`.env.example`；修改 `README.md`；若需微调 `.gitignore`）
4. **反模式检查**：无新造 API、无对既有脚本行为的破坏性改动

---

## 缺失项清单（回应"有什么缺失"）

| 优先级 | 缺失项 | 处理 |
|---|---|---|
| 高 | 盲化样本结果无法自动回连真实 checkpoint 计分（`manifest.json` 未被 scorecard 消费） | Phase 1 新增 `eval_blind.py` |
| 高 | 环境变量无集中文档 / 无 `.env.example` | Phase 2 补齐 |
| 中 | 根 README 未提 `run_llm_benchmark.py` / `compare_models.py` / `generate_report.py` / `blind.py` | Phase 3 README 章节 |
| 中 | 误报率/漏报率公式未在根 README 明示 | Phase 3 README 章节 |
| 低 | `docs/` 下 6 个失效链接（deployment.md 等，非本次范围） | 可选，如顺手可修 |
