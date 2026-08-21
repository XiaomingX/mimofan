# Plan 14 — Harness 长程任务轨迹日志（Trajectory Log）保存

> 目标：让 mimofan 的 harness 在长程任务执行时保存**完整、可回放、可标注、可分析**的轨迹日志，
> 供标注平台的标注、失败/行为分析和模型优化（SFT/DPO/RL）使用。

## 背景与现状结论（Phase 0 调研结果）

### mimofan 现状：**没有生产路径落盘的轨迹日志**

经代码实证（非 issue 状态），mimofan 有**三套"轨迹类"数据结构，但全部处于"已实现、核心循环未接线"**状态：

| 机制 | 文件 | 事件模型 | 落盘位置 | 接线现状 |
|---|---|---|---|---|
| `SessionEventSink` | `crates/tui/src/core/engine/trace.rs` | `TurnStart/AssistantText/ToolCall/HypothesisOp/PocResult/TurnEnd` | `~/.mimofan/tasks/<task_id>/session.jsonl` | **零非测试调用点**（trace.rs:124-127 明言"不接线"；grep 仅 trace.rs 自测） |
| `EventLog`/`EventReplay` | `crates/tui/src/tools/event_stream.rs` | `TurnStart/ToolCall/ToolResult/AgentSpawn/AgentDone/Error/Checkpoint/Custom` | 调用方自定路径 | 唯一真实写入 = `headless_gate.rs:150` 的 `write_failure`（只写 `Error`） |
| `StateStore` JSONL transcript | `crates/state/src/lib.rs` | 对话消息级 `{ts,thread_id,role,content,item}` | `<state_dir>/rollouts/<thread_id>.jsonl` | 仅在线程初始化/恢复/fork（`core/src/thread.rs`）与 prompt 端点（`core/src/lib.rs:498`）写；**核心 `turn_loop.rs` 不写** |

**证据**：磁盘实测 `~/.mimofan/tasks/` 与 `~/.mimofan/rollouts/` 为空/不存在；全盘无 `session.jsonl`/`events.jsonl`。
**易误判点**：issue #838 虽标记 CLOSED（2026-08-15），但功能并未真正接线落地——任何"已支持轨迹日志"的论断必须以接线代码为准，非 issue 状态（呼应记忆 `feedback_verify_teammate_claims`）。

### 现有可用产物（工具级快照，非逐轮时序轨迹）

- vuln-hunt 工具各自持久化产物到 `<workspace>/.mimofan/`：`hypotheses.json`、`gadget_chain.json`、`run_poc.json`。
- `EvalHarness`（`crates/tui/src/eval/mod.rs:366-400`）在配置 `artifacts_dir`+`task_id` 时把它们转存到 `<artifacts_dir>/<task_id>/`，供 `benchmark/vuln_hunt/evaluate.py` 三维打分（Consistency/Trace/Reproduce）。
- 这些是**工具最终输出快照**，不含中间推理、每轮 tool 调用时序、失败中间态 → 不足以为标注/DPO/RL 提供轨迹。

### 结论

mimofan **没有**支持保存"harness 长程任务轨迹日志"。最接近、最该复用的是 `trace.rs` 的 `SessionEventSink`，但它需要**接线**到 `turn_loop.rs` 与 `EvalHarness`。

---

## 行业最佳实践（Phase 0 调研结果）

### 主流轨迹格式

1. **ATIF v1.7（Agent Trajectory Interchange Format，JSONL）**——已被 Arize Phoenix、阿里云 AgentLoop、Harbor、AutoAgent 采用，事实准标准。字段设计：
   - `source: system|user|agent`、`message`、`reasoning_content`（thought）、
   - `tool_calls[]: {tool_call_id, function_name, arguments}`、
   - `observation: {results[]: {source_call_id, content}}`（用 **`source_call_id ↔ tool_call_id` 配对**，不依赖顺序，多工具并行不错位）、
   - `metrics: {prompt_tokens, completion_tokens, cost_usd}`、`final_metrics` 汇总。
2. **SWE-agent `.traj`（编码助手最相关参考）**：`trajectory[] + history[] + info`。每 step：`{action, observation, response, thought, execution_time, state, query, extra_info}`；`info` 含 **`exit_status` 枚举**（submitted/exit_cost/exit_format/exit_api…）与 `model_stats`——**记录"为什么结束"**。
3. **ShareGPT（训练出口）**：扁平 `[{from: system|human|gpt|tool, value}]`。Hermes 把推理统一成 `<think>` 标签、工具调用统一成 `<tool_call>/<tool_response>`，直接喂 HuggingFace SFT/DPO/RL。

