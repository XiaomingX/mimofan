# mimofan 使用说明

> 面向中国开发者的使用指南，说人话，不绕弯子。
> 最后更新：2026-06-29

---

## mimofan 能干什么？

简单说：**你用自然语言告诉它要做什么，它帮你写代码、改文件、跑命令。**

比如：
- "帮我写一个 FastAPI 的 hello world"
- "把 src/utils.py 里的 print 全改成 logging"
- "跑一下测试，看看有没有报错"
- "帮我提交代码并创建 PR"

它会调用大模型（MiMo、DeepSeek、GPT、Claude 等）思考，然后用内置工具（shell、文件读写、Git 等）把活干了。

---

## 核心概念

### 1. 会话（Session）

一次对话就是一个会话。会话包含：
- 用户输入
- AI 回复
- 工具调用记录
- 上下文历史

### 2. 线程（Thread）

线程是会话中的一个对话流。一个会话可以有多个线程（分支对话）。

### 3. 工具（Tool）

工具是 AI 可以调用的能力。内置工具包括：
- `shell` — 执行 shell 命令
- `read_file` — 读取文件
- `write_file` — 写入文件
- `search` — 搜索文件内容
- `web_search` — 网络搜索

### 4. 模式（Mode）

mimofan 有三种工作模式：

| 模式 | 说明 | 审批策略 |
|------|------|---------|
| `agent` | 标准模式，工具调用需要确认 | 需要用户确认 |
| `plan` | 规划模式，只分析不执行 | 不执行工具 |
| `yolo` | 自动模式，跳过所有审批 | 自动执行 |

---

## 安装

### 方式 1：npm 全局安装（推荐）

```bash
npm install -g mimofan
```

或者使用 pnpm：

```bash
pnpm add -g mimofan
```

安装后你会得到两个命令：
- `mimofan` — 完整功能的 TUI 终端界面
- `codew` — 轻量 CLI 工具

### 方式 2：从源码编译

```bash
# 需要 Rust 1.88+
git clone https://github.com/XiaomingX/mimofan.git
cd mimofan
cargo build --release -p mimofan-cli -p mimofan
```

编译产物在 `target/release/` 下。

### 方式 3：Docker

```bash
docker build -t mimofan .
docker run -it mimofan
```

---

## 首次配置

### 1. 设置 API Key

最简单的方式是创建 `.env` 文件或直接设置环境变量：

```bash
# DeepSeek（默认 provider）
export DEEPSEEK_API_KEY="sk-你的key"

# 或者用 OpenAI 兼容接口
export OPENAI_API_KEY="sk-你的key"

# 或者用 Anthropic
export ANTHROPIC_API_KEY="sk-ant-你的key"
```

### 2. 初始化配置文件

```bash
# 复制示例配置
cp config.example.toml ~/.mimofan/config.toml

# 编辑配置（至少填好 api_key）
vim ~/.mimofan/config.toml
```

**最小配置**（`~/.mimofan/config.toml`）：

```toml
provider = "deepseek"
api_key = "sk-你的deepseek-key"
default_text_model = "deepseek-v4-pro"
```

### 3. 支持的模型提供商

| Provider | 配置值 | 说明 |
|----------|--------|------|
| DeepSeek | `deepseek` | 默认，国内直连 |
| DeepSeek (中国) | `deepseek-cn` | 同上，别名 |
| OpenAI | `openai` | GPT 系列 |
| Anthropic | `anthropic` | Claude 系列 |
| OpenRouter | `openrouter` | 聚合多家模型 |
| 小米 MiMo | `xiaomi-mimo` | 国产模型 |
| 硅基流动 | `siliconflow` | 国内平台 |
| 火山引擎 | `volcengine` | 字节跳动 |
| 月之暗面 | `moonshot` | Kimi 系列 |
| 智谱 | `zai` | GLM 系列 |
| NVIDIA NIM | `nvidia-nim` | NVIDIA 托管 |
| 阿里 Qwen | `qwen` | 通义千问 |
| 阶跃星辰 | `stepfun` | Step 系列 |
| MiniMax | `minimax` | 海螺 AI |
| DeepInfra | `deepinfra` | 开源模型托管 |

