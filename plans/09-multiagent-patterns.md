# Plan: 借鉴 Anthropic 多智能体系统研究,补齐 mimofan 缺失的基础能力

> 参考: https://www.anthropic.com/research/multiagent-systems (2026-08-13)
> 目标: 把文章中"已验证有效"且 mimofan 当前缺失的模式,作为基础能力实现进来,
>       用 MECE 基准(含新增 D07 多 Agent 编排域)做验收,最后发小版本。

## 0. 调研结论(事实基线,均来自源码证据)

### 文章核心有效模式
- **协调 swarm + 共享论坛 + arbiter 仲裁**:45 个隔离 agent 通过共享论坛协调找漏洞,发现 266 个(独立并行仅 21 个);独立 arbiter agent 做最终有效性判定,杜绝从众/误报。
- **可验证的 bake-off 冲突解决**:用客观可验证指标(如"编译通过率/Rust check")打破目标冲突 truce。
- **每 agent 独立执行环境 + 共享有限资源**:独立 VM + 共享仓库/论坛,隔离失败会导致"turf war"互杀。
- **反模式警示**:无层级平等同伴协调会崩;从众/低方差导致集体错误(30 agent 建同名分支);过早共识(hidden profile)忽略个别决定性事实;字面目标执行引发战争。

### mimofan 现状(已具备)
- 角色化 sub-agent(7 种)+ 工具/上下文/worktree 隔离 + fork 缓存复用 + 并发限流 gate
- 持久化 DAG 任务队列(`TaskManager`)+ 依赖边 + 环检测
- Fleet 多 worker 编排(Local/Ssh 进程式 `mimofan exec`,ledger/租约/心跳/背压/崩溃恢复)
- Workflow 声明式 DAG 引擎(并行/顺序/条件/retry/resume)
- cron 定时自动化
- MECE 1000 基准(13 域,D07=多 Agent 编排)+ 多层 harness

### mimofan 真缺口(文章强调且当前缺失)
1. **AgentBus 未暴露给模型**:`bus.rs` 只实现运行时内部 API,未 `impl ToolSpec`,注册表无 bus。模型无法主动 publish/subscribe,agent 间只能经父中转(`<mimo:subagent.done>`)。→ 缺"共享论坛"。
2. **无 arbiter/仲裁者角色**:`SubAgentType` 7 种里没有独立的有效性判定者,无法做文章里"独立 arbiter 判定漏洞是否 new+valid"的去重/去伪。
3. **无协同知识汇聚(共享论坛)持久化**:AgentBus 内存态,进程重启不落盘;无跨 agent 的"发现 X 已被提交"共享黑板,易从众重复。
4. **无 bake-off 可验证冲突解决原语**:目标冲突时缺"用客观指标(编译/测试通过)裁决"的工具。
5. **Fleet 无模型驱动路径**:只能 CLI/TOML 驱动,模型不能经 ToolSpec 主动编排 fleet;Docker host 未实现。

---

## Phase 1: 提交 Issue(对齐根因,先建 issue 再动手)

为每个缺口建 issue,引用文章对应发现作依据。编号延续 #834+。

- **#834 feat(subagent)**: 把 AgentBus 暴露为模型可见 ToolSpec(`bus.publish/subscribe/state_*`),让子 agent 能主动协调(对应文章"协调 swarm 靠共享论坛")。
- **#835 feat(subagent)**: 新增 `Arbiter` 角色类型 + 去重/去伪判定工具(对应文章"独立 arbiter 判定 new+valid")。
- **#836 feat(subagent)**: AgentBus 持久化 + 共享黑板(跨进程重启恢复,防从众重复)。
- **#837 feat(subagent)**: bake-off 可验证冲突解决原语(客观指标:编译/测试通过率裁决)。
- **#838 feat(fleet)**: 模型驱动 Fleet 的 ToolSpec 包装(local/ssh)+ Docker host adapter 占位实现。
- **#839 test(bench)**: MECE D07 多 Agent 编排域新增可验收样本(覆盖 swarm 协调/arbiter/共享论坛场景)。

每个 issue 正文含:依据文章哪段、当前证据(文件路径)、验收标准(grep/单测/MECE 域提升)。

## Phase 2: 实现 #834 — AgentBus 暴露为模型可调工具
**What**: 在 `crates/tui/src/tools/subagent/bus.rs` 增加 `impl ToolSpec`(或新增 `bus_tool.rs`),封装 `publish/subscribe/state_set/state_get` 为工具调用;在 `tools/mod.rs` / `registry.rs` 注册为 `bus`。
**Doc refs**: `bus.rs` 现有方法签名(`publish` L130 / `subscribe` L92 / `state_set` L141 / `state_get` L147);参考 `AgentTool`(`tool.rs:52` impl ToolSpec)的注册范式。
**Verify**:
- `cargo build` 零 warning;`bus` 出现在工具注册表(grep `register.*bus` / tool name `bus`)。
- 单测:子 agent 经 `bus.publish` 发布,另一 agent 经 `bus.subscribe` 收到(新增 `crates/tui/tests/tools_subagent_bus.rs`)。
**Anti-pattern**: 不要重造 bus 内部机制,只做 ToolSpec 包装层;不要改 AgentBus 现有 API 语义。

## Phase 3: 实现 #836 — AgentBus 持久化 + 共享黑板
**What**: `bus.rs` 增加 journal(追加写 `.mimofan/state/bus_journal.jsonl`)+ 启动时 replay 恢复订阅/状态;共享 KV 作为"黑板"供 swarm 写"已发现 X"。
**Doc refs**: 参考 `persistence.rs`(subagent 持久化范式)与 `manager.rs:28` bus 构造点。
**Verify**:
- 进程重启后 `bus.state_get` 仍可读到重启前写入的 key(集成测试)。
- 单测:并发 30 agent 写同名 key,后写者能读到"已存在"→ 验证防从众(对应文章 30 agent 同名分支反模式)。
**Anti-pattern**: 不要把全部消息落盘(只落 state KV + 订阅关系),避免 IO 爆炸。

