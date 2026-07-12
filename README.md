# mimofan

> 跑在终端里的 AI 编码助手，开源、MIT 协议、Rust 实现。
> 内置默认对接 Xiaomi MiMo；其他模型通过 OpenAI 兼容协议接入。

mimofan 给你一句话目标，它就能自己规划、调用工具、改代码、跑测试，直到把活干完。

三种使用形态：
- **TUI 终端界面**（交互式，全功能）
- **CLI 单次调用**（`mimofan "帮我修这个 bug"`）
- **HTTP/JSON-RPC 服务**（嵌入其他系统或 IDE）

---

## 快速安装

```bash
# 推荐：pnpm 安装
pnpm add -g mimofan

# 或 npm
npm install -g mimofan

# 或直接下载二进制
curl -fsSL https://mimofan.net/install.sh | sh
```

源码编译（需要 Rust 1.88+）：

```bash
cargo install mimofan-cli --locked
```

详细安装步骤见 [`docs/INSTALL.md`](docs/INSTALL.md)。

---

## 快速上手

```bash
# 1. 配置 MiMo
mkdir -p ~/.mimofan
cp config.example.toml ~/.mimofan/config.toml
# 编辑 ~/.mimofan/config.toml：
#   provider = "xiaomi-mimo"
#   api_key = "你的 MIMO_API_KEY"

# 2. 验证
mimofan doctor

# 3. 启动 TUI
mimofan

# 或单次调用
mimofan-cli "帮我写一个 FastAPI hello world"
```

完整使用手册见 [`USER_GUIDE.md`](USER_GUIDE.md)。

---

## 项目结构

```
mimofan/
├── crates/
│   ├── cli/          # CLI 入口
│   ├── app-server/   # HTTP/JSON-RPC 服务
│   ├── tui/          # TUI 入口
│   ├── core/         # 核心引擎（Runtime + Turn Loop）
│   ├── agent/        # 模型注册 + 路由解析
│   ├── config/       # 配置 schema + Provider 路由
│   ├── protocol/     # 应用层 JSON 协议 DTO
│   ├── tools/        # 内置工具集
│   ├── mcp/          # MCP server 集成
│   ├── hooks/        # 生命周期钩子
│   ├── execpolicy/   # 执行策略 + 沙箱
│   ├── state/        # SQLite 持久化
│   ├── secrets/      # 密钥管理
│   └── release/      # 版本检查
├── integrations/     # IM 桥接（飞书 / 微信）
├── docs/             # 子文档
└── scripts/          # 构建脚本
```

---

## 文档索引

| 文档 | 内容 |
|------|------|
| [USER_GUIDE.md](USER_GUIDE.md) | 用户使用指南 |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 架构说明（开发者） |
| [ARCHITECTURE_PLAN.md](ARCHITECTURE_PLAN.md) | 架构改进计划（DDD 视角） |
| [docs/INSTALL.md](docs/INSTALL.md) | 安装方式 |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | 配置文件参考 |
| [docs/PROMPTS.md](docs/PROMPTS.md) | 提示词分层 |
| [docs/MODES.md](docs/MODES.md) | TUI 模式 |
| [docs/MCP.md](docs/MCP.md) | MCP 桥接 |
| [docs/SUBAGENTS.md](docs/SUBAGENTS.md) | 子 Agent 用法 |
| [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) | 快捷键 |

---

## 贡献

提交 PR 前请阅读 [`AGENTS.md`](AGENTS.md) 的工作约定。
涉及用户接口改动（CLI 参数、配置文件字段、TUI 快捷键、HTTP API），请先在 issue 里讨论。

## 许可

MIT License。
