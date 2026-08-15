# 计划：模块化 harness 架构 + 漏洞挖掘长程评测（参考 deepseek-harness）

> 目标：借鉴 https://github.com/deepseek-ai/deepseek-harness 的「Everything is a Plugin / 能力即 seam」理念，把 mimofan 改造为更松耦合、可被外部生态接入的 harness；并补齐 deepseek-harness **本身缺失**的 benchmark/verifier 层，用于验收大模型在漏洞挖掘长程任务（规划能力、一致性）上的表现。
>
> 执行顺序（用户要求）：**先提交 issue → 实现 → 验收（含新增 benchmark 验收）→ 合并主干 → 发布小版本**。本计划按此顺序拆成阶段，每个阶段自包含、可在新对话中执行。
>
> 参考资料：deepseek-harness `docs/architecture.md`、`docs/subsystems/{core,tools,llm-streaming,sandbox}.md`（Cordis 插件运行时 + SessionEvent 流 + ToolGuard + LlmAdapter + SandboxMode）。

## 关联 issue（阶段 1 已建，2026-08-15）
| Issue | 标题 | 对应阶段 |
|-------|------|---------|
| #834 | 工具注册注入 seam（extra_tools 钩子） | 阶段 2 |
| #835 | 统一 SandboxBackend 为引擎级 per-task seam | 阶段 3 |
| #836 | turn_loop 抽成 EngineStep 拦截器链 | 阶段 3/5 |
| #837 | 实现 MockLlmClient + LlmProvider trait | 阶段 4 |
| #838 | SessionEvent 追加日志（可重放评分面） | 阶段 5 |
| #839 | vuln-hunt 任务 + verifier benchmark 套件 | 阶段 6 |
| #840 | 发布小版本 0.0.18 + 模块化架构文档 | 阶段 7 |

---

## 调研结论（Phase 0 证据）

### deepseek-harness 值得借鉴的点
1. **能力即 seam（插件运行时）**：`ctx.tools` / `ctx.sandbox` / `ctx.llm` / `ctx.fs` 等都是"服务定义 + Provider + Consumer"的 seam；挂载一个 plugin 即可替换整个能力实现，可逆、可组合（Cordis `cordis.patch.yml` 分层）。
2. **可重放 SessionEvent 流**：`session` 是 append-only `SessionEvent` 日志（source of truth），fork/resume/telemetry/评分都从同一事件流派生 —— 这是干净的评测回放面。
3. **ToolGuard 管线**：`execute() → tools/pre-execute(allow/deny/ask) → ToolGuard(reject) → tools/execute(timeout/retry/metrics) → post-execute`，工具执行有护栏与超时/重试/指标。
4. **LlmAdapter 抽象**：`abstract stream(options)` + `registerAdapter(providers, adapter)`，providers 经 `cordis.yml` 声明。
5. **Per-task 沙箱 seam**：`ctx.sandbox.confine(argv, policy) → ConfinedArgv`，fail-closed，`SandboxMode = read-only|workspace-write|danger-full-access`。

### deepseek-harness 的缺口（我们要自己建）
- **没有 benchmark / 数据集 / 评分 / verifier 包**。`BENCHMARK.md` 仅一段；`packages/` 无 task/dataset/score/verify。验证"exploit 是否真的触发"必须由外部 harness 自建（如 Cybench 式验证脚本驱动 per-workspace agent）。

### mimofan 现状与缺口（审计证据）
- `crates/tui` 是中心化胖前端，依赖几乎所有 crate；`staticanalysis` 是唯一真正解耦的下层库 crate。
- 工具注册 = `registry.rs` 硬编码 `with_xxx_tools()` builder 链，**无外部注入 seam**（但有 `ToolSpec` trait + `McpToolAdapter` 可用）。
- `turn_loop.rs` ~3200 行巨型过程函数，**无 trait 化拦截点**，无可重放 SessionEvent 流。
- 两套沙箱并存未统一：`SandboxManager`（OS 级 Seatbelt/Landlock）与 `SandboxBackend`（远程/容器，trait `exec(&cmd,&env)->SandboxOutput`）。
- `MockLlmClient` **仅存在于注释**（eval/mod.rs:343），未实现；`EvalHarness` 直接调内部函数而非真实 `Engine`/`ToolRegistry`。
- `benchmark/` 是数据（agentbench/p0 性能），**无可执行任务/验证套件**。
- 现有可用的 seam 种子：`ToolSpec`、`SandboxBackend`、`run_poc.evaluate(stdout,stderr,expect)`（PoC 复现判定）、`hypothesis` 一致性门。

