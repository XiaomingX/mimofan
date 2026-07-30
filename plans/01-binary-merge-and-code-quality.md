# 实施计划：二进制合并与代码质量改进

## 概述

本计划涵盖 4 个改进方向，按依赖关系排序执行。

## Issues

| Issue | 标题 | 优先级 |
|-------|------|--------|
| #380 | 合并 CLI/TUI 为单一二进制 | P1 |
| #381 | 统一退出码常量 | P2 |
| #382 | 审查所有 let _ = 模式 | P3 |
| #383 | 验证日志滚动机制 | P3 |

---

## Phase 1: 统一退出码常量 (#381)

**目标**：消除硬编码退出码，建立统一常量

### 步骤

1. **创建退出码常量文件**
   - 文件：`crates/tui/src/exit_codes.rs`
   - 内容：
     ```rust
     /// 成功退出码
     pub const EXIT_SUCCESS: i32 = 0;
     /// 通用错误退出码
     pub const EXIT_ERROR: i32 = 1;
     /// 沙箱拒绝退出码
     pub const EXIT_SANDBOX_DENIED: i32 = 2;
     ```

2. **替换硬编码**
   - `crates/tui/src/hooks.rs:728`
   - `crates/tui/src/core/engine/turn_loop.rs:2683`
   - `crates/tui/src/tools/shell.rs` 多处
   - `crates/tui/src/tools/tasks.rs:350`

3. **添加模块声明**
   - `crates/tui/src/lib.rs` 添加 `mod exit_codes;`

### 验证

- `cargo test -p mimofan`
- `cargo clippy -p mimofan`
- grep 确认无残留硬编码

---

## Phase 2: 审查 let _ = 模式 (#382)

**目标**：为所有错误忽略添加注释说明

### 步骤

1. **收集所有 let _ = 位置**
   - 使用 `grep -rn "let _ =" crates/ --include="*.rs"`

2. **分类审查**
   - Channel 发送：添加 `// Intentional: receiver dropped` 注释
   - 关闭信号：添加 `// Intentional: best-effort shutdown` 注志
   - 文件操作：评估是否需要日志
   - 数据库操作：评估是否需要日志

3. **为重要操作添加日志**
   - 文件删除失败
   - 数据库操作失败

### 验证

- `cargo test --workspace`
- 人工审查每个注释

---

## Phase 3: 验证日志滚动 (#383)

**目标**：确保日志不会无限增长

### 步骤

1. **审查 TUI 日志滚动**
   - 验证 `prune_old_logs()` 在所有退出路径执行
   - 验证 `TuiLogGuard` 的 Drop 实现

2. **验证多实例隔离**
   - 检查日志文件名包含 PID
   - 验证并发写入安全

3. **测试边界情况**
   - 磁盘空间不足
   - 日志目录不存在
   - 权限问题

### 验证

- 手动测试：运行多个实例
- 检查日志文件是否按日期分割
- 验证旧日志被清理

---

## Phase 4: 合并二进制 (#380)

**目标**：将 CLI 独有命令移入 TUI，只保留 `mimofan` 二进制

### 步骤

1. **识别 CLI 独有命令**
   - auth, config, model, thread, metrics, update, mcp-server, completion

2. **移动命令到 TUI crate**
   - 从 `crates/cli/src/lib.rs` 移动命令处理函数
   - 在 TUI 的 `Commands` 枚举中添加缺失的命令
   - 在 TUI 的 `run()` 函数中添加命令处理

3. **更新 TUI 入口点**
   - `crates/tui/src/main.rs` 保持不变

4. **更新 CLI 入口点**
   - `crates/cli/src/main.rs` 改为调用 TUI 的 `run()` 函数
   - 或者删除 CLI crate，只保留 TUI

5. **更新 Cargo.toml**
   - 从 workspace members 中移除 cli（如果删除）
   - 更新 default-members

### 验证

- `cargo build --release -p mimofan`
- 测试所有命令
- 验证无功能遗漏

---

## 执行顺序

```
Phase 1 (#381) → Phase 2 (#382) → Phase 3 (#383) → Phase 4 (#380)
```

Phase 1-3 是独立的代码质量改进，可以并行执行。
Phase 4 依赖 Phase 1-3 完成后再执行。

---

## 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 合并后 CLI 功能缺失 | 高 | 详细功能对比测试 |
| 退出码变更影响外部集成 | 中 | 保持向后兼容 |
| 日志滚动影响调试 | 低 | 保留足够的日志历史 |
| let _ = 注释遗漏 | 低 | 代码审查 |
