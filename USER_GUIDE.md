# mimofan 用户指南

本指南帮助你从零开始使用 mimofan，以 **小米 MiMo** 为主线，其他服务商作为参考。

---

## 1. 快速开始

### 1.1 安装

```bash
# 推荐：pnpm 安装
pnpm add -g mimofan

# 或 npm
npm install -g mimofan

# 或直接下载二进制
curl -fsSL https://mimofan.net/install.sh | sh
```

源码编译（需要 Rust 1.88+）：

```bash
cargo install mimofan-cli --locked
```

### 1.2 配置

```bash
mkdir -p ~/.mimofan
cp config.example.toml ~/.mimofan/config.toml
```

编辑 `~/.mimofan/config.toml`，配置 MiMo：

```toml
provider = "xiaomi-mimo"
api_key = "YOUR_MIMO_API_KEY"
base_url = "https://api.xiaomimimo.com/v1"
default_text_model = "mimo-v2.5-pro"
```

或使用环境变量：

```bash
export MIMO_API_KEY="YOUR_MIMO_API_KEY"
export MIMO_BASE_URL="https://api.xiaomimimo.com/v1"
export MIMOFAN_MODEL="mimo-v2.5-pro"
export MIMOFAN_PROVIDER="xiaomi-mimo"
```

### 1.3 验证

```bash
mimofan doctor       # 检查配置、API key、网络连接
mimofan auth status  # 查看当前生效的认证信息
```

### 1.4 启动

```bash
# TUI 交互式终端（推荐）
mimofan

# CLI 单次对话
mimofan-cli "用 Python 写一个 hello world"

# 指定 profile
mimofan --profile work
```

---

## 2. 三种使用形态

### TUI 终端界面（推荐）

交互式全功能界面，支持即时反馈、流式输出、上下文记忆。

```bash
mimofan
mimofan --provider xiaomi-mimo  # 临时切换
mimofan --config /path/to/config.toml
```

### CLI 命令行

适合脚本、自动化、单次任务。

```bash
mimofan-cli "帮我写一个 Hello World"
echo "解释这段代码" | mimofan-cli
mimofan-cli --model mimo-v2.5-pro "快速回答"
```

### HTTP 服务

嵌入到其他系统或 IDE 插件。

```bash
mimofan app-server --bind 127.0.0.1:8787
# 或 stdio 模式
mimofan app-server --stdio
```

---

## 3. 多 Profile 管理

把不同场景配置为独立 Profile，免去反复修改：

```toml
[profiles.mimo]
provider = "xiaomi-mimo"
api_key = "YOUR_MIMO_KEY"
default_text_model = "mimo-v2.5-pro"

[profiles.deepseek]
provider = "deepseek"
api_key = "YOUR_DEEPSEEK_KEY"
default_text_model = "deepseek-v4-pro"
```

启动时切换：

```bash
mimofan --profile deepseek
# 或
MIMOFAN_PROFILE=deepseek mimofan
```

---

## 4. 常用配置

| 字段 | 说明 | 默认值 |
|------|------|--------|
| `provider` | 服务商 ID | `xiaomi-mimo` |
| `api_key` | API 密钥 | 必填 |
| `base_url` | API 地址 | 服务商默认 |
| `default_text_model` | 默认模型 | - |
| `allow_shell` | 允许执行 shell | `true` |
| `approval_policy` | 审批策略 | `on-request` |
| `max_subagents` | 最大子代理数 | `10` |
| `reasoning_effort` | 推理等级 | `max` |

---

## 5. 配置文件位置

| 文件 | 用途 |
|------|------|
| `~/.mimofan/config.toml` | 主配置 |
| `~/.mimofan/settings.toml` | UI 偏好 |
| `~/.mimofan/permissions.toml` | 工具权限规则 |
| `.mimofan/config.toml` | 项目级覆盖（优先级更高） |
| `.mimofan/constitution.json` | 项目级提示词追加 |

---

## 6. 附录：其他服务商

所有服务商都通过 OpenAI 兼容协议接入，配置方式一致。

### DeepSeek

```toml
provider = "deepseek"
api_key = "YOUR_DEEPSEEK_KEY"
```

### Anthropic

```toml
provider = "anthropic"
api_key = "sk-ant-xxxxxx"
```

### 通用 OpenAI 兼容

```toml
provider = "openai"
api_key = "YOUR_KEY"
base_url = "https://api.openai.com/v1"
default_text_model = "gpt-4o"
```

### 其他常用服务商

```toml
# 硅基流动
provider = "siliconflow"

# OpenRouter
provider = "openrouter"

# 月之暗面 Moonshot/Kimi
provider = "moonshot"
```

完整 provider 列表和模型 ID 见 `config.example.toml` 顶部注释。

---

## 7. 诊断命令

```bash
mimofan doctor         # 检查配置 / API key / 连接
mimofan doctor --json  # JSON 输出，便于脚本处理
mimofan auth status    # 当前生效的认证
```

---

## 8. 进一步阅读

- [ARCHITECTURE.md](ARCHITECTURE.md) — 架构说明（面向开发者）
- [docs/INSTALL.md](docs/INSTALL.md) — 详细安装指南
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) — 配置文件字段参考
- [docs/MODES.md](docs/MODES.md) — TUI 模式（Plan / Agent / YOLO）
- [docs/MCP.md](docs/MCP.md) — MCP 外部工具桥接
- [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) — 快捷键
