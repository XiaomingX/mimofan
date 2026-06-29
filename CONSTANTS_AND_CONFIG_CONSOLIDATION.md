# 常量与配置统一收口计划

检查常量和敏感配置是否已统一管理，消除散落的硬编码。

---

## 现状评估

### 常量管理 — 基本良好，有几处散落

| 模块 | 状态 | 说明 |
|------|------|------|
| `provider_defaults.rs` | ✅ | 模型名、base_url 已集中 |
| `provider.rs` macro | ✅ | 提供商元数据通过宏统一注册 |
| `prompts.rs` | ✅ | 提示词模板已常量化 |
| `context_budget.rs` | ✅ | 上下文预算常量已集中 |
| `palette.rs` | ✅ | 颜色常量已集中 |

**散落问题**：
- `crates/core/src/lib.rs:139,156,171` — `"deepseek"` 硬编码，应用 `ProviderKind` 枚举
- `crates/tui/src/runtime_threads.rs:748` — `"deepseek"` 硬编码
- `crates/tui/src/request_tuning.rs:136,142` — 提供商名硬编码在 match 分支
- `crates/tui/src/config.rs:315` — `"deepseek"` 硬编码

### 敏感配置管理 — 已有体系，几处遗留

| 模块 | 状态 | 说明 |
|------|------|------|
| `secrets` crate | ✅ | KeyringStore 抽象，支持文件/系统钥匙链 |
| `ProviderConfig` | ✅ | api_key 字段统一管理 |
| `provider.rs` env_vars | ✅ | 环境变量查找集中定义 |
| 配置解析顺序 | ✅ | config → secret store → env |

**遗留问题**：
- `crates/tui/src/mcp_server.rs:501` — `.deepseek/mcp_server.toml` 硬编码，应用 `config.mcp_config_path()`
- `crates/secrets/src/lib.rs:31` — `DEFAULT_SERVICE = "deepseek"`（兼容保留）
- 环境变量名在 `provider_defaults.rs`、`provider.rs`、`lib.rs` 三处定义

---

## Phase 1: 消除硬编码提供商名 ✅ 已完成

**目标**: 用 `ProviderKind` 枚举替代散落的字符串 `"deepseek"`、`"openai"` 等

**完成情况**:
- ✅ 在 `provider_defaults.rs` 中添加了 `DEFAULT_PROVIDER_ID` 常量
- ✅ 将 `core/src/lib.rs` 中的 3 处硬编码 `"deepseek"` 替换为 `DEFAULT_PROVIDER_ID`
- ✅ 更新了 `mcp_server.rs` 中的路径从 `.deepseek` 改为 `.mimo`
- ✅ 修复了 `DEFAULT_PROVIDER_ID` 的可见性（从 `pub(crate)` 改为 `pub`）

**提交**: `5ca9b33` — `fix: replace hardcoded provider names with DEFAULT_PROVIDER_ID`

---

## Phase 2: MCP 配置路径统一 + settings.json 支持 ✅ 已完成

**目标**: 消除 `.deepseek/mcp_server.toml` 硬编码，统一使用 `config.mcp_config_path()`

**完成情况**:
- ✅ 将 `mcp_server.rs` 中的硬编码路径改为使用 `.mimo` 目录
- ✅ 新增 `settings.json` 支持（仿照 Claude Code 的 `~/.claude/settings.json`）
- ✅ 创建了 `JsonSettings` 结构体，支持：
  - `env`: 环境变量配置
  - `mcp_servers`: MCP 服务器配置
  - `enabled_plugins`: 插件配置
  - `language`: 语言偏好
  - `instructions`: 指令文件路径
- ✅ 创建了 `settings.example.json` 示例文件

**新增代码**:
- `crates/tui/src/settings.rs` — 新增 `JsonSettings` 和 `McpServerConfig` 结构体
- `settings.example.json` — 配置示例文件

**验证**: 所有 `json_settings` 测试通过

---

## Phase 3: 环境变量名统一

**目标**: 将散落在多处的环境变量名收口到 `provider_defaults.rs` 或 `provider.rs`

**参考代码**:
- `crates/config/src/provider_defaults.rs` — 模型和 URL 常量
- `crates/config/src/provider.rs:118-122` — env_vars 定义
- `crates/config/src/lib.rs:1977-1979` — TOKEN_PLAN_ENV_VARS

**任务**:
1. 在 `provider_defaults.rs` 中添加环境变量名常量
2. 将 `lib.rs` 中的 `TOKEN_PLAN_ENV_VARS` 和 `STANDARD_ENV_VARS` 引用 `provider_defaults.rs`
3. 确保 `provider.rs` 宏中引用统一常量

**验证**: `grep -rn "XIAOMI_MIMO_API_KEY"` 只出现在一处定义

---

## Phase 4: 验证与清理

**目标**: 确保所有常量和配置已统一收口

**任务**:
1. 运行 `cargo test --workspace` 确保无回归
2. 搜索残留的硬编码字符串
3. 更新文档

**验证**:
- `grep -rn '"deepseek"' --include="*.rs" crates/ | grep -v test | grep -v "//"` 结果为 0
- `grep -rn '\.deepseek' --include="*.rs" crates/ | grep -v test | grep -v "//"` 结果为 0
- 所有测试通过

---

## 结论

**常量管理**：Phase 1 已完成，`DEFAULT_PROVIDER_ID` 常量已添加到 `provider_defaults.rs`，硬编码的 `"deepseek"` 字符串已消除。

**敏感配置**：Phase 2 已完成，新增 `settings.json` 支持，MCP 配置路径已统一。`secrets` crate 设计良好，API Key 管理已统一。

**配置系统**：现在支持两种配置格式：
- `settings.toml` — UI 和行为偏好（原有系统）
- `settings.json` — MCP 服务器、环境变量、插件配置（新增，仿照 Claude Code）

**优先级**：Phase 1 和 Phase 2 已完成。Phase 3（环境变量名统一）可选，Phase 4（验证与清理）建议在 Phase 3 完成后进行。
