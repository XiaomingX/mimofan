# Claude 项目指南

## 项目概述

Mimofan 是一个基于 Rust 的终端 AI 编程助手（对标 opencode / claude code），
支持多种 LLM 服务商（DeepSeek、OpenAI、Anthropic、Zai 等），提供
TUI 界面、子智能体系统和 MCP 工具集成。

## 工作区 Crate（14 个成员）

```
mimofan-app-server   HTTP 应用服务器（axum）
mimofan              TUI 界面 + CLI 入口（ratatui），运行时 API，任务管理，
                       工具执行循环，模型/服务商选择器
mimofan-core         核心引擎：轮次循环、会话、事件
mimofan-config       配置：服务商、路由、模型清单
mimofan-protocol     协议定义：工具、消息格式
mimofan-agent        子智能体系统
mimofan-tools        内置工具实现
mimofan-mcp          MCP 服务器集成
mimofan-hooks        工具前后钩子
mimofan-execpolicy   执行策略（安全沙箱）
mimofan-secrets      密钥/密钥管理
mimofan-state        状态持久化（通过 rusqlite 的 SQLite）
mimofan-release      发布工具
```

默认成员：`app-server`、`tui`

### Crate 依赖流（简化版）

```
app-server             （二进制 crate，HTTP 入口点）
  └─ core               （引擎、轮次循环、会话）

tui                     （二进制 crate，自己实现运行时，不依赖 core）
  ├─ config             （服务商、路由、模型配置）
  ├─ protocol           （工具和消息类型）
  ├─ tools              （内置工具）
  ├─ execpolicy         （沙箱）
  └─ secrets            （密钥管理）

core → config, protocol, agent, tools, mcp, hooks, execpolicy, secrets, state
config → execpolicy, secrets
hooks → protocol
tools → protocol
```

## 技术栈

- Rust 2024 版本，rust-version = "1.88"（需要 `let_chains` 特性）
- tokio（完整特性）异步运行时
- ratatui TUI 框架
- clap CLI 解析
- serde / serde_json / toml 序列化
- reqwest HTTP 客户端（rustls）
- rusqlite SQLite（bundled）
- axum HTTP 框架
- tracing 日志

## 构建 / 测试 / 格式化快速参考

```bash
cargo fmt                                         # 格式化所有代码
cargo test -p mimofan --locked                  # TUI 测试
cargo test -p mimofan-config                    # 配置测试
cargo test -p mimofan-protocol                  # 协议测试
cargo test --workspace                            # 完整工作区测试
cargo build --release -p mimofan          # Release 构建
```

### 已知测试问题（预先存在，非回归）

- `config_command_allow_shell_*` 在 `~/.mimofan/settings.toml` 有
  `default_mode = "yolo"` 时失败（测试不是隔离的）
- `run_verifiers_background_*` 在完整套件并行时不稳定，但单独运行时通过

## 代码风格约定

- Rust 标准风格；每次提交前运行 `cargo fmt`
- 无 `any` 等效类型：避免使用 `Box<dyn Any>`，改用 trait 或枚举
- 库错误类型优先使用 `thiserror`，二进制 crate 只用 `anyhow`
- 所有代码都通过 tokio 异步；除非是 I/O 密集型且经过测量，否则避免使用 `spawn_blocking`
- 每次提交只关注一个问题；写真实的提交信息
- 除非行为已实际验证（构建了二进制、运行了测试、复现了修复），否则提交为 **WIP**

## 服务商系统

Mimofan 只支持云 API 服务商。本地推理服务商（Ollama）和
各种冗余云服务商（HuggingFace、DeepInfra、Together、Arcee、Fireworks、
Novita、WanjieArk）已被明确移除。服务商配置位于
`mimofan-config`，路由解析在 `config/src/route/`。

详见 `config/config.example.toml` 了解服务商配置。

## 关键设计约束

1. **仅限智能体表面**：面向模型的子智能体工具是 **`agent` 仅**。
   不存在 `agent_open` / `agent_eval` / `agent_close` / `delegate_to_agent`。
2. **无运行时提示词/标签注入**：`constitution.md`（通过
   `~/.mimofan/constitution.json`）是唯一的基础提示词。
