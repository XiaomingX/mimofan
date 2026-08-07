# CODEBUDDY.md — mimofan 项目指南

mimofan 是一个 Rust 终端 AI 编码助手，原生支持 Xiaomi MiMo & DeepSeek 等多 Provider。

## 多 Agent 并行协作约定（Agent Teams）

本项目支持并鼓励使用 CodeBuddy 的 **Agent Teams** 能力，让多个 agent 同时并行工作。
并行 agent 会自动加载本文件作为项目上下文。

### 何时用 Agent Teams（多实例并行）
- 并行探索：多个成员同时调研问题的不同方面。
- 新模块开发：每个成员负责独立模块，互不干扰。
- 跨层开发：前端/后端/测试各由不同成员负责，同步推进。
- 竞争性调试：成员并行验证不同假设，更快收敛根因。

### 关键约束（避免冲突与浪费）
- **文件归属清晰**：拆分任务时确保每个成员负责**不同的文件集合**，禁止两个成员同时编辑同一文件（会互相覆盖）。
- **共享任务列表**：用 Task List 协调，成员完成当前任务后自动认领下一个未分配/未阻塞任务。
- **委派模式**：按 `Shift+Tab` 切换，让领导只做协调（拆分/分配/汇总），实际工作全部由成员/subagent 完成。
- **权限**：并行成员默认继承 `subagentPermissionMode`（在 `~/.codebuddy/settings.json` 已设为 `dontAsk`），无需逐个弹权限请求。

### 怎么启动一个并行团队（自然语言即可）
```
我需要给 mimofan 添加批量导入功能。创建一个团队：
- 一个负责后端 API 与数据库迁移
- 一个负责前端界面与交互
- 一个负责编写集成测试
先让架构师成员设计接口规范，其他人基于规范并行开发。
```
或指定规模与模型：
```
创建一个 4 人团队并行重构这些模块，每个成员使用 lite 模型。
```

### 与成员交互
- `@成员名` 直接对话；`@all` 广播；`Ctrl+T` 切换任务列表；`↓` 进入成员焦点导航；`Ctrl+O` 返回主视图。
- 已完成成员收到新消息会自动重启，可随时重新唤醒。

### 已知限制（实验阶段）
- 每个会话只能管理一个团队，清理后才能建新团队。
- 不支持嵌套团队；领导角色固定、不可转让。
- 无会话恢复：`/resume`、`/rewind` 不会恢复成员，需让领导重新生成。

## 分支与 Worktree 约定

- **大重构用 worktree**：涉及多文件、多 crate 的收敛/重构类任务（如 Provider 模式收敛），应在独立 git worktree 中开发（例如 `git worktree add ../agent-mimofan-worktree -b refactor/xxx`），避免污染主工作区。
- **合并前先验证**：worktree 内确保 `cargo build`（零 warning）与 `cargo test`（全 workspace 零失败）通过，再合并回主干。
- **合并在主仓库执行**：在**主仓库**中 `git merge <worktree 分支>` 到 `main`（worktree 不持有 `main`），确认无冲突且构建/测试仍绿后 `git push origin main`。
- **合并后删除过时分支**：合并到主干后，必须清理该分支——本地 `git branch -d <branch>`、远程 `git push origin --delete <branch>`，并用 `git worktree remove <path> --force` 删除 worktree 目录。不要长期保留已合并的特性分支。
