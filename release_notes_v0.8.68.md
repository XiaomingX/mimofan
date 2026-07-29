## v0.8.68 - 七项用户侧能力对齐 Claude Code / Gemini

本次 Release 关闭了 7 个 Issue（#24–#29, #56），全面补全了对齐竞品（Claude Code / Gemini / Antigravity）的用户侧 slash 命令入口，并完成了全量死代码与 Mock/Placeholder 的审计清理。

---

### ✨ 新增与增强功能

| Issue | 功能简述 | 命令 |
|---|---|---|
| [#24](https://github.com/XiaomingX/mimofan/issues/24) | 模式一键切换，对齐 Claude Code / Gemini | `/auto` `/plan` `/yolo` |
| [#25](https://github.com/XiaomingX/mimofan/issues/25) | 统一回滚入口，支持快照回滚与对话回溯 | `/rewind [N\|chat]` |
| [#26](https://github.com/XiaomingX/mimofan/issues/26) | 交互式需求澄清（强制 LLM 执行访谈而非直接写代码） | `/grill-me <task>` |
| [#27](https://github.com/XiaomingX/mimofan/issues/27) | 代码化简去重（提取公共函数、消除重复逻辑） | `/simplify <target>` |
| [#28](https://github.com/XiaomingX/mimofan/issues/28) | 安全审计闭环：OWASP Top 10、密钥泄漏、Unsafe Rust | `/code-review [path]` |
| [#29](https://github.com/XiaomingX/mimofan/issues/29) | 规划-执行分离工作流，对齐 Claude-mem | `/make-plan <task>` `/do [next\|all\|N]` |

### 🔧 内部优化

| Issue | 内容 |
|---|---|
| [#56](https://github.com/XiaomingX/mimofan/issues/56) | 全量 Mock/Placeholder/未接线功能审计：确认 ModelRegistry、ContextBudget、PromptZones 等历史遗留模块均已完整接入生产路径，无死代码残留 |

### 🧪 验证结果

- **单元测试**: 82 passed, 0 failed（`cargo test -p mimofan --lib`）
- **Release 编译**: Exit Code 0（`cargo build --release`）
- **Clippy 静态分析**: 0 warnings（`-D warnings` 门禁）

### 📦 安装

下载 `mimofan-v0.8.68-macos-x86_64.tar.gz`，解压后将 `mimofan` 和 `mimofan-cli` 放入 `PATH` 即可使用。

```bash
tar -xzf mimofan-v0.8.68-macos-x86_64.tar.gz
cd mimofan-v0.8.68-macos-x86_64
sudo mv mimofan mimofan-cli /usr/local/bin/
mimofan --version
```