---

## 阶段 1：提交 issue（先规划，后实现）

创建以下 GitHub issue（每个含「动机 / 借鉴来源 / 验收口径 / 拆解步骤」），编号由 gh 分配：

1. **`feat: 工具注册注入 seam（extra_tools 钩子）`** — 给 `tool_setup.rs` / `registry.rs` 增加 `fn extra_tools() -> Vec<Arc<dyn ToolSpec>>` 钩子，允许 eval driver 或外部 crate 在构建 `ToolRegistry` 时追加 native 工具，不破坏现有 builder。借鉴 dsh 的 provider seam 思想（低风险）。
2. **`feat: 统一 SandboxBackend 为引擎级 per-task seam`** — 提升 `ToolContext::with_sandbox_backend` 为 `Engine::sandbox_for(workspace)` 可覆盖方法，支持 per-task/per-workspace 选沙箱策略；统一 `SandboxManager` 与 `SandboxBackend` 两条路径。
3. **`feat: turn_loop 抽成 EngineStep 拦截器链`** — 以 `Box<dyn TurnInterceptor>` 包裹现有逻辑，暴露 pre-step / request / post-step / turn-stopping 切点（先包裹、渐进迁移，不重写）。
4. **`feat: 实现 MockLlmClient + LlmProvider trait`** — 落地 eval/mod.rs:343 占位的 record/replay mock，使 eval 可无网驱动 `Engine` 并接真模型做长程评测。
5. **`feat: SessionEvent 追加日志`** — 在 `trace.rs` 基础上规范化 append-only 事件流，供 eval 做任务成功/假设一致性事后 scorer。
6. **`feat: vuln-hunt 任务 + verifier benchmark 套件`** — 在 `benchmark/` 下新增可执行 harness：定义 2–3 个真实靶场任务（fastjson gadget / spring RCE 简化版 / 配置漏洞），用 `hypothesis`+`gadget_chain_trace`+`run_poc` 跑长程链路，自动评分（假设一致性、gadget 链命中、PoC realized）。
7. **`chore: 发布小版本 + 模块化架构文档`** — bump 到 0.0.18，写 `docs/ARCHITECTURE.md` 说明 seam 抽象与生态接入方式。

> 验收：7 个 issue 创建成功，且 #6 的 benchmark 套件在阶段 6 真正实现前保持 OPEN。

---

## 阶段 2：工具注册注入 seam（低风险，先打通外部接入）

**实现**：复制「`ToolSpec` 已是统一 trait + `McpToolAdapter` 已存在」这一事实，不重写 registry。在 `crates/tui/src/core/engine/tool_setup.rs` 增加：
```rust
/// 外部 crate / eval driver 可在此追加 native 工具，无需改 builder。
pub fn extra_default_tools() -> Vec<Arc<dyn ToolSpec>> { Vec::new() }
```
并在默认 builder 链末尾 `.with_tools(extra_default_tools())`。使 eval driver 能在测试时注入 `run_poc`/`hypothesis` 等组合而不碰 registry 源码。

**文档参考**：`registry.rs:51` `ToolRegistry::register`；`registry.rs:1250` `McpToolAdapter`。
**验证清单**：
- 单元/集成测试：构造一个 `extra_tools` 返回自定义 `ToolSpec`，断言它被注册进默认 registry（名字出现在 `list_tools`）。
- `cargo build -p mimofan` 零新增 warning。
**反模式护栏**：不要重写为反射/动态发现；不要删除现有 `with_xxx_tools()` 链。

