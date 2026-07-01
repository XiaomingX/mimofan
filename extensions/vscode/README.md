# Mimofan VS Code 扩展

Mimofan 官方 VS Code 扩展脚手架，用于本地开发。

当前首个版本刻意保持精简，已实现功能：

- 在集成终端中打开 Mimofan
- 在可见终端中启动 `mimofan serve --http`
- 通过 `/health` 和 `/v1/runtime/info` 检查本地运行时状态
- 在状态栏显示连接状态
- 显示只读 Agent 视图，展示来自 `/v1/threads/summary` 的近期运行时线程摘要
- 显示来自 `/v1/snapshots` 的近期只读恢复点
- 自动刷新只读 Agent 视图，使分支/工作区元数据在 agent 工作时保持同步

以下功能尚未实现：完整聊天 Webview、VS Code Agent 视图聊天/编辑器集成、内联编辑应用、市场发布流程、重试/撤销/快照 GUI 端点。

## 本地使用

```bash
pnpm install
pnpm compile
pnpm package
code --install-extension mimofan-vscode-0.8.53.vsix
```

在 VS Code 设置中配置以下选项：
- `mimofan.commandPath` — 命令路径
- `mimofan.runtimeHost` — 运行时主机
- `mimofan.runtimePort` — 运行时端口
- `mimofan.runtimeToken` — 运行时令牌
- `mimofan.agentViewRefreshIntervalSeconds` — Agent 视图自动刷新间隔（秒）

将刷新间隔设为 `0` 可禁用自动只读刷新。

除非你明确使用可信的本地网络控件进行前端代理，否则请将运行时保持在 `127.0.0.1`。
