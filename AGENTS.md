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
7. **过时副本分支遗留**：从**不领先 `origin/main` 的旧 `main`** 切出 worktree /
   分支，而 `main` 后续已通过正式 PR 合入相同功能。结果产生一批「看似祖先、
   实为等价副本」的遗留分支，`git diff` 因基线漂移显示海量假删除，
   且差点被当成「未合并工作」强行合入（真实事故：4 个 `feat/issue-*` 分支
   内容已逐字节/仅格式差异存在于 `main`）。
8. **等价副本强行合并**：未核验就 `git merge` 一个工作**已是 `main` 祖先**的分支，
   产生大量假冲突并退化为 `main` 现状，反而引入混乱。
9. **Rust 编译错误堆积**：跨 crate 接口变更 / 合并冲突解决后未立即 `cargo check`，
   一次性大改后爆出几十条签名 / 模块 / trait 错误，定位困难。

> 结论：冲突的本质是**「没有统一的归属边界 + 没有先取证就动手 + 危险的全局 git 操作
> + 分支基线不领先主干 + 合并前未核验等价性」**。
> 下述方案通过**物理隔离（worktree）+ 逻辑归属（文件清单）+ 取证优先 + 锁隔离
> + 基线对齐（§2.4）+ 合并前等价性核验（§2.5）+ Rust 小步验证（§2.6）**
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
| 7 | **基线对齐**：开 worktree / 分支前先 `git fetch` 并基于 `origin/main`，不从不领先的本地 `main` 切出 | 过时副本遗留 |
| 8 | **合并前核验**：`rebase origin/main` + `merge-base --is-ancestor` 严格判定，禁止把已是 main 祖先的等价副本分支强行合并 | 重复实现 / 假冲突 |
| 9 | **Rust 小步验证**：改完即 `cargo check` 受影响 crate，零 error 再继续；跨 crate 接口先定义后调用方 | 编译错误堆积 |

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

### 2.4 开 worktree / 分支前：先对齐最新 `origin/main`（防「过时副本」遗留）

> 真实事故：多个分支从 18 小时前的旧 `main` 切出，期间 `main`
> 已通过正式 PR 合入相同功能。结果产生一批「看起来是祖先、实为等价副本」
> 的遗留分支；`git diff main..branch` 因基线漂移显示「删万行」，
> 实则功能是 `main` 已有且更完整的版本。

开 worktree / 建分支的**强制前置动作**：

```bash
git fetch origin
# 确认要做的改动在最新 origin/main 上确实不存在（不是已有功能的重复实现）
git log origin/main --oneline | grep -i "<功能关键词>"
# 从最新 origin/main 切出，而非本地旧 main
git worktree add ../agent-mimofan-<name> -b feat/<name> origin/main
```

- **禁止**从本地落后 `origin/main` 的 `main` 直接 `git worktree add -b`；
- 并行开多个 worktree 时，每个都基于 `origin/main`（先统一 `fetch`）；
- 若发现目标功能已在 `origin/main`，**不要**新开分支，直接复用或关闭任务。

### 2.5 合并前：rebase + 等价性核验（防把过时副本强行合入）

合并回 `main` 前，必须执行：

```bash
git fetch origin
git rebase origin/main          # 先对齐最新主干，暴露真实冲突而非基线漂移
# 核验「这个分支是否还有 main 没有的工作」
git merge-base --is-ancestor <branch-tip> origin/main && echo "已合入, 直接删分支" || echo "仍有独有工作"
```

- 若 `merge-base --is-ancestor` 为真 → 分支工作**已是 main 的祖先**，
  **直接删除分支**，不要合并（合并会 conflict 并退化为 main 现状）；
- 若分支与 `main` 同名文件**内容逐字节/仅格式差异** → 视为重复实现，删除分支；
- 只有确认分支含 `main` **没有**的真实改动时，才 `git merge` / `rebase` 合并；
- 合并后**立即**删本地 + 远程分支（见 §2.2 清理三步），杜绝堆积。

> 判断「是否已在 main」**必须按 commit 哈希 + 文件内容严格核验**，
> 不能只看 `git log` 标题（merge 会带入父提交标题造成误报）。

### 2.6 Rust 改动：小步 `cargo check` 验证，避免编译错误堆积

> 本仓库 Rust 改动容易在合并/接口变更时编译失败（签名不匹配、
> 模块未注册、trait 未实现等）。务必小步验证，不要一次性大改后
> 才发现几十个错误。

纪律：

1. **改完即 `cargo check -p <受影响crate>`**（比 `cargo build` 快得多），
   零 error 再继续；
2. 跨 crate 接口变更（如函数签名 / 新增模块）时，**先改定义、再改全部调用方**，
   用 `cargo check` 让编译器列出每个调用点，逐个修；
3. 解决合并冲突后，**必须** `cargo check` 验证再 `git commit`；
4. 注意 feature 作用域：如 `lang-java` 属于 `mimofan-staticanalysis`，
   `cargo test -p mimofan --features lang-java` 会报错；feature 要加在定义它的包上；
5. 全 workspace 验收用 `cargo build`（零 warning）＋ 针对性 `cargo test`，
   不靠「看起来应该能编」的推断。

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
