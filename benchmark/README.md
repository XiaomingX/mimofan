# Mimofan Benchmark Suite

验收 mimofan 二进制能力的自动化测试框架。

## 目录结构

```
benchmark/
├── README.md                 # 本文件
├── tui_benchmark.sh          # 基准测试（exec + CLI 命令）
├── prompts/                  # 测试用例
│   ├── code_generation.txt   # 代码生成测试
│   ├── knowledge_qa.txt      # 知识问答测试
│   ├── math_reasoning.txt    # 数学推理测试
│   ├── creative_writing.txt  # 创意写作测试
│   └── multi_turn.txt        # 多轮对话测试
├── metrics.sh                # 评估指标计算
└── results/                  # 测试结果输出目录
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

### 运行基准测试

```bash
./tui_benchmark.sh
```

### 计算评估指标

```bash
./metrics.sh results/
```

## 评分标准

| 等级 | 分数范围 | 说明 |
|------|----------|------|
| S | 90-100 | 优秀，生产就绪 |
| A | 80-89 | 良好，可正常使用 |
| B | 70-79 | 合格，基本可用 |
| C | 60-69 | 及格，需改进 |
| D | <60 | 不及格，需重大修复 |
