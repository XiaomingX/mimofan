# mimofan 架构：模块化能力（插件化 seam）

> 本文件说明 mimofan 的模块化能力抽象，供生态自由排列组合。设计遵循 MECE（每条能力是一个正交轴）与奥卡姆剃刀（manifest 驱动组合，最小必要抽象，不引入运行时插件热挂载）。

## 核心思想

**能力 = 一个可组合的 seam trait；插件层 = 一个 manifest 声明要加载哪些 provider。**

mimofan 已具备 4 个 seam 种子，每个都是一条独立的能力轴：

| 轴 | Seam trait / 抽象 | 定义位置 | 可插拔点 |
|----|------------------|----------|----------|
| **A. 工具** | `ToolSpec` | `crates/tui/src/tools/spec.rs:1322` | `PluginRegistry::assemble` 注入额外工具 |
| **B. 沙箱** | `SandboxBackend` | `crates/tui/src/sandbox/backend.rs:70` | `Engine::sandbox_for(workspace)` per-task 选 backend |
| **C. 循环** | `TurnInterceptor` | `crates/tui/src/core/engine/interceptor.rs` | 在 turn_loop 既有点包裹（pre_step/request/turn_stopping） |
| **D. LLM** | `LlmClient` | `crates/tui/src/llm_client/mod.rs:64` | `MockLlmClient`（eval 无网驱动）/ `ApiClient`（真模型） |
| **E. 会话事件** | `SessionEvent` / `SessionEventSink` | `crates/tui/src/core/engine/trace.rs` | append-only 落盘，供 verifier 事后评分 |

> 注：`LlmClient` 因 async + `impl Trait` 返回**非 dyn-compatible**，故以具体类型 `ApiClient` 承载；其余 seam 均可做 `Arc<dyn Trait>` 对象。

## 插件清单（PluginManifest）

`crates/tui/src/plugins/manifest.rs` 定义 `PluginManifest`（TOML/YAML 序列化），声明启用哪些能力 provider：

```toml
[tools]
extra = ["hypothesis", "gadget_chain_trace", "run_poc"]   # 从已知 extra 工具注册表注入

[sandbox]
backend = "per_task"        # per_task | container | none

[llm]
provider = "mock"           # mock (eval 无网) | api (真模型)

[trace]
session_events = true       # 启用 SessionEvent 落盘
```

`crates/tui/src/plugins/registry.rs` 的 `assemble(manifest) -> AssembledCapabilities` 是**唯一**把各 seam 拼起来的地方（单一事实源，避免散落）：

```rust
pub struct AssembledCapabilities {
    pub tools: Vec<Arc<dyn ToolSpec>>,
    pub sandbox: Option<Arc<dyn SandboxBackend>>,
    pub llm: Option<Arc<ApiClient>>,
    pub session_events: bool,
}
```

`crates/tui/src/core/engine/tool_setup.rs` 在构造默认工具注册表时调用 `assemble` 并经 `with_extra_tools` 注入额外工具。

## 生态接入方式（排列组合）

1. **加一个 native 工具**：实现 `ToolSpec`（`crates/tui/src/tools/spec.rs`），在 `plugins/registry.rs` 的注册表登记其名，manifest 的 `[tools].extra` 即可选中——无需改 builder 源码。
2. **换沙箱后端**：实现一个 `SandboxBackend`，在 manifest `[sandbox].backend` 选择（或让 `Engine::sandbox_for` 按 workspace 返回它）。
3. **无网评测**：eval 用 `MockLlmClient`（`crates/tui/src/llm_client/mock.rs`）回放 canned 响应驱动真实 `Engine` + 工具链，见 `crates/tui/src/eval/mod.rs`。
4. **长程漏洞挖掘验收**：`benchmark/vuln_hunt/` 提供可执行 harness，verifier 按三维度自动评分——一致性（hypothesis 证据门）、追踪（gadget chain 命中）、复现（run_poc realized）。

## 与 deepseek-harness 的取舍

deepseek-harness 用 Cordis 运行时做能力即 plugin（运行时热挂载、可逆组合）。mimofan 取同样的**松耦合思想**，但因 Rust 静态环境，采用 **manifest 驱动 + trait 对象注入** 而非运行时热挂载——达到同等「生态可自由排列组合」效果，且更简单（奥卡姆）。deepseek-harness **本身没有** benchmark/verifier 层，其漏洞验收需外部自建；本项目的 `benchmark/vuln_hunt/` 即补上这一层。

## 反模式

- 不要幻想有运行时 hot-swap——manifest 只「选 provider」，不做解释执行 DSL。
- 不要重写 `turn_loop.rs` 主循环——`TurnInterceptor` 只包裹现有点。
- 不要直连 `Command::new` 本地 shell——执行必经 `SandboxBackend`。
- 不要破坏 `hypothesis` 一致性门 / `run_poc` 沙箱隔离 / `gadget_chain_trace` 空 KB 报错等已验证逻辑。
