# mimofan 使用说明

> 终端 AI 编程助手 —— 说人话的使用指南

---

## 1. 快速开始（30 秒上手）

### 1.1 安装

```bash
# 方式一：cargo 编译安装（推荐，需要 Rust 1.88+）
cargo install --path crates/tui --locked

# 方式二：bun 全局安装
bun add -g mimofan

# 方式三：从源码编译
git clone https://github.com/XiaomingX/mimofan.git
cd mimofan
cargo build --release -p mimofan
# 编译好的二进制在 target/release/mimofan
```

### 1.2 零配置启动

不需要手动创建配置文件。设置环境变量就能直接用：

```bash
# 用小米 MiMo（默认推荐）
export XIAOMI_MIMO_API_KEY="你的密钥"
mimofan

# 首次运行没设环境变量？别慌，会自动弹出配置向导
mimofan
```

### 1.3 最常用的三个命令

```bash
# 启动全屏交互界面
mimofan

# 单次指令（干完就退出，不进 TUI）
mimofan exec "帮我写一个正则表达式匹配邮箱"

# 系统诊断（看看哪里有问题）
mimofan doctor
```

---

## 2. 安装方法详解

### 2.1 前置条件

- **Rust 1.95+**（代码本身用 `let_chains` 特性需要 1.88；下限制约来自依赖 rusqlite 0.40 / libsqlite3-sys 0.38 需 1.95。项目已在 `Cargo.toml.rust-version` 显式声明）
- **macOS / Linux**（Windows 未测试）

检查 Rust 版本：

```bash
rustc --version   # 需要 >= 1.95
```

### 2.2 编译安装

```bash
# 编译 release 版本（优化过，运行快）
cargo build --release -p mimofan

# 编译产物在
ls target/release/mimofan
```

### 2.3 验证安装

```bash
mimofan --version
mimofan doctor    # 自动诊断配置和环境问题
```

---

## 3. 配置说明

### 3.1 配置文件位置

```
~/.mimofan/config.toml        # 主配置文件
~/.mimofan/mcp.json           # MCP 工具服务器配置
~/.mimofan/constitution.json  # 项目级提示词约束（可选）
```

### 3.2 最小配置

只需要填 API Key 就能用：

```toml
provider = "openai-compatible"
api_key = "你的_API_KEY"
base_url = "https://api.deepseek.com/beta"
default_text_model = "deepseek-v4-pro"
```

### 3.3 多服务商配置

项目支持 Profile 机制，一套配置管多个环境：

```bash
# 命令行切换
mimofan --profile mimo
mimofan --profile deepseek

# 环境变量切换
export MIMOFAN_PROFILE=deepseek
mimofan
```

配置示例（`~/.mimofan/config.toml`）：

```toml
# 默认用 MiMo（OpenAI 兼容协议）
provider = "openai-compatible"
api_key = "YOUR_KEY"
default_text_model = "mimo-v2.5-pro"

# DeepSeek profile
[profiles.deepseek]
provider = "openai-compatible"
api_key = "YOUR_DEEPSEEK_KEY"
default_text_model = "deepseek-v4-pro"

# 通义千问 profile
[profiles.qwen]
provider = "openai-compatible"
api_key = "YOUR_DASHSCOPE_KEY"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
default_text_model = "qwen-max"

# Anthropic Messages API
[profiles.anthropic]
provider = "anthropic-compatible"
api_key = "YOUR_ANTHROPIC_KEY"
base_url = "https://api.xiaomimimo.com/anthropic"  # 必须以 /anthropic 结尾
default_text_model = "mimo-v2.5"
```

**注意：** base_url 以 `/anthropic` 结尾时，自动使用 Anthropic Messages API 协议；否则使用 OpenAI Chat Completions 协议。

### 3.4 安全配置

```toml
# 执行策略
allow_shell = true
approval_policy = "on-request"    # on-request（默认）| untrusted | never
sandbox_mode = "workspace-write"  # read-only | workspace-write | danger-full-access

# YOLO 模式（全自动，慎用）
# approval_policy = "never"
```

| 模式 | 行为 | 建议 |
|------|------|------|
| `on-request` | 危险操作弹窗确认 | 默认推荐 |
| `untrusted` | 只有不信任的操作才确认 | 一般不改 |
| `never` | 全自动不确认 | 只在信任的仓库用 |

### 3.5 常用可选配置

