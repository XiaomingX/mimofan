# GitHub Issues Analysis and Status

Fetched at: 2026-07-28T18:49:21

## [x] Issue #11: feat: 支持定时/夜间任务队列 (/night /time)
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/11](https://github.com/XiaomingX/mimofan/issues/11)

### Description / Action Plan
## 功能描述

支持定时执行任务队列，允许用户在夜间低峰时段排队发送多条任务，依次执行。

## 动机

- 夜间 API 有折扣/低延迟优势，但用户无法手动在 0 点发消息
- 白天高峰期等待时间长，影响工作效率
- 希望睡前安排好任务，醒来直接拿结果

## 期望行为

- 提供 `/night` 或 `/time` 类指令，支持设定任务执行时间
- 支持任务队列：任务 A 执行完自动执行任务 B、任务 C……
- 提供任务列表管理界面，查看排队/执行中/已完成状态

## 参考

类似功能请求：#1331

---

## [x] Issue #12: feat: 不支持的输入类型（图片等）返回结构化错误而非崩溃
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/12](https://github.com/XiaomingX/mimofan/issues/12)

### Description / Action Plan
## 功能描述

当模型不支持某种输入类型（如图片）时，返回结构化的错误提示，而非直接报错或中断对话。

## 问题

mimo-v2.5-pro 遇到图片输入时会直接报错/中断，没有优雅降级。

## 期望行为

- API 层增加前置判断：支持 → 正常处理；不支持 → 返回可控的结构化提示
- 上层调用方能据此做降级处理（如切换模型、提示用户）

## 参考

#1173

---

## [x] Issue #13: feat: 支持长期记忆能力（如 claude-mem），减少重复上下文消耗
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/13](https://github.com/XiaomingX/mimofan/issues/13)

### Description / Action Plan
## 功能描述

集成长期记忆系统（如 claude-mem），使模型能跨会话记住关键信息，避免每次对话都重新传递完整上下文。

## 动机

- 每次会话重复传递项目背景、用户偏好等信息，token 消耗大
- 模型无法记住用户之前的决策和习惯，体验割裂
- 长期记忆可显著降低上下文长度，节省成本

## 期望行为

- 支持对接 claude-mem 等记忆服务，自动存储/检索关键信息
- 记忆按项目、用户维度隔离
- 记忆内容可搜索、可管理（查看/删除）
- 会话开始时自动注入相关记忆，减少手动重复输入

## 参考

claude-mem: https://github.com/nicobailon/claude-mem

---

## [x] Issue #14: fix: MiMo 推理模型多轮对话需正确处理 reasoning_content 字段
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/14](https://github.com/XiaomingX/mimofan/issues/14)

### Description / Action Plan
## 问题描述

MiMo v2.5 Pro 等推理模型在多轮对话中，要求所有 assistant 历史消息必须包含 `reasoning_content` 字段，否则 API 返回 400 错误。

当前代码中，缺少 `reasoning_content` 的空 assistant 消息会被直接过滤并输出 WARN 日志，可能导致上下文丢失。

## 期望行为

- 自动检测 MiMo 推理模型（mimo-v2.5-pro、mimo-v2.5 等）
- 过滤空消息时，保留含 `reasoning_content` 的消息
- 为缺失 `reasoning_content` 的 assistant 历史消息自动注入空值，避免 API 报错

---

## [x] Issue #15: feat: 压缩后自动缓存预热，提升 cache hit 率
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/15](https://github.com/XiaomingX/mimofan/issues/15)

### Description / Action Plan
## 功能描述

上下文压缩（compaction）后前缀剧变，下一次请求必定全量缓存未命中。需在压缩完成后自动发送 cache warmup 请求。

## 问题

- 压缩后系统提示和消息列表变化，导致 DeepSeek/MiMo 的前缀缓存失效
- 下一次真实请求变成全量 cache miss，浪费输入 token

## 期望行为

- 压缩完成后立即调用 `build_cache_warmup_request()` 发送预热请求
- 使用压缩后的新消息列表构建预热请求
- 可选：命中率持续低于 40% 时自动触发预热

## 参考代码

- `crates/tui/src/client/chat.rs:600-697` — `build_cache_warmup_request()`
- `crates/tui/src/runtime_threads.rs:2245` — `compact_thread()`
- `PREFIX_CACHE_OPTIMIZATION.md` — Phase 1 详细方案

---

## [x] Issue #20: 废弃过时的 toml 依赖，配置文件入口统一收口到 ~/.mimo/setting.json
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/20](https://github.com/XiaomingX/mimofan/issues/20)

### Description / Action Plan
## 背景

当前项目中使用了 `toml` crate 作为配置文件的解析依赖，存在以下问题：

1. **依赖过时**：`toml` 依赖版本较旧，维护成本较高，且与生态中其他库存在潜在兼容性问题。
2. **配置入口分散**：目前配置文件路径在多处硬编码或分散管理，缺乏统一标准。

## 目标

- 废弃并移除过时的 `toml` 依赖，改用 `serde_json` 对 JSON 格式配置进行统一解析。
- 将所有配置文件的读写入口统一收口到 `~/.mimo/setting.json`，替换现有 `~/.mimofan/settings.toml` 路径。

## 改动范围

- [ ] 移除 `Cargo.toml` 中的 `toml` 依赖
- [ ] 将配置模块（`mimofan-config`）的解析格式从 TOML 切换为 JSON
- [ ] 更新所有引用配置路径的代码，统一使用 `~/.mimo/setting.json`
- [ ] 提供迁移工具或启动时自动迁移逻辑（检测旧 `settings.toml` 并转换为 `setting.json`）
- [ ] 更新文档与 README，反映新的配置路径
- [ ] 补充相应的测试用例，覆盖新配置路径的读写逻辑

## 注意事项

- 迁移需保持向后兼容：若检测到旧配置文件（`~/.mimofan/settings.toml`），应给出提示并引导用户迁移，不应静默失败。
- 路径变更（`~/.mimofan/` → `~/.mimo/`）属于破坏性变更，需在 CHANGELOG 中记录，并在下一个 Minor/Major 版本发布时包含迁移指南。

## 参考

- 当前配置模块：`crates/mimofan-config/`
- 相关测试：`config_command_allow_shell_*`（已知受 `default_mode = "yolo"` 影响，需注意隔离）

---

## [x] Issue #21: 废弃对 Linux 和 Windows 的编译支持，先专注做好基于 macOS 的能力
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/21](https://github.com/XiaomingX/mimofan/issues/21)

### Description / Action Plan
## 背景

当前项目同时维护 macOS、Linux、Windows 三个平台的编译目标，带来以下问题：

1. **维护成本高**：跨平台兼容性处理（路径、系统调用、终端行为等）占用了大量开发精力。
2. **质量分散**：在资源有限的阶段，三端并进导致每个平台的体验都难以打磨到位。
3. **macOS 优先**：核心用户群和主要开发环境集中在 macOS，Linux/Windows 的实际使用覆盖有限。

## 目标

- 在当前阶段**废弃 Linux 和 Windows 的官方编译支持**，将精力集中于 macOS 平台。
- 保持代码层面的跨平台潜力（避免引入仅 macOS 特有的强依赖），以便未来重新支持其他平台时成本可控。

## 改动范围

- [ ] 移除 CI/CD 中 Linux 和 Windows 的构建与测试 workflow（`.github/workflows/`）
- [ ] 更新 `Cargo.toml` 中与平台相关的条件编译配置，标记非 macOS target 为不支持
- [ ] 在 README 中明确标注当前仅支持 macOS，并说明 Linux/Windows 暂不维护
- [ ] 在 `CHANGELOG.md` 中记录此破坏性变更
- [ ] 关闭或标注与 Linux/Windows 相关的 open issues，注明当前策略

## 注意事项

- **非永久废弃**：此举为阶段性策略，待 macOS 端能力稳定后，可再评估重新支持其他平台的路径。
- **代码保留**：不删除现有跨平台条件编译代码，仅停止 CI 构建与官方发布，降低未来恢复成本。
- **社区沟通**：在 PR 和 Release Notes 中清晰说明原因，避免社区误解为项目放弃跨平台方向。

## 讨论

欢迎社区就此策略提出意见，尤其是对 Linux 有强依赖的用户或贡献者，可在此 issue 下反馈实际使用场景，供后续重新支持时参考优先级。

---

## [x] Issue #22: 废弃对 IDEA 插件、VSCode 插件的编译支持，先专注做好基于 TUI 的能力
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/22](https://github.com/XiaomingX/mimofan/issues/22)

### Description / Action Plan
## 背景

当前项目同时维护 TUI、IDEA 插件、VSCode 插件三个交互端，带来以下问题：

1. **维护成本高**：IDE 插件涉及独立的插件 SDK、发布流程（JetBrains Marketplace / VS Code Marketplace）、版本兼容矩阵，维护代价远高于 TUI。
2. **体验割裂**：在资源有限阶段，三端并进导致核心能力（Agent 调度、上下文管理、工具调用）在各端实现不一致，难以统一打磨。
3. **TUI 优先**：TUI 是最接近底层、迭代最快、依赖最少的交互界面，适合作为核心能力的首要落地载体。

## 目标

- **阶段性废弃** IDEA 插件与 VSCode 插件的官方编译与发布支持。
- 将研发精力集中于 TUI 端，将其打磨为稳定、高质量的核心交互入口。
- 保持插件相关代码结构完整，以便未来能力成熟后低成本恢复插件支持。

## 改动范围

- [ ] 移除 CI/CD 中 IDEA 插件和 VSCode 插件的构建、打包、发布 workflow
- [ ] 在对应插件目录（如 `plugins/idea/`、`plugins/vscode/`）添加 `DEPRECATED` 说明文件
- [ ] 更新 README，明确当前仅维护 TUI 端，插件方向暂停
- [ ] 在 `CHANGELOG.md` 中记录此破坏性变更
- [ ] 关闭或归档与插件相关的 open issues，注明当前策略

## 注意事项

- **非永久废弃**：此为阶段性收缩策略，待 TUI 核心能力（Agent、工具链、配置体系）稳定后，重新评估插件端的恢复优先级。
- **代码保留**：不删除现有插件代码，仅停止 CI 构建与 Marketplace 发布，降低未来恢复成本。
- **社区沟通**：在 PR 和 Release Notes 中清晰说明原因，对有插件使用需求的用户提供 TUI 的替代方案引导。

## 讨论

欢迎对 IDEA / VSCode 插件有强依赖的社区用户在此反馈实际使用场景，相关需求将作为后续恢复支持时的优先级参考。

---

## [x] Issue #23: 优化基于 macOS 下的 UI 与体验，在配色和动画效果方面符合主流最佳实践
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/23](https://github.com/XiaomingX/mimofan/issues/23)

### Description / Action Plan
## 背景

当前 TUI 界面在 macOS 终端下的视觉体验较为朴素，存在以下问题：

1. **配色方案缺乏设计感**：颜色选取随意，未遵循现代终端 UI 的配色规范（如对比度、语义色、暗色模式适配）。
2. **动画与过渡效果缺失**：加载状态、Agent 执行进度、面板切换等关键交互缺少流畅的动画反馈，体验生硬。
3. **与 macOS 系统风格脱节**：未充分利用 macOS 终端（Terminal.app / iTerm2 / Warp）对 256色/真彩色/Unicode 的支持能力。

## 目标

打造符合现代 macOS 终端最佳实践的 TUI 视觉体验，具体包括：

- **配色系统**：建立统一的语义化调色板（primary / accent / success / warning / error / muted），支持暗色主题，对比度符合 WCAG AA 标准。
- **动画效果**：在关键交互节点（启动 splash、Agent 思考中、工具调用进度、流式输出）引入细腻的 spinner / progress bar / fade-in 动画。
- **排版与布局**：优化字体权重层级、间距节奏、面板边框风格，提升整体视觉层次感。
- **适配主流终端**：针对 iTerm2、Warp、macOS Terminal.app 做兼容性验证，确保真彩色与 Nerd Font 图标正常渲染。

## 改动范围

- [ ] 新增 `theme` 模块，统一管理 TUI 配色 token（基于 `ratatui` 的 `Style` 系统）
- [ ] 重构现有组件配色，替换硬编码颜色为 theme token 引用
- [ ] 引入/优化 spinner 动画组件，用于 Agent 执行、工具调用等异步等待场景
- [ ] 优化 progress bar 样式，支持带百分比、速率、ETA 的丰富展示
- [ ] 面板切换、消息追加等场景增加 fade-in / slide 过渡效果（帧率控制在合理范围内）
- [ ] 补充视觉回归测试或截图对比，防止后续改动破坏视觉一致性
- [ ] 更新文档，说明主题配置方式（预留用户自定义主题入口）

## 参考与灵感

- [Charm.sh 生态](https://charm.sh/)：Bubble Tea / Lip Gloss / Glamour 的设计理念
- [Warp Terminal](https://www.warp.dev/) 的 block-based UI 交互模式
- [lazygit](https://github.com/jesseduffield/lazygit) / [k9s](https://github.com/derailed/k9s) 的成熟 TUI 配色实践
- macOS Human Interface Guidelines 在终端场景下的色彩与动效原则

## 注意事项

- 动画帧率需受控，避免在低性能机器或 SSH 远程场景下造成渲染抖动。
- 所有配色变更需在无真彩色环境（256色、16色降级）下保持可用性。
- 视觉改动属于非破坏性变更，但需保证键盘导航与可访问性不受影响。

---

## [x] Issue #24: 确认并补全 /auto 与 /plan 模式，切换方式对齐 Claude Code / Gemini，降低用户迁移成本
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/24](https://github.com/XiaomingX/mimofan/issues/24)

### Description / Action Plan
## 背景

经代码审查，当前 TUI 的模式体系如下：

### 现状

`AppMode` 枚举已有三个模式（[tui/src/tui/app.rs](../blob/main/crates/tui/src/tui/app.rs)）：

| 模式 | 内部值 | 快捷键 | 描述 |
|------|--------|--------|------|
| Agent | `AppMode::Agent` | `1` | 自主任务执行，需工具审批 |
| Plan | `AppMode::Plan` | `2` | 只读规划，禁止 shell / 写操作 |
| Yolo | `AppMode::Yolo` | `3` | 全量工具访问，无审批拦截 |

**当前切换方式**：`/mode [agent|plan|yolo|1|2|3]`

**缺失**：
1. **无独立的 `/plan` slash 命令**：用户必须输入 `/mode plan`，而非直觉上的 `/plan`。
2. **无独立的 `/auto` slash 命令**：`auto` 目前仅是模型选择关键词（`app.auto_model`），不是可交互切换的应用层模式入口。
3. **切换路径与主流工具不一致**：Claude Code 和 Gemini 均支持直接在输入框输入 `/plan`、`/auto` 触发模式切换，认知路径更短。

---

## 目标

1. **补全独立 slash 命令**：新增 `/plan`、`/auto`、`/agent`、`/yolo` 作为 `/mode` 子命令的快捷别名，使用户可以直接触发模式切换而无需记住 `/mode` 前缀。
2. **对齐主流工具习惯**：切换交互与 Claude Code / Gemini 保持一致，降低用户认知偏差和习惯迁移成本。
3. **明确 `/auto` 的语义定位**：区分「自动模型选择」（`auto_model`）与「Auto 执行模式」，避免命名混淆。

---

## 改动范围

### 1. 新增模式别名 slash 命令

在 `crates/tui/src/commands/groups/config/mod.rs` 中注册以下独立命令：

```rust
// 对齐 Claude Code / Gemini 的直觉路径
"/plan"  => config::mode(app, Some("plan"))
"/agent" => config::mode(app, Some("agent"))
"/yolo"  => config::mode(app, Some("yolo"))
```

> 注：当前已有汉字别名（`jihua` → plan，`zidong` → yolo），英文直觉路径反而缺失。

### 2. 明确 `/auto` 的语义

- [ ] **方案 A**：`/auto` 作为「自动模型选择」的独立命令（对应 `app.auto_model = true`），并在 `/mode` 帮助文本中区分说明。
- [ ] **方案 B**：评估是否引入 `AppMode::Auto` 作为第四种模式（自动根据任务类型切换 Agent/Plan），对齐 Claude Code 的 `auto` 模式语义。

### 3. 更新帮助与自动补全

- [ ] `/mode` 的 usage 文本更新，列出所有别名入口
- [ ] TUI help 视图（`views/help.rs`）同步更新模式说明
- [ ] 自动补全候选词增加 `plan`、`auto`、`agent`、`yolo`

### 4. 文档更新

- [ ] `USER_GUIDE.md` 补充模式切换的快捷方式说明
- [ ] `CHANGELOG.md` 记录新增的 slash 命令别名

---

## 参考对比

| 工具 | 进入 Plan 模式 | 进入 Auto 模式 |
|------|---------------|----------------|
| Claude Code | `/plan` | `/auto` |
| Gemini CLI | `/plan` | `/auto` |
| **本项目（现状）** | `/mode plan` | ❌ 无对应入口 |
| **本项目（目标）** | `/plan` 或 `/mode plan` | `/auto` 或 `/mode auto` |

---

## 注意事项

- `/plan` 等新命令应作为 `/mode plan` 的**别名**，不重复实现逻辑，保持单一事实来源。
- 需确认 `/auto` 的最终语义（模型选择 vs 执行模式）后再实现，避免后续重构。
- 向后兼容：`/mode [agent|plan|yolo|1|2|3]` 原有路径继续有效，不废弃。

---

## [x] Issue #25: 确认并补全 /rewind 命令，对齐 Claude Code / Gemini 的回滚入口，降低用户迁移成本
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/25](https://github.com/XiaomingX/mimofan/issues/25)

### Description / Action Plan
## 背景

经代码审查，当前项目已具备**完整的回滚基础设施**，但入口路径与主流工具不一致。

---

### 现状盘点

#### ✅ 已有能力 1 — 工作区文件快照回滚（`/restore`）

实现位于 `crates/tui/src/commands/groups/skills/restore.rs` 和 `crates/tui/src/snapshot/`：

- 每次 turn 前后自动对工作区生成 `pre-turn:<seq>` / `post-turn:<seq>` git 快照（独立 side-repo，不污染用户自己的 `.git`）。
- 用户可通过 `/restore`（列出快照）或 `/restore N`（还原到第 N 个快照）回滚文件变更。
- 非 YOLO 模式下有安全拦截，保护未信任工作区。

#### ✅ 已有能力 2 — 对话历史回溯（`Esc-Esc` Backtrack）

实现位于 `crates/tui/src/tui/backtrack.rs`：

- `Esc → Esc` 二段快捷键触发对话回滚覆盖层，可逐步选择历史某条用户消息并 fork 对话线程。
- 设计上与文件快照解耦，仅回滚上下文，不还原文件。

#### ❌ 缺失 — `/rewind` slash 命令

Claude Code 和 Gemini 均提供 `/rewind` 作为「回到上一个操作点」的统一入口，用户迁移时会本能地输入 `/rewind`，当前只能得到「命令不存在」的错误。

---

## 对比分析

| 能力 | Claude Code | Gemini CLI | **本项目（现状）** | **本项目（目标）** |
|------|-------------|------------|-------------------|-------------------|
| 文件回滚 | `/rewind` | `/rewind` | `/restore N` ✅ | `/rewind [N]` 作为别名 |
| 对话回溯 | `/rewind` | `/rewind` | `Esc-Esc` 快捷键 ✅ | `/rewind` 亦触发对话回溯选择 |
| 统一入口 | ✅ | ✅ | ❌ | ✅ |

---

## 目标

新增 `/rewind` 作为现有回滚能力的**统一别名入口**，无需重复实现底层逻辑：

1. **`/rewind`（无参数）**：等同于 `/restore`，列出最近快照，同时提示 `Esc-Esc` 可进行对话回溯。
2. **`/rewind N`（数字参数）**：等同于 `/restore N`，直接还原到第 N 个快照。
3. **`/rewind chat`（可选）**：以 TUI 方式触发对话回溯覆盖层（等同 `Esc-Esc` 效果），方便纯键盘流用户。

---

## 改动范围

- [ ] 在 `crates/tui/src/commands/groups/skills/mod.rs` 注册 `rewind` 命令，路由至 `restore::restore()`
- [ ] 在 `CommandInfo` 中补充 `rewind` 的 name、description、usage 字段
- [ ] `/rewind` 无参数时的帮助文本同时说明文件快照回滚与 `Esc-Esc` 对话回溯两种路径
- [ ] TUI help 视图（`views/help.rs`）同步更新，列出 `/rewind` 入口
- [ ] 自动补全候选词增加 `rewind`
- [ ] `USER_GUIDE.md` 补充 `/rewind` 说明（含与 `/restore` 的关系）
- [ ] `CHANGELOG.md` 记录新增别名

---

## 注意事项

- **`/restore` 保持不变**：`/rewind` 是别名，不废弃、不重构现有命令，保持向后兼容。
- **语义边界**：`/rewind` 统一描述为「回到上一个操作点」，帮助文本中明确区分「文件快照回滚」与「对话上下文回溯」两种子能力，避免用户混淆。
- **安全策略继承**：`/rewind N` 继承 `/restore N` 的 YOLO/trust 检查逻辑，无需重新实现。

---

## [x] Issue #26: 确认并实现 /grill-me 命令，对齐 Antigravity 交互式需求澄清模式，降低用户迁移成本
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/26](https://github.com/XiaomingX/mimofan/issues/26)

### Description / Action Plan
## 背景

经代码审查，当前 TUI 中**不存在 `/grill-me` 命令**（全局 grep 无任何命中）。

该命令是 Antigravity（Google AGY CLI）中的核心交互模式，用于在执行复杂任务前通过结构化访谈帮助 Agent 完全理解用户意图，避免因需求模糊导致的返工。用户从 Antigravity 迁移到 mimofan 时，会本能地输入 `/grill-me`。

---

## 目标

实现 `/grill-me` 命令，语义与 Antigravity 保持一致：

> **用户主动在对话框输入 `/grill-me`，Agent 针对当前待执行任务发起逐步访谈，完全理解需求后输出结构化总结，用户确认后再正常执行。**

---

## 交互流程设计

```
用户: /grill-me
Agent: 好的，我来逐步澄清这个任务。
       ① [问题 1，附推荐答案]
用户: [回答 1]
Agent: ② [问题 2，附推荐答案]
用户: [回答 2]
      ... （直到所有关键分支澄清完毕）
Agent: ✅ 需求理解总结：
       - 目标：...
       - 约束：...
       - 期望输出：...
       确认后我将开始执行。[确认 / 继续调整]
用户: 确认
Agent: [正常执行任务]
```

**关键设计原则**：
1. **逐问逐答**：每次只问一个问题，避免信息过载。
2. **附推荐答案**：Agent 对每个问题给出自己的推荐选项，降低用户思考成本。
3. **可随时跳出**：用户可输入 `skip` 或 `done` 直接进入总结阶段。
4. **结构化总结**：访谈结束后 Agent 输出格式化需求摘要，用户确认后方可执行。

---

## 改动范围

- [ ] 新增 `crates/tui/src/commands/groups/core/grill.rs`，实现 `/grill-me` 命令逻辑
- [ ] 在 `crates/tui/src/commands/groups/core/core.rs` 中注册命令入口（name: `"grill-me"`，aliases: `["grill"]`）
- [ ] 实现访谈状态机：`Idle → Grilling { question_idx } → Summarizing → Done`
- [ ] Agent 在 `/grill-me` 触发后，基于当前 composer 草稿或上一条用户消息生成访谈问题树
- [ ] 访谈结束后将结构化需求总结写入 composer，等待用户确认后提交
- [ ] TUI help 视图（`views/help.rs`）同步更新，列出 `/grill-me` 说明
- [ ] 自动补全候选词增加 `grill-me`
- [ ] `USER_GUIDE.md` 补充 `/grill-me` 使用说明
- [ ] `CHANGELOG.md` 记录新增命令

---

## 参考对比

| 工具 | 交互式需求澄清命令 |
|------|--------------------|
| Antigravity (AGY) | `/grill-me` |
| **本项目（现状）** | ❌ 无对应命令 |
| **本项目（目标）** | `/grill-me` 或 `/grill` |

---

## 注意事项

- 访谈问题应由 LLM 动态生成，基于任务上下文，而非硬编码模板。
- 访谈状态需与当前 session 绑定，用户切换 tab 或新建 session 时不继承访谈状态。
- 不引入新的 `AppMode`：`/grill-me` 是一次性的交互流程，不是持久模式，执行完成后自动回到正常 Agent 模式。
- 与 `/plan` 模式区分：`/grill-me` 关注**需求澄清**（理解用户意图），`/plan` 关注**方案规划**（设计实现路径），两者可配合使用（先 `/grill-me` → 再 `/plan`）。

---

## [ ] Issue #27: 确认并实现 /simplify 命令：保持功能不变前提下提取公共函数、消除重复代码
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/27](https://github.com/XiaomingX/mimofan/issues/27)

### Description / Action Plan
## 背景

经代码审查，当前 TUI 中**不存在 `/simplify` 命令**（全局 grep 无任何命中）。

`/simplify` 是面向代码简化重构的 slash 命令，其核心语义是：

> **在不改变任何可观测行为的前提下，自动识别并提取代码中的重复逻辑为公共函数/方法，减少冗余，提升可维护性。**

该命令定位为 **纯重构命令**（语义安全性高），与 `/plan`（规划）、`/grill-me`（需求澄清）形成互补，覆盖「写完代码后的整理阶段」。

---

## 目标

实现 `/simplify` slash 命令，对齐主流 AI 编码工具（Claude Code、Gemini、Cursor 等）的代码简化工作流：

1. **提取公共函数**：识别跨文件/跨模块的重复代码片段，建议或自动提取为共享函数/方法。
2. **消除冗余逻辑**：合并相似的条件分支、简化嵌套结构、替换手写循环为惯用抽象。
3. **保证行为不变**：所有变更须通过现有测试套件验证，不引入任何语义变化。

---

## 交互流程设计

```
用户: /simplify [可选：指定文件或目录]
Agent: 正在分析代码库，识别重复模式...
       发现以下可简化点：
       ① crates/tui/src/a.rs:42 与 crates/tui/src/b.rs:87 存在相同逻辑，建议提取为 fn shared_xxx()
       ② crates/config/src/x.rs 中 parse_*() 系列函数有 3 处重复的错误处理模板，建议提取为宏或辅助函数
       ...
       是否应用以上重构？[全部应用 / 逐项确认 / 取消]
用户: 逐项确认
Agent: [逐条展示 diff，用户逐一批准]
Agent: ✅ 重构完成，运行测试验证行为一致性...
       测试通过，所有用例 green。
```

---

## 改动范围

### 实现方式选项（二选一，待讨论）

**方案 A：Skill 实现（推荐，低侵入）**
- 新增 `crates/tui/assets/skills/simplify/SKILL.md`，定义 `/simplify` 技能的 prompt 和工具调用策略
- 利用现有 skill 系统和 `run_skill_by_name` 路径注册，无需改动命令注册层
- 与现有 `v4-best-practices`、`skill-creator` 等内置 skill 并列

**方案 B：原生 slash 命令实现（重量级）**
- 新增 `crates/tui/src/commands/groups/core/simplify.rs`
- 实现静态分析 + LLM 辅助的重复代码检测逻辑
- 注册到命令路由

### 公共改动（无论哪种方案）
- [ ] TUI help 视图（`views/help.rs`）同步更新
- [ ] 自动补全候选词增加 `simplify`
- [ ] `USER_GUIDE.md` 补充 `/simplify` 使用说明
- [ ] `CHANGELOG.md` 记录新增命令

---

## 参考对比

| 工具 | 代码简化/重构命令 |
|------|-----------------|
| Claude Code | `/simplify` |
| Gemini CLI | `/simplify` |
| Cursor | AI 重构建议（内联触发） |
| **本项目（现状）** | ❌ 无对应入口 |
| **本项目（目标）** | `/simplify [path]` |

---

## 注意事项

- **行为安全性优先**：`/simplify` 必须在 Agent 模式（非 Yolo）下运行，每条重构建议均需用户确认，防止静默修改破坏逻辑。
- **测试门禁**：重构完成后 Agent 应自动触发 `run_tests`，仅当测试全绿时才报告成功；测试失败需自动回滚或提示用户。
- **范围控制**：支持 `/simplify`（全项目）和 `/simplify <path>`（指定文件/目录）两种粒度，避免大项目下全量分析超出上下文窗口。
- **与 `/plan` 的区分**：`/simplify` 是**纯重构**，不涉及需求变更；`/plan` 是**功能规划**，涉及新行为设计。两者定位清晰，不重叠。

---

## [ ] Issue #28: 补全 /code-review 安全审计能力：识别安全漏洞并生成修复计划，对齐 Claude Code / Gemini 入口
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/28](https://github.com/XiaomingX/mimofan/issues/28)

### Description / Action Plan
## 背景

经代码审查，项目中**已存在 `/review` 命令**（`crates/tui/src/commands/groups/skills/review.rs`），但其定位是**通用代码审查**，存在以下缺口：

### 现状盘点

| 能力 | 现状 |
|------|------|
| `/review <target>` slash 命令 | ✅ 已有，触发 review skill |
| `code_reviewer.md` 系统提示 | ✅ 已有，输出 JSON 格式的 issues/suggestions |
| `review` 内置 tool（子智能体） | ✅ 已有，别名覆盖 `code-review` / `code_review` / `reviewer` |
| **安全漏洞专项扫描** | ❌ 缺失，现有 prompt 无安全专项指令 |
| **自动生成修复计划** | ❌ 缺失，仅输出 JSON 诊断，无后续修复 action |
| **`/code-review` 独立 slash 命令** | ❌ 缺失，用户需输入 `/review`，与主流工具入口不一致 |

---

## 目标

在现有 `/review` 基础上，增强为具备安全审计与修复闭环能力的 `/code-review` 命令，对齐 Claude Code / Gemini 的安全审查工作流：

1. **`/code-review`（别名）**：新增为 `/review` 的入口别名，消除用户迁移成本。
2. **安全专项扫描**：强化 `code_reviewer.md` prompt，增加安全漏洞检测维度（OWASP Top 10、Rust 不安全代码、secret 泄漏、依赖漏洞等）。
3. **修复闭环**：发现安全问题后，Agent 自动制定修复计划并在用户确认后执行修复。

---

## 交互流程设计

```
用户: /code-review [可选：文件/目录]
Agent: 正在扫描安全风险与代码质量问题...

       🔴 高危 [2 项]
       ① src/tui/oauth.rs:47 — access_token 硬编码写入日志，存在 secret 泄漏风险
       ② crates/config/src/lib.rs:123 — 反序列化外部输入未做长度限制，存在 DoS 风险

       🟡 中危 [3 项]
       ③ ...

       🟢 低危 / 建议 [5 项]
       ④ ...

       是否制定修复计划？[是 / 仅查看 / 取消]
用户: 是
Agent: 修复计划：
       ① 移除 oauth.rs:47 的 token 日志输出，改用 [REDACTED] 占位符
       ② 为 lib.rs:123 的反序列化添加 max_size 限制
       ...
       [全部应用 / 逐项确认]
用户: 逐项确认
Agent: [逐条展示 diff，用户逐一批准后执行]
Agent: ✅ 修复完成，运行测试验证...测试全绿。
```

---

## 改动范围

### 1. 新增 `/code-review` 别名

在 `crates/tui/src/commands/groups/skills/mod.rs` 中注册：
```rust
"code-review" | "code_review" => review::review(app, arg),
```

### 2. 强化 `code_reviewer.md` 安全扫描维度

在现有 JSON schema 基础上扩展 `security_issues` 字段，覆盖：
- **Secret 泄漏**：硬编码 token/密钥/凭据
- **不安全依赖**：`Cargo.lock` 中已知漏洞版本（可联动 `cargo audit`）
- **Rust unsafe 代码**：`unsafe` 块的合理性审查
- **输入验证缺失**：外部输入未做边界检查
- **OWASP Top 10**：注入、认证、权限等通用安全类别

### 3. 修复闭环能力

- [ ] review 结果输出后，提示用户「是否制定修复计划」
- [ ] Agent 基于 review 结果生成结构化修复步骤（调用现有 `edit_file`/`apply_patch` 工具）
- [ ] 修复完成后自动触发 `run_tests` 门禁
- [ ] 修复失败时自动回滚（利用现有 snapshot 机制）

### 4. 公共改动

- [ ] TUI help 视图更新，列出 `/code-review` 与 `/review` 的关系说明
- [ ] 自动补全候选词增加 `code-review`
- [ ] `USER_GUIDE.md` 补充安全审计工作流说明
- [ ] `CHANGELOG.md` 记录改动

---

## 参考对比

| 工具 | 安全/代码审查命令 | 修复闭环 |
|------|-----------------|---------|
| Claude Code | `/code-review` | ✅ |
| Gemini CLI | `/code-review` | ✅ |
| **本项目（现状）** | `/review <target>` | ❌ 仅诊断 |
| **本项目（目标）** | `/code-review` 或 `/review` | ✅ 诊断 + 修复计划 + 执行 |

---

## 注意事项

- **`/review` 保持不变**：`/code-review` 作为别名，向后兼容，原有用法不受影响。
- **安全扫描非破坏性**：扫描阶段只读，修复阶段需用户明确确认方可写入。
- **依赖 `cargo audit`（可选）**：若检测到 `cargo audit` 可用，自动联动扫描 `Cargo.lock` 中的已知 CVE；不可用时降级为纯 LLM 静态分析。
- **范围控制**：`/code-review`（全项目）/ `/code-review <path>`（指定范围），防止超出上下文窗口。

---

## [ ] Issue #29: 确认并实现 /make-plan 与 /do 命令，对齐 Claude-mem 的「规划-执行」分离工作流
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/29](https://github.com/XiaomingX/mimofan/issues/29)

### Description / Action Plan
## 背景

经代码审查，项目中**已具备强大的规划与执行基础设施**，但缺失与 Claude-mem 对齐的用户侧 slash 命令入口。

---

### 现状盘点

| 组件 | 状态 | 位置 |
|------|------|------|
| `plan` 内部工具（`update_plan`） | ✅ 已有 | `crates/tui/src/tools/plan.rs` |
| `todo` 内部工具（任务列表） | ✅ 已有 | `crates/tui/src/tools/todo.rs` |
| `AppMode::Plan`（规划模式） | ✅ 已有，含 Plan Confirmation 弹窗 | `tui/src/tui/app.rs` |
| `/goal`（持久目标循环 / WhaleFlow） | ✅ 已有 | `commands/groups/project/mod.rs` |
| **`/make-plan`** | ❌ **缺失** | — |
| **`/do`** | ❌ **缺失** | — |

---

## Claude-mem 语义参考

Claude-mem 中的「规划-执行」分离工作流：

| 命令 | 语义 |
|------|------|
| `/make-plan <任务描述>` | Agent 生成结构化执行计划（含步骤列表、依赖关系、风险项），**仅规划不执行**，结果持久化到会话/文件 |
| `/do [步骤编号 / all]` | 根据已有计划**执行**指定步骤或全部步骤，支持断点续做 |

核心价值：**「想清楚再动手」** — 先用 `/make-plan` 让用户确认方案，再用 `/do` 分批执行，避免 Agent 在未理清思路时直接修改代码。

---

## 目标

新增 `/make-plan` 与 `/do` 命令，复用现有基础设施，提供对齐主流工具的「规划-执行」分离入口：

### `/make-plan <任务描述>`

1. 切换到 `AppMode::Plan`（只读规划模式）
2. Agent 基于任务描述生成结构化计划（调用现有 `update_plan` tool）
3. 弹出现有 Plan Confirmation 弹窗（`tui/src/tui/plan_prompt.rs`）供用户审阅
4. 计划持久化到会话 todo 列表（`todo` tool）

### `/do [step_id|all]`

1. 读取当前会话中已有的 todo/plan 列表
2. 切换到 `AppMode::Agent`，执行指定步骤（或全部）
3. 逐步更新 todo 状态（pending → in_progress → completed）
4. 执行完成后输出摘要

---

## 交互流程设计

```
# 阶段一：规划
用户: /make-plan 将配置系统从 TOML 迁移到 JSON
Agent: 正在生成执行计划（Plan 模式，不执行任何写操作）...

       📋 迁移计划 v1
       ① [pending] 移除 Cargo.toml 中的 toml 依赖
       ② [pending] 重写 crates/config/src/lib.rs 解析逻辑（toml → serde_json）
       ③ [pending] 更新配置路径 ~/.mimofan/settings.toml → ~/.mimo/setting.json
       ④ [pending] 编写迁移工具，检测旧配置并自动转换
       ⑤ [pending] 运行测试套件验证行为一致性

       接受计划？[接受 / 修改 / 取消]
用户: 接受

# 阶段二：执行（可分批）
用户: /do 1
Agent: [执行步骤 ①] 移除 toml 依赖...完成 ✅
       计划进度：1/5 (20%)

用户: /do all
Agent: [执行步骤 ②③④⑤] 依次执行...
       ✅ 迁移完成，所有测试通过。
```

---

## 改动范围

### 1. 新增 `/make-plan` 命令

- [ ] 在 `crates/tui/src/commands/groups/project/` 新增 `make_plan.rs`
- [ ] 触发 `AppMode::Plan` + `update_plan` tool + Plan Confirmation 弹窗
- [ ] 注册命令（name: `make-plan`，aliases: `["makeplan", "mp"]`）

### 2. 新增 `/do` 命令

- [ ] 新增 `crates/tui/src/commands/groups/project/do_cmd.rs`
- [ ] 读取当前 todo 列表，切换 `AppMode::Agent`，按步骤分发执行
- [ ] 支持 `/do all`（全部）/ `/do <N>`（指定步骤编号）/ `/do next`（下一个 pending）
- [ ] 注册命令（name: `do`，注意与 Rust 关键字冲突，文件名用 `do_cmd`）

### 3. 公共改动

- [ ] TUI help 视图同步更新
- [ ] 自动补全候选词增加 `make-plan`、`do`
- [ ] `USER_GUIDE.md` 补充「规划-执行」工作流说明
- [ ] `CHANGELOG.md` 记录新增命令

---

## 参考对比

| 工具 | 规划命令 | 执行命令 |
|------|---------|---------|
| Claude-mem | `/make-plan` | `/do` |
| Antigravity (AGY) | `/plan` | `/goal` |
| **本项目（现状）** | `/mode plan`（切换模式，无参数规划） | `/goal`（持久目标循环） |
| **本项目（目标）** | `/make-plan <描述>` | `/do [all\|N\|next]` |

---

## 注意事项

- **`/make-plan` ≠ `/plan` 模式**：`/plan` 是持久的 AppMode，`/make-plan` 是一次性的计划生成命令，生成后自动退出 Plan 模式。
- **`/do` ≠ `/goal`**：`/goal` 是目标驱动的自主循环（WhaleFlow），`/do` 是用户显式触发的步进式执行，粒度更细、更可控。
- **状态持久化**：`/make-plan` 生成的 todo 列表需与 session 绑定，`/do` 跨多轮对话时仍可续做。
- **`do` 关键字冲突**：Rust 中 `do` 是保留关键字，模块文件命名为 `do_cmd.rs`，命令注册 name 字段用字符串 `"do"` 无影响。

---

## [ ] Issue #30: 【长程任务一致性】排查现有问题与竞品差距，制定优化待办
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/30](https://github.com/XiaomingX/mimofan/issues/30)

### Description / Action Plan
## 目标视角

> **在保证正确性的前提下，提升长程任务（多轮、多 turn、跨 session）的执行一致性。**

---

## 现状盘点（代码已有机制）

经代码审查，项目已具备以下长程一致性基础设施：

| 机制 | 实现位置 | 状态 |
|------|---------|------|
| 上下文压缩（Compaction） | `src/compaction.rs` | ✅ 默认开启，token-only 触发（v0.8.11 升级） |
| Goal Loop（持久目标循环） | `src/goal_loop.rs` | ✅ 无 continuation cap，run-until-done |
| Prefix Cache 稳定性管理 | `src/prefix_cache.rs` | ✅ SHA-256 指纹，检测 system/tool drift |
| 工作区快照（Snapshot） | `src/snapshot/` | ✅ 每 turn pre/post 快照，side-repo 隔离 |
| Slop Ledger（技术债追踪） | `src/slop_ledger.rs` | ✅ 跨 session 残留问题可见性 |
| Retry 机制 | `src/retry_status.rs` | ✅ HTTP 层自动重试，TUI 显示倒计时 |
| Plan Tool + Todo Tool | `src/tools/plan.rs` + `todo.rs` | ✅ 步骤追踪，状态机（pending/in_progress/completed） |
| Memory Service（外部长期记忆） | `src/memory_service.rs` | ✅ 集成 claude-mem，跨 session 记忆 |
| Verifier 运行 | `src/tools/`（run_verifiers） | ✅ 后台验证，已知并发不稳定（预存在问题） |

---

## 识别出的问题与差距

### 🔴 P0 — 严重

**1. Compaction 后目标漂移无检测机制**
- 压缩摘要由 LLM 生成，存在语义损失风险。压缩后 Agent 可能「忘记」原始目标约束（如「不修改测试文件」「保持 API 兼容」）。
- Claude Code / Gemini：压缩摘要中强制保留「当前目标」和「已确认约束」字段，作为 system-level anchor。
- **差距**：当前 `CompactionConfig` 无目标锚点保留机制。

**2. Plan/Todo 状态与 Goal Loop 未深度集成**
- Goal Loop 在 continuation 决策时未读取 Todo 列表状态，无法感知「步骤 ③ 已完成，跳至 ④」。
- 实际表现：长程任务中 Agent 可能重复执行已完成步骤，或在 compaction 后丢失进度感知。
- **差距**：Claude Code 的 task loop 与 plan state 强绑定，每次 continuation 检查 todo 状态。

---

### 🟡 P1 — 重要

**3. Prefix Cache 漂移检测有但无自动修复**
- `PrefixStabilityManager` 能检测 system prompt / tool list 漂移，但仅发出事件，未触发自动稳定化（如回滚到 pinned 版本）。
- **差距**：Gemini 在检测到 prefix 变化时自动重新 pin，保证 cache 命中率稳定。

**4. 子智能体结果一致性无校验**
- 子智能体完成任务后，父 Agent 直接信任返回结果，未验证其是否与原始目标一致（如文件确实被修改、测试确实通过）。
- **差距**：Claude Code 在 sub-agent 完成后执行「claimed side effects verification」（已在 AGENTS.md 中提及）。

**5. Session 重连后 Goal 状态恢复不完整**
- 用户关闭 TUI 后重新打开，Goal Loop 的历史进度无法完整恢复，只能重新描述目标。
- **差距**：Codex 支持 session checkpoint，重连后从断点继续。

---

### 🟢 P2 — 改进

**6. Verifier 并发不稳定（已知预存在问题）**
- `run_verifiers_background_*` 测试在完整套件并行时不稳定（AGENTS.md 已记录）。
- 根因：verifier 与快照/文件系统操作存在竞争条件，需隔离测试沙箱。

**7. 长程任务中 context_budget 压力未主动告警**
- `ContextBudget` 模块已有 `PressureLevel`（Low/Medium/High/Critical），但 High/Critical 时未触发主动 compaction 建议或自动 compact。
- **差距**：Claude Code 在 70% 时主动建议，90% 时强制 compact。

**8. Slop Ledger 未与 Goal Loop 集成**
- 技术债条目记录后，Goal Loop 不感知这些残留，可能在下一个任务中重复踩坑。
- **差距**：需在每次 turn 开始时将相关 slop 条目注入 system context。

---

## 与竞品差距汇总

| 能力维度 | mimofan | Claude Code | Gemini | Codex |
|---------|---------|-------------|--------|-------|
| Compaction 后目标锚点保留 | ❌ | ✅ | ✅ | ❌ |
| Todo 状态与 Goal Loop 集成 | ❌ | ✅ | 部分 | ❌ |
| Prefix Cache 自动稳定化 | ❌（仅检测） | ✅ | ✅ | N/A |
| Sub-agent 结果验证 | ❌ | ✅ | 部分 | ✅ |
| Session 断点续做 | ❌ | ✅ | ❌ | ✅ |
| Context 压力主动告警 + 自动 compact | 部分 | ✅ | ✅ | ❌ |
| 长期记忆跨 session 注入 | ✅（memory_service） | ✅ | ✅ | ❌ |

---

## 优化计划待办

- [ ] **[P0]** Compaction 摘要中强制保留「当前目标 + 已确认约束」anchor 字段（`compaction.rs`）
- [ ] **[P0]** Goal Loop continuation 决策读取 Todo 状态，跳过已完成步骤（`goal_loop.rs` + `tools/todo.rs`）
- [ ] **[P1]** Prefix Cache 漂移检测后自动重新 pin，保证 cache 命中率（`prefix_cache.rs`）
- [ ] **[P1]** Sub-agent 完成后执行 claimed side effects 校验（`tools/subagent/mod.rs`）
- [ ] **[P1]** Goal 状态持久化到 `~/.mimo/` 目录，支持 session 重连后断点续做
- [ ] **[P2]** Context pressure High/Critical 时自动触发 compaction 建议（`context_budget.rs` → TUI）
- [ ] **[P2]** Verifier 测试沙箱隔离，解决并发不稳定问题（`run_verifiers_background_*`）
- [ ] **[P2]** 每次 turn 开始时将相关 Slop Ledger 条目注入 system context（`slop_ledger.rs` → `prompts.rs`）

---

## [ ] Issue #31: 【降低 Token 浪费】保证效果的前提下，排查现有问题与竞品差距，制定优化待办
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/31](https://github.com/XiaomingX/mimofan/issues/31)

### Description / Action Plan
## 目标视角

> **在保证任务效果（输出质量、功能正确性、用户体验）的前提下，降低每轮交互及长程任务中不必要的 token 消耗（浪费）。**

「保证效果」强调：节约 token 不能以牺牲代码质量、答复准确性、工具调用成功率为代价——所有优化须在效果不降的约束下进行。

---

## 现状盘点（代码已有机制）

经代码审查，项目已具备以下 token 节约基础设施：

| 机制 | 实现位置 | 效果 |
|------|---------|------|
| Prefix Cache 稳定管理 | `src/prefix_cache.rs` | system + tool 前缀 SHA-256 指纹，保证 DeepSeek 自动缓存命中 |
| 上下文压缩（Compaction） | `src/compaction.rs` | 超阈值时 LLM 摘要替换历史消息，token-only 触发（80% window） |
| 大输出路由（Large Output Router） | `src/tools/large_output_router.rs` | 工具输出 >4096 token 时由 Flash 子智能体摘要，仅摘要进入父上下文 |
| Tool Result 去重 | `src/tools/tool_result_retrieval.rs` | 重复工具结果标记 deduplicated，避免相同内容多次进入上下文 |
| Context Budget 数学 | `src/context_budget.rs` | 统一 budget 推导，防止 output reservation 使 input 下溢 |
| Token Estimate Cache | `src/core/engine/token_estimate_cache.rs` | 复用 token 估算结果，减少重复计算开销 |
| Auto Model Routing | `src/tui/auto_router.rs` | 自动为简单任务选择 Flash（低成本）而非 Opus/Sonnet |
| Slop Ledger | `src/slop_ledger.rs` | 追踪技术残留，防止 Agent 重复处理相同问题 |

---

## 识别出的问题与差距

### 🔴 P0 — 严重 Token 浪费（且影响效果）

**1. Compaction 摘要使用主模型，浪费 quota 且摘要质量不稳定**
- 压缩时 `CompactionConfig.model` 默认使用 `DEFAULT_TEXT_MODEL`，大模型做摘要反而引入幻觉风险（摘要错误丢失关键上下文）。
- **差距**：Claude Code 强制用最小模型做 compaction，成本低且摘要质量稳定（小模型在摘要任务上不逊于大模型）；Gemini 专用 Flash 摘要模型。
- **效果影响**：摘要错误 → 后续 turn Agent 基于错误上下文决策 → 任务失败。

**2. Prefix cache 漂移无法自动修复，cache miss 代价无上限**
- `prefix_cache.rs` 检测漂移后仅发出事件，未触发重新 pin，导致后续每轮 cache miss，重复传输数千 token 的 system prompt + tool schema。
- **效果影响**：cache miss 不影响功能正确性，但延迟增加，成本上升。
- **差距**：Claude Code 使用 `cache_control` 标记（Anthropic API 原生支持），确保静态前缀持久缓存；Gemini 使用 server-side caching API（explicit TTL）。

**3. 工具输出大量重复进入上下文，压缩模型重复处理相同内容**
- 多轮任务中，同一文件被多次 `read_file`，内容重复积累无跨轮去重。
- Large Output Router 仅覆盖单次 >4096 token 的输出，中等大小（1000~4096 token）的重复文件读取无保护。
- **效果影响**：重复内容占用上下文 → 压缩时相同内容被多次摘要 → 摘要失真风险升高。

---

### 🟡 P1 — 显著浪费（效果中性或轻微影响）

**4. Plan 模式下仍发全量工具描述（含被禁用工具的 schema）**
- `AppMode::Plan` 禁止 shell 和写操作，但工具列表完整发送，模型需在 schema 中「识别」哪些被禁用，认知负担增加。
- **差距**：Claude Code 在 Plan 模式下裁剪工具列表（只读工具），节约 20-30% tool schema token，同时降低模型调用禁用工具的概率。
- **效果影响**：工具列表过长 → 模型偶发调用被拦截的工具 → 用户看到错误提示，体验下降。

**5. 子智能体 system prompt 与父 Agent 高度重复**
- 每个子智能体携带独立完整 system prompt（含 constitution.md），多子智能体并行时重复 token 乘以数量。
- **效果影响**：重复 prompt 不降低效果，但多子智能体场景下成本线性增长，制约 fleet 规模。
- **差距**：Codex 使用共享 system prompt 池；Gemini 隐式 context sharing。

**6. Auto Model Routing 上下文仅 6 条消息，路由准确率有限**
- `auto_router.rs` 路由决策仅基于最近 6 条消息，对长程任务的任务类型判断不准。
- **效果影响**：复杂任务被路由到 Flash → 输出质量下降；简单任务路由到 Opus → 成本浪费。两种错误都影响效果或成本。
- **差距**：Claude Code 路由使用完整 task description + 历史摘要，准确率更高。

**7. 会话级文件内容无指纹去重**
- 同一会话多次 `read_file` 同一文件，无「文件已在上下文」感知，内容重复注入。
- **差距**：需建立会话级「文件内容哈希 → 首次注入 turn 编号」索引，后续引用改为指针，减少重复内容。

---

### 🟢 P2 — 可改进（效果不变，纯节约）

**8. Token 消耗无细粒度可观测性，无法定向优化**
- 当前仅整体 cost_status，无法区分 system prompt / tool schema / history / output 各自占比。
- **效果影响**：无可观测性 → 无法识别哪个维度浪费最多 → 优化方向盲目。
- **差距**：Claude Code `/cost` 命令输出细粒度 token breakdown；Gemini 提供 token attribution dashboard。

**9. Reasoning Effort 与任务复杂度未动态匹配**
- `ReasoningEffort` 设置后全程固定，`/restore`、`/mode` 切换等简单命令仍消耗高思考力 token。
- **效果影响**：简单任务用高 effort → 浪费 thinking token，但结果不变；复杂任务用低 effort → 效果下降（需防止此情况）。
- **差距**：Claude Code 根据任务分类动态调整 effort（非代码类任务自动 low）。

**10. Compaction 触发阈值静态，未感知剩余任务量**
- 阈值固定 80% 窗口，不考虑「任务还剩多少步」——近结尾任务可直接完成无需压缩，早期任务应更早压缩保留余量。
- **效果影响**：过早压缩 → 丢失仍需引用的上下文 → 任务失败；过晚压缩 → 窗口溢出 → 被迫截断。

---

## 效果保证约束（优化边界）

> 以下红线任何优化方案均不得触碰：

| 约束 | 说明 |
|------|------|
| 代码质量不降 | 节约 token 不能导致 Agent 产出质量下降（如代码错误率升高） |
| 工具调用成功率不降 | 工具 schema 裁剪后，模型仍须能正确调用所有可用工具 |
| 答复准确性不降 | Compaction / 摘要不能丢失影响后续决策的关键信息 |
| 用户体验不降 | 延迟、错误提示频率不能因节约措施而增加 |

---

## 与竞品差距汇总

| Token 节约维度 | mimofan | Claude Code | Gemini | Codex |
|--------------|---------|-------------|--------|-------|
| Prefix cache 稳定性（漂移后自动修复） | ❌（检测无修复） | ✅（cache_control） | ✅（server-side TTL） | ❌ |
| Compaction 使用轻量模型 | ❌（同主模型） | ✅（最小模型） | ✅（Flash 专用） | ❌ |
| 工具输出跨轮去重 | 部分（单轮） | ✅（working set） | 部分 | ❌ |
| Plan 模式工具列表裁剪 | ❌ | ✅ | 部分 | ❌ |
| 子智能体 prompt 去重 | ❌ | 部分 | ✅ | ✅ |
| 文件内容指纹去重 | ❌ | ✅ | ❌ | ❌ |
| Token breakdown 可观测性 | ❌ | ✅ | ✅ | ❌ |
| Effort 动态匹配任务复杂度 | ❌ | ✅ | 部分 | ❌ |
| 动态 compaction 触发时机 | ❌ | 部分 | ❌ | ❌ |

---

## 优化计划待办（按优先级）

- [ ] **[P0]** Compaction 强制使用轻量 Flash 模型（`compaction.rs` `CompactionConfig.model` 默认值改为 flash）
- [ ] **[P0]** 为 system prompt 和工具描述增加显式 `cache_control` 标记，prefix 漂移后快速重建缓存（`prompts.rs` + `core/engine/context.rs`）
- [ ] **[P0]** 会话级文件内容哈希索引，避免同文件多次 `read_file` 重复注入（扩展 `working_set.rs`）
- [ ] **[P1]** Plan 模式下裁剪工具描述，只发送只读工具 schema（`core/engine/tool_catalog.rs`）
- [ ] **[P1]** 子智能体继承父级 system prompt prefix，减少重复 token（`tools/subagent/mod.rs`）
- [ ] **[P1]** Auto Model Routing 扩展上下文窗口（6条 → 完整任务描述 + 历史摘要），提升路由准确率
- [ ] **[P1]** Large Output Router 阈值可配置，扩展至中等大小（1000~4096 token）重复工具结果的跨轮去重
- [ ] **[P2]** 新增 `/cost` 命令，输出细粒度 token breakdown（system/tools/history/output 分项）
- [ ] **[P2]** Reasoning Effort 根据任务分类自动降级（非代码类命令 → low effort），并防止复杂任务误降级
- [ ] **[P2]** Goal 进度感知的动态 compaction 触发阈值（末期放宽，初期收紧）

---

## [x] Issue #32: 补全 Cmd+Click 打开 .md 文档能力，对齐 Claude Code / Gemini 的文件直达交互
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/32](https://github.com/XiaomingX/mimofan/issues/32)

### Description / Action Plan
## 背景

经代码审查，项目**已具备相关基础设施**，但 Cmd+Click 直接打开 `.md` 文件的完整路径尚不通畅。

---

## 现状盘点

| 能力 | 状态 | 位置 |
|------|------|------|
| OSC 8 超链接渲染 | ✅ 已有 | `tui/src/tui/osc8.rs`，现代终端（iTerm2、Warp、Ghostty 等）Cmd+Click 可跳转 URL |
| `try_open_file_at_line`（编辑器打开文件） | ✅ 已有 | `tui/src/tui/history.rs:1867`，识别 `path:line` 模式，调用 `$VISUAL`/`$EDITOR`/vim |
| `.md` 扩展名识别 | ✅ 已有 | `looks_like_file_path` 已含 `"md"` 匹配 |
| 右键上下文菜单 → "Open file at line" | ✅ 已有 | `mouse_ui.rs:905`，`ContextMenuAction::OpenFileAtLine` |
| **Cmd+Click 直接触发打开** | ❌ **缺失** | 无鼠标事件修饰键检测路径 |
| **纯文件路径（无 `:line`）的 `.md` 打开** | ❌ **缺失** | `try_open_file_at_line` 要求 `path:line` 格式，纯路径如 `README.md` 无法触发 |
| **OSC 8 链接绑定本地文件路径** | ❌ **缺失** | 当前 OSC 8 主要用于 URL，本地 `file://` 路径未系统集成 |

---

## 与竞品对比

| 交互方式 | Claude Code | Gemini CLI | **本项目（现状）** | **本项目（目标）** |
|---------|-------------|------------|-------------------|-------------------|
| Cmd+Click 打开文件 | ✅ | ✅ | ❌（需右键菜单） | ✅ |
| 纯路径（无行号）打开 | ✅ | ✅ | ❌ | ✅ |
| `.md` 文件优先用 Markdown 渲染器预览 | ✅ | ❌（用编辑器） | ❌ | 可选 |
| OSC 8 `file://` 本地超链接 | ✅ | ✅ | 部分 | ✅ |

---

## 目标

在现有 OSC 8 + `try_open_file_at_line` 基础上，补全 **Cmd+Click 直接打开 `.md` 文档**的完整交互路径，对齐 Claude Code / Gemini 的文件直达体验。

---

## 交互流程设计

**场景 1：转录区消息中出现 `.md` 文件路径**
```
Agent: 请参考 docs/CONTRIBUTING.md 中的规范...
       ^^^^^^^^^^^^^^^^^^^^^ ← Cmd+Click → 用 $EDITOR 打开该文件
```

**场景 2：OSC 8 超链接（现代终端）**
```
Agent: [README.md](file:///path/to/README.md)
       ← 终端原生 Cmd+Click → 用 $EDITOR 或 open 打开
```

**场景 3：无终端 OSC 8 支持时的降级**
```
- 鼠标左键单击文件路径文本 → 高亮选中
- 右键菜单 → "Open file" （现有路径，保持不变）
```

---

## 改动范围

### 1. 鼠标事件修饰键检测（核心）

在 `tui/src/tui/mouse_ui.rs` 的左键点击处理中，增加修饰键判断：

```rust
MouseEventKind::Down(MouseButton::Left) => {
    // Cmd+Click (macOS) 或 Ctrl+Click (通用) 触发文件打开
    if mouse.modifiers.contains(KeyModifiers::SUPER)   // macOS Cmd
        || mouse.modifiers.contains(KeyModifiers::CONTROL) // 通用 Ctrl
    {
        let text = get_text_at_mouse_pos(app, mouse);
        if try_open_file_at_pos(&text, &app.workspace) {
            app.status_message = Some("Opened file".to_string());
            return true;
        }
    }
    // ... 原有逻辑
}
```

### 2. 扩展 `try_open_file_at_line` 支持纯路径

在 `tui/src/tui/history.rs` 中，扩展匹配逻辑：
- 当前：仅匹配 `path:line` 格式
- 目标：同时匹配纯路径（如 `README.md`、`docs/CONTRIBUTING.md`），行号默认为 1

### 3. OSC 8 本地文件路径集成

在 Markdown 渲染管道中，将 `[text](relative/path.md)` 类型的本地链接转换为 `file://` URL 再注入 OSC 8，使现代终端原生 Cmd+Click 可用。

### 4. 待办清单

- [ ] `tui/src/tui/mouse_ui.rs`：增加 Cmd+Click 修饰键检测分支
- [ ] `tui/src/tui/history.rs`：`try_open_file_at_line` 扩展支持纯路径（无 `:line` 后缀）
- [ ] Markdown 渲染管道：本地相对路径转换为 `file://` URL 注入 OSC 8
- [ ] `USER_GUIDE.md`：说明 Cmd+Click 打开文件的使用方式
- [ ] 补充单元测试（`looks_like_file_path` + 路径解析逻辑）

---

## 注意事项

- **安全边界**：仅允许打开工作区目录内的路径，防止任意文件读取（`workspace.join(path).starts_with(workspace)` 校验）。
- **降级策略**：不支持鼠标修饰键的终端（如 tmux 部分配置），右键菜单 "Open file" 保持不变作为兜底。
- **`.md` 优先级**：默认用 `$EDITOR` 打开，与其他文件一致；可通过 `open_md_with_preview = true` 配置项开启 TUI 内预览（可选，后续迭代）。
- **crossterm 兼容性**：`MouseEvent.modifiers` 在 macOS 下携带 `SUPER`（Cmd）标志需在 iTerm2、Warp、Ghostty 下各自验证；同时提供 `Ctrl+Click` 作为通用备选。


---

## [ ] Issue #33: 【启动性能】分析启动时序，识别可后移操作，提升感知启动速度，对齐 Claude Code / Gemini
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/33](https://github.com/XiaomingX/mimofan/issues/33)

### Description / Action Plan
## 背景与目标

> **分析当前项目启动时序，识别可后移的阻塞操作，让用户感受到更快的启动速度（首帧可见时间 < 200ms）。**

参考竞品 Claude Code / Gemini 的启动优化策略，降低用户迁移认知成本。

---

## 当前启动时序分析（代码实证）

经代码审查（`crates/tui/src/lib.rs` 约 L5900 区域），当前启动路径按顺序执行以下操作：

| 步骤 | 操作 | 类型 | 是否阻塞首帧 | 可否后移 |
|------|------|------|------------|---------|
| 1 | `Config::load()`：读取并解析配置文件 | 磁盘 I/O | ✅ 阻塞 | ❌ 必须（依赖 config） |
| 2 | `ensure_config_file_exists()`：首次运行建配置文件 | 磁盘 I/O | ✅ 阻塞 | ⚠️ 可异步化 |
| 3 | `Settings::load()`：加载 bracketed_paste 等 TUI 设置 | 磁盘 I/O | ✅ 阻塞 | ⚠️ 可用默认值先渲染 |
| 4 | `skills::install_system_skills()`：安装内置 skill（首次） | 磁盘 I/O + 文件复制 | ✅ 阻塞 | ✅ **可后移** |
| 5 | `session_manager::prune_workspace_snapshots()`：清理 7 天前快照 | 磁盘 I/O + git | ✅ 阻塞 | ✅ **可后移** |
| 6 | `tools::truncate::prune_older_than()`：清理 spillover 文件 | 磁盘 I/O | ✅ 阻塞 | ✅ **可后移** |
| 7 | `session_manager.cleanup_old_sessions()`：清理旧 session | 磁盘 I/O | ✅ 阻塞 | ✅ **可后移** |
| 8 | `tui::run_tui()`：启动 TUI 事件循环 | 渲染 | — | — |
| 9 | （首轮）`SkillRegistry::discover()`：扫描 skills 目录 | 磁盘 I/O | ✅ 阻塞首轮响应 | ✅ **可懒加载** |
| 10 | （首轮）model catalog / routing inventory | 网络或缓存 | ✅ 阻塞首轮响应 | ✅ **可后台预热** |

---

## 识别出的问题

### 🔴 P0 — 直接影响感知启动速度

**1. 三个 prune 操作串行阻塞主线程**
- `prune_workspace_snapshots` + `prune_older_than` + `cleanup_old_sessions` 在 `run_tui()` 调用前串行执行。
- 快照 prune 涉及 `git` 进程调用（`~10ms/次`，但积累多个工作区后明显），session cleanup 涉及遍历 `~/.mimofan/sessions/` 目录。
- **差距**：Claude Code 将所有 prune 操作放入后台 tokio 任务，首帧渲染完成后才执行；Gemini 使用 `spawn_blocking` + 低优先级线程。

**2. `install_system_skills` 阻塞首次启动**
- 首次运行时复制内置 skill 文件（`delegate`、`skill-creator` 等）到用户目录，涉及多次文件写入。
- 非首次运行虽然快，但仍需磁盘 stat 检查每个 skill 文件。
- **差距**：Claude Code 将 skill 安装延迟到首次 `/skills` 命令调用时；Gemini 在后台 idle 时安装。

### 🟡 P1 — 间接影响响应速度

**3. `SkillRegistry::discover()` 每轮重新扫描**
- 每次执行需要 skill 的命令（如 `/review`、`/restore`）都调用 `discover()` 扫描磁盘目录。
- 不存在会话级缓存，频繁调用时重复 I/O。
- **差距**：Claude Code 在启动时一次性构建 skill 索引并缓存到内存，后续用 inotify/FSEvents 监听变更。

**4. 首轮 model routing inventory 无预热**
- 用户发第一条消息时，`resolve_auto_route_with_inventory` 可能触发模型列表的首次加载。
- **差距**：Claude Code 在后台预热 model catalog；Gemini 在启动画面展示时异步预加载。

### 🟢 P2 — 用户体验优化

**5. 无启动进度反馈**
- 当前启动时屏幕空白直到 TUI 首帧渲染完成，用户无法判断是否在加载。
- **差距**：Claude Code 显示轻量 spinner + 版本号；Gemini 显示「Initializing...」占位文字。

**6. 冷启动与热启动无区分优化**
- 每次启动均执行全量初始化，无论是第一次还是第 100 次启动。
- **差距**：Claude Code 在 daemon 模式下保持后台进程，热启动 < 50ms。

---

## 与竞品对比

| 优化策略 | mimofan | Claude Code | Gemini |
|---------|---------|-------------|--------|
| Prune 操作异步化 | ❌（主线程串行） | ✅（后台 tokio task） | ✅（spawn_blocking） |
| Skill 安装延迟加载 | ❌（启动时同步） | ✅（首次调用时） | ✅（后台 idle） |
| Skill 目录缓存 | ❌（每次扫描） | ✅（内存缓存 + FSEvents） | 部分 |
| Model catalog 预热 | ❌ | ✅ | ✅ |
| 启动进度反馈 | ❌ | ✅（spinner） | ✅（占位文字） |
| 热启动优化 | ❌ | ✅（daemon 模式） | ❌ |

---

## 优化计划待办

- [ ] **[P0]** 将三个 prune 操作（snapshot、spillover、sessions）移至后台 `tokio::spawn`，不阻塞 `run_tui()` 调用（`lib.rs` L5930~L5960）
- [ ] **[P0]** `install_system_skills` 改为懒加载：首次调用 skill 相关命令时触发，而非每次启动
- [ ] **[P1]** `SkillRegistry::discover()` 增加会话级内存缓存，用 `OnceLock<SkillRegistry>` 或 `Arc<RwLock<>>` 持有，首次加载后 FSEvents 监听变更
- [ ] **[P1]** 启动后立即后台预热 model routing inventory（`tokio::spawn` + `resolve_auto_route_with_inventory`）
- [ ] **[P2]** 启动阶段增加轻量进度反馈（spinner 或「Loading...」占位），首帧渲染前即可显示
- [ ] **[P2]** 评估 daemon 模式可行性：后台保持进程，`mimo` 命令直接 attach，实现热启动 < 100ms
- [ ] 补充启动时间基准测试，量化各步骤耗时（`tracing::instrument` + startup flame graph）


---

## [ ] Issue #34: 【记忆能力】保证效果前提下，默认提升长期记忆与短期记忆能力，制定优化待办
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/34](https://github.com/XiaomingX/mimofan/issues/34)

### Description / Action Plan
## 目标视角

> **在保证效果（信息准确性、注入相关性、用户隐私）的前提下，默认提升长期记忆和短期记忆能力，让 Agent 在每次对话中都能充分利用历史积累的知识。**

---

## 现状盘点（代码实证）

### 长期记忆（跨 session 持久化）

| 机制 | 实现位置 | 默认状态 | 说明 |
|------|---------|---------|------|
| `memory.md` 用户记忆文件 | `src/memory.rs` | ❌ **opt-in**（需 `[memory] enabled = true`） | 每 turn 注入 `<user_memory>` block 到 system prompt |
| `remember` 工具 | `src/tools/remember.rs` | 随 memory 功能开关 | 模型主动追加记忆条目到 `memory.md`，auto-approve |
| 外部 Memory Service | `src/memory_service.rs` | ❌ **opt-in**（需配置 URL） | HTTP 客户端集成 claude-mem，支持语义搜索 |
| `/memory` slash 命令 | `commands/groups/memory/memory.rs` | ✅ 可用 | 查看/清除/编辑 memory.md |
| `# 快捷记录` | `memory.rs` | 随 memory 功能开关 | composer 输入以 `#` 开头直接追加到 memory.md |

### 短期记忆（会话内上下文）

| 机制 | 实现位置 | 说明 |
|------|---------|------|
| 对话历史（api_messages） | `tui/app.rs` | 完整 turn 历史，受 context window 限制 |
| project_context（`AGENTS.md` 等） | `src/project_context.rs` | 项目级指令，每 turn 注入 system prompt |
| Compaction 摘要 | `src/compaction.rs` | 窗口满时压缩历史，摘要替换原始消息 |
| Slop Ledger | `src/slop_ledger.rs` | 技术债记录，**未注入** system prompt |
| Todo/Plan 状态 | `src/tools/todo.rs` + `plan.rs` | 任务追踪，**未自动注入**上下文 |

---

## 识别出的问题与差距

### 🔴 P0 — 严重影响记忆效果

**1. 长期记忆默认关闭（opt-in），新用户零记忆体验**
- `[memory] enabled` 默认 `false`，新用户不配置则 Agent 无任何跨 session 记忆。
- **差距**：Claude Code 默认开启 `~/.claude/memory.md`（`CLAUDE.md` 优先，memory 自动追加）；Gemini 默认读取 `~/.gemini/memory.md`。
- **效果保证**：改为**默认开启**但文件为空，不注入空内容（已有 `if content.trim().is_empty() { return None; }` 保护），零开销。

**2. 外部 Memory Service 无自动语义检索，仅全量加载**
- 当前 `memory_service.rs` 实现了语义搜索 API（`MemorySearchRequest`），但 `memory.rs:144` 的加载逻辑是**全量加载**所有记忆，未按当前任务做相关性过滤。
- 大量记忆时 token 浪费严重，且无关记忆可能干扰 Agent 决策。
- **差距**：Claude Code / Gemini 在每次 turn 前以当前用户输入为 query 做语义检索，只注入 top-K 相关记忆。

**3. `remember` 工具触发时机完全依赖模型主动性**
- 模型只有在"意识到"某信息值得记忆时才调用 `remember`，缺乏系统性触发。
- 用户明确表达的偏好（如「以后总是用 Rust 2021 edition」）未必每次都被 Agent 记住。
- **差距**：Claude Code 在 turn 结束时有后台「记忆蒸馏」步骤，分析对话识别可记忆信息并自动追加。

---

### 🟡 P1 — 显著影响记忆质量

**4. `memory.md` 无结构化分类，随时间膨胀后质量下降**
- 记忆条目全部平铺为 bullet list，无分类（用户偏好/项目约定/技术知识），随条目增加检索效率下降。
- 达到 `MAX_MEMORY_SIZE`（100KB）后被截断，早期重要记忆可能丢失。
- **差距**：Claude Code 的 memory 文件支持 `## Category` 分节，定期自动合并/去重相似条目。

**5. Slop Ledger 未注入 system context**
- `slop_ledger.rs` 记录了跨 session 的技术债和已知问题，但 Agent 每次 turn 不感知这些信息，可能重复踩坑。
- **差距**：需在 turn 开始时将与当前工作区相关的 slop 条目注入 system prompt（类似 memory block）。

**6. Todo/Plan 状态无法跨 session 持久化**
- `todo` tool 和 `plan` tool 的状态仅存在于当前 session 的内存中，用户关闭 TUI 后丢失。
- **差距**：Claude Code 将 todo 状态序列化到 `~/.claude/todos/`，下次启动自动恢复；Codex 有 task checkpoint。

**7. project_context 加载无增量更新，每 turn 重新读取**
- `AGENTS.md` 等文件每 turn 全量读取，文件内容变化无法实时感知（需重启）。
- **差距**：Gemini 使用 FSEvents/inotify 监听 project context 文件，变更时自动热重载。

---

### 🟢 P2 — 可改进

**8. 记忆注入位置固定，无法按重要性动态排序**
- `user_memory_block` 总是追加在 system prompt 末尾（`prompts.rs:1033`），无法将高重要性记忆前置。
- **差距**：Claude Code 支持 memory 条目优先级标记（`!important`），高优先级条目前置注入。

**9. 无记忆健康度可观测性**
- 用户无法从 TUI 中直观看到「当前有多少条记忆」「哪些记忆最近被用到」「memory.md 使用了多少空间」。
- **差距**：`/memory` 命令仅显示原始文件内容，缺少统计摘要和使用频率信息。

**10. 短期记忆（compaction 摘要）无关键信息保护**
- Compaction 时 LLM 生成摘要，可能丢失仍需引用的关键上下文（如「用户要求不修改测试文件」）。
- 详见 Issue #30（长程任务一致性），此处标记关联。

---

## 与竞品差距汇总

| 记忆能力维度 | mimofan | Claude Code | Gemini | Codex |
|------------|---------|-------------|--------|-------|
| 长期记忆默认开启 | ❌（opt-in） | ✅ | ✅ | ❌ |
| 语义检索相关记忆注入 | ❌（全量） | ✅（top-K） | ✅（top-K） | ❌ |
| 记忆自动蒸馏（turn 结束后） | ❌ | ✅ | 部分 | ❌ |
| 记忆结构化分类 + 去重 | ❌ | ✅ | ❌ | ❌ |
| Todo/Plan 跨 session 持久化 | ❌ | ✅ | ❌ | ✅ |
| Slop Ledger 注入 system context | ❌ | N/A | N/A | N/A |
| project_context 热重载 | ❌ | ❌ | ✅ | ❌ |
| 记忆优先级排序 | ❌ | ✅ | ❌ | ❌ |
| 记忆健康度可观测性 | ❌ | 部分 | ❌ | ❌ |

---

## 优化计划待办

### 长期记忆
- [ ] **[P0]** 将 `[memory] enabled` 默认值改为 `true`，空文件不注入（已有保护），零开销（`config.rs`）
- [ ] **[P0]** `memory_service` 加载改为按当前 user input 做语义检索（top-5 相关），替代全量加载（`memory.rs:144`）
- [ ] **[P1]** 每次 turn 结束后后台执行「记忆蒸馏」：分析对话识别可记忆信息，自动追加到 `memory.md`
- [ ] **[P1]** `memory.md` 支持 `## Category` 分节（用户偏好 / 项目约定 / 技术知识），并定期自动合并相似条目
- [ ] **[P2]** `/memory` 命令增加统计摘要（条目数、文件大小、最近使用的条目）
- [ ] **[P2]** 支持记忆条目优先级标记（`!important`），高优先级前置注入

### 短期记忆
- [ ] **[P1]** Todo/Plan 状态序列化持久化到 `~/.mimo/todos/<workspace-hash>.json`，启动时自动恢复
- [ ] **[P1]** 每次 turn 开始时将当前工作区相关的 Slop Ledger 条目注入 system context（`slop_ledger.rs` → `prompts.rs`）
- [ ] **[P2]** project_context 文件（`AGENTS.md` 等）增加 FSEvents/inotify 监听，变更时热重载无需重启
- [ ] **[P2]** Compaction 摘要保留「当前约束」anchor 字段（关联 Issue #30）


---

## [x] Issue #35: 【/compact 命令】确认现状并修复：增加压缩预览、参数支持和结果反馈，对齐 Claude Code / Gemini
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/35](https://github.com/XiaomingX/mimofan/issues/35)

### Description / Action Plan
## 背景

经代码审查，`/compact` 命令**已存在**，但存在若干需要确认和优化的问题。

---

## 现状盘点（代码实证）

| 组件 | 状态 | 位置 |
|------|------|------|
| `/compact` slash 命令 | ✅ **已有** | `commands/groups/session/compact.rs` |
| 中文别名 `yasuo`（压缩） | ✅ 已有 | `COMMAND_INFO.aliases` |
| `AppAction::CompactContext` → `Op::CompactContext` | ✅ 已有 | `core/ops.rs` + `tui/ui.rs:7367` |
| `handle_manual_compaction()` | ✅ 已有 | `core/engine.rs:1611` |
| Hotbar 按钮触发 compaction | ✅ 已有 | `tui/hotbar/actions.rs:223` |
| `/debug` 建议低 cache 命中率时用 `/compact` | ✅ 已有 | `debug.rs:540` |
| `auto_compact` 配置项 | ✅ 已有 | `config.rs`，`/config auto_compact` 可开关 |

---

## 发现的问题

### 🔴 P0 — 需要修复

**1. `/compact` 无参数支持，无法指定压缩策略**

当前 `/compact`（无参数）直接触发全量压缩。Claude Code 和 Gemini 均支持：
- `/compact` — 默认压缩（摘要模式）
- `/compact --force` — 强制压缩（即使未到阈值）
- `/compact --strategy=aggressive` — 激进压缩（更短摘要）

**当前问题**：用户在上下文压力不高时误触 `/compact`，会丢失尚需引用的历史上下文。

**修复方案**：
```rust
// compact.rs 扩展参数解析
pub fn compact(app: &mut App, arg: Option<&str>) -> CommandResult {
    let force = arg.map(|a| a.contains("--force")).unwrap_or(false);
    let strategy = parse_compact_strategy(arg); // normal | aggressive
    CommandResult::with_message_and_action(
        format!("Context compaction triggered ({strategy})..."),
        AppAction::CompactContext { force, strategy },
    )
}
```

**2. 压缩前无状态确认，用户无法预判压缩影响**

当前 `/compact` 立即执行，不显示「当前 token 使用量」「预计压缩后保留多少 token」等信息，用户无法做知情决策。

**修复方案**：`/compact` 无 `--force` 时先显示预览：
```
当前上下文：68,432 tokens（68% 窗口）
预计压缩后：~12,000 tokens
压缩将丢失 ~56,000 tokens 的历史细节。
确认？[y/N]
```

**3. 压缩后无明确反馈**

压缩完成后仅系统消息「Context compaction triggered...」，未显示实际压缩结果（压缩前/后 token 数、摘要长度）。

---

### 🟡 P1 — 对齐竞品体验

**4. `/compact` 与 auto_compact 无协同说明**

用户不清楚 `/compact`（手动）与 `auto_compact`（自动）的关系和触发时机差异。

**修复**：`/compact` 帮助文本中说明自动压缩阈值（当前 80% 窗口），以及手动与自动的优先级关系。

**5. 别名不完整，用户习惯路径断裂**

- Claude Code 用户习惯：`/compact`（已有）✅
- Gemini 用户习惯：`/compact`（已有）✅  
- 但 `/compress` 别名缺失（部分用户的直觉词）

---

## 与竞品对比

| 功能维度 | mimofan（现状） | Claude Code | Gemini |
|---------|---------------|-------------|--------|
| `/compact` 命令 | ✅ | ✅ | ✅ |
| 压缩前预览 token 影响 | ❌ | ✅ | ✅ |
| `--force` / `--strategy` 参数 | ❌ | ✅ | 部分 |
| 压缩后结果反馈 | ❌（仅触发消息） | ✅（前后 token 数） | ✅ |
| `/compress` 别名 | ❌ | ❌ | ✅ |

---

## 修复计划待办

- [ ] **[P0]** `compact.rs` 解析 `arg` 参数，支持 `--force`（跳过确认）和 `--strategy=aggressive`
- [ ] **[P0]** `/compact`（无 `--force`）先查询当前 token 使用量，展示压缩预览并等待用户确认
- [ ] **[P0]** `handle_manual_compaction()` 完成后通过 `Event::status` 返回实际压缩结果（压缩前/后 token 数）
- [ ] **[P1]** `COMMAND_INFO.aliases` 增加 `"compress"`
- [ ] **[P1]** `/compact` 帮助文本中说明与 `auto_compact` 的关系和阈值
- [ ] 补充 `/compact` 的集成测试（`session/acceptance.rs` 已有框架）


---

## [x] Issue #36: 【专家团并行】修复 Fleet max_concurrent_tasks 硬编码为 1 导致子任务实际串行的问题
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/36](https://github.com/XiaomingX/mimofan/issues/36)

### Description / Action Plan
## 背景

经代码审查，项目已具备**完整的 Fleet 并行子任务基础设施**，但存在关键的并发度限制问题，导致「专家团」模式的并行能力未能充分发挥。

---

## 现状盘点（代码实证）

### 已有能力

| 组件 | 状态 | 位置 |
|------|------|------|
| Fleet Manager（调度中枢） | ✅ 完整 | `src/fleet/manager.rs` |
| FleetExecutor（工作进程执行） | ✅ 完整 | `src/fleet/executor.rs` |
| FleetScheduler（任务调度策略） | ✅ 完整 | `src/fleet/scheduler.rs` |
| FleetLedger（任务状态账本） | ✅ 完整 | `src/fleet/ledger.rs` |
| SubAgent 角色系统（reviewer/implementer/...） | ✅ 完整 | `tools/subagent/mod.rs` |
| `is_parallel_safe_read_only_tool`（并行安全判断） | ✅ 完整 | `fleet/worker_runtime.rs:611` |
| `/fleet [setup\|status]` 用户入口 | ✅ 完整 | `commands/groups/core/fleet.rs` |

### 🔴 关键问题：并发度硬限制为 1

**代码证据**（`fleet/manager.rs`）：
```rust
// L706
max_concurrent_tasks: Some(1),

// L1200
max_concurrent_tasks: Some(1),

// L1212
max_concurrent_tasks: Some(1),
```

**三处**代码路径均将 `max_concurrent_tasks` 硬编码为 `1`，意味着：
- Fleet 任务队列虽然存在，但**同一时刻只有 1 个 worker 在执行**
- 专家团的所有子任务**串行**处理，「并行」能力实质上被禁用
- `is_parallel_safe_read_only_tool` 虽识别了哪些工具可并行，但无实际调度路径消费此信息

---

## 问题分析

### 🔴 P0 — 核心并行能力缺失

**1. `max_concurrent_tasks` 硬编码为 1，并行能力名存实亡**

预期行为（对标 Claude Code / Gemini）：
```
用户: 帮我同时审查 crates/config 和 crates/tui 两个模块
Agent: 启动 Expert Team...
       [worker-1] 正在审查 crates/config... (并行)
       [worker-2] 正在审查 crates/tui...   (并行)
       [合并] 汇总两个审查结果...
```

当前实际行为：
```
[worker-1] 审查 crates/config... (完成)
[worker-2] 审查 crates/tui...   (等待 worker-1 完成后才开始)
```

**修复方案**：
```rust
// 从配置读取，默认 4（与 max_subagents 一致）
max_concurrent_tasks: Some(config.fleet_max_concurrent.unwrap_or(4)),
```

**2. 并行安全判断（`is_parallel_safe_read_only_tool`）有实现但无调度路径**

`is_parallel_safe_read_only_tool` 正确识别了哪些工具可安全并行（read_file、grep_files 等），但 `FleetScheduler` 在分配任务时未区分并行安全与非并行安全的任务，缺少动态并发度调整逻辑。

---

### 🟡 P1 — 专家团 UX 与竞品差距

**3. 无「专家团模式」的用户侧入口**

当前 `/fleet setup` 是管理员配置流，不是「为当前任务快速召集专家团」的直觉入口。

Claude Code 的 `claude --parallel` / Gemini 的多 agent 模式均有更简洁的召唤路径：
- 缺少 `/expert-team <任务描述>` 一键启动多专家并行的命令
- 用户不清楚何时会自动触发 fleet 并行

**4. 并行任务结果合并策略不明确**

多个 worker 并行完成后，结果汇总由父 Agent 自行处理，无结构化合并协议。
Claude Code 有显式的 `merge_results` 工具调用，Gemini 有 aggregator 角色。

---

## 与竞品对比

| 并行能力维度 | mimofan（现状） | Claude Code | Gemini | Codex |
|------------|---------------|-------------|--------|-------|
| 并行子任务调度基础设施 | ✅（但限并发=1） | ✅ | ✅ | ✅ |
| 实际并发执行 | ❌（串行） | ✅（默认 4） | ✅（默认 N） | ✅ |
| 并行安全工具识别 | ✅ | ✅ | ✅ | ❌ |
| 并发度可配置 | ❌（硬编码 1） | ✅ | ✅ | 部分 |
| 专家团一键召唤入口 | ❌ | ✅ | ✅ | ❌ |
| 结果合并结构化协议 | ❌ | ✅ | ✅ | ❌ |

---

## 修复与补全计划待办

### 修复（现有问题）

- [ ] **[P0]** 将 `fleet/manager.rs` 三处 `max_concurrent_tasks: Some(1)` 改为从配置读取，默认 `4`（与 `max_subagents` 保持一致）
- [ ] **[P0]** `FleetScheduler` 在任务分配时消费 `is_parallel_safe_read_only_tool` 判断，对只读任务动态提升并发度上限
- [ ] **[P0]** 新增 `fleet_max_concurrent` 配置项（`crates/config`），允许用户设定并发度上限

### 补全（对标竞品）

- [ ] **[P1]** 新增 `/expert-team <任务描述>` slash 命令，一键启动多专家角色并行处理当前任务
- [ ] **[P1]** 并行任务结果引入结构化合并协议（`aggregator` 角色或 `merge_results` 工具）
- [ ] **[P1]** Fleet status 视图实时展示各 worker 的并行执行进度（当前仅展示状态，无并行可视化）
- [ ] **[P2]** `USER_GUIDE.md` 补充专家团并行处理使用说明

---

## 注意事项

- **安全边界**：并发写操作需互斥，`is_parallel_safe_read_only_tool` 已识别安全边界，扩展并发时只对只读任务开放高并发，写操作保持串行或加锁。
- **资源限制**：并发度上限应与 `max_subagents` 联动，避免系统资源耗尽。
- **AGENTS.md 约束**：根据项目规范，子智能体深度保持不变，并行扩展不增加深度，只扩展同层并发宽度。


---

## [ ] Issue #37: 【内存优化】保证效果前提下降低内存消耗：HistoryCell 瘦身、渲染缓存 LRU、子 Agent 历史回收
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/37](https://github.com/XiaomingX/mimofan/issues/37)

### Description / Action Plan
## 目标视角

> **在保证效果（渲染流畅、响应速度、功能正确性）的前提下，降低运行时内存消耗和内存浪费，提升长时间运行会话的稳定性。**

---

## 现状盘点（代码实证）

项目已有若干内存优化机制：

| 已有机制 | 实现位置 | 效果 |
|---------|---------|------|
| `TranscriptViewCache` 逐 cell 渲染缓存 | `tui/transcript.rs` | `Arc<Vec<Line>>` 共享，避免 O(N·lines) 克隆，解决 issue #78 |
| `TokenEstimateCache` token 估算缓存 | `core/engine/token_estimate_cache.rs` | 消除每 turn 多次重复 token 计算（~2ms/次）|
| 大工具输出 spillover（磁盘外存） | `tools/truncate.rs` | >阈值的工具输出写磁盘，不全量持有在内存 |
| `tool_result_retrieval` 按需检索 | `tools/tool_result_retrieval.rs` | 模型按需取 spillover 内容，不全量加载 |
| 启动时 prune spillover 文件 | `lib.rs` | 清理超期 spillover 文件防磁盘无限增长 |

---

## 识别出的问题与差距

### 🔴 P0 — 严重内存浪费

**1. `HistoryCell` 完整保留在内存，长对话无上限增长**

- `App.history`（`Vec<HistoryCell>`）保存所有历史 cell，无容量上限。
- 每个 `HistoryCell::ToolResult` 可能含完整的工具输出文本（大文件 diff、编译输出等），长时间运行后内存无上限增长。
- `TranscriptViewCache` 的 `Arc<Vec<Line<'static>>>` 也按 cell 数线性增长，且渲染后的 `Line<'static>` 中包含 owned `Span` 字符串，内存占用远大于原始文本。
- **差距**：Claude Code 对超出可视窗口 N 条的历史 cell 进行「内容瘦身」（只保留摘要/元数据，原始文本丢弃）；Gemini 将老 cell 序列化到磁盘。

**2. `HistoryCell` 与 `session.messages`（API 消息）双重存储，内容重复**

- UI 层的 `HistoryCell` 和引擎层的 `session.messages`（`Vec<Message>`）均保存对话内容，存在大量内容重叠（assistant 消息文本既在 `HistoryCell::Assistant` 中，也在 `session.messages` 的 `content` 字段中）。
- 长对话时双份存储造成 2× 内存占用。
- **差距**：Codex 的 TUI 层仅持有 `cell_index → message_index` 的索引映射，UI 渲染时从 `session.messages` 引用，不重复存储。

**3. 子智能体 `SharedSubAgentManager` 持有全量历史**

- `SharedSubAgentManager`（`Arc<RwLock<SubAgentManager>>`）持有每个子智能体的完整对话历史（`PersistedSubAgentState`）。
- 多子智能体并行时（fleet 修复后可能达到 4 并发），内存占用 = 主 Agent + N 个子 Agent 各自的完整历史，可能达到数百 MB。
- **差距**：Claude Code 子 Agent 完成后立即释放其完整对话历史，只保留「任务结果摘要」。

---

### 🟡 P1 — 显著内存浪费

**4. `TokenEstimateCache` audit_ring 容量 64 条，但仅单 key 缓存**

- `AUDIT_RING_CAPACITY = 64` 条审计环持续增长无回收触发。
- cache 本身只存单个 `cached_tokens: Option<usize>`，每次 `messages_revision` 变化即 miss，没有多版本 LRU。
- 问题：高速流式输出阶段（每 delta 都 bump revision），每帧都 miss 导致每帧完整重走 message 历史，即便历史未实质变化。
- **差距**：应对「新 delta 仅追加到最后一条消息」做增量估算（只重算最后一条消息的 token 数），而非全量重走。

**5. `TranscriptViewCache` 无内存上限，长对话后无限增长**

- `CachedCell` 使用 `Arc<Vec<Line<'static>>>` 避免克隆，但 cache 本身按 cell index 索引，永不回收老 cell 的渲染结果。
- 历史 1000 个 cell 的大 diff 渲染结果全部保留在内存，即便用户从未滚动到该位置。
- **修复**：引入 LRU 上限（如最近 200 个 cell），老 cell 的渲染缓存按需驱逐，需要时重新渲染。

**6. `SkillRegistry::discover()` 每次重新分配，不复用**

- 每次命令（`/review`、`/restore`）调用 `discover()` 创建新的 `SkillRegistry`（`HashMap`），分配并丢弃，无会话级缓存。
- **差距**：一次性分配 + `OnceLock` 持有，后续调用零分配。

---

### 🟢 P2 — 可改进

**7. 无运行时内存可观测性**

- 当前无任何内存使用量展示（历史 cell 数、session.messages 大小、cache 占用等）。
- **差距**：Claude Code `/status` 输出包含「Memory: ~42 MB」；Gemini 侧边栏显示内存压力指示器。
- **修复**：`/status` 或 `/debug` 增加内存摘要（估算 cell 数 × 平均 cell 大小、spillover 文件总大小等）。

**8. `Vec<Message>` 历史消息无压缩存储**

- `session.messages` 中的历史消息以 JSON 字符串形式在 Rust 结构体中 owned 存储，无压缩。
- 大型代码 diff / 文件内容的消息反复克隆（`SessionSnapshot`、`compaction` 输入等）。
- **差距**：对超过 1KB 的消息内容可按需做 zstd 压缩后存储，读取时解压，内存节约 60-80%。

---

## 与竞品差距汇总

| 内存优化维度 | mimofan | Claude Code | Gemini | Codex |
|------------|---------|-------------|--------|-------|
| 历史 cell 内容瘦身（超出视口后） | ❌ | ✅ | ✅ | ❌ |
| HistoryCell/Message 去重存储 | ❌（双份） | 部分 | ❌ | ✅（索引引用） |
| 子 Agent 完成后释放历史 | ❌ | ✅ | 部分 | ✅ |
| 渲染缓存 LRU 上限 | ❌（无上限） | ✅ | ✅ | ✅ |
| Token 估算增量计算 | ❌（全量重走） | ✅ | 部分 | ❌ |
| 内存使用可观测性 | ❌ | ✅（/status） | ✅（侧边栏） | ❌ |
| 消息内容压缩存储 | ❌ | ❌ | ✅（可选） | ❌ |

---

## 优化计划待办

### 修复（P0）

- [ ] **[P0]** 对超出可视滚动窗口 N 条（如 500 条）的老 `HistoryCell`，将 `content` 替换为空或摘要字符串，释放原始文本内存（`tui/app.rs` + `tui/history.rs`）
- [ ] **[P0]** 子 Agent 完成后调用 `SubAgentManager::release_history(agent_id)`，只保留结果摘要，释放完整对话历史（`tools/subagent/mod.rs`）
- [ ] **[P0]** `TokenEstimateCache` 对「仅最后一条消息追加 delta」的情况做增量估算，避免全量重走 message 历史

### 优化（P1）

- [ ] **[P1]** `TranscriptViewCache` 引入 LRU 上限（默认 200 cell），超出时驱逐老 cell 渲染结果，需时重渲染
- [ ] **[P1]** `SkillRegistry::discover()` 改为会话级 `OnceLock<SkillRegistry>` 缓存，减少重复分配
- [ ] **[P1]** 调查 `HistoryCell` 与 `session.messages` 重叠内容，引入共享引用减少双份存储（长期目标）

### 可观测性（P2）

- [ ] **[P2]** `/status` 或 `/debug` 增加内存摘要输出：history cell 数、估算内存占用、spillover 文件总大小、TranscriptViewCache 大小
- [ ] **[P2]** 评估超大消息内容（>1KB）zstd 压缩存储的可行性和收益

---

## 效果保证约束（优化红线）

| 约束 | 说明 |
|------|------|
| 渲染流畅不降级 | 历史 cell 内容瘦身后，用户滚动到历史区域时仍须能完整显示（按需从 session.messages 重建） |
| 复制/导出功能不受影响 | `/export` 和选中复制必须能访问完整文本，不能因内容瘦身而截断 |
| 子 Agent 结果完整性 | 释放子 Agent 历史前必须确保结果已完整传递给父 Agent |


---

## [ ] Issue #38: 【界面布局】保证效果前提下简化与美化页面布局，去除非核心冗余信息
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/38](https://github.com/XiaomingX/mimofan/issues/38)

### Description / Action Plan
## 目标视角

> **在保证交互效果与核心信息完整的前提下，简化 TUI 页面布局、去除非核心及冗余提示/营销推广信息，建立现代、沉浸且符合主流终端最佳实践的极简视觉体验。**

---

## 现状盘点与问题排查

经代码审查，当前 TUI（`crates/tui/src/tui/`）在视觉布局上存在以下影响美观与效率的问题：

### 🔴 P0 — 冗余提示与视觉干扰

1. **首次启动及 Welcome/Banner 提示块较冗长**
   - 包含多行引导、促销/推广、声明等辅助信息，挤占了终端黄金视口区域。
   - 用户无法在启动后第一眼聚焦于输入框和核心对话。
2. **Footer/Header 状态栏信息密度过高且缺乏层次**
   - `footer_ui.rs` 与 `ui.rs` 中同时展示过多非核心状态（如冗长的环境路径、多次重复的模型指示符、非紧急的状态标语等）。
   - 信息缺乏优先级视效区分，视觉噪点较多。

### 🟡 P1 — 布局与卡片美观度提升

3. **Tool Call / History 单元格边框与间距过于繁复**
   - 工具执行卡片、Markdown 渲染面板的边框和分划线占用过多物理行。
   - 在小尺寸终端窗口下体验较为拥挤。
4. **配色方案统一性待强化**
   - 现有的主题配色方案在暗色模式下对比度未经过极致调优，缺乏类似 Glassmorphism/Neumorphism 或 Claude Code 式的精致现代渐变沉浸感。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 视觉布局维度 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **界面极简度** | 辅助状态栏与提示较多 | 极简，仅保留输入框与极少状态 | 极简，清晰分割线 | 中规中矩 |
| **无干扰视口** | 启动 Banner 占用视口空间 | 启动仅保留图标 + 核心输入提示 | 极简 Banner，回车即清空 | 占用较多空间 |
| **工具卡片渲染** | 显式长边框与多行状态 | 折叠式轻量 Pills 标签 | 简洁缩进行 | 块级面板 |
| **配色美学** | 传统终端高亮 | 现代化沉浸式调色盘 (Theme tailored) | Material 风格现代配色 | 标准 ANSI 调色 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 清理 Banner & 营销/冗余引导信息**
  - 在 `tui/ui.rs` / `main.rs` 中精简 Welcome 欢迎区，移除无关的推广提示及多余文案。
  - 提供 `suppress_welcome_banner` 配置选项支持完全静默启动。
- [ ] **[P0] 优化 Footer / Header 状态栏布局**
  - 重构 `tui/footer_ui.rs`，按「必要（如 Token/Mode）」与「辅助」区分信息级次，隐藏次要状态。
  - 减少状态栏行高占用，释放给 Transcript 主视口。
- [ ] **[P1] 简化 Tool Call 与 Transcript 卡片边框**
  - 精简 `active_cell.rs` / `history/tool_run.rs` 中的渲染元素，采用简洁的前缀 Glyph 或微型 Pills 代替大面积边框线。
- [ ] **[P1] 升级 macOS 终端美学与主题对比度**
  - 优化 `deepseek_theme.rs` 配色盘，采用更符合主流最佳实践的柔和 ANSI 配色方案。
- [ ] **[P2] 文档与配置**
  - 在 `USER_GUIDE.md` 中增加极简布局设置说明。


---

## [ ] Issue #39: 【Replay Session 性能优化】保证效果前提下引入分批写盘与分片让步，提升长会话恢复流畅度
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/39](https://github.com/XiaomingX/mimofan/issues/39)

### Description / Action Plan
## 目标视角

> **在保证会话数据完整性与回放正确性的前提下，排查与优化 Replay Session（会话恢复/回放）及磁盘写盘性能，评估引入分批写盘（Batch Disk Writes）与分片让步（Chunked Yielding / Async Sleep Coalescing）的可行性，降低 UI 卡顿并提升长会话回放流畅度。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/tui/persistence_actor.rs`、`session_manager.rs` 及 `app.rs`）：

### 1. 写盘机制现状
- 项目已使用单独的 `persistence_actor.rs`（Persistence Actor）异步处理会话 checkpoint 的保存。
- **潜在问题**：在流式输出（Streaming Delta）极高频触发或长对话每 turn 包含大量 Tool Outputs 时，如果同步或未经过 debounce/batch 聚合的 serialization 被触发，容易产生磁盘 I/O 峰值。

### 2. Session Replay / 恢复性能现状
- 在 Resume / Replay 庞大历史会话时，`app.rs` 会一次性同步加载反序列化所有 `HistoryCell` 并进行一次性 Wrapping 计算。
- 当 Message 数超百条或包含上千行代码 Block 时，主渲染线程未做**分片让步 (Chunked Yielding)**，会导致界面出现数百毫秒乃至秒级的 UI 冻结（TUI 渲染线程挂起）。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 维度 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **Replay 加载策略** | 同步一次性加载 & Wrapping | 增量分片加载 (Chunked Load) | 懒加载历史视口 | 后台 Actor 增量填充 |
| **主线程让步 (Yielding)** | 缺分片 yield / sleep | 超过 N 条时分批 `yield_now()` | 异步协程分步渲染 | 强解耦与分片更新 |
| **磁盘写盘策略** | Persistence Actor | 动态 Batch/Debounce 写盘 | 增量 Append-only Log | WAL 日志 + 异步 Flushing |

---

## 识别出的问题 (P0 / P1)

1. **[P0] Replay Session 时缺少分片让步 (Yielding)，大会话恢复引发界面冻结**
   - 恢复长会话时，一次性渲染及计算整个 History 数组，阻塞了 TUI 主循环。
   - 需要在 History 恢复与 Cache 预加载过程中加入分片处理逻辑，按 chunk 显式调用 `tokio::task::yield_now()`。
2. **[P1] 写盘持久化缺乏动态 Batch 策略**
   - 在高频工具日志更新与流式推导过程中，写盘请求可能过于频繁。需要优化 `persistence_actor.rs` 中的批处理与 Write Debouncing 机制。

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 引入 Replay Session 分片加载与主线程让步**
  - 重构会话恢复/回放机制，分批次（如每 20 个 Cell 一组）解析并渲染 `HistoryCell`。
  - 在批次之间增加 `yield_now()` 让出主线程执行权，防止主 UI 线程掉帧/卡顿。
- [ ] **[P0] 优化磁盘分批写盘 (Batch Disk Writes)**
  - 在 `persistence_actor.rs` 中强化节流与合并写入逻辑，对频繁的临时状态更新采用 Debounce/Batching，减少无效磁盘 I/O。
- [ ] **[P1] 增加 Replay / Persistence 性能测试基准**
  - 针对 500+ Cells 的大 Session 测试回放及写盘耗时，确保加载卡顿降低 70% 以上。
- [ ] **[P2] 文档补充**
  - 在架构文档中补充 Session Replay 优化设计。


---

## [ ] Issue #40: 【大数据量渲染与滚动优化】保证效果前提下引入视口虚拟化拼接 (Virtual Window Flattening)，提升长转录区滚动帧率
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/40](https://github.com/XiaomingX/mimofan/issues/40)

### Description / Action Plan
## 目标视角

> **在保证数据渲染准确性与流式实时性的前提下，评估与优化长对话/海量历史单元格 (High-Volume Transcript) 下的加载与滚动性能，解决大文本与多 Tool Call 场景下的滚动帧率下降与渲染卡顿，提升操作流畅度。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/tui/transcript.rs` 及 `ui.rs`）：

### 1. 现有优化（已有成效）
- `TranscriptViewCache` 实现了基于 `revision` 的单 Cell 增量渲染缓存，使用 `Arc<Vec<Line<'static>>>` 规避了逐帧深度克隆单元格的问题。

### 2. 识别出的性能瓶颈（大数据量场景）
- **[瓶颈 A] 视口外 Cell 的逐帧 Flatten 拼装开销**
  - 虽然单 Cell 渲染有缓存，但 `TranscriptViewCache::ensure` 依然需要逐帧遍历所有 Cell 并将 `per_cell` 中的 `Arc<Vec<Line>>` 扁平化拼装（Flatten）到全局的 `lines: Vec<Line>` 中。
  - 在包含数万行历史（大 Diff / 海量工具输出）的超长会话中，滚动时 `lines` 的重新 Alloc 与拼装将显著拖慢帧率。
- **[瓶颈 B] 全量 Line Metadata 校验与检索**
  - `line_meta` 与 `rail_prefix_widths` 为全量静态数组，在海量数据滚动时，视口外的 Metadata 计算与内存重排未做虚拟化切片 (Virtual Window Slicing)。
- **[瓶颈 C] 视口虚拟化 (Virtual Scrolling) 深度不够**
  - 未完全做到「只针对当前可见视口 Area (Visible Window) + 上下缓冲区 (Overscan Buffer) 进行 Flatten 与 Line 裁剪」，导致隐藏在折叠块或视口几千行之外的节点依然参与了每帧的拼接逻辑。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 滚动与加载维度 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **单 Cell 渲染缓存** | ✅ (`revision` 机制) | ✅ | ✅ | ✅ |
| **视口虚拟化 (Virtual Window)** | ⚠️ 全量 Flatten 后截取视口 | ✅ 仅展开并 Flatten 可见视口 Line | ✅ 动态视口高度切片 | ✅ 严格虚拟列表 (Virtual List) |
| **超大 Diff/Text 裁剪** | ⚠️ 部分依赖溢出写盘 | ✅ 高度智能折叠 + 懒渲染 | ✅ 超长行动态 Truncate | ✅ 分块 Block 延迟挂载 |
| **滚动帧率 (60 FPS 体验)** | 大数据量下帧率在 30-45 FPS | 稳定 60 FPS 无卡顿 | 稳定 60 FPS | 稳定 60 FPS |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 实施基于可见视口的虚拟化 Flatten 拼接 (Virtual Window Flattening)**
  - 重构 `TranscriptViewCache` 的 `flatten` 流程，仅计算并拼接视口索引区间 `[top - overscan, bottom + overscan]` 内的 Line。
  - 避免将不可见的数万行历史 Line 拼接到全局 `Vec<Line>` 中，降低每帧内存分配与 CPU 拷贝开销。
- [ ] **[P1] 优化超长行 (Long Line) 与复杂 Diff 的延迟计算**
  - 对文本行宽远超终端宽度的超长字符做提前截断或延迟 Wrap 计算，减少 Unicode 字符宽度推算的性能开销。
- [ ] **[P1] 补充大数据量滚动帧率与渲染基准测试 (Benchmark)**
  - 编写包含 50,000+ 行 Transcript 的 TUI 渲染 Benchmark 测试，验证视口虚拟化后帧渲染时间降至 < 8ms (满足 60 FPS)。
- [ ] **[P2] 文档与基准更新**
  - 更新 `USER_GUIDE.md` 及架构文档中关于虚拟列表与转录区渲染的说明。


---

## [ ] Issue #41: 【启动性能】保证效果前提下延迟加载非必要工具与 MCP 服务，提升启动速度
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/41](https://github.com/XiaomingX/mimofan/issues/41)

### Description / Action Plan
## 目标视角

> **在保证 Tool 调度效果与功能正确性的前提下，优化启动性能，对非核心/重量级工具（如自定义脚本工具、MCP 工具、次要扩展内置工具）实施延迟加载 (Lazy Tool Loading / Tool Deferral)，降低启动时的 CPU/I/O 开销。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/tools/registry.rs`、`lib.rs` 及 `core/engine/tool_setup.rs`）：

### 1. 现有工具加载机制
- `ToolRegistry` 在启动与 Session 初始化阶段，通过同步链条注册所有内置工具（Built-in Tools）、MCP Server 暴露的工具以及 Skills 相关的 Dynamic Tool Specs。
- 虽然 `ToolRegistry` 内部使用了 `OnceLock` 缓存 API 转换结果，但所有 Tool 的 JSON Schema 校验与实例化解析在启动阶段即全量完成。

### 2. 识别出的性能瓶颈 (Startup Overhead)
- **[瓶颈 A] 外部 MCP 工具同步加载与拉取**
  - 在配置了多个 MCP Server 的情况下，启动阶段需同步加载与解析 MCP Config 并构建工具 Schema。外部进程建立或远程拉取增加了延迟。
- **[瓶颈 B] 非核心工具预先全量实例化**
  - 所有次要/领域专用工具（如高级文档处理、数据校验、图像生成辅助工具等）均在启动时一次性加载到 `HashMap` 中，即使本次对话仅进行简短代码修改。
- **[瓶颈 C] 工具 JSON Schema 规范化与序列化开销**
  - 启动时为所有工具构建 `schema_canonicalize` 与 `schema_sanitize` 占据了启动 CPU 时间。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 工具加载维度 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **内置工具加载** | 启动全量同步注册 | 核心工具先装入，扩展按需装载 | 按需挂载模式 | 静态模块按需加载 |
| **MCP 工具加载** | 启动时同步读取解析 | 后台异步预热 + 按需注入 | 后台延迟建立 Client | 惰性按需初始化 |
| **Tool Catalog 构建** | 启动完成 Schema 校验 | 首次 Tool Search / Direct Call 时计算 | 懒加载构造缓存 | 分片缓存 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 实施非核心工具与 MCP 工具的延迟加载 (Lazy Tool Initialization)**
  - 将工具划分为「核心基础工具」（如 `read_file`、`edit_file`、`shell`）与「扩展工具」（MCP 工具、特殊领域工具）。
  - 核心工具在启动时立刻加载；扩展工具改用 Lazy Proxy 占位，在 Model 尝试检索或首次调用时才真正完成完整的结构化解析与挂载。
- [ ] **[P0] MCP 工具的后台异步连接与延迟挂载**
  - 将 MCP 配置文件加载与 Server Handshake 移出启动同步主流程，通过 `tokio::spawn` 后台异步建立，避免卡顿首帧。
- [ ] **[P1] 优化 Tool Schema 的静态缓存与增量注册**
  - 静态编译并缓存内置核心工具的标准 API Tool Spec JSON，避免每次启动重算 Schema。
- [ ] **[P2] 文档与日志**
  - 在启动日志与帮助文档中增加关于 Lazy Tool Setup 的追踪指标。


---

## [ ] Issue #42: 【稳定性防护】长会话与多任务下防 OOM 崩溃、渲染黑屏与假死，引入 Panic Safe 终端自愈
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/42](https://github.com/XiaomingX/mimofan/issues/42)

### Description / Action Plan
## 目标视角

> **在保证长会话与多任务流畅运行的前提下，排查并消除内存占用过高、渲染线程异常黑屏 (Black Screen) 以及渲染进程崩溃/假死的问题，建立自我修复与容错机制。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/tui/ui.rs`、`app.rs` 及 `frame_rate_limiter.rs`）：

### 1. 内存泄漏与膨胀导致崩溃 (Out Of Memory Crash)
- 在长时间对话或并发执行多个子任务 (Multi-task / Sub-agent Fleet) 场景下，`HistoryCell` 缓存、`session.messages` 以及 `TranscriptViewCache` 的 `Vec<Line>` 在没有限制机制的情况下持续膨胀。
- 当系统内存吃紧或触发系统的 OOM Killer 时，极易导致整个 TUI 渲染进程突然退出或崩溃。

### 2. 界面黑屏与渲染挂起 (Black Screen / Render Hang)
- 当转录区文本过于庞大，或者单帧计算（如终端窗口尺寸快速改变引起 `TranscriptViewCache::ensure` 全量重算）抛出未捕获异常 / Panic 时，Ratatui 渲染管线可能陷入黑屏或控制台 Raw 模式失效，界面死锁。
- 缺少 TUI 渲染层的 `catch_unwind` 与 Panic Safe 恢复屏障，一旦某个单元格样式计算溢出或越界，整个 UI 渲染线程崩塌。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 稳定与防护维度 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| ** Panic Safe 恢复** | 缺少 UI 帧渲染 Panic 隔离 | 拥有 UI Frame Error Boundary | 完整 Terminal Panic Recovery | 异常捕获并重置 Terminal |
| **超大内存容错** | 易因超长 History OOM | 动态写盘 + 强制 Compaction 保护 | 视口内存上限自动剪裁 | 限制历史 Cell 活跃上锁 |
| **黑屏/死锁自愈** | 终端需强制 kill/重启 | 自动检测无响应并重置终端状态 | `Ctrl+C` 恢复机制 | 异常退回 Safe Fallback 模式 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 引入 TUI 渲染层 Panic 隔离屏障与自动终端恢复 (Panic Recovery Guard)**
  - 在 `ui.rs` 的渲染主循环周围包裹 Panic 捕获屏障（`std::panic::catch_unwind`），若渲染某帧发生 Panic，自动恢复 Terminal Raw Mode 并切入 Safe Mode，避免黑屏崩溃。
- [ ] **[P0] 设立极限内存保底防护策略 (OOM Preemption & Emergency Trimming)**
  - 当检测到当前进程内存超过软上限（如 1GB）时，自动触发深度的 `HistoryCell` 内容剪裁与高强度 Compaction，释放废弃对象的内存，防止 OOM 崩溃。
- [ ] **[P1] 优化多任务并发与窗口 Resize 时的防抖重绘 (Debounced Redraw)**
  - 在 `frame_rate_limiter.rs` 中加强多 Task / 子 Agent 同时写输出时的 Frame Debounce，避免极高频刷屏卡死渲染管线。
- [ ] **[P2] 增加崩溃与黑屏日志诊断命令**
  - 提供 `/debug crash-log` 用于分析渲染失常与面板挂起原因。


---

## [ ] Issue #43: 【内存泄漏排查】保证效果前提下确认套件卸载、MCP/SubAgent 销毁场景无内存泄漏，健全 Teardown 链条
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/43](https://github.com/XiaomingX/mimofan/issues/43)

### Description / Action Plan
## 目标视角

> **在保证动态扩展能力（套件/Plugin 卸载、MCP 服务切断、Skill 注销、子 Agent 销毁等）正常工作的前提下，全面排查与确认各个特定场景下的内存泄漏 (Memory Leak) 隐患，建立健全的 Drop 资源清理机制与内存监控测试，保障长周期运行零残留。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/tools/plugin.rs`、`subagent/mod.rs` 及 `registry.rs`）：

### 1. 动态套件与 Plugin/MCP 卸载后的内存残留隐患
- 当用户在运行期卸载插件/扩展套件或注销 MCP 服务时，`ToolRegistry` 中移除了映射键，但涉及的后台 `Arc<Mutex<...>>` 引用、线程 Channel 句柄或绑定的静态缓存可能未能完备实现 `Drop` 释放。
- 循环引用 (`Arc` 循环引用) 或残留在全局广播 Handler 列表中会导致相关内存与缓存无法解构。

### 2. 子 Agent / 专家团销毁后的残余 Mailbox & Task State
- 在子 Agent/Task 完成并终止后，`subagent/mailbox.rs` 或线程池管理器中可能依然挂载着废弃的异步 Task Handle 和消息队列缓冲区。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 动态卸载与内存回收 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **插件/套件卸载** | HashMap 移除，缺少深度 Drop 校验 | 显式 Disconnect Hook + GC 触发 | 隔离沙箱子进程，直接 kill 释放 | 闭包资源显式注销 |
| **MCP 断开连接** | 依赖 Tokio 资源自动回收 | 清理底层 Socket & Handler 引用 | 进程隔离安全回收 | 清理 Handler 队列 |
| **内存泄漏回归测试** | 缺少单元/集成级 Leak Sanitizer | 包含 AddressSanitizer (ASan) 自动化 CI | 自动化 Heap Profiling | 结构化 Drop 测试 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 健全插件/扩展套件及 MCP 服务卸载的 `Drop` & Teardown 链条**
  - 为 `ToolRegistry`、MCP 客户端和插件模块增加显式的 Teardown/Unload 接口，确保移除注册的同时断开所有异步 Channel 及 `Arc` 弱化/解构。
- [ ] **[P0] 补充子 Agent 与异步任务终止后的 Mailbox / Cache 彻底清理**
  - 在 `subagent/mailbox.rs` 及 Task Manager 中，当 SubAgent 处于 `Completed`/`Killed` 状态时，强制清空其收件箱与状态 Handle，避免隐式泄漏。
- [ ] **[P1] 建立自动化内存泄漏诊断与 Sanitizer CI 监控**
  - 在集成测试套件中引入针对套件反复「加载-卸载」100 次后的内存分配追踪测试（Heap Profiling Benchmark），确保分配增长为 0。
- [ ] **[P2] 文档规范**
  - 在 `AGENTS.md` 及开发指南中制定组件与 Resource Manager 的生命周期规范。


---

## [ ] Issue #44: 【界面国际化治理】保证效果前提下消除标题栏与页面显示中英文混杂问题，统一多语言体验
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/44](https://github.com/XiaomingX/mimofan/issues/44)

### Description / Action Plan
## 目标视角

> **在保证国际化 (i18n) 与多语言一致性的前提下，排查并消除标题栏 (Header Bar)、状态栏 (Footer) 及页面转录/菜单控件中出现的「中英文不一致/半英半中混杂」问题，实现语言环境的一致性与专业性。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/tui/ui.rs`、`footer_ui.rs` 及 `localization.rs`）：

### 1. 国际化 (i18n) 多语言体系现状
- 项目已具备 `localization` 模块和 `MessageId` 枚举用于处理界面文本多语言化。

### 2. 识别出的中英文混杂问题 (Bilingual Mixed UI Issues)
- **[问题 A] 标题栏与 Header 文本硬编码中英文混写**
  - 在 `ui.rs` / `sidebar.rs` 中，部分 Header 标题（如 `[Agent Mode] (代理模式)` 或 `Mode: Yolo / 快捷键: Ctrl+C`）直接硬编码了双语拼接字符串，导致界面显得不规范、缺乏一致性。
- **[问题 B] 状态栏 (Footer UI) 与命令菜单描述混杂**
  - 在命令菜单 `slash_menu.rs` 及状态栏提示文案中，英文命令 Key（如 `/compact`）旁边混用了中文说明，而在英文 Locale 下依然强制显示中文注释，没有完全受 `ui_locale` (En/Zh) 驱动。
- **[问题 C] 系统提示消息 (System Status Messages) 语言不一致**
  - 部分 Tool 执行状态或 Error Message 使用英文，而某些静态 Action 通知（如 "已切换模式"）又写死了中文。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 语言与国际化维度 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **界面语言纯粹度** | 存在局部中英文拼接硬编码 | 严格根据 System Locale 纯英文/纯目标语言 | 纯正单语言 UI | 纯英文 UI |
| **命令帮助与菜单** | 双语文本混杂在同一个 Menu 项中 | 统一的 i18n 资源文件匹配 | 多语言 Dictionary 清晰分立 | 单一规范语言 |
| **模式/状态切换提示** | 混写模式 | 统一规范提示文案 | 统一规范提示 | 纯正专业术语 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 治理标题栏 (Header) 与状态栏 (Footer) 的中英文混杂**
  - 全面排查 `ui.rs`、`footer_ui.rs` 和 `sidebar.rs` 中的硬编码双语字符串，将其抽离并收口至 `localization` 字典库中。
- [ ] **[P0] 统一 Slash Menu 与快捷键帮助面板的多语言渲染**
  - 重构 `slash_menu.rs` 与 `keybindings.rs`，使菜单项的 `description` 根据用户设定的 `ui_locale` (Zh/En) 动态切换显示纯中文或纯英文。
- [ ] **[P1] 系统 Action 通知与 Tool 状态消息统一国际化**
  - 规范状态提示文案（如 `AppAction` 触发的 Status Notification），避免「英文命令名 + 中文状态谓词」导致的文法不协调。
- [ ] **[P2] 增加 i18n 缺失项的单元与回归测试**
  - 编写 i18n 字典覆盖率测试，确保 `MessageId` 在 `Zh` 与 `En` 两种 Locale 下都有 100% 的对应翻译映射。


---

## [ ] Issue #45: 【新手引导与上手体验】保证效果前提下引入首次运行配置向导与 Quickstart 场景指引
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/45](https://github.com/XiaomingX/mimofan/issues/45)

### Description / Action Plan
## 目标视角

> **在保证高效上手与极低认知负荷的前提下，设计并完善新用户首次启动引导 (First-Run Onboarding Wizard / Quickstart Guide)，帮助用户快速、无缝地完成 API Key 配置、工作区信任授权及核心 Slash 命令摸索，降低习惯迁移成本。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/lib.rs` 及 `crates/tui/src/tui/views/`）：

### 1. 现有引导与首次运行逻辑
- `lib.rs` 中包含了 `skip_onboarding` 标记及首次自动创建配置文件的逻辑 `ensure_config_file_exists`。

### 2. 识别出的体验问题 (Onboarding Issues)
- **[问题 A] 首次启动缺少交互式配置向导 (Interactive Setup Wizard)**
  - 若用户未设置 API Key 或配置文件缺失，直接抛出命令行 Error 或停留在静止的界面上，缺乏一步步引导用户输入 API Key、选择模型和配置环境的交互式 Modal 向导。
- **[问题 B] 常用 Slash 命令缺少交互式 Quickstart 示例卡片**
  - 新用户首次进入转录界面后，无法直观了解最常用且与 Claude Code / Gemini 对齐的核心命令（如 `/plan`、`/mode`、`/compact`、`/review`），导致用户需要阅读外部文档。
- **[问题 C] 工作区安全与信任提示缺乏清晰说明**
  - 在首次打开某项目代码库时，针对 YOLO/Agent 模式授权与 Shell 执行安全提示缺乏直观的可视觉引导。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 引导与上手体验 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **首次启动向导** | 依赖配置检测，缺少向导 modal | 交互式 Step-by-step Wizard (OAuth/Key) | 极简 Key 配置提示 | 引导式登录与模式选择 |
| **Quickstart 卡片** | 依赖 `/help` 打印长文本 | 视口内轻量内联快捷 Example 提示 | 引导式样例 Prompt | 核心操作面板指引 |
| **认知负荷** | 需主动查找配置文件或命令行参数 | 零门槛键盘上下键完成初次配置 | 极简 CLI 输入 | 向导模式 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 引入首次运行交互式配置向导 (First-Run Onboarding Wizard Modal)**
  - 在无配置文件或缺少 API Key 时，弹出精致的交互向导 Modal，引导用户：
    1. 选择 API Provider 及填写 Key；
    2. 选择默认模型（Flash / DeepSeek / Pro）；
    3. 确认初始运行模式（Agent 还是 YOLO）。
- [ ] **[P0] 增加首次进入时的可交互 Quickstart 提示区**
  - 在转录区为空时，展示 3-4 个最常用的核心场景快捷点击/回车卡片（如 `1. 尝试 /plan 进行规划`、`2. 尝试 /code-review 检查安全`）。
- [ ] **[P1] 新增 `/quickstart` Slash 命令**
  - 随时允许用户输入 `/quickstart` 重新唤起首次引导向导或查看场景示例。
- [ ] **[P2] 文档与提示关联**
  - 在配置向导中提供直接跳转至 Antigravity Guide 与设置说明的链接与按键说明。


---

## [ ] Issue #46: 【上下文压缩与回放优化】保证效果前提下防止反复压缩死循环、摘要泄漏与会话卡死
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/46](https://github.com/XiaomingX/mimofan/issues/46)

### Description / Action Plan
## 目标视角

> **在保证长会话语义连续性与防丢失的前提下，优化与确认上下文压缩 (Context Compaction) 和历史回放 (History Replay) 策略，解决反复压缩 (Repeated Compaction Loops)、摘要信息泄漏 (Summary Leakage) 以及会话卡住/死锁 (Session Freeze) 的问题。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/compaction.rs`、`core/engine/turn_loop.rs` 及 `engine.rs`）：

### 1. 现有压缩与回放逻辑
- `compaction.rs` 提供了 token-only 触发的上下文压缩逻辑，旨在超大对话场景下用 LLM 产生的 Summary 替换掉之前的 `Message` 历史。

### 2. 识别出的缺陷与隐患 (Compaction & Replay Issues)
- **[缺陷 A] 反复压缩死循环 (Repeated Compaction Loop)**
  - 当生成的新摘要内容依然较庞大，或者压缩后未能大幅拉低 Token 占用时，由于门限检测处于临界点，下一 Turn 会立即再次触发 Compaction，导致 Agent 陷入连续自我压缩的死循环（反复消耗 Token 且无法响应用户命令）。
- **[缺陷 B] 摘要信息泄漏 (Summary Leakage to User View)**
  - 压缩产生的内部 Prompt/Summary 结构有时会意外泄漏到 TUI 的转录视口中，使用户看到包含 `<summary>`、`<context_compaction>` 的中间元数据。
- **[缺陷 C] 压缩过程中异步线程死锁导致会话卡住 (Session Lockup)**
  - 压缩过程调用 LLM 时如果遭遇超时、网络中断或并发状态锁死，`engine.rs` 的消息循环可能挂起，导致整个 Session 状态变为 Unresponsive，无法继续交互或恢复。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 压缩与回放策略维度 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **防重复压缩机制** | 临界点易反复触发 | 压缩后冷却期 (Cooldown Period) | 动态升降阈值门控 | 严格一次性截断 Guard |
| **摘要泄漏隔离** | 偶尔打入 HistoryCell 泄漏 | 严格隔离为 Internal System Node | 隐式上下文节点 | 引擎层安全剥离 |
| **异常恢复与解锁** | 超时或报错可能卡住 | 自动 Timeout 降级与上下文回滚 | 异步解锁与错误提示 | 兜底保留核心片段并复位 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 增加防反复压缩冷却机制 (Compaction Cooldown Guard)**
  - 在 `compaction.rs` 与 `turn_loop.rs` 中引入压缩冷却计数器（如压缩后至少经过 N 次 Turn 或 Token 下降低于预估值时禁止再次触发），切断死循环。
- [ ] **[P0] 彻底隔离压缩元数据与摘要泄漏 (Metadata Leak Isolation)**
  - 严格清理 `HistoryCell` 渲染管线，确保压缩产生的 `<summary>` 与 system prompt 调整属于引擎内部 Message，绝不溢出到前台视图。
- [ ] **[P0] 增强压缩过程的 Timeout 保护与死锁自愈 (Async Compaction Self-Healing)**
  - 为 Compaction LLM 调用设置硬超时（如 30s），一旦超时或网络异常，安全回滚至未压缩状态并弹出告警，防止 Session 无响应挂起。
- [ ] **[P1] 优化 History Replay 与压缩记录的协同**
  - 会话恢复 (Replay) 时，直接加载稳定后的 Checkpoint Summary，无需重新触发逻辑判断。


---

## [ ] Issue #47: 【自动化任务列表排序修复】保证效果前提下修复 Todo/Task 列表状态排序异常，对齐 Claude Code 置顶与沉底策略
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/47](https://github.com/XiaomingX/mimofan/issues/47)

### Description / Action Plan
## 目标视角

> **在保证自动化任务列表 (Todo/Task List) 追踪准确性与直观性的前提下，确认与排查任务列表中是否存在状态/优先级排序异常（例如已完成任务挤占顶部、进行中任务置底、或无序错乱），并对齐主流 Agent 工具的排序展示体验。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/tools/todo.rs` 及 `history/checklist.rs`）：

### 1. 现有 Todo 列表存储与渲染
- `TodoList`（`tools/todo.rs`）内部使用 `Vec<TodoItem>` 按 Insertion ID（ID 递增 1..N）顺序追加和存储任务。
- `snapshot()` 方法直接返回原生的 `Vec<TodoItem>` 列表副本。

### 2. 识别出的排序与展示异常 (Task List Sorting Issues)
- **[异常 A] 默认仅按 ID 物理顺序输出，未按 Task 状态权重排序**
  - 当某中间任务（如 ID 2）被更新为 `completed`，而后追加了新任务（ID 5，`in_progress`）或 `pending` 任务时，渲染列表依然机械地按 ID 1..N 输出。
  - 用户界面上会出现 `Completed` → `In Progress` → `Completed` → `Pending` 的散乱交错，缺乏结构化视角。
- **[异常 B] `in_progress` (正在进行) 核心任务没有凸显置顶**
  - 在长 Task 列表中，当前唯一的 `in_progress` 任务被淹没在长串 `completed` / `pending` 列表项中间，用户无法直观在最醒目位置看到「Agent 当前正在做什么」。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 任务列表排序与视图 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **正在执行任务位置** | 按物理 ID 混合排列 | **强制置顶** 并伴有动态图标/Pills | 置顶/高亮卡片显示 | 独立 Current Active 栏 |
| **已完成任务组处理** | 混排在列表中 | 默认沉底或自动折叠置于末尾 | 底部划线打勾样式 | 底部归档显示 |
| **状态排序逻辑** | 无显式 Sort 排序 | `In Progress` > `Pending` > `Completed` | 逻辑状态排序 | 按阶段 Chunk 排序 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 修复与重构 Task List 的状态权重排序算法 (State-Weighted Sorting)**
  - 在 `TodoListSnapshot` 与渲染 `checklist.rs` 时，引入符合直觉的逻辑排序标准：
    1. **`In Progress`**（正在执行）：权重最高，优先置顶；
    2. **`Pending`**（待处理）：按原始 ID/Priority 顺序排在中间；
    3. **`Completed`**（已完成）：自动沉底，并支持在 TUI 卡片中折叠显示。
- [ ] **[P1] 提供 `raw` (原始 ID 顺序) 与 `grouped` (逻辑状态分组) 视图模式**
  - 允许在 TUI 的 Checklist 视图中切换是否按逻辑分组排序，确保兼顾结构化视角与原始逻辑步骤视角。
- [ ] **[P1] 补充 Task List 排序单元测试**
  - 在 `tools/todo.rs` 中编写单元测试，验证多次 `update_status` 之后 snapshot 排序状态的正确性与稳定性。


---

## [ ] Issue #48: 【/init 命令排查与优化】确认 /init 命令现状，增加覆盖确认保护、--force 参数与多语言框架识别
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/48](https://github.com/XiaomingX/mimofan/issues/48)

### Description / Action Plan
## 背景与现状盘点

经代码审查，项目中 **` /init ` Slash 命令已经存在**：
- **位置**：`crates/tui/src/commands/groups/project/init.rs`
- **功能定位**：分析项目代码库结构、生成/更新根目录下的 `AGENTS.md` 文件。
- **工作机制**：预先提取语言/依赖/框架上下文，然后向 Agent 发送结构化 Prompt（`build_init_prompt`），指引 Agent 深度读取源码并产出 `AGENTS.md`。

---

## 识别出的缺陷与优化点 (Issues & Gaps)

虽然 `/init` 已实现，但与 Claude Code / Gemini CLI 的 `/init` 体验相比存在以下不足与需修复的问题：

### 🔴 P0 — 覆盖范围与单点文件生成限制

1. **硬编码生成 `AGENTS.md`，不支持兼顾 `CLAUDE.md` / `WHALE.md` 等兼容配置**
   - 当前 `/init` 仅硬编码提示 Agent 写入 `AGENTS.md`。在习惯使用 `CLAUDE.md` 或拥有 legacy `.claude/instructions.md` 的项目中，缺失可选的文件生成标记或多文件指引。
2. **缺少覆盖提示与交互确认 (Overwrite Confirmation)**
   - 当 `AGENTS.md` 存在时，当前直接发送 prompt 让 Agent "in place" 更新，缺乏在执行前向用户弹出 Confirmation 询问「检测到 AGENTS.md 已存在，是否覆盖或追加？[y/N]」的明确控制。

### 🟡 P1 — 提取分析精度与模式对齐

3. **预提取语言/框架规则不够全面**
   - 现有的 `detect_test_frameworks` 与 `detect_build_systems` 仅覆盖了 Rust/Node.js/Python 的常规依赖，对于 Go (go.mod)、Java (pom.xml/build.gradle) 或 Docker 等缺乏快速提论。
4. **切换与触发模式统一**
   - 交互模式需对齐 Claude Code 与 Gemini 的触发习惯（支持 `/init` 以及带参数 `/init --force`）。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 命令能力维度 | mimofan (现状) | Claude Code | Gemini CLI |
|---|---|---|---|
| **`/init` 命令存在** | ✅ 已存在 (`project/init.rs`) | ✅ 已存在 | ✅ 已存在 |
| **覆盖前确认** | ❌ 缺失确认提示 | ✅ 会明确询问用户 | ✅ 会提示确认 |
| **多框架依赖预查** | 部分支持 (Rust/JS/Py) | ✅ 深度全面扫描 | ✅ 全面扫描 |
| **模式切合度** | 对齐 Claude Code 生成 prompt 逻辑 | 标准生成 CLAUDE.md | 标准生成 GEMINI.md |

---

## 修复与优化计划待办 (Todo List)

- [ ] **[P0] 修复并增加 `AGENTS.md` 覆盖确认逻辑 (Overwrite Protection)**
  - 在 `init.rs` 执行时，若检测到 `workspace.join("AGENTS.md").exists()` 且未指定 `--force` 参数，弹出 Confirmation 交互弹窗或状态提示，避免静默覆盖用户原有配置。
- [ ] **[P0] 支持 `--force` 与 `--type` 参数扩展**
  - 支持 `/init --force` 直接覆盖。
  - 支持指定兼容生成文件（如 `/init --claude` 同时生成 `CLAUDE.md` 索引）。
- [ ] **[P1] 扩充预提取规则库 (Pre-gathered Context Detection)**
  - 在 `init.rs` 中补全 Go (`go.mod`)、Java (`pom.xml`/`build.gradle`)、C/C++ (`CMakeLists.txt`) 及 Dockerfile 的检测逻辑，加速 Agent 分析效率。
- [ ] **[P2] 文档与测试**
  - 补充 `/init` 参数与覆盖保护的单元测试。


---

## [x] Issue #49: 【文件读取优化】优化 read_file 循环检测阈值与单次读取上限，防止大文件分页读取被误中断
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/49](https://github.com/XiaomingX/mimofan/issues/49)

### Description / Action Plan
## 目标视角

> **在保证系统不陷入无限循环或 Token 暴涨的前提下，优化文件读取 (`read_file`) 的熔断/循环检测机制与单次分页限制，降低 Agent 在正常分页/分段读取大型源文件时被误判定为重复循环而被中断的概率。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/src/tools/file.rs` 及 `core/engine/turn_loop.rs`）：

### 1. `read_file` 的硬性行数限制与分页策略
- `file.rs` 中定义了硬编码上界 `HARD_MAX_READ_LINES = 500`。
- 当读取超过 500 行的大型源码文件时，`read_file` 返回带有截断与分页提示的文本（`[TRUNCATED] Showing lines ... To continue, call read_file with start_line=...`）。

### 2. 误判循环与中断的潜在隐患 (False Positive Loop Interrupts)
- **[问题 A] 连续分页读取被熔断器 (Circuit Breaker) 误判定为死循环**
  - 当 Agent 顺序翻页读取长文件（例如依次读取 `start_line=1` -> `501` -> `1001` -> `1501`）时，若引擎层（如 `turn_loop.rs`）对连续调用同名 Tool (`read_file`) 的检测过于敏感，容易将其判定为「失控循环 (Runaway Loop)」，强行终止 Turn。
- **[问题 B] 参数仅变更 `start_line` 触发防重复调用告警**
  - 部分简单防重逻辑仅比对 `tool_name` 而未深度校验参数差异，导致正常的分页游标推进被误识别为无意义重试。
- **[问题 C] 单次 `HARD_MAX_READ_LINES` 门槛过低**
  - 对于动辄数千行的现代代码库文件，500 行的强截断过于保守，导致 Agent 必须被迫发送 5~10 次连续 Tool Call 才能读完一个文件，极易触发调频限制或熔断。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 文件读取与循环保护 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **单次读取限制** | 500 行硬上限 | 动态自适应（~2000 行 / 自动分块） | 高上限 (~100KB/次) | 自动流式切片 |
| **重复调用判定** | 易因连续 `read_file` 误触 | 识别 `start_line` 游标变化，不触发 | 参数变动不判定为 Loop | 参数感知型熔断 |
| **大文件读取提示** | 提示 `read_file` 手动翻页 | 自动配合大文件 Router 或增量加载 | 智能提示定位区块 | 分段并发预取 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 优化引擎层工具循环检测器的参数感知逻辑 (Parameter-Aware Loop Detection)**
  - 在 `turn_loop.rs` 或相关 Circuit Breaker 中，检测到相同 Tool (`read_file`) 连续调用时，校验参数 `start_line` 或 `path` 是否发生有效改变。若属于游标递进的正常读取，不计入死循环计数。
- [ ] **[P0] 调优 `HARD_MAX_READ_LINES` 单次读取上限**
  - 将单次读取上限由 500 行适当放宽至 1500~2000 行（或改为基于 Byte 大小动态推算），减少长文件读取时的轮询交互次数。
- [ ] **[P1] 与 Large Output Router 联动**
  - 当整文件超过单次读取上限时，自动允许大模型通过 Large Output Router 一次性获取核心结构摘要，避免频繁分页读盘。
- [ ] **[P2] 单元测试**
  - 增加长文件连续 5 次分页读取场景的集成测试，验证其 100% 不会被引擎 Circuit Breaker 中断。


---

## [x] Issue #50: 【时间时区适配】排查并修复硬编码 UTC 问题，统一使用本地时间（北京时间）与提示注入
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/50](https://github.com/XiaomingX/mimofan/issues/50)

### Description / Action Plan
## 目标视角

> **排查与确认项目中涉及日期与时间显示（如历史 Session 列表、状态栏、导出 Markdown 头部、日志时间戳以及 Prompt 系统时间注入等）是否统一适配了用户本地时间（中国地区用户适配北京时间 UTC+8），避免 UTC 与本地时间混淆引发的认知偏误。**

---

## 现状盘点与代码实证

经代码审查（`session_manager.rs`、`tasks.rs`、`review.rs` 及 `prompts.rs`）：

### 1. 现有时间戳处理机制
- 项目大量使用 `chrono::Utc::now()` 来生成时间戳与格式化字符串（例如 `%Y-%m-%d %H:%M UTC` 或 ISO 8601 RFC3339 `...Z` 格式）。

### 2. 识别出的时间/时区问题 (Date & Timezone Issues)
- **[问题 A] Session 列表与会话时间显示强制硬编码为 `UTC`**
  - 在 `session_manager.rs:980` 中，格式化输出会话时间的函数写死为 `format!("{} ({age})", dt.format("%Y-%m-%d %H:%M UTC"))`。
  - 对于国内用户而言，显示的时间比北京时间晚 8 小时，极易造成「今天的会话显示为昨天」或时间顺序对不上的错觉。
- **[问题 B] 注入给 Agent 的 System Context 缺少明确的本地时间/时区注入**
  - 在 `prompts.rs` 中，发送给 LLM 的系统前缀未显式包含当前本地时间与时区（例如 `Current Local Time: 2026-07-28 12:26:00 CST (Asia/Shanghai)`）。
  - 当用户提问「今天几号」「上一周的代码修改」时，Agent 容易根据模型训练时的默认 UTC 产生日期推算偏差。
- **[问题 C] 文件导出 (`/export`) 与日志格式缺乏本地化适配**
  - `/export` 生成的默认文件名或文档头部使用了 `Utc::now()` 格式化，未优先采用系统的 `Local::now()`。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 时间与时区适配维度 | mimofan (现状) | Claude Code | Gemini CLI | Codex |
|---|---|---|---|---|
| **界面 UI 时间显示** | 强硬编码为 UTC 字符串 | 根据系统环境显示 Local Time | 根据系统环境显示 Local Time | Local Time |
| **System Prompt 时间注入** | 缺乏显式本地时区注入 | 显式注入系统 Local Timestamp | 显式注入当前 Date/Time | 注入 UTC + 时区标记 |
| **文件导出时间戳** | 部分使用 `Utc::now()` | 本地系统时间 (`Local::now()`) | 本地系统时间 | 本地系统时间 |

---

## 优化计划待办 (Todo List)

- [ ] **[P0] 修复 Session 列表与 TUI 界面时间显示为本地时间 (Local Timezone / 北京时间)**
  - 修改 `session_manager.rs` 及 `ui.rs` 中的时间格式化逻辑，将 `Utc` 转换为 `Local`（例如在 CST 下格式化为 `YYYY-MM-DD HH:MM`），消除 UTC 造成的 8 小时偏差认知负荷。
- [ ] **[P0] 在 System Prompt 中注入完整的当前本地时间与时区标记**
  - 在 `prompts.rs` 中添加动态 Context 信息（如 `Current Local Time: YYYY-MM-DD HH:MM:SS (Offset: +08:00)`），确保 Agent 能够准备推导时间相关逻辑。
- [ ] **[P1] 规范文件导出 (`/export`) 与报告生成的时间戳**
  - 在 `export` 命令、Review 报告及任务持久化记录中统一采用 `Local::now()` 进行文件名及 Head 的时间渲染。
- [ ] **[P2] 单元测试**
  - 增加不同 Timezone 下时间渲染的单元测试。


---

## [x] Issue #51: 【/models 命令排查与优化】确认 /models 命令现状，升级为交互式选择 Modal 并支持参数快捷切换
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/51](https://github.com/XiaomingX/mimofan/issues/51)

### Description / Action Plan
## 背景与现状盘点

经代码审查，项目中的 **`/models` Slash 命令已经存在**：
- **位置**：`crates/tui/src/commands/groups/core/models.rs` 及 `core.rs`
- **中文别名**：`moxingliebiao`（模型列表）
- **功能定位**：调用 `AppAction::FetchModels` 异步查询当前 API Provider 暴露的可用模型列表，并打印输出到转录区。
- **关联视图**：TUI 拥有 `ModelPickerView` 弹窗界面（快捷键或配置弹出）。

---

## 识别出的缺陷与优化点 (Issues & Gaps)

虽然 `/models` 命令已实现，但与 Claude Code / Gemini CLI 的 `/models` 或 `/model` 交互相比，存在以下体验缺陷与待修复点：

### 🔴 P0 — 交互模式分割与直观度不足

1. **`/models` 仅静止打印文本，无法直接在列表中交互按键切换**
   - 当前 `/models` 仅在转录区以静态文字形式打印包含的模型名称。
   - 用户无法通过方向键在 `/models` 的输出列表中进行选择并回车直接切换当前模型，必须另行打开 `ModelPicker` 弹窗或手动输入 `/model <name>`。
2. **缺少带参数切模型支持**
   - `/models <model_name>` 直接切换模型的快捷语义缺失（用户需要知道 `/model` 与 `/models` 的微妙差异）。

### 🟡 P1 — 模型元数据与分级展示

3. **模型信息展示过于简陋**
   - 当前仅列出 Model ID 列表，未展示模型的 Context Window 上限（如 128k / 1M）、是否支持 Interleaved Thinking/Reasoning Effort、以及价格/速度等级提示。
4. **与 `ModelPickerView` 弹窗缺乏一体化路由**
   - 应使 `/models` 在交互模式下直接唤起弹窗级 `ModelPickerView`，同时在非交互 (CLI/Script) 模式下输出结构化模型列表。

---

## 与竞品对比 (Claude Code / Gemini / Codex)

| 命令能力维度 | mimofan (现状) | Claude Code | Gemini CLI |
|---|---|---|---|
| **`/models` 命令存在** | ✅ 已存在 (`core/models.rs`) | ✅ 已存在 | ✅ 已存在 |
| **交互式选择 Modal** | 静态打印文本（弹窗另开） | ✅ 直接唤起交互 Selector | ✅ 交互 Selector |
| **模型上下文/特性提示** | 仅显示名称 ID | ✅ 显示 Context Size & Specs | ✅ 显示能力标记 |
| **参数切换兼容** | 不带参数 | 支持参数快捷切换 | 支持参数快捷切换 |

---

## 修复与优化计划待办 (Todo List)

- [ ] **[P0] 升级 `/models` 为交互式 Modal / 列表直接选择体验**
  - 修改 `models.rs` / `core.rs` 中的执行逻辑：在 TUI 模式下运行 `/models` 时，直接唤起或填充 `ModelPickerView`，允许用户上下键选择并回车切换模型。
- [ ] **[P0] 支持 `/models <model_name>` 快捷传参切换**
  - 当带有参数时（如 `/models deepseek-reasoner`），直接触发模型切换动作，等价于 `/model <name>`，降低用户记忆成本。
- [ ] **[P1] 丰富模型元数据展示 (Context Size & Reasoning Tag)**
  - 在模型列表/弹窗中，为每个模型标注 context window 容积（如 `128k` / `1M`）及 `Reasoning` 能力标记。
- [ ] **[P2] 单元测试**
  - 补充 `/models` 列表获取与快捷切换的单元测试。


---

## [x] Issue #52: 【统一收口化简】保证效果前提下，排查并迁移遗留的 mimofan-tui / mimofan-cli 引用为 mimofan
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/52](https://github.com/XiaomingX/mimofan/issues/52)

### Description / Action Plan
## 目标视角

> **在保证效果（功能正确性、用户体验、编译稳定性）的前提下，全面排查当前项目中仍然使用 `mimofan-tui` 或 `mimofan-cli` 的地方，统一收口化简，完成向 `mimofan` 的最终迁移，消除遗留命名混乱，提升项目一致性和可维护性。**

---

## 现状盘点与代码实证

经代码审查（`crates/cli/`、`crates/tui/`、`Cargo.toml`、`.github/workflows/`）：

### 1. 已完成的迁移（✅）

| 组件 | 状态 | 位置 |
|------|------|------|
| 工作区 Crate 命名 | ✅ 已迁移 | `Cargo.toml` members: `crates/cli`, `crates/tui` |
| 二进制产物命名 | ✅ 已迁移 | 构建输出为 `mimofan`（非 `mimofan-tui`） |
| 文档引用 | ✅ 已迁移 | `ARCHITECTURE.md`、`README.md` 使用 `mimofan` |

### 2. 遗留引用（❌ 需清理）

#### 🔴 P0 — 代码中的遗留引用

**1. `mimofan_cli::run_cli()` 调用（3 处）**
- `crates/tui/src/main.rs:6` — `//! Delegates to mimofan_cli::run_cli()`
- `crates/tui/src/main.rs:15` — `mimofan_cli::run_cli()`
- `crates/cli/src/main.rs:15` — `mimofan_cli::run_cli()`

**问题**：虽然 crate 名称已改为 `mimofan-cli`，但代码中仍使用下划线形式 `mimofan_cli`（Rust 的 crate 名称规范）。这是**正确的**（Rust 自动将 `-` 转为 `_`），但文档注释中应保持一致。

**2. `mimofan-tui` 遗留兼容逻辑（`crates/cli/src/update.rs`）**
- L208: `if prefix == "mimofan-tui"` — 识别遗留二进制
- L210: `"mimofan-tui"` — 返回遗留名称
- L213: `if prefix == "mimofan-tui"` — 再次检查
- 注释提及 "legacy two-binary layout (mimofan + mimofan-tui)"

**问题**：这是为旧版本用户设计的兼容逻辑，用于自动更新时识别遗留二进制。属于**有意保留**的兼容代码，但应评估是否仍需要。

**3. 测试代码中的 `mimofan-tui` 引用（7 处）**
- `crates/cli/src/update.rs` 中的测试用例引用 `mimofan-tui` 作为测试输入

**问题**：测试用例验证遗留兼容逻辑是否正确工作，属于**正常测试**，但应确保测试覆盖的是当前行为而非过时逻辑。

#### 🟡 P1 — 文档与配置中的遗留引用

**4. `.kiro/specs/` 目录中的遗留引用（15+ 处）**
- `tui-lib-bin-refactor/tasks.md` — 多处 `cargo build -p mimofan-tui`
- `tui-lib-bin-refactor/design.md` — 设计文档引用旧名称
- `tui-lib-bin-refactor/requirements.md` — 需求文档引用旧名称

**问题**：`.kiro/` 是本地开发规格目录，不影响生产，但会造成混淆。

**5. `ARCHITECTURE.md` 中的遗留引用（2 处）**
- L189: `use mimofan_cli::run_cli;`
- L192: `mimofan_cli::run_cli()`

**问题**：架构文档中的代码示例使用了下划线形式，虽然语法正确，但与文档其他部分的 `mimofan` 命名不一致。

---

## 与竞品对比

| 命名一致性维度 | mimofan（现状） | Claude Code | Gemini CLI | Codex |
|---------------|----------------|-------------|------------|-------|
| **Crate/包命名** | ✅ 统一 `mimofan` | ✅ `claude-code` | ✅ `gemini-cli` | ✅ `codex` |
| **二进制命名** | ✅ 统一 `mimofan` | ✅ `claude` | ✅ `gemini` | ✅ `codex` |
| **代码引用一致性** | ⚠️ 遗留 `mimofan_cli` | ✅ 完全一致 | ✅ 完全一致 | ✅ 完全一致 |
| **文档引用一致性** | ⚠️ 部分遗留 | ✅ 完全一致 | ✅ 完全一致 | ✅ 完全一致 |
| **遗留兼容处理** | ✅ 有兼容逻辑 | N/A | N/A | N/A |

---

## 问题分析

### 🔴 P0 — 需要评估

**1. `mimofan-tui` 遗留兼容逻辑是否仍需保留？**

当前 `update.rs` 中的兼容逻辑用于：
- 检测旧版本用户是否仍有 `mimofan-tui` 二进制
- 在自动更新时正确识别和替换遗留二进制

**评估问题**：
- 距离 `mimofan-tui` → `mimofan` 迁移已过去多少版本？
- 是否仍有用户停留在需要此兼容逻辑的旧版本？
- 如果用户基数已全部迁移，可安全移除此兼容代码

**2. 文档中的 `mimofan_cli` 下划线引用是否需要统一？**

Rust 中 crate 名称 `mimofan-cli` 在代码中自动转为 `mimofan_cli`，这是语言规范。但文档中可以：
- 选项 A：保持 `mimofan_cli`（代码正确性）
- 选项 B：统一为 `mimofan`（文档简洁性）

---

## 优化计划待办

### 评估（需决策）

- [ ] **[P0] 评估遗留兼容逻辑的必要性**
  - 确认当前用户版本分布，判断 `mimofan-tui` 兼容逻辑是否仍有实际用途
  - 如果用户已全部迁移，移除 `update.rs` 中的遗留兼容代码（约 20 行）
  - 如果仍有旧版本用户，保留但添加注释说明移除时间点

- [ ] **[P0] 统一文档中的命名引用**
  - `ARCHITECTURE.md` 中的 `mimofan_cli` 示例是否改为 `mimofan`（需测试验证）
  - `.kiro/specs/` 目录中的遗留引用是否更新（低优先级，本地开发文档）

### 清理（低风险）

- [ ] **[P1] 清理 `.kiro/specs/` 中的遗留引用**
  - 更新 `tui-lib-bin-refactor/` 下的文档，将 `mimofan-tui` 改为 `mimofan`
  - 低风险，不影响编译和运行

- [ ] **[P1] 统一测试用例中的命名**
  - 确认测试用例中的 `mimofan-tui` 引用是测试遗留兼容逻辑还是过时代码
  - 如果是测试兼容逻辑，添加注释说明；如果是过时代码，更新为当前命名

### 文档（可选）

- [ ] **[P2] 更新 `USER_GUIDE.md`**
  - 确保所有命令示例使用 `mimofan` 而非 `mimofan-tui`
  - 添加迁移说明（如有旧版本用户）

- [ ] **[P2] 更新 `CHANGELOG.md`**
  - 记录遗留兼容逻辑的移除（如果决定移除）

---

## 效果保证约束（优化红线）

| 约束 | 说明 |
|------|------|
| 编译不中断 | 所有修改必须通过 `cargo build` 和 `cargo test` |
| 向后兼容 | 如果移除遗留兼容逻辑，需确保旧版本用户能平滑升级 |
| 文档准确性 | 文档中的代码示例必须可运行，不能误导开发者 |
| 测试覆盖率 | 移除兼容逻辑时，必须确认相关测试用例已更新 |

---

## 注意事项

1. **`mimofan_cli` vs `mimofan`**：Rust 中 `mimofan-cli` crate 在代码中必须写为 `mimofan_cli`，这是语言规范。文档中是否统一为 `mimofan` 需权衡代码正确性与文档简洁性。

2. **遗留兼容的生命周期**：`mimofan-tui` 兼容逻辑是为旧版本用户设计的，移除前需评估用户版本分布。建议在移除前至少保留 2 个大版本周期。

3. **`.kiro/` 目录**：这是本地开发规格目录，不影响生产环境，清理优先级较低。

4. **测试用例**：测试中的 `mimofan-tui` 引用如果是验证遗留兼容逻辑，则属于正常测试，不应随意修改。

---

## 成功标准

1. ✅ 代码中无意外的 `mimofan-tui` 引用（仅保留必要的兼容逻辑）
2. ✅ 文档中命名引用一致（统一为 `mimofan` 或明确说明 `mimofan_cli`）
3. ✅ 编译和测试全部通过
4. ✅ 遗留兼容逻辑的移除决策有据可依（用户版本数据支撑）

---

## [x] Issue #53: 【配置统一】迁移 settings.toml 为 settings.json，消除格式碎片化
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/53](https://github.com/XiaomingX/mimofan/issues/53)

### Description / Action Plan
## 目标视角

> **在保证效果（功能正确性、用户体验、向后兼容）的前提下，全面排查当前项目中对 `settings.toml` 的依赖，制定向 `settings.json` 的统一迁移计划，消除配置格式碎片化，提升项目一致性和可维护性。**

---

## 现状盘点与代码实证

经代码审查（`crates/tui/`、`crates/config/`），当前 TOML 格式配置文件涉及 **4 个模块**，分散在 **~/.mimofan/** 目录下：

### 1. TOML 格式配置文件清单

| 文件 | 模块 | 用途 | 格式 |
|------|------|------|------|
| `~/.mimofan/settings.toml` | `settings.rs` | UI 偏好（主题、语言、动画、折叠模式等） | TOML |
| `~/.mimofan/tui.toml` | `settings.rs` | TUI 特定设置（主题、字体、快捷键） | TOML |
| `~/.mimofan/config.toml` | `config.rs` | API 密钥、提供商配置、子智能体配置 | TOML |
| `~/.mimofan/skills_state.toml` | `skill_state.rs` | 技能启用/禁用状态 | TOML |

### 2. 已存在的 JSON 格式文件

| 文件 | 用途 | 格式 |
|------|------|------|
| `~/.mimofan/settings.json` | MCP 服务器配置、插件启用 | JSON |
| `~/.mimofan/workspace-trust.json` | 工作区信任状态 | JSON |
| `~/.mimofan/mcp.json` | MCP 服务器注册 | JSON |
| `~/.mimofan/composer_stash.jsonl` | Composer 暂存 | JSONL |

### 3. 代码中的 TOML 依赖（关键引用）

#### `crates/tui/src/settings.rs`
```rust
const SETTINGS_FILE_NAME: &str = "settings.toml";
const TUI_PREFS_FILE_NAME: &str = "tui.toml";

// 加载：toml::from_str(&content)
// 保存：toml::to_string_pretty(self)
// 注释保留：merge_and_preserve_comments(&serialized, &raw)
```

#### `crates/tui/src/config_persistence.rs`
```rust
// 加载：toml::from_str::<toml::Value>(&raw)
// 保存：toml::to_string_pretty(&doc)
```

#### `crates/tui/src/skill_state.rs`
```rust
const STATE_FILE_NAME: &str = "skills_state.toml";
// 加载：toml::from_str(&raw)
// 保存：toml::to_string_pretty(&on_disk)
```

#### 其他引用位置
- `crates/tui/src/lib.rs:660` — PDF 工具提示引用 `settings.toml`
- `crates/tui/src/tools/file.rs:171,180` — PDF 提取失败提示引用 `settings.toml`
- `crates/tui/src/tui/ui.rs:1590,1731` — 设置持久化注释
- `crates/tui/src/tui/app.rs:60,1033,1151,2254-2273,2813,2827` — 设置加载/保存逻辑
- `crates/tui/src/tui/theme_picker.rs:217` — 主题选择器提示
- `docs/CONFIGURATION.md` — 用户文档

---

## 问题分析

### 1. 格式碎片化

当前存在 **TOML** 和 **JSON** 两种配置格式并存：
- 新用户可能困惑：应该编辑 `.toml` 还是 `.json`？
- 开发者维护两套序列化/反序列化逻辑
- `serde_json` 已是项目依赖（`crates/tui/Cargo.toml:53`），无额外依赖成本

### 2. TOML 特性依赖

`settings.rs` 使用了 TOML 的 **注释保留功能**（`merge_and_preserve_comments`），这是 JSON 不支持的特性：
```rust
// settings.rs:622-626
let body = if path.exists() {
    let raw = std::fs::read_to_string(&path)?;
    mimofan_config::merge_and_preserve_comments(&serialized, &raw).unwrap_or_else(|e| {
        tracing::warn!("failed to merge settings comments, saving without them: {e:#}");
        serialized
    })
```

### 3. 向后兼容需求

- 老用户可能有 `settings.toml`，迁移时需保留旧文件作为 fallback
- 避免丢失用户自定义配置
- 迁移期间需支持读取两种格式

---

## 迁移策略

### 方案 A：仅迁移 settings.toml（推荐）

**范围**：仅将 `settings.rs` 中的 `settings.toml` 迁移为 `settings.json`

**理由**：
- `settings.json` 已存在并被使用（MCP 配置）
- `settings.toml` 与 `settings.json` 功能重叠
- 减少用户困惑：一个 `settings.json` 统管所有 UI 偏好

**不迁移的部分**：
- `config.toml` — 保留，作为项目级配置（API 密钥、提供商）
- `tui.toml` — 保留，作为 TUI 专用偏好（已标记 `#[allow(dead_code)]`，待 #657 完成后再评估）
- `skills_state.toml` — 保留，作为技能状态文件

### 方案 B：全量迁移

**范围**：将所有 TOML 配置迁移到 JSON

**风险**：
- 改动范围大（4 个模块）
- `config.toml` 可能有用户自定义的注释（TOML 支持，JSON 不支持）
- `skills_state.toml` 结构简单，迁移收益低

---

## 优化计划待办

### Phase 1：基础准备（低风险）

- [ ] **[P1] 创建 `settings.json` 读取/写入函数**
  - 文件：`crates/tui/src/settings.rs`
  - 新增 `SETTINGS_FILE_NAME_JSON: &str = "settings.json"`
  - 实现 `load_from_json()` 和 `save_to_json()` 方法
  - 使用 `serde_json::from_str` / `serde_json::to_string_pretty`

- [ ] **[P1] 添加迁移逻辑**
  - 文件：`crates/tui/src/settings.rs`
  - 新增 `migrate_settings_toml_to_json_if_needed()` 函数
  - 迁移策略：如果 `settings.json` 不存在但 `settings.toml` 存在，则读取 TOML 并写入 JSON
  - 迁移后保留 `settings.toml` 作为 backup（不删除）

### Phase 2：加载逻辑更新（中风险）

- [ ] **[P2] 更新 `Settings::load()` 函数**
  - 文件：`crates/tui/src/settings.rs` (L437-496)
  - 优先级：`settings.json` > `settings.toml`（向后兼容）
  - 如果 `settings.json` 存在，从 JSON 加载
  - 否则尝试从 `settings.toml` 加载（并触发迁移）

- [ ] **[P2] 更新 `Settings::save()` 函数**
  - 文件：`crates/tui/src/settings.rs` (L611-635)
  - 统一写入 `settings.json`
  - 移除 TOML 注释保留逻辑（JSON 不需要）

### Phase 3：辅助函数更新（低风险）

- [ ] **[P3] 更新 `settings_path_candidates()` 函数**
  - 文件：`crates/tui/src/settings.rs` (L1196-1214)
  - 返回值增加 JSON 路径

- [ ] **[P3] 更新 `migrate_settings_file_to_primary_if_needed()` 函数**
  - 文件：`crates/tui/src/settings.rs` (L1217-1241)
  - 适配 JSON 格式

- [ ] **[P3] 更新 `auto_compact_explicitly_configured()` 函数**
  - 文件：`crates/tui/src/settings.rs` (L500-516)
  - 使用 `serde_json::from_str` 替代 `toml::from_str`

### Phase 4：用户提示更新（低风险）

- [ ] **[P4] 更新提示文本**
  - `crates/tui/src/lib.rs:660` — `settings.json`
  - `crates/tui/src/tools/file.rs:171,180` — `settings.json`
  - `crates/tui/src/tui/ui.rs:1590,1731` — `settings.json`
  - `crates/tui/src/tui/app.rs:2262` — `settings.json`
  - `crates/tui/src/tui/theme_picker.rs:217` — `settings.json`

### Phase 5：文档更新（低风险）

- [ ] **[P5] 更新 `docs/CONFIGURATION.md`**
  - 将 `~/.mimofan/settings.toml` 改为 `~/.mimofan/settings.json`
  - 添加迁移说明

- [ ] **[P5] 更新 `USER_GUIDE.md`**
  - 相关配置路径说明

---

## 技术细节

### JSON 序列化注意事项

1. **字段命名**：JSON 使用 `snake_case`（与 TOML 一致）
2. **Option 字段**：使用 `#[serde(skip_serializing_if = "Option::is_none")]` 避免输出 `null`
3. **默认值**：使用 `#[serde(default)]` 确保向后兼容
4. **浮点数**：`f64` 在 JSON 中默认输出，需注意精度

### 迁移脚本伪代码

```rust
fn migrate_settings_toml_to_json_if_needed(json_path: &Path, toml_path: &Path) {
    if json_path.exists() || !toml_path.exists() {
        return; // 已有 JSON 或无 TOML 文件
    }
    
    let toml_content = std::fs::read_to_string(toml_path)?;
    let settings: Settings = toml::from_str(&toml_content)?;
    let json_content = serde_json::to_string_pretty(&settings)?;
    std::fs::write(json_path, json_content)?;
    
    tracing::info!("Migrated settings.toml to settings.json");
}
```

### 测试策略

- [ ] 单元测试：验证 JSON 加载/保存正确性
- [ ] 集成测试：验证迁移逻辑（TOML → JSON）
- [ ] 边界测试：空文件、损坏文件、缺少字段

---

## 效果保证约束（优化红线）

| 约束 | 说明 |
|------|------|
| 编译不中断 | 所有修改必须通过 `cargo build` 和 `cargo test` |
| 向后兼容 | 老用户的 `settings.toml` 必须能正常读取并迁移 |
| 数据不丢失 | 迁移过程中不能覆盖用户现有配置 |
| UI 不受影响 | TUI 界面设置保存/加载行为保持不变 |
| 文档准确性 | 所有文档中的配置路径必须更新 |

---

## 注意事项

1. **注释丢失**：JSON 不支持注释，用户在 `settings.toml` 中的注释会在迁移后丢失。建议在迁移时通过日志提示用户。

2. **`settings.json` 已存在**：用户的 `~/.mimofan/settings.json` 可能已有 MCP 配置。迁移时需要 **合并** 而非覆盖，或者确保 UI 偏好字段与 MCP 配置字段不冲突。

3. **`tui.toml` 和 `config.toml` 保留**：这两个文件暂不迁移，保持 TOML 格式。后续可单独评估。

4. **`merge_and_preserve_comments` 函数**：迁移完成后，如果 `config.toml` 仍使用 TOML，该函数仍需保留。

---

## 成功标准

1. ✅ `settings.json` 正确读取和保存所有 UI 偏好
2. ✅ 老用户的 `settings.toml` 自动迁移到 `settings.json`（不丢失数据）
3. ✅ TUI 界面设置保存/加载行为不变
4. ✅ 所有文档中的配置路径更新为 `settings.json`
5. ✅ `cargo build` 和 `cargo test` 全部通过
6. ✅ 无新增 clippy 警告

---

## 预估工作量

| 阶段 | 工作量 | 风险 |
|------|--------|------|
| Phase 1：基础准备 | 1-2 小时 | 低 |
| Phase 2：加载逻辑更新 | 2-3 小时 | 中 |
| Phase 3：辅助函数更新 | 1-2 小时 | 低 |
| Phase 4：用户提示更新 | 0.5 小时 | 低 |
| Phase 5：文档更新 | 0.5 小时 | 低 |
| **总计** | **5-8 小时** | **低-中** |

---

## 优先级建议

**推荐方案 A（仅迁移 settings.toml）**：
- 范围小、风险低、收益明确
- 与已存在的 `settings.json` 保持一致
- `config.toml` 和 `skills_state.toml` 留待后续评估

**实施顺序**：Phase 1 → Phase 2 → Phase 3 → Phase 4 → Phase 5

**预计完成时间**：1-2 个工作日


---

## [x] Issue #54: refactor: migrate .deepseek/ directory references to .mimofan/
- **State**: `closed`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/54](https://github.com/XiaomingX/mimofan/issues/54)

### Description / Action Plan
## Background

As part of the rebrand from DeepSeek to Mimofan, we need to migrate all  directory references to  for consistency. The  path is already the primary path in most places, but several legacy references to  remain.

## Scope

### Code Changes Required

#### 1. 
- **Line 18**: Remove `.deepseek/instructions.md` from `DOC_FILENAMES` array
- **Line 4**: Update priority comment to remove `.deepseek/instructions.md`

#### 2. 
- **Line 72**: Remove `.deepseek/` from gitignore entries
- **Line 23,63**: Update function name and comments from `ensure_deepseek_gitignored` to reflect .mimofan only

#### 3. 
- **Line 358**: Change fallback path from `.deepseek/events.jsonl` to `.mimofan/events.jsonl`

#### 4. 
- **Lines 219, 254, 361, 432, 445, 606**: Replace `.deepseekignore` with `.mimoignore` (or keep both for compatibility)

#### 5. Documentation/Comments Updates
-  - Update comment
-  - Update legacy path comment  
-  - Update comment
-  - Update path example in doc comment
-  - Update systemd example path

#### 6. Existing Migration Logic (Keep)
-  - Legacy migration from  to  should be KEPT for backward compatibility

### Testing

- Run `cargo test --workspace` to verify no regressions
- Test project doc discovery with only `.mimofan/instructions.md` present
- Test `/init` command to ensure gitignore entries are correct

## Acceptance Criteria

- [ ] All `.deepseek/` directory path references removed from codebase
- [ ] Legacy secrets migration logic preserved
- [ ] All tests pass
- [ ] Documentation updated where applicable

## Related

- Part of the rebrand initiative (mimo-tui → mimofan)

---

## [ ] Issue #55: 【上下文记忆·状态共享·缓存机制】排查与优化
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/55](https://github.com/XiaomingX/mimofan/issues/55)

### Description / Action Plan
## 背景

排查当前系统中上下文记忆、状态共享、缓存机制的实现情况，评估是否能提升系统稳定性和可观测性。

## 现有实现清单

### 1. 上下文记忆

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 用户记忆 | `memory.rs` | ✅ 已实现 | `~/.mimofanfan/memory.md` 文件，支持 `# foo` 快速追加 |
| 外部记忆服务 | `memory_service.rs` | ⚠️ 未集成 | 客户端已实现，但 `load_from_service`/`store_to_service` 标记为 `dead_code` |
| 项目上下文 | `project_context.rs` | ✅ 已实现 | AGENTS.md/CLAUDE.md/instructions.md 加载 |

### 2. 缓存机制

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| LLM 响应缓存 | `llm_response_cache.rs` | ✅ 已实现 | LRU 256条，进程级全局状态，无跨会话持久化 |
| 前缀缓存稳定性 | `prefix_cache.rs` | ✅ 已实现 | SHA-256 指纹，三区域模型，会话级稳定性管理 |
| 项目上下文缓存 | `project_context_cache.rs` | ✅ 已实现 | thread_local LRU，8条容量 |

### 3. 状态共享

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 运行时线程管理 | `runtime_threads.rs` | ✅ 已实现 | `Arc<Mutex<...>>` 共享状态 |
| 会话管理 | `session_manager.rs` | ✅ 已实现 | 会话保存/恢复 |
| 状态持久化 | `state/` | ✅ 已实现 | SQLite 持久化 |

### 4. 可观测性

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 日志追踪 | 各模块 | ✅ 已实现 | `tracing` 基础使用 |
| Telemetry | `config.rs` | ⚠️ 部分实现 | `telemetry` 配置项存在，但未见实际采集逻辑 |
| 缓存命中率 | `prompts.rs` | ⚠️ 仅提示 | footer 显示 cache hit %，无持久化统计 |

---

## 待办清单

### 已完成 [x]

- [x] 用户记忆文件加载与追加 (`memory.rs`)
- [x] LLM 响应缓存 (`llm_response_cache.rs`)
- [x] 前缀缓存稳定性管理 (`prefix_cache.rs`)
- [x] 项目上下文缓存 (`project_context_cache.rs`)
- [x] 运行时线程状态共享 (`runtime_threads.rs`)
- [x] SQLite 状态持久化 (`state/`)
- [x] 会话保存/恢复 (`session_manager.rs`)

### 待分析与处理 [ ]

- [ ] **外部记忆服务集成**：`memory_service.rs` 客户端已实现但未集成，评估是否需要接入 claude-mem 等外部服务
- [ ] **记忆系统可观测性**：当前记忆加载/追加操作无 tracing 日志，建议添加
- [ ] **缓存命中率统计**：`llm_response_cache` 无 hit/miss 统计，建议添加 metrics
- [ ] **跨会话缓存持久化**：当前 `llm_response_cache` 仅进程内，评估是否需要磁盘持久化
- [ ] **Telemetry 采集**：`telemetry` 配置项存在但未实现采集逻辑，评估是否需要
- [ ] **状态共享锁优化**：`runtime_threads.rs` 使用 `Mutex`，评估是否需要升级为 `RwLock`
- [ ] **记忆系统错误处理**：`load_from_service`/`store_to_service` 错误被静默忽略，建议添加日志
- [ ] **缓存容量配置化**：各缓存容量硬编码，评估是否需要暴露为配置项

---

## 相关 Issue

- #34 【记忆能力】
- #37 【内存优化】
- #43 【内存泄漏排查】

---

## 验证方法

```bash
# 编译检查
cargo check -p mimofan

# 测试验证
cargo test -p mimofan --lib -- memory
cargo test -p mimofan --lib -- cache

# Clippy 检查
cargo clippy -p mimofan --lib
```

---

## [ ] Issue #56: 【Mock/Placeholder/未实现功能】排查与清理
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/56](https://github.com/XiaomingX/mimofan/issues/56)

### Description / Action Plan
## 背景

排查当前系统中理应实现但仍是 mock 数据、placeholder 或未实现的功能。

---

## 发现清单

### 1. 明确标记为 placeholder / not yet implemented

| 模块 | 文件 | 位置 | 说明 |
|------|------|------|------|
| StatusItem | `config.rs:784-786` | `LastToolElapsed`, `RateLimit` | "placeholder until wired" — 状态栏项未接线 |
| Remote Setup | `remote_setup/mod.rs:116-118` | `--apply` 路径 | "auto-provision not yet implemented" — 云端自动配置未实现 |
| CLI Remote Setup | `cli/src/lib.rs:354` | `remote-setup` | "not yet implemented" |

### 2. 标记为 deferred / wired in a later pass

| 模块 | 文件 | 位置 | 说明 |
|------|------|------|------|
| TuiPrefs | `settings.rs:44,72,93` | 整个模块 | "deferred to a later settings pass (#657)" — TUI 偏好设置未接入 |
| ContextBudget | `context_budget.rs:36-44` | 整个模块 | "consumers are wired in a later pass" — 上下文预算计算未接入引擎 |
| ModelRegistry | `model_registry.rs:31-33` | 整个模块 | "consumers are wired in a later pass" — 模型注册表未接入生产代码 |
| PromptZones | `prompt_zones.rs:22,300,329` | `AppendLog`/TurnScratch` | "not yet wired into the request path" — 三区域提示词合同未接入 |

### 3. 有 #[allow(dead_code)] 的功能模块

| 模块 | 文件 | 说明 |
|------|------|------|
| GoalLoop | `goal_loop.rs` | 目标循环编排器，多个枚举变体未使用 |
| Palette | `palette.rs` | 大量颜色函数未使用 |
| Features | `features.rs` | 整个模块 `allow(dead_code)` |
| Prompts | `prompts.rs` | 整个模块 `allow(dead_code)` |
| SkillState | `skill_state.rs` | 部分字段未使用 |

### 4. 外部记忆服务（已实现客户端但未集成）

| 模块 | 文件 | 说明 |
|------|------|------|
| MemoryService | `memory_service.rs` | 客户端已实现，但 `load_from_service`/`store_to_service` 标记为 `dead_code` |

---

## 待办清单

### 已完成 [x]

- [x] 用户记忆文件加载 (`memory.rs`)
- [x] LLM 响应缓存 (`llm_response_cache.rs`)
- [x] 前缀缓存稳定性管理 (`prefix_cache.rs`)
- [x] 项目上下文缓存 (`project_context_cache.rs`)
- [x] Remote Setup Bundle 生成 (`remote_setup/bundle.rs`)

### 待分析与处理 [ ]

- [ ] **TuiPrefs 接入 (#657)**：`settings.rs` 中的 `TuiPrefs` 模块已实现但未接入启动流程，需评估接入时机
- [ ] **ContextBudget 接入引擎**：`context_budget.rs` 的预算计算未接入引擎容量检查点和 TUI 压力指示器
- [ ] **ModelRegistry 接入生产代码**：`model_registry.rs` 的模型注册表未替换 `models.rs` 中分散的硬编码
- [ ] **PromptZones 三区域合同接入**：`prompt_zones.rs` 的 `AppendLog`/TurnScratch` 未接入请求路径
- [ ] **StatusItem 接线**：`LastToolElapsed` 和 `RateLimit` 状态项未接线到实际数据源
- [ ] **Remote Setup --apply 实现**：云端自动配置路径（`--apply`）未实现，当前仅生成 bundle
- [ ] **外部记忆服务集成**：`memory_service.rs` 客户端已实现但未集成到会话流程
- [ ] **GoalLoop Dead Code 清理**：评估 `Blocked`/ContinuationLimit` 枚举变体是否需要实现或移除
- [ ] **Palette Dead Code 清理**：评估未使用的颜色函数是否需要实现或移除
- [ ] **Features 模块评估**：评估 `features.rs` 是否需要接入或移除

---

## 相关 Issue

- #657 — TuiPrefs 设置 pass
- #891 / #1976 / #2058 / #2029 — GoalLoop lineage
- #2264 — 三区域提示词合同
- #3071 / #3073 — ModelRegistry
- #3215 — GoalLoop orchestrator

---

## 验证方法

```bash
# 检查 dead_code 警告
cargo check -p mimofan 2>&1 | grep "dead_code"

# 检查 TODO/FIXME
grep -rn "TODO.*implement\|FIXME.*implement\|not yet implemented" crates/

# 检查 #[allow(dead_code)]
grep -rn "#\[allow(dead_code)\]" crates/ | grep -v test
```

---

## [ ] Issue #57: 【结构化日志查询】问题-答案记录筛选与长程任务分析
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/57](https://github.com/XiaomingX/mimofan/issues/57)

### Description / Action Plan
## 背景

用户需要在日志中查询结构化的问题和答案记录，以便筛选长程任务进行标注和分析模型优劣。

---

## 现有实现

### 1. 会话存储结构

**位置**: `~/.mimofanfan/sessions/` 目录

**文件格式**: JSON (`session_YYYYMMDD_HHMMSS.json`)

**SessionMetadata 结构** (`session_manager.rs:100-135`):
```json
{
  "id": "session-uuid",
  "title": "从首条消息提取的标题",
  "created_at": "2026-07-28T12:00:00Z",
  "updated_at": "2026-07-28T12:30:00Z",
  "message_count": 15,
  "total_tokens": 125000,
  "model": "mimo-v2.5-pro",
  "workspace": "/path/to/project",
  "mode": "agent",
  "cost": {
    "session_cost_usd": 0.05,
    "session_cost_cny": 0.35,
    "subagent_cost_usd": 0.02,
    "subagent_cost_cny": 0.14
  },
  "parent_session_id": null,
  "cumulative_turn_secs": 180
}
```

**消息结构** (`messages` 数组):
```json
{
  "role": "user|assistant|system",
  "content": [{"type": "text", "text": "..."}],
  "model": "mimo-v2.5-pro",
  "usage": {"input_tokens": 1000, "output_tokens": 500}
}
```

### 2. 运行时日志

**位置**: `~/.mimofanfan/logs/tui-YYYY-MM-DD-PID.log`

**格式**: tracing-subscriber 格式化日志

**内容**: 
- 工具调用记录
- 错误和警告
- 性能指标
- 网络请求

### 3. 会话导出

**命令**: `/export [path]`

**输出格式**: Markdown (`chat_export_YYYYMMDD_HHMMSS.md`)

**内容结构**:
```markdown
# Chat Export

**Model:** mimo-v2.5-pro
**Workspace:** /path/to/project
**Date:** 2026-07-28 12:00:00

---

**You:**
用户问题...

---

**Assistant:**
模型回答...

---

**Tool:**
工具调用详情...
```

### 4. 会话搜索

**API**: `SessionManager::search_sessions(query)`

**功能**: 按标题搜索会话

**限制**: 仅支持标题搜索，不支持内容搜索

---

## 待办清单

### 已完成 [x]

- [x] 会话元数据存储 (id, title, model, tokens, cost, timestamps)
- [x] 消息内容存储 (role, content, usage)
- [x] 会话导出为 Markdown
- [x] 会话标题搜索
- [x] 运行时日志记录 (tracing)

### 待分析与处理 [ ]

- [ ] **内容搜索 API**：当前 `search_sessions` 仅支持标题搜索，需扩展支持消息内容搜索
- [ ] **结构化查询接口**：提供按 model/tokens/cost/duration 等字段筛选的 API
- [ ] **批量导出工具**：支持批量导出会话为结构化格式 (JSON/CSV)
- [ ] **长程任务识别**：基于 message_count/cumulative_turn_secs 等字段自动识别长程任务
- [ ] **会话标签系统**：允许用户为会话添加标签 (如 "长程任务", "代码生成", "调试")
- [ ] **统计分析 API**：提供按模型/时间段/工作区等维度的统计分析
- [ ] **日志结构化**：将运行时日志转换为结构化格式 (JSON Lines)
- [ ] **查询 CLI 命令**：提供 `mimofan sessions query` 命令支持复杂查询

---

## 使用示例

### 当前可用的查询方式

```bash
# 1. 列出所有会话
ls ~/.mimofanfan/sessions/

# 2. 查看会话元数据
cat ~/.mimofanfan/sessions/session_*.json | jq '.metadata'

# 3. 搜索会话标题
grep -l "关键词" ~/.mimofanfan/sessions/*.json

# 4. 统计会话数量
ls ~/.mimofanfan/sessions/*.json | wc -l

# 5. 查看特定模型的会话
cat ~/.mimofanfan/sessions/*.json | jq 'select(.metadata.model == "mimo-v2.5-pro")'

# 6. 查看长程任务 (消息数 > 20)
cat ~/.mimofanfan/sessions/*.json | jq 'select(.metadata.message_count > 20)'

# 7. 查看高 token 消耗会话
cat ~/.mimofanfan/sessions/*.json | jq 'select(.metadata.total_tokens > 100000)'

# 8. 查看运行时日志
cat ~/.mimofanfan/logs/tui-*.log | grep "tool_call"
```

### 期望的查询方式 (待实现)

```bash
# 按内容搜索
mimofan sessions query --content "API 设计"

# 按模型筛选
mimofan sessions query --model mimo-v2.5-pro

# 按 token 范围筛选
mimofan sessions query --min-tokens 50000 --max-tokens 200000

# 按时间范围筛选
mimofan sessions query --after 2026-07-01 --before 2026-07-31

# 识别长程任务
mimofan sessions query --long-task --min-messages 20

# 批量导出
mimofan sessions export --format csv --output sessions.csv

# 统计分析
mimofan sessions stats --by-model --by-date
```

---

## 相关 Issue

- #34 【记忆能力】
- #39 【Replay Session 性能优化】

---

## 验证方法

```bash
# 检查会话目录
ls -la ~/.mimofanfan/sessions/

# 检查会话文件格式
cat ~/.mimofanfan/sessions/*.json | head -1 | jq .

# 检查日志目录
ls -la ~/.mimofanfan/logs/

# 测试导出功能
# 在 TUI 中执行 /export test.md
```

---

## [ ] Issue #58: 【.claude/ vs .mimofan/ 对比分析】功能差距与改进建议
- **State**: `open`
- **Link**: [https://github.com/XiaomingX/mimofan/issues/58](https://github.com/XiaomingX/mimofan/issues/58)

### Description / Action Plan
## 背景

对比 `.claude/` 和 `.mimofan/` 目录结构与配置，识别 `.claude/` 设计更优的功能点，提出改进建议。

---

## 目录结构对比

### `.claude/` 目录 (29 项)

| 目录/文件 | 说明 | `.mimofan/` 是否存在 |
|-----------|------|------------------------|
| `CLAUDE.md` | 全局指令文件 | ❌ 无 |
| `settings.json` | 全局配置 | ✅ 有 |
| `mcp.json` | MCP 服务器配置 | ❌ 无（在 config.toml 中） |
| `history.jsonl` | 完整对话历史 | ❌ 无 |
| `plans/` | 计划文件存储 | ❌ 无 |
| `tasks/` | 任务状态存储 | ❌ 无 |
| `projects/` | 项目级配置 | ❌ 无 |
| `file-history/` | 文件修改历史 | ❌ 无 |
| `plugins/` | 插件管理 | ❌ 无 |
| `workflows/` | 工作流定义 | ❌ 无 |
| `sessions/` | 会话存储 | ✅ 有 |
| `skills/` | 技能定义 | ✅ 有 |
| `telemetry/` | 遥测数据 | ❌ 无 |
| `backups/` | 备份目录 | ❌ 无 |
| `debug/` | 调试信息 | ❌ 无 |
| `shell-snapshots/` | Shell 快照 | ❌ 无 |
| `paste-cache/` | 粘贴缓存 | ❌ 无 |
| `session-env/` | 会话环境变量 | ❌ 无 |
| `cache/` | 通用缓存 | ❌ 无 |
| `templates/` | 模板目录 | ❌ 无 |

### `.mimofan/` 目录 (18 项)

| 目录/文件 | 说明 | `.claude/` 是否存在 |
|-----------|------|------------------------|
| `config.toml` | 主配置 | ❌ 无（在 settings.json） |
| `settings.json` | UI 偏好 | ✅ 有 |
| `state.db` | SQLite 状态 | ❌ 无 |
| `memory.db` / `memory.sqlite` | 记忆存储 | ❌ 无 |
| `audit.log` | 审计日志 | ❌ 无 |
| `sessions/` | 会话存储 | ✅ 有 |
| `skills/` | 技能定义 | ✅ 有 |
| `logs/` | 日志目录 | ❌ 无 |
| `crashes/` | 崩溃转储 | ❌ 无 |
| `secrets/` | 密钥存储 | ❌ 无 |
| `hooks/` | 钩子脚本 | ❌ 无 |
| `automations/` | 自动化任务 | ❌ 无 |
| `state/` | 状态目录 | ❌ 无 |
| `tool_outputs/` | 工具输出 | ❌ 无 |

---

## `.claude/` 设计优势（待改进项）

### 1. 全局指令文件 `CLAUDE.md` ⭐⭐⭐

**`.claude/` 设计**:
- 全局 `CLAUDE.md` 定义跨项目的编码规范、技术栈规则
- 项目级 `CLAUDE.md` 定义项目特定规则
- 自动加载，无需手动配置

**`.mimofan/` 现状**:
- 无全局指令文件
- 仅通过 `instructions` 字段指定单个文件

**改进建议**:
- [ ] 支持 `~/.mimofan/AGENTS.md` 作为全局指令
- [ ] 支持项目级 `.mimofan/AGENTS.md` 覆盖
- [ ] 自动合并全局和项目级指令

---

### 2. 文件系统忽略规则 `filesystem.ignore` ⭐⭐⭐

**`.claude/` 设计**:
`settings.json` 中定义详细的文件忽略规则：
```json
"filesystem": {
  "ignore": [
    "**/node_modules/**",
    "**/.next/**",
    "**/dist/**",
    "**/*.log",
    "**/.env*"
  ]
}
```

**`.mimofan/` 现状**:
- 无文件忽略配置
- 依赖硬编码的忽略列表

**改进建议**:
- [ ] 在 `settings.json` 中添加 `filesystem.ignore` 字段
- [ ] 支持用户自定义忽略规则
- [ ] 合并默认规则和用户规则

---

### 3. 项目级配置目录 `projects/` ⭐⭐

**`.claude/` 设计**:
- 按项目路径存储配置：`projects/-Users-a0000-mywork-.../settings.json`
- 支持项目特定的权限、模型、MCP 服务器配置

**`.mimofan/` 现状**:
- 仅 `config.toml` 中的 `[projects]` 部分
- 无独立的项目配置目录

**改进建议**:
- [ ] 创建 `~/.mimofan/projects/` 目录
- [ ] 支持项目级 `settings.json`
- [ ] 支持项目级 MCP 服务器配置

---

### 4. 计划文件存储 `plans/` ⭐⭐

**`.claude/` 设计**:
- 存储 `/make-plan` 生成的计划文件
- 按任务 ID 命名：`01-api-proxy-fix.md`
- 支持计划的持久化和复用

**`.mimofan/` 现状**:
- 无计划文件存储
- 计划仅在会话中存在

**改进建议**:
- [ ] 创建 `~/.mimofan/plans/` 目录
- [ ] 支持 `/make-plan` 输出持久化
- [ ] 支持计划的加载和复用

---

### 5. 任务状态存储 `tasks/` ⭐⭐

**`.claude/` 设计**:
- 存储后台任务状态
- JSON 格式：`{id}.json`
- 支持任务的持久化和恢复

**`.mimofan/` 现状**:
- `state.db` 中存储任务
- 无独立的任务文件

**改进建议**:
- [ ] 创建 `~/.mimofan/tasks/` 目录
- [ ] 支持任务状态的文件存储
- [ ] 支持任务的导入/导出

---

### 6. 文件修改历史 `file-history/` ⭐⭐

**`.claude/` 设计**:
- 按会话存储文件修改历史
- 支持文件的版本回溯
- 用于 `/undo` 命令

**`.mimofan/` 现状**:
- 无文件修改历史
- 不支持 `/undo`

**改进建议**:
- [ ] 创建 `~/.mimofan/file-history/` 目录
- [ ] 记录每次文件修改
- [ ] 支持 `/undo` 命令

---

### 7. 插件系统 `plugins/` ⭐⭐

**`.claude/` 设计**:
- 支持第三方插件（LSP、代码简化等）
- 插件市场机制
- 插件配置和缓存

**`.mimofan/` 现状**:
- `skills/` 目录，但功能有限
- 无插件市场

**改进建议**:
- [ ] 扩展 `skills/` 为完整的插件系统
- [ ] 支持插件的安装、更新、卸载
- [ ] 创建插件市场

---

### 8. MCP 服务器独立配置 `mcp.json` ⭐⭐

**`.claude/` 设计**:
- 独立的 `mcp.json` 文件
- 支持多个 MCP 服务器
- 每个服务器可独立启用/禁用

**`.mimofan/` 现状**:
- MCP 配置在 `config.toml` 中
- 配置分散

**改进建议**:
- [ ] 支持 `~/.mimofan/mcp.json` 独立配置
- [ ] 支持 MCP 服务器的动态管理
- [ ] 支持 MCP 服务器的热重载

---

### 9. 权限系统 `permissions` ⭐⭐

**`.claude/` 设计**:
```json
"permissions": {
  "allow": [],
  "unmatchedCommand": "allow",
  "unmatchedFileAccess": "allow"
}
```

**`.mimofan/` 现状**:
- 无细粒度权限控制
- 仅 `trust_level` 配置

**改进建议**:
- [ ] 在 `settings.json` 中添加 `permissions` 字段
- [ ] 支持命令级别的权限控制
- [ ] 支持文件访问的权限控制

---

### 10. 工作流定义 `workflows/` ⭐⭐

**`.claude/` 设计**:
- 存储 `/workflow` 定义
- 支持工作流的复用

**`.mimofan/` 现状**:
- 无工作流目录

**改进建议**:
- [ ] 创建 `~/.mimofan/workflows/` 目录
- [ ] 支持工作流的定义和存储
- [ ] 支持工作流的执行

---

### 11. 遥测数据 `telemetry/` ⭐

**`.claude/` 设计**:
- 存储使用遥测数据
- 用于性能分析和改进

**`.mimofan/` 现状**:
- 无遥测数据目录

**改进建议**:
- [ ] 创建 `~/.mimofan/telemetry/` 目录
- [ ] 收集匿名使用数据
- [ ] 支持遥测的启用/禁用

---

### 12. 完整对话历史 `history.jsonl` ⭐

**`.claude/` 设计**:
- JSONL 格式存储完整对话
- 支持对话的搜索和分析

**`.mimofan/` 现状**:
- 仅 `sessions/` 中的摘要

**改进建议**:
- [ ] 支持 `~/.mimofan/history.jsonl` 完整历史
- [ ] 支持对话的搜索
- [ ] 支持对话的导出

---

## 优先级排序

| 优先级 | 改进项 | 预计工作量 |
|--------|--------|------------|
| P0 | 全局指令文件 `AGENTS.md` | 2-3 天 |
| P0 | 文件忽略规则 `filesystem.ignore` | 1-2 天 |
| P1 | 项目级配置目录 | 2-3 天 |
| P1 | 计划文件存储 | 1-2 天 |
| P1 | 文件修改历史 | 3-5 天 |
| P2 | 任务状态存储 | 2-3 天 |
| P2 | 插件系统扩展 | 5-7 天 |
| P2 | MCP 独立配置 | 1-2 天 |
| P2 | 权限系统 | 2-3 天 |
| P3 | 工作流定义 | 2-3 天 |
| P3 | 遥测数据 | 2-3 天 |
| P3 | 完整对话历史 | 3-5 天 |

---

## 验证方法

```bash
# 检查目录结构
ls -la ~/.mimofan/

# 检查配置文件
cat ~/.mimofan/settings.json | python3 -m json.tool

# 测试全局指令加载
# 创建 ~/.mimofan/AGENTS.md 后重启 TUI
```

---