### 小米 MiMo 配置示例

```bash
# 环境变量方式
export XIAOMI_MIMO_API_KEY="your-api-key"
export XIAOMI_MIMO_BASE_URL="https://token-plan-cn.xiaomimimo.com/v1"  # CN 区域

# 或在 config.toml 中
[providers.xiaomi_mimo]
api_key = "your-api-key"
base_url = "https://token-plan-cn.xiaomimimo.com/v1"
```

MiMo API 端点：

| 区域 | 地址 |
|------|------|
| 新加坡（默认） | `https://token-plan-sgp.xiaomimimo.com/v1` |
| 中国大陆 | `https://token-plan-cn.xiaomimimo.com/v1` |
| 阿姆斯特丹 | `https://token-plan-ams.xiaomimimo.com/v1` |
| 按量付费 | `https://api.xiaomimimo.com/v1` |

---

## 基本使用

### 启动 TUI 终端界面

```bash
mimofan
```

进入后你会看到一个终端界面，直接打字跟 AI 对话就行。

### 常用 TUI 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Enter` | 发送消息 |
| `Ctrl+L` | 压缩上下文（对话太长时用） |
| `Ctrl+R` | 恢复上次会话 |
| `Shift+Tab` | 切换思考模式（off/high/max） |
| `Ctrl+C` | 取消当前操作 |
| `/help` | 查看帮助 |

### 常用斜杠命令

| 命令 | 功能 |
|------|------|
| `/model <模型名>` | 切换模型 |
| `/provider <provider>` | 切换提供商 |
| `/compact` | 压缩上下文 |
| `/cost` | 查看本次会话费用 |
| `/resume` | 恢复上次会话 |
| `/mode agent` | 切换到 Agent 模式 |
| `/mode plan` | 切换到 Plan 模式 |
| `/mode yolo` | 切换到 YOLO 模式（跳过审批） |
| `/clear` | 清空当前对话 |
| `/queue <消息>` | 排队一条消息（离线时用） |

### 使用 CLI 命令行

```bash
# 一次性提问
codew "帮我写个 Python 快排"

# 指定模型
codew --model deepseek-v4-flash "解释这段代码"

# 指定 provider
codew --provider openai "用 GPT 帮我写个 API"

# 恢复上次会话
codew --resume

# 启动 HTTP 服务器模式
mimofan serve --http
```

---

## 三种运行模式

mimofan 有三种模式，控制 AI 的行为方式：

### Agent 模式（默认）

AI 会主动使用工具完成任务，但危险操作需要你确认。

```
你：帮我写个 hello world
AI：好的，我来创建 main.rs
    [调用 write_file 工具]
    已完成！文件写入 src/main.rs
```

### Plan 模式

AI 只做规划，不执行。适合复杂任务的前期设计。

```
你：帮我重构这个项目
AI：我建议分三步：
    1. 先提取公共模块
    2. 然后拆分大文件
    3. 最后更新测试
    要我开始执行吗？
```

### YOLO 模式

AI 自己决定一切，不问你。适合你信任 AI 的场景。

```bash
# 启动时指定
mimofan --yolo

# 或在 TUI 中切换
/mode yolo
```

---

## 安全模式说明

### 审批策略（approval_policy）

| 策略 | 说明 |
|------|------|
| `on-request` | 默认。危险操作需要你确认 |
| `never` | 不需要确认，AI 自己决定（YOLO 模式） |
| `untrusted` | 严格模式，几乎所有操作都要确认 |

### 沙箱模式（sandbox_mode）

| 模式 | 说明 |
|------|------|
| `workspace-write` | 默认。只能在当前项目目录写文件 |
| `read-only` | 只能读，不能写 |
| `danger-full-access` | 完全放开（危险！） |

### 自动放行命令

有些命令你确定安全，可以配置自动放行：

```toml
# ~/.mimofan/config.toml
auto_allow = ["git status", "cargo check", "npm test"]
```

---

## 子代理系统

