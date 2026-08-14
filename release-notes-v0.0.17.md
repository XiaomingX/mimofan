# mimofan v0.0.17

本轮（LoopX 续作）在已落地的代码库语义索引基础上，补齐**研究编排范式全链路命令壳 + GitHub 共同作者署名**，并大规模落地**安全审计门、浏览器/反向 MCP/ACP 协议扩展、记忆混合检索与跨会话推理、子代理 DAG 编排与循环守卫持久化**等能力。

## 能力更新

### 研究编排范式
- **可机评优化回路 `/evolve`（#751）**：外部 evaluator 拥有正确性，代理不自我报分。
- **可复现性纪律 `/repro`（#754）**：固化 `BRIEF.md` + `env_snapshot.json` + `provenance.jsonl`，默认零行为变更。
- **研究成果物汇总 `/artifact`（#750）**：汇总到 `initiatives/<id>/`，`--publish` 走副作用闸门，不自动推远程。
- **独立评审者 `/reviewer`（#752）**：只读审核 claim，作为 `/artifact` 公开章节前置门。
- **GitHub 共同作者署名**：`git_commit` 默认追加 `Co-Authored-By: mimofan` trailer。

### 安全（Security）
- **AUTO 权限分类器两阶段 + fail-closed（#730）**：未知工具默认落入拒绝分支。
- **输入侧提示注入扫描（#723）**、**skill 供应链 provenance + 隔离审计（#731）**。
- **内容密钥扫描 / 流式脱敏 / 路径穿越防护（#718/#680/#648）**。
- **execpolicy 路径安全内核 `path_guard`（#681）**、**命令注入 fuzz（#640）**、**命令注入绕过 + SAST 修复（#756/#715/#670）**、**trust_mode 与路径边界解耦（#733）**。

### 工具与协议（Tools & Protocol）
- **浏览器自动化工具（#743）**：`navigate`/`click`/`type`/`screenshot`/`eval_js`，复用 SSRF guard。
- **反向 MCP server 接线到 CLI 子命令（#746）**、**ACP 能力矩阵扩展（#745）**、**用量/成本/工具分析洞察（#744）**、**聚合工具调用度量（#734）**、**有效上下文利用率度量（#735）**、**结构化输出 `syntheticOutput`（#729）**、**`/share --local` 本地导出（#688）**。

### 记忆（Memory）
- **用户建模 `UserProfile`（#732）**、**consolidation 模块 + 重要性评分（#716）**、**Embedder trait 抽象（#712）**、**混合检索 RRF + score_breakdown（#714）**、**hybrid_bm25 关键词召回（#777/#778）**、**跨会话推理 `session_id`（#777）**、**`MemoryStats` + `/status` 接线（#628）**、**LongMemEval 评测 harness（#777）**、**edit/apply 首试成功率基准（#689）**、**compaction 事实保留率断言（#629）**。

### 子代理与编排（Subagent & Orchestration）
- **DAG 任务编排 + 结构化 plan 维度**、**`fork_turns` 窗口裁剪（#702）**、**`task_shell_stop`（#776）**、**后台 shell 完成注入 `<task-notification>`（#696）**、**pre-turn 快照 fire-and-forget（#643）**、**多目标 `GoalQueue`（#654）**、**sidebar 与 `ui_event_loop` 拆分（#647）**、**循环守卫跨回合持久化 + 崩溃恢复 + 日志脱敏**、**循环守卫两维度（#694）**、**bus + `task_claims` 接线（#699）**。

### 模型与路由 / 平台可观测
- **catalog 实时刷新 + 磁盘持久化（#3385/#787）**、**AUTO 路由 token 成本上报（#692）**、**prefix-cache 命中率（#646）**。
- **`mimofan-telemetry` crate，feature-gated OTel 桥接（#726）**、**调用图可达性骨架（#598）**、**自动化 Webhook 投递（#671/#775）**。

### TUI / UX
- **首屏欢迎页 redesign + 美化**、**HNSW 删除残影修复**。

## 验证

- **构建**：`cargo build --release`（workspace version `0.0.17`）成功，零 warning；产物 `mimofan 0.0.17 (067c9b956340)`。
- **单元测试**：`cargo test --workspace --lib` —— **764 passed / 0 failed**（含 `mimofan-memory` 75 passed、`mimofan-execpolicy` 32 passed、`mimofan-secrets` 18 passed、`mimofan-tui` 608 passed 等全部 crate 绿）。
- **修复的测试回归**：#777 引入的 `should_remember` salience gate（短内容不写入长期记忆，有意设计）与 3 个旧测试假设冲突（`rebuild_is_noop_for_default_backend`、`prune_removes_expired`、`count_dual_store_consistency_reports_both`），已将测试内容补足至通过 salience 阈值，**未改动功能行为**。
- **未纳入本次验证范围**：`cargo test --workspace` 的集成/二进制测试（LongMemEval harness、edit/apply 基准等）需真模型或耗时极长，不阻塞 release；已由对应独立单测与基准条目覆盖。

## 资产

- `mimofan-v0.0.17-x86_64-apple-darwin.tar.gz`：macOS Intel 二进制包（本机打包）。
- 其他平台（macOS arm64 / Linux musl x64+arm64 / Windows x64）由 CI 发布矩阵产出。