```toml
# 推理模式（思考深度）
reasoning_effort = "max"    # off | high | max

# 费用显示货币
cost_currency = "cny"       # usd | cny

# 网页搜索引擎（可选）
[search]
provider = "duckduckgo"     # 默认免费，不需要密钥

# 桌面通知
[notifications]
method = "auto"

# 工作区快照（方便撤销 AI 修改）
[snapshots]
enabled = true
```

---

## 4. TUI 界面操作

### 4.1 基础按键

| 按键 | 功能 |
|------|------|
| Enter | 发送消息 |
| Shift+Enter 或 Alt+Enter | 换行（不发送） |
| Ctrl+C | 中止当前任务 / 退出 |
| Ctrl+L | 清空对话历史 |
| PageUp / PageDown | 滚动查看历史 |
| Shift+Tab | 切换推理模式（off / high / max） |

### 4.2 斜杠命令

在输入框输入 `/` 开头的命令：

| 命令 | 功能 |
|------|------|
| `/plan <目标>` | 让 AI 先列计划，你审核后再执行 |
| `/clear` | 清屏重置 |
| `/help` | 查看帮助 |
| `/restore` | 撤销 AI 的修改（需要开启 snapshots） |
| `/exit` | 退出 |

### 4.3 安全授权

AI 要执行 Shell 命令时会弹出授权窗口：

- `y` —— 允许执行
- `n` —— 拒绝操作

---

## 5. 使用场景

### 场景一：加功能 / 修 Bug

```
> 帮我在 src/main.rs 里加一个检查网络连接的函数，并写对应的单元测试
```

AI 自动完成：读代码 → 写函数 → 写测试 → 跑 `cargo test` → 红了就修。

### 场景二：快速问答

```bash
mimofan exec "用 Python 写一个简单的爬虫脚本，保存到 spider.py"
```

### 场景三：重构代码

```
> 把 src/parser.rs 里的 parse_config 函数重构一下，当前太长了，拆成几个小函数
```

### 场景四：读文档回答问题

```
> 读取根目录下的 ARCHITECTURE_CN.md，告诉我 core 和 tui 的关系
```

### 场景五：Plan 模式（先规划再执行）

```
/plan 给项目加一个用户认证模块，包括登录、注册、JWT token
```

AI 会先列出详细计划，你确认后才开始动手。

---

## 6. MCP 工具集成

MCP（Model Context Protocol）让你可以给 AI 接外部工具。

### 6.1 配置 MCP 服务器

编辑 `~/.mimofan/mcp.json`：

```json
{
  "servers": {
    "my-tool": {
      "command": "node",
      "args": ["my-mcp-server.js"],
      "env": {}
    }
  }
}
```

### 6.2 确保功能开关打开

```toml
# ~/.mimofan/config.toml
[features]
mcp = true
```

---

## 7. 常见问题

**Q: 启动报 "Config not found" 或连接超时？**

检查配置文件路径和 API Key：

```bash
# 确认配置文件存在
cat ~/.mimofan/config.toml

# 运行诊断
mimofan doctor
```

**Q: 每次执行命令都要按 y 确认，太烦了？**

在配置中开启 YOLO 模式（只在信任的仓库用）：

```toml
approval_policy = "never"
```

**Q: 怎么让 AI 读取本地文档？**

直接在对话里说：

```
读取根目录下的 ARCHITECTURE_CN.md，然后回答：core 和 tui 是什么关系？
```

**Q: 支持哪些大模型？**

> 项目不绑定具体产品，只要对方说 OpenAI / Anthropic / Gemini 三种兼容线协议之一即可接入。内置了大量模型别名，下面只列常用的。

| 服务商 | 模型示例 | 配置方式（`provider` 字段） |
|--------|----------|---------|
| 小米 MiMo | mimo-v2.5-pro（模型别名 `mimo`/`pro`）、mimo-v2.5（别名 `omni`） | 默认 `openai-compatible` |
| DeepSeek | deepseek-v4-pro / deepseek-v4-flash | `openai-compatible` |
| 通义千问 | qwen-max | `openai-compatible` + dashscope base_url |
| OpenAI | gpt-4 系列 | `openai-compatible` |
| Anthropic | claude-opus-4 / claude-sonnet-4 系列 | `anthropic-compatible` + 以 `/anthropic` 结尾的 base_url |
| Kimi / GLM / MiniMax / 腾讯混元 等 | 各厂商模型 | `openai-compatible` + 对应 base_url |

