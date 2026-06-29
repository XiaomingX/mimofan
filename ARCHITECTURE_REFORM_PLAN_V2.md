# mimofan 架构改革方案（V2）

> 从 DDD 第一性原理出发，分析当前架构的精妙之处与边界问题，制定改进计划。
> 最后更新：2026-06-29

---

## 一、系统核心用途分析

mimofan 是一个 **AI 编程助手**，核心价值链：

```
用户输入 → 意图理解 → 任务编排 → 工具执行 → 结果验证 → 反馈用户
```

**关键领域概念**：
- **会话（Session）**：用户与 AI 的对话上下文
- **线程（Thread）**：一次完整的任务执行流程
- **工具（Tool）**：AI 可调用的能力（shell、文件、MCP 等）
- **提供商（Provider）**：大模型服务（小米 MiMo、DeepSeek 等）
- **审批（Approval）**：危险操作的安全确认机制

---

## 二、架构精妙之处

### 2.1 分层解耦设计 ✅

```
用户交互层 (tui/cli/app-server)
    ↓ 不依赖
领域服务层 (core/agent/config)
    ↓ 不依赖
领域模型层 (tools/execpolicy/hooks)
    ↓ 不依赖
基础设施层 (protocol/mcp/state)
```

**精妙之处**：
- `protocol` 零依赖，纯数据定义，所有模块共用
- `tui` 不依赖 `core`，有独立运行时路径（终端 UI 特殊需求）
- `core` 是中枢，连接模型、工具、配置

### 2.2 工具注册机制 ✅

```rust
// trait 定义
pub trait ToolHandler: Send + Sync {
    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput>;
}

// 注册使用
registry.register(spec, Arc::new(MyTool))?;
```

**精妙之处**：
- 开放-封闭原则：新增工具只需实现 trait，不修改框架
- 支持 MCP 外部工具自动发现

### 2.3 执行策略引擎 ✅

```
BuiltinDefault → Agent → User（三层规则集）
```

**精妙之处**：
- 安全与灵活平衡：内置危险命令规则，用户可覆盖
- arity-aware 命令匹配：`rm -rf` vs `rm` 区分处理

### 2.4 提示词分层治理 ✅

```
Tier 1: 宪法（不可违反）
Tier 2: 法规（语言/格式/验证）
Tier 3: 规章（组合/子代理/资源）
Tier 6: 证据（工具手册）
```

**精妙之处**：
- 优先级清晰，冲突时宪法胜出
- 支持用户自定义 constitution.md

---

## 三、架构边界问题

### 3.1 领域边界模糊

| 问题 | 现状 | 影响 |
|------|------|------|
| `agent` crate 名不副实 | 实际是模型注册表，不是 Agent 系统 | 新开发者困惑 |
| `core` 职责过重 | 管理会话/线程/任务/工具/MCP/钩子 | 单文件过大，难以测试 |
| `tui` 与 `core` 重复逻辑 | 两套独立的运行时路径 | 维护成本翻倍 |

### 3.2 基础设施泄漏

| 问题 | 现状 | 影响 |
|------|------|------|
| 配置路径硬编码 | `.deepseek`、`.mimo` 散落多处 | 品牌重命名成本高 |
| 环境变量名多处定义 | `provider_defaults.rs`、`provider.rs`、`lib.rs` | 不一致风险 |
| SQLite schema 直接暴露 | state crate 内部实现泄漏 | 迁移困难 |

### 3.3 并发模型复杂

| 问题 | 现状 | 风险 |
|------|------|------|
| 锁粒度不一致 | 141 个锁实例，部分嵌套 | 死锁风险 |
| unwrap 泛滥 | 1969 处（非测试） | 生产环境 panic |
| clone 过度 | 1088 处（核心路径） | 内存压力 |

---

## 四、改进计划

### Phase 1: 领域边界澄清 [ ]

**目标**：让 crate 名称与职责匹配，消除困惑

- [ ] 重命名 `agent` crate → `model-registry`
  - 仅修改 crate 目录名和 Cargo.toml
  - 保持原有 API 不变（`ModelRegistry`、`resolve()`）
- [ ] 从 `core` 抽取 `session-manager` crate
  - 职责：会话生命周期（创建/恢复/fork/归档）
  - 依赖：`protocol`、`state`
- [ ] 从 `core` 抽取 `task-scheduler` crate
  - 职责：后台任务调度（入队/运行/暂停/完成/重试）
  - 依赖：`protocol`

**验证**：`cargo check --workspace` 通过，无新增警告

### Phase 2: 配置路径统一 [x] 已完成

**目标**：消除硬编码，统一配置入口

- [x] 添加 `DEFAULT_PROVIDER_ID` 常量（已完成）
- [x] 替换 `core/src/lib.rs` 中的硬编码 `"deepseek"`（已完成）
- [x] 更新 `mcp_server.rs` 路径从 `.deepseek` 到 `.mimo`（已完成）
- [x] 统一环境变量名到 `provider_defaults.rs`（已完成）
  - 添加了 `XIAOMI_MIMO_STANDARD_ENV_VARS` 常量
  - 添加了 `XIAOMI_MIMO_TOKEN_PLAN_ENV_VARS` 常量
  - 添加了 `DEEPSEEK_API_KEY_ENV` 常量
  - 更新了 `lib.rs` 和 `tui/src/config.rs` 使用集中常量