### 事件粒度惯例（两种心智模型）

- **Step 化**（ATIF/SWE-agent/OTel）：一个 step 同时承载 消息 + 推理 + 工具调用 + 观测 + metrics。
- **扁平对话**（ShareGPT）：`[role, content]` 序列，训练友好。
- **一次会话 = 一条 trace = JSONL 一行**：`{trajectory_id, session_id, timestamp, model, completed} + step[]`。一个标注单元 = 一行。

### 关键工程决策与陷阱

| 主题 | 最佳实践 |
|---|---|
| 序列化 | **JSONL**（流式追加/增量续写/易 diff/checkpoint-resume）；JSON 只用于单条轨迹；Parquet 留给大规模分析阶段。 |
| 脱敏（PII） | **写入 trace 前**做；五策略按字段选 `mask/tokenize/hash/drop/encrypt`，agent 任务 `tokenize` 最有用（保留实体一致性）。保存 `input_hash` 而非完整输入；工具输出做**字段最小化**；不要事后补救。 |
| 工具输出过大 | 写入时设字节/行数上限 + 保留截断标记（OTel/OpenInference 对超大 IO 懒加载/外部化）。 |
| 禁止伪造 ID | session/trace ID 取不到就留空，别编造 UUID（OTel 原则）。 |
| 失败轨迹 | 单独归档（Hermes：成功 `trajectory_samples.jsonl` / 失败 `failed_trajectories.jsonl`），失败轨迹正好作 DPO rejected 样本。 |
| 未闭合推理标签 | 训练前过滤，防止模型学到"推理可半途而废"。 |
| schema 一致 | 喂 HuggingFace（转 Arrow/Parquet）前字段归一化（Hermes 用 `tool_stats` 零填充）。 |
| 标注元数据 | trace identifier、session timestamp、model version、参考材料（策略文档）帮标注者判断。 |
| 标注控件 | 逐 turn pass/fail + 多选失败分类 + severity + free-text 解释；多标注者取 Cohen's kappa。 |

---

## 设计决策

1. **主格式：JSONL**（对齐 ATIF step 化结构 + SWE-agent `exit_status` + mimofan 现有 `SessionEvent` 事件模型）。
2. **复用 `SessionEvent`/`SessionEventSink` 为地基**，扩展事件种类与字段以覆盖通用长程任务（不只 vuln-hunt），并**接线**到核心循环。
3. **落盘位置**：`~/.mimofan/tasks/<task_id>/`（保持 `SessionEventSink` 既定路径，兼容 README 所述）；`EvalHarness` 另把轨迹复制到 `artifacts_dir/<task_id>/trajectory.jsonl` 随产物一起被 `evaluate.py`/分析消费。
4. **标注/分析出口**：提供一次性转换器 `trajectory.jsonl → ShareGPT.jsonl`（训练）与 ATIF 兼容视图（分析/标注），不做运行时双写，避免格式漂移。
5. **脱敏前置**：在 `emit` 层统一做（`input_hash` + 工具输出上限 + 截断标记），不侵入业务逻辑。

---

## Phase 1：扩展 `SessionEvent` 模型并接线核心循环

### 1.1 扩展事件模型（`crates/tui/src/core/engine/trace.rs`）

**做什么**：把 `SessionEventKind` 从 vuln-hunt 专用扩展为通用长程任务轨迹事件，并补充字段：
- 新增 kinds：`ToolResult`（工具观测/输出）、`AgentSpawn`/`AgentDone`（子 agent）、`Error`、`SessionEnd`（含 `exit_status` 枚举，借鉴 SWE-agent）。
- `SessionEvent` 增字段：`source: system|user|agent`、`reasoning/thought`、`tool_call_id`、`tool_result`（截断后输出 + 是否截断标记）、`metrics {prompt_tokens, completion_tokens, cost_usd}`（可选）、`session_id`、`model`。
- **兼容**：`#[serde(default, skip_serializing_if)]` 保证旧记录可读、新字段可选。

