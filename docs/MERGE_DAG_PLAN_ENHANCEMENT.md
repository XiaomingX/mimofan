# 合并总结：DAG 任务编排 + Plan 结构化维度

- **提交**：`79f8501`（`2074c35..79f8501`，已 push 到 `origin/main`）
- **日期**：2026-08-14
- **对标对象**：nac（arcee-ai 的 agent harness，竞品）

## 背景

对比 nac 与 mimofan 的核心能力，确认 mimofan 大部分能力已具备或领先（Skills、并行工具、压缩、双向 MCP、Plan 模式、会话回滚、AGENTS.md、HTTP API）。识别出两个 nac 已实现、mimofan 缺失（或仅半成品）的明确缺口，实施并合并到主干。

## 改动一：接线 DAG 任务编排调度器（之前是死代码）

mimofan 已有完整 DAG 算法 `crates/tui/src/tools/subagent/decomposer.rs`（拓扑排序、环检测、波次并行分组），但全仓库仅测试引用、运行时完全未使用。nac 有真正运行的 DAG 执行层（波次拓扑 + 失败传播）。

**新增 / 修改**：
- `crates/tui/src/tools/subagent/task_graph.rs`（新增）— `TaskGraphTool`（`run_task_graph` 工具），模型驱动分解（对齐 nac）：
  - 模型传入带 `depends_on` 的任务图 → 校验（环 / 重复 id / 缺失依赖）→ 波次并行执行。
  - 同一波内独立节点**并发** spawn；波次间**严格串行**（前波全完成后才进下一波）。
  - 失败传播：某节点失败 → 其下游依赖被标记为 `Skipped`，不再等待。
  - 大 DAG 触发 admission 容量拒绝时指数退避重试（最多 4 次）。
- `decomposer.rs` — 新增 `Skipped` 状态变体与 `skip_downstream()` 失败传播方法。
- `tool.rs` — `spawn_subagent_from_input` 提升为 `pub(crate)`，复用全部 spawn 细节（worktree / model routing / admission）。
- `mod.rs` / `registry.rs` — `pub mod task_graph;` 声明 + `with_subagent_tools` 注册 `TaskGraphTool`。

**关键设计**：复用现有 `parent_completion_tx` 完成通道等待波次完成（而非轮询 manager 或改 spawn handle）；不引入新进程边界，复用现有子代理调度。

## 改动二：增强 Plan 结构化维度（借鉴 nac workset）

原 `PlanStep` 仅有 `step/status` 扁平列表，且 `AcceptanceCriteria`/`verify_step` 是已写好但未接线的半成品（`update_plan` 恒传空 evidence，验收门形同虚设）。

**修改**（`crates/tui/src/tools/plan.rs`）：
- `PlanItemArg` / `PlanStep` / `PlanSnapshot` 增加 `depends_on` / `scope` / `role` / `acceptance` / `evidence` 字段（全 `Option` + `serde(default)`，向后兼容旧 `.plan.json`）。
- 四处透传：`update` / `snapshot` / `apply_snapshot` / `from_tool_input`。
- 抽出 `evaluate_step_gate()` 纯函数并接入 `update_plan`：completed 步骤若有 `acceptance`，按引号内短语做 required-substring 验收；失败仅标记 `verification_failed`，不阻断已完成状态。
- `input_schema` + 工具说明更新，让模型可传结构化维度。
- `crates/tui/src/tui/history/plan.rs` — TUI 渲染补充 `[role@scope]` 与 `✓gate` 标签。

## 验证

- `cargo build -p mimofan`：零新增 warning（仅预存在的 `with_parallel_tool` deprecated 提示，非本次改动）。
- `cargo clippy -p mimofan`：相关文件无警告。
- `cargo test -p mimofan --lib tools::`：**221 通过，0 失败**。
- 新增单测：
  - `skip_downstream` 传递性跳过 / 已完成下游不降级
  - `parallel_groups` 依赖排序（根单独一波，独立节点同波）
  - Plan 字段 `update` → `snapshot` → `apply_snapshot` 往返一致
  - 验收门：引号短语证据匹配 / 缺失 / 无 acceptance 自动通过
  - `extract_required_substrings` 引号短语提取

## 明确排除（非本次范围）

- DAG 跨 episode 持久化（nac 的 SQLite workset 持久；本轮先做执行层接线）
- 沙箱默认开启（属行为变更，单独评估）
- nac 的 Podman / SSH 远程执行（mimofan 已有多后端沙箱）
- Web 富仪表盘（mimofan 仅有移动控制页 + HTTP API）

## 后续建议

- 若要让 `run_task_graph` 被模型更主动使用，可在 agent 系统提示词里补充何时优先用 DAG 而非多个 `agent` 调用。
- DAG 持久化可作为下一轮独立 issue（复用现有 side-git snapshot 机制）。
