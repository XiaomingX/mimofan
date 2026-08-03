# AI Agent 开发规范

本文件定义 AI Agent（Claude Code、Codex、Cursor 等）在 mimofan 项目中的标准开发工作流。
**代码质量规则、架构约束、技术细节一律以 `CLAUDE.md` 为准**，本文件仅规定工作流程。

---

## 1. 开发模式：Git Worktree 隔离

**核心原则**：每个任务（feature、fix、refactor）在独立 worktree 中进行，绝不直接在 `main` 上修改。

### 1.1 创建 Worktree

```bash
# 从 main 创建隔离 worktree
git worktree add .claude/worktrees/<task-slug> -b <branch-name>

# 示例
git worktree add .claude/worktrees/fix-mcp-auth -b fix/mcp-auth-timeout
```

### 1.2 分支命名规范

| 类型 | 格式 | 示例 |
|---|---|---|
| 功能 | `feat/<slug>` | `feat/a2a-protocol` |
| 修复 | `fix/<slug>` | `fix/mcp-oauth-timeout` |
| 重构 | `refactor/<slug>` | `refactor/separate-tests` |
| 文档 | `docs/<slug>` | `docs/update-architecture` |
| 性能 | `perf/<slug>` | `perf/reduce-clone-ui` |

### 1.3 Worktree 清理

```bash
# 完成后移除 worktree
git worktree remove .claude/worktrees/<task-slug>

# 清理残留
git worktree prune
```

---

## 2. 开发流程（必须按顺序执行）

```
┌─────────────┐    ┌──────────────┐    ┌──────────────┐    ┌─────────────┐
│  1. 拉取    │───▶│  2. 开发     │───▶│  3. 验证     │───▶│  4. 提交    │
│  main 最新  │    │  代码修改    │    │  CI 全通过   │    │  规范 commit │
└─────────────┘    └──────────────┘    └──────────────┘    └─────────────┘
                                                                   │
                                                                   ▼
                                                            ┌─────────────┐
                                                            │  5. 推送    │
                                                            │  创建 PR    │
                                                            └─────────────┘
```

### Step 1：同步 main

```bash
git checkout main
git pull origin main
```

### Step 2：创建 Worktree 并开发

```bash
git worktree add .claude/worktrees/<task-slug> -b <branch-name>
cd .claude/worktrees/<task-slug>
# ... 进行代码修改 ...
```

### Step 3：验证（CI 门禁，必须全部通过）

```bash
# 格式化检查
cargo fmt --all -- --check

# Clippy 静态分析
cargo clippy --workspace --all-features --locked -- \
  -D warnings \
  -A clippy::uninlined_format_args \
  -A clippy::too_many_arguments \
  -A clippy::unnecessary_map_or \
  -A clippy::assertions_on_constants

# 工作区编译检查
cargo check --workspace

# 完整测试
cargo test --workspace
```

**任何一步失败都必须修复后重新验证，禁止跳过。**

### Step 4：规范提交

```bash
git add -A
git commit -m "type(scope): 简明扼要的中文描述

- 具体修改点 1
- 具体修改点 2"
```

**Commit 消息规范**：
- `type`：feat / fix / refactor / docs / perf / test / chore
- `scope`：影响的 crate（如 `tui`、`config`、`mcp`）
- 描述：中文，简洁明了
- **禁止添加机器人/工具 `Co-authored-by` 尾部**（Claude、codex、cursor）——CI `check-coauthor-trailers.py` 会拒绝
- 为外部贡献者保留 `Co-authored-by` 信用时，使用 `.github/AUTHOR_MAP` 中的规范 GitHub noreply 身份

### Step 5：推送并创建 PR

```bash
git push origin <branch-name>
gh pr create --title "type(scope): 标题" --delete-branch --body "$(cat <<'EOF'
## Summary

简述修改内容和原因。

## Testing

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features`
- [ ] `cargo test --workspace --all-features`

## Checklist

- [ ] Updated docs or comments as needed
- [ ] Added or updated tests where relevant
- [ ] Verified TUI behavior manually if UI changes
EOF
)"
```

---

## 3. 代码质量规则

**完整规则见 `CLAUDE.md`**，以下为 Agent 最常违反的要点速查：

| 要点 | 要求 | 详见 CLAUDE.md |
|---|---|---|
| 禁止裸 `unwrap()` | 使用 `.expect("reason")` 或 `?` | 错误处理 |
| 禁止 `Box<dyn Any>` | 使用枚举或 trait | 类型安全 |
| 禁止硬编码密钥 | 使用 `mimofan-secrets` 或环境变量 | 安全 |
| 禁止绕过 `execpolicy` | 所有 shell 命令通过沙箱 | 安全 |
| 禁止新增 `#[allow(clippy::...)]` | 修复 clippy 警告 | Clippy 配置 |
| 新依赖 | 先在 `[workspace.dependencies]` 声明 | 依赖卫生 |
| 库 crate 错误 | `thiserror`；二进制 crate 用 `anyhow` | 错误处理 |
| 异步测试 | `#[tokio::test]`；时间敏感用 `start_paused` | 测试标准 |

---

## 4. 架构约束

**完整约束见 `CLAUDE.md`「关键设计约束」**，核心不可违反项：

1. 面向模型的工具仅限 `agent`，不存在 `agent_open` / `agent_eval` / `agent_close` / `delegate_to_agent`
2. 不引入容量/一致性/运行时标签系统
3. `constitution.md` 是唯一基础提示词，不注入运行时提示词
4. 子智能体深度可配置，不新增任意限制
5. 不提交推测性的 `spawn_blocking` 修复（v0.8.61 已解决 TUI 冻结）

---

## 5. 发布流程（仅维护者）

```bash
# 从 main 创建发布候选分支
git checkout main && git pull
git checkout -b release/v<version>

# 推送并创建 Release PR
git push origin release/v<version>
gh pr create --title "release: v<version>" --body "Release candidate for v<version>"
```

- 版本号在 `Cargo.toml` 中管理
- **未经 Hunter 明确批准，禁止打标签、发布 GitHub Release、推送发布工件**

---

## 6. 快速参考

```bash
# 格式化
cargo fmt

# 验证三件套
cargo fmt --all -- --check && cargo clippy --workspace --all-features --locked -- -D warnings && cargo test --workspace

# 构建 Release
cargo build --release -p mimofan
```
