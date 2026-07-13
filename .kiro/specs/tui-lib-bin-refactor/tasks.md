# tui-lib-bin 拆分任务清单

> 参考设计文档：`.kiro/specs/tui-lib-bin-refactor/design.md`

## Phase 1: 基础结构

- [ ] 1. 创建 `crates/tui/src/lib.rs` 基础结构
  - 创建空的 `lib.rs`，声明所有 `mod`（从 `main.rs` 移过来）
  - 保持所有模块声明顺序不变
  - `main.rs` 简化为调用 `lib::run()`
  - _验证: `cargo build -p mimofan-tui` 通过_

- [ ] 1.1 移动模块声明到 lib.rs
  - 复制 `main.rs` 前 107 行的 `mod` 声明到 `lib.rs`
  - 保持顺序不变
  - _验证: cargo build 通过_

- [ ] 1.2 简化 main.rs
  - 移除所有 `mod` 声明
  - `main()` 函数改为调用 `lib::run()`
  - _验证: `mimofan --help` 输出不变_

## Phase 2: 应用层抽取

- [ ] 2. 创建 `crates/tui/src/app/` 目录
  - `app/mod.rs` — 模块入口
  - `app/args.rs` — CLI 参数解析
  - `app/startup.rs` — 启动逻辑
  - `app/run.rs` — 主运行循环

- [ ] 2.1 抽取 `Cli` 结构体和参数解析
  - 从 `main.rs` 提取 `Cli` struct 和 `Commands` enum
  - 提取 `FeatureToggles`
  - 移到 `app/args.rs`
  - _验证: `cargo build -p mimofan-tui` 通过_

- [ ] 2.2 抽取启动逻辑
  - 提取 `configure_windows_console_utf8()`
  - 提取 `install_rustls_crypto_provider()`
  - 提取 `dotenv()` 调用
  - 移到 `app/startup.rs`
  - _验证: `cargo build -p mimofan-tui` 通过_

- [ ] 2.3 抽取主运行循环
  - 提取 `run()` 函数
  - 处理 `Commands` 分支
  - 移到 `app/run.rs`
  - _验证: `mimofan tui` 启动正常_

## Phase 3: 传输层抽取

- [ ] 3. 创建 `crates/tui/src/transport/` 目录
  - `transport/mod.rs` — `Transport` trait 定义
  - `transport/stdio.rs` — stdio 模式
  - `transport/tui.rs` — TUI 模式
  - `transport/tcp.rs` — TCP 模式

- [ ] 3.1 定义 `Transport` trait
  - `trait Transport { fn run(&self, app: &mut Application) -> Result<ExitCode>; }`
  - 移到 `transport/mod.rs`
  - _验证: cargo build 通过_

- [ ] 3.2 实现 StdioTransport
  - 从 `app/run.rs` 提取 stdio 逻辑
  - 移到 `transport/stdio.rs`
  - _验证: `mimofan --help` 正常_

- [ ] 3.3 实现 TuiTransport
  - 从 `app/run.rs` 提取 TUI 逻辑
  - 移到 `transport/tui.rs`
  - _验证: `mimofan` 启动正常_

## Phase 4: 目录重组

- [ ] 4. UI 模块重组
  - 创建 `crates/tui/src/ui/` 目录
  - 移动 `tui_history`, `tui_picker`, `tui_widgets`, `tui_views` 到 `ui/`
  - 更新 `lib.rs` 的模块路径
  - _验证: cargo build + 功能测试通过_

- [ ] 4.1 移动 tui 子模块到 ui/ 目录
  - `src/tui/` 改名为 `src/ui/`
  - 更新所有 import 路径
  - _验证: `cargo build -p mimofan-tui` 通过_

- [ ] 4.2 验证 TUI 功能
  - 启动 TUI: `mimofan`
  - 检查渲染是否正常
  - _验证: TUI 可用_

## 最终验证

- [ ] 5. 完整验证
  - `cargo build --release -p mimofan-cli -p mimofan-tui`
  - `cargo test -p mimofan-tui`
  - `cargo clippy -p mimofan-tui -- -D warnings`
  - `mimofan --help`
  - `mimofan --version`
  - `mimofan tui --help`
  - _验证: 所有命令正常_

---

## 任务依赖关系

```
Phase 1 (基础结构)
├── 1.1 移动模块声明
└── 1.2 简化 main.rs
    ↓
Phase 2 (应用层抽取)
├── 2.1 抽取 CLI 参数
├── 2.2 抽取启动逻辑
└── 2.3 抽取运行循环
    ↓
Phase 3 (传输层抽取)
├── 3.1 定义 Transport trait
├── 3.2 实现 StdioTransport
└── 3.3 实现 TuiTransport
    ↓
Phase 4 (目录重组)
├── 4.1 移动 tui 到 ui/
└── 4.2 验证 TUI 功能
    ↓
Final (完整验证)
└── 5. 完整验证
```
