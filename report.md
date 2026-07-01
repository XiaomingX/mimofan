# 项目分析与配置报告

根据你的要求，我对当前项目（特别是 `tui` 模块）进行了分析，以下是针对你的四个问题的详细答复：

## 1. TUI 默认支持哪些 Skill（通过 `/` 唤起的命令）？

在 TUI 中，通过 `/` 唤起的功能主要分为内置的“命令（Command）”以及可以由用户扩展的“技能（Skill）”。

**默认集成的系统核心命令群（Command Groups）包括：**
*   **配置类 (`/config`)**：包含 `/settings`（查看配置）、`/mode`（切换运行模式）、`/theme`（切换主题）、`/logout` 等。
*   **项目类 (`/project`)**：包含 `/init`（初始化项目上下文）、`/goal`（设定长线目标）、`/share`、`/lsp` 等。
*   **会话类 (`/session`)**：包含 `/new`（新建会话）、`/sessions`（列出历史会话）、`/load`、`/save`、`/compact`（压缩会话上下文）、`/export` 等。
*   **调试/状态类 (`/debug`)**：包含 `/tokens`（查看 Token 消耗）、`/cost`（费用计算）、`/balance`、`/cache` 等。
*   **技能管理类 (`/skills`)**：包含 `/skills`（列出所有可用技能）、`/skills sync`（同步/下载远程技能库）。

**关于真正的 Agent 技能（Agent Skills）：**
除了内置命令，系统还支持在 `~/.mimofan/skills/` 或项目目录的 `.agents/skills/` 下以 `SKILL.md` 形式定义的高级组合能力。TUI 默认内置了基础代码执行与文件读取能力，通过 `/skills` 命令可以看到当前环境已加载的所有扩展能力。

---

## 2. TUI 默认支持的 `settings.json` 配置项及用途

根据源码中的 `JsonSettings` 结构定义（`crates/tui/src/settings.rs`），`~/.mimofan/settings.json`（仅接受下列严格限定的字段，添加未知字段会导致应用无法启动报错）：

*   **`env`** 
    *   **类型**: 键值对字典 (HashMap<String, String>)
    *   **用途**: 为 TUI 及大模型进程自动注入特定的环境变量（例如指定一些内部 API 路径等）。
*   **`mcp_servers`**
    *   **类型**: 字典 (HashMap<String, McpServerConfig>)
    *   **用途**: 用于配置外部的 MCP (Model Context Protocol) 插件服务器。每个服务可以配置 `command`（启动命令）、`args`（启动参数）、`env`（环境要求）以及 `enabled`（是否启用）。
*   **`enabled_plugins`**
    *   **类型**: 布尔值字典 (HashMap<String, bool>)
    *   **用途**: 控制特定原生插件的开启与关闭状态。
*   **`language`**
    *   **类型**: 字符串 (Option<String>)
    *   **用途**: 用户的首选自然语言偏好（例如 `"Chinese"` 或 `"zh-Hans"`），模型会依据此项自动调整输出内容的语言。
*   **`instructions`**
    *   **类型**: 字符串数组 (Vec<String>)
    *   **用途**: 定义额外需要自动加载进系统提示词（System Prompt）中的指令文件路径或内容段落。

*(注意：类似 `primary_model` 或 `api_key` 这种 API 设置通常存储在 `config.toml` 中，而非 `settings.json`)*

---

## 4. 系统默认集成哪些系统提示词（Prompts）？

系统采用了模块化的提示词组装策略，所有提示词源码位于 `crates/tui/src/prompts/` 目录下，并在编译期被静态集成到二进制文件中。默认集成的核心提示词文件包括：

1.  **核心宪法（Base Constitution）**: 
    *   `constitution.md`：奠定 Agent 的基础身份、执行策略、工具分类规范以及绝对不可触碰的安全红线。
2.  **人格设定（Personalities）**:
    *   `calm.md`（默认）：冷静、空间感强、克制的回答风格。
    *   `playful.md`：热情、活泼的替代风格（目前是预留模式，可通过配置开放）。
3.  **运行模式补丁（Modes Deltas）**:
    *   `agent.md`：全自动 Agent 代理模式的行为规范。
    *   `plan.md`：强制规划模式，要求执行前进行深入分析和制定执行蓝图。
    *   `yolo.md`：放任模式，减少对风险操作的拦截，最大化执行效率。
4.  **授权策略（Approval Policies）**:
    *   `auto.md`：自动授权大部分安全指令。
    *   `suggest.md`：仅提供建议，需用户确认。
    *   `never.md`：静默/禁止危险执行。
5.  **其他扩展提示词**:
    *   `compact.md`：会话压缩（上下文接力）时的记忆格式模版。
    *   `continuation.md`：长线目标未能单次完成时的延续汇报模版。
    *   `memory_guidance.md`：有关如何将用户偏好保存为持久记忆的指南。
    *   `locale_preamble_zh_hans.md` / `locale_closer_zh_hans.md`：专为中文用户准备的**强约束中文输出**前后缀，确保大模型无论在何种混杂代码的场景下都不会突然跳回英文。