3. **子智能体深度可配置**；除非明确需要并解释清楚，否则不要新增任意限制。
4. **子智能体 TUI 冻结已解决**：v0.8.61 切换修复了它。不要提交
   推测性的 `spawn_blocking` 修复。

## 文件组织

```
ARCHITECTURE.md           架构文档（中文，根目录）
docs/CONFIGURATION.md     配置指南
docs/SUBAGENTS.md         子智能体指南
docs/MCP.md               MCP 集成指南
docs/MODES.md             模式（plan/agent/yolo）
docs/PROMPTS.md           提示词工程索引
config/config.example.toml  示例配置
~/.mimofan/settings.toml          用户设置
~/.mimofan/constitution.json      宪法（基础提示词）
```

### 大文件（已从 .claudeignore 排除）

下列文件稳定且很少修改，已从 `.claudeignore` 排除以节省 token；行数会变动，不在此逐一标注。完整索引见 `ARCHITECTURE.md` 第 8 节。

- `crates/tui/src/localization.rs` — TUI 字符串翻译（zh-Hans），`tr(MessageId)`、`Locale`
- `crates/tui/src/prompts.rs` — 模式系统提示词，`build_system_prompt()`
- `crates/tui/src/tui/widgets/mod.rs` — UI 组件实现
- `crates/tui/src/tui/views/mod.rs` — 模态框/对话框视图系统
- `crates/tui/src/tui/ui.rs` — UI 渲染主循环
- `crates/tui/src/lib.rs` — 模块声明与 re-export
- `crates/tui/src/tools/subagent/mod.rs` — 子智能体工具
- `crates/tui/src/tui/app.rs` — TUI 应用状态机
- `crates/tui/src/config.rs` — TUI 配置管理
- `crates/tui/src/runtime_threads.rs` — 线程运行时
- `crates/tui/src/core/engine.rs` — 引擎核心
- `crates/tui/src/runtime_api.rs` — 运行时 API
- `crates/tui/src/tools/shell.rs` — Shell 工具
- `crates/tui/src/mcp.rs` — MCP 集成

## 详细文档

深入了解请阅读 `docs/` 下的相应文档：
- 架构和 crate 关系：`ARCHITECTURE.md`（根目录）
- 配置和路由：`docs/CONFIGURATION.md`
- 子智能体系统：`docs/SUBAGENTS.md`
- MCP 集成：`docs/MCP.md`
- 操作模式：`docs/MODES.md`
- 提示词工程：`docs/PROMPTS.md`

---

## 性能最佳实践

### 内存与分配

- **避免不必要的 `.clone()`**：代码库在 `subagent/mod.rs`、`ui.rs`、`engine.rs`、`runtime_threads.rs` 中有高密度的 `clone()`。在生命周期允许的情况下优先使用借用（`&str` 优于 `String`，`&[T]` 优于 `Vec<T>`）。使用 `Arc::clone()` 进行共享所有权，而不是深拷贝数据。
- **最小化热路径中的 `.to_string()`**：UI 渲染（`ui.rs` 约 200+ 次调用）和测试文件是最大的问题。仅在拼接时使用 `format!`；对于静态文本优先使用 `&str` 引用。对于 `Display` 类型，使用 `write!` 写入复用的缓冲区，而不是重复的 `to_string()`。
- **读多写少的数据优先使用 `RwLock` 而非 `Mutex`**：引擎和运行时正确使用了 `tokio::sync::RwLock` 进行共享状态。访问配置/会话/注册表的新代码应遵循此模式。`Mutex` 仅适用于短临界区且读取不频繁的情况。
- **大型负载使用 `Bytes` / `Cow<'static, str>`**：LLM 响应、流式块和工具输出可能很大。避免在异步边界间复制大字符串；尽可能使用 `bytes::Bytes` 或 `Cow`。

### 异步与并发

