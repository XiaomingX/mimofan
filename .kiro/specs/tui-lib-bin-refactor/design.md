# tui-lib-bin 拆分设计文档

## 1. 概述

### 1.1 问题描述

`crates/tui/src/lib.rs` 是空文件，`crates/tui/src/main.rs` 包含 6848 行代码，所有模块都堆在一个巨大的二进制 crate 里。

**存在的问题：**
- TUI 内部模块无法被其他入口复用
- `cargo doc` 失效，IDE 跳转失效
- 增量编译劣化，任何小改动都触发整个 TUI 重新链接
- 测试只能在 binary crate 里写

### 1.2 目标

把 `mimofan-tui` 从纯 binary crate 拆分成 `lib + bin` 结构：

```
crates/tui/
├── src/
│   ├── lib.rs          # 库入口，导出公开 API
│   ├── main.rs         # 二进制入口，仅负责启动
│   ├── app/            # 应用层（命令行参数解析、启动逻辑）
│   ├── repl/           # REPL 状态机（已有子目录）
│   ├── ui/             # TUI 渲染层
│   ├── transport/      # 传输层（stdio / TCP / HTTP）
│   └── ...
```

**约束：**
- CLI 参数必须完全兼容，不影响用户使用方式
- 仅重构代码组织，不改变运行时行为

---

## 2. 当前结构分析

### 2.1 main.rs 模块清单

```rust
// 界面相关
mod repl;           // REPL 状态机（已有子目录）
mod rlm;            // RLM 会话管理（已有子目录）
mod tui;            // TUI 渲染
mod tui_history;    // 历史记录
mod tui_picker;    // 选择器
mod tui_widgets;    // 组件
mod tui_views;     // 视图

// 核心业务
mod client;        // LLM 客户端
mod core;          // 核心引擎
mod models;        // 消息模型
mod session_manager; // 会话管理
mod state_machine; // 状态机

// 工具与扩展
mod tools;         // 内置工具
mod mcp;           // MCP 集成
mod mcp_server;    // MCP 服务器
mod hooks;         // 生命周期钩子
mod execpolicy;    // 执行策略

// 配置与设置
mod config;        // 配置解析
mod config_ui;     // 配置 UI
mod config_persistence; // 配置持久化
mod settings;      // 设置

// 子系统
mod automation_manager;
mod fleet;         // 子代理管理
mod goal_loop;
mod memory;
mod skills;
mod compaction;    // 上下文压缩

// 云端集成
mod acp_server;    // ACP 服务器
mod oauth;
mod remote_setup;

// 工具函数
mod artifacts;
mod child_env;
mod dependencies;
mod error_taxonomy;
mod errors;
mod eval;
mod features;
mod localization;
mod logging;
mod model_catalog;
mod model_inventory;
mod model_profile;
mod model_registry;
mod model_routing;
mod network_policy;
mod palette;
mod prefix_cache;
mod pricing;
mod project_context;
mod project_context_cache;
mod project_doc;
mod prompt_zones;
mod prompts;
mod purge;
mod request_tuning;
mod resource_telemetry;
mod retry_status;
mod route_budget;
mod route_runtime;
mod runtime_api;
mod runtime_log;
mod runtime_threads;
mod sandbox;
mod seam_manager;
mod shell_dispatcher;
mod skill_state;
mod slop_ledger;
mod snapshot;
mod status;
mod task_manager;
mod tls;
mod tool_output_receipts;
mod vision;
mod worker_profile;
mod working_set;
mod workspace_discovery;
mod workspace_trust;
mod utils;
```

### 2.2 已有的子目录模块

- `src/repl/` — REPL 状态机
- `src/rlm/` — RLM 会话管理
- `src/commands/` — slash 命令
- `src/client/` — LLM 客户端
- `src/core/` — 核心引擎
- `src/fleet/` — 子代理管理
- `src/sandbox/` — 沙箱后端
- `src/tui/` — TUI 渲染
- `src/llm_client/` — LLM 客户端封装
- `src/mcp/` — MCP 集成

---

## 3. 目标模块划分

### 3.1 推荐的模块层次

