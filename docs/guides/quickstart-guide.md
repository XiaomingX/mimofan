# mimofan 快速上手 · 试用手册

> **mimofan 是一个跑在终端里的 AI 编程搭档。** 你用自然语言下指令，它调用大模型思考，再用工具（读文件、改代码、跑命令）把活干完。本文用 5 分钟带你走完「安装 → 配置 → 第一次对话 → 第一次真正干活」的主流程。

---

## 快速开始

mimofan 是终端应用，无需图形界面。你只需要一个终端和 Rust 工具链（或直接下载预编译二进制）。

```bash
# 用 cargo 编译安装
cargo install --path crates/tui --locked

# 或者下载预编译二进制
# https://github.com/XiaomingX/mimofan/releases
```

安装完成后，直接输入 `mimofan` 即可启动全屏交互界面。

---

## 零配置启动（默认小米 MiMo）

无需手动创建配置文件，设置一个环境变量就能跑起来：

```bash
export XIAOMI_MIMO_API_KEY="你的_XIAOMI_MIMO_API_KEY"
mimofan
```

> **提示**：如果没设置任何环境变量，首次运行 `mimofan` 会自动启动**交互式配置向导**，一步步帮你写好配置。什么都不用担心。

---

## 页面长什么样

mimofan 是经典的全屏 TUI（终端用户界面），布局如下：

- **主聊天区**（中央）：你和 AI 的对话滚动区，模型流式输出。
- **输入框**（底部）：在这里输入自然语言指令，`Enter` 发送，`Shift+Enter` 换行。
- **侧边栏**（左侧，`Tab` 切换焦点）：快捷操作、会话列表、模型切换。
- **状态栏**（底部）：当前模式（normal/plan/yolo）、当前模型、Git 分支、token 用量。

> 这是一套「前台聊天、后厨干活」的分层：你在前台只看到结论与需要你确认的地方，工具执行的细节在后台悄悄进行。

---

## 能做什么

### 1. 自然语言编程

直接用一句话让 AI 干活，它会把「分析 → 改码 → 跑验证 → 修错」整套流程跑完：

```text
> 这个函数报错了 "index out of bounds"，帮我修复并添加测试
```

### 2. 单次指令模式（不进 TUI）

适合脚本化、管道、CI 场景，一次性下指令拿到结果：

```bash
mimofan exec "帮我写一个正则表达式匹配邮箱"
mimofan exec --auto "列出 crates/ 目录"        # --auto 允许工具自动执行
mimofan exec --auto --output-format stream-json "修复失败测试"
```

### 3. 自动诊断

环境有问题？一条命令自查：

```bash
mimofan doctor
```

### 4. 计划模式与安全授权

每次 AI 要执行危险操作（改文件、跑命令）时，都会先弹授权窗口问你「可以吗？」：

- `y`：允许这一次
- `n`：拒绝
- `a`：本次会话全部允许

> 这是 mimofan 的**安全边界**：它只在需要你决策时才开口，其余时间不弹窗、不打扰。

---

## 五分钟体验路线

1. **启动**：`export XIAOMI_MIMO_API_KEY="..."` 然后 `mimofan`。
2. **打个招呼**：输入 `hi`，确认模型能正常回复。
3. **干一件小事**：让 AI「列出当前目录结构并解释每个文件的作用」。
4. **体验授权**：让 AI「运行 cargo test」，观察它在执行前停下来等你确认。
5. **切换模式**：输入 `/plan 重构这段代码`，体验「先列计划、审核后再执行」。
6. **换个模型**：`/model deepseek-chat`，看它在不同模型间切换。

---

## 常见问题（先读这里）

**Q: 启动时提示 Config not found 或连接超时？**
检查 `~/.mimofan/config.toml` 的路径和 `api_key` 是否正确，然后运行 `mimofan doctor` 自动诊断。

**Q: 每次执行命令都要按 y 确认，很烦，能关掉吗？**
可以在配置中设置 `approval_policy = "yolo"` 开启全自动模式。**但只在信任的仓库里开**，否则 AI 可能在你不注意时做出破坏性改动。

**Q: 我设置了 MiMo 环境变量，但想用 DeepSeek / 通义千问怎么办？**
这些非内置服务商走「自定义 provider」，在 `~/.mimofan/config.toml` 里配置 `base_url` 和 `api_key`（详见下方「接入更多模型」）。

**Q: 如何切换模型？**
TUI 内用 `/model deepseek-chat` 或 `/model gpt-4`；也可以启动时带 `--model <name>`。

**Q: 如何防止 AI 偏离我的计划？**
使用 `/freeze 只修复这个 bug` 冻结计划，Agent 将严格在冻结范围内工作，任何越界操作都需你确认。

**Q: 能让 AI 读取本地文档吗？**
可以，直接说「读取根目录下的 ARCHITECTURE.md 并回答我的问题」。

---

## 安全边界

- **危险操作需确认**：改文件、执行 shell、写网络请求等操作默认会先征求你的同意（`approval_policy = "on-request"`）。
- **沙箱可配置**：默认 `sandbox_mode = "workspace-write"`（只允许在工作区内写），可收紧为 `read-only` 或放开为 `danger-full-access`。
- **权限规则独立文件**：工具的 ask 规则放在 `~/.mimofan/permissions.toml`，与主配置分离。
- **YOLO 模式慎用**：`approval_policy = "yolo"` / `/yolo` 是全自动执行，请只在完全信任的仓库中使用。
- **不弹窗不打扰**：mimofan 的设计原则是「只在需要你决策时才开口」。

---

## 下一步

- 想深度掌握全部能力？请看 [使用手册](personal-workspace-user-guide.md)。
- 想了解配置项全貌？请看 [docs/CONFIGURATION.md](../CONFIGURATION.md)。
- 想接入更多模型？继续往下看。

---

## 附录：接入更多模型（provider）

mimofan 只有三种协议模式：`openai-compatible` / `anthropic-compatible` / `gemini-compatible`。配置在 `~/.mimofan/config.toml`：

```toml
# DeepSeek
provider = "openai-compatible"
api_key = "你的_DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com/v1"
default_text_model = "deepseek-chat"

# 通义千问
provider = "openai-compatible"
api_key = "你的_DASHSCOPE_API_KEY"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
default_text_model = "qwen-max"
```

> Anthropic 兼容模式要求 `base_url` 以 `/anthropic` 结尾，mimofan 会自动检测并切换为 Anthropic Messages API 协议。
