# mimofan 用户指南

本指南帮助你从零开始使用 mimofan，以 **小米 MiMo** 为主线，其他服务商作为参考。

---

## 1. 快速开始

### 1.1 安装

**方式一：npm/pnpm（推荐）**

```bash
# pnpm（推荐）
pnpm add -g mimofan

# 或 npm
npm install -g mimofan
```

**方式二：直接下载二进制**

```bash
curl -fsSL https://mimofan.net/install.sh | sh
```

**方式三：源码编译（需要 Rust 1.88+）**

```bash
cargo install mimofan-cli --locked
cargo install mimofan --locked
```

### 1.2 配置

```bash
# 创建配置目录
mkdir -p ~/.mimofan

# 复制配置文件
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

### 1.3 验证安装

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

## 3. 三种工作模式

在 TUI 界面按 `Tab` 切换：**Plan → Agent → YOLO**

| 模式 | 说明 | 适用场景 |
|------|------|----------|
| **Plan** | 设计优先。只读工具可用，禁止 shell 和文件写入 | 调研、分析、规划 |
| **Agent** | 多步工具使用。shell 需配置 `allow_shell = true`，每次调用需审批 | 日常开发 |
| **YOLO** | 全自动。启用 shell + 信任模式，所有工具自动通过 | 可信仓库的快速任务 |

### 工具可用性对比

| 工具类型 | Plan | Agent | YOLO |
|---------|:----:|:-----:|:----:|
| 只读文件/搜索/诊断 | ✅ | ✅ | ✅ |
| 文件写入/补丁 | ❌ | ✅ | ✅ |
| Shell 命令 | ❌ | 需审批 | ✅ |
| 付费/外部服务 | 需审批 | 需审批 | 自动通过 |

---

## 4. 多 Profile 管理

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

[profiles.work]
provider = "openai"
api_key = "YOUR_OPENAI_KEY"
default_text_model = "gpt-4o"
```

启动时切换：

```bash
mimofan --profile deepseek
# 或
MIMOFAN_PROFILE=deepseek mimofan
```

---

## 5. 配置文件说明

### 5.1 配置文件位置

| 文件 | 用途 |
|------|------|
| `~/.mimofan/config.toml` | 主配置 |
| `~/.mimofan/settings.toml` | UI 偏好 |
| `~/.mimofan/permissions.toml` | 工具权限规则 |
| `.mimofan/config.toml` | 项目级覆盖（优先级更高） |
| `.mimofan/constitution.json` | 项目级提示词追加 |

### 5.2 常用配置项

| 字段 | 说明 | 默认值 |
|------|------|--------|
| `provider` | 服务商 ID | `xiaomi-mimo` |
| `api_key` | API 密钥 | 必填 |
| `base_url` | API 地址 | 服务商默认 |
| `default_text_model` | 默认模型 | - |
| `allow_shell` | 允许执行 shell | `false` |
| `approval_policy` | 审批策略 | `on-request` |
| `max_subagents` | 最大子代理数 | `10` |
| `reasoning_effort` | 推理等级 | `max` |

### 5.3 环境变量

| 变量 | 说明 |
|------|------|
| `MIMOFAN_PROVIDER` | 服务商 ID |
| `MIMOFAN_MODEL` | 默认模型 |
| `MIMOFAN_BASE_URL` | API 地址 |
| `MIMO_API_KEY` | MiMo API 密钥 |
| `DEEPSEEK_API_KEY` | DeepSeek API 密钥 |
| `MIMOFAN_HOME` | 数据目录（默认 `~/.mimofan`） |

---

## 6. 子 Agent

子 agent 是 mimofan 的嵌套任务执行机制。父 agent 通过 `agent` 工具启动子 agent，子 agent 完成后返回结果。

### 角色类型

