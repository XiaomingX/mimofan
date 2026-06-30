# mimofan 架构与优化路线图 (DDD 视角)

从第一性原理和领域驱动设计 (DDD) 出发，mimofan 作为一个终端 AI 协同编程助手（AI Harness），其核心本质是**“跨宿主操作领域的双向反馈环”**。它将大语言模型的规划能力与本地操作系统的执行环境有机结合，通过循环反馈来自动化软件开发生命周期。

---

## 1. 领域驱动设计 (DDD) 限界上下文

系统的核心领域划分为以下几个限界上下文 (Bounded Contexts)：

```mermaid
graph TD
    subgraph UI_Context [用户交互上下文 (TUI/CLI)]
        App[crates/tui::app]
        UI[crates/tui::ui]
    end

    subgraph Core_Context [核心引擎上下文]
        Engine[crates/tui::core::engine]
        TurnLoop[crates/tui::core::engine::turn_loop]
    end

    subgraph Tool_Context [本地操作与集成上下文]
        Shell[crates/tui::tools::shell]
        SubAgent[crates/tui::tools::subagent]
        Mcp[crates/tui::mcp]
    end

    subgraph Config_Context [配置防腐上下文]
        Config[crates/tui::config]
        Settings[crates/tui::settings]
    end

    UI_Context -->|状态映射与用户指令| Core_Context
    Core_Context -->|调用链分发| Tool_Context
    Core_Context -.->|读取系统参数| Config_Context
    Tool_Context -.->|感知/修改| OS((宿主操作系统))
```

*   **用户交互上下文 (User Interface Context)**: 负责渲染 TUI 界面（基于 `ratatui`）、处理用户快捷键及接收指令。
*   **核心引擎上下文 (Core Engine Context)**: 系统的核心子域。负责生命周期流控，维护 `TurnLoop`（大轮次事件循环）、历史快照快滚及 Token 预算控制。
*   **本地操作与集成上下文 (Tool & Integration Context)**: 支撑子域。封装具体的 Shell 执行、文件检索、子智能体（Sub-agent）递归调度以及三方标准 MCP (Model Context Protocol) 服务的长连接与进程管道管理。
*   **配置防腐上下文 (Configuration Anticorruption Context)**: 共享内核。屏蔽底层文件路径差异、提供高频读取时的内存级防腐层缓存，过滤零散的 Base URL 和密钥获取逻辑。

---

## 2. 架构设计的精妙之处

1.  **统一模型路由适配层 (`crates/agent`)**
    系统定义了通用的模型适配接口，无论物理上提供商是 OpenAI、Anthropic 还是本地的 vLLM、SGLang、Ollama，在领域核心眼里均归一化为统一的路由实体。这极大地降低了接入新模型的维护成本。
2.  **细粒度的会话快照与回滚 (`/restore`)**
    核心引擎维护了内存与持久化（SQLite）双轨制的状态树。每一轮执行（Turn）前均生成系统级 snapshot，用户可一键将整个工作区结构、会话历史回滚到任意一个健康轮次，实现了 AI 自主改代码时的“无损撤销”。
3.  **支持递归自改善的子智能体管理 (`crates/tui/src/tools/subagent`)**
    在处理宏观宏大目标（如 `/goal`）时，核心引擎会通过子智能体组件并发派生独立上下文的子智能体，并在独立的工作树（git worktree 或分支）上平行试错与编译，完成后合并回母版。这种自底向上的分治策略大幅提高了复杂任务的成功率。

---

## 3. 现存的架构边界缺陷

1.  **核心引擎与 TUI 强耦合 (Tui-Engine Entanglement)**
    *   **问题**：核心流控的 `TurnLoop` 和大量的交互状态直接写在 `crates/tui` 下，导致 `App` 结构体异常庞大。外部的 `crates/cli` 为实现无界面运行，不得不反向依赖或大量复制 TUI 下的引擎配置，增加了边界模糊风险。
    *   **改进方向**：剥离 `TurnLoop` 到 `crates/core` 内，让 `App` 沦为纯粹的展示者，实现 Tui 和 Engine 彻底解耦。
2.  **MCP 进程交互同步背压隐患 (MCP Stdio Backpressure)**
    *   **问题**：MCP 的 stdio 通道在处理超大数据帧（如大型 LSP 返回的格式化信息）时采用同步阻塞读取，在终端高负载时有概率发生锁死或阻塞工作池线程的情况。
    *   **改进方向**：将 MCP stdio 通道彻底重构为基于 `Tokio mpsc` 的全异步背压管道。
3.  **配置读写竞争风险 (Config File I/O Race)**
    *   **问题**：多个子智能体并发执行时，若高频触发配置刷新或 `Settings::load`，会产生对 `settings.json` 的重复磁盘 IO 读取和多进程读写竞争。

---

## 4. 架构优化改进计划 (路线图)

我们本着资深架构师第一性原理，拒绝迎合或罗列无用待办，仅保留真正能降低系统复杂度、提高扩展性的优化项：

*   [x] **统一配置解析防腐层 (Memory Cache Consolidation)**
    *   在 `crates/tui/src/settings.rs` 中引入全局静态 `SETTINGS_CACHE`（基于 `LazyLock` 与 `RwLock` 缓存），接管磁盘配置读取，切断了高频请求下的重复磁盘 IO 风险，保证多线程与子智能体并发时的读取吞吐量。
*   [ ] **核心引擎与 UI 拆分 (Tui/Engine Split)**
    *   剥离 `crates/tui/src/tui/app.rs` 与 `crates/tui/src/core/engine` 下的事件大轮次流转，将 `TurnLoop` 下沉到 `crates/core` 内。
*   [ ] **MCP 长连接背压异步管道化 (Async Backpressure MCP Pipeline)**
    *   将 `crates/mcp` 下的标准 I/O 读取部分移植到 Tokio 非阻塞信道中，设置合理的 Buffer 限制，防止外部大进程输出堵塞工作线程。
