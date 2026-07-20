# mimofan 🚀

> **跑在终端里的 AI 编程助手 —— 像 Pair Developer 一样帮你写代码、修 Bug、跑测试**
> 
> 基于 Rust 实现，原生支持 **Xiaomi MiMo** 模型，同时兼容 DeepSeek、OpenAI、通义千问等主流大模型。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg)]()
[![MCP Ready](https://img.shields.io/badge/MCP-Supported-green.svg)](docs/MCP.md)

---

## ⚡ 新手 3 分钟极速上手

### 第一步：安装 `mimofan`

确保本地已安装 Node.js (推荐 18+)，直接在终端运行以下命令：

```bash
# 使用 pnpm 安装（推荐）
pnpm add -g mimofan

# 或使用 npm 安装
npm install -g mimofan
```

*(如果你是 Rust 开发者，也可以直接使用 `cargo install mimofan-cli --locked` 源码安装)*

---

### 第二步：配置 API Key

1. 凭据获取：前往 [Xiaomi MiMo 开放平台](https://api.xiaomimimo.com) 或你的模型服务商后台获取 `API_KEY`。
2. 创建配置文件夹并添加配置：

在终端中执行以下命令（复制粘贴即可）：

```bash
mkdir -p ~/.mimofan
cat << 'EOF' > ~/.mimofan/config.toml
# mimofan 基础配置文件

provider = "xiaomi-mimo"
api_key = "替换为你的_MIMO_API_KEY"
base_url = "https://api.xiaomimimo.com/v1"
default_text_model = "mimo-v2.5-pro"
EOF
```

*(提示：用文本编辑器打开 `~/.mimofan/config.toml`，将 `替换为你的_MIMO_API_KEY` 改为你真实的密钥)*

---

### 第三步：检查环境健康状态

运行诊断命令，确认 API Key 与网络连接是否一切正常：

```bash
mimofan doctor
```

如果看到 `OK` 提示，说明你已成功配置！

---

### 第四步：进入终端界面开始体验

在你的代码项目根目录下直接运行：

```bash
mimofan
```

界面启动后，试着输入：
> `“帮我检查一下当前目录下的代码，给我总结一下它的功能”`

---

## 🎮 新手必看：TUI 终端界面操作指南

启动 `mimofan` 后，你将进入全屏交互式终端界面。

### 1. 基础按键与交互

| 按键操作 | 功能说明 |
|----------|----------|
| `Enter` (回车) | **发送消息** 给 AI |
| `Shift + Enter` 或 `Alt + Enter` | 在输入框中 **换行** |
| `Ctrl + C` | 中止当前正在运行的任务 / 退出程序 |
| `Ctrl + L` | **清空屏** 历史记录 |
| `PageUp / PageDown` | 向上/向下滚动查看历史对话 |

---

### 2. 安全授权确认 (AI 执行命令时的弹窗)

当 AI 需要修改你的文件或在你的电脑上运行终端脚本时，界面会弹出 **授权询问弹窗**：

- **按 `y`**：授权允许 AI 执行当前操作。
- **按 `n`**：拒绝该操作，AI 会停止此步并寻找其他方案。

---

### 3. 常用斜杠指令 (Slash Commands)

在对话框中输入 `/` 可以触发快捷指令：

* `/plan <目标>`：让 AI 先列出详细的设计/重构计划，由你审核通过后再开始改代码。
* `/clear`：清屏并重置当前对话上下文。
* `/help`：查看内置的帮助信息与常用操作快捷键。
* `/exit`：退出 `mimofan` 界面。

---

## 💡 真实开发场景实战

### 场景一：给项目添加新功能 / 修改 Bug
打开终端，进入你的代码目录，启动 `mimofan` 后直接对话：

```text
> 帮我在 src/main.rs 里加一个检查网络连接的函数，并写对应的单元测试
```

`mimofan` 会自动做以下事情：
1. 🔍 **读取代码**：自动找到 `src/main.rs` 并分析上下文。
2. ✍️ **编写代码**：插入符合规范的新函数与测试。
3. 🧪 **运行验证**：自动运行 `cargo test` 确保测试通过！

---

### 场景二：单次命令行快捷调用 (无需进入 TUI)

如果你只是想在 Bash 脚本里使用，或者快速问一个问题，可以使用 `mimofan-cli`：

```bash
# 让 AI 一句话回答问题
mimofan-cli "帮我写一个匹配电子邮件地址的正则表达式"

# 让 AI 在当前目录下生成文件
mimofan-cli "帮我用 Python 写一个简单的爬虫脚本，保存到 spider.py"
```

---

## ⚙️ 切换其他大模型 (DeepSeek / Qwen / OpenAI)

除了默认的小米 MiMo 外，你可以在 `~/.mimofan/config.toml` 中轻松切换其他模型：

### 切换到 DeepSeek

```toml
provider = "deepseek"
api_key = "你的_DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com/v1"
default_text_model = "deepseek-chat"
```

### 切换到通义千问 / 阿里云 / 其他 OpenAI 兼容 Endpoints

```toml
provider = "openai-compatible"
api_key = "你的_DASHSCOPE_API_KEY"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
default_text_model = "qwen-max"
```

---

## ❓ 新手踩坑 FAQ

<details>
<summary><b>Q1: 启动时提示 <code>Config not found</code> 或连接超时？</b></summary>

请检查 `~/.mimofan/config.toml` 文件路径是否正确，以及 `api_key` 是否正确填入。运行 `mimofan doctor` 命令可自动查明网络及配置问题。
</details>

<details>
<summary><b>Q2: 执行命令时一直要按 <code>y</code> 确认，觉得麻烦怎么关掉？</b></summary>

在 `~/.mimofan/config.toml` 中添加 `approval_policy = "yolo"` 即可开启全自动无人值守模式（注意：全自动模式下 AI 会自动运行脚本，请在信任的代码仓库中使用）。
</details>

<details>
<summary><b>Q3: 我能让 AI 读取我本地的 Markdown 文档或项目说明吗？</b></summary>

可以！直接在对话里告诉 AI：“读取根目录下的 USER_GUIDE.md 并回答我的问题”，AI 会自动调用内置的文件读取工具加载文档。
</details>

---

## 📚 进阶文档导航

* 📖 [USER_GUIDE.md](USER_GUIDE.md) — 完整用户进阶教程
* 📐 [ARCHITECTURE.md](ARCHITECTURE.md) — 系统架构设计与原理
* 🔌 [docs/MCP.md](docs/MCP.md) — 连接外部 MCP 扩展服务
* 🤝 [AGENTS.md](AGENTS.md) — 参与项目贡献指南

---

## 📄 开源许可

本项目遵循 [MIT License](LICENSE) 开源协议。
