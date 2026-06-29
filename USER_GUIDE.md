# MiMoFan 本地安装与使用指南

MiMoFan 是一个 Rust 终端 AI 编程助手，支持多家 LLM 提供商（DeepSeek、OpenAI、Anthropic、小米 MiMo 等）。

## 环境要求

- **Rust ≥ 1.88**（项目使用 `let_chains` 特性）
- macOS / Linux / Windows
- 至少一个 LLM 提供商的 API Key

## 1. 安装 Rust

```bash
# macOS / Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 确认版本
rustc --version   # 需要 >= 1.88
```

## 2. 克隆并编译

```bash
git clone https://github.com/XiaomingX/mimofan.git
cd mimofan

# 编译（release 模式，推荐）
cargo build --release -p mimofan-cli -p mimofan

# 编译产物位于：
#   target/release/mimofan
```

> 首次编译约需 3-5 分钟，后续增量编译很快。

## 3. 配置

复制示例配置文件到用户目录：

```bash
mkdir -p ~/.mimofan
cp config.example.toml ~/.mimofan/config.toml
```

编辑 `~/.mimofan/config.toml`，至少配置以下内容：

```toml
# 选择提供商（以 DeepSeek 为例）
provider = "deepseek"
api_key = "sk-xxxxxxxxxxxxxxxx"    # 替换为你的真实 Key
base_url = "https://api.deepseek.com/beta"
default_text_model = "deepseek-v4-pro"
```

### 支持的主要提供商

| 提供商 | provider 值 | 环境变量（可替代 config） |
|--------|------------|-------------------------|
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` |
| 小米 MiMo | `xiaomi-mimo` | `XIAOMI_MIMO_API_KEY` |
| OpenAI | `openai` | `OPENAI_API_KEY` |
| Anthropic | `anthropic` | `ANTHROPIC_API_KEY` |
| OpenRouter | `openrouter` | `OPENROUTER_API_KEY` |
| 月之暗面 | `moonshot` | `MOONSHOT_API_KEY` |
| 智谱 Z.AI | `zai` | `Z_AI_API_KEY` |

> 完整列表见 `config.example.toml`。

### 环境变量方式（无需修改配置文件）

```bash
export DEEPSEEK_API_KEY="sk-xxxxxxxx"
export DEEPSEEK_BASE_URL="https://api.deepseek.com/beta"
export DEEPSEEK_MODEL="deepseek-v4-pro"
```

## 4. 运行

```bash
# 直接运行编译产物
./target/release/mimofan

# 或安装到 PATH 后运行
cargo install --path crates/cli
mimofan
```

启动后进入 TUI 交互界面，在底部输入框输入问题即可开始对话。

## 5. 常用操作

| 操作 | 说明 |
|------|------|
| 直接输入文字 | 向 AI 提问 |
| `/mode` | 切换模式（plan / agent / yolo） |
| `/provider <name>` | 切换 LLM 提供商 |
| `Shift+Tab` | 切换推理强度（off / high / max） |
| `Ctrl+C` | 取消当前操作 |
| `Ctrl+D` | 退出 |
| `/help` | 查看所有命令 |

## 6. 开发模式运行（不编译 release）

```bash
# debug 模式直接运行
cargo run -p mimofan-cli --bin mimofan

# 或
cargo run -p mimofan --bin mimofan
```

## 7. 运行测试

```bash
# 全量测试
cargo test --workspace

# 单个 crate 测试
cargo test -p mimofan-config
cargo test -p mimofan-protocol
cargo test -p mimofan-tui --locked

# 格式化检查
cargo fmt --all -- --check

# Clippy 静态分析
cargo clippy --workspace --all-features --locked
```

## 8. 目录结构速览

```
mimofan/
├── crates/
│   ├── cli/          # CLI 入口，二进制名 mimofan
│   ├── tui/          # TUI 界面 + 主逻辑
│   ├── core/         # 核心引擎（turn loop、session）
│   ├── config/       # 配置加载
│   ├── protocol/     # 协议定义
│   ├── agent/        # 子 Agent 系统
│   ├── tools/        # 内置工具
│   └── ...
├── config.example.toml   # 配置模板
└── docs/                 # 详细文档
```

## 常见问题

**Q: 编译报错 "requires rustc 1.88"**
→ 运行 `rustup update` 升级 Rust 工具链。

**Q: 启动后提示 API Key 为空**
→ 确认 `~/.mimofan/config.toml` 中 `api_key` 已填写，或设置了对应环境变量。

**Q: 网络代理**
→ 设置环境变量 `HTTPS_PROXY=http://127.0.0.1:7890`（替换为你的代理地址）。