mimofan 可以派生子代理来并行处理复杂任务。

### 什么时候会派生子代理？

- 任务可以拆分成独立的子任务
- 需要并行处理多个文件
- 任务需要不同的专业知识

### 子代理的行为

```
你：帮我给这个项目写单元测试
AI：我会派生 3 个子代理并行处理：
    - 子代理 1：处理 src/models/ 的测试
    - 子代理 2：处理 src/services/ 的测试
    - 子代理 3：处理 src/utils/ 的测试
    [3 个子代理并行工作]
    全部完成！共生成 15 个测试文件。
```

### 子代理配置

```toml
# ~/.mimofan/config.toml
[subagent]
max_concurrent = 20        # 最大并发数
max_depth = 3              # 最大嵌套深度
```

---

## 配置 MCP 工具服务器

MCP（Model Context Protocol）让 mimofan 能调用外部工具。

### 配置方法

创建 `~/.mimofan/mcp.json`：

```json
{
  "servers": {
    "my-tool": {
      "command": "node",
      "args": ["path/to/my-mcp-server.js"]
    }
  }
}
```

配置后重启 mimofan，MCP 工具自动对 AI 可见。

### 常用 MCP 服务器

| 服务器 | 用途 |
|--------|------|
| `@anthropic/mcp-server-filesystem` | 文件系统操作 |
| `@anthropic/mcp-server-github` | GitHub API |
| `@anthropic/mcp-server-slack` | Slack 消息 |

---

## Skills 技能系统

Skills 是可复用的提示词模板，让 AI 专注于特定任务。

### 使用内置 Skills

在对话中输入 `/load_skill <skill名>` 或直接让 AI 加载：

```
请加载 pdf 技能，帮我分析这个 PDF 文件
```

### 创建自定义 Skills

在 `~/.mimofan/skills/` 下创建目录：

```
~/.mimofan/skills/
  my-skill/
    SKILL.md          # 技能提示词（必须）
    helper-script.sh  # 辅助脚本（可选）
```

`SKILL.md` 示例：

```markdown
# 我的代码审查技能

你是一个代码审查专家。当用户请求代码审查时：
1. 检查代码风格
2. 查找潜在 bug
3. 评估性能问题
4. 给出改进建议
```

---

## Hooks 钩子系统

Hooks 让你在工具执行前后自动运行自定义脚本。

### 配置方法

在 `~/.mimofan/config.toml` 中添加：

```toml
[[hooks]]
event = "tool_call_before"
command = "echo '即将执行工具: $TOOL_NAME'"

[[hooks]]
event = "tool_call_after"
command = "echo '工具执行完成: $TOOL_NAME, 结果: $TOOL_RESULT'"
```

### 可用事件

| 事件 | 触发时机 |
|------|----------|
| `tool_call_before` | 工具执行前 |
| `tool_call_after` | 工具执行后 |

---

## 后台任务

mimofan 可以在后台运行长时间任务。

### 使用方法

```
# 在 TUI 中创建后台任务
/task add 跑完整的测试套件

# 查看任务列表
/task list

# 查看任务详情
/task read <任务ID>
```

### 通过 HTTP API 管理任务

```bash
# 启动 HTTP 服务器
mimofan serve --http

# 创建任务
curl -X POST http://localhost:3000/v1/tasks \
  -H "Content-Type: application/json" \
  -d '{"prompt": "跑测试"}'

# 查看任务
curl http://localhost:3000/v1/tasks
```

---

## 会话管理

### 恢复会话

```bash
# CLI 方式
codew --resume

# TUI 中
# 按 Ctrl+R 或输入 /resume
```

### 会话存储位置

- `~/.mimofan/sessions/` — 会话历史
- `~/.mimofan/sessions/checkpoints/` — 崩溃恢复点
- `~/.mimofan/snapshots/` — 工作区快照

### 上下文压缩

对话太长时，上下文会占满窗口。用 `/compact` 压缩：

- 自动总结之前的对话
- 保留关键信息
- 释放上下文空间

---

## 常见问题

### Q: API Key 在哪设置？

