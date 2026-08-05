# 更新日志

本项目的所有重要变更都记录在此。版本遵循[语义化版本控制](https://semver.org/)，从工作区根目录（`Cargo.toml` → `[workspace.package] version`）递增。

## [0.0.9] - 2026-08-05

### Added
- **Issue Monitor 工作流** ([#560](https://github.com/XiaomingX/mimofan/issues/560))：新增 issue 监控与自动处理流程，便于跟踪上游反馈与自动归档。

### Changed
- **架构规划收尾（DDD 第一性原理梳理，零功能新增、零交互层改动）**：
  - 标记 `mimofan-memory` 为 **experimental（未集成）**，避免零上游依赖的僵尸模块误导用户 ([#570](https://github.com/XiaomingX/mimofan/issues/570))。
  - `execpolicy` 双实现厘清为**互补**而非重复：CLI/tui 走本地文件策略、crate 提供可复用引擎；仅移除 `shell.rs` 的死导入 ([#572](https://github.com/XiaomingX/mimofan/issues/572))。
  - 双重「运行时」（`crate::Runtime` 无界面 API 核心 / `tui::Engine` 交互循环）厘清为**两个限界上下文**，命名撞车误判，不合并 ([#574](https://github.com/XiaomingX/mimofan/issues/574))。
  - UI 层 3 处渲染 IO（提示建议静态客户端、剪贴板落盘、文件树读取）确认为**展示层正当职责**，不做端口化注入（过度设计）([#576](https://github.com/XiaomingX/mimofan/issues/576))。
  - 冗余英文文档清理并中文化架构说明 ([#568](https://github.com/XiaomingX/mimofan/issues/568))。
  - DDD 拆分 5 个「上帝文件」模块 ([#566](https://github.com/XiaomingX/mimofan/issues/566))。

### Security
- **deny 规则大小写不敏感（行为变化，更安全）** ([#580](https://github.com/XiaomingX/mimofan/issues/580))：tui 的 deny/allow 匹配统一改用 crate 的 lowercase `canonical_executable_form`，`execpolicy.toml` 中 `deny = ["rm *"]` 现在也能拦住大写命令（`RM -rf /` / `SUDO RM -rf /`），不再因大小写绕过 deny 规则。这是本版本唯一的行为变化点。

---

Older releases: [CHANGELOG.md](https://github.com/XiaomingX/mimofan/blob/main/CHANGELOG.md) and [docs/CHANGELOG_ARCHIVE.md](https://github.com/XiaomingX/mimofan/blob/main/docs/CHANGELOG_ARCHIVE.md).