| 角色 | 说明 | 可写文件 | 可执行 shell |
|------|------|:--------:|:-----------:|
| `general` | 通用，默认角色 | ✅ | ✅ |
| `explore` | 只读探索，快速定位代码 | ❌ | 只读 |
| `plan` | 分析规划，不执行 | 最少 | 最少 |
| `review` | 代码审查，给出评分 | ❌ | 只读 |
| `implementer` | 实现具体改动 | ✅ | ✅ |
| `verifier` | 运行测试验证 | ❌ | 测试专用 |

### 使用方式

```
agent(
  task: "找出所有调用 Foo.bar 的地方",
  type: "explore"
)
```

### 并发控制

- 最大并发数：`max_subagents`（默认 20，范围 1-20）
- 子 agent 不能再嵌套 `agent` 工具（叶子节点）

---

## 7. MCP 外部工具

mimofan 支持通过 MCP（Model Context Protocol）加载外部工具。

### 初始化

```bash
mimofan mcp init    # 创建 MCP 配置文件
mimofan mcp list    # 查看已配置的服务器
mimofan mcp tools   # 查看可用工具
```

### 配置文件

默认路径：`~/.mimofan/mcp.json`

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

MCP 工具名称格式：`mcp__<server>__<tool>`

---

## 8. 常用命令

### 诊断命令

```bash
mimofan doctor         # 检查配置 / API key / 连接
mimofan doctor --json  # JSON 输出，便于脚本处理
mimofan auth status    # 当前生效的认证
```

### 会话管理

```bash
mimofan --resume <ID>    # 恢复指定会话
mimofan --continue       # 继续最近会话
mimofan --workspace <DIR> # 指定工作区
```

---

## 9. 其他服务商配置

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

### 国内常用服务商

```toml
# 硅基流动
provider = "siliconflow"

# OpenRouter
provider = "openrouter"

# 阿里云百炼
provider = "openai"
[providers.openai]
api_key = "YOUR_DASHSCOPE_API_KEY"
base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"
```

完整 provider 列表和模型 ID 见 `config.example.toml` 顶部注释。

---

## 10. 快捷键

### 全局

| 快捷键 | 功能 |
|--------|------|
| `F1` / `Ctrl-/` | 帮助 |
| `Ctrl-K` | 命令面板 |
| `Ctrl-C` | 取消当前操作 / 关闭弹窗 |
| `Tab` | 切换模式（Plan → Agent → YOLO） |
| `Ctrl-R` | 恢复会话 |
| `Ctrl-L` | 刷新屏幕 |
| `Esc` | 关闭弹窗 / 取消操作 |

### 编辑器

| 快捷键 | 功能 |
|--------|------|
| `Enter` | 发送消息 |
| `Alt-Enter` / `Ctrl-J` | 换行 |
| `↑` / `↓` | 历史记录 |
| `Tab` | 补全（`/` 命令 / `@` 提及） |

### `@` 提及

输入 `@<部分文件名>` 打开文件补全。`↑`/`↓` 选择，`Tab` 或 `Enter` 确认。

---

## 11. 常见问题

### npm 下载超时

设置镜像源或使用 cargo 安装：

```bash
npm config set registry https://registry.npmmirror.com
```

### `mimofan update` 被墙

通过 CNB 镜像安装：

```bash
cargo install --git https://cnb.cool/mimofan.net/mimofan --tag vX.Y.Z mimofan-cli --locked --force
```

### macOS 提示"无法验证开发者"

```bash
xattr -d com.apple.quarantine ~/.local/bin/mimofan
```

---

## 12. 进一步阅读

- [ARCHITECTURE.md](ARCHITECTURE.md) — 架构说明（面向开发者）
- [docs/INSTALL.md](docs/INSTALL.md) — 详细安装指南
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) — 配置文件字段参考
- [docs/MODES.md](docs/MODES.md) — TUI 模式详解
- [docs/MCP.md](docs/MCP.md) — MCP 外部工具桥接
- [docs/SUBAGENTS.md](docs/SUBAGENTS.md) — 子 Agent 用法
- [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) — 快捷键完整列表
- [docs/PROMPTS.md](docs/PROMPTS.md) — 提示词工程详解
