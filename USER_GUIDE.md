# Mimofan 用户指南

本指南以 **小米 MiMo** 为主线，从零走通 mimofan 的配置与使用。其他服务商作为附录参考。

---

## 1. 快速开始（以 MiMo 为例）

### 1.1 准备配置文件

```bash
mkdir -p ~/.mimofan
cp config.example.toml ~/.mimofan/config.toml
```

### 1.2 配置 MiMo（最小可用配置）

编辑 `~/.mimofan/config.toml`，把顶部 provider 与 api_key 改成 MiMo：

```toml
provider = "xiaomi-mimo"
api_key = "YOUR_XIAOMI_KEY"
base_url = "https://api.xiaomimimo.com/v1"   # 按量付费 (sk-)
# base_url = "https://token-plan-sgp.xiaomimimo.com/v1"  # Token Plan (tp-)
default_text_model = "mimo-v2.5-pro"
```

或者直接用环境变量，省去改配置文件：

```bash
export MIMO_API_KEY="YOUR_XIAOMI_KEY"
export MIMO_BASE_URL="https://api.xiaomimimo.com/v1"
export MIMOFAN_MODEL="mimo-v2.5-pro"
export MIMOFAN_PROVIDER="xiaomi-mimo"
```

### 1.3 验证连接

```bash
mimofan doctor       # 检查 provider、api_key、网络联通
mimofan auth status  # 查看当前生效的认证信息
```

### 1.4 启动

```bash
# TUI 交互式终端
mimofan

# CLI 单次对话
mimofan-cli "用 Python 写一个 hello world"

# 指定 profile（见下文）
mimofan --profile work
```

走通以上 4 步，即可用 MiMo 驱动 mimofan。

---

## 2. Profile：多环境并存

把不同服务商放进同一份配置的不同 profile，免去重复切换：

```toml
# 顶层仍是默认 provider / model（兼容老配置）
provider = "xiaomi-mimo"
api_key = "DEFAULT_KEY"
default_text_model = "mimo-v2.5-pro"

[profiles.work]
api_key = "WORK_KEY"
base_url = "https://api.xiaomimimo.com/v1"

[profiles.backup-deepseek]
provider = "deepseek"
api_key = "DEEPSEEK_KEY"
base_url = "https://api.deepseek.com/beta"
default_text_model = "deepseek-v4-pro"
```

切换方式：

```bash
mimofan --profile backup-deepseek
# 或
DEEPSEEK_PROFILE=work mimofan
```

---

## 3. 三端用法

### TUI（交互终端）

```bash
mimofan                              # 默认启动
mimofan --provider xiaomi-mimo       # 临时切换服务商
mimofan --config /path/to/config.toml
mimofan --profile work
```

### CLI（命令行）

```bash
mimofan-cli "帮我写一个 Hello World"
echo "解释这段代码" | mimofan-cli
mimofan-cli --model mimo-v2.5-pro "快速回答"
```

### 子代理

子代理由 TUI / CLI 自动调用，无需单独启动。控制并发：

```toml
max_subagents = 10   # 1-20
```

---

## 4. 常用配置项速查

| 字段 | 说明 | 默认值 |
|------|------|--------|
| `provider` | 服务商 ID | `deepseek` |
| `api_key` | API 密钥 | 必填 |
| `base_url` | API 地址 | 服务商默认 |
| `default_text_model` | 默认模型 | `deepseek-v4-pro` |
| `allow_shell` | 允许执行 shell | `true` |
| `approval_policy` | 审批策略 | `on-request` |
| `max_subagents` | 最大子代理数 | `10` |
| `reasoning_effort` | 推理等级 | `max` |

> 想用 MiMo 时，把 `provider` 改成 `xiaomi-mimo`，并把 `default_text_model` 设为 `mimo-v2.5-pro`。

---

## 5. 诊断命令

```bash
mimofan doctor         # 检查配置 / API key / 连接
mimofan doctor --json  # JSON 输出，便于脚本处理
mimofan auth status    # 当前生效的认证
```

---

## 6. 配置文件位置

| 文件 | 用途 |
|------|------|
| `~/.mimofan/config.toml` | 主配置 |
| `~/.mimofan/settings.toml` | UI 偏好 |
| `~/.mimofan/permissions.toml` | 工具权限规则 |
| `.mimofan/config.toml` | 项目级覆盖 |
| `AGENTS.md` | 项目级 agent 指令 |

---

## 附录 A：其他常用服务商

写法与 MiMo 一致：选 `provider`、填 `api_key`，可改 `base_url` / `model`。

### DeepSeek
```toml
provider = "deepseek"
api_key = "YOUR_DEEPSEEK_API_KEY"
# base_url = "https://api.deepseek.com/beta"
# default_text_model = "deepseek-v4-pro"
```
环境变量：`DEEPSEEK_API_KEY`

### Anthropic
```toml
provider = "anthropic"
api_key = "sk-ant-xxxxxxxx"
# base_url = "https://api.anthropic.com"
# default_text_model = "claude-sonnet-4-6"
```
环境变量：`ANTHROPIC_API_KEY`

### 通用 OpenAI 兼容
```toml
provider = "openai"
api_key = "YOUR_KEY"
base_url = "https://api.openai.com/v1"
default_text_model = "gpt-4o"
```
环境变量：`OPENAI_API_KEY`

### 硅基流动 SiliconFlow
```toml
provider = "siliconflow"
api_key = "YOUR_KEY"
# base_url = "https://api.siliconflow.com/v1"
# default_text_model = "deepseek-ai/DeepSeek-V4-Pro"
```
环境变量：`SILICONFLOW_API_KEY`

### OpenRouter
```toml
provider = "openrouter"
api_key = "YOUR_KEY"
# base_url = "https://openrouter.ai/api/v1"
# default_text_model = "deepseek/deepseek-v4-pro"
```
环境变量：`OPENROUTER_API_KEY`

### 月之暗面 Moonshot/Kimi
```toml
provider = "moonshot"
api_key = "YOUR_KEY"
# base_url = "https://api.moonshot.ai/v1"
# default_text_model = "kimi-k2.6"
```
环境变量：`MOONSHOT_API_KEY`

### Ollama（本地推理）
```toml
provider = "ollama"
# base_url = "http://localhost:11434/v1"
default_text_model = "qwen2.5-coder:7b"
```

> 完整 provider 列表与更多模型 ID（NVIDIA NIM、Novita、Fireworks、Arcee、SGLang、vLLM、HuggingFace、Together、Qianfan、StepFun、Z.AI、Codex 等）请直接看 `config.example.toml` 顶部注释与 `[providers.*]` 段。