> 模式规范名只有三种：`openai-compatible` / `anthropic-compatible` / `gemini-compatible`（kebab-case，无历史别名）。模型名里的 `mimo`/`deepseek` 等是模型标识，不是模式。

> 想看完整模型清单，搜 `crates/agent/src/lib.rs` 里的 `ModelRegistry`。

**Q: 编译报错 "package requires rustc 1.88+"？**

升级 Rust：

```bash
rustup update stable
```

**Q: 推理模式怎么切换？**

- TUI 里按 **Shift+Tab** 在 off / high / max 之间切换
- 或在配置里设置 `reasoning_effort = "max"`

**Q: 怎么查看花了多少钱？**

配置费用显示：

```toml
cost_currency = "cny"   # 显示人民币
# 或
cost_currency = "usd"   # 显示美元
```

---

## 8. 进阶能力速览（已开放）

以下能力随 `main` 合入，升级后即可用（详见 `docs/NEW_CAPABILITIES_GUIDE.md`）：

| 能力 | 命令/入口 | 一句话 |
|------|----------|--------|
| 网络安全检测 | `security_audit` / `attack_surface` / `protocol_check` / `access_control` 等工具 + 内置 `vuln-hunt` skill | 用离线静态分析引擎扫描代码漏洞（semgrep/污点/攻击面/访问控制），模型在对话中直接调用 |
| 长程任务轨迹 | 默认开启（opt-out） | 会话过程自动落盘 `trajectory.jsonl`，供标注/分析/训练 |
| 评测闭环 | `mimofan eval` | 用 Mock 模型驱动真实工具跑评测，含一致性/追踪/复现三维评分 |
| 可机评优化回路 | `/evolve <goal>` | 你给 evaluator 脚本，AI 只出候选，脚本裁决，避免自我评价 |
| 可复现性纪律 | `/repro <brief>` | 固化 `BRIEF.md` + 环境快照 + provenance 留痕 |
| 研究成果物汇总 | `/artifact <id> [--publish]` | 把研究产物汇总为可复现目录，只收录通过评审的 Claim |
| 独立评审者 | `/reviewer [<id>]` | 只读审核，执行者与评审者职责分离 |
| 子智能体协作 | `/subagents`、`/fleet` | 并行拆任务、多 Agent 协作、独立 worktree |
| 共同作者署名 | `git_commit` 默认 `co_authored_by: true` | 提交自动追加 `Co-Authored-By: mimofan`（可关） |

> 提示：安全检测工具族并非在普通对话入口默认全开——它面向「网络安全研究人员/评测」场景接线（完整面经 `with_full_agent_surface` 与 `tool_setup.rs`）。普通编码会话若要调用，可在 `/plan` 或 Agent 模式下让模型以相应工具完成，或参考 `docs/SUBAGENTS.md`。

---

## 9. 文档索引

| 文档 | 内容 |
|------|------|
| [ARCHITECTURE_CN.md](ARCHITECTURE_CN.md) | 中文架构说明（分层图/依赖/提示词/入口/用例） |
| [ARCHITECTURE_IMPROVEMENT_PLAN.md](ARCHITECTURE_IMPROVEMENT_PLAN.md) | 系统架构设计与 DDD 改进计划（含待办清单 [ ]/[x]） |
| [ARCHITECTURE_STABILITY.md](ARCHITECTURE_STABILITY.md) | 稳定性/性能/可扩展性专项报告（内存/死锁/并发风险） |
| [EVOLUTION_CRAWLER.md](EVOLUTION_CRAWLER.md) | 百亿级 URL 分布式爬虫 + 开源情报监测演进路线 |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | 配置详解 |
| [docs/MCP.md](docs/MCP.md) | MCP 工具集成 |
| [docs/SUBAGENTS.md](docs/SUBAGENTS.md) | 子智能体系统 |
| [docs/MODES.md](docs/MODES.md) | 操作模式（Plan / Agent / YOLO） |
| [docs/PROMPTS.md](docs/PROMPTS.md) | 提示词工程 |
| [docs/NEW_CAPABILITIES_GUIDE.md](docs/NEW_CAPABILITIES_GUIDE.md) | 新增能力（/evolve /repro /artifact /reviewer 等） |
| [CLAUDE.md](CLAUDE.md) | 开发者与 AI 协作指南 |

---

> 最后更新：2026-09-03