**文档参考**：
- `crates/tui/src/core/engine/trace.rs:75-119`（现有 `SessionEventKind`/`SessionEvent`）
- `crates/tui/src/tools/event_stream.rs:27-63`（`EventKind`/`EventEnvelope` 的 `ToolResult/AgentSpawn/Error` 事件种类与 `ts+seq` 结构，可直接借鉴）
- SWE-agent `exit_status` 枚举：`submitted/exit_command/exit_cost/exit_format/exit_api/exit_error`

**验证**：`SessionEvent` 新字段 round-trip 单测；旧 `.jsonl` 用 `read_session` 读不 panic（缺字段走默认）。

**反模式护栏**：不要新增不存在于这些来源的"发明字段"；保持 append-only 不可变（trace.rs:119 护栏）。

### 1.2 接线 `turn_loop.rs`（核心循环）

**做什么**：在 `crates/tui/src/core/engine/turn_loop.rs` 的既有拦截点调用 `SessionEventSink::emit`。已确认的拦截点：
- turn 开始 → `TurnStart`
- 每轮 tool 调用 → `ToolCall`（`tool_name` + `tool_input`）
- tool 返回后 → `ToolResult`（输出截断）
- `should_stop_after_plan_tool`（turn_loop.rs:2552）→ 停止原因记录
- `rx_steer`（turn_loop.rs:218/821）→ 转向/干预事件
- 会话结束 → `SessionEnd`（`exit_status`）

**实现方式**：`turn_loop` 结构体增加一个 `Option<SessionEventSink>` 字段 + `task_id`，由外部（headless/CLI/harness）注入；`None` 时不写，**零行为变化**（默认关闭，避免影响现有性能/行为——呼应 trace.rs:126 "zero-behavior-change" 原则）。emit 为 best-effort，I/O 失败只 `debug!` 不 panic。

**文档参考**：
- `crates/tui/src/core/engine/turn_loop.rs:218,2552,821`（拦截点）
- `crates/tui/src/core/engine/trace.rs:128-174`（`SessionEventSink::open/emit/path`）
- `plans/01-modular-harness.md:113`（原计划指明的接线点：`should_stop_after_plan_tool`、`rx_steer`、`run_poc`/`hypothesis` 调用处）

**验证**：注入 sink 跑一段含工具调用的会话，断言 `session.jsonl` 出现 `TurnStart/ToolCall/ToolResult/SessionEnd` 事件，且 `read_session` 可回放为有序序列。

**反模式护栏**：不要把 UI 实时事件与领域轨迹混用（trace.rs:119 / plans/01:119）；不得在无 sink 时引入写盘开销。

---

## Phase 2：接线 `EvalHarness` 产生任务轨迹

**做什么**：`EvalHarness`（`crates/tui/src/eval/mod.rs`）作为 W4 harness 驱动，在 `run_async` 的 while 循环内（eval/mod.rs:246-313）打开 `SessionEventSink(task_id)` 并 emit：
- 每轮 mock turn → `TurnStart`
- 每次 `registry.execute_full`（eval/mod.rs:257）→ `ToolCall` + `ToolResult`（含截断输出）
- 循环结束 → `SessionEnd`（含 success/exit_status）

并把 `session.jsonl`（从 `~/.mimofan/tasks/<task_id>/`）复制到 `<artifacts_dir>/<task_id>/trajectory.jsonl`，与 `hypotheses.json`/`gadget_chain.json`/`run_poc.json` 并列。

**文档参考**：
- `crates/tui/src/eval/mod.rs:246-313`（while 循环、execute_full）、`366-400`（persist_tool_artifact/write_artifact 模式）
- `crates/tui/src/eval/mod.rs:137-150`（`EvalHarnessConfig` 现有 `artifacts_dir`/`task_id` 字段，直接复用）

**验证**：新增测试（仿 eval/mod.rs:738-761 `harness_persists_vulnhunt_artifacts`）：配置 `artifacts_dir`+`task_id` 后 `run()`，断言 `<artifacts_dir>/<task_id>/trajectory.jsonl` 存在且含 `ToolCall`/`ToolResult`/`SessionEnd` 事件。

**反模式护栏**：artifact 写失败必须 best-effort（`let _ =`，同 eval/mod.rs:277）；不得改变 `metrics.success` 现有语义。

---

## Phase 3：Python 分析/标注适配层（`benchmark/vuln_hunt/`）

