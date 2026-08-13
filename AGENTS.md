# AGENTS.md — 多 Agent / 多 Worktree 并发开发协调约定

本文件面向**通过 CodeBuddy / code agent 并行驱动开发**的场景：多个 subagent、
多个 git worktree 同时工作时，如何**从根上避免代码冲突与互相覆盖**。
它是对 `CLAUDE.md`（含「Worktree 开发约定」「Agent Teams」）与 `CODEBUDDY.md`
的**补充与操作化**，遇到矛盾以本文件 + `CLAUDE.md` 为准。

---

## 0. 为什么需要这套约定（冲突根因）

并行开发真正的冲突，绝大多数**不是** `git merge` 时的文本冲突，而是：

1. **共享工作区的全局 git 操作波及全员**：在共享 worktree 里跑
   `git reset --hard` / `git stash push ... && ... && git stash pop`，
   会因为**全局 stash** 把其他 agent 的工作 `stranded` 到多个 stash 中，
   并被并发 agent 改写而静默丢失。（真实事故：后台测试命令的
   `stash push/pop` 导致工作丢失。）
2. **worktree 路径陷阱**：在多 worktree 并行时，若用**相对路径或主仓库路径**
   编辑文件，改动会落到**主仓库或另一个 worktree**，而非预期目标，
   造成「已实现的功能被重复实现 / 改错文件」。
3. **重复派活**：当任务以成对命名出现（如 `edit-foo` / `impl-edit-foo`、
   中文名 + 英文名各派一次），两个 agent 会**同时写同一文件**，互相覆盖。
4. **过期备份副本制造假阻塞**：同名文件留两份、状态性注释过期，
   队友会据此误报「已被阻塞 / 已实现」，导致重复工作或错误裁决。
5. **「已复核」结论与磁盘不符**：队友的复核报告可能未反映真实改动；
   直接信任会基于错误前提动手。
6. **并行 `cargo test` 撞锁**：即使隔离了 `target` 目录，仍会撞上**自己遗留进程**
   持有的 file lock，导致假失败。

> 结论：冲突的本质是**「没有统一的归属边界 + 没有先取证就动手 + 危险的全局 git 操作」**。
> 下述方案通过**物理隔离（worktree）+ 逻辑归属（文件清单）+ 取证优先 + 锁隔离**
> 把并发从「会撞」变成「撞不到」。

---

## 1. 核心原则（按优先级）

| # | 原则 | 直接解决的根因 |
|---|------|----------------|
| 1 | **文件归属隔离**：每个 agent 只负责**互不相交的文件集合**，禁止两人同时编辑同一文件 | 根因 3、2 |
| 2 | **worktree 物理隔离**：跨 crate / 多文件重构必须在**独立 worktree** 开发 | 根因 1、6 |
| 3 | **共享任务列表协调**：用 Task List 认领，完成一个再认领下一个 | 根因 3 |
| 4 | **取证优先**：动手前先 `ls` / `git diff` / `git status` 自查，不靠推断 | 根因 4、5 |
| 5 | **git 禁忌**：共享 worktree 内**禁止**裸 `reset --hard` / `stash push-pop` | 根因 1 |
| 6 | **编译 / 测试锁隔离**：隔离 `CARGO_TARGET_DIR`，用 `--no-run` + 直跑测试二进制 | 根因 6 |

---

## 2. 推荐工作流

### 2.1 启动并行团队（自然语言即可）

```
我需要给 mimofan 添加批量导入功能。创建一个团队：
- 一个负责后端 API 与数据库迁移
- 一个负责前端界面与交互
- 一个负责编写集成测试
先让架构师成员设计接口规范，其他人基于规范并行开发。
```

约束（详见 `CODEBUDDY.md` 的 Agent Teams 章节）：
- 每个成员负责**不同的文件集合**；
- 用**共享 Task List** 协调认领；
- 权限默认继承 `subagentPermissionMode`（已设为 `dontAsk`）。

### 2.2 大重构：必须开 worktree

```bash
git worktree add ../agent-mimofan-worktree -b refactor/xxx
```

- worktree 内确保 `cargo build`（零 warning）+ `cargo test`（全 workspace 零失败）通过，再合并；
- **合并在主仓库执行**：`git merge <worktree 分支>` 到 `main`，确认无冲突且构建/测试仍绿后 `git push origin main`；
- 合并后**立即清理**：`git branch -d <branch>`、`git push origin --delete <branch>`、`git worktree remove <path> --force`。

### 2.3 合并闭环（越快越好）

- 完成即 `git commit`，**不要长期堆积未提交改动**；
- 已合并分支尽快删除，避免分支堆积与混淆（详见 `CLAUDE.md` Worktree 约定）。

---

## 3. 可直接复用的协调提示词模板