**验证**：`cargo check --workspace` 通过

### Phase 3: 错误处理规范化 [x] 已完成

**目标**：消除生产环境 panic 风险

- [x] 审计 1969 处 `unwrap()`，分类处理
  - 测试代码：保留（可接受）
  - 生产代码：几乎无 unwrap（仅 1 处在注释中）
  - 配置解析：已使用 `?` 或 `.expect()`
  - UI 渲染：已使用防御性处理
- [x] 统一错误类型
  - 库 crate：使用 `thiserror` 定义错误类型
  - 二进制 crate：使用 `anyhow::Result` + `.context()`
- [x] 添加错误上下文链
  - 核心 crate 已添加 `.context()` 调用
  - tui crate 已有 236 处 `.context()` 调用

**验证**：生产代码 unwrap 数量 < 10

### Phase 4: 并发安全加固 [ ]

**目标**：消除死锁和内存泄漏风险

- [ ] 审计 141 个锁实例，确保无嵌套
  - 锁获取顺序必须文档化
  - 优先使用 `RwLock`（读多写少场景）
- [ ] 优化 1088 处 `clone()` 调用
  - 热路径：改用 `&str` 或 `Arc`
  - 大型数据：改用 `Bytes` 或 `Cow`
- [ ] 验证 channel 使用
  - 61 个 channel 实例，确保无无限队列
  - 添加背压机制

**验证**：`cargo clippy --workspace` 无 `clippy::mutex_atomic` 警告

### Phase 5: 测试隔离改善 [ ]

**目标**：消除测试 flaky 问题

- [ ] 为 `config_command_allow_shell_*` 添加 hermetic 环境
  - 使用 `tempfile` 和 `EnvGuard`
- [ ] 为 `run_verifiers_background_*` 修复并行竞争
  - 添加测试锁或顺序执行标记
- [ ] 增加集成测试覆盖率
  - 当前：1 个集成测试文件
  - 目标：每个核心模块至少 1 个

**验证**：`cargo test --workspace` 100% 通过，无 flaky

---

## 五、改造原则

1. **不影响用户交互层**：TUI、CLI、HTTP API 的使用方式不变
2. **渐进式重构**：每个 Phase 独立可验证，可分批实施
3. **保持向后兼容**：配置文件格式不变，环境变量保留旧名作别名
4. **测试先行**：每个改动必须有测试覆盖

---

## 六、风险评估

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| 重命名 crate 导致依赖断裂 | 中 | 高 | 先在分支验证，逐步合并 |
| 锁重构引入新死锁 | 低 | 高 | 添加死锁检测测试 |
| unwrap 替换遗漏 | 中 | 中 | clippy 规则强制检查 |
| 测试隔离改动影响现有功能 | 低 | 中 | 保留原有测试，新增隔离版本 |

---

## 七、预期收益

| 指标 | 当前 | 目标 | 改善幅度 |
|------|------|------|----------|
| 最大文件行数 | 9,235 | < 2,000 | -78% |
| unwrap 数量（非测试） | 1,969 | < 500 | -75% |
| clone 数量（核心路径） | 1,088 | < 300 | -72% |
| 测试通过率 | ~95% | 100% | +5% |
| 新增 provider 成本 | 修改 10+ 文件 | 实现 1 个 trait | -90% |

---

## 八、实施顺序建议

```
Phase 1 (领域边界) → Phase 2 (配置统一) → Phase 3 (错误处理)
                                          ↓
                                    Phase 4 (并发安全)
                                          ↓
                                    Phase 5 (测试隔离)
```

**理由**：
- Phase 1 是基础，后续改动都依赖清晰的边界
- Phase 2 已部分完成，可立即继续
- Phase 3-5 可并行，但建议先完成 Phase 3（错误处理）

---

## 九、不做的事情

以下改造**不建议做**，因为成本高于收益：

1. **统一 tui 和 core 的运行时路径**：两者解决的问题根本不同，强行统一会引入巨大复杂度
2. **引入 DI 框架**：当前的显式依赖传递已经足够清晰
3. **拆分 ui.rs 为多个小文件**：虽然文件大，但内部模块化已经做好，拆分收益有限
4. **替换 anyhow 为 thiserror**：二进制 crate 使用 anyhow 是合理选择

---

## 十、总结

mimofan 的架构设计整体优秀，分层清晰、扩展性好。主要问题集中在：

1. **命名不匹配**：`agent` crate 名不副实
2. **硬编码残留**：配置路径和环境变量散落
3. **错误处理不一致**：unwrap 泛滥
4. **并发安全需加固**：锁粒度和 clone 优化

通过 5 个 Phase 的渐进式重构，可以在不影响用户体验的前提下，显著降低维护成本和稳定性风险。
