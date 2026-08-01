# 米魔范 (mimofan)

> 终端 AI 编程助手 —— 像魔法一样帮你写代码、修 Bug、跑测试
>
> 基于 Rust 实现，原生支持小米 MiMo 模型，兼容 DeepSeek、OpenAI、通义千问等主流大模型。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-macOS-lightgrey.svg)]()
[![MCP Ready](https://img.shields.io/badge/MCP-Supported-green.svg)](docs/MCP.md)

---

## 📖 产品故事

### 为什么做米魔范？

在 AI 编程助手百花齐放的今天，我们发现一个痛点：**大多数工具只解决了"写代码"的问题，却没有解决"写好代码"的问题。**

- 有的工具生成代码很快，但不理解项目上下文，产出质量参差不齐
- 有的工具功能强大，但配置复杂，学习成本高
- 有的工具依赖特定平台，无法融入开发者的工作流

我们相信，一个好的编程助手应该像一位默契的搭档——**它知道你的项目结构，理解你的编码习惯，在你需要时出现，在你不需要时安静。**

### 米魔范是什么？

"米魔范"这个名字来自三个词的融合：

- **米** —— 源自小米 MiMo，代表我们对国产大模型生态的支持
- **魔** —— 魔法般的体验，让复杂的编程任务变得简单
- **范** —— 标准与范式，我们追求的不只是"能用"，而是"好用"的标杆

米魔范是一个**终端原生的 AI 编程助手**。它不是 IDE 插件，不是网页工具，而是直接运行在你终端里的伙伴。无论你用 Vim、Emacs 还是 VS Code 的终端，它都能无缝融入。

### 我们坚持什么？

1. **终端优先** —— 开发者 80% 的时间在终端，米魔范就在终端
2. **零配置启动** —— 设置一个 API Key 就能用，不需要写复杂的配置文件
3. **安全可控** —— 每个敏感操作都需要你确认，你的代码库永远在你手中
4. **开源透明** —— MIT 协议，代码完全开放，你可以审计每一行逻辑

### 一句话总结

> **米魔范 = 终端原生 + AI 魔法 + 开发范式**
>
> 它不只是一个工具，而是你编码工作流中那个"懂你"的搭档。

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
export MIMO_API_KEY="你的_MIMO_API_KEY"
mimofan

# 或使用 DeepSeek / OpenAI / 通义千问
export DEEPSEEK_API_KEY="你的_DEEPSEEK_API_KEY"
mimofan
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
| DeepSeek | deepseek-chat | `provider = "deepseek"` |
| 通义千问 | qwen-max | `provider = "openai-compatible"` |
| OpenAI | gpt-4 | `provider = "openai-compatible"` |

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

### DeepSeek

```toml
provider = "deepseek"
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

直接告诉 AI："读取根目录下的 USER_GUIDE.md 并回答我的问题"。

---

## 文档

- [USER_GUIDE.md](USER_GUIDE.md) -- 用户进阶教程
- [ARCHITECTURE.md](ARCHITECTURE.md) -- 系统架构设计
- [docs/MCP.md](docs/MCP.md) -- MCP 扩展服务
- [AGENTS.md](AGENTS.md) -- 贡献指南

---

## 开源许可

本项目遵循 [MIT License](LICENSE) 开源协议。