- **结构化并发**：使用 `tokio::select!` 配合 `CancellationToken` 进行取消（已在引擎中使用）。避免没有句柄的原始 `tokio::spawn`——使用 `utils.rs` 中的 `spawn_supervised` 模式。
- **通道模式**：使用 `mpsc` 进行生产者-消费者（engine→UI），`broadcast` 进行扇出（事件），`oneshot` 进行请求-响应。代码库已遵循此模式；新代码也必须如此。
- **避免阻塞运行时**：`spawn_blocking` 仅对 CPU 密集型工作（git rev-parse、文件哈希）是合理的。I/O 密集型阻塞（文件读取）应使用 `tokio::fs`。代码库在约 24 个地方使用了 `spawn_blocking`——在新增之前验证必要性。
- **批量操作**：使用 `FuturesUnordered` 分组相关异步操作（已在引擎的子智能体编排中使用）。避免在可能并行时顺序 `await`。

### HTTP 与网络

- **连接复用**：`reqwest::Client` 通过 `Arc` 共享（在 `client.rs` 中）。永远不要为每个请求创建新客户端。
- **流式处理**：对 LLM 响应使用 `reqwest` 流式处理（已到位）。在发送前缓冲工具输出以避免部分消息开销。
- **重试策略**：遵循 `llm_client/mod.rs` 中现有的 `with_retry` 模式，使用指数退避 + 抖动。尊重 `Retry-After` 头。

---

## 可维护性最佳实践

### 文件大小与模块结构

- **需要分解的热点文件**：
  - `tui/ui.rs`（11,317 行）—— 拆分为 `ui/chat.rs`、`ui/sidebar.rs`、`ui/footer.rs`、`ui/picker.rs` 等
  - `tui/lib.rs`（6,827 行）—— 模块声明与 re-export，可按子目录分组
  - `tools/subagent/mod.rs`（6,584 行）—— 子智能体工具，可拆分为独立子模块
- **目标**：源文件不超过 1,000 行。测试文件应镜像源结构，每个不超过 500 行。
- **模块深度**：`tui/src/` 目录有 71 个顶层 `.rs` 文件——考虑将相关模块分组到子目录（例如 `tools/`、`core/`、`fleet/` 已存在；`config_*`、`model_*`、`runtime_*` 模块应类似分组）。

### 依赖卫生

- **工作区依赖**：所有共享 crate（`serde`、`tokio`、`anyhow` 等）在 `[workspace.dependencies]` 中声明。新依赖必须先在此添加，然后在 crate `Cargo.toml` 中用 `workspace = true` 引用。
- **特性标志**：`tui` crate 有 `tui`/`web`/`json`/`toml` 特性。保持特性最小化，避免用特性门控核心逻辑。
- **无重复版本**：定期运行 `cargo tree -d` 检查版本漂移。CI `check-versions.sh` 脚本强制执行。

### 错误处理

- **库 crate**（`config`、`protocol`、`secrets`、`execpolicy`、`tools`）：使用 `thiserror` 进行类型化错误。定义 `enum FooError` 并加 `#[derive(Error)]`。
- **二进制 crate**（`tui`、`cli`、`app-server`）：使用 `anyhow::Result` 配合 `.context()` 获取丰富的错误链。使用 `bail!` 进行提前返回。
- **反模式：裸 `unwrap()`**：代码库有约 2,600 个 `unwrap()` 调用。在生产代码中，替换为 `?` 或 `.expect("reason")`。在测试中，首选带描述性消息的 `expect()` 而非裸 `unwrap()`。热点：`mcp/tests.rs`（233）、`fleet/manager.rs`（117）、`snapshot/repo.rs`（116）。
- **反模式：静默吞错**：永远不要 `let _ = result` 用于可失败操作，除非有注释解释为什么错误可以忽略。

### 测试标准

- **覆盖率**：5,654 个同步测试 + 531 个异步测试。新功能需要测试。
- **测试命名**：使用 `test_<模块>_<场景>_<预期>` 模式。
- **异步测试**：对异步代码使用 `#[tokio::test]`。对时间敏感测试优先使用 `#[tokio::test(start_paused = true)]`。
- **测试隔离**：测试不能依赖外部状态（`~/.mimofan/`、环境变量）。使用 `tempfile` 和环境守卫（`config/tests.rs` 中的 `EnvGuard`）。
- **快照测试**：在适当时使用 `insta` 或 `expect_test` 进行 UI 输出验证。
- **集成测试**：放在 `tests/` 目录，带 `support/` 辅助工具。

---

## 代码质量规则

### Clippy 配置

