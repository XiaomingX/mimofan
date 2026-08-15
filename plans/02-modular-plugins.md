# 计划：插件化能力架构（modular plugins，MECE + 奥卡姆剃刀）

> 本计划是 `plans/01-modular-harness.md`（issue #834–#840）的**细化执行版**。目标：把 mimofan 拆分为可被生态自由排列组合的**插件能力**，整体更符合 MECE（每条能力是一个正交轴）与奥卡姆剃刀（最小必要抽象、manifest 驱动组合、不重写现有逻辑）。
>
> 执行模型（用户要求）：**多 agent 并行**做各 workstream（每个 workstream 只改**互不相交的文件集合**，零编辑冲突）；全部完成后**统一在合并阶段做编译验证**（`cargo build` + `cargo test`），而非每步都编。
>
> 调研证据（Phase 0，本轮已抽取精确签名）：见各 workstream 的「Allowed APIs」。deepseek-harness 经核实**无 verifier 层**，其价值在于「能力即 seam（Cordis 插件运行时）」+ 可重放 `SessionEvent` + per-task 沙箱 —— 我们取其**插件化思想**，用 Rust 静态环境的「manifest 驱动 + trait 注入」达到同等松耦合，避免 Cordis 式运行时热挂载的复杂度（奥卡姆）。

---

## 核心架构决策（MECE + Occam）

**一条原则：能力 = 一个可组合的 seam trait；插件层 = 一个 manifest 声明要加载哪些 provider。**

mimofan 已天然具备 4 个 seam 种子（抽取确认）：`ToolSpec`、`SandboxBackend`、`LlmClient`、`SessionEvent`(待加)。我们引入一个**薄 glue 层** `crates/tui/src/plugins/`，包含：

1. `PluginManifest`（TOML/YAML）：声明启用的能力 provider，例如：
   ```toml
   [tools] extra = ["vuln_hunt"]          # 从 extra_tools 钩子注入
   [sandbox] backend = "per_task"         # 走 sandbox_for(workspace) seam
   [llm] provider = "mock" | "api"        # 选 LlmClient 实现
   [trace] session_events = true          # 启用 SessionEvent 落盘
   ```
2. `PluginRegistry::assemble(manifest) -> AssembledCapabilities { tools: Vec<Arc<dyn ToolSpec>>, sandbox: Option<Arc<dyn SandboxBackend>>, llm: Arc<dyn LlmClient>, trace_sink: Option<SessionEventSink> }` —— 唯一把各 seam 拼起来的地方（单一事实源，避免散落）。

**MECE 轴**：
- **A. 工具插件 seam**（#834）— 外部能力可注入，不经 builder 源码。
- **B. 沙箱插件 seam**（#835）— per-task 选 backend，统一两套沙箱。
- **C. 循环拦截 seam**（#836）— turn_loop 抽拦截器，供 eval/扩展。
- **D. LLM 插件 seam**（#837）— LlmProvider trait + MockLlmClient，eval 可无网驱动。
- **E. 会话事件 + 验收**（#838/#839）— SessionEvent 落盘 + vuln-hunt verifier benchmark。

**奥卡姆护栏**：不做运行时 hot-swap（Cordis 式）；不重写 turn_loop 为全新架构（先包裹）；manifest 只做「选 provider」，不做「解释执行 DSL」。

---

## 并行 workstream 划分（文件互不相交，零冲突）

| Workstream | 负责轴 | 仅改动的文件（互不相交） | 依赖 |
|-----------|-------|------------------------|------|
| **W1 工具插件** | A | `tools/registry.rs`(加 `with_extra_tools`)、`core/engine/tool_setup.rs`(钩子)、`tools/mod.rs`(声明)、**新增** `plugins/manifest.rs`+`plugins/registry.rs` | 无 |
| **W2 沙箱插件** | B | `sandbox/mod.rs`、`sandbox/backend.rs`、`core/engine/engine.rs`(:2648 注入点) | 无 |
| **W3 循环拦截** | C | **新增** `core/engine/interceptor.rs`、`core/engine/turn_loop.rs`(包裹现有点) | 无 |
| **W4 LLM 插件** | D | **新增** `llm_client/mock.rs`、`eval/mod.rs`(改 `run` 驱真实 Engine) | 无 |
| **W5 事件+验收** | E | `core/engine/trace.rs`(加 `SessionEvent`)、**新增** `benchmark/vuln_hunt/` | W3（事件在拦截点 emit，但可先加类型后接） |

> 注：W3 与 W5 在 `turn_loop.rs`/`trace.rs` 有轻微重合，但 W3 只动「拦截器 trait + 包裹调用」、W5 只动「事件类型定义 + 在已有调用处 emit」，且 `SessionEvent` 是新类型不冲突。若并行仍担心，W5 的 `trace.rs` 事件**类型定义**可先独立提交，emit 接线随 W3 一起。