### 3.1 给 Leader / Supervisor（拆分与分配）

```
你是一个并行开发团队的协调者。任务：<一句话目标>。

步骤：
1. 先把任务拆成**互不相交的文件集合**，每个子任务绑定明确的文件路径清单。
2. 为每个子任务起**唯一、无歧义**的名称（禁止出现成对/中英文重复命名）。
3. 通过 TeamCreate 派生成员，每个成员只认领**自己那份文件清单**。
4. 成员完成当前任务后自动认领下一个未分配/未阻塞任务（共享 Task List）。
5. 你只做协调（拆分/分配/汇总），实际改动全部由成员完成。
6. 合并前，逐一确认每个成员已在**自己的 worktree** 内 cargo build/test 全绿。

禁止：
- 让两个成员编辑同一文件；
- 使用相对路径或主仓库路径编辑（必须用 worktree 绝对路径）；
- 在共享 worktree 跑 `git reset --hard` / `git stash push ... && pop`。
```

### 3.2 给每个 Member（执行者）

```
你是并行团队的一个成员，负责以下**专属文件清单**（其他人不会动这些文件）：
<文件清单>

执行前必做（取证优先）：
1. `ls <目标路径>` 确认文件存在且属于你的 worktree；
2. `git diff --stat` / `git status` 确认当前工作区状态，不靠推断；
3. 若发现同名备份副本或状态性注释，先核实其真实性再下结论。

执行中：
- 只用 worktree 绝对路径编辑；
- 隔离编译：设置 `CARGO_TARGET_DIR` 到本 worktree 私有目录；
- 跑测试用 `cargo test --no-run` 生成后**直跑测试二进制**，避免撞锁假失败；
- 不跑 `git reset` / `git stash`，需要暂存先抽到 /tmp 保底。

完成后：
- 立刻 `git commit`（不要堆积）；
- 在共享 Task List 标记完成，并认领下一个未分配任务；
- 向 Leader 回报：改了哪些文件、构建/测试结果、是否有交叉依赖待合并。

对队友结论保持怀疑：若队友声称「已复核 / 已实现」，先 `git diff` 自行验证再采纳。
```

### 3.3 为什么这套提示词能减少冲突

- **文件清单 == 物理边界**：把「可能撞」变成「没有交集」，从数学上消除同文件双写；
- **唯一命名 + 共享 Task List**：消除了重复派活（根因 3）与认领竞争；
- **取证优先（ls/git diff）**：在动手前就暴露路径陷阱（根因 2）与过期副本（根因 4）；
- **绝对路径 + worktree 隔离**：让每个 agent 的改动落在确定位置，互不串扰；
- **git 禁忌 + /tmp 保底**：根除全局 stash 导致的静默丢失（根因 1）；
- **锁隔离编译测试**：消除并行 cargo 撞锁假失败（根因 6）；
- **尽快 commit + 合并闭环**：缩短「未提交改动共存」窗口，降低事故面。

---

## 4. git 操作禁忌速查（共享 worktree）

| 操作 | 是否允许 | 说明 |
|------|:--------:|------|
| `git commit`（本 worktree） | ✅ | 完成即提交，缩短共存窗口 |
| `git worktree add/remove` | ✅ | 物理隔离的标准手段 |
| `git merge <分支>`（仅主仓库） | ✅ | 合并回主干的唯一入口 |
| `git reset --hard` | ❌ | 全局副作用，会清掉他人工作 |
| `git stash push ... && pop` | ❌ | 全局 stash 致工作 stranded 并被并发改写 |
| 相对路径 / 主仓路径编辑 | ❌ | 落到错误位置，重复实现或改错文件 |
| 裸 `git commit -a` / 大范围 add | ❌ | 易把他人未提交改动一并带入 |

**误事故障恢复姿势**：若已发生 stash 事故，**禁用 `git stash pop`**
（会覆盖），改用路径限定 `git checkout <stash> -- <path>` 精确恢复。

---

## 5. 编译 / 测试隔离（Rust workspace）

```bash
# 每个 worktree 用独立 target 目录，避免 target 争用
export CARGO_TARGET_DIR="$PWD/target-$(basename $PWD)"

# 生成测试二进制后直跑，绕开并行 cargo 抢锁
cargo test --no-run --workspace
# 然后直接执行生成的测试二进制（路径见 target/debug/deps/）
```

---

## 6. 与其他文档的关系

- `CLAUDE.md`：「Worktree 开发约定」「关键设计约束」「代码质量规则」为权威基线；
- `CODEBUDDY.md`：「Agent Teams」多实例并行约定；
- `docs/SUBAGENTS.md`：subagent 角色与并发上限；
- 本文件聚焦**跨 agent / 跨 worktree 的协调与防冲突**，是对上面三者的操作化补充。
