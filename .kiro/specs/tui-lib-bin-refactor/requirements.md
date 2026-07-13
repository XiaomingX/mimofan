# tui-lib-bin 拆分需求文档

## 1. 概述

### 1.1 背景

`mimofan-tui` 是一个纯 binary crate（`lib.rs` 为空），所有代码堆在 6848 行的 `main.rs` 里。这导致：
- 增量编译劣化
- 模块无法被其他 crate 复用
- `cargo doc` 和 IDE 支持失效

### 1.2 目标

将 `mimofan-tui` 重构为 `lib + bin` 结构，使核心模块可被 `app-server` 等其他入口复用。

### 1.3 用户故事

**作为** 开发者
**我想要** 把 `mimofan-tui` 拆成 lib + bin 结构
**以便** 增量编译更快、模块可复用、IDE 支持正常

---

## 2. 需求

### 2.1 兼容性需求

- **R-2.1**: 重构后 `mimofan --help` 输出必须与重构前完全一致
- **R-2.2**: 重构后 `mimofan tui --help` 输出必须与重构前完全一致
- **R-2.3**: 重构后 `mimofan --version` 输出必须与重构前完全一致
- **R-2.4**: 所有现有的 CLI 参数必须继续工作

### 2.2 功能需求

- **R-2.5**: 重构后 TUI 界面功能必须与重构前完全一致
- **R-2.6**: 重构后 CLI 单次对话功能必须与重构前完全一致
- **R-2.7**: 重构后 stdio 模式功能必须与重构前完全一致
- **R-2.8**: 重构后会话恢复功能必须与重构前完全一致

### 2.3 代码组织需求

- **R-2.9**: `lib.rs` 必须导出所有公开模块
- **R-2.10**: `main.rs` 必须仅包含 `fn main()` 入口逻辑
- **R-2.11**: 应用层逻辑必须抽取到 `app/` 模块
- **R-2.12**: 传输层逻辑必须抽取到 `transport/` 模块

### 2.14 工程质量需求

- **R-2.14**: 重构过程中 `cargo build -p mimofan-tui` 必须始终通过
- **R-2.15**: 重构过程中 `cargo test -p mimofan-tui` 必须始终通过
- **R-2.16**: 重构后 `cargo clippy -p mimofan-tui` 必须无警告

---

## 3. 验收标准

### 3.1 编译验证

```bash
# 编译成功
cargo build -p mimofan-tui

# 测试通过
cargo test -p mimofan-tui

# Clippy 无警告
cargo clippy -p mimofan-tui -- -D warnings
```

### 3.2 CLI 参数验证

```bash
# 这些命令的输出必须与重构前完全一致
mimofan --help
mimofan tui --help
mimofan --version
mimofan --profile test --help
```

### 3.3 功能验证

```bash
# TUI 启动
mimofan
# 期望: TUI 界面正常显示

# CLI 单次对话
echo "hello" | mimofan-cli
# 期望: 输出正常

# 会话恢复
mimofan --resume <session_id>
# 期望: 恢复成功
```

---

## 4. 约束

- 仅重构代码组织，不改变运行时行为
- 保持模块依赖关系不变
- 不删除任何功能代码
- 不修改任何业务逻辑