```
src/
├── lib.rs                 # 库入口
├── main.rs                # 二进制入口（~50 行）
│
├── app/                   # 应用层
│   ├── mod.rs
│   ├── args.rs           # clap 参数解析
│   ├── startup.rs        # 启动逻辑
│   └── run.rs            # 主运行循环
│
├── ui/                    # TUI 渲染层
│   ├── mod.rs
│   ├── widgets/          # UI 组件
│   ├── views/            # 视图
│   ├── history.rs        # 历史记录
│   ├── picker.rs         # 选择器
│   └── ...
│
├── repl/                  # REPL 状态机（已有）
│   ├── mod.rs
│   ├── runtime.rs
│   └── sandbox.rs
│
├── rlm/                   # RLM 会话管理（已有）
│   ├── mod.rs
│   ├── bridge.rs
│   ├── prompt.rs
│   ├── session.rs
│   └── turn.rs
│
├── transport/             # 传输层（新增）
│   ├── mod.rs
│   ├── stdio.rs          # stdio 模式
│   ├── tcp.rs            # TCP 模式
│   └── http.rs           # HTTP 模式
│
├── core/                  # 核心业务（已有子目录）
│
├── client/                # LLM 客户端（已有子目录）
│
├── fleet/                 # 子代理管理（已有子目录）
│
└── mcp/                   # MCP 集成（已有子目录）
```

### 3.2 渐进式拆分策略

由于 main.rs 很大（6848 行），采用渐进式拆分：

**Phase 1: 基础结构**
1. 创建 `src/lib.rs`，把 `main.rs` 的模块声明移过去
2. `main.rs` 只保留 `fn main()` 调用 `lib::run()`
3. 创建基础目录结构

**Phase 2: 应用层抽取**
1. 把 CLI 参数解析移到 `app/args.rs`
2. 把启动逻辑移到 `app/startup.rs`
3. 把主运行循环移到 `app/run.rs`

**Phase 3: 传输层抽取**
1. 创建 `transport/` 目录
2. 把 stdio/tcp/http 模式抽象成 `Transport` trait

**Phase 4: 目录重组**
1. 按限界上下文把相关模块分组到 `ui/`、`core/` 等目录

---

## 4. 关键设计决策

### 4.1 传输层抽象

```rust
// transport/mod.rs

pub trait Transport: Send + Sync {
    fn run(&self, app: &mut Application) -> Result<ExitCode>;
    fn name(&self) -> &str;
}

pub enum TransportMode {
    Stdio,
    Tui { mouse_capture: bool, alt_screen: bool },
    Tcp { bind: SocketAddr },
    Http { bind: SocketAddr },
}
```

### 4.2 应用层入口

```rust
// app/mod.rs

pub struct AppConfig {
    pub command: Commands,
    pub feature_toggles: FeatureToggles,
    pub prompt: Vec<String>,
    pub yolo: bool,
    pub max_subagents: Option<usize>,
    pub config: Option<PathBuf>,
    pub verbose: bool,
    pub profile: Option<String>,
    pub workspace: Option<PathBuf>,
    pub resume: Option<String>,
    pub continue_session: bool,
    // ...
}

pub fn run(config: AppConfig) -> Result<ExitCode>;
```

### 4.3 库导出

```rust
// lib.rs

pub mod app;
pub mod ui;
pub mod repl;
pub mod rlm;
pub mod transport;
pub mod core;
pub mod client;
pub mod fleet;
pub mod mcp;

pub use app::{run, AppConfig};
```

---

## 5. 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| 破坏 CLI 参数兼容性 | 每次重构后运行 `mimofan --help` 验证 |
| 引入循环依赖 | 先分析依赖图，按拓扑顺序拆分 |
| 增量编译收益不明显 | 只移动代码，不改变依赖关系 |
| 回归问题 | 每阶段运行 `cargo test -p mimofan-tui` |

---

## 6. 验证计划

### 6.1 每次重构后必须验证

```bash
# 1. 编译通过
cargo build -p mimofan-tui

# 2. CLI 参数兼容
mimofan --help
mimofan tui --help
mimofan --version

# 3. 功能测试
cargo test -p mimofan-tui

# 4. clippy 检查
cargo clippy -p mimofan-tui -- -D warnings
```

### 6.2 回归测试

```bash
# 启动 TUI
mimofan

# 单次对话
mimofan-cli "hello"

# 恢复会话
mimofan --resume <session_id>
```

---

## 7. 实施顺序

1. **Phase 1**: 创建 lib.rs，移动模块声明（纯移动，无逻辑变更）
2. **Phase 2**: 抽取 app/ 层（args.rs, startup.rs）
3. **Phase 3**: 抽取 transport/ 层
4. **Phase 4**: 目录重组

每阶段都是一个独立的 commit，便于回滚。