---

## W1 — 工具插件 seam（#834）

**What to implement（复制现有模式，不重写）：**
- 在 `crates/tui/src/tools/registry.rs` 增加 `pub fn with_extra_tools(self, extra: Vec<Arc<dyn ToolSpec>>) -> Self { let mut s=self; for t in extra { s=s.with_tool(t); } s }`（仿 `with_tool` 模式，registry.rs:486）。
- 在 `crates/tui/src/core/engine/tool_setup.rs:97` 之后，调用 `PluginRegistry::assemble` 拿到的 `extra` 工具并经 `with_extra_tools` 注入（插入点已确认 line 91-97）。
- 新增 `crates/tui/src/plugins/manifest.rs`：`PluginManifest` 结构体（serde，TOML/YAML）+ `from_file`/`from_defaults`；`plugins/registry.rs`：`PluginRegistry::assemble(manifest) -> AssembledCapabilities`。
- `tools/mod.rs` 加 `pub mod plugins;`（注意：若 W1 独占 `plugins/` 目录声明，其他 workstream 不碰 `mod.rs` 的 plugins 行，避免冲突）。

**Allowed APIs（已抽取）：** `ToolRegistryBuilder::with_tool`(registry.rs:486)、`with_call_graph_tool`(592)/`with_hypothesis_tools`(603)/`with_run_poc_tools`(624) 作模板、`ToolSpec` trait(spec.rs:1322)、`ToolContext::new`(spec.rs:567)、`build_turn_tool_registry_builder`(tool_setup.rs:55)、`extra_tools` 钩子插入点(tool_setup.rs:97)。

**Verification checklist：**
- 单元：构造 `extra_tools` 返回 1 个自定义 `ToolSpec`，断言 `assemble` 后的 `ToolRegistry.names()` 含其名（用 `registry.names()` registry.rs:80，非不存在的 `list_tools`）。
- `grep -L "with_extra_tools" crates/tui/src/tools/registry.rs` 应命中（方法存在）。
- 不破坏 `with_xxx_tools()` 链式调用。

**Anti-pattern guards：** 不要改为反射/动态发现；不要删现有 builder；不要碰 `McpToolAdapter`(registry.rs:1243) 已有逻辑。

---

## W2 — 沙箱插件 seam（#835）

**What to implement：**
- 在 `Engine`（`core/engine/engine.rs`）增加可覆盖方法 `pub fn sandbox_for(&self, workspace: &str) -> Option<Arc<dyn SandboxBackend>> { self.sandbox_backend.clone() }`（字段 engine.rs:368，注入点 engine.rs:2648 已确认）。
- `tool_setup.rs` 构造 `ToolContext` 时把 `engine.sandbox_for(workspace)` 结果经 `ToolContext::with_sandbox_backend`(spec.rs:766) 注入 —— 复用现有 `engine.rs:2648` 逻辑，只是来源改为可覆盖方法。
- 把 `SandboxManager`（`sandbox/mod.rs:293`）收敛为 `SandboxBackend` 的一个实现：新增 `impl SandboxBackend for SandboxManager`（在 `sandbox/mod.rs` 内），`exec` 内部转调 `prepare`(mod.rs:359)+本地执行，消除「两套并存」但**保留 OS 级行为**。
- `PluginManifest [sandbox] backend` 可选 `per_task`(走 `sandbox_for`) / `container` / `none`。

**Allowed APIs：** `SandboxBackend::exec`(backend.rs:75)、`SandboxOutput`(backend.rs:15)、`create_backend`(backend.rs:90)、`OpenSandboxBackend`(opensandbox.rs:42)、`ContainerBackend`(container.rs)、`ToolContext::with_sandbox_backend`(spec.rs:766)、`Engine` 字段 engine.rs:368、`exec_shell` 路由 shell_tools.rs:231。

**Verification checklist：**
- 单元：构造 fake `SandboxBackend`，`engine.sandbox_for` 返回它，`run_poc`(经 `sandbox_backend`) 断言 `exec` 被调用且 `realized` 正确（复用 `tools::run_poc` 测试，确认仍绿）。
- `cargo test -p mimofan --lib tools::run_poc` 绿。

**Anti-pattern guards：** 禁止 `Command::new` 直连本地 shell（必经 `SandboxBackend`）；不要破坏 `SandboxManager` 现有 OS 级路径；`impl SandboxBackend for SandboxManager` 不要复制逻辑，转调 `prepare`。

---

## W3 — 循环拦截 seam（#836）

