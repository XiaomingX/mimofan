# Mimofan Benchmark Suite

验收 mimofan 二进制能力的自动化测试框架。

## 目录结构

```
benchmark/
├── README.md                    # 本文件
├── configs/                     # 配置文件
├── docker/                      # Docker 配置
├── memory/                      # 记忆系统测试
├── model-comparison/            # 横向模型对比样本
├── run_observability_bench.sh   # fleet::observability 测试
└── run_all.sh                   # 一键运行所有测试
```

## 评估指标

### 基础指标

| 指标 | 说明 | 计算方式 |
|------|------|----------|
| `latency_p50` | 响应时间中位数 | 所有测试响应时间排序后取中间值 |
| `latency_p95` | 响应时间 95 分位 | 所有测试响应时间排序后取 95% 位置 |
| `success_rate` | 成功率 | 成功请求数 / 总请求数 |
| `token_throughput` | 吞吐量 | 总 token 数 / 总时间 |

### 质量指标

| 指标 | 说明 | 评估方式 |
|------|------|----------|
| `code_correctness` | 代码正确性 | 生成代码能否通过编译/运行 |
| `answer_accuracy` | 答案准确性 | 与标准答案的相似度 |
| `instruction_following` | 指令遵循度 | 是否按照要求格式/长度输出 |
| `context_retention` | 上下文保持 | 多轮对话中是否保持上下文 |

### 稳定性指标

| 指标 | 说明 | 评估方式 |
|------|------|----------|
| `error_rate` | 错误率 | 错误请求数 / 总请求数 |
| `timeout_rate` | 超时率 | 超时请求数 / 总请求数 |
| `crash_count` | 崩溃次数 | 进程异常退出次数 |

## 运行方式

### 环境变量

```bash
export MIMOFAN_TEST_HOME=/tmp/mimofan-benchmark
export MIMOFAN_TEST_API_KEY=your_api_key
export MIMOFAN_TEST_BASE_URL=your_base_url
export MIMOFAN_TEST_MODEL=your_model
```

### 一键运行所有测试

```bash
./run_all.sh
```

### 单独运行测试

#### fleet::observability 测试

验收 fleet 可观测性能力：

```bash
./run_observability_bench.sh
```

### API 路由逻辑

mimofan 通过以下逻辑自动选择 API 协议：

1. 如果 `provider` 是 `XiaomiMimo` 或 `Custom`
2. 且 `base_url` 以 `/anthropic` 结尾
3. 则使用 **Anthropic Messages API** 协议
4. 否则使用 **OpenAI Chat Completions** 协议

### 配置示例

#### Anthropic Messages API

```toml
provider = "custom"
api_key = "你的_ANTHROPIC_API_KEY"
base_url = "https://api.xiaomimimo.com/anthropic"
default_text_model = "mimo-v2.5"
```

#### OpenAI Chat Completions API

```toml
provider = "custom"
api_key = "你的_XIAOMI_MIMO_API_KEY"
base_url = "https://api.xiaomimimo.com/v1"
default_text_model = "mimo-v2.5"
```

## 评分标准

| 等级 | 分数范围 | 说明 |
|------|----------|------|
| S | 90-100 | 优秀，生产就绪 |
| A | 80-89 | 良好，可正常使用 |
| B | 70-79 | 合格，基本可用 |
| C | 60-69 | 及格，需改进 |
| D | <60 | 不及格，需重大修复 |

## 评测框架 / Harness（仅仓库地址，无自有样本文件）

以下为可用于搭建 code-harness 验收流程的评测框架 / 执行系统。其本身不带自有样本数据集，而是接入上文 `report/sample_sets.md` 中的 A 类数据集运行。

| 仓库名称 | 地址 | 用途说明 |
|----------|------|----------|
| Harbor | https://github.com/harbor-framework/harbor | Terminal-Bench 团队推出的 agent 评测与 RL rollout 执行框架；统一编排任务 / 容器 / Agent / 验证脚本 / reward，是 Terminal-Bench 2.0 的官方 harness，并通过 registry 接入 SWE-Bench、SWE-Lancer 等第三方数据集。 |
| OpenHands | https://github.com/All-Hands-AI/OpenHands | 开源 coding agent，自带 evaluation harness，可将 SWE-bench 等 benchmark 接入 agent runtime，用于端到端评测 agent 在真实仓库中修复 issue 的能力。 |
| Inspect AI | https://github.com/UKGovernmentBEIS/inspect_ai | UK AISI 出品的通用 LLM 评测框架；dataset-task-solver-scorer 抽象，内置 200+ 预置评测（含 coding / agent / CTF 类），支持 Docker / K8s 沙箱与多轮工具调用，可作为构建 code-harness eval 的基础设施。 |
