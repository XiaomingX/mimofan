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
| `MIMOFAN_MCP_CONFIG` | MCP 配置文件路径 |

## 审批控制

MCP 工具受审批策略控制。可通过钩子强制审批：

```toml
[[hooks.hooks]]
event = "tool_call_before"
command = '''echo '{"decision":"ask"}' '''
condition = { type = "tool_name", name = "mcp__*" }
```

## 作为 Agent 工具后端（无需 LLM SK）

mimofan 本身可以作为**本地工具后端**被任意 MCP 客户端（Claude Code、其他 agent）
驱动：客户端有模型/SDK，mimofan 只跑本地工具（SAST/DAST），不调用任何 LLM、
不消耗任何 SK。

启动 stdio MCP 服务器：

```bash
mimofan serve --mcp
```

JSON-RPC 握手后 `tools/list` 默认即包含 7 个安全工具（无需配置）：

| 工具 | 类型 | 说明 |
|------|------|------|
| `security_audit` | SAST | 单文件 semgrep 风格规则扫描 |
| `gadget_chain_trace` | SAST | gadget 链调用追踪 |
| `auto_gadget_discovery` | SAST | 范式级 Java gadget 链自动发现 |
| `attack_surface` | SAST | 攻击面枚举（source/sink/入口点） |
| `protocol_check` | SAST | 协议/类型状态安全检查 |
| `access_control` | SAST | 入口点授权检查（含 Spring/JAX-RS/Shiro 注解） |
| `run_poc` | DAST | 本地沙箱执行 PoC 命令，按 `expect` 标记判定 `realized` |

`run_poc` 无需容器配置：MCP 服务器把本地 OS 沙箱（Linux Landlock /
macOS seatbelt）注册为 `SandboxBackend`，命令在本地执行。

调用示例（JSON-RPC stdio）：

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
  "name":"run_poc",
  "arguments":{"command":"echo pwned","expect":"pwned"}}}
```

外部 MCP 工具调用会写入轨迹 `~/.mimofan/tasks/mcp-server/session.jsonl`
（`tool_call`/`tool_result`/`error` 事件，`source:"mcp"`），可用以下命令查看
原始数据（后训练/评测可直接解析 JSONL）：

```bash
mimofan export-session mcp-server --raw    # 原始工具输入/输出
mimofan export-session mcp-server         # 默认重脱敏
```

门控脚本：`scripts/check_mcp_tools.sh`（7/7 工具）、
`scripts/check_dast_backend.sh`（run_poc realized=true）。
