# mimofan v0.0.11

本次发布聚焦**跨平台编译可用性修复**，并随附漏洞挖掘能力底层设施的收尾。

## 修复

- **Linux 编译失败（[issue #585](https://github.com/XiaomingX/mimofan/issues/585)）**：补齐 macOS-gated 函数在非 macOS 平台的 fallback，修复 `cargo install --git` 在 Linux 上的 `E0425`/`E0308` 编译错误。
  - `normalize_macos_modifiers`（`composer_ui.rs`）新增非 macOS 恒等 fallback。
  - `native_ocr_available` / `try_native_ocr`（`image_ocr.rs`）新增非 macOS fallback（返回 `false` / `Ok(None)`）。
  - `probe_bwrap_available` / `probe_cgroup_version`（`diagnostics.rs`）补 Linux 真实探测分支，并修正非 Linux 返回值类型。
  - `install_parent_death_signal` / `install_server_parent_death_signal`（`shell.rs` / `cli_commands/mod.rs`）补 Linux 实现（`prctl(PR_SET_PDEATHSIG, SIGKILL)`），父进程异常退出时回收子进程。
  - 消除 `try_headless_browser_fetch` 的跨平台未使用变量 warning。
  - **验证**：macOS `cargo check` 与 `x86_64-unknown-linux-musl` 交叉编译均通过（零 warning）。

## 收尾

- **LSP request/response 基础设施（[issue #597](https://github.com/XiaomingX/mimofan/issues/597)）**：已于 v0.0.10 实现并合入 main，本轮通过双平台编译验证后关闭 issue。作为 MECE L0 前置已落地。

## 资产

- `mimofan-v0.0.11-x86_64-apple-darwin.tar.gz`：macOS Intel 二进制包。
- 其他平台（macOS arm64 / Linux musl x64+arm64 / Windows x64）由 CI 发布矩阵产出。
