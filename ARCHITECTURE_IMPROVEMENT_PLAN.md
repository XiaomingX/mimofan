# 架构改进计划

> 基于 DDD 理论的架构分析与改进方案

---

## 1. 架构现状分析

### 1.1 精妙之处

| 维度 | 评价 |
|------|------|
| **Crate 依赖图** | 14 个 crate 的依赖图是 DAG，方向严格向下，无环 -- Rust workspace 工程纪律的体现 |
| **共享内核** | `protocol` crate 作为 DTO 层，多入口共享同一套类型，是 DDD 最佳实践 |
| **端口化设计** | 22+ 个 trait 端口（`ToolHandler`、`SandboxBackend`、`LlmClient`、`McpBackend` 等），扩展点清晰 |
| **端口反转** | `BalanceProvider` 端口反转已实施（`ui/ports.rs`），证明团队理解 DDD 端口化 |

### 1.2 存在的问题

#### 问题 1：tui crate 膨胀（最严重）

- **现状**：432 个 .rs 文件、205,859 行，占全仓 85%+
- **问题**：不只是 TUI -- 包含 LLM 客户端、MCP 集成、60 个工具实现、提示词构建、配置管理、运行时引擎等全部业务逻辑
- **影响**：编译慢、维护难、职责混乱

#### 问题 2：双重运行时

- **现状**：`core::Runtime`（2,177 行）和 `tui` 自己的引擎（3,737 行）+ 运行时（3,843 行）并存
- **问题**：处理相似问题但 API 不同，职责重叠
- **影响**：代码重复、维护成本高

#### 问题 3：UI 层直接 IO

- **现状**：`prompt_suggestion.rs` 在渲染层直接发 HTTP 请求；`file_tree.rs`、`clipboard.rs` 等有大量同步 `std::fs` 调用
- **问题**：违反分层架构原则，UI 层不应直接访问外部资源
- **影响**：测试困难、耦合度高

#### 问题 4：execpolicy 重复定义

- **现状**：`tui/src/execpolicy/`（11 文件）与独立的 `crates/execpolicy` 并存
- **问题**：同一功能存在两套实现
- **影响**：维护成本高、可能不一致

#### 问题 5：mimofan-memory 孤立

- **现状**：workspace 中有完整的向量记忆系统，但没有任何 crate 依赖它
- **问题**：功能未被集成
- **影响**：代码浪费

---

## 2. 改进计划

### Phase 1：tui crate 拆分（最高优先级）

**目标**：将 tui crate 中的非 TUI 逻辑拆分为独立 crate

**待办事项**：

- [x] 拆分测试代码：从 235 个源文件中删除空的 #[cfg(test)] 模块，提取 6 个测试到独立文件
- [x] 拆分 tui/ui/mod.rs：从 6983 行拆分为 6 个子模块（provider_and_model、view_dispatch、message_dispatch、mcp_shell、workspace）
- [x] 拆分 tools/subagent/mod.rs：从 6260 行拆分为 10 个子模块（constants、helpers、runtime_config、manager、projection、tool、execution、parser、registry、prompts）
- [x] 拆分 tui/app.rs：从 5479 行拆分为 10 个子模块（state、actions、events、helpers、impl_core、impl_history、impl_streaming、impl_composer、impl_actions）
- [x] 简化 UI picker：删除 theme_picker.rs 和 feedback_picker.rs，只保留核心 picker
- [ ] 拆分工具层：将 `crates/tui/src/tools/` 移动到 `crates/tools/`（已有独立 crate）
- [ ] 拆分 LLM 客户端：将 `crates/tui/src/llm/` 移动到 `crates/llm-client/`
- [ ] 拆分提示词：将 `crates/tui/src/prompts/` 移动到 `crates/prompts/`
- [x] 拆分本地化：将 `crates/tui/src/localization.rs` 移动到 `crates/localization/`
- [x] 拆分 MCP 传输层：将 `crates/tui/src/mcp.rs` 中的传输实现拆分到 `crates/tui/src/mcp/transport.rs`
- [ ] 更新 tui crate 的依赖关系
- [ ] 验证编译和测试通过