**What to implement（包裹，不重写）：**
- 新增 `crates/tui/src/core/engine/interceptor.rs`：定义 `pub trait TurnInterceptor { fn pre_step(&self, _ctx:&EngineContext)->() {} fn request(&self, _req:&mut GenerateOptions) {} fn post_step(&self, _ev:&StepEvent) {} fn turn_stopping(&self, _state:&TurnState)->Option<bool> {} }`（默认空实现，调用方在现有点调用）。
- 在 `turn_loop.rs` 现有拦截点**包裹**调用（不改逻辑体）：
  - `rx_steer` 处理处(turn_loop.rs:210/:750) 前后调 `interceptor.pre_step`；
  - 构建请求处调 `interceptor.request`；
  - `should_stop_after_plan_tool`(turn_loop.rs:2471) 调用前调 `interceptor.turn_stopping` 作为额外 stop 决策（OR 语义）。
- `Engine` 持有 `interceptors: Vec<Box<dyn TurnInterceptor>>`，`Engine::new` 可注入（从 `PluginManifest` 加载）。

**Allowed APIs：** `impl Engine`(turn_loop.rs:65)、主 `loop`(203/727)、`self_heal_hint`(turn_loop.rs:56)、`rx_steer`(210/750)、`should_stop_after_plan_tool`(2471)、`Engine::new`(engine.rs:614)。

**Verification checklist：**
- 现有 `cargo test` 全绿（包裹不改变行为）。
- 单测：实现一个 `TurnInterceptor` 令 `turn_stopping` 返回 `Some(true)`，断言该 turn 提前停止（用 headless/eval 驱动或最小 Engine 测试）。
- grep 确认 `interceptor.` 调用出现在 turn_loop.rs 的 3 个包裹点。

**Anti-pattern guards：** 不要重写为全新架构；不要动 `self_heal_hint` 内部逻辑；默认实现必须空（不强制拦截）。

---

## W4 — LLM 插件 seam（#837）

**What to implement：**
- 新增 `crates/tui/src/llm_client/mock.rs`：`pub struct MockLlmClient { ... }` 实现 `LlmClient` trait（`llm_client/mod.rs:64`，方法 `create_message_stream(&self, request: MessageRequest) -> Result<StreamEventBox>` mod.rs:80）。支持 `push_message_response(Vec<StreamEventBox>)`（record/replay）；canned `StreamEventBox` 含 `text-delta`/`tool-call-delta`/`finish`（对齐 deepseek `StreamChunk` 语义）。
- 定义 `trait LlmProvider { fn client(&self) -> Arc<dyn LlmClient>; }`（薄抽象，`ApiClient` 实现之）。
- 改 `EvalHarness::run`(`eval/mod.rs:161`)：不再直接调 `list_dir/read_file/...`(173-237)，改构造真实 `Engine` + `ToolRegistry`（经 W1 的 `extra_tools` 注入 hypothesis/gadget_chain/run_poc），用 `MockLlmClient` 喂任务 prompt，跑完整 agent loop；保留 `EvalMetrics`(80-86) 与 `validate_outputs`(530)。

**Allowed APIs：** `LlmClient` trait(`llm_client/mod.rs:64`)、`create_message_stream`(mod.rs:80)、`StreamEventBox`(mod.rs:50)、`EvalHarness`(eval/mod.rs:150)、`EvalHarnessConfig.record_dir`(mod.rs:127)、`Engine::new`(engine.rs:614)、W1 的 `extra_tools` 钩子。

**Verification checklist：**
- 新测试：用 `MockLlmClient` 回放「调用 gadget_chain_trace → run_poc」canned 响应，断言 `Engine` 真跑了这两个工具且 `run_poc.realized` 出现在 `EvalRun` 结果里。
- `cargo test -p mimofan --lib eval` 全绿。
- grep 确认 `eval/mod.rs` 不再直接调用 `edit_file_append`/`exec_shell` 内部函数（改为走 Engine）。

**Anti-pattern guards：** mock 仅在 `cfg(test)`/eval 路径，不污染生产 `ApiClient`；不要给 `ApiClient` 加 mock 分支开关。

---

## W5 — 会话事件 + 验收（#838/#839）