CI 运行 clippy：
```bash
cargo clippy --workspace --all-features --locked -- \
  -D warnings \
  -A clippy::uninlined_format_args \
  -A clippy::too_many_arguments \
  -A clippy::unnecessary_map_or \
  -A clippy::assertions_on_constants
```

新代码必须通过 clippy 而不添加新的 `#[allow(clippy::...)]` 属性。
目前 40 个文件有 clippy allow 属性——减少，不要增加。

### 格式化

- 无 `rustfmt.toml`——使用默认 rustfmt 设置。
- CI 强制执行 `cargo fmt --all -- --check`。
- 提交前始终运行 `cargo fmt`。

### 类型安全

- **无 `Box<dyn Any>`**：使用枚举或 trait 进行多态。代码库中 `Box<dyn>` 使用极少（约 10 个文件）——保持如此。
- **新类型模式**：优先使用 `struct ProviderId(String)` 而非原始 `String` 作为领域标识符（已在 `route/ids.rs` 中使用）。
- **Serde 卫生**：在配置结构体上使用 `#[serde(deny_unknown_fields)]`。对具有合理默认值的可选字段使用 `#[serde(default)]`。

### 并发安全

- **`Arc<Mutex<T>>` vs `Arc<RwLock<T>>`**：读主导时使用 `RwLock`（配置、会话状态）。写密集或短临界区使用 `Mutex`。
- **避免锁顺序错误**：同时持有多个锁时在注释中记录锁获取顺序。
- **`CancellationToken`**：用于生成任务的优雅关闭。引擎已使用此模式。

### 死锁防护模式

**问题**：持有写锁时调用获取读锁的方法会导致死锁。

```rust
// ❌ 错误：死锁！
pub async fn start_task(&self, task_id: &str) -> Result {
    let mut tasks = self.tasks.write().await;  // 获取写锁
    let mut running = self.running.write().await;  // 获取写锁

    // is_ready() 尝试获取 tasks.read() 和 running.read() → 死锁！
    if !self.is_ready(task_id).await {
        return Err(...);
    }
    // ...
}
```

**解决方案 1**：创建内部辅助方法，接受预获取的锁引用

```rust
// ✅ 正确：内部方法接受锁引用
fn is_ready_internal(
    task_id: &str,
    tasks: &HashMap<String, Task>,
    running: &HashSet<String>,
) -> bool {
    // 使用已获取的锁，不再尝试获取新锁
    tasks.get(task_id).map(|t| /* 检查逻辑 */).unwrap_or(false)
}

pub async fn is_ready(&self, task_id: &str) -> bool {
    let tasks = self.tasks.read().await;
    let running = self.running.read().await;
    Self::is_ready_internal(task_id, &tasks, &running)
}

pub async fn start_task(&self, task_id: &str) -> Result {
    // 先用不可变借用检查
    {
        let tasks = self.tasks.read().await;
        let running = self.running.read().await;
        if !Self::is_ready_internal(task_id, &tasks, &running) {
            return Err(...);
        }
    }
    // 再用可变借用更新
    let mut tasks = self.tasks.write().await;
    let mut running = self.running.write().await;
    // ...
}
```

**解决方案 2**：分阶段获取锁（先检查，后更新）

```rust
// ✅ 正确：分阶段获取锁
pub async fn start_task(&self, task_id: &str) -> Result {
    // 阶段 1：只读检查
    let is_ready = {
        let tasks = self.tasks.read().await;
        let running = self.running.read().await;
        Self::is_ready_internal(task_id, &tasks, &running)
    };

    if !is_ready {
        return Err(...);
    }

    // 阶段 2：可变更新
    let mut tasks = self.tasks.write().await;
    let mut running = self.running.write().await;
    // ...
}
```

**锁获取顺序规则**：
1. 始终按固定顺序获取多个锁（如：tasks → running → history）
2. 在模块文档中记录锁顺序
3. 避免在持有锁时调用可能获取同一锁的方法
4. 优先使用内部辅助方法传递锁引用

### 安全

- **无硬编码密钥**：所有密钥来自 `mimofan-secrets` 或环境变量。
- **文件权限**：`secrets` crate 检查密钥文件的 0600 权限。
- **命令执行**：所有 shell 命令通过 `execpolicy` 沙箱。永远不要在用户可见路径中用原始 `Command::new()` 绕过。
- **输入清理**：工具名称通过 `to_api_tool_name()` 清理。发送给 LLM 的用户输入必须正确转义。

