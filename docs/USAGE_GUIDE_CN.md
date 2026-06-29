# mimofan 使用指南（中文版）

> 本指南帮助你快速上手 mimofan，包括安装、配置、基本使用和高级功能。
> 最后更新：2026-06-29

---

## 快速开始

### 1. 安装

```bash
# 从源码构建
git clone https://github.com/your-org/mimofan.git
cd mimofan
cargo build --release -p mimofan-cli

# 二进制文件在 target/release/mimofan
```

### 2. 配置

创建配置文件 `~/.mimofan/config.toml`：

```toml
[provider.xiaomi_mimo]
api_key = "your-api-key"
base_url = "https://api.xiaomi.com"
```

或者使用环境变量：

```bash
export XIAOMI_MIMO_API_KEY="your-api-key"
```

### 3. 启动

```bash
# TUI 模式（默认）
mimofan

# 命令行模式
mimofan run "帮我写个 hello world"

# HTTP 服务器模式
mimofan serve --port 3000
```

---

## 基本使用

### 与 AI 对话

启动 TUI 后，直接输入你的需求：

```
你：帮我写一个计算斐波那契数列的 Python 函数
AI：好的，我来帮你写...

[AI 创建文件 fibonacci.py]

你：测试一下这个函数
AI：让我运行测试...

[AI 执行 python fibonacci.py]
```

### 使用工具

mimofan 会自动使用工具来完成任务：

- **文件操作**：创建、读取、修改文件
- **Shell 命令**：运行终端命令
- **代码搜索**：查找代码、搜索内容
- **子代理**：并行处理多个任务

### 审批机制

默认情况下，mimofan 在执行危险操作前会请求你的确认：

```
AI：我需要删除临时文件，是否继续？
你：y (确认) / n (拒绝)
```

你可以在配置中调整审批策略：

```toml
[execution]
default_mode = "auto"  # 自动批准安全操作
```

---

## 配置详解

### 主配置文件 (`~/.mimofan/config.toml`)

```toml
# Provider 配置
[provider.xiaomi_mimo]
api_key = "your-key"
base_url = "https://api.xiaomi.com"

# 模型配置
[model]
default = "mimo-v2.5-pro"

# 执行策略
[execution]
default_mode = "auto"
timeout = 300

# MCP 服务器
[[mcp]]
name = "github"
command = "mcp-server-github"
args = ["--verbose"]
```

### JSON 配置 (`~/.mimofan/settings.json`)

```json
{
  "env": {
    "MIMOFAN_API_KEY": "your-key",
    "MIMOFAN_BASE_URL": "https://api.xiaomi.com"
  },
  "mcpServers": {
    "github": {
      "command": "mcp-server-github",
      "args": ["--verbose"],
      "enabled": true
    }
  },
  "language": "Chinese"
}
```

### 环境变量

| 变量名 | 说明 |
|--------|------|
| `XIAOMI_MIMO_API_KEY` | 小米 MiMo 标准 API 密钥 |
| `XIAOMI_MIMO_TOKEN_PLAN_API_KEY` | 小米 MiMo 计费计划 API 密钥 |
| `DEEPSEEK_API_KEY` | DeepSeek API 密钥（兼容旧版） |
| `MIMOFAN_BASE_URL` | 自定义 API 端点 |
| `MIMOFAN_LOG` | 日志级别（info/debug/warn） |

---

## 高级功能

### 子代理

mimofan 可以派遣子代理并行处理任务：

```
你：分析这个项目的代码质量，并检查安全性问题
AI：我将派遣两个子代理：
  - 子代理 A：分析代码质量
  - 子代理 B：检查安全问题

[两个子代理并行工作，结果汇总后返回]
```

### Skills 技能

创建可复用的技能：

1. 创建目录 `~/.mimofan/skills/my-skill/`
2. 添加 `SKILL.md`：

```markdown
# My Skill

这是一个自定义技能，用于...

## 使用方法
1. 步骤一
2. 步骤二
```

