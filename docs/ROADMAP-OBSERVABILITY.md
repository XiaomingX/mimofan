# Mimofan 可观测性与评估能力建设路线图

> GitHub Issue: [#527](https://github.com/XiaomingX/mimofan/issues/527)

## 背景

参考竞品 Langfuse、Helicone、OpenLLMetry、Ragas 的能力矩阵，分析 mimofan 项目在 LLM 可观测性、评估、实验管理方面的缺失能力。

## 竞品核心能力对比

| 能力 | Langfuse | Helicone | OpenLLMetry | Ragas | mimofan 当前 |
|------|----------|----------|-------------|-------|-------------|
| 追踪/日志 | ✅ 嵌套 trace/span | ✅ 代理层自动记录 | ✅ OTel 标准采集 | ❌ | ⚠️ 本地文件日志 |
| 评估/评分 | ✅ LLM-as-Judge | ✅ Eval Scores | ❌ | ✅ 核心能力 | ⚠️ 离线 mock 评估 |
| 成本分析 | ✅ Dashboard 级 | ✅ 请求粒度 | ⚠️ 基础估算 | ❌ | ⚠️ 单次调用级别 |
| 延迟监控 | ✅ Timeline 分解 | ✅ 性能指标 | ✅ 捕获导出 | ❌ | ⚠️ 无 span 分解 |
| 用户反馈 | ✅ 原生支持 | ✅ 原生支持 | ❌ | ❌ | ❌ 缺失 |
| A/B 测试 | ✅ Prompt 版本对比 | ✅ 实验模块 | ❌ | ✅ 实验优先 | ❌ 缺失 |
| 实验管理 | ✅ 数据集+实验 UI | ✅ Experiments | ❌ | ✅ 数据集管理 | ❌ 缺失 |
| 提示词管理 | ✅ 版本控制 | ✅ AI Gateway | ❌ | ❌ | ❌ 缺失 |
| 合成数据 | ❌ | ❌ | ❌ | ✅ 核心能力 | ❌ 缺失 |

## mimofan 当前能力盘点

### 已有能力

| 模块 | 文件 | 能力描述 |
|------|------|---------|
| Tracing 日志 | `crates/tui/src/runtime_log.rs` | 基于 tracing-subscriber 的文件日志，7天保留 |
| Token 估算缓存 | `crates/tui/src/core/engine/token_estimate_cache.rs` | 内容版本化 key，64条审计环 |
| 前缀缓存监控 | `crates/tui/src/prefix_cache.rs` | SHA-256 指纹追踪 system prompt 变更 |
| 资源遥测 | `crates/tui/src/resource_telemetry.rs` | token/时间预算，三级压力等级 |
| Fleet 拓扑 | `crates/tui/src/fleet/observability.rs` | parent-child 拓扑，峰值资源使用 |
| 定价引擎 | `crates/tui/src/pricing.rs` | per-million-token 定价，USD+CNY 双币种 |
| 成本归集 | `crates/tui/src/cost_status.rs` | 侧通道捕获后台 LLM 调用成本 |
| 离线评估 | `crates/tui/src/eval.rs` | EvalHarness 六步工具环评估 |
| 模型对比 | `evals/harness.py` | Python 横向模型延迟/质量对比 |

### 缺失能力（按优先级排序）

#### P0 - 核心缺失

| # | 能力 | 说明 |
|---|------|------|
| 1 | OpenTelemetry/OTLP 集成 | 无法导出到 Jaeger/Grafana 等标准后端 |
| 2 | 结构化事件追踪 | 无 trace_id/span_id 贯穿请求全链路 |
| 3 | 端到端 Agent 评估 | EvalHarness 是纯离线 mock，不调用真实 LLM |
| 4 | 评估结果持久化 | 无历史记录，无法做趋势分析和回归检测 |

#### P1 - 重要增强

| # | 能力 | 说明 |
|---|------|------|
| 5 | 会话级成本聚合 | cost_status 仅做帧级 drain，无日/周/月维度汇总 |
| 6 | 成本预算/告警 | 无 token 预算上限或费用超限告警 |
| 7 | A/B 实验框架 | 无法对比不同 prompt 策略或模型配置效果 |
| 8 | 用户反馈采集 | 无原生反馈收集机制 |

#### P2 - 体验优化

| # | 能力 | 说明 |
|---|------|------|
| 9 | 实时 cost dashboard | cost 仅在 footer 隐式展示，无独立面板 |
| 10 | 结构化日志搜索 | 本地日志无索引，无法按 trace_id/session_id 查询 |
| 11 | 工具调用审计日志 | 无集中审计记录 |
| 12 | 延迟异常告警 | 无基于阈值的监控或通知 |

---

## 实施计划

### Phase 1: 基础追踪层（OpenTelemetry 集成）

**目标**: 建立标准的 trace/span 数据模型，支持导出到主流后端

**任务**:
- [ ] 设计 trace/span 数据模型（参考 OpenTelemetry Semantic Conventions for LLM）
- [ ] 实现 `crates/observability/` crate，封装 OTLP 导出
- [ ] 在 engine、LLM client、tool executor 关键路径埋点
- [ ] 支持配置导出目标（Jaeger/OTLP collector/本地文件）
- [ ] 添加 `MIMOFAN_OTEL_ENDPOINT` 环境变量配置

**验证**: 能在 Jaeger UI 中看到完整的请求链路

#### 1.1 Crate 结构设计

```
crates/observability/
├── Cargo.toml
└── src/
    ├── lib.rs              # 公开 API
    ├── config.rs           # 配置结构体
    ├── trace.rs            # TraceId, SpanId, TraceContext
    ├── span.rs             # Span, SpanKind, SpanStatus
    ├── metrics.rs          # OTel Metrics（token usage, latency）
    ├── export/
    │   ├── mod.rs
    │   ├── otlp.rs         # OTLP gRPC/HTTP 导出
    │   ├── json.rs         # 本地 JSON 文件导出
    │   └── stdout.rs       # stdout 调试导出
    └── semantic/
        ├── mod.rs
        ├── gen_ai.rs       # GenAI 语义约定属性
        └── attributes.rs   # 自定义属性常量
```

#### 1.2 依赖配置

```toml
# crates/observability/Cargo.toml
[package]
name = "mimofan-observability"
version = "0.1.0"
edition = "2024"

[dependencies]
opentelemetry = { workspace = true, features = ["trace"] }
opentelemetry_sdk = { workspace = true, features = ["trace", "rt-tokio"] }
opentelemetry-otlp = { workspace = true, features = ["grpc-tonic"] }
tracing-opentelemetry = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }

# Metrics support
opentelemetry-otlp = { workspace = true, features = ["metrics"] }
```

#### 1.3 GenAI 语义约定实现

参考 [OTel GenAI Semantic Conventions](https://github.com/open-telemetry/semantic-conventions-genai)：

```rust
// crates/observability/src/semantic/gen_ai.rs

/// GenAI Operation Names
pub mod operation {
    pub const CHAT: &str = "chat";
    pub const TEXT_COMPLETION: &str = "text_completion";
    pub const GENERATE_CONTENT: &str = "generate_content";
    pub const EMBEDDINGS: &str = "embeddings";
}

/// GenAI Span Names
pub mod span {
    /// Inference span: `{operation} {model}`
    pub fn inference(operation: &str, model: &str) -> String {
        format!("{operation} {model}")
    }
    
    /// Tool execution span
    pub fn tool_execution(tool_name: &str) -> String {
        format!("tool.execute.{tool_name}")
    }
    
    /// Agent root span
    pub fn agent_execute() -> String {
        "agent.execute".to_string()
    }
}

/// GenAI Attribute Keys
pub mod attribute {
    pub const OPERATION_NAME: &str = "gen_ai.operation.name";
    pub const PROVIDER_NAME: &str = "gen_ai.provider.name";
    pub const REQUEST_MODEL: &str = "gen_ai.request.model";
    pub const RESPONSE_MODEL: &str = "gen_ai.response.model";
    pub const REQUEST_TEMPERATURE: &str = "gen_ai.request.temperature";
    pub const REQUEST_TOP_P: &str = "gen_ai.request.top_p";
    pub const REQUEST_MAX_TOKENS: &str = "gen_ai.request.max_tokens";
    pub const USAGE_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
    pub const USAGE_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
    pub const USAGE_TOTAL_TOKENS: &str = "gen_ai.usage.total_tokens";
    pub const FINISH_REASON: &str = "gen_ai.response.finish_reasons";
    
    // Tool attributes
    pub const TOOL_NAME: &str = "gen_ai.tool.name";
    pub const TOOL_TYPE: &str = "gen_ai.tool.type";
    pub const TOOL_CALL_ID: &str = "gen_ai.tool.call.id";
}

/// GenAI Metric Names
pub mod metric {
    pub const CLIENT_TOKEN_USAGE: &str = "gen_ai.client.token.usage";
    pub const CLIENT_OPERATION_DURATION: &str = "gen_ai.client.operation.duration";
    pub const CLIENT_TTFT: &str = "gen_ai.client.operation.time_to_first_chunk";
    pub const INVOKE_AGENT_DURATION: &str = "gen_ai.invoke_agent.duration";
    pub const INVOKE_AGENT_TOOL_CALLS: &str = "gen_ai.invoke_agent.tool_calls";
    pub const EXECUTE_TOOL_DURATION: &str = "gen_ai.execute_tool.duration";
}
```

#### 1.4 Span 层级设计

基于 GenAI 语义约定，LLM Agent 的 span 层级：

```
[Root Span] agent.execute
  ├─ [Span] chat deepseek-chat          (gen_ai.inference.client)
  │     attributes: gen_ai.operation.name="chat", gen_ai.provider.name="deepseek",
  │                 gen_ai.request.model="deepseek-chat"
  │     events: gen_ai.client.inference.operation.details (opt-in: messages)
  ├─ [Span] tool.execute.shell           (gen_ai.execute_tool.internal)
  │     attributes: gen_ai.tool.name="shell", gen_ai.tool.type="function"
  ├─ [Span] chat deepseek-chat          (gen_ai.inference.client -- 2nd call)
  │     attributes: gen_ai.usage.input_tokens=3200, gen_ai.usage.output_tokens=180
  └─ [Span] tool.execute.file_read
        attributes: gen_ai.tool.name="file_read"
```

#### 1.5 OTLP 导出实现

```rust
// crates/observability/src/export/otlp.rs

use opentelemetry::global;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use opentelemetry::KeyValue;

pub struct OtlpExporter {
    provider: SdkTracerProvider,
}

impl OtlpExporter {
    pub fn new(endpoint: &str, service_name: &str) -> anyhow::Result<Self> {
        let resource = Resource::builder()
            .with_service_name(service_name)
            .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
            .build();

        let exporter = SpanExporter::builder()
            .with_endpoint(endpoint)
            .build()?;

        let provider = SdkTracerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(exporter)
            .build();

        Ok(Self { provider })
    }

    pub fn init(&self) {
        global::set_tracer_provider(self.provider.clone());
    }

    pub fn shutdown(&self) -> anyhow::Result<()> {
        self.provider.shutdown()?;
        Ok(())
    }
}
```

#### 1.6 tracing-opentelemetry 桥接

```rust
// crates/observability/src/trace.rs

use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

pub fn init_tracing_subscriber(
    exporter: &OtlpExporter,
) -> anyhow::Result<()> {
    exporter.init();

    let tracer = global::tracer("mimofan");
    let otel_layer = OpenTelemetryLayer::new().with_tracer(tracer);

    // 注意：tracing_subscriber::set_global_default 只能调用一次
    // 需要在 main 函数中调用
    Ok(())
}

// 使用标准 tracing 宏，自动创建 OTel span
#[tracing::instrument(skip_all, fields(
    gen_ai.operation.name = "chat",
    gen_ai.provider.name = %provider,
    gen_ai.request.model = %model,
))]
pub async fn call_llm(
    provider: &str,
    model: &str,
    messages: &[Message],
) -> Result<String> {
    let result = llm_client.chat(messages).await?;

    // 添加 token 使用量到 span
    tracing::Span::current().record("gen_ai.usage.input_tokens", result.input_tokens);
    tracing::Span::current().record("gen_ai.usage.output_tokens", result.output_tokens);

    Ok(result.content)
}
```

#### 1.7 采样策略

```rust
use opentelemetry_sdk::trace::Sampler;

// 推荐配置：采样 20% 的请求，错误请求全部采样
let provider = SdkTracerProvider::builder()
    .with_sampler(Sampler::ParentBased(Box::new(
        Sampler::TraceIdRatioBased(0.2)
    )))
    .with_resource(resource)
    .with_batch_exporter(exporter)
    .build();
```

#### 1.8 配置结构

```rust
// crates/observability/src/config.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    pub enabled: bool,
    pub exporter: ExporterType,
    pub endpoint: Option<String>,
    pub service_name: String,
    pub sample_rate: f64,
    pub log_messages: bool,  // 是否记录消息内容（敏感）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExporterType {
    Otlp,
    Jaeger,
    Json,
    Stdout,
    None,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            exporter: ExporterType::None,
            endpoint: None,
            service_name: "mimofan".to_string(),
            sample_rate: 1.0,
            log_messages: false,
        }
    }
}
```

#### 1.9 集成到现有代码

关键埋点位置：

| 位置 | 文件 | Span 类型 | 属性 |
|------|------|----------|------|
| LLM 调用 | `tui/src/core/engine/*.rs` | `gen_ai.inference.client` | model, tokens, latency |
| 工具执行 | `tui/src/tools/*.rs` | `gen_ai.execute_tool.internal` | tool_name, duration |
| Agent 根 span | `tui/src/tools/subagent/mod.rs` | `agent.execute` | task_id, total_duration |
| 流式响应 | `tui/src/core/engine/streaming.rs` | 事件 | chunk_count, ttft |

---

### Phase 2: 评估框架增强

**目标**: 从离线 mock 升级为真实 LLM 评估，支持回归检测

**任务**:
- [ ] 扩展 `evals/` 目录，添加真实 LLM 评估 harness
- [ ] 实现评估结果 JSONL 持久化（`~/.mimofan/evals/`）
- [ ] 添加回归检测：对比当前 eval 与历史 baseline
- [ ] 集成 Ragas 风格的评估指标（Faithfulness/Relevancy/Correctness）
- [ ] 支持 SWE-bench 风格的编码任务基准

**验证**: 能运行 eval 并生成历史趋势报告

#### 2.1 目录结构

```
evals/
├── harness.py              # 现有：横向模型对比（保留）
├── metrics/
│   ├── __init__.py
│   ├── base.py             # 指标基类
│   ├── faithfulness.py     # 忠实度指标
│   ├── relevancy.py        # 相关性指标
│   ├── correctness.py      # 正确性指标
│   ├── context_precision.py # 上下文精确度
│   ├── context_recall.py   # 上下文召回率
│   └── llm_judge.py        # LLM-as-Judge 通用实现
├── benchmarks/
│   ├── __init__.py
│   ├── base.py             # 基准任务基类
│   ├── coding/             # 编码任务
│   │   ├── __init__.py
│   │   ├── swe_bench.py    # SWE-bench 任务
│   │   └── tasks.jsonl     # 任务定义
│   ├── general/            # 通用任务
│   │   ├── __init__.py
│   │   └── tasks.jsonl
│   └── rag/                # RAG 任务
│       ├── __init__.py
│       └── tasks.jsonl
├── runner.py               # 评估运行器
├── reporter.py             # 报告生成（HTML/JSON）
├── regression.py           # 回归检测
├── storage.py              # 结果持久化
└── config.py               # 评估配置
```

#### 2.2 Ragas 风格指标实现

##### Faithfulness（忠实度）

```python
# evals/metrics/faithfulness.py

from ragas.metrics import faithfulness
from ragas.llms import LangchainLLMWrapper
from ragas.embeddings import LangchainEmbeddingsWrapper
from langchain_openai import ChatOpenAI, OpenAIEmbeddings

class FaithfulnessMetric:
    """
    计算公式: Faithfulness = supported_claims / total_claims
    
    实现流程:
    1. LLM 将答案拆解为原子语句
    2. 对每个语句与上下文进行 NLI 验证
    3. 计算被支持语句的比例
    """
    
    def __init__(self, llm=None, embeddings=None):
        self.llm = llm or LangchainLLMWrapper(ChatOpenAI(model="gpt-4o"))
        self.embeddings = embeddings or LangchainEmbeddingsWrapper(
            OpenAIEmbeddings()
        )
    
    def evaluate(
        self,
        question: str,
        answer: str,
        contexts: list[str],
    ) -> float:
        """
        Args:
            question: 用户问题
            answer: 模型回答
            contexts: 检索到的上下文列表
        
        Returns:
            忠实度分数 (0.0 ~ 1.0)
        """
        from datasets import Dataset
        
        dataset = Dataset.from_dict({
            "user_input": [question],
            "response": [answer],
            "retrieved_contexts": [contexts],
        })
        
        result = faithfulness.evaluate(
            dataset=dataset,
            llm=self.llm,
            embeddings=self.embeddings,
        )
        
        return result["faithfulness"]
```

##### Answer Relevancy（答案相关性）

```python
# evals/metrics/relevancy.py

from ragas.metrics import answer_relevancy

class RelevancyMetric:
    """
    计算公式: score = mean(cosine_similarities) × int(not all_noncommittal)
    
    实现流程:
    1. LLM 根据答案反向生成多个候选问题
    2. 用嵌入模型计算原始问题与生成问题的余弦相似度
    3. 取所有相似度的均值
    """
    
    def __init__(self, llm=None, embeddings=None):
        self.llm = llm
        self.embeddings = embeddings
    
    def evaluate(
        self,
        question: str,
        answer: str,
    ) -> float:
        """
        Args:
            question: 用户问题
            answer: 模型回答
        
        Returns:
            相关性分数 (0.0 ~ 1.0)
        """
        from datasets import Dataset
        
        dataset = Dataset.from_dict({
            "user_input": [question],
            "response": [answer],
        })
        
        result = answer_relevancy.evaluate(
            dataset=dataset,
            llm=self.llm,
            embeddings=self.embeddings,
        )
        
        return result["answer_relevancy"]
```

##### Factual Correctness（事实正确性）

```python
# evals/metrics/correctness.py

from ragas.metrics import FactualCorrectness

class CorrectnessMetric:
    """
    计算公式: 基于 NLI 的 Precision/Recall/F1
    
    实现流程:
    1. 分别将 response 和 reference 分解为原子声明
    2. 双向 NLI 验证得到 TP、FP、FN
    3. 计算 F-beta 分数
    """
    
    def __init__(self, llm=None, mode="f1", atomicity="high"):
        self.llm = llm
        self.mode = mode  # "precision", "recall", "f1"
        self.atomicity = atomicity  # "low", "high"
    
    def evaluate(
        self,
        answer: str,
        reference: str,
    ) -> float:
        """
        Args:
            answer: 模型回答
            reference: 标准答案
        
        Returns:
            事实正确性分数 (0.0 ~ 1.0)
        """
        from datasets import Dataset
        
        dataset = Dataset.from_dict({
            "response": [answer],
            "reference": [reference],
        })
        
        fc = FactualCorrectness(
            mode=self.mode,
            atomicity=self.atomicity,
        )
        
        result = fc.evaluate(
            dataset=dataset,
            llm=self.llm,
        )
        
        return result["factual_correctness"]
```

#### 2.3 评估运行器

```python
# evals/runner.py

import json
from pathlib import Path
from datetime import datetime
from typing import Any

from .metrics.faithfulness import FaithfulnessMetric
from .metrics.relevancy import RelevancyMetric
from .metrics.correctness import CorrectnessMetric
from .storage import EvalStorage

class EvalRunner:
    """评估运行器：执行基准任务并收集指标"""
    
    def __init__(
        self,
        model: str,
        provider: str,
        storage_dir: Path = Path.home() / ".mimofan" / "evals",
    ):
        self.model = model
        self.provider = provider
        self.storage = EvalStorage(storage_dir)
        
        # 初始化指标
        self.faithfulness = FaithfulnessMetric()
        self.relevancy = RelevancyMetric()
        self.correctness = CorrectnessMetric()
    
    def run_task(
        self,
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """运行单个评估任务"""
        # 1. 调用模型
        response = self._call_model(
            task["question"],
            task.get("context"),
        )
        
        # 2. 计算指标
        metrics = {}
        
        if "reference" in task:
            metrics["faithfulness"] = self.faithfulness.evaluate(
                question=task["question"],
                answer=response,
                contexts=task.get("contexts", []),
            )
            metrics["correctness"] = self.correctness.evaluate(
                answer=response,
                reference=task["reference"],
            )
        
        metrics["relevancy"] = self.relevancy.evaluate(
            question=task["question"],
            answer=response,
        )
        
        return {
            "task_id": task["id"],
            "model": self.model,
            "provider": self.provider,
            "question": task["question"],
            "response": response,
            "reference": task.get("reference"),
            "metrics": metrics,
            "timestamp": datetime.utcnow().isoformat(),
        }
    
    def run_benchmark(
        self,
        benchmark_path: Path,
    ) -> list[dict[str, Any]]:
        """运行完整基准测试"""
        results = []
        
        with open(benchmark_path) as f:
            for line in f:
                task = json.loads(line)
                result = self.run_task(task)
                results.append(result)
        
        # 持久化结果
        self.storage.save_results(
            model=self.model,
            results=results,
        )
        
        return results
    
    def _call_model(
        self,
        question: str,
        context: str | None = None,
    ) -> str:
        """调用模型获取回答"""
        # 实际实现中调用 mimofan 的 LLM client
        # 这里简化为直接调用 API
        pass
```

#### 2.4 回归检测

```python
# evals/regression.py

from pathlib import Path
from typing import Any
import json

class RegressionDetector:
    """回归检测：对比当前 eval 与历史 baseline"""
    
    def __init__(self, storage_dir: Path):
        self.storage_dir = storage_dir
    
    def detect(
        self,
        current_results: list[dict[str, Any]],
        baseline_id: str | None = None,
        threshold: float = 0.05,  # 5% 退化阈值
    ) -> dict[str, Any]:
        """
        检测回归
        
        Args:
            current_results: 当前评估结果
            baseline_id: baseline 评估 ID（None 则用最近一次）
            threshold: 退化阈值（相对变化）
        
        Returns:
            回归检测报告
        """
        # 加载 baseline
        baseline = self._load_baseline(baseline_id)
        
        if not baseline:
            return {
                "status": "no_baseline",
                "message": "No baseline found for comparison",
            }
        
        # 计算指标变化
        changes = self._compare_results(baseline, current_results)
        
        # 检测退化
        regressions = []
        improvements = []
        
        for metric_name, change in changes.items():
            if change["relative_change"] < -threshold:
                regressions.append({
                    "metric": metric_name,
                    "baseline": change["baseline_value"],
                    "current": change["current_value"],
                    "change_percent": change["relative_change"] * 100,
                })
            elif change["relative_change"] > threshold:
                improvements.append({
                    "metric": metric_name,
                    "baseline": change["baseline_value"],
                    "current": change["current_value"],
                    "change_percent": change["relative_change"] * 100,
                })
        
        return {
            "status": "regression" if regressions else "ok",
            "regressions": regressions,
            "improvements": improvements,
            "summary": self._generate_summary(regressions, improvements),
        }
    
    def _compare_results(
        self,
        baseline: list[dict],
        current: list[dict],
    ) -> dict[str, Any]:
        """对比两组结果的指标均值"""
        baseline_metrics = self._aggregate_metrics(baseline)
        current_metrics = self._aggregate_metrics(current)
        
        changes = {}
        for metric_name in baseline_metrics:
            if metric_name in current_metrics:
                baseline_val = baseline_metrics[metric_name]
                current_val = current_metrics[metric_name]
                relative_change = (current_val - baseline_val) / baseline_val
                
                changes[metric_name] = {
                    "baseline_value": baseline_val,
                    "current_value": current_val,
                    "absolute_change": current_val - baseline_val,
                    "relative_change": relative_change,
                }
        
        return changes
    
    def _aggregate_metrics(
        self,
        results: list[dict],
    ) -> dict[str, float]:
        """聚合多条结果的指标均值"""
        metric_sums = {}
        metric_counts = {}
        
        for result in results:
            for metric_name, value in result.get("metrics", {}).items():
                metric_sums[metric_name] = metric_sums.get(metric_name, 0) + value
                metric_counts[metric_name] = metric_counts.get(metric_name, 0) + 1
        
        return {
            name: metric_sums[name] / metric_counts[name]
            for name in metric_sums
        }
    
    def _load_baseline(self, baseline_id: str | None) -> list[dict] | None:
        """加载 baseline 结果"""
        # 实现从存储加载
        pass
    
    def _generate_summary(
        self,
        regressions: list,
        improvements: list,
    ) -> str:
        """生成摘要"""
        if not regressions:
            return "No regressions detected."
        
        lines = [f"Detected {len(regressions)} regression(s):"]
        for r in regressions:
            lines.append(
                f"  - {r['metric']}: {r['baseline']:.3f} → {r['current']:.3f} "
                f"({r['change_percent']:+.1f}%)"
            )
        
        return "\n".join(lines)
```

#### 2.5 结果持久化

```python
# evals/storage.py

import json
from pathlib import Path
from datetime import datetime
from typing import Any

class EvalStorage:
    """评估结果持久化存储"""
    
    def __init__(self, storage_dir: Path):
        self.storage_dir = storage_dir
        self.storage_dir.mkdir(parents=True, exist_ok=True)
    
    def save_results(
        self,
        model: str,
        results: list[dict[str, Any]],
    ) -> str:
        """
        保存评估结果
        
        Args:
            model: 模型名称
            results: 评估结果列表
        
        Returns:
            eval_id
        """
        eval_id = datetime.utcnow().strftime("%Y%m%d_%H%M%S")
        
        # 创建目录
        eval_dir = self.storage_dir / model / eval_id
        eval_dir.mkdir(parents=True, exist_ok=True)
        
        # 保存结果 JSONL
        with open(eval_dir / "results.jsonl", "w") as f:
            for result in results:
                f.write(json.dumps(result) + "\n")
        
        # 保存汇总
        summary = self._generate_summary(results)
        with open(eval_dir / "summary.json", "w") as f:
            json.dump(summary, f, indent=2)
        
        return eval_id
    
    def load_results(
        self,
        model: str,
        eval_id: str,
    ) -> list[dict[str, Any]]:
        """加载评估结果"""
        eval_dir = self.storage_dir / model / eval_id
        results = []
        
        with open(eval_dir / "results.jsonl") as f:
            for line in f:
                results.append(json.loads(line))
        
        return results
    
    def list_evals(self, model: str) -> list[str]:
        """列出模型的所有评估 ID"""
        model_dir = self.storage_dir / model
        if not model_dir.exists():
            return []
        
        return sorted([
            d.name for d in model_dir.iterdir()
            if d.is_dir()
        ])
    
    def _generate_summary(
        self,
        results: list[dict[str, Any]],
    ) -> dict[str, Any]:
        """生成评估汇总"""
        metric_sums = {}
        metric_counts = {}
        
        for result in results:
            for metric_name, value in result.get("metrics", {}).items():
                metric_sums[metric_name] = metric_sums.get(metric_name, 0) + value
                metric_counts[metric_name] = metric_counts.get(metric_name, 0) + 1
        
        return {
            "total_tasks": len(results),
            "metrics": {
                name: {
                    "mean": metric_sums[name] / metric_counts[name],
                    "min": min(
                        r["metrics"][name]
                        for r in results
                        if name in r.get("metrics", {})
                    ),
                    "max": max(
                        r["metrics"][name]
                        for r in results
                        if name in r.get("metrics", {})
                    ),
                }
                for name in metric_sums
            },
            "timestamp": datetime.utcnow().isoformat(),
        }
```

#### 2.6 持久化格式

```jsonl
{"eval_id": "20260802_001", "model": "deepseek-v4", "timestamp": "2026-08-02T10:00:00Z", "metrics": {"faithfulness": 0.92, "relevancy": 0.88, "correctness": 0.95}, "latency_p50_ms": 1200, "cost_usd": 0.015}
```

---

### Phase 3: 成本分析增强

**目标**: 实现会话级/跨会话成本聚合，支持预算告警

**任务**:
- [ ] 扩展 cost_status，添加 SQLite 持久化（复用 mimofan-state）
- [ ] 实现日/周/月维度的成本聚合查询
- [ ] 添加按模型/提供商的成本拆分报表
- [ ] 实现成本预算配置和超限告警
- [ ] 在 TUI 中添加 `/cost` 命令查看成本统计

**验证**: 能查询历史成本并设置预算告警

#### 3.1 数据库 Schema

```sql
-- 成本记录表
CREATE TABLE cost_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    trace_id TEXT,
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    tokens_input INTEGER NOT NULL,
    tokens_output INTEGER NOT NULL,
    cache_hits INTEGER DEFAULT 0,
    cache_misses INTEGER DEFAULT 0,
    cost_usd REAL NOT NULL,
    cost_cny REAL,
    latency_ms INTEGER,
    timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);

-- 日聚合表（定时任务生成）
CREATE TABLE cost_daily (
    date TEXT NOT NULL,
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    total_tokens_in INTEGER DEFAULT 0,
    total_tokens_out INTEGER DEFAULT 0,
    total_cache_hits INTEGER DEFAULT 0,
    total_cost_usd REAL DEFAULT 0,
    total_cost_cny REAL DEFAULT 0,
    request_count INTEGER DEFAULT 0,
    avg_latency_ms REAL DEFAULT 0,
    PRIMARY KEY (date, model, provider)
);

-- 月聚合表
CREATE TABLE cost_monthly (
    month TEXT NOT NULL,  -- YYYY-MM
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    total_tokens_in INTEGER DEFAULT 0,
    total_tokens_out INTEGER DEFAULT 0,
    total_cost_usd REAL DEFAULT 0,
    total_cost_cny REAL DEFAULT 0,
    request_count INTEGER DEFAULT 0,
    PRIMARY KEY (month, model, provider)
);

-- 预算配置表
CREATE TABLE budget_config (
    id INTEGER PRIMARY KEY,
    scope TEXT NOT NULL,  -- 'daily', 'monthly', 'total'
    limit_usd REAL NOT NULL,
    limit_cny REAL,
    alert_threshold REAL DEFAULT 0.8,  -- 80% 时告警
    alert_webhook TEXT,
    enabled BOOLEAN DEFAULT 1
);

-- 索引
CREATE INDEX idx_cost_entries_session ON cost_entries(session_id);
CREATE INDEX idx_cost_entries_trace ON cost_entries(trace_id);
CREATE INDEX idx_cost_entries_model ON cost_entries(model, provider);
CREATE INDEX idx_cost_entries_timestamp ON cost_entries(timestamp);
```

#### 3.2 Rust 实现

```rust
// crates/state/src/cost.rs

use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use chrono::{NaiveDate, NaiveDateTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    pub id: Option<i64>,
    pub session_id: String,
    pub trace_id: Option<String>,
    pub model: String,
    pub provider: String,
    pub tokens_input: i64,
    pub tokens_output: i64,
    pub cache_hits: i64,
    pub cost_usd: f64,
    pub cost_cny: Option<f64>,
    pub latency_ms: Option<i64>,
    pub timestamp: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostDaily {
    pub date: NaiveDate,
    pub model: String,
    pub provider: String,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub total_cost_usd: f64,
    pub request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub id: Option<i64>,
    pub scope: String,  // "daily", "monthly", "total"
    pub limit_usd: f64,
    pub alert_threshold: f64,
    pub alert_webhook: Option<String>,
    pub enabled: bool,
}

pub struct CostStore {
    conn: Connection,
}

impl CostStore {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    /// 插入成本记录
    pub fn insert(&self, entry: &CostEntry) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO cost_entries (
                session_id, trace_id, model, provider,
                tokens_input, tokens_output, cache_hits,
                cost_usd, cost_cny, latency_ms, timestamp
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.session_id,
                entry.trace_id,
                entry.model,
                entry.provider,
                entry.tokens_input,
                entry.tokens_output,
                entry.cache_hits,
                entry.cost_usd,
                entry.cost_cny,
                entry.latency_ms,
                entry.timestamp,
            ],
        )?;
        
        Ok(self.conn.last_insert_rowid())
    }

    /// 查询日聚合
    pub fn get_daily_summary(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<CostDaily>> {
        let mut stmt = self.conn.prepare(
            "SELECT 
                DATE(timestamp) as date,
                model,
                provider,
                SUM(tokens_input) as total_tokens_in,
                SUM(tokens_output) as total_tokens_out,
                SUM(cost_usd) as total_cost_usd,
                COUNT(*) as request_count
            FROM cost_entries
            WHERE DATE(timestamp) BETWEEN ?1 AND ?2
            GROUP BY date, model, provider
            ORDER BY date DESC"
        )?;
        
        let rows = stmt.query_map(params![start_date, end_date], |row| {
            Ok(CostDaily {
                date: row.get(0)?,
                model: row.get(1)?,
                provider: row.get(2)?,
                total_tokens_in: row.get(3)?,
                total_tokens_out: row.get(4)?,
                total_cost_usd: row.get(5)?,
                request_count: row.get(6)?,
            })
        })?;
        
        rows.collect()
    }

    /// 检查预算
    pub fn check_budget(
        &self,
        scope: &str,
    ) -> Result<Option<BudgetAlert>> {
        let limit = self.conn.query_row(
            "SELECT limit_usd, alert_threshold 
             FROM budget_config 
             WHERE scope = ?1 AND enabled = 1",
            params![scope],
            |row| {
                Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?))
            },
        );
        
        match limit {
            Ok((limit_usd, threshold)) => {
                let current = self.get_current_spend(scope)?;
                let usage_ratio = current / limit_usd;
                
                if usage_ratio >= threshold {
                    Ok(Some(BudgetAlert {
                        scope: scope.to_string(),
                        limit_usd,
                        current_spend: current,
                        usage_ratio,
                        exceeded: usage_ratio >= 1.0,
                    }))
                } else {
                    Ok(None)
                }
            }
            Err(_) => Ok(None),
        }
    }

    fn get_current_spend(&self, scope: &str) -> Result<f64> {
        let query = match scope {
            "daily" => "SELECT COALESCE(SUM(cost_usd), 0) 
                        FROM cost_entries 
                        WHERE DATE(timestamp) = DATE('now')",
            "monthly" => "SELECT COALESCE(SUM(cost_usd), 0) 
                          FROM cost_entries 
                          WHERE strftime('%Y-%m', timestamp) = strftime('%Y-%m', 'now')",
            _ => "SELECT COALESCE(SUM(cost_usd), 0) 
                  FROM cost_entries",
        };
        
        self.conn.query_row(query, [], |row| row.get(0))
    }
}

#[derive(Debug, Clone)]
pub struct BudgetAlert {
    pub scope: String,
    pub limit_usd: f64,
    pub current_spend: f64,
    pub usage_ratio: f64,
    pub exceeded: bool,
}
```

#### 3.3 TUI `/cost` 命令

```rust
// crates/tui/src/commands/cost.rs

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

pub fn render_cost_dashboard(
    frame: &mut Frame,
    area: Rect,
    cost_store: &CostStore,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // 今日概览
            Constraint::Min(10),   // 详细统计
        ])
        .split(area);

    // 今日概览
    render_daily_summary(frame, chunks[0], cost_store);
    
    // 详细统计表
    render_cost_table(frame, chunks[1], cost_store);
}

fn render_daily_summary(
    frame: &mut Frame,
    area: Rect,
    cost_store: &CostStore,
) {
    let today = cost_store.get_today_summary().unwrap_or_default();
    
    let block = Block::default()
        .title("今日成本概览")
        .borders(Borders::ALL);
    
    let text = format!(
        "总花费: ${:.4} | 请求数: {} | 平均延迟: {:.0}ms | Token: {}",
        today.total_cost_usd,
        today.request_count,
        today.avg_latency_ms,
        today.total_tokens_in + today.total_tokens_out,
    );
    
    let paragraph = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(Color::White));
    
    frame.render_widget(paragraph, area);
}
```

#### 3.4 预算配置

```toml
# ~/.mimofan/settings.toml

[budget]
daily_limit_usd = 10.0
monthly_limit_usd = 200.0
alert_threshold = 0.8  # 80% 时告警
alert_webhook = "https://hooks.slack.com/..."

[budget.models]
# 按模型设置单独预算
"deepseek-v4-pro" = { daily_limit_usd = 5.0 }
"claude-3.5-sonnet" = { daily_limit_usd = 20.0 }
```

---

### Phase 4: 实验管理

**目标**: 支持 A/B 测试和 prompt 版本管理

**任务**:
- [ ] 设计实验配置格式（实验组/对照组/指标）
- [ ] 实现 prompt 版本控制（`~/.mimofan/prompts/`）
- [ ] 添加实验运行器，支持并行对比
- [ ] 实现结果持久化和统计分析
- [ ] 在 TUI 中添加 `/experiment` 命令

**验证**: 能运行 A/B 实验并生成对比报告

#### 4.1 Prompt 版本控制

```
~/.mimofan/prompts/
├── system/
│   ├── v1.txt              # 初始版本
│   ├── v2.txt              # 优化版本
│   ├── v3.txt              # 最新版本
│   └── manifest.json       # 版本元数据
├── tools/
│   ├── shell_v1.txt
│   └── shell_v2.txt
└── templates/
    └── code_review_v1.txt
```

**manifest.json**:
```json
{
  "current_version": "v3",
  "versions": {
    "v1": {
      "created_at": "2026-07-01T10:00:00Z",
      "description": "初始 system prompt",
      "author": "user",
      "tags": ["baseline"]
    },
    "v2": {
      "created_at": "2026-07-15T14:30:00Z",
      "description": "优化了代码生成能力",
      "author": "user",
      "tags": ["optimization"]
    },
    "v3": {
      "created_at": "2026-08-02T09:00:00Z",
      "description": "增加安全约束",
      "author": "user",
      "tags": ["security", "latest"]
    }
  }
}
```

#### 4.2 实验配置格式

```json
{
  "experiment_id": "prompt-v2-test",
  "name": "System Prompt V2 对比测试",
  "description": "测试新的 system prompt 对输出质量的影响",
  "created_at": "2026-08-02T10:00:00Z",
  "status": "running",
  "groups": [
    {
      "name": "control",
      "model": "deepseek-v4-pro",
      "prompt_version": "v1",
      "traffic_percent": 50
    },
    {
      "name": "treatment",
      "model": "deepseek-v4-pro",
      "prompt_version": "v2",
      "traffic_percent": 50
    }
  ],
  "metrics": ["faithfulness", "relevancy", "latency_p50", "cost"],
  "min_samples": 100,
  "max_duration_hours": 24
}
```

#### 4.3 实验运行器

```python
# evals/experiment.py

import json
import random
from pathlib import Path
from datetime import datetime
from typing import Any

class ExperimentRunner:
    """A/B 实验运行器"""
    
    def __init__(self, config_path: Path):
        self.config = self._load_config(config_path)
        self.results_dir = config_path.parent / "results"
        self.results_dir.mkdir(exist_ok=True)
    
    def run(
        self,
        tasks: list[dict[str, Any]],
        num_samples: int = 100,
    ) -> dict[str, Any]:
        """
        运行实验
        
        Args:
            tasks: 评估任务列表
            num_samples: 每组采样数
        
        Returns:
            实验结果
        """
        results = {group["name"]: [] for group in self.config["groups"]}
        
        for i, task in enumerate(tasks[:num_samples]):
            # 随机分配到实验组
            group = self._assign_group()
            
            # 使用对应配置调用模型
            response = self._call_model(
                task=task,
                model=group["model"],
                prompt_version=group["prompt_version"],
            )
            
            # 记录结果
            results[group["name"]].append({
                "task_id": task["id"],
                "response": response,
                "group": group["name"],
                "timestamp": datetime.utcnow().isoformat(),
            })
        
        # 计算统计指标
        stats = self._compute_stats(results)
        
        # 保存结果
        self._save_results(results, stats)
        
        return stats
    
    def _assign_group(self) -> dict:
        """根据流量比例分配实验组"""
        rand = random.random() * 100
        cumulative = 0
        
        for group in self.config["groups"]:
            cumulative += group["traffic_percent"]
            if rand <= cumulative:
                return group
        
        return self.config["groups"][-1]
    
    def _call_model(
        self,
        task: dict,
        model: str,
        prompt_version: str,
    ) -> str:
        """调用模型"""
        # 加载对应版本的 prompt
        prompt_path = Path.home() / ".mimofan" / "prompts" / "system" / f"{prompt_version}.txt"
        system_prompt = prompt_path.read_text()
        
        # 调用 LLM API
        # 实际实现中集成 mimofan 的 LLM client
        pass
    
    def _compute_stats(
        self,
        results: dict[str, list],
    ) -> dict[str, Any]:
        """计算统计指标"""
        stats = {}
        
        for group_name, group_results in results.items():
            # 计算各指标的均值、标准差
            metrics = {}
            for metric in self.config["metrics"]:
                values = [
                    r.get("metrics", {}).get(metric, 0)
                    for r in group_results
                ]
                if values:
                    metrics[metric] = {
                        "mean": sum(values) / len(values),
                        "std": (sum((x - sum(values)/len(values))**2 for x in values) / len(values)) ** 0.5,
                        "min": min(values),
                        "max": max(values),
                    }
            
            stats[group_name] = {
                "sample_count": len(group_results),
                "metrics": metrics,
            }
        
        return stats
    
    def _save_results(
        self,
        results: dict,
        stats: dict,
    ):
        """保存实验结果"""
        timestamp = datetime.utcnow().strftime("%Y%m%d_%H%M%S")
        
        # 保存详细结果
        with open(self.results_dir / f"{timestamp}_results.json", "w") as f:
            json.dump(results, f, indent=2)
        
        # 保存统计摘要
        with open(self.results_dir / f"{timestamp}_stats.json", "w") as f:
            json.dump(stats, f, indent=2)
```

#### 4.4 统计显著性检验

```python
# evals/significance.py

from scipy import stats
import numpy as np

def test_significance(
    control_values: list[float],
    treatment_values: list[float],
    alpha: float = 0.05,
) -> dict[str, Any]:
    """
    检验两组数据的统计显著性
    
    Args:
        control_values: 对照组指标值
        treatment_values: 实验组指标值
        alpha: 显著性水平
    
    Returns:
        检验结果
    """
    # t 检验
    t_stat, p_value = stats.ttest_ind(
        control_values,
        treatment_values,
    )
    
    # 效应量 (Cohen's d)
    pooled_std = np.sqrt(
        (np.var(control_values) + np.var(treatment_values)) / 2
    )
    cohens_d = (np.mean(treatment_values) - np.mean(control_values)) / pooled_std
    
    # 置信区间
    ci_lower = np.mean(treatment_values) - np.mean(control_values) - 1.96 * pooled_std / np.sqrt(len(treatment_values))
    ci_upper = np.mean(treatment_values) - np.mean(control_values) + 1.96 * pooled_std / np.sqrt(len(treatment_values))
    
    return {
        "t_statistic": t_stat,
        "p_value": p_value,
        "significant": p_value < alpha,
        "cohens_d": cohens_d,
        "effect_size": "small" if abs(cohens_d) < 0.5 else "medium" if abs(cohens_d) < 0.8 else "large",
        "confidence_interval": (ci_lower, ci_upper),
        "control_mean": np.mean(control_values),
        "treatment_mean": np.mean(treatment_values),
        "improvement": np.mean(treatment_values) - np.mean(control_values),
        "improvement_percent": (np.mean(treatment_values) - np.mean(control_values)) / np.mean(control_values) * 100,
    }
```

---

### Phase 5: 用户反馈与告警

**目标**: 建立用户反馈采集和异常告警机制

**任务**:
- [ ] 实现反馈采集 API（`/feedback` 命令）
- [ ] 添加反馈数据持久化和关联到 trace
- [ ] 实现延迟/错误率异常检测
- [ ] 支持 Webhook/Slack 通知
- [ ] 在 TUI 中添加 `/feedback` 命令

**验证**: 能采集反馈并收到异常告警

#### 5.1 反馈数据模型

```rust
// crates/state/src/feedback.rs

use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use chrono::NaiveDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    pub id: Option<i64>,
    pub trace_id: String,
    pub session_id: String,
    pub rating: Option<i8>,        // -1, 0, 1 (thumbs down/neutral/up)
    pub comment: Option<String>,
    pub tags: Vec<String>,
    pub created_at: NaiveDateTime,
}

pub struct FeedbackStore {
    conn: Connection,
}

impl FeedbackStore {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    /// 插入反馈
    pub fn insert(&self, feedback: &Feedback) -> Result<i64> {
        let tags_json = serde_json::to_string(&feedback.tags).unwrap_or_default();
        
        self.conn.execute(
            "INSERT INTO feedback (
                trace_id, session_id, rating, comment, tags, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                feedback.trace_id,
                feedback.session_id,
                feedback.rating,
                feedback.comment,
                tags_json,
                feedback.created_at,
            ],
        )?;
        
        Ok(self.conn.last_insert_rowid())
    }

    /// 查询 trace 的反馈
    pub fn get_by_trace(&self, trace_id: &str) -> Result<Vec<Feedback>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, trace_id, session_id, rating, comment, tags, created_at
             FROM feedback
             WHERE trace_id = ?1
             ORDER BY created_at DESC"
        )?;
        
        let rows = stmt.query_map(params![trace_id], |row| {
            let tags_str: String = row.get(5)?;
            let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
            
            Ok(Feedback {
                id: row.get(0)?,
                trace_id: row.get(1)?,
                session_id: row.get(2)?,
                rating: row.get(3)?,
                comment: row.get(4)?,
                tags,
                created_at: row.get(6)?,
            })
        })?;
        
        rows.collect()
    }

    /// 获取反馈统计
    pub fn get_stats(
        &self,
        start_date: NaiveDateTime,
        end_date: NaiveDateTime,
    ) -> Result<FeedbackStats> {
        let mut stmt = self.conn.prepare(
            "SELECT 
                COUNT(*) as total,
                SUM(CASE WHEN rating = 1 THEN 1 ELSE 0 END) as positive,
                SUM(CASE WHEN rating = -1 THEN 1 ELSE 0 END) as negative,
                SUM(CASE WHEN rating = 0 THEN 1 ELSE 0 END) as neutral
             FROM feedback
             WHERE created_at BETWEEN ?1 AND ?2"
        )?;
        
        let stats = stmt.query_row(params![start_date, end_date], |row| {
            Ok(FeedbackStats {
                total: row.get(0)?,
                positive: row.get(1)?,
                negative: row.get(2)?,
                neutral: row.get(3)?,
            })
        })?;
        
        Ok(stats)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackStats {
    pub total: i64,
    pub positive: i64,
    pub negative: i64,
    pub neutral: i64,
}
```

#### 5.2 TUI `/feedback` 命令

```rust
// crates/tui/src/commands/feedback.rs

use crossterm::event::{KeyCode, KeyEvent};

pub fn handle_feedback_input(
    key: KeyEvent,
    state: &mut FeedbackState,
) -> Option<FeedbackAction> {
    match key.code {
        KeyCode::Char('1') => Some(FeedbackAction::Rate(-1)),
        KeyCode::Char('2') => Some(FeedbackAction::Rate(0)),
        KeyCode::Char('3') => Some(FeedbackAction::Rate(1)),
        KeyCode::Char('c') => {
            state.enter_comment_mode();
            None
        }
        KeyCode::Enter => {
            if state.has_rating() {
                Some(FeedbackAction::Submit)
            } else {
                None
            }
        }
        KeyCode::Esc => Some(FeedbackAction::Cancel),
        _ => None,
    }
}

pub fn render_feedback_dialog(
    frame: &mut Frame,
    area: Rect,
    state: &FeedbackState,
) {
    let block = Block::default()
        .title("用户反馈")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::DarkGray));
    
    let text = vec![
        Line::from("请评价刚才的回答："),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [1] ", Style::default().fg(Color::Red)),
            Span::raw("👎 不满意"),
        ]),
        Line::from(vec![
            Span::styled("  [2] ", Style::default().fg(Color::Yellow)),
            Span::raw("😐 一般"),
        ]),
        Line::from(vec![
            Span::styled("  [3] ", Style::default().fg(Color::Green)),
            Span::raw("👍 满意"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [C] ", Style::default().fg(Color::Cyan)),
            Span::raw("添加评论"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  [Enter] 提交  [Esc] 取消",
            Style::default().fg(Color::Gray),
        )),
    ];
    
    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);
    
    frame.render_widget(paragraph, area);
}
```

#### 5.3 异常检测

```rust
// crates/observability/src/alerts.rs

use std::collections::VecDeque;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub latency_threshold_ms: u64,      // 延迟阈值
    pub error_rate_threshold: f64,      // 错误率阈值
    pub window_size_minutes: u64,       // 检测窗口大小
    pub min_samples: usize,             // 最小样本数
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            latency_threshold_ms: 5000,
            error_rate_threshold: 0.1,  // 10%
            window_size_minutes: 5,
            min_samples: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub alert_type: AlertType,
    pub severity: Severity,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub metrics: AlertMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertType {
    HighLatency,
    HighErrorRate,
    CostBudgetExceeded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertMetrics {
    pub current_value: f64,
    pub threshold: f64,
    pub window_samples: usize,
}

pub struct AlertDetector {
    config: AlertConfig,
    latency_buffer: VecDeque<(DateTime<Utc>, u64)>,
    error_buffer: VecDeque<(DateTime<Utc>, bool)>,
    webhook_url: Option<String>,
}

impl AlertDetector {
    pub fn new(config: AlertConfig, webhook_url: Option<String>) -> Self {
        Self {
            config,
            latency_buffer: VecDeque::new(),
            error_buffer: VecDeque::new(),
            webhook_url,
        }
    }

    /// 记录请求结果
    pub fn record_request(
        &mut self,
        latency_ms: u64,
        is_error: bool,
    ) -> Vec<Alert> {
        let now = Utc::now();
        
        // 添加到缓冲区
        self.latency_buffer.push_back((now, latency_ms));
        self.error_buffer.push_back((now, is_error));
        
        // 清理过期数据
        self.cleanup_buffer(now);
        
        // 检测异常
        let mut alerts = Vec::new();
        
        if let Some(alert) = self.check_latency(now) {
            alerts.push(alert);
        }
        
        if let Some(alert) = self.check_error_rate(now) {
            alerts.push(alert);
        }
        
        alerts
    }

    fn cleanup_buffer(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::minutes(self.config.window_size_minutes as i64);
        
        while let Some((time, _)) = self.latency_buffer.front() {
            if *time < cutoff {
                self.latency_buffer.pop_front();
            } else {
                break;
            }
        }
        
        while let Some((time, _)) = self.error_buffer.front() {
            if *time < cutoff {
                self.error_buffer.pop_front();
            } else {
                break;
            }
        }
    }

    fn check_latency(&self, now: DateTime<Utc>) -> Option<Alert> {
        if self.latency_buffer.len() < self.config.min_samples {
            return None;
        }
        
        let avg_latency: u64 = self.latency_buffer.iter()
            .map(|(_, latency)| latency)
            .sum::<u64>() / self.latency_buffer.len() as u64;
        
        if avg_latency > self.config.latency_threshold_ms {
            Some(Alert {
                alert_type: AlertType::HighLatency,
                severity: if avg_latency > self.config.latency_threshold_ms * 2 {
                    Severity::Critical
                } else {
                    Severity::Warning
                },
                message: format!(
                    "平均延迟过高: {}ms (阈值: {}ms)",
                    avg_latency, self.config.latency_threshold_ms
                ),
                timestamp: now,
                metrics: AlertMetrics {
                    current_value: avg_latency as f64,
                    threshold: self.config.latency_threshold_ms as f64,
                    window_samples: self.latency_buffer.len(),
                },
            })
        } else {
            None
        }
    }

    fn check_error_rate(&self, now: DateTime<Utc>) -> Option<Alert> {
        if self.error_buffer.len() < self.config.min_samples {
            return None;
        }
        
        let error_count = self.error_buffer.iter()
            .filter(|(_, is_error)| *is_error)
            .count();
        
        let error_rate = error_count as f64 / self.error_buffer.len() as f64;
        
        if error_rate > self.config.error_rate_threshold {
            Some(Alert {
                alert_type: AlertType::HighErrorRate,
                severity: if error_rate > self.config.error_rate_threshold * 2 {
                    Severity::Critical
                } else {
                    Severity::Warning
                },
                message: format!(
                    "错误率过高: {:.1}% (阈值: {:.1}%)",
                    error_rate * 100,
                    self.config.error_rate_threshold * 100
                ),
                timestamp: now,
                metrics: AlertMetrics {
                    current_value: error_rate,
                    threshold: self.config.error_rate_threshold,
                    window_samples: self.error_buffer.len(),
                },
            })
        } else {
            None
        }
    }

    /// 发送 Webhook 通知
    pub async fn send_webhook(&self, alert: &Alert) -> anyhow::Result<()> {
        let webhook_url = match &self.webhook_url {
            Some(url) => url,
            None => return Ok(()),
        };
        
        let payload = serde_json::json!({
            "text": format!(
                "🚨 *Mimofan Alert*\n\n*Type:* {:?}\n*Severity:* {:?}\n*Message:* {}\n*Time:* {}",
                alert.alert_type,
                alert.severity,
                alert.message,
                alert.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            ),
        });
        
        reqwest::Client::new()
            .post(webhook_url)
            .json(&payload)
            .send()
            .await?;
        
        Ok(())
    }
}
```

#### 5.4 配置示例

```toml
# ~/.mimofan/settings.toml

[alerts]
enabled = true
latency_threshold_ms = 5000
error_rate_threshold = 0.1
window_size_minutes = 5
min_samples = 10
webhook_url = "https://hooks.slack.com/services/..."

[alerts.notifications]
# 通知方式
desktop = true      # 桌面通知
webhook = true      # Webhook 通知
log = true          # 写入日志
```

---

## 参考资源

- [OpenTelemetry Semantic Conventions for LLM](https://github.com/open-telemetry/semantic-conventions/pull/1183)
- [Langfuse Architecture](https://langfuse.com/docs/architecture)
- [Ragas Metrics](https://docs.ragas.io/en/latest/concepts/metrics/)
- [Helicone Experiments](https://docs.helicone.ai/features/experiments)
- [OpenLLMetry SDK](https://github.com/traceloop/openllmetry)

---

## 成功标准

1. 能在 Jaeger 中查看完整的 LLM 请求链路
2. 能运行真实 LLM 评估并生成历史趋势
3. 能查询跨会话的成本统计
4. 能运行 A/B 实验并对比结果
5. 能采集用户反馈并关联到具体 trace

---

## 里程碑规划

| 阶段 | 预计时间 | 交付物 |
|------|---------|--------|
| Phase 1 | 2-3 周 | crates/observability crate + Jaeger 集成 |
| Phase 2 | 2-3 周 | evals 框架增强 + 回归检测 |
| Phase 3 | 1-2 周 | 成本持久化 + /cost 命令 |
| Phase 4 | 2-3 周 | 实验管理 + /experiment 命令 |
| Phase 5 | 1-2 周 | 反馈采集 + 告警通知 |
| **总计** | **8-13 周** | 完整的可观测性与评估能力 |

---

**Labels**: `enhancement`, `observability`, `evaluation`, `roadmap`
**Milestone**: v0.9.0 或下一个主要版本