**What to implement：**
- 在 `crates/tui/src/core/engine/trace.rs`（已有 `TraceId` trace.rs:17）**新增** `pub struct SessionEvent { kind, turn, ts, tool_calls, assistant_text, hypothesis_ops, poc_results }` + `pub struct SessionEventSink`（append-only 写 `~/.mimofan/tasks/<id>/session.jsonl`）。`SessionEvent` 全新类型，无冲突。
- emit 接线：在 `hypothesis`/`run_poc` 工具调用处与 W3 拦截点 emit 事件（可随 W3 一起，或 W5 独立加类型 + 最小 emit）。
- 新增 `benchmark/vuln_hunt/`（Python 驱动，复用 `benchmark/agentbench` 模式）：
  - `samples/` 下 2–3 个精简靶场（fastjson gadget / spring RCE / 配置漏洞），每 `task.json` 含 `prompt` + `expected{ expected_gadgets, expected_poc_expect }`。
  - 驱动：每任务拉独立临时 workspace，用 W4 `MockLlmClient`（无网）或 `--live` 真模型跑 `Engine` + `vuln-hunt` skill。
  - verifier 三维自动评分：一致性（hypothesis 事件流：每个 confirmed 有≥1 证据且无证据 resolve 被拒）、追踪（gadget_chain_trace `satisfied` 命中 `expected_gadgets`）、复现（run_poc.realized==true）。输出 `results/<task>.json` + 汇总记分卡。
  - `run.sh` 跑全量。

**Allowed APIs：** `TraceId`(trace.rs:17)、`hypothesis` 工具(`tools/hypothesis.rs` 一致性门)、`gadget_chain_trace`(`tools/gadget_chain.rs`)、`run_poc.evaluate`(`tools/run_poc.rs:62`)、`vuln-hunt` skill(`assets/skills/vuln-hunt/SKILL.md`)、W3 拦截点、`benchmark/agentbench` 现有结构。

**Verification checklist：**
- 单测：`SessionEvent` 写→读→结构一致 round-trip。
- `bash benchmark/vuln_hunt/run.sh` 跑通，内置精简靶场至少 1 个三维度全 1；CI 无网回放可跑。
- `cargo test` + `pytest benchmark/vuln_hunt` 绿。

**Anti-pattern guards：** verifier 不要只做子串匹配即满分（须查假设一致性 + gadget 链命中）；靶场放 `benchmark/` 数据目录不硬编码进 crate；`SessionEvent` 保持 append-only 不可变，不与 UI 实时事件混用。

---

## 合并阶段（统一编译验证，仅此一次）

所有 workstream 在各自 worktree/分支完成后，统一执行：

1. **合并**：按 CODEBUDDY.md 在 main 合并各分支（worktree 开发 → main 合并 → push）。
2. **统一编译验证（关键，仅此步跑全量 build/test）**：
   - `cargo build -p mimofan 2>&1 | tail` 必须 **零 error + 零新增 warning**（预存 `mimofan-memory` cfg / `mimofan-secrets` unused 允许）。
   - `cargo test -p mimofan --lib` 全绿（重点 `tools::hypothesis`/`run_poc`/`gadget_chain`、`eval`、`interceptor` 新测试）。
   - `cargo test -p mimofan-staticanalysis` 全绿。
   - `bash benchmark/vuln_hunt/run.sh` 产出记分卡。
   - **Anti-pattern grep**：`grep -rn "println!\|eprintln!" crates/tui/src/tools/ crates/tui/src/plugins/`（应无新命中，模块 deny）；`grep -rn "Command::new" crates/tui/src/tools/run_poc.rs`（应无直连）；`grep -rn "MockLlmClient" crates/tui/src/llm_client/mod.rs`（应只在 mock.rs 与 eval 路径）。
3. **文档 + 小版本**：新建 `docs/ARCHITECTURE.md`（四 seam + `PluginManifest` 生态接入方式）；更新 `REQUIREMENTS_BACKLOG.md`；根 `Cargo.toml` `version` 0.0.17 → **0.0.18**（`version.workspace=true` 单一来源）；`cargo build` 确认；commit `chore: release 0.0.18 (modular plugins)`；`git tag v0.0.18`；`git push --tags origin main`。
4. 关闭 #834–#840。

**Verification（合并后）：** `git log origin/main..HEAD` 含各 workstream commit；`git describe --tags` 含 `v0.0.18`；`cargo build -p mimofan` 绿；benchmark 记分卡存在。

---

## 总体反模式护栏（贯穿）
- 不要幻想 deepseek-harness 有现成 verifier —— 它**没有**，#839 必须自建。
- 不要重写 turn_loop 为全新架构 —— W3 先包裹拦截器。
- 不要引入运行时插件热挂载 —— Rust 静态环境用 manifest + trait 注入即可（奥卡姆）。
- 不要破坏已验证逻辑：`hypothesis` 一致性门 / `run_poc` 沙箱隔离 / `gadget_chain_trace` 空 KB 报错。
- 每个 workstream 只改自己那列文件；合并前不做全量编译（避免并行期反复重编），统一在合并阶段验证。
- manifest 只「选 provider」，不做「解释执行 DSL」。