**做什么**：新增一个只读转换器脚本 `benchmark/vuln_hunt/trajectory_export.py`，把 `trajectory.jsonl` 转成两种出口（不做运行时双写）：
1. **ShareGPT 训练出口** `--export sharegpt`：`[{from, value}]`，推理归一成 `<think>…</think>`、工具调用归一成 `<tool_call>/<tool_response>`；system prompt 不落盘（防泄漏）。输出 `trajectory_samples.jsonl`（成功）与 `failed_trajectories.jsonl`（失败/`exit_status` 非成功，作 DPO rejected）。
2. **ATIF 兼容视图** `--export atif`：重组为 `{trajectory_id, session_id, model, completed, steps[]}`，每 step 用 `source_call_id ↔ tool_call_id` 配对；供 Phoenix/标注平台导入。

**文档参考**：
- 现有 verifier：`benchmark/vuln_hunt/evaluate.py`（读 `~/.mimofan/tasks/<id>/` 产物、写 `results/<task_id>.json` 的模式）
- Hermes `_convert_to_trajectory_format`（`<think>/<tool_call>/<tool_response>` 归一）
- ATIF step 结构（`source`/`tool_calls`/`observation.results[].source_call_id`）

**验证**：
- 喂入 Phase 2 产生的真实 `trajectory.jsonl`，`--export sharegpt` 输出行 `from ∈ {system,human,gpt,tool}` 且每轮 gpt 后必跟 tool 或结束；`--export atif` 的每条 `tool_call_id` 都能在 `observation.results[].source_call_id` 找到配对。
- 失败任务归入 `failed_trajectories.jsonl`。

**反模式护栏**：不得在转换中新增字段或改变事实；system prompt 一律过滤；工具输出超出阈值时保留截断标记。

---

## Phase 4：脱敏与写入上限（写入前）

**做什么**：在 `SessionEventSink::emit`（或 Phase 1 的接线层）统一做：
- 工具输入/输出超过阈值（如 `max_chars`）时截断 + 标记 `truncated: true`（复用 `eval/mod.rs:683-690 truncate_output` 思路）。
- 可选：`input_hash`（sha256）替代完整敏感输入（默认关闭，`emit` 层配置开关）。
- I/O 错误 best-effort：`debug!` 记录，不 panic、不污染结果。

**文档参考**：
- `crates/tui/src/eval/mod.rs:683-690`（truncate_output）
- OTel GenAI PII opt-in 原则、PII 五策略（mask/tokenize/hash/drop/encrypt）

**验证**：注入超长工具输出，断言落盘行含截断标记且总字节受限。

**反模式护栏**：不要为所有路径强制开启（默认不改变行为）；脱敏只做"写入前"，不改动工具真实返回值。

---

## Final Phase：整体验证

1. `cargo build`（workspace 零 warning）通过。
2. 全 workspace `cargo test` 零失败（新增 round-trip + harness 轨迹 + 截断测试）。
3. 端到端：配置 `artifacts_dir`+`task_id` 跑 `EvalHarness::run` → 断言 `trajectory.jsonl` 生成 → `python3 benchmark/vuln_hunt/trajectory_export.py --export sharegpt` 与 `--export atif` 均成功且字段契约成立。
4. 反模式 grep：确认无新增"发明字段"、无 UI 事件与轨迹混用、无未做截断的整段工具输出落盘。

---

## 涉及文件清单（改互不相交，便于并行）

| 阶段 | 文件 | 变更 |
|---|---|---|
| P1 | `crates/tui/src/core/engine/trace.rs` | 扩展 `SessionEventKind`/`SessionEvent` 字段 + round-trip 测试 |
| P1 | `crates/tui/src/core/engine/turn_loop.rs` | 注入 `Option<SessionEventSink>`，在拦截点 emit |
| P2 | `crates/tui/src/eval/mod.rs` | `EvalHarness` 开 sink、emit、复制 `trajectory.jsonl` + 测试 |
| P3 | `benchmark/vuln_hunt/trajectory_export.py`（新增） | ShareGPT / ATIF 两个只读出口 |
| P4 | `crates/tui/src/core/engine/trace.rs`（或接线层） | 截断 + `input_hash`（可选） |

## 开放问题（需确认）

