---
name: parallel-agent-lock-guard
description: 并行 Agent / 多 agent 开发（monorepo、cargo workspace、code harness agent）时规避锁竞争（cargo/npm package cache 锁、git index 锁、共享文件写冲突）的最佳实践与可执行配置。当用户说"并行 agent 锁竞争""多 agent 并行构建失败/卡住""package cache lock""避免 git index.lock""并行编译怎么隔离""避免下次锁竞争"时触发。
agent_created: true
---

# 并行 Agent 锁竞争规避（parallel-agent-lock-guard）

## 目的
多 agent 并行开发时，多个进程会抢同一份**共享可变状态**（cargo registry、git index、npm 全局 cache、共享文件），触发 file lock 串行化甚至死等。本 skill 提供分层隔离方案与可落地配置，让并行 agent 互不阻塞。

## 何时使用
- 用户启动/规划多 agent 并行开发框架（monorepo、code harness agent、Claude Code 多 agent 插件）。
- 出现 `Blocking waiting for file lock on package cache`、git `index.lock` 冲突、文件被覆盖/读到半成品。
- 用户问"并行 agent 锁竞争怎么解决""并行构建如何隔离""如何避免下次锁竞争"。

## 锁竞争分类（4 类，按频率）
| 类型 | 现象 | 根因 |
|---|---|---|
| 包管理器缓存锁（最常见） | `Blocking waiting for file lock on package cache` | 多 cargo/npm 进程抢 `~/.cargo/registry` 或全局 npm cache 写锁 |
| git index/ref 锁 | `Unable to create '.git/index.lock'` | 并行 agent 在同一工作副本做 git 写操作 |
| 共享文件写冲突 | 文件被覆盖、编译读到半成品 | 多 agent 写同一文件/目录（如同一 Cargo.toml） |
| CI runner 内锁 | 同机并行 step 互等 | 同 runner 内多个构建 step 共享 registry/cache |

## 最佳实践（5 层，MECE）
**A. 进程级——下载与编译分离**
- 所有写 registry/cache 的操作（`cargo fetch` / `npm install` / `cargo install`）作为**全局一次性预热**，跑在 agent 并行启动之前。
- 并行阶段所有 agent 用 `--offline` / `--locked` / `--frozen`，只读 cache，不碰写锁。
- `install` 类工具（cargo-audit 等）**禁止与 build 并行**，要么预装进基础镜像，要么放串行 phase。

**B. 仓库级——git worktree 隔离工作副本**
- 每个 agent 一个独立 worktree，彻底隔离 `index.lock` / `refs`，互不干扰。

**C. 文件级——按域分片所有权 + 写队列**
- monorepo 按 crate/package 给 agent 分配目录所有权，不交叉写。
- 任何必须写共享资源（git push、写汇总文件）的操作走 harness 的**单一串行队列**。

**D. 缓存级——sccache 是解药不是问题**
- sccache 原生并发安全。配置 `SCCACHE_DIR` 持久化可放大命中、避免重复编译，反而缓解竞争。

**E. 调度级——重试退避**
- 编译类 job 加重试退避（如 3 次重试 + 30s sleep），吸收瞬时锁等待。

## 可执行配置
1. **预热 + 离线只读**：运行 `bash scripts/warm-cargo-parallel.sh <project_dir>`（等价于 `cargo fetch --locked`），之后各 agent 用：
   ```bash
   CARGO_TARGET_DIR=./target-agent-$AGENT_ID \
   CARGO_NET_OFFLINE=true \
   cargo build --locked --offline --target $TARGET
   ```
2. **git worktree 隔离**：
   ```bash
   git worktree add -b agent/$AGENT_ID ../agent-$AGENT_ID main
   cd ../agent-$AGENT_ID   # 独立 index/working tree，零锁竞争
   ```
3. **彻底隔离（备选）**：为每个 agent 设独立 `CARGO_HOME`——registry cache 完全不共享，代价是重复下载，仅在无法预热时用。
4. **sccache 持久化**：确保 `SCCACHE_DIR` 在 agent 间共享且持久（mimofan 已配置 `~/.cargo/config.toml` 并关闭 `incremental`）。
5. **install 串行化**：cargo-audit / 其他 `cargo install` 工具移到串行 phase 末或预装。

## 针对 mimofan 的具体约定
- 项目是 cargo workspace（`resolver=2`、`edition=2024`），多 crate 用 `.workspace=true` 复用中央依赖。
- 已配 sccache，实测命中 ~96%，二次重建仅重编 1 crate——保持 `SCCACHE_DIR` 持久以延续收益。
- **真实案例**：agent-mimofan 曾并行 `cargo build` + `cargo install cargo-audit`，互相阻塞 ~13min 于 package cache 锁。规避=build 前先 `cargo fetch --locked`，install/工具串行或预装。
- **依赖多版本分叉**（starlark / portable-pty 等传递依赖）属 semver 分叉，**不是锁竞争**，不在本 skill范围；勿为消除重复而强制统一版本。

## 工作流（落地步骤）
1. 并行启动前：在该仓库跑 `bash scripts/warm-cargo-parallel.sh <project_dir>`（= `cargo fetch --locked`）完成 registry 预热。
2. 给每个 agent 分配独立 git worktree + 独立 `CARGO_TARGET_DIR`。
3. 并行阶段：各 agent `CARGO_NET_OFFLINE=true cargo build --locked --offline --target <t>`。
4. install 类工具移到串行 phase 或预装进基础镜像。
5. 写共享资源（git push、汇总文件）走 harness 单一串行队列。

## 反模式（不要做）
- 不在并行区跑 `cargo install` / `cargo update`（写 Cargo.lock + registry）。
- 不让多 agent 共享同一 worktree 做 git 写。
- 不要为"消除重复依赖版本"而强制统一版本——那是传递依赖分叉，风险高、非锁竞争。
