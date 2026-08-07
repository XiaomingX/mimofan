# 米谋 (mimofan)

> 终端 AI 编程助手 —— 与你并肩，谋定而后动
>
> 基于 Rust 实现，原生支持小米 MiMo 模型，兼容 DeepSeek、OpenAI、通义千问等主流大模型。

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-macOS-lightgrey.svg)]()
[![MCP Ready](https://img.shields.io/badge/MCP-Supported-green.svg)](docs/MCP.md)

---

## 米谋是什么

米谋（mimofan）是一个跑在终端里的 AI 编程搭档：你用自然语言下指令，它调用大模型思考，再用工具（读文件、改代码、跑命令）把活干完，工作流是「模型决策 → 工具执行 → 结果回灌 → 再决策」的闭环。

基于 Rust 实现，原生支持小米 MiMo，兼容 DeepSeek、OpenAI、通义千问等主流大模型。对标 Claude Code / OpenCode，差异在：**本地优先 · MIT 协议 · 默认 MiMo**。

它能做什么：

- 读懂项目结构，理解模块关系
- 生成符合规范的代码与测试
- 写完自动跑验证，红了就修
- 给报错信息就能定位到具体代码行
- 不弹窗不打扰，只在需要你决策时才开口（每次危险操作都会先问「可以吗？」）
- Spec Freeze 冻结计划，防止 Agent 偏离
- Issue Monitor 监控 GitHub Issue，自动关联 PR

> 米谋不是一个替你写代码的工具，而是一个帮你把代码写好的搭档。谋定而后动，知止而有得。

---

## 30 秒极速上手（零配置开箱即用）

### 1. 安装

```bash
# 使用 cargo 编译安装
cargo install --path crates/tui --locked

# 或下载预编译二进制
# https://github.com/XiaomingX/mimofan/releases
```

### 2. 零配置直接启动

无需手动创建配置文件！直接设置环境变量即可一键启动：

```bash
# 使用 小米 MiMo (默认)
export XIAOMI_MIMO_API_KEY="你的_XIAOMI_MIMO_API_KEY"
mimofan

# DeepSeek / OpenAI / 通义千问 等非内置服务商，请使用自定义 provider：
# 在 ~/.mimofan/config.toml 中按下方「配置示例」填写后运行 mimofan
```

> **提示**：若未设置环境变量，首次运行 `mimofan` 会自动启动交互式配置向导帮你完成配置。

---

## 常用调用方式

```bash
# 启动全屏 TUI 交互界面
mimofan

# 单次指令模式（不进入 TUI）
mimofan exec "帮我写一个正则表达式匹配邮箱"

# 自动运行系统诊断
mimofan doctor
```

---

## 高频实用场景

### 场景一：快速修复 Bug

```text
> 这个函数报错了 "index out of bounds"，帮我修复并添加测试

# AI 会自动：
# 1. 分析错误原因
# 2. 修复代码
# 3. 编写测试用例
# 4. 运行 cargo test 验证
```

### 场景二：代码重构

```text
> 把这个 500 行的函数拆分成多个小函数，保持功能不变

# AI 会自动：
# 1. 分析函数逻辑
# 2. 提取公共部分
# 3. 重构代码结构
# 4. 运行测试确保无回归
```

### 场景三：生成单元测试

```text
> 给 src/auth.rs 里的 login 函数写完整的单元测试，覆盖正常和异常情况

# AI 会自动：
# 1. 读取函数签名和逻辑
# 2. 生成测试用例
# 3. 添加 mock 和 fixture
# 4. 运行测试验证
```

### 场景四：代码审查

```text
> 帮我审查这个 PR 的代码，重点关注安全性和性能问题

# AI 会自动：
# 1. 读取 diff 变更
# 2. 分析潜在问题
# 3. 给出改进建议
```

### 场景五：文档生成

```text
> 给这个模块写 README 文档，包括使用示例和 API 说明

# AI 会自动：
# 1. 分析模块结构
# 2. 提取公开 API
# 3. 生成文档和示例
```

---

## 核心能力

### AI 编程助手

- 自动读取代码上下文，理解项目结构
- 生成符合规范的代码和测试
- 自动运行验证命令确保正确性

### 多模型支持

| 服务商 | 模型示例 | 配置（`provider` 字段） |
|--------|----------|------|
| 小米 MiMo | mimo-v2.5-pro | 默认（`openai-compatible`） |
| Anthropic (Messages API) | claude 系列 | `provider = "anthropic-compatible"` + `/anthropic` 结尾的 base_url |
| DeepSeek | deepseek-v4-pro / deepseek-v4-flash | `provider = "openai-compatible"` |
| 通义千问 | qwen-max | `provider = "openai-compatible"` + dashscope base_url |
| OpenAI | gpt-4 系列 | `provider = "openai-compatible"` |

> 模式只有三种：`openai-compatible` / `anthropic-compatible` / `gemini-compatible`（kebab-case 规范名，无历史别名）。

### 操作模式

- **交互模式**: 全屏 TUI 终端界面
- **单次模式**: `mimofan exec` 命令行调用
- **Plan 模式**: 先列计划，审核后再执行
- **YOLO 模式**: 全自动执行（信任仓库时使用）

---

## TUI 操作指南

### 基础按键

| 按键 | 功能 |
|------|------|
| Enter | 发送消息 |
| Shift+Enter / Alt+Enter | 换行 |
| Ctrl+C | 中止任务 / 退出 |
| Ctrl+L | 清空历史 |
| PageUp/PageDown | 滚动历史 |
| Tab | 切换侧边栏焦点 |

### 安全授权

AI 执行命令时会弹出授权窗口：

- `y`: 允许执行
- `n`: 拒绝操作
- `a`: 本次会话全部允许

---

## 斜杠指令大全

### 基础指令

| 指令 | 说明 | 示例 |
|------|------|------|
| `/help` | 查看帮助 | `/help` |
| `/clear` | 清屏重置 | `/clear` |
| `/exit` | 退出 | `/exit` |
| `/model` | 切换模型 | `/model deepseek-chat` |
| `/provider` | 切换服务商 | `/provider custom` |

### 模式切换

| 指令 | 说明 | 示例 |
|------|------|------|
| `/plan` | 进入规划模式 | `/plan 实现用户登录功能` |
| `/auto` | 自动模式 | `/auto` |
| `/yolo` | 全自动模式 | `/yolo` |
| `/fast` | 快速模式 | `/fast` |
| `/normal` | 恢复正常 | `/normal` |

### 代码管理

| 指令 | 说明 | 示例 |
|------|------|------|
| `/freeze` | 冻结计划 | `/freeze 只修复登录 bug` |
| `/unfreeze` | 解冻计划 | `/unfreeze` |
| `/stash` | 暂存更改 | `/stash` |
| `/anchor` | 设置锚点 | `/anchor` |

### 任务管理

| 指令 | 说明 | 示例 |
|------|------|------|
| `/plan` | 生成计划 | `/plan 重构数据库模块` |
| `/monitor` | Issue 监控 | `/monitor create fix-bug --issues 123,456` |
| `/subagents` | 子智能体 | `/subagents` |
| `/fleet` | 舰队管理 | `/fleet` |

### 其他

| 指令 | 说明 | 示例 |
|------|------|------|
| `/translate` | 翻译模式 | `/translate` |
| `/voice` | 语音输入 | `/voice` |
| `/hooks` | 钩子管理 | `/hooks` |
| `/memory` | 记忆管理 | `/memory` |

---

## 进阶功能

### Spec Freeze（计划冻结）

防止 Agent 偏离既定计划：

```text
> /freeze 只修复登录页面的 bug，不要改动其他模块

# Agent 将严格在冻结的范围内工作
# 任何越界操作都需要你确认
```

### Issue Monitor（Issue 监控）

监控 GitHub Issue 并自动关联 PR：

```text
> /monitor create fix-auth --issues 123,456 --repo owner/repo

# 监控指定 Issue 的状态变化
# 自动创建或更新关联的 PR
```

### 子智能体（Sub-agents）

并行处理复杂任务：

```text
> 帮我同时重构 auth 模块和 database 模块

# AI 会启动多个子智能体并行工作
# 每个子智能体在独立的 worktree 中操作
```

---

## 配置示例

### Anthropic (Messages API)

使用 Anthropic 原生 Messages API 协议，需要将 base_url 设置为以 `/anthropic` 结尾：

```toml
provider = "anthropic-compatible"
api_key = "你的_ANTHROPIC_API_KEY"
base_url = "https://api.xiaomimimo.com/anthropic"
default_text_model = "mimo-v2.5"
```

> **重要**: base_url 必须以 `/anthropic` 结尾，mimofan 会自动检测并使用 Anthropic Messages API 协议。
> 如果 base_url 不以 `/anthropic` 结尾，将使用 OpenAI Chat Completions 协议。

### OpenAI Chat Completions (MiMo 模型)

MiMo 模型兼容 OpenAI Chat Completions 接口：

```toml
provider = "openai-compatible"
api_key = "你的_XIAOMI_MIMO_API_KEY"
base_url = "https://api.xiaomimimo.com/v1"
default_text_model = "mimo-v2.5"
```

### DeepSeek

```toml
provider = "openai-compatible"
api_key = "你的_DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com/v1"
default_text_model = "deepseek-chat"
```

### 通义千问

```toml
provider = "openai-compatible"
api_key = "你的_DASHSCOPE_API_KEY"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
default_text_model = "qwen-max"
```

---

## 常见问题

**Q: 启动时提示 Config not found 或连接超时？**

检查 `~/.mimofan/config.toml` 文件路径和 `api_key` 是否正确。运行 `mimofan doctor` 自动诊断。

**Q: 每次执行命令都要按 y 确认，如何关闭？**

在配置中添加 `approval_policy = "yolo"` 开启全自动模式（仅在信任的仓库中使用）。

**Q: 能让 AI 读取本地文档吗？**

直接告诉 AI："读取根目录下的 ARCHITECTURE.md 并回答我的问题"。

**Q: 如何切换模型？**

使用 `/model` 命令：`/model deepseek-chat` 或 `/model gpt-4`。

**Q: 如何防止 AI 偏离计划？**

使用 `/freeze` 命令冻结当前计划：`/freeze 只修复这个 bug`。

---

## 文档

- [ARCHITECTURE.md](ARCHITECTURE.md) -- 系统架构设计
- [docs/MCP.md](docs/MCP.md) -- MCP 扩展服务
- [docs/SUBAGENTS.md](docs/SUBAGENTS.md) -- 子智能体指南
- [docs/MODES.md](docs/MODES.md) -- 操作模式详解
- [CLAUDE.md](CLAUDE.md) -- 开发者指南

---

## 贡献

欢迎贡献代码！请查看 [CONTRIBUTING.md](CONTRIBUTING.md) 了解详情。

---

## 开源许可

本项目遵循 MIT License 开源协议。
