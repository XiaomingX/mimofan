# 更新日志

本项目的所有重要变更都记录在此。版本遵循[语义化版本控制](https://semver.org/)，从工作区根目录（`Cargo.toml` → `[workspace.package] version`）递增。

## [0.0.7] - 2026-08-04

### Changed
- **代码质量检查与清理**：修复循环依赖问题（protocol ↔ execpolicy），清理过时注释和版本引用
- **架构优化**：localization 模块独立为 crate，cli/runtime_threads 拆分为子目录
- **测试代码分离**：清理未使用的导入和 dead_code 警告

### Fixed
- 修复 `test_match_osc8_fragment` 测试中的 `\x08` → `'8'` bug
- 更新 API 配置验证脚本（`benchmark/api_providers_test.sh`）

---

Older releases: [CHANGELOG.md](https://github.com/XiaomingX/mimofan/blob/main/CHANGELOG.md) and [docs/CHANGELOG_ARCHIVE.md](https://github.com/XiaomingX/mimofan/blob/main/docs/CHANGELOG_ARCHIVE.md).