---

## 管理默认值

- 将社区 PR 和问题视为维护者证据。在合并、收割、关闭或推迟工作前检查代码、测试、链接的问题、评论和 CI。
- 没有 Hunter 的明确批准，不要打标签、发布、创建 GitHub Release 或推送发布工件。
- 保持 Mimofan 品牌同时保留对 DeepSeek 模型/服务商的一等支持和遗留迁移关怀。
- 为收割的工作保留贡献者信誉，包括作者身份、`Co-authored-by`、`Harvested from PR #N by @handle` 以及适用的变更日志/发布说明。使用 `.github/AUTHOR_MAP` 中的规范 GitHub noreply 身份；永远不要添加机器人/工具 `Co-authored-by` 尾部（Claude、codex、cursor）——`check-coauthor-trailers.py` CI 门会拒绝它们。

## 暂存集成分支

- 对于发布队列，从实际登陆分支创建一次性本地分支，例如 `scratch/vX.Y.Z-pr-train-YYYYMMDD`。
- 使用暂存分支批量合并或挑选候选 PR 头，以发现哪些冲突、测试和重叠是真实的。
- 不要发布暂存分支本身。它可能包含嘈杂的合并提交、部分冲突解决和不相关的 PR 交互。
- 暂存实验后，仅将安全结果作为窄提交或直接合并移回发布分支。保持每个最终提交可解释和可测试。
- 对 `main` 干净的 PR 不一定对发布分支干净。在称为合并就绪之前，针对将实际接收工作的分支测试可合并性。
- 对于已批准的 PR，将批准视为强优先级信号。在登陆前仍检查差异、评论、检查结果和发布分支冲突。

## Worktree 开发约定

- 大型重构或多模式收敛类任务，应在独立 git worktree 中进行（例如 `git worktree add ../agent-mimofan-worktree -b refactor/xxx`），避免污染主工作区、便于并行验证。
- worktree 内的开发完成后，先在 worktree 中确保 `cargo build`（零 warning）与 `cargo test`（全 workspace 零失败）通过，再合并回主干。
- 合并方式：在**主仓库**中 `git merge <worktree 分支>` 到 `main`（worktree 本身不持有 `main`，合并必须在主工作区执行）。确认无冲突且构建/测试仍绿后，再 `git push origin main`。
- **合并到主干后，必须删除该 worktree 分支**（本地 + 远程均过期即清理）：本地 `git branch -d <branch>`，远程 `git push origin --delete <branch>`；对应 worktree 目录用 `git worktree remove <path> --force` 清理。不要长期保留已合并的特性分支，避免分支堆积与混淆。

## 当前发布工作

- 从最新交接和 `git branch --show-current` 确认当前发布通道的活动分支；最近的工作通过小 PR 在 `main` 上登陆，而非长期存在的 `codex/...` 集成分支。此仓库存在于多设备，因此不要硬编码检出路径；在你拥有的本地检出中工作，并在编辑前确认分支。
- 从 `Cargo.toml` 读取工作区版本；它随发布通道推进。没有 Hunter 的明确批准，不要打标签、发布、创建 GitHub Release、推送发布工件或合并到 `main`。
- 基于活动交接中命名的当前 GitHub 发布里程碑进行发布分类（`gh issue list --repo XiaomingX/mimofan --milestone "<current>" --state open`），除非 Hunter 给出更新的分支/里程碑。
- 按此顺序处理队列：发布阻塞项、最近批准的 PR、小范围的干净 PR、有明显修复的阻塞 PR、可安全收割的脏 PR，然后是更大的架构问题。
- 优先在暂存分支上批量发现 PR 冲突，然后将已审查、已署名、已测试的切片收割回发布分支。
- 在声称问题完成之前，验证分支是否已包含等效工作。如果已包含，准备 GitHub 注释/关闭路径而非重新实现。
- 见上方「构建 / 测试 / 格式化快速参考」了解构建/测试命令，以及「关键设计约束」了解已移除机制护栏（仅限智能体表面、无生命周期/一致性系统）。