---

## 阶段 3：统一 SandboxBackend 为引擎级 per-task seam

**实现**：在 `Engine`（`turn_loop.rs:65` `impl Engine`）增加可覆盖方法：
```rust
pub fn sandbox_for(&self, workspace: &str) -> Option<Arc<dyn SandboxBackend>> {
    self.default_sandbox_backend.clone() // 现有 ToolContext 注入点提升
}
```
并让 `tool_setup` 在构造 `ToolContext` 时调用 `engine.sandbox_for(workspace)` 注入 `sandbox_backend`，从而每个 task/workspace 可选不同沙箱（dsh `ctx.sandbox` 思想）。同时把 `SandboxManager`（OS 级）收敛为 `SandboxBackend` 的一个实现，消除两套并存。

**文档参考**：`sandbox/backend.rs:70` trait；`spec.rs:526` `sandbox_backend` 字段；`spec.rs:766` `with_sandbox_backend`。
**验证清单**：
- `run_poc` 走 `sandbox_for` 注入的 backend 仍返回 `realized`；单元测试构造一个 fake backend 断言 `exec` 被调用。
- `cargo test -p mimofan --lib tools::run_poc` 仍绿。
**反模式护栏**：不要引入 `Command::new` 直连本地 shell（必须经 `SandboxBackend`）；不要破坏现有 `SandboxManager` 的 OS 级路径行为。

---

## 阶段 4：实现 MockLlmClient + LlmProvider（让 eval 可无网驱动 Engine）

**实现**：落地 eval/mod.rs:343 占位的 mock。新增 `crates/tui/src/llm_client/mock.rs`：
- `MockLlmClient` 支持 `push_message_response(Vec<StreamChunk>)`（record 模式记录、replay 模式回放 canned `StreamChunk`：`text-delta`/`tool-call-delta`/`finish`，对齐 dsh `StreamChunk` 语义）。
- `LlmProvider` trait 抽象 `stream(options) -> AsyncIterable<StreamChunk>`，`ApiClient` 实现之；eval 用 `MockLlmClient` 替换。
- 修改 `EvalHarness::run` 改为驱动**真实 `Engine` + `ToolRegistry`**（而非直接调内部函数），用 `MockLlmClient` 喂入任务 prompt，跑完整 agent loop。

**文档参考**：dsh `LlmAdapter`（`docs/subsystems/llm-streaming.md`）；`eval/mod.rs:150` `EvalHarness`。
**验证清单**：
- 新增测试：用 `MockLlmClient` 回放一段 "调用 gadget_chain_trace → run_poc" 的 canned 响应，断言 `Engine` 真跑了这两个工具且 `run_poc.realized` 出现在结果里。
- `cargo test -p mimofan --lib eval` 全绿。
**反模式护栏**：不要在生产 `ApiClient` 里留 mock 分支开关；mock 仅在 `cfg(test)`/eval 路径。

---

## 阶段 5：SessionEvent 追加日志（可重放评分面）

**实现**：在 `trace.rs` 基础上定义 `SessionEvent{ kind, turn, ts, tool_calls, assistant_text, hypothesis_ops, poc_results }` 的 append-only JSONL 落盘（`~/.mimofan/tasks/<id>/session.jsonl`）。在 `turn_loop.rs` 现有拦截点（`should_stop_after_plan_tool`、`rx_steer`、`run_poc`/`hypothesis` 调用处）emit 事件。供阶段 6 scorer 事后读取。

**文档参考**：dsh `SessionEvent` + `deriveMessages()` 回放理念；`turn_loop.rs:2471` stop 拦截点。
**验证清单**：
- 跑一个含 `hypothesis`/`run_poc` 的会话，断言 `session.jsonl` 含对应事件且可重放为消息序列。
- 单元：解析器 round-trip（写→读→结构一致）。
**反模式护栏**：不要把 UI 实时事件（现有 trace.rs）与领域 SessionEvent 混用；保持 append-only 不可变。

---

## 阶段 6：vuln-hunt 任务 + verifier benchmark 套件（核心新增验收）

