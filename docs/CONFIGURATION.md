# 配置说明

mimofan 从 TOML 配置文件 + 环境变量读取配置。启动时也会加载工作区的 `.env` 文件。

## 项目指令文件

- **`AGENTS.md`** — 项目级 agent 指令（跨 agent 生效）
- **`.mimofan/constitution.json`** — mimofan 专属的优先级/信任策略

> `WHALE.md` 已弃用，仍兼容读取但不再推荐。

## 配置文件位置

默认：`~/.mimofan/config.toml`

覆盖方式：
- CLI：`mimofan --config /path/to/config.toml`
- 环境变量：`MIMOFAN_CONFIG_PATH=/path/to/config.toml`

### 项目级覆盖

工作区下的 `.mimofan/config.toml` 可覆盖部分全局配置：

| 字段 | 作用 |
|------|------|
| `model` | 覆盖默认模型 |
| `reasoning_effort` | 强制推理等级 |
| `approval_policy` | 收紧审批策略 |
| `sandbox_mode` | 收紧沙箱策略 |
| `max_subagents` | 限制子 agent 并发数 |
| `allow_shell` | `false` 可禁用 shell |

## Profile 配置

同一文件可定义多个 profile：

```toml
api_key = "DEFAULT_KEY"
default_text_model = "deepseek-v4-pro"

[profiles.work]
api_key = "WORK_KEY"
base_url = "https://api.deepseek.com/beta"

[profiles.xiaomi]
provider = "xiaomi-mimo"
api_key = "YOUR_KEY"
```

选择 profile：`mimofan --profile work` 或 `DEEPSEEK_PROFILE=work`

## 核心配置项

| 字段 | 类型 | 说明 |
|------|------|------|
| `provider` | string | 服务商 ID（`deepseek`, `xiaomi-mimo`, `openai` 等） |
| `api_key` | string | API 密钥 |
| `base_url` | string | API 地址 |
| `default_text_model` | string | 默认模型 |
| `allow_shell` | bool | 是否允许执行 shell 命令（默认 `false`） |
| `approval_policy` | string | 审批策略：`on-request` / `untrusted` / `never` |
| `sandbox_mode` | string | 沙箱模式：`read-only` / `workspace-write` / `danger-full-access` |
| `max_subagents` | int | 最大子 agent 数（1-20） |
| `reasoning_effort` | string | 推理等级：`off` / `low` / `medium` / `high` / `max` |

## 环境变量

### 核心变量

| 变量 | 说明 |
|------|------|
| `MIMOFAN_PROVIDER` | 服务商 ID |
| `MIMOFAN_MODEL` | 默认模型 |
| `MIMOFAN_BASE_URL` | API 地址 |
| `DEEPSEEK_API_KEY` | API 密钥 |
| `MIMOFAN_HOME` | 数据目录（默认 `~/.mimofan`） |

### 服务商专用变量

每个服务商都有对应的 `*_API_KEY`、`*_BASE_URL`、`*_MODEL` 变量，格式为 `<PROVIDER>_API_KEY`。常用：

- `XIAOMI_MIMO_API_KEY` / `MIMO_API_KEY`
- `OPENAI_API_KEY`
- `ANTHROPIC_API_KEY`
- `SILICONFLOW_API_KEY`

完整列表见 `config.example.toml`。

### 功能变量

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_ALLOW_SHELL` | `1` 启用 shell |
| `DEEPSEEK_APPROVAL_POLICY` | 审批策略 |
| `DEEPSEEK_MEMORY` | `on` 启用用户记忆 |
| `DEEPSEEK_LOG_LEVEL` | 日志级别 |
| `NO_ANIMATIONS` | `1` 禁用动画 |

## 自定义 OpenAI 兼容网关

```toml
provider = "openai"
default_text_model = "your-model-id"

[providers.openai]
api_key = "YOUR_KEY"
base_url = "https://your-gateway.example/v1"
```

阿里云百炼 / DashScope：

```toml
provider = "openai"
[providers.openai]
api_key = "YOUR_DASHSCOPE_API_KEY"
base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"
```

## 设置文件（UI 偏好）

`~/.mimofan/settings.toml` 存储 UI 偏好：

| 字段 | 说明 | 默认值 |
|------|------|--------|
| `theme` | 主题 | `system` |
| `auto_compact` | 自动压缩上下文 | 模型感知 |
| `show_thinking` | 显示思考过程 | - |
| `show_tool_details` | 显示工具详情 | - |
| `locale` | UI 语言 | `auto` |
| `default_mode` | 默认模式 | `agent` |
| `cost_currency` | 货币单位 | `usd` |

## 生命周期钩子

```toml
[[hooks.hooks]]
event = "tool_call_before"
command = "~/.mimofan/hooks/check.sh"
condition = { type = "tool_name", name = "exec_shell" }
```

支持的事件：`message_submit`、`tool_call_before`、`turn_end`、`subagent_spawn`、`subagent_complete`

## Feature Flags

```toml
[features]
shell_tool = true
subagents = true
web_search = true
mcp = true
```

单次覆盖：`mimofan --enable web_search` / `mimofan --disable subagents`

## Web 搜索

默认使用 DuckDuckGo，无需 API key。可选：`bing`、`tavily`、`bocha`、`metaso`、`searxng`、`baidu`

```toml
[search]
provider = "duckduckgo"
```

## `mimofan doctor`

```bash
mimofan doctor        # 检查配置、API key、MCP、skills
mimofan doctor --json # JSON 输出，适合 CI
```
