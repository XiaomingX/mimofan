# 米谋 (mimofan)

> 终端 AI 编程助手 —— 与你并肩，谋定而后动
>
> 基于 Rust 实现，原生支持小米 MiMo 模型，兼容 DeepSeek、OpenAI、通义千问等主流大模型。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-macOS-lightgrey.svg)]()
[![MCP Ready](https://img.shields.io/badge/MCP-Supported-green.svg)](docs/MCP.md)

---

## 📖 产品故事

### 缘起

每个程序员都有过这样的深夜：

屏幕上是跑了三遍还是红的测试，脑子里是理不清的逻辑分支，手边是第几杯已经凉透的咖啡。你盯着代码，代码也盯着你，彼此对峙，谁也不肯先开口。

这时候你会想——**如果有个人能帮我看看就好了。**

不是那种冷冰冰的代码补全，不是那种只会复制粘贴的 Stack Overflow 搜索器。而是一个真正能理解你在做什么、想做什么、为什么这么做的搭档。

**米谋，就是为这样的时刻而生。**

### 名字的由来

"米谋"二字，藏着我们的初心：

- **米** —— 源自小米 MiMo。我们相信国产大模型的能力，也相信好的工具应该站在好的基座上。每一粒米，都是粮食；每一个模型，都有它的价值。
- **谋** —— 谋定而后动。写代码不是敲键盘的速度竞赛，而是思考的艺术。米谋不替你思考，而是帮你想清楚再动手。先谋后断，方能行稳致远。

合在一起，**米谋**——以智慧为谋，以代码为米，一粒一粒，种出属于你的软件稻田。

### 我们相信什么？

**1. 终端是程序员的第二个家**

IDE 很好，但终端才是开发者真正栖息的地方。Git 在终端里，Docker 在终端里，部署在终端里，排查问题在终端里。米谋选择终端作为自己的家，是因为这里离你最近。

**2. 好的工具应该像空气**

你不会注意到空气的存在，直到它变得浑浊。好的编程助手也是如此——它不应该打断你的思路，不应该强迫你切换窗口，不应该让你学一堆新的配置语法。它应该在你需要时出现，在你不需要时安静地待在后台。

**3. 代码是写给人看的**

米谋生成的每一行代码，都以"人能读懂"为第一标准。它会帮你写注释，会遵循项目的代码规范，会在修改前先理解上下文。因为它知道，代码的读者首先是你的队友，然后才是编译器。

**4. 信任需要时间建立**

所以米谋不会擅自执行任何危险操作。每一次文件修改、每一次命令执行，它都会停下来问你："可以吗？" 这不是啰嗦，这是尊重。你的代码库是你的心血，米谋深知这一点。

### 它会做什么？

- 📖 **读懂你的项目** —— 自动分析代码结构，理解模块关系
- ✍️ **写出规范的代码** —— 遵循你的编码风格，生成可维护的代码
- 🧪 **帮你验证** —— 写完代码自动跑测试，红了就修，绿了才停
- 🔧 **修 Bug 也是一把好手** —— 给它一个报错信息，它能帮你定位到具体的代码行
- 🤫 **安静地工作** —— 不弹窗，不打扰，只在需要你决策时才开口

### 一句话

> **米谋不是一个替你写代码的工具，而是一个帮你把代码写好的搭档。**
>
> 谋定而后动，知止而有得。

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
