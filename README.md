# mimofan

> 终端 AI 编程助手 -- 像 Pair Developer 一样帮你写代码、修 Bug、跑测试
>
> 基于 Rust 实现，原生支持小米 MiMo 模型，兼容 DeepSeek、OpenAI、通义千问等主流大模型。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg)]()
[![MCP Ready](https://img.shields.io/badge/MCP-Supported-green.svg)](docs/MCP.md)

---

## 快速开始

### 安装

```bash
# bun（推荐）
bun add -g mimofan

# 或源码安装
cargo install --path crates/tui --locked
```

### 配置 API Key

```bash
mkdir -p ~/.mimofan
cat << 'EOF' > ~/.mimofan/config.toml
provider = "xiaomi-mimo"
api_key = "替换为你的_MIMO_API_KEY"
base_url = "https://api.xiaomimimo.com/v1"
default_text_model = "mimo-v2.5-pro"
EOF
```

### 启动

```bash
# 检查配置
mimofan doctor

# 启动 TUI
mimofan

# 单次调用
mimofan exec "帮我写一个正则表达式匹配邮箱"
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