3. 通过 `/load-skill` 命令调用

### Hooks 钩子

在工具执行前后运行自定义脚本：

```toml
[[hooks]]
event = "tool_call_before"
command = "echo 'About to run: $TOOL_NAME'"

[[hooks]]
event = "tool_call_after"
command = "python audit.py $TOOL_NAME"
```

### 工作流

使用 Starlark 脚本定义复杂工作流：

```python
# workflow.star
def main():
    # 运行测试
    result = shell("cargo test")
    if result.exit_code != 0:
        fail("Tests failed")
    
    # 构建
    shell("cargo build --release")
    
    return "Build successful"
```

---

## 运行模式

### Agent 模式（默认）

标准模式，AI 自主决策使用哪些工具。

```bash
mimofan  # 默认进入 Agent 模式
```

### Plan 模式

AI 先制定计划，确认后再执行。

```bash
mimofan --mode plan
```

### YOLO 模式

跳过所有审批确认，适合信任的环境。

```bash
mimofan --mode yolo
```

---

## HTTP API

启动 HTTP 服务器：

```bash
mimofan serve --port 3000
```

### API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | 聊天补全（兼容 OpenAI 格式） |
| `/threads` | POST | 创建新线程 |
| `/threads/:id` | GET | 获取线程详情 |
| `/threads/:id/messages` | GET | 获取消息历史 |
| `/health` | GET | 健康检查 |

### 示例请求

```bash
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mimo-v2.5-pro",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

---

## MCP 服务器集成

### 配置 MCP 服务器

在 `settings.json` 中添加：

```json
{
  "mcpServers": {
    "github": {
      "command": "mcp-server-github",
      "args": ["--verbose"],
      "env": {
        "GITHUB_TOKEN": "your-token"
      }
    }
  }
}
```

### 使用 MCP 工具

配置后，MCP 工具自动对 AI 可见：

```
你：查看我的 GitHub 仓库列表
AI：我将使用 GitHub MCP 工具来获取...

[AI 调用 MCP 工具获取仓库列表]
```

---

## 故障排查

### 常见问题

**Q: 启动报错 "No API key found"**
A: 检查配置文件或环境变量是否正确设置。

**Q: 工具执行超时**
A: 在配置中增加超时时间：
```toml
[execution]
timeout = 600
```

**Q: MCP 服务器连接失败**
A: 检查服务器命令是否正确：
```bash
# 测试 MCP 服务器
mcp-server-github --verbose
```

### 日志查看

```bash
# 启用调试日志
MIMOFAN_LOG=debug mimofan

# 查看审计日志
cat ~/.mimofan/audit.log
```

### 重置配置

```bash
# 备份并重置
mv ~/.mimofan ~/.mimofan.backup
mimofan  # 会创建新的默认配置
```

---

## 最佳实践

### 1. 明确你的需求

```
❌ "帮我改代码"
✅ "在 src/main.rs 中添加一个命令行参数 --verbose，用于启用详细日志"
```

### 2. 使用 Plan 模式处理复杂任务

```
mimofan --mode plan
你：重构这个项目，将所有数据库操作移到单独的模块
AI：我制定了以下计划：
  1. 创建 src/db.rs
  2. 迁移数据库相关代码
  3. 更新导入
  是否确认执行？
```

### 3. 利用子代理并行处理

```
你：同时检查代码质量和安全性
[两个子代理并行工作，效率翻倍]
```

### 4. 定期清理会话

```bash
# 查看会话列表
ls ~/.mimofan/sessions/

# 删除旧会话
rm ~/.mimofan/sessions/old-session.json
```

---

## 参考资料

- [架构说明](./ARCHITECTURE_CN_V2.md)
- [配置指南](./CONFIGURATION.md)
- [Provider 配置](./PROVIDERS.md)
- [子代理系统](./SUBAGENTS.md)
- [MCP 集成](./MCP.md)
- [运行模式](./MODES.md)