## Phase 4: 实现 #835 — Arbiter 角色 + 去重/去伪工具
**What**: `types.rs` `SubAgentType` 增 `Arbiter`;新工具 `arbiter.judge(item)` 接收待判定项,查询 bus 黑板判定是否 new+valid(去重=已在黑板;去伪=触发独立二次校验 prompt)。`tool.rs` 增加对应 ToolSpec。
**Doc refs**: `types.rs:39` SubAgentType 定义;`role_posture_permits`(`mod.rs:1895`)——Arbiter 应为 read-only 姿态(只读 bus + 调用 judge,不写代码)。
**Verify**:
- 单测:提交一个已知重复项 → arbiter 返回 `Duplicate`;提交未见过且经校验有效的项 → `Valid`。
- 对应文章"45 agent 协调 + 独立 arbiter 判 new+valid"的减半误报效果。
**Anti-pattern**: Arbiter 不要有写权限(避免变成另一个 implementer);去重判定必须基于持久化黑板(Phase 3)。

## Phase 5: 实现 #837 — bake-off 可验证冲突解决原语
**What**: 新工具 `bakeoff.run(criterion, candidates)` —— criterion 为可客观验证的指标(如 `cargo build`/`cargo test` 通过),对候选方案各跑一次,返回通过者胜出;供冲突 agent 经共享论坛达成 truce(对应文章 Mythos 5 的 bake-off)。
**Doc refs**: 复用 `crates/tui/src/tools/` 现有 shell 执行能力;结果回写 bus 黑板。
**Verify**:
- 单测:两个冲突候选,仅其一编译通过 → bakeoff 返回该候选。
**Anti-pattern**: 不要让 agent 自己声明胜负,必须跑客观命令取退出码。

## Phase 6: 实现 #838 — 模型驱动 Fleet 的 ToolSpec + Docker 占位
**What**: 新增 `fleet_tool.rs` `impl ToolSpec`,包装 `FleetManager` 的 `create_run`/`run_to_completion`(CLI 路径:`cli/fleet_cmd.rs:284`),暴露 `fleet.run(spec)`;Docker host 在 `validate_worker_hosts`(`manager.rs:1333`)改为"占位实现 + 明确 unsupported 提示"而非硬 reject(或保留 reject 但文档化)。
**Doc refs**: `fleet/manager.rs:387 run_to_completion`、`task_spec.rs:98 load_task_spec_document`、`executor.rs:234 start_worker_on_host`(仅 Local/Ssh)。
**Verify**:
- `cargo build` 零 warning;`fleet` 出现在工具注册表。
- 集成测试:`fleet.run` 经 ToolSpec 启动 local worker 完成一个 TOML spec 任务。
**Anti-pattern**: 不要绕过 `validate_worker_hosts` 直接允许 Docker(未实现 adapter 会运行时崩);如需 Docker,单独开 issue。

## Phase 7: 实现 #839 — MECE D07 新增可验收样本
**What**: 在 `benchmark/agentbench/samples/mece_1000/` 与 `MECE_TAXONOMY.md` 的 D07 域新增样本,覆盖:
  - swarm 协调(多 agent 经共享论坛分工找漏洞)
  - arbiter 去重/去伪
  - 共享论坛防从众(不建同名分支)
  - bake-off 客观裁决
**Doc refs**: `benchmark/agentbench/mece_bench.py`(T1/T2/T3 判定 + 反作弊系数)`benchmark/agentbench/samples/MECE_TAXONOMY.md`。
**Verify**:
- `python benchmark/agentbench/mece_bench.py --domain D07` 能跑;新样本可被判分(至少有一部分 T3 exec 命中)。
- 反作弊:同 key T3 通过才把 T1 升 1.0(复用现有系数逻辑)。

## Phase 8: 验收(全 workspace 测试 + 基准对比)
- `cargo build`(零 warning)+ `cargo test --workspace`(零失败)。
- `cargo test` 覆盖 Phase 2-6 新增单测。
- 跑 MECE D07:`python benchmark/agentbench/mece_bench.py --domain D07`,记录基线 vs 优化后分数,确认提升且无回退。
- 跑 `benchmark/fleet_test.sh` 确认 Fleet 改动无回归。
- 反模式 grep 自检:确认 bus 工具是包装层(非重造)、Arbiter 为 read-only、bakeoff 走客观退出码、Docker 未非法放行。

## Phase 9: 合并主干 + 发小版本
- 在主仓库 `git merge <worktree/feature 分支>` 到 `main`(CODEBUDDY.md 约定:大重构用 worktree,合并在主仓库)。
- 确认 build/test/bench 仍绿 → `git push origin main`。
- 清理特性分支(`git branch -d` + `git push origin --delete` + `git worktree remove --force`)。
- 发小版本:`Cargo.toml` version 0.0.17 → **0.0.18**(`git tag v0.0.18` + push tag)。
- 关闭 issue #834-#839,在 #839 评论附 D07 提升数据。

## 关键风险与记忆交叉校验
- 历史 memory 提到 `tools-security worktree` 曾整树丢失、安全接线未合入 → **本计划不碰 MCP/plugin 审计门**,仅新增独立能力,降低丢失面。
- 历史 memory 称 workflow/team/Arena/synthetic-output 为"真缺口",但本次源码已确认 `workflow.rs` 实装 → 以本次磁盘证据为准,不重复实现 workflow。
- AgentBus 内存态为本次新确认事实(非 memory 旧结论),Phase 3 持久化是真实增量。