1. **开关方式**：轨迹日志默认关闭、由 harness/CLI flag 显式开启（推荐，零行为变化），还是要全局默认开？——建议默认关。
2. **task_id 来源**：`turn_loop.rs` 生产路径的 task_id 从哪来？headless 已有 `HeadlessGateConfig`（`headless_gate.rs`）；CLI/交互态是否也要落盘？
3. **脱敏粒度**：`input_hash` 是否本期做，还是先只做截断（推荐先截断，hash 下期）。
4. **训练出口优先级**：ShareGPT 出口是否本期交付，还是先只做 ATIF 分析视图？（建议 ATIF 先行，ShareGPT 作为训练扩展）

---

## 实施状态（2026-08-21）

已实施并验证通过（`cargo build -p mimofan` 零错误；`session_event`/`eval`/`turn_loop` 测试全过；Python `trajectory_export.py --selftest` PASS）。

### 实际改动

| 阶段 | 文件 | 变更 |
|---|---|---|
| P1.1 | `crates/tui/src/core/engine/trace.rs` | 扩展 `SessionEventKind`（+`ToolResult/AgentSpawn/AgentDone/Error/SessionEnd`）、`SessionEvent`（+`source/tool_result/tool_call_id/session_id/model/exit_status/truncated`，全 `skip_serializing_if` 向后兼容）；新增 `SessionEventSink::open_at(path)`（写任意路径）、`Debug` impl、`MAX_TOOL_OUTPUT_CHARS`（16KiB）截断；新增 4 个测试 |
| P1.2 | `crates/tui/src/core/engine/turn_loop.rs` | `handle_deepseek_turn` 在 `TurnContext.session_sink_path` 接线：TurnStart/工具执行前 ToolCall/工具执行后 ToolResult/SessionEnd（最佳路径选在工具执行**前**发 ToolCall）；新增 `now_ts()`；全部 `let _ =` best-effort |
| P1.2 | `crates/tui/src/core/turn.rs` | `TurnContext` 新增 `session_sink_path: Option<PathBuf>`（默认 None，零行为变化） |
| P1.2 | `crates/tui/src/core/engine.rs` | `mod trace` → `pub(crate) mod trace`（让 eval 等 crate 内模块可 emit/replay） |
| P2 | `crates/tui/src/eval/mod.rs` | `EvalHarnessConfig.trajectory_dir`；`run_async` 用 `SessionEventSink::open_at` 写 `<trajectory_dir>/<task_id>/trajectory.jsonl`，emit TurnStart/ToolCall/ToolResult/SessionEnd（统一 `SessionEvent` 格式）；SessionEnd 的 `exit_status` 按 `tool_errors` 判 completed/failed；新增 `now_ts()` + 测试 `harness_persists_session_trajectory` |
| P3 | `benchmark/vuln_hunt/trajectory_export.py`（新增） | ShareGPT 出口（`--user-prompt` 注入 human 消息，失败轨迹路由到 `failed_trajectories.jsonl`）；ATIF 出口（tool_call_id ↔ source_call_id 配对）；`--selftest` |
| P4 | `crates/tui/src/core/engine/trace.rs` | `emit` 内统一截断 `tool_result.content`（>16KiB 截断并置 `truncated: true`，写入前不改动调用方事件）；2 个截断测试 |

### 排查后修复的隐患

1. **ToolCall 位置**：原 emit 在工具执行**后**（语义错），已移到执行前。
2. **eval exit_status**：原恒 "completed"，已按 `tool_errors` 判 completed/failed。
3. **ShareGPT 缺 human 消息**：训练样本残缺，已加 `--user-prompt`（+ 占位 human + `meta.user_prompt_injected`）。
4. **README 过时**：原称"turn_loop 未接线"，已更新为"已接线但默认关闭 + EvalHarness 是现成入口 + 导出说明"。

### 开放问题处理

- 开关方式：**默认关闭**（`session_sink_path = None` 即不写），EvalHarness 通过 `trajectory_dir` 显式开启——采用建议方案。
- task_id 来源：生产路径默认不落盘；真实 harness 任务启用需设置 `session_sink_path`（文档已说明，待 EngineConfig flag 设计）。
- 脱敏粒度：**本期只做截断**（input_hash 下期）。
- 训练出口：**ShareGPT + ATIF 本期均交付**（原建议 ATIF 先行，实际一并交付）。
