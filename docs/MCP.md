# MCP（外部工具服务器）

mimofan 通过 MCP（Model Context Protocol）加载外部工具。支持本地 stdio 进程和远程 HTTP 服务器。

## 初始化

```bash
mimofan mcp init    # 创建 MCP 配置文件
mimofan mcp list    # 查看已配置的服务器
mimofan mcp tools   # 查看可用工具
```

## 配置文件

默认路径：`~/.mimofan/mcp.json`

### stdio 服务器

```json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "your-token" }
    }
  }
}
```

### HTTP 服务器

```json
{
  "mcpServers": {
    "remote-tools": {
      "url": "https://your-mcp-server.example/sse"
    }
  }
}
```

## TUI 命令

- `/mcp` — 查看 MCP 状态
- `/mcp init` — 初始化配置

MCP 工具名称格式：`mcp__<server>__<tool>`

## 环境变量

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_MCP_CONFIG` | MCP 配置文件路径 |

## 审批控制

MCP 工具受审批策略控制。可通过钩子强制审批：

```toml
[[hooks.hooks]]
event = "tool_call_before"
command = '''echo '{"decision":"ask"}' '''
condition = { type = "tool_name", name = "mcp__*" }
```