三种方式（优先级从高到低）：
1. 环境变量：`export DEEPSEEK_API_KEY="sk-xxx"`
2. `.env` 文件：在项目根目录创建 `.env`
3. 配置文件：`~/.mimofan/config.toml` 中的 `api_key` 字段

### Q: 怎么切换模型？

TUI 中输入 `/model deepseek-v4-flash` 或 `/model mimo-v2.5-pro`。

### Q: 怎么用国产模型？

配置 provider 为 `xiaomi-mimo`、`siliconflow`、`volcengine` 等，填好对应的 API Key。

### Q: 对话太长报错怎么办？

输入 `/compact` 压缩上下文，或者开一个新会话。

### Q: 怎么让 AI 不问我直接干活？

启动时加 `--yolo` 参数，或在 TUI 中输入 `/mode yolo`。

### Q: 文件被改坏了怎么恢复？

mimofan 会自动创建快照。用 `/restore` 命令恢复到操作前的状态。

### Q: 怎么在团队中共享配置？

在项目根目录创建 `.mimofan/config.toml`，团队成员自动继承。

### Q: 怎么查看 AI 用了多少钱？

TUI 中输入 `/cost` 查看本次会话的 token 用量和费用。

### Q: 支持中文对话吗？

完全支持。mimofan 会自动跟随你的语言，你用中文它就回中文。

---

## 环境变量速查

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_API_KEY` | DeepSeek API Key |
| `DEEPSEEK_BASE_URL` | DeepSeek API 地址 |
| `DEEPSEEK_MODEL` | 默认模型 |
| `DEEPSEEK_PROVIDER` | 默认 provider |
| `OPENAI_API_KEY` | OpenAI API Key |
| `ANTHROPIC_API_KEY` | Anthropic API Key |
| `DEEPSEEK_APPROVAL_POLICY` | 审批策略 |
| `DEEPSEEK_SANDBOX_MODE` | 沙箱模式 |
| `DEEPSEEK_ALLOW_SHELL` | 是否允许 shell 命令 |
| `DEEPSEEK_YOLO` | 是否启用 YOLO 模式 |
| `DEEPSEEK_LOG_LEVEL` | 日志级别 |
| `RUST_LOG` | Rust 日志过滤 |

---

## HTTP API 接口

启动服务器：`mimofan serve --http`

### 主要端点

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/v1/threads` | 创建新对话 |
| POST | `/v1/threads/{id}/messages` | 发送消息 |
| GET | `/v1/threads` | 列出所有对话 |
| GET | `/v1/threads/{id}` | 获取对话详情 |
| POST | `/v1/tasks` | 创建后台任务 |
| GET | `/v1/tasks` | 列出任务 |
| GET | `/v1/models` | 列出可用模型 |
| GET | `/v1/capabilities` | 查询服务器能力 |

### 使用示例

```bash
# 创建对话并发送消息
curl -X POST http://localhost:3000/v1/threads \
  -H "Content-Type: application/json" \
  -d '{"metadata": {}}'

# 发送消息（流式 SSE 返回）
curl -X POST http://localhost:3000/v1/threads/<thread_id>/messages \
  -H "Content-Type: application/json" \
  -d '{"input": "帮我写个 hello world"}'
```

---

## 实战用例

### 用例 1：快速创建项目

```
你：帮我用 FastAPI 创建一个 REST API 项目，包含用户管理 CRUD

AI 会：
1. 创建项目目录结构
2. 写 main.py、models.py、routes.py
3. 创建 requirements.txt
4. 写 Dockerfile
5. 跑一下确认能启动
```

### 用例 2：批量重构

```
你：把项目里所有的 print 语句改成 logging

AI 会：
1. 搜索所有包含 print 的文件
2. 逐个文件修改
3. 确保 import logging 已添加
4. 跑测试确认没有破坏
```

### 用例 3：代码审查

```
你：帮我审查最近一次 commit 的代码

AI 会：
1. 查看 git log 和 diff
2. 分析代码风格、潜在 bug、性能问题
3. 给出改进建议
```

### 用例 4：调试问题

