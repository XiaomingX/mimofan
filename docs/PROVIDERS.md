# 服务商列表

mimofan 支持以下服务商，通过 `provider` 字段选择。

## 选择方式

- CLI：`mimofan --provider <id>`
- TUI：`/provider <id>` 或服务商选择器
- 环境变量：`MIMOFAN_PROVIDER=<id>`
- 配置文件：`provider = "<id>"`

## 支持的服务商

| ID | TOML 表 | 协议 | API Key 环境变量 |
|---|---|---|---|
| `deepseek` | `[providers.deepseek]` | OpenAI | `DEEPSEEK_API_KEY` |
| `xiaomi-mimo` | `[providers.xiaomi_mimo]` | OpenAI | `XIAOMI_MIMO_API_KEY` |
| `openai` | `[providers.openai]` | OpenAI | `OPENAI_API_KEY` |
| `anthropic` | `[providers.anthropic]` | Anthropic | `ANTHROPIC_API_KEY` |
| `siliconflow` | `[providers.siliconflow]` | OpenAI | `SILICONFLOW_API_KEY` |
| `siliconflow-CN` | `[providers.siliconflow_cn]` | OpenAI | `SILICONFLOW_API_KEY` |
| `openrouter` | `[providers.openrouter]` | OpenAI | `OPENROUTER_API_KEY` |
| `nvidia-nim` | `[providers.nvidia_nim]` | OpenAI | `NVIDIA_API_KEY` |
| `fireworks` | `[providers.fireworks]` | OpenAI | `FIREWORKS_API_KEY` |
| `moonshot` | `[providers.moonshot]` | OpenAI | `MOONSHOT_API_KEY` |
| `zai` | `[providers.zai]` | OpenAI | `ZAI_API_KEY` |
| `stepfun` | `[providers.stepfun]` | OpenAI | `STEPFUN_API_KEY` |
| `minimax` | `[providers.minimax]` | OpenAI | `MINIMAX_API_KEY` |
| `deepinfra` | `[providers.deepinfra]` | OpenAI | `DEEPINFRA_API_KEY` |
| `qianfan` | `[providers.qianfan]` | OpenAI | `QIANFAN_API_KEY` |
| `ollama` | `[providers.ollama]` | OpenAI | 可选 |
| `sglang` | `[providers.sglang]` | OpenAI | 可选 |
| `vllm` | `[providers.vllm]` | OpenAI | 可选 |

> `ollama`、`sglang`、`vllm` 为本地推理，默认无需 API key。

## 默认配置示例

### DeepSeek

```toml
provider = "deepseek"
api_key = "YOUR_DEEPSEEK_KEY"
# 默认 base_url: https://api.deepseek.com/beta
# 默认模型: deepseek-v4-pro
```

### 小米 MiMo

```toml
provider = "xiaomi-mimo"
api_key = "YOUR_KEY"
# Token Plan (tp-...) 默认: https://token-plan-sgp.xiaomimimo.com/v1
# 按量付费默认: https://api.xiaomimimo.com/v1
# 默认模型: mimo-v2.5-pro
```

### 硅基流动 SiliconFlow

```toml
provider = "siliconflow"
api_key = "YOUR_KEY"
# 默认: https://api.siliconflow.com/v1
# 默认模型: deepseek-ai/DeepSeek-V4-Pro
```

### Ollama（本地）

```toml
provider = "ollama"
# 默认: http://localhost:11434/v1
# 模型名直接使用 ollama 标签，如 qwen2.5-coder:7b
```

## 视觉模型

图像分析通过 `[vision_model]` 单独配置：

```toml
[features]
vision_model = true

[vision_model]
model = "mimo-v2.5"
api_key = "YOUR_KEY"
base_url = "https://api.xiaomimimo.com/v1"
```

## 环境变量优先级

配置查找顺序：CLI `--api-key` → 配置文件 → 系统密钥环 → 环境变量

查看当前状态：`mimofan auth status`
