# mimofan 配置使用说明书

本教程面向**安装 mimofan 之后的使用者**，讲解 mimofan 一共有哪些**配置文件**、每个文件里有哪些**可配置项**，以及它们如何生效。

> 若只想快速上手，直接看 [快速开始](#快速开始) 与 [配置文件总览](#配置文件总览)。
> 已有 `docs/CONFIGURATION.md` 作为精简速查表，本文档是它的完整教程版。

---

## 一、配置文件总览

mimofan 安装后，用户可自行修改的配置文件分布在「用户级（全局）」和「项目级（工作区）」两个位置：

| 配置文件 | 默认路径 | 作用域 | 是否必须 | 说明 |
|----------|----------|--------|----------|------|
| `config.toml` | `~/.mimofan/config.toml` | 用户全局 | 否（有默认值） | **主配置文件**，绝大多数开关都在这里 |
| `permissions.toml` | `~/.mimofan/permissions.toml` | 用户全局 | 否 | 细粒度命令/工具审批规则（ask 规则） |
| `mcp.json` | `~/.mimofan/mcp.json` | 用户全局 | 否 | MCP 外部工具服务器配置 |
| `settings.toml` | `~/.mimofan/settings.toml` | 用户全局 | 否 | UI 偏好（主题、语言、默认模式等） |
| `.mimofan/config.toml` | `<工作区>/.mimofan/config.toml` | 项目级 | 否 | 只覆盖部分全局配置（见[项目级覆盖](#七项目级覆盖)） |
| `.mimofan/constitution.json` | `<工作区>/.mimofan/constitution.json` | 项目级 | 否 | 项目级「章程」：优先级/信任策略，高优先级 |
| `.env` | 工作区根目录或 mimofan home | 用户/项目 | 否 | 环境变量文件，启动时自动加载 |

> 所有产品状态（记忆、笔记、快照、日志等）默认都存放在 `~/.mimofan/` 下（可用 `MIMOFAN_HOME` 或 `MIMO_HOME` 改变）。

### 配置加载优先级（由低到高）

```
内置默认值
  < 全局配置文件 (config.toml / settings.toml / permissions.toml / mcp.json)
    < 项目级覆盖 (.mimofan/config.toml，仅能收紧安全策略)
      < 环境变量 (MIMOFAN_*)
        < CLI 参数 (--config / --profile / --enable / --disable 等)
```

---

## 二、快速开始

### 1. 生成主配置文件

复制仓库里的示例配置到 home 目录后按需修改：

```bash
mkdir -p ~/.mimofan
cp config/config.example.toml ~/.mimofan/config.toml
# 编辑 ~/.mimofan/config.toml，填入你的 API Key 等
```

最简配置（使用 DeepSeek）：

```toml
provider = "openai-compatible"
api_key = "YOUR_DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com/beta"
default_text_model = "deepseek-v4-pro"
```

### 2. 指定配置文件位置（可选）

```bash
# 通过 CLI
mimofan --config /path/to/config.toml

# 通过环境变量
export MIMOFAN_CONFIG_PATH=/path/to/config.toml
```

### 3. 校验配置是否生效

```bash
mimofan doctor          # 检查配置、API key、MCP、skills
mimofan doctor --json   # JSON 输出，适合 CI
```

---

## 三、主配置文件 `config.toml` 全量配置项

主配置采用 TOML 格式，顶层是标量字段，其余按功能分组为 `[表]`。以下按区块列出**所有可配置项**。

### 3.1 顶层字段（服务商与基础）

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `provider` | string | `openai-compatible` | 线协议模式，仅 `openai-compatible` / `anthropic-compatible` / `gemini-compatible` 三种（kebab-case 规范名，无历史别名） |
| `api_key` | string | 必填非空 | API 密钥 |
| `base_url` | string | 依 provider | 服务商 API 地址 |
| `http_headers` | map | `{}` | 兼容 OpenAI 协议的网关自定义请求头，如 `{ "X-Model-Provider-Id" = "..." }` |
| `default_text_model` | string | `deepseek-v4-pro` | 默认文本模型（如 `deepseek-v4-pro` / `deepseek-v4-flash` / `mimo-v2.5-pro`） |
| `model` | string | 同 `default_text_model` | 模型别名 |
| `auth_mode` | string | - | 鉴权模式 |
| `output_mode` | string | - | 输出模式 |
| `verbosity` | string | - | 输出详细程度 |
| `log_level` | string | - | 日志级别 |
| `telemetry` | bool | - | 是否上报遥测 |
| `approval_policy` | string | `on-request` | 审批策略：`on-request` / `untrusted` / `never` |
| `sandbox_mode` | string | `workspace-write` | 沙箱模式：`read-only` / `workspace-write` / `danger-full-access` / `external-sandbox` |
| `allow_shell` | bool | `true` | 是否允许执行 shell 命令 |
| `reasoning_effort` | string | `max` | 推理等级：`off` / `low` / `medium` / `high` / `max`（`low`/`medium` 映射为 `high`） |
| `cost_currency` | string | `usd` | 成本显示货币：`usd` / `cny` |
| `max_subagents` | int | `10` | 子 Agent 最大并发（1–20） |
| `auto_allow` | list | `[]` | 自动批准的命令前缀，如 `["cargo check", "npm run"]` |
| `skills_dir` | string | `~/.mimofan/skills` | 技能目录 |
| `mcp_config_path` | string | `~/.mimofan/mcp.json` | MCP 配置文件路径 |
| `notes_path` | string | `~/.mimofan/notes.txt` | 笔记文件路径 |
| `memory_path` | string | `~/.mimofan/memory.md` | 用户记忆文件路径 |

### 3.2 `[update]` 更新检查

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `check_for_updates` | bool | `true` | 启动时后台检查新版本。内网环境可设为 `false` |

### 3.3 `[memory]` 用户记忆

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | `false` | 开启后启动时读取 `memory_path` 并注入系统提示词 |

### 3.4 `[speech]` 语音输出

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `output_dir` | string | `./speech` | 语音/TTS 输出目录 |

### 3.5 外部沙盒执行

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `sandbox_backend` | string | `none` | 远程沙盒后端：`none` / `opensandbox` |
| `sandbox_url` | string | - | 沙盒服务地址 |
| `sandbox_api_key` | string | - | 沙盒 API Key |

### 3.6 `[search]` 网页搜索

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `provider` | string | `duckduckgo` | 搜索引擎：`duckduckgo` / `bing` / `tavily` / `bocha` / `metaso` / `searxng` / `baidu` / `volcengine` / `sofya` |
| `api_key` | string | - | 部分服务（如 tavily/bocha）需要的密钥 |

### 3.7 `[network]` 网络策略

控制 `fetch_url` / `web_search` / MCP 的外网请求权限。缺失该段则不拦截（保守放行）。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `default` | string | `allow` | 默认动作：`allow` / `deny` / `prompt`（提示确认） |
| `allow` | list | `[]` | 放行域名白名单 |
| `deny` | list | `[]` | 拒绝域名黑名单 |
| `audit` | bool | `false` | 是否记录到 `~/.mimofan/audit.log` |

### 3.8 `[skills]` 技能管理

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `registry_url` | string | 社区默认 | `/skill install` 的安装源索引地址 |
| `max_install_size_bytes` | int | - | 单技能最大安装字节数 |

### 3.9 `[tui]` 终端界面

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `alternate_screen` | string | `auto` | 是否使用 TUI 全屏：`auto` / `on` / `off` |
| `mouse_capture` | bool | `true` | 启用鼠标文本选中 |
| `terminal_probe_timeout_ms` | int | `500` | 终端探测超时（毫秒） |
| `stream_chunk_timeout_secs` | int | `300` | 流式分块超时（秒） |
| `osc8_links` | bool | `true` | 渲染可点击超链接 |
| `status_items` | list | 内置默认值 | 状态栏模块：`mode` / `model` / `status` / `git_branch` / `tokens` / `cache` |

### 3.10 `[features]` 功能开关

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `shell_tool` | bool | `true` | shell 工具 |
| `subagents` | bool | `true` | 子 Agent |
| `web_search` | bool | `true` | 网页搜索 |
| `apply_patch` | bool | `true` | 补丁应用 |
| `mcp` | bool | `true` | MCP 工具 |
| `exec_policy` | bool | `true` | 执行策略引擎 |

> 单次覆盖：`mimofan --enable web_search` / `mimofan --disable subagents`

### 3.11 `[vision_model]` 视觉模型

用于 `image_analyze` 图片识别工具。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `model` | string | - | 视觉模型 ID（如 `gemini-3.1-flash-lite-preview`） |
| `api_key` | string | - | 视觉模型 API Key |

### 3.12 `[retry]` 失败重试

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | `true` | 是否启用重试 |
| `max_retries` | int | `3` | 最大重试次数 |
| `initial_delay` | float | `1.0` | 初始延迟（秒） |
| `max_delay` | float | `60.0` | 最大延迟（秒） |
| `exponential_base` | float | `2.0` | 指数退避基数 |

### 3.13 `[context]` 上下文压缩

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | `false` | 是否启用上下文压缩 |
| `verbatim_window_turns` | int | `16` | 逐字保留的最近轮数 |
| `l1_threshold` | int | `192000` | L1 压缩阈值（token） |
| `l2_threshold` | int | `384000` | L2 压缩阈值 |
| `l3_threshold` | int | `576000` | L3 压缩阈值 |
| `seam_model` | string | `deepseek-v4-flash` | 压缩用模型 |

### 3.14 `[capacity]` 并发调度与压力控制

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | `false` | 是否启用容量控制器 |
| `low_risk_max` | float | `0.50` | 低风险最大占用 |
| `medium_risk_max` | float | `0.62` | 中风险最大占用 |
| `severe_min_slack` | float | `-0.25` | 严重最小余量 |
| `severe_violation_ratio` | float | `0.40` | 严重违规比例 |
| `refresh_cooldown_turns` | int | `6` | 刷新冷却轮数 |
| `replan_cooldown_turns` | int | `5` | 重规划冷却轮数 |
| `max_replay_per_turn` | int | `1` | 每轮最大重放次数 |
| `min_turns_before_guardrail` | int | `4` | 护栏前最小轮数 |
| `profile_window` | int | `8` | 画像窗口 |
| `*_prior` | float | 见示例 | 各模型优先级（如 `deepseek_v4_pro_prior = 3.5`） |

### 3.15 `[profiles.*]` 多环境配置

通过 `mimofan --profile <name>` 或 `MIMOFAN_PROFILE=<name>` 快速切换环境。每个 profile 可覆盖 `provider` / `api_key` / `base_url` / `default_text_model` 等。

```toml
[profiles.mimo]
provider = "openai-compatible"
api_key = "YOUR_XIAOMI_KEY"
default_text_model = "mimo-v2.5-pro"

[profiles.deepseek]
provider = "openai-compatible"
api_key = "YOUR_DEEPSEEK_API_KEY"
default_text_model = "deepseek-v4-pro"
```

### 3.16 `[notifications]` 桌面通知

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `method` | string | `auto` | 通知方式：`auto` / `osc9` / `bel` / `off` |
| `threshold_secs` | int | `30` | 触发通知的最少耗时（秒） |
| `include_summary` | bool | `false` | 是否包含摘要 |

### 3.17 `[snapshots]` 工作区快照

每个回合前后创建本地项目快照，可用 `/restore` 撤销 AI 修改，存于 `~/.mimofan/snapshots/`。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | `true`（缺失时） | 是否启用 |
| `max_age_days` | int | `7` | 快照最大保留天数 |
| `max_workspace_gb` | int | `2` | 工作区最大占用（GB） |

### 3.18 `[lsp]` LSP 诊断

编辑后调用本地 LSP 服务器诊断，将报错返回给模型。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | `false` | 是否启用 |
| `poll_after_edit_ms` | int | `5000` | 编辑后轮询间隔（毫秒） |
| `max_diagnostics_per_file` | int | `20` | 单文件最大诊断数 |
| `include_warnings` | bool | `false` | 是否包含警告 |

### 3.19 `[hooks]` 生命周期钩子

工具调用、任务结束等生命周期自动执行 Shell 脚本。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `enabled` | bool | `true`（缺失时） | 是否启用 |

详细钩子写法见下方 [3.24](#324-钩子-hooks-详细写法) 或 `docs/CONFIGURATION.md`。

### 3.20 `[runtime_api]` 运行 API

控制本地 HTTP 后端 API 的跨域来源。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `cors_origins` | list | `[]` | 允许的 CORS 来源，如 `["http://localhost:5173"]` |

### 3.21 `[tools]` 工具覆写与插件

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `plugin_dir` | string | `~/.mimofan/tools` | 自定义工具插件目录 |
| `always_load` | list | `[]` | 始终加载的工具列表 |
| `overrides` | map | `{}` | 内置工具覆写为脚本，如 `{ "exec_shell" = { type = "script", path = "audit-exec-shell.sh" } }` |

### 3.22 `[fleet]` 舰队控制与角色权限

Agent Fleet 的信任、安全与角色注册表。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `default_trust_level` | string | `sandbox` | 默认信任等级 |
| `require_identity_verification` | bool | `true` | 是否要求身份校验 |
| `max_trust_level` | string | `operator` | 最大信任等级 |

### 3.23 `[[hotbar]]` 快捷操作槽位

侧边栏快捷按钮 1–8，可绑定斜杠命令。

```toml
[[hotbar]]
slot = 1
label = "voice"
action = "voice.toggle"
```

### 3.24 钩子（hooks）详细写法

```toml
[[hooks.hooks]]
event = "tool_call_before"
command = "~/.mimofan/hooks/check.sh"
condition = { type = "tool_name", name = "exec_shell" }
```

支持的事件：`message_submit` / `tool_call_before` / `turn_end` / `subagent_spawn` / `subagent_complete`。

### 3.25 自定义 OpenAI 兼容网关

```toml
provider = "openai-compatible"
default_text_model = "your-model-id"

[providers.openai]
api_key = "YOUR_KEY"
base_url = "https://your-gateway.example/v1"
```

阿里云百炼 / DashScope：

```toml
provider = "openai-compatible"
[providers.openai]
api_key = "YOUR_DASHSCOPE_API_KEY"
base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"
```

---

## 四、权限文件 `permissions.toml`

与 `config.toml` 同级，存放细粒度「ask 规则」。当某工具/命令满足规则条件时，执行前会向你确认。

```toml
# ~/.mimofan/permissions.toml
[[rules]]
type = "tool_name"
name = "exec_shell"
# 其余字段由具体规则的 schema 决定（deny_unknown_fields 严格校验）
```

规则以数组形式写入 `[[rules]]`，每条是一个 `ToolAskRule`。mimofan 会在每次相关工具调用前评估这些规则。

---

## 五、MCP 配置 `mcp.json`

路径默认 `~/.mimofan/mcp.json`（可用 `MIMOFAN_MCP_CONFIG` 或 `mcp_config_path` 覆盖）。

### stdio 服务器

```json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "your-token" }
    }
  }
}
```

### HTTP 服务器

```json
{
  "mcpServers": {
    "remote-tools": {
      "url": "https://your-mcp-server.example/sse"
    }
  }
}
```

常用命令：

```bash
mimofan mcp init   # 创建 MCP 配置文件
mimofan mcp list   # 查看已配置服务器
mimofan mcp tools  # 查看可用工具
```

TUI 中可用 `/mcp` 查看状态。MCP 工具名格式：`mcp__<server>__<tool>`，受审批策略控制。

---

## 六、UI 偏好 `settings.toml`

路径 `~/.mimofan/settings.toml`，存储界面偏好（非核心行为，改了即时影响 UI）。

| 字段 | 说明 | 默认值 |
|------|------|--------|
| `theme` | 主题 | `system` |
| `auto_compact` | 自动压缩上下文 | 模型感知 |
| `show_thinking` | 显示思考过程 | - |
| `show_tool_details` | 显示工具详情 | - |
| `locale` | UI 语言 | `auto` |
| `default_mode` | 默认模式 | `agent` |
| `cost_currency` | 货币单位 | `usd` |

---

## 七、项目级覆盖

除了全局配置，你还可以在**某个具体项目（工作区）**里放置 `.mimofan/config.toml`，只覆盖部分全局配置。这是「不信任的外部输入」，**只能收紧、不能放宽**安全策略。

### 可覆盖字段

| 字段 | 作用 |
|------|------|
| `model` | 覆盖默认模型 |
| `default_text_model` | 覆盖默认文本模型 |
| `reasoning_effort` | 强制推理等级 |
| `approval_policy` | 收紧审批策略 |
| `sandbox_mode` | 收紧沙箱策略 |
| `max_subagents` | 限制子 Agent 并发数 |
| `allow_shell` | 设为 `false` 可禁用 shell |
| `tools` | 工具相关覆盖 |
| `providers.*` | 各 provider 配置覆盖 |

> 注意：凭据、endpoint、provider 选择、auth/session、遥测、网络策略、技能源、LSP 命令表等**不允许**被项目级配置覆盖。

### `.mimofan/constitution.json`（项目章程）

项目级「最高优先级」指令文件，定义项目的优先级/信任策略。它会被渲染为独立的权威块，优先级**高于**普通 prose 指令（AGENTS.md）。可用 `mimofan project init` 初始化。

---

## 八、环境变量与 `.env`

启动时 mimofan 会加载工作区或 home 下的 `.env` 文件；`Shell 导出的环境变量`会覆盖 `.env` 文件中的同名值。

### 通用核心变量

| 变量 | 说明 |
|------|------|
| `MIMOFAN_PROVIDER` | 线协议模式，仅 `openai-compatible` / `anthropic-compatible` / `gemini-compatible` |
| `MIMOFAN_MODEL` | 默认模型 |
| `MIMOFAN_BASE_URL` | API 地址 |
| `DEEPSEEK_API_KEY` | 默认 API 密钥 |
| `MIMOFAN_HOME` / `MIMO_HOME` | 数据目录（默认 `~/.mimofan`） |
| `MIMOFAN_CONFIG_PATH` | 主配置文件路径 |
| `MIMOFAN_MCP_CONFIG` | MCP 配置文件路径 |

### 服务商专用变量

每个服务商都有 `<PROVIDER>_API_KEY` / `<PROVIDER>_BASE_URL` / `<PROVIDER>_MODEL` 形式变量。常用：

- `OPENAI_COMPATIBLE_API_KEY` / `OPENAI_COMPATIBLE_BASE_URL` / `OPENAI_COMPATIBLE_MODEL`
- `OPENAI_API_KEY`
- `ANTHROPIC_API_KEY`
- `SILICONFLOW_API_KEY`
- `NVIDIA_NIM_API_KEY` / `NVIDIA_NIM_BASE_URL` / `NVIDIA_NIM_MODEL`
- `ATLASCLOUD_API_KEY` / `ATLASCLOUD_BASE_URL` / `ATLASCLOUD_MODEL`

已弃用别名（仍可读取，优先级低于上面的通用键，仅为兼容存量配置保留）：
`XIAOMI_MIMO_API_KEY` / `MIMO_API_KEY` / `XIAOMI_MIMO_BASE_URL` / `XIAOMI_MIMO_MODEL`。

### 功能变量

| 变量 | 说明 |
|------|------|
| `MIMOFAN_ALLOW_SHELL` | `1` 启用 shell |
| `MIMOFAN_APPROVAL_POLICY` | 审批策略 |
| `MIMOFAN_MEMORY` | `on` 启用用户记忆 |
| `MIMOFAN_LOG_LEVEL` | 日志级别 |
| `NO_ANIMATIONS` | `1` 禁用动画 |
| `RUST_LOG` | TUI 轻量日志（`mimofan=debug` 等） |

### `.env` 示例（来自 `config/.env.example`）

```dotenv
# DeepSeek API（默认 provider）
# DEEPSEEK_API_KEY=

# NVIDIA NIM 托管的 DeepSeek V4
# DEEPSEEK_PROVIDER=nvidia-nim
# NVIDIA_API_KEY=
# NVIDIA_NIM_BASE_URL=https://integrate.api.nvidia.com/v1
# NVIDIA_NIM_MODEL=deepseek-ai/deepseek-v4-pro

# 安全默认值
# DEEPSEEK_APPROVAL_POLICY=on-request
# DEEPSEEK_SANDBOX_MODE=workspace-write
# DEEPSEEK_ALLOW_SHELL=true

# 日志
# DEEPSEEK_LOG_LEVEL=debug
# RUST_LOG=mimofan=debug
```

> 复制 `config/.env.example` 为 `.env` 后，只取消注释你想使用的值即可。密钥请保存在本地 `.env`，勿提交到仓库。

---

## 九、配置命令与校验

```bash
mimofan doctor          # 检查配置、API key、MCP、skills
mimofan doctor --json   # JSON 输出，适合 CI
```

修改配置后建议运行 `mimofan doctor` 确认没有语法或键值错误。

---

## 十、常见配置示例

### 例 1：只用 DeepSeek，最强推理

```toml
provider = "openai-compatible"
api_key = "YOUR_DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com/beta"
default_text_model = "deepseek-v4-pro"
reasoning_effort = "max"
cost_currency = "cny"
```

### 例 2：启用用户记忆 + 工作区快照

```toml
[memory]
enabled = true

[snapshots]
enabled = true
max_age_days = 7
max_workspace_gb = 2
```

### 例 3：严格安全（仅只读沙箱、自动拒绝危险命令）

```toml
allow_shell = false
approval_policy = "never"
sandbox_mode = "read-only"

[[rules]]
type = "tool_name"
name = "exec_shell"
```

### 例 4：多环境切换

```toml
[profiles.work]
provider = "openai-compatible"
api_key = "WORK_KEY"
base_url = "https://api.deepseek.com/beta"

[profiles.xiaomi]
provider = "openai-compatible"
api_key = "YOUR_KEY"
default_text_model = "mimo-v2.5-pro"
```

```bash
mimofan --profile work
# 或
export MIMOFAN_PROFILE=work
```

---

## 附：相关文档

- `docs/CONFIGURATION.md` — 精简速查表
- `docs/MCP.md` — MCP 外部工具配置
- `docs/MODES.md` — 运行模式
- `config/config.example.toml` — 完整带注释的主配置示例
- `config/.env.example` — 环境变量示例
