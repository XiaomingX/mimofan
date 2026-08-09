# mimofan vs CodeBuddy / Claude Code 记忆机制对比

> 调研产出（research）：梳理 mimofan 现有记忆/上下文架构，对标 CodeBuddy（`CODEBUDDY.md` 分层自动记忆）与 Claude Code（`CLAUDE.md` + 子目录注入 + 向量召回），列出差距与可落地建议。
> 代码事实基于 `2b0ca30`。纯分析文档，不含代码改动。

## 1. 维度对比总表

| 维度 | mimofan | CodeBuddy | Claude Code | 差距 |
|---|---|---|---|---|
| **存储位置** | 文件记忆（`memory/` 目录：`note.md`/`user.md` 等）+ 向量记忆（sled 本地库） | 文件（`MEMORY.md` 索引 + 分类 `.md`）+ 自动抽取持久化 | `CLAUDE.md` + `.claude/` 子目录 + 可选向量 | mimofan 无「索引文件」概念，文件记忆靠 tag 路由 |
| **分类体系** | 文件记忆按 `#tag` 路由（`# user foo` → `user.md`）；向量记忆按 `kind` | 分层：`user`/`feedback`/`project`/`reference` 四类 + 索引 | 不分明类，靠目录与文件名 | mimofan 分类弱、无索引层 |
| **写入方式** | 显式：`/note`、`/vmemory remember`、agent `remember` 工具 | 自动抽取（对话中识别偏好/事实）+ 显式 `/memory` | 显式编辑 `CLAUDE.md` | mimofan **无自动抽取**，完全依赖用户/agent 显式写入 |
| **检索/注入** | `MemoryInjector` 注入系统提示；向量语义召回互补文件记忆 | 会话启动时加载分层记忆 | 启动时加载 `CLAUDE.md` + 子目录规则 | mimofan 已具备语义召回，但缺少「启动时分层加载」结构化 |
| **向量召回** | ✅ `embedding`（OpenAI/DeepSeek API）+ `hnsw-rs` + `sled`，按 `MIMOFAN_MEMORY_API_KEY` 优雅降级 | 有向量召回层 | 有向量召回 | 能力对标，mimofan 默认编译但需配置 key 才启用 |
| **生命周期治理** | ✅ `forget`/`update` + 反记忆清单 + 陈旧性校验（commit `67adb10`） | 自动去重/过期 | 手动维护 | mimofan 已补齐，接近对标 |
| **项目上下文** | `project_context` + `AGENTS.md` + `Compact Instructions` | `CODEBUDDY.md` 分层注入 | `CLAUDE.md` + 子目录 `CLAUDE.md` | mimofan 多套并存，缺乏统一入口 |

## 2. mimofan 现有架构事实（证据）

- **记忆 crate**：`crates/memory/src/` 提供跨会话能力——文本 embedding（API）、向量存储检索（hnsw-rs + sled）、观测压缩、跨会话注入、知识 agent。
  - `lib.rs:10-21`：默认编译（`vector-memory` 进 `default` features），运行时按 `MIMOFAN_MEMORY_API_KEY` 优雅降级（`enabled()==false` 时零网络零磁盘副作用）。
  - `injector.rs:7,52,71`：`MemoryInjector` 持 `EmbeddingService`，负责跨会话记忆注入系统提示。
  - `optimization.rs:243` `ObservationStore`、`embedding.rs:66` `EmbeddingService`：观测存储与向量化。
- **用户命令**（`crates/tui/src/commands/groups/memory/`）：
  - `/note [add|list|show|edit|remove|clear|path]`（`mod.rs:34`）——文件型记忆，按 `#tag` 路由（如 `# user foo` → `user.md`，`memory.rs:21,88`）。
  - `/memory [show|path|clear|edit|help]`（`mod.rs:40`）——文件记忆管理。
  - `/vmemory [status|remember <kind> <text>|query <text>|list|help]`（`mod.rs:48`、`vmemory.rs:22,114-166`）——向量记忆：语义存储/召回/列举。
- **agent 写入**：`remember` 工具（`memory.rs:21` 注释）允许子 agent 写记忆；`/vmemory remember` 供用户写观测。
- **项目上下文**：`project_context` + `AGENTS.md` + `Compact Instructions` 三段并存，无统一「项目记忆索引」。

## 3. 关键差距

1. **无自动抽取**：CodeBuddy 能从对话自动抽取偏好/事实并持久化；mimofan 完全依赖显式 `/note`、`remember` 工具。用户不主动写则无记忆积累。
2. **分类弱、无索引**：mimofan 文件记忆靠 `#tag` 路由到零散 `.md`，无 `MEMORY.md` 式索引；检索靠全量注入或向量召回，缺少「分层按需加载」。
3. **多套上下文并存**：`project_context` / `AGENTS.md` / `Compact Instructions` 三套机制目标重叠，缺少统一编排入口（参见项目内 `ARCHITECTURE_IMPROVEMENT_PLAN` Phase D）。
4. **向量记忆默认沉默**：虽默认编译，但 `MIMOFAN_MEMORY_API_KEY` 未配置时完全不启用，多数用户实际只用文件记忆，语义召回能力被浪费。

## 4. 可落地建议

- **P1 自动抽取**：在 turn_loop 收尾阶段，用廉价模型从对话抽取「偏好/事实/项目约定」候选，经 `remember` 工具落盘（对标 CodeBuddy 自动抽取）；需用户可关闭（`/memory auto off`）。
- **P2 记忆索引**：新增 `MEMORY.md` 式索引文件，作为文件记忆的入口与去重层；`/note list` 与注入均走索引。
- **P2 统一上下文编排**：将 `project_context` / `AGENTS.md` / `Compact Instructions` 收敛为单一「项目记忆加载器」，避免三套机制漂移。
- **P2 向量记忆默认引导**：未配置 key 时，`/vmemory status` 给出一键引导（生成 key 配置或回退到本地轻量 embedding），降低能力门槛。
- **已达标项**：生命周期治理（forget/update/反记忆/陈旧性）已对标，无需重做，仅需在文档中明确标注。

## 5. 结论

mimofan 记忆机制在「存储/向量召回/生命周期」三维度已对标 CodeBuddy/Claude Code；**最大差距是自动抽取与分层索引**，其次是多套项目上下文的统一编排。建议优先级：自动抽取（P1）> 索引与编排（P2）。
