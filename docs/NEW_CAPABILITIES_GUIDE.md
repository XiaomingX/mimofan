# 新增能力使用教程

本教程介绍 mimofan 最近新增的三项能力，以及它们的使用方法与配置方式：

1. [`/evolve`](#一-evolve-可机评优化回路) —— 可机评优化回路（issue #751）
2. [`/repro`](#二-repro-可复现性纪律) —— 可复现性纪律（issue #754）
3. [GitHub 共同作者署名](#三-github-共同作者署名-co-authored-by-mimofan) —— `git_commit` 自动追加 mimofan 署名

> 这三项能力均随 `main` 分支合入（commit `626feb1` 起）。若你的版本早于该提交，请先升级 mimofan。

---

## 一、`/evolve` 可机评优化回路

### 能力简介

`/evolve` 对标 open-discovery 的 program-evolution 范式，把「优化一个程序」变成一场**可机评**的回路：

- 你给出 **目标（goal）**、**baseline 程序** 和 **evaluator 脚本**；
- **evaluator 拥有正确性裁决权**，AI（mimofan）只负责提出候选改动，不自己报分；
- 外部 evaluator 程序逐个裁决候选是否「正确且更优」，胜出的候选才留痕并作为下一轮父本。

核心思想：代理不自我评价，正确性由你提供的脚本说了算，避免「AI 自己说变好了」的不可信结论。

### 使用方法

```
/evolve <goal>
```

别名：`/evolve`、`/optimize`。

示例：

```
/evolve 降低 tokenizer 编码延迟
```

执行后 mimofan 会：

1. 进入 **Agent 模式**（模型可读写文件、调用工具）；
2. 提示你确定 baseline 程序路径与 evaluator 脚本路径；
3. 调用 `crate::evolve` 逻辑层完成回路，工作流如下：

| 步骤 | 调用 | 说明 |
|------|------|------|
| 1 | `evolve::lock_baseline(baseline, evaluator, goal, out)` | 拷贝 baseline + evaluator 到隔离目录、计算哈希、首次求值，写入 `lock.json`。**已锁定则拒绝覆盖**，保证可比性 |
| 2 | 写候选改动到 `evolution/candidates/<id>/` | 受 evaluator 约束的候选实现 |
| 3 | `evolve::run_evaluator_on(evaluator, candidate)` | 由 evaluator 裁决；用 `EvaluatorOutput::is_winner()` 判定 `valid && improved` |
| 4 | `evolve::record_candidate(evolution_dir, lineage)` | 胜出者留痕（含 `parent_id` / `patch_summary`），并作为下一轮父本 |
| 5 | 重复 2–4 | 直到预算用尽或不再有改进 |

### evaluator 契约

evaluator 脚本最后一行为一个 JSON 对象（前面可打印日志），mimofan 解析如下字段：

```json
{
  "valid": true,
  "improved": false,
  "objective": { "name": "speedup", "value": 1.24, "baseline_value": 1.0, "direction": "maximize" },
  "metrics": {},
  "failures": []
}
```

- `valid`：候选是否通过正确性门（evaluator 判定，**非** AI 自述）。
- `improved`：在 `valid` 前提下，是否优于 baseline。
- `objective`：可选的目标度量，`improvement_ratio()` 会按 `direction`（`maximize`/`minimize`）归一化。

只有 `valid && improved` 的候选才算「winner」进入下一步。

### 配置方法

`/evolve` 目前为**命令层编排**，没有独立开关。其行为由 evaluator 脚本决定，无需额外配置。相关纯逻辑层（`crates/tui/src/evolve/`）自带单元测试，可在 CI 中验收。

> 提示：`lock_baseline` 拒绝覆盖已有 `lock.json`，且会校验锁定后 baseline/evaluator 未被改写（哈希比对）。这是为了确保每轮优化都在同一基准上可比，请勿手动改动锁定目录里的副本。

---

## 二、`/repro` 可复现性纪律

### 能力简介

`/repro` 对标 open-discovery 的可复现性范式，把一次研究/开发的意图与证据**固化下来**，让多轮迭代后原始目标不漂移、结果可被他人复现：

- `BRIEF.md` 是研究的**唯一事实源**（single source of truth）；
- `provenance.jsonl` 记录每条结论/代码的来源（回合、模型、读写文件、父候选）；
- `env_snapshot.json` 抓取环境快照（rust/python 版本 + 依赖锁哈希）。

该命令**默认只落盘证据、不触发任何行为变更**，对正在运行的会话无副作用。

### 使用方法

```
/repro <brief>
```

别名：`/repro`、`/reproducibility`、`/brief`。

示例：

```
/repro 复现 paper-X 的 method-Y 并对比 baseline
```

执行后 mimofan 会在**当前工作目录**下生成：

```
<工作区>/
└── repro/
    ├── BRIEF.md            # 你的 brief 原文，作为唯一事实源
    ├── provenance.jsonl    # 起点留痕（turn-0）
    └── env_snapshot.json   # rust/python 版本 + Cargo.lock 哈希前缀
```

- `BRIEF.md` 首行是 `# Research Brief` 标题，其后为你的 brief 原文；
- `env_snapshot.json` 尽力而为抓取（任一命令失败对应字段为 `None`，不整体失败）；
- `provenance.jsonl` 起点记录由 `/repro` 本身写入（`turn-0`），运行中的回路（goal_loop / evolve）后续会按需累积更多 provenance 记录。

### 配置方法

`/repro` 没有独立开关，开箱即用。如果你想让运行中的回路自动累积 provenance，由对应回路（goal_loop / evolve）负责调用 `repro::record_provenance`，无需用户配置。

---

## 三、GitHub 共同作者署名（`Co-Authored-By: mimofan`）

### 能力简介

当你用 mimofan 通过 `git_commit` 工具提交代码时，mimofan 会**默认在 commit message 末尾追加 mimofan 的共同作者署名**：

```
🤖 Generated with [mimofan](https://github.com/XiaomingX/mimofan)

Co-Authored-By: mimofan <noreply@xiaoming.com>
```

这与 Claude Code（`includeCoAuthoredBy`）、CodeBuddy 的行为一致：

- GitHub 会把 `Co-Authored-By` 里的人/工具显示为**共同作者（co-author）**，出现在提交详情与仓库贡献者列表中；
- **真实 committer 不变**——committer 仍是运行 mimofan 的你的 git 身份，署名只是额外标注「本提交有 mimofan 协助」。

> 为什么你看到 Claude Code / CodeBuddy 的提交「显示提交者是工具」？正是这个 trailer 的效果，而非 committer 被改写。可用各自工具的开关关闭。

### 使用方法

正常情况下**无需任何操作**——`git_commit` 默认开启署名。提交后 `git log` 会看到消息末尾带上述 trailer。

### 配置方法

`git_commit` 新增了一个布尔参数 `co_authored_by`，**默认 `true`**：

```json
{
  "message": "feat(tools): 添加某能力",
  "co_authored_by": true
}
```

- `co_authored_by: true`（默认）：追加 mimofan 署名；
- `co_authored_by: false`：不追加，提交消息保持你给的原样。

模型在调用 `git_commit` 时可按需要传该参数；例如公司规范要求提交者必须是真实员工账号时，传 `false` 即可。

### 防重复机制

若消息中**已包含** `Co-Authored-By: mimofan`，mimofan 不会重复追加。这意味着对同一条提交做 `amend` 也不会让 trailer 叠加。

---

## 四、小结

| 能力 | 命令 / 参数 | 默认行为 | 如何关闭 |
|------|-------------|----------|----------|
| 可机评优化回路 | `/evolve <goal>` | 进入 Agent 模式跑回路 | 不调用即可 |
| 可复现性纪律 | `/repro <brief>` | 落盘 BRIEF + 环境快照 + provenance | 不调用即可 |
| GitHub 共同作者署名 | `git_commit` 的 `co_authored_by` | `true`（追加 mimofan 署名） | 传 `false` |

三项能力均已在 `main` 合入并通过单元测试，无需额外安装步骤，升级 mimofan 后即可用。