**预期效果**：tui crate 行数减少 60%+，职责清晰

### Phase 2：运行时统一

**目标**：评估 core 和 tui 的引擎重叠度，合并或明确边界

**待办事项**：

- [ ] 分析 `core::Runtime` 和 `tui` 引擎的功能重叠
- [ ] 确定统一方案：合并或明确分工
- [ ] 实施统一方案
- [ ] 验证编译和测试通过

**预期效果**：消除双重运行时，降低维护成本

### Phase 3：UI 层 IO 收口

**目标**：通过端口注入替代直接 IO

**待办事项**：

- [ ] 识别 UI 层所有直接 IO 调用
- [ ] 定义端口 trait（如 `HttpClient`、`FileSystem`）
- [ ] 实现端口适配器
- [ ] 修改 UI 层代码使用端口注入
- [ ] 验证编译和测试通过

**预期效果**：UI 层不再直接访问外部资源，测试更容易

### Phase 4：execpolicy 去重

**目标**：统一执行策略实现

**待办事项**：

- [ ] 分析 `tui/src/execpolicy/` 和 `crates/execpolicy` 的差异
- [ ] 确定保留哪个实现
- [ ] 删除重复实现
- [ ] 更新依赖关系
- [ ] 验证编译和测试通过

**预期效果**：消除重复代码，维护成本降低

### Phase 5：memory 接入评估

**目标**：评估是否需要集成向量记忆系统

**待办事项**：

- [ ] 评估 mimofan-memory 的功能和成熟度
- [ ] 确定是否需要集成
- [ ] 如果需要，设计集成方案
- [ ] 实施集成
- [ ] 验证编译和测试通过

**预期效果**：决定是否启用向量记忆功能

---

## 3. 实施路径

### 3.1 优先级排序

1. **Phase 1**（最高）：tui crate 拆分 -- 立即开始
2. **Phase 2**（高）：运行时统一 -- Phase 1 完成后
3. **Phase 3**（中）：UI 层 IO 收口 -- Phase 2 完成后
4. **Phase 4**（中）：execpolicy 去重 -- 与 Phase 3 并行
5. **Phase 5**（低）：memory 接入评估 -- 最后处理

### 3.2 时间估算

| Phase | 预估工时 | 依赖 |
|-------|---------|------|
| Phase 1 | 5-8 人天 | 无 |
| Phase 2 | 3-5 人天 | Phase 1 |
| Phase 3 | 3-5 人天 | Phase 2 |
| Phase 4 | 2-3 人天 | 无（可并行） |
| Phase 5 | 2-4 人天 | 无 |
| **总计** | **15-25 人天** | - |

### 3.3 验证标准

每个 Phase 完成后必须满足：

1. `cargo build --workspace` 编译通过
2. `cargo test --workspace` 测试通过
3. `cargo clippy --workspace` 无新警告
4. `cargo fmt --check` 格式正确
5. 核心功能正常（TUI 启动、LLM 调用、工具执行）

---

## 4. 注意事项

1. **只影响底层**：改进只影响内部架构，不影响用户交互层（TUI、CLI、HTTP 接口）
2. **MECE 原则**：每个 Phase 职责明确，不重叠
3. **奥卡姆剃刀**：不引入不必要的复杂性
4. **渐进式改进**：每个 Phase 独立可交付，风险可控
5. **不新增功能**：只做存量优化，不添加新特性

---

## 5. 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 拆分导致编译错误 | 高 | 小步拆分，每步验证 |
| 运行时统一引入 bug | 高 | 充分测试，灰度发布 |
| UI 层 IO 收口影响性能 | 中 | 性能测试，必要时保留同步路径 |
| execpolicy 去重影响安全 | 高 | 安全测试，保留沙箱功能 |
| memory 接入不成熟 | 低 | 评估后决定是否实施 |

---

> 最后更新：2026-08-03
