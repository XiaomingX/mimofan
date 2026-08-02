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

- 📖 读懂项目结构，理解模块关系
- ✍️ 生成符合规范的代码与测试
- 🧪 写完自动跑验证，红了就修
- 🔧 给报错信息就能定位到具体代码行
- 🤫 不弹窗不打扰，只在需要你决策时才开口（每次危险操作都会先问「可以吗？」）

> 米谋不是一个替你写代码的工具，而是一个帮你把代码写好的搭档。谋定而后动，知止而有得。

---

## ⚡ 30 秒极速上手 (零配置开箱即用)

### 1. 安装

```bash
# 使用 bun 全局安装（推荐）
bun add -g mimofan

# 或使用 cargo 编译安装
cargo install --path crates/tui --locked
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

> 💡 **提示**：若未设置环境变量，首次运行 `mimofan` 会自动启动交互式配置向导帮你完成配置。

---

## 💡 常用调用方式

```bash
# 启动全屏 TUI 交互界面
mimofan

# 单次指令模式（不进入 TUI）
mimofan exec "帮我写一个正则表达式匹配邮箱"

# 自动运行系统诊断
mimofan doctor
```

---

## 核心能力

### AI 编程助手

- 自动读取代码上下文，理解项目结构
- 生成符合规范的代码和测试
- 自动运行验证命令确保正确性

### 多模型支持

| 服务商 | 模型示例 | 配置 |
|--------|----------|------|
| 小米 MiMo | mimo-v2.5-pro | 默认 |
| Anthropic (Messages API) | mimo-v2.5 | `provider = "custom"` + `/anthropic` base_url |
| DeepSeek | deepseek-chat | `provider = "custom"` |
| 通义千问 | qwen-max | `provider = "custom"` |
| OpenAI | gpt-4 | `provider = "custom"` |

### 操作模式

- **交互模式**: 全屏 TUI 终端界面
- **单次模式**: `mimofan exec` 命令行调用
- **Plan 模式**: 先列计划，审核后再执行

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

### 安全授权

AI 执行命令时会弹出授权窗口：
- `y`: 允许执行
- `n`: 拒绝操作

### 斜杠指令

- `/plan <目标>`: 生成执行计划
- `/clear`: 清屏重置
- `/help`: 查看帮助
- `/exit`: 退出

---

## 使用场景

### 场景一：添加功能 / 修复 Bug

```text
> 帮我在 src/main.rs 里加一个检查网络连接的函数，并写对应的单元测试
```

AI 会自动：
1. 读取 `src/main.rs` 分析上下文
2. 插入新函数与测试
3. 运行 `cargo test` 验证

### 场景二：快速问答

```bash
mimofan exec "用 Python 写一个简单的爬虫脚本，保存到 spider.py"
```

---

## 配置示例

### Anthropic (Messages API)

使用 Anthropic 原生 Messages API 协议，需要将 base_url 设置为以 `/anthropic` 结尾：

```toml
provider = "custom"
api_key = "你的_ANTHROPIC_API_KEY"
base_url = "https://api.xiaomimimo.com/anthropic"
default_text_model = "mimo-v2.5"
```

> ⚠️ **重要**: base_url 必须以 `/anthropic` 结尾，mimofan 会自动检测并使用 Anthropic Messages API 协议。
> 如果 base_url 不以 `/anthropic` 结尾，将使用 OpenAI Chat Completions 协议。

### OpenAI Chat Completions (MiMo 模型)

MiMo 模型兼容 OpenAI Chat Completions 接口：

```toml
provider = "custom"
api_key = "你的_XIAOMI_MIMO_API_KEY"
base_url = "https://api.xiaomimimo.com/v1"
default_text_model = "mimo-v2.5"
```

### DeepSeek

```toml
provider = "custom"
api_key = "你的_DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com/v1"
default_text_model = "deepseek-chat"
```

### 通义千问

```toml
provider = "custom"
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

---

## 文档

- [ARCHITECTURE.md](ARCHITECTURE.md) -- 系统架构设计与 DDD 改进计划
- [docs/MCP.md](docs/MCP.md) -- MCP 扩展服务
- [CLAUDE.md](CLAUDE.md) -- 开发者与 AI 协作指南

---

## 开源许可

本项目遵循 MIT License 开源协议。