```
你：跑测试时报错 IndexError: list index out of range，帮我看看

AI 会：
1. 读取报错的测试文件
2. 分析可能的原因
3. 定位问题代码
4. 给出修复方案
```

### 用例 5：写文档

```
你：帮我给这个模块写 README 和 API 文档

AI 会：
1. 分析模块代码
2. 生成 README.md
3. 生成 API 文档
4. 包含使用示例
```

---

## 常用函数和 API（开发者向）

### 核心入口函数

```rust
// 1. 创建运行时（最顶层入口）
use mimofan_core::Runtime;

let runtime = Runtime::new(
    config,           // 配置
    model_registry,   // 模型注册表
    state,            // 状态存储
    tool_registry,    // 工具注册表
    mcp_manager,      // MCP 管理器
    exec_policy,      // 执行策略
    hooks,            // 钩子系统
)?;

// 2. 发送消息
let response = runtime.send_message(MessageRequest {
    model: "mimo-v2.5-pro".to_string(),
    messages: vec![Message::user("帮我写个函数")],
    max_tokens: 4096,
    ..Default::default()
}).await?;

// 3. 流式接收
let mut stream = runtime.send_message_stream(request).await?;
while let Some(event) = stream.next().await {
    match event? {
        EventFrame::ResponseDelta { delta, .. } => print!("{}", delta),
        EventFrame::ToolCall { name, .. } => println!("[工具] {}", name),
        _ => {}
    }
}
```

### 工具注册

```rust
use mimofan_tools::{ToolHandler, ToolSpec, ToolRegistry};

// 实现工具
struct MyTool;
#[async_trait]
impl ToolHandler for MyTool {
    fn kind(&self) -> ToolKind { ToolKind::Function }
    async fn handle(&self, inv: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        Ok(ToolOutput::Function { body: Some(json!({"ok": true})), success: true })
    }
}

// 注册
let mut registry = ToolRegistry::default();
registry.register(spec, Arc::new(MyTool))?;
```

### 配置解析

```rust
use mimofan_config::{ConfigStore, RouteResolver};

let store = ConfigStore::load(Some("config.toml"))?;
let resolver = RouteResolver::new(store.config());
let candidate = resolver.resolve(&request)?;
// candidate.model, candidate.base_url, candidate.context_window
```

### 执行策略检查

```rust
use mimofan_execpolicy::{ExecPolicyEngine, ExecPolicyContext};

let engine = ExecPolicyEngine::new();
let decision = engine.check(&ExecPolicyContext {
    command: "rm -rf /tmp/test",
    cwd: "/workspace",
    tool: "shell",
    ask_for_approval: AskForApproval::OnRequest,
    sandbox_mode: SandboxMode::Seatbelt,
})?;
// decision.requires_approval == true
```

### 状态持久化

```rust
use mimofan_state::StateStore;

let store = StateStore::open("~/.mimofan/state.db")?;
store.upsert_thread(&ThreadMetadata { id: "t1".into(), .. })?;
store.append_message(&MessageRecord { thread_id: "t1".into(), role: "user".into(), .. })?;
let messages = store.list_messages("t1")?;
```

---

## 配置文件速查

| 文件 | 位置 | 用途 |
|------|------|------|
| 主配置 | `~/.mimofan/config.toml` | Provider、模型、策略 |
| 运行时设置 | `~/.mimofan/settings.toml` | 模式、UI 偏好 |
| MCP 配置 | `~/.mimofan/mcp.json` | MCP 服务器 |
| 技能目录 | `~/.mimofan/skills/` | 自定义技能 |
| 会话历史 | `~/.mimofan/sessions/` | 对话记录 |
| 审计日志 | `~/.mimofan/audit.log` | 操作审计 |

---

## 构建与测试

```bash
# 格式化
cargo fmt

# 编译
cargo build

# 测试
cargo test -p mimofan-config        # 配置测试
cargo test -p mimofan-protocol      # 协议测试
cargo test -p mimofan --locked      # TUI 测试
cargo test --workspace              # 全量测试

# 发布构建
cargo build --release -p mimofan-cli -p mimofan
```