**实现**：在 `benchmark/vuln_hunt/` 下新增可执行 harness（Python 驱动，复用现有 `benchmark/agentbench` 模式）：
1. **任务定义**：2–3 个靶场（`samples/` 下含已知漏洞的精简 fastjson/spring/配置样本），每个 `task.json` 含 `prompt` + `expected`（`expected_gadgets`、`expected_poc_expect`）。
2. **驱动**：每个任务拉独立临时 workspace，用阶段 4 的 `MockLlmClient` 或真模型跑 `Engine` + `vuln-hunt` skill 长程链路。
3. **Verifier（自动评分）**：
   - **一致性分**：解析 `hypothesis` 事件流，确认每个 `resolve=confirmed` 都有 ≥1 证据且无证据 resolve 被拒（复用一致性门逻辑）。
   - **追踪分**：`gadget_chain_trace` 报告的 `satisfied` 链是否命中 `expected_gadgets`。
   - **复现分**：`run_poc.realized` 是否为 true（对照 `expected_poc_expect`）。
   - 产出 `results/<task>.json` + 汇总记分卡（三维度 0–1）。
4. 提供 `run.sh`：跑全量任务 → 输出记分卡。

**文档参考**：dsh `headless-agent` 的 per-workspace 隔离 + JSONL fixture 思路；现有 `run_poc.evaluate` / `hypothesis` 一致性门 / `gadget_chain_trace` 缺口标注。
**验证清单**：
- `bash benchmark/vuln_hunt/run.sh` 跑通，对内置精简靶场产出记分卡；至少一个任务三维度全 1（已知可解样本）。
- CI 可跑（无网时用 `MockLlmClient` 回放 canned 响应验证管线；真模型评测为可选 `--live`）。
- `cargo test` / `pytest` 全绿。
**反模式护栏**：verifier 不要只做"子串匹配即满分"——必须检查假设一致性与 gadget 链命中；不要硬编码靶场路径到 crate 内（放 `benchmark/` 数据目录）。

---

## 阶段 7：合并主干 + 发布小版本

**实现**：
1. 各阶段经 verify（build 零 warning + 对应测试绿）+ anti-pattern grep（`println!`/`Command::new` 直连/未授权 `unwrap`）全部通过后，在各自 worktree/分支合并回 main（按 CODEBUDDY.md：worktree 开发→main 合并→push）。
2. 更新 `REQUIREMENTS_BACKLOG.md` 与新建 `docs/ARCHITECTURE.md`（说明 seam 抽象：ToolSpec/SandboxBackend/LlmProvider/SessionEvent 四 seam + 生态接入方式）。
3. **小版本**：根 `Cargo.toml` `version` 0.0.17 → **0.0.18**（workspace 单一来源，`version.workspace=true`）；`cargo build` 确认；commit `chore: release 0.0.18 (modular harness + vuln-hunt bench)`；`git tag v0.0.18`；`git push --tags origin main`。
4. 关闭阶段 1 的 7 个 issue（#6 benchmark 实现后关闭；其余按落地情况关闭）。

**验证清单**：
- `git log origin/main..HEAD` 显示各阶段 commit；`cargo build -p mimofan` 绿；`bash benchmark/vuln_hunt/run.sh` 产出记分卡。
- `git describe --tags` 含 `v0.0.18`。

---

## 总体反模式护栏（贯穿）
- 不要幻想 deepseek-harness 有现成 verifier —— 它**没有**，必须自建（阶段 6）。
- 不要重写 `turn_loop.rs` 为全新架构 —— 先包裹拦截器（阶段 3/5 渐进）。
- 不要引入运行时插件热挂载（Cordis 式）—— Rust 静态环境用 `extra_tools` 注入 + trait seam 即可达到「松耦合、可外接」。
- 不要破坏现有 `hypothesis` 一致性门 / `run_poc` 沙箱隔离 / `gadget_chain_trace` 空 KB 报错等已验证逻辑。
- 每个阶段独立验证后再合并；合并前必须 `cargo build` 零新增 warning + 测试绿。
