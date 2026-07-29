# Mimofan 候选需求（Verse）

> 来源：11 个竞品仓库 issues 分析，筛选 TUI + macOS + UX/Token 优化相关
> 生成日期：2026-07-29

---

## 一、Token 成本优化（节省 Token）

### 高优先级

| # | 来源 | Issue | 标题 | 描述 | 预估收益 |
|---|------|-------|------|------|----------|
| T1 | opencode | #39294 | [x] system prompt 路径浪费 3.3K tokens/轮 | `<available_skills>` 包含完整绝对路径，每轮浪费约 3.3K tokens | 每轮节省 3.3K tokens |
| T2 | kilocode | #12628 | [-] 长会话 compaction 对免费模型失败 | chunk-based compaction 不适配 token 限制小的免费模型，导致重试浪费 | 避免 compaction 重试浪费 |
| T3 | kilocode | #12615 | [x] 自动 compaction 导致无限循环 | compaction 触发条件不当，陷入无限压缩循环 | 避免无限 token 消耗 |
| T4 | kimi-cli | #2472 | [x] compaction 重新加载 system prompt 浪费 20K tokens | compaction 后重新发送完整 system prompt + AGENTS.md | 每次 compaction 节省 ~20K tokens |
| T5 | kimi-cli | #2525 | [-] Goal 模式空操作持续消耗 token | Goal 模式在等待外部条件时持续发起 "continue" 调用 | 避免空操作 token 浪费 |
| T6 | claude-mem | #3443 | [x] Observer 历史无限增长：单次调用 1M tokens | 观察者对话历史无界增长，导致单次 API 调用达 1M tokens | 避免指数级 token 增长 |
| T7 | claude-mem | #3274 | [-] 子智能体观察污染主会话上下文 | 子智能体观察注入主会话，增加无关 tokens | 减少无效 context |
| T8 | gemini-cli | #28362 | [x] Token 消耗无限循环 | 代理进入无限重试循环，消耗 token 但不产出 | 避免零产出 token 消耗 |
| T9 | gemini-cli | #28339 | [x] 429 错误导致无限回退循环 | 429 限流导致无限重试，无超时无错误显示 | 避免限流时 token 浪费 |
| T10 | kimi-code | #2177 | [-] 超大工具结果阻止去重提醒 | >50K 字符的工具结果阻止去重提醒到达模型 | 避免上下文膨胀 |
| T11 | MiMo-Code | #1855 | [x] /btw 侧问题发送百万 token | 声称"无 context 成本"但发送完整会话历史，达 1M-1.5M tokens | 避免侧问题 token 爆炸 |
| T12 | MiMo-Code | #1854 | [x] OOM 10-20GB：无界 summary.diffs | 长会话累积 100MB+/消息 diffs，导致 10-20GB 内存和崩溃 | 避免内存导致的崩溃重试 |

### 中优先级

| # | 来源 | Issue | 标题 | 描述 |
|---|------|-------|------|------|
| T13 | opencode | #39444 | [x] GLM-5.2 prompt cache 失效 | 自定义/反向代理路径导致 prompt cache 从 512 跌到 0 |
| T14 | opencode | #39291 | [x] compaction 发送变异 thinking block → 400 重试循环 | compaction 机制缺陷导致永久重试 |
| T15 | kimi-cli | #2465 | [x] reasoning_effort: null 导致严格 API 拒绝 | thinking "off" 时序列化为 null，严格 API 拒绝 |
| T16 | kimi-cli | #2523 | [-] compaction 后重新打开已完成任务 | compaction 后任务状态丢失，导致重复执行 |
| T17 | claude-mem | #3409 | [-] 配置过滤选项未连接到查询 | 用户配置的过滤器从未生效 |
| T18 | claude-mem | #3163 | [x] 标题级样板观察绕过 content_hash | 近重复观察涌入 recall 窗口 |
| T19 | gemini-cli | #28312 | [ ] 代理自提示超出范围 | 代理在设计阶段超出范围写代码，消耗额外 token |
| T20 | MiMo-Code | #1882 | [-] 更多模型不支持推理强度参数 | 无法调整 DeepSeek 等模型的 reasoning effort |
| T21 | kimi-code | #2257 | [x] footer 显示累计 token 用量 | 用户无法感知消耗 |
| T22 | kimi-cli | #2394 | [-] 暴露每轮 token 用量 | Token 用量从 StatusUpdate 丢弃 |

---

## 二、用户体验优化（更快响应 / 常用能力 / 易用性）

### 高优先级

| # | 来源 | Issue | 标题 | 描述 | 分类 |
|---|------|-------|------|------|------|
| U1 | opencode | #39380 | [ ] session 切换与 transcript 长度无关 | 长对话时切换 session 有明显延迟 | 性能 |
| U2 | opencode | #39342 | [ ] Markdown 流式输出冗余 Tree-sitter 高亮 | 每个流式块重新高亮整个文档，CPU 高占用 | 性能 |
| U3 | opencode | #39208 | [x] glob/grep 工具无超时 | 一个调用挂起 session 21 分钟 | 稳定性 |
| U4 | kimi-code | #2296 | [ ] 权限审批后滚动位置重置 | 审批工具权限后滚动跳回顶部 | TUI 交互 |
| U5 | kimi-code | #2154 | [ ] 子智能体完成后仍显示为运行中 | 状态展示 bug，计时器不停 | 子智能体 |
| U6 | kimi-code | #2195 | [ ] 禁用备选屏幕保留终端 scrollback | SSH 场景核心痛点 | TUI 配置 |
| U7 | kimi-cli | #2501 | [ ] TUI 热切换推理强度 | 无需重启即可切换 thinking effort | TUI 交互 |
| U8 | kimi-cli | #2528 | [ ] Shell 输出过长淹没 TUI | shell 工具输出洪水般涌入 | 输出管理 |
| U9 | kimi-cli | #2422 | [ ] 对话完成后滚动跳到底部 | 强制滚动到底部，无法查看历史 | 滚动行为 |
| U10 | kimi-cli | #2417 | [ ] 文本换行在单词中间截断 | 长单词/路径在行边界错误断行 | 文本渲染 |
| U11 | MiMo-Code | #1707 | [x] 子智能体执行期间 TUI 冻结 | TUI 在子智能体执行时完全无响应 | TUI 稳定性 |
| U12 | MiMo-Code | #1868 | [ ] 任务耗时显示 | 无任务耗时信息，无法评估性能 | 信息展示 |
| U13 | gemini-cli | #28395 | [ ] 主线程阻塞 I/O 导致 UI 卡顿 | 同步文件操作阻塞渲染循环 | 性能 |
| U14 | gemini-cli | #28355 | [ ] MCP 工具发现静默阻塞 10 分钟 | 服务器返回不匹配 id 时无超时无警告 | 稳定性 |
| U15 | gemini-cli | #28340 | [ ] 重试时不显示进度 | 连接失败重试时 spinner 卡在 "Thinking..." | 状态反馈 |

### 中优先级

| # | 来源 | Issue | 标题 | 描述 | 分类 |
|---|------|-------|------|------|------|
| U16 | opencode | #39346 | [ ] 模型选择器缩写搜索 | deepff → Deepseek V4 Flash Free | 便捷操作 |
| U17 | opencode | #39338 | [ ] 输入光标样式可配置 | line/beam vs block | 个性化 |
| U18 | opencode | #39438 | [ ] Tab/Shift-Tab 循环 timeline | timeline 弹窗支持循环导航 | 交互 |
| U19 | opencode | #39462 | [ ] CJK 字符后 @ 文件补全失效 | 中文用户高频场景功能失效 | CJK 支持 |
| U20 | opencode | #39475 | [ ] 外部编辑器打开时保持 TUI | 编辑工作流减少上下文切换 | TUI 集成 |
| U21 | opencode | #39483 | [ ] 可配置启动欢迎信息 | TUI 启动屏个性化 | 个性化 |
| U22 | kilocode | #12503 | [ ] TUI 内设置对话框 | 无需编辑外部配置文件 | 配置 |
| U23 | kilocode | #12465 | [ ] TUI 查看版本/环境信息 | 基础信息可访问性 | 信息展示 |
| U24 | kilocode | #12441 | [ ] 隐私模式模糊 PII | 终端共享/录屏场景 | 安全 |
| U25 | kilocode | #12446 | [ ] Read 工具 repr 模式 | 非 ASCII 字符转义显示 | CJK 支持 |
| U26 | kilocode | #12443 | [ ] 斜杠命令切换 auto-approve | 快速模式切换 | 便捷操作 |
| U27 | kimi-cli | #2448 | [ ] Yolo 模式仍显示审批提示 | 自动审批工作流被中断 | 模式一致性 |
| U28 | kimi-cli | #2450 | [ ] 窄终端宽度导致 TUI 崩溃 | 未处理窄终端情况 | 稳定性 |
| U29 | kimi-cli | #2474 | [ ] TUI 全量重绘导致闪烁 | 不必要的全屏重绘 | 渲染性能 |
| U30 | kimi-cli | #2441 | [ ] @filename 文件引用语法 | 用户无法用 @ 引用文件 | 便捷操作 |
| U31 | kimi-code | #2276 | [ ] 自定义状态栏 | spinner 文案 + session id | 信息展示 |
| U32 | kimi-code | #2181 | [ ] 终端转义序列泄漏导致误取消 | 输入处理健壮性 | 输入处理 |
| U33 | MiMo-Code | #1936 | [ ] 无效 session ID 导致黑屏 | 无错误反馈，需重启 | 稳定性 |
| U34 | MiMo-Code | #1849 | [ ] AI 回答截断不可见 | 长响应在终端中被切断 | 输出管理 |
| U35 | MiMo-Code | #1842 | [ ] 粘贴文本被误识别为图片 | 语音输入文本被当作图片上传 | 输入处理 |
| U36 | MiMo-Code | #1834 | [ ] 基于历史的自适应自动审批 | 重复审批常用命令 | 便捷操作 |
| U37 | MiMo-Code | #1804 | [ ] macOS 中文输入法文字颜色不可见 | 深色主题下输入法候选文字黑色 | macOS 特定 |
| U38 | MiMo-Code | #1839 | [ ] /btw 长回答无流式输出 | 侧问题响应不流式，溢出无法滚动 | TUI 交互 |
| U39 | gemini-cli | #28300 | [ ] /rewind 后代理循环中断 | 回退对话后代理停止执行 | 会话管理 |
| U40 | gemini-cli | #28482 | [ ] 静默执行失败 | 代理静默失败无错误消息 | 错误处理 |
| U41 | gemini-cli | #28579 | [ ] 并行工具调用 400 错误 | 并行调用在 Gemini 3+ 上失败 | 工具调用 |
| U42 | gemini-cli | #28416 | [ ] macOS 符号链接路径不一致 | GlobTool 返回原始路径但内部使用解析路径 | macOS 特定 |
| U43 | gemini-cli | #28337 | [ ] macOS OAuth 凭证持久化失败 | 每次运行需重新认证 | macOS 特定 |
| U44 | claude-mem | #3186 | [ ] 每个 hook 额外 250-300ms 延迟 | 不必要的 shell 探测 PATH | 响应速度 |

---

## 三、稳定性与安全

| # | 来源 | Issue | 标题 | 描述 | 分类 |
|---|------|-------|------|------|------|
| S1 | opencode | #39292 | [x] Mac M4 多 session 系统冻结 | 并发使用导致系统冻结 | 稳定性 |
| S2 | opencode | #39463 | [x] SQLite WAL 文件增长到 1GB+ | macOS 临时目录无限制增长 | 资源管理 |
| S3 | opencode | #39165 | [x] /model 切换后 SQLite 崩溃 | 常用操作导致 session 损坏 | 稳定性 |
| S4 | opencode | #39456 | [x] 子智能体工具调用静默中止 (macOS) | 自定义 provider 上子智能体失败 | 稳定性 |
| S5 | MiMo-Code | #1915 | [x] Checkpoint rebuild 导致无限循环 | 完成任务后重新注入恢复提示 | 稳定性 |
| S6 | MiMo-Code | #1967 | [x] 请求拒绝后永久挂起 | 拒绝权限请求后 TUI 无响应 | 稳定性 |
| S7 | MiMo-Code | #1746 | [x] Plan 模式执行 bash 命令 | 计划模式应仅计划，不应执行 | 安全 |
| S8 | gemini-cli | #28555 | [ ] SSRF 漏洞 (CVSS 8.6) | URL 验证绕过 DNS 解析 | 安全 |
| S9 | gemini-cli | #28418 | [ ] Shell 变量展开绕过检测 | $VAR 模式绕过 shell 命令检测 | 安全 |
| S10 | gemini-cli | #28392 | [-] 后台 shell 临时目录泄漏 | gemini-shell-* 目录从不清理 | 资源管理 |
| S11 | gemini-cli | #28271 | [x] 事件驱动无限循环 | MAX_TURNS 限制无法阻止零延迟事件洪泛 | 稳定性 |
| S12 | claude-mem | #3404 | [ ] macOS 45GB 内存泄漏 | 203 个观察者子进程累积 4 天 | 资源管理 |
| S13 | claude-mem | #3340 | [x] macOS sleep/wake 后 100% CPU | worker 守护进程在唤醒后 CPU 满载 | 资源管理 |

---

## 四、按优先级汇总（Top 20 推荐实施）

| 优先级 | 编号 | 来源 | 标题 | 类型 | 预估工作量 |
|--------|------|------|------|------|-----------|
| ~~🔴 P0~~ | T1 | opencode | [x] system prompt 路径浪费 3.3K tokens/轮 | Token | 小 |
| ~~🔴 P0~~ | T4 | kimi-cli | [x] compaction 重新加载 system prompt 浪费 20K tokens | Token | 中 |
| ~~🔴 P0~~ | U3 | opencode | [x] glob/grep 工具无超时 | 稳定性 | 小 |
| ~~🔴 P0~~ | U11 | MiMo-Code | [x] 子智能体执行期间 TUI 冻结 | 稳定性 | 中 |
| ~~🔴 P0~~ | S1 | opencode | [x] Mac M4 多 session 系统冻结 | 稳定性 | 中 |
| 🟠 P1 | T2 | kilocode | [-] 长会话 compaction 对免费模型失败 | Token | 中 |
| ~~🟠 P1~~ | T3 | kilocode | [x] 自动 compaction 无限循环 | Token | 中 |
| 🟠 P1 | T5 | kimi-cli | [-] Goal 模式空操作 token 消耗 | Token | 小 |
| 🟠 P1 | U1 | opencode | [ ] session 切换性能 | 性能 | 中 |
| 🟠 P1 | U4 | kimi-code | [ ] 权限审批后滚动重置 | TUI 交互 | 小 |
| 🟠 P1 | U5 | kimi-code | [ ] 子智能体完成后仍显示运行中 | 子智能体 | 小 |
| 🟠 P1 | U7 | kimi-cli | [ ] TUI 热切换推理强度 | TUI 交互 | 中 |
| 🟠 P1 | U8 | kimi-cli | [ ] Shell 输出过长淹没 TUI | 输出管理 | 小 |
| 🟠 P1 | U15 | gemini-cli | [ ] 重试时不显示进度 | 状态反馈 | 小 |
| 🟡 P2 | T10 | kimi-code | [-] 超大工具结果阻止去重 | Token | 中 |
| ~~🟡 P2~~ | T21 | kimi-code | [x] footer 显示 token 用量 | 信息展示 | 小 |
| 🟡 P2 | U2 | opencode | [ ] Markdown 流式冗余高亮 | 性能 | 中 |
| 🟡 P2 | U9 | kimi-cli | [ ] 对话完成后滚动跳到底部 | 滚动行为 | 小 |
| 🟡 P2 | U19 | opencode | [ ] CJK 后 @ 补全失效 | CJK 支持 | 中 |
| ~~🟡 P2~~ | S7 | MiMo-Code | [x] Plan 模式执行 bash 命令 | 安全 | 小 |

---

## 五、新增仓库筛选结果（v2）

### jcode (1jehuang/jcode)

| # | 来源 | Issue | 标题 | 描述 | 分类 |
|---|------|-------|------|------|------|
| J1 | jcode | #651 | [x] macOS stdin 检测常量错误 | `TH_STATE_WAITING` 应为 3 非 2，导致 stdin 管道输入完全失效 | macOS 核心 |
| J2 | jcode | #644 | [x] 大上下文模型 compaction 永不触发 | 1M 窗口模型的 80% 阈值=800K，永不触发，每次重发完整历史 | Token |
| J3 | jcode | #540 | [x] 长会话 TUI 输入和滚动退化 | 输入延迟增加、鼠标滚轮导致大量重绘 | TUI 性能 |
| J4 | jcode | #632 | [x] TUI 粘贴文本 panic (critical) | `is_char_boundary` 断言失败导致崩溃 | 稳定性 |
| J5 | jcode | #617 | [-] MCP 连接超时阻塞首次调用 30 秒 | 每次冷启动 30 秒无响应等待 | 启动体验 |
| J6 | jcode | #583 | [x] 信息组件滚动时跳动闪烁 | 模型/上下文/用量信息在滚动时不稳定 | TUI 视觉 |
| J7 | jcode | #621 | [x] macOS 菜单栏图标不显示 | 系统级交互体验问题 | macOS 特定 |
| J8 | jcode | #559 | [x] API 429/5xx 重试不足 | 仅重试 3 次（~9 秒），之后直接失败 | 稳定性 |
| J9 | jcode | #570 | [ ] @file 文件提及 frecency 排序 | TUI 输入框 @ 文件自动补全 | 便捷操作 |
| J10 | jcode | #539 | [x] /rewind 恢复编辑文件 | 回退对话时同时恢复文件编辑 | 工作流安全 |
| J11 | jcode | #629 | [x] TUI 跟随终端 ANSI 配色 | 支持自定义主题，iTerm2 用户保持统一风格 | 个性化 |

### hermes-agent (NousResearch/hermes-agent)

| # | 来源 | Issue | 标题 | 描述 | 分类 |
|---|------|-------|------|------|------|
| H1 | hermes-agent | #73872 | [-] SSE 心跳帧导致 stale 检测器失效 | 空心跳帧重置计时器，连接永远挂起 | 稳定性 |
| H2 | hermes-agent | #74003 | [x] /model 命令响应 7+ 分钟 | 高频操作延迟严重退化 | 性能 |
| H3 | hermes-agent | #73772 | [x] 模型静默回退无通知 | 429/502/超时时用户不知道回退了哪个模型 | 信息透明 |
| H4 | hermes-agent | #73678 | [x] 子代理完成消息干扰可读性 | 完整结果逐字转储到会话历史 | 输出管理 |
| H5 | hermes-agent | #73702 | [x] 禁用未使用 skill 注入系统提示词 | 155 个 skill 每轮注入 ~15K 字符 | Token |
| H6 | hermes-agent | #73825 | [x] 模型列表含不存在的 3.x 模型 | 选择后静默 404 回退无提示 | 模型管理 |
| H7 | hermes-agent | #73823 | [x] Token 统计永久失效 (schema>=22) | 无法追踪 token 消耗 | Token |
| H8 | hermes-agent | #73848 | [x] 就地压缩导致 session 无法结束 | state.db 无限增长 | 资源管理 |
| H9 | hermes-agent | #73891 | [x] 上下文压缩器耗尽 provider 池 | 524 超时后暂停 60 秒不切换 provider | 稳定性 |
| H10 | hermes-agent | #73777 | [ ] 空 content 重试浪费 35-40 秒 | Anthropic 返回空 content 时无效重试 | Token |
| H11 | hermes-agent | #73748 | [x] 限流时 turn 丢失不重试 | 用户消息被静默丢弃 | 稳定性 |
| H12 | hermes-agent | #73680 | [x] 多实例模型切换互相污染 | 一个实例切换模型影响另一个实例 | 会话隔离 |

### openclaw (openclaw/openclaw)

| # | 来源 | Issue | 标题 | 描述 | 分类 |
|---|------|-------|------|------|------|
| O1 | openclaw | #115454 | [ ] totalTokens ~2x 真实上下文 | 每 1-2 条消息触发一次 compaction | Token |
| O2 | openclaw | #115273 | [x] Prompt cache 滑动窗口 ~0% 命中率 | 每 turn 以全额 token 重发前缀 | Token |
| O3 | openclaw | #115272 | [x] 模型切换 reasoning drop 翻转 | 每次 failover 摧毁 prompt cache | Token |
| O4 | openclaw | #115546 | [x] compaction 超时远低于 deadline | 大 session compaction 100% 失败 | Token |
| O5 | openclaw | #115437 | [ ] 支持 fast mode (2.5x 吞吐) | Anthropic fast mode 可加速响应 | 性能 |
| O6 | openclaw | #115372 | [x] TUI 选择器原语 | 缺少 AskUserQuestion 等价物 | TUI 交互 |
| O7 | openclaw | #115826 | [ ] 插件渲染瞬态 TUI 运行状态 | 长操作中无实时进度反馈 | TUI 交互 |
| O8 | openclaw | #115387 | [x] 可滚动文件查看器 | 大文件无法查看 | TUI 功能 |

### claude-code-best (claude-code-best/claude-code)

| # | 来源 | Issue | 标题 | 描述 | 分类 |
|---|------|-------|------|------|------|
| C1 | claude-code-best | #1307 | [x] 按 Provider/模型配置上下文窗口 | 避免过早/过晚压缩 | Token |
| C2 | claude-code-best | #1310 | [ ] push/pop 上下文栈 + /digest 蒸馏 | 比 /compact 更高效的上下文管理 | Token |
| C3 | claude-code-best | #1309 | [ ] 长会话任务锚点重注入 | 检测上下文退化并警告 | 会话管理 |
| C4 | claude-code-best | #1302 | [x] filter-map 单次遍历优化 | 渲染路径性能优化 | 性能 |
| C5 | claude-code-best | #1301 | [-] OpenAI 兼容模型 reasoning_effort | 跨 Provider 统一思考模式控制 | Token |
| C6 | claude-code-best | #1308 | [ ] /workflows 延迟执行 + 误停 agent | 工作流管理当前行为导致任务中断 | 子智能体 |
| C7 | claude-code-best | #70 | [ ] 任务不同阶段切换模型 | 规划用 opus，执行用国产模型 | 成本优化 |
| C8 | claude-code-best | #410 | [x] 大段用户输入折叠显示 | 提升 TUI 可读性 | TUI 交互 |
| C9 | claude-code-best | #1248 | [-] LSP 文件跟踪内存泄漏修复 | 长期使用稳定性 | 稳定性 |

---

## 六、按优先级汇总（Top 30 推荐实施）

| 优先级 | 编号 | 来源 | 标题 | 类型 | 预估工作量 |
|--------|------|------|------|------|-----------|
| ~~🔴 P0~~ | T1 | opencode | [x] system prompt 路径浪费 3.3K tokens/轮 | Token | 小 |
| ~~🔴 P0~~ | T4 | kimi-cli | [x] compaction 重新加载 system prompt 浪费 20K tokens | Token | 中 |
| ~~🔴 P0~~ | U3 | opencode | [x] glob/grep 工具无超时 | 稳定性 | 小 |
| ~~🔴 P0~~ | U11 | MiMo-Code | [x] 子智能体执行期间 TUI 冻结 | 稳定性 | 中 |
| ~~🔴 P0~~ | S1 | opencode | [x] Mac M4 多 session 系统冻结 | 稳定性 | 中 |
| ~~🔴 P0~~ | J2 | jcode | [x] 大上下文模型 compaction 永不触发 | Token | 中 |
| ~~🔴 P0~~ | J4 | jcode | [x] TUI 粘贴文本 panic (critical) | 稳定性 | 中 |
| ~~🔴 P0~~ | H7 | hermes-agent | [x] Token 统计永久失效 | Token | 小 |
| 🟠 P1 | O1 | openclaw | [ ] totalTokens ~2x 真实上下文 | Token | 中 |
| 🟠 P1 | T2 | kilocode | [-] 长会话 compaction 对免费模型失败 | Token | 中 |
| ~~🟠 P1~~ | T3 | kilocode | [x] 自动 compaction 无限循环 | Token | 中 |
| 🟠 P1 | T5 | kimi-cli | [-] Goal 模式空操作 token 消耗 | Token | 小 |
| 🟠 P1 | U1 | opencode | [ ] session 切换性能 | 性能 | 中 |
| 🟠 P1 | U4 | kimi-code | [ ] 权限审批后滚动重置 | TUI 交互 | 小 |
| 🟠 P1 | U5 | kimi-code | [ ] 子智能体完成后仍显示运行中 | 子智能体 | 小 |
| 🟠 P1 | U7 | kimi-cli | [ ] TUI 热切换推理强度 | TUI 交互 | 中 |
| 🟠 P1 | U8 | kimi-cli | [ ] Shell 输出过长淹没 TUI | 输出管理 | 小 |
| 🟠 P1 | U15 | gemini-cli | [ ] 重试时不显示进度 | 状态反馈 | 小 |
| 🟠 P1 | J5 | jcode | [-] MCP 连接超时阻塞 30 秒 | 启动体验 | 中 |
| 🟠 P1 | H1 | hermes-agent | [-] SSE 心跳导致连接挂起 | 稳定性 | 小 |
| ~~🟠 P1~~ | H5 | hermes-agent | [x] 禁用未使用 skill 注入 | Token | 小 |
| 🟠 P1 | O5 | openclaw | [ ] 支持 fast mode (2.5x 吞吐) | 性能 | 中 |
| 🟠 P1 | C2 | claude-code-best | [ ] push/pop 上下文栈 + /digest | Token | 大 |
| 🟠 P1 | C5 | claude-code-best | [-] OpenAI reasoning_effort | Token | 小 |
| 🟡 P2 | T10 | kimi-code | [-] 超大工具结果阻止去重 | Token | 中 |
| ~~🟡 P2~~ | T21 | kimi-code | [x] footer 显示 token 用量 | 信息展示 | 小 |
| 🟡 P2 | U2 | opencode | [ ] Markdown 流式冗余高亮 | 性能 | 中 |
| 🟡 P2 | U9 | kimi-cli | [ ] 对话完成后滚动跳到底部 | 滚动行为 | 小 |
| ~~🟡 P2~~ | J3 | jcode | [x] 长会话 TUI 输入滚动退化 | TUI 性能 | 中 |
| ~~🟡 P2~~ | H3 | hermes-agent | [x] 模型静默回退无通知 | 信息透明 | 小 |

---

## 七、loop-engineering 仓库需求（新增）

> 来源：cobusgreyling/loop-engineering issues 分析
> 筛选条件：TUI + macOS + UX/Token 优化相关

### Token 成本优化

| # | Issue | 标题 | 描述 | 分类 | 预估收益 |
|---|-------|------|------|------|----------|
| L1 | #246 | [ ] maker/checker 分离验证 | 独立 Verify agent 定义 done，运行时强制权限隔离，避免无效重试浪费 token | Token 优化 | 避免无效执行浪费 token |
| L2 | #108 | [x] 成本追踪示例 | loop-cost 命令缺少使用示例，无法帮助用户追踪和优化 token 消耗 | Token 可视化 | 用户感知消耗，主动优化 |
| L3 | #153 | [x] 断路器防止无限循环 | loop-context circuit breaker 在重复失败或达到最大尝试次数时终止，防止 token 无限消耗 | Token 优化 | 避免无限重试浪费 token |
| L4 | #169 | [x] 上下文管理 | loop-context 帮助管理会话上下文，避免上下文膨胀导致的 token 浪费 | Token 优化 | 减少无效上下文 token |

### 用户体验优化

| # | Issue | 标题 | 描述 | 分类 | 预估收益 |
|---|-------|------|------|------|----------|
| L5 | #408 | [ ] 并发 worktree 管理 | 两个 createWorktree() 并发执行时 manifest 丢失条目，导致 worktree 追踪失败 | 稳定性 | 避免 worktree 丢失导致的重复创建 |
| L6 | #393 | [x] 安全命令执行 | inputs.command 未正确引用，多参数/多行命令容易出错 | 安全 | 避免命令执行失败导致的重试 |
| L7 | #390 | [x] 沙箱执行 | loop-sandbox 提供临时 worktree 隔离，安全执行修复命令 | 安全 | 安全试错，避免污染主分支 |
| L8 | #391 | [x] 门控机制 | loop-gate 提供 denylist 和 auto-merge allowlist，控制自动化边界 | 安全 | 防止危险操作，提高信任度 |
| L9 | #139 | [x] 参数验证 | 无效数值参数被静默接受，导致安全限制被绕过 | 稳定性 | 快速失败，避免错误配置导致的问题 |

### 按优先级汇总

| 优先级 | 编号 | Issue | 标题 | 类型 | 预估工作量 |
|--------|------|-------|------|------|-----------|
| 🟠 P1 | L5 | #408 | [ ] 并发 worktree 管理 | 稳定性 | 中 |
| 🟠 P1 | L1 | #246 | [ ] maker/checker 分离验证 | Token | 中 |
| ~~🟠 P1~~ | L3 | #153 | [x] 断路器防止无限循环 | Token | 小 |
| ~~🟠 P1~~ | L7 | #390 | [x] 沙箱执行 | 安全 | 中 |
| ~~🟠 P1~~ | L8 | #391 | [x] 门控机制 | 安全 | 中 |
| ~~🟡 P2~~ | L6 | #393 | [x] 安全命令执行 | 安全 | 小 |
| ~~🟡 P2~~ | L9 | #139 | [x] 参数验证 | 稳定性 | 小 |
| ~~🟡 P2~~ | L4 | #169 | [x] 上下文管理 | Token | 小 |
| ~~🟡 P2~~ | L2 | #108 | [x] 成本追踪示例 | Token | 小 |

---

## 八、筛选说明

### 筛选标准

1. **TUI 相关**：排除纯 GUI/Web/Desktop/VSCode 需求
2. **macOS 适用**：排除 Windows-only bug
3. **提高用户体验**：更快响应速度、常用能力、易用性
4. **降低用户成本**：节省 token、优化上下文、减少资源消耗

### 排除的类别

- 纯 Web/Desktop/GUI 需求
- 计费/订阅/退款问题
- Windows-only bug
- 非 TUI 功能（ACP 协议、状态页等）
- 服务端/Bug 非本地问题
- 登录/认证基础设施
- 插件/扩展生态系统
- 品牌/元数据

### 仓库来源统计

| 仓库 | 筛选出数量 | 关键发现 |
|------|-----------|----------|
| opencode | 12 | system prompt 路径浪费、session 切换性能、Tree-sitter 冗余高亮 |
| kilocode | 19 | compaction 问题、TUI 设置对话框、权限配置 |
| gemini-cli | 20 | Token 消耗循环、主线程阻塞 I/O、MCP 超时 |
| claude-mem | 17 | Observer 历史无限增长、macOS 45GB 内存泄漏 |
| MiMo-Code | 22 | /btw 百万 token、OOM 10-20GB、TUI 冻结 |
| kimi-code | 19 | 滚动重置、子智能体状态、Shell 输出淹没 |
| kimi-cli | 27 | compaction 浪费 20K tokens、热切换推理强度 |
| jcode | 11 | macOS stdin 常量错误、大模型 compaction 失效、粘贴 panic |
| hermes-agent | 14 | Token 统计失效、SSE 心跳挂起、模型切换污染 |
| openclaw | 8 | totalTokens 2x 偏差、prompt cache 0% 命中、fast mode |
| claude-code-best | 9 | push/pop 上下文栈、按模型配置窗口、reasoning_effort |
| loop-engineering | 9 | 并发 worktree 管理、maker/checker 分离、断路器、沙箱执行 |
| **合计** | **187** | |

---

## 六、新增仓库筛选结果（v3）

> 范围：仅保留 **TUI（终端 UI）+ macOS** 相关、且属于「提升用户体验（更快响应 / 常用能力）/ 降低用户成本（节省 token）」的候选需求。
> 严格过滤：GUI-only 仓库（stepfun-ai/gelab-zero）与浏览器自动化（browser-use/browser-harness）无终端/TUI 场景，已整体排除；conductor / deer-flow / pydantic-harness 的 Web UI 与非终端 issue 已剔除，仅保留 terminal/macOS 相关或可直接迁移到 TUI Agent 的 token/能力项。
> 源仓库（15 个，#5=#4 charmbracelet/crush、#7=#6 conductor-oss/conductor 为重复）：CodeWhale, gelab-zero, plandex, crush, conductor, DeepSeek-Reasonix, superpowers, deer-flow, warp, trae-agent, qwen-code, cline, browser-harness, aider, pydantic-ai-harness。

### Hmbown/CodeWhale (CW)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| CW1 | Hmbown/CodeWhale | #549 | Interactive TUI hangs on 'working.' at 100% CPU (macOS ARM64, v0.8.7) | macOS ARM64 上 TUI 卡在 'working.' 且 CPU 占满，交互完全无响应 | macOS/perf | P1 |
| CW2 | Hmbown/CodeWhale | #2968 | remote-workbench: self-hosted Mac target — workbench in apple/container Linux VM (zero cloud cost) | 支持自托管 Mac 工作节点（Apple/容器 Linux VM），零云端成本 | macOS/cost | P2 |
| CW3 | Hmbown/CodeWhale | #3143 | Add prompt source map and context-usage report for rules/tools/memory/skills | 提供 prompt 来源映射与上下文用量分项报告（rules/tools/memory/skills） | token/UX | P1 |
| CW4 | Hmbown/CodeWhale | #3906 | perf(tui): render() re-estimates context tokens over ALL api_messages every frame | 每帧 render 对全部 api_messages 重新序列化估算 tokens，热路径浪费 CPU | token/perf | P1 |
| CW5 | Hmbown/CodeWhale | #3190 | feat(tui): surface token throughput during streaming | 流式输出时显示 token 吞吐速度 | token/UX | P1 |
| CW6 | Hmbown/CodeWhale | #166 | UI: real-time cost counter during sub-agent work | 子代理工作期间实时显示成本计数 | token/UX | P1 |
| CW7 | Hmbown/CodeWhale | #1120 | 缓存命中方面似乎还是有些问题 (cache hits still problematic) | prompt cache 命中率异常，未稳定命中，每轮重发前缀 | token | P1 |
| CW8 | Hmbown/CodeWhale | #3474 | /model /sessions TUI selector extremely low text contrast on macOS terminal | macOS 终端下选择器文字对比度过低难以辨认 | macOS/TUI | P2 |
| CW9 | Hmbown/CodeWhale | #1670 | theme='system' detects dark on macOS Light mode with Ghostty | macOS Light 模式下 theme=system 误检测为深色 | macOS/TUI | P2 |
| CW10 | Hmbown/CodeWhale | #1556 | deepseek 在 macOS 下的 ghostty 会一直闪屏 (flickering on Ghostty) | macOS Ghostty 终端下 TUI 持续闪烁，iTerm2 正常 | macOS/TUI | P2 |

### plandex-ai/plandex (PL)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| PL1 | plandex-ai/plandex | #324 | Suggestion: HADS format for context files — reduces token waste for loaded docs | 用 HADS 格式组织上下文文件，减少加载文档的 token 浪费 | token | P1 |
| PL2 | plandex-ai/plandex | #89 | Token limit exceeded before adding conversation | 加入对话前即触发 token 上限 | token | P1 |
| PL3 | plandex-ai/plandex | #34 | Stream buffer tokens too high for file 'go.mod' | 单文件流缓冲 token 过高（go.mod） | token/perf | P2 |
| PL4 | plandex-ai/plandex | #349 | nil pointer dereference crash during build of large files | 构建大文件时 nil pointer 崩溃 | perf/stability | P2 |
| PL5 | plandex-ai/plandex | #251 | Average cost to use this? | 用户关心使用成本，需要成本可见与优化指引 | token | P2 |
| PL6 | plandex-ai/plandex | #297 | Add CLI flags --local and --host for non-interactive local mode | 非交互本地模式与自定义 host 的 CLI 参数（常用能力） | UX/common | P2 |

### charmbracelet/crush (CR)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| CR1 | charmbracelet/crush | #1055 | Missing auto-compaction of context; freezes when starting a new session after context exhausted | 缺少自动 compaction，上下文耗尽后开新会话冻结 | token/stability | P0 |
| CR2 | charmbracelet/crush | #337 | macOS panic: runtime error: invalid memory address or nil pointer dereference | macOS 下 nil 指针 panic 崩溃 | macOS/stability | P1 |
| CR3 | charmbracelet/crush | #2918 | High CPU/RAM usage while streaming long thinking traces in assistant message | 流式长思考链路时 CPU/内存占用高 | perf/token | P1 |
| CR4 | charmbracelet/crush | #3167 | Feature: Display real-time token generation speed in status bar | 状态栏显示实时 token 生成速度 | token/UX | P1 |
| CR5 | charmbracelet/crush | #3373 | URLs in Crush output are not clickable (no OSC 8) — Ghostty & Kitty on macOS | macOS Ghostty/Kitty 下输出 URL 不可点击（缺 OSC 8 超链） | macOS/TUI | P1 |
| CR6 | charmbracelet/crush | #993 | Token & Cost count does not function with a custom provider | 自定义 provider 下 token/成本统计失效 | token | P1 |
| CR7 | charmbracelet/crush | #555 | Unnecessary Input Token Spend - Full Project Context Sent on Every Request | 每轮请求发送完整项目上下文，输入 token 浪费 | token | P0 |
| CR8 | charmbracelet/crush | #447 | Local LM Studio/Ollama Custom Providers Support | 支持本地 LM Studio/Ollama 自定义 provider（常用能力） | UX/common | P2 |
| CR9 | charmbracelet/crush | #3136 | Pasting images from WeChat screenshot clipboard fails on macOS | macOS 下从微信截图剪贴板粘贴图片失败 | macOS/TUI | P2 |
| CR10 | charmbracelet/crush | #3429 | No mouse text selection, click-to-position cursor, or copy support (macOS, Warp) | macOS/Warp 下无鼠标文本选择与点击定位光标 | macOS/TUI | P2 |

### conductor-oss/conductor (CO)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| CO1 | conductor-oss/conductor | #1226 | OpenCode providers hang indefinitely in Conductor while native OpenCode UI completes | Conductor 中 OpenCode provider 永久挂起，而原生 OpenCode TUI 正常完成（跨工具 TUI 代理挂起） | perf/TUI | P2 |
| CO2 | conductor-oss/conductor | #1143 | git not found on macOS when launched from GUI (Tauri empty env issue) | macOS 从 GUI(Tauri) 启动时光 git 找不到（空环境变量） | macOS | P2 |

### esengine/DeepSeek-Reasonix (DR)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| DR1 | esengine/DeepSeek-Reasonix | #3999 | Mac command+c 双击两次退出终端会话 | macOS 下 Cmd+C 双击误退出终端会话 | macOS/TUI | P1 |
| DR2 | esengine/DeepSeek-Reasonix | #3734 | macOS: codegraph 进程在 Reasonix 退出后残留，导致系统卡顿 | 退出后 codegraph 进程残留导致 macOS 卡顿 | macOS/perf | P1 |
| DR3 | esengine/DeepSeek-Reasonix | #3655 | TUI can be suspended by tty input and leave terminal modes dirty | TTY 输入可挂起 TUI 并残留终端模式（需清理） | macOS/TUI | P1 |
| DR4 | esengine/DeepSeek-Reasonix | #5627 | docs say mouse reporting disabled but TUI enables MouseMode, blocking native copy | TUI 实际启用 MouseMode 阻止终端原生文本复制 | macOS/TUI | P1 |
| DR5 | esengine/DeepSeek-Reasonix | #4626 | When waiting for confirmation of input, it will hang up and exit | 等待确认输入时挂起退出 | macOS/stability | P1 |
| DR6 | esengine/DeepSeek-Reasonix | #4211 | cli 1.4.0 之后跑一会儿就自动退出 | CLI 运行一段时间后自动退出 | macOS/stability | P1 |
| DR7 | esengine/DeepSeek-Reasonix | #6387 | v1.17.11 输入框只能显示一行 | 输入框仅显示一行，不可多行 | macOS/TUI | P1 |
| DR8 | esengine/DeepSeek-Reasonix | #6603 | Mac 使用 reasonix CLI 无法复制粘贴 | macOS 下无法复制粘贴 | macOS/TUI | P1 |
| DR9 | esengine/DeepSeek-Reasonix | #3511 | 多行输入支持 | 需要多行输入支持 | macOS/TUI | P1 |
| DR10 | esengine/DeepSeek-Reasonix | #5324 | Scrolling up to view past messages does not work anymore (CLI) | CLI 下向上滚动查看历史失效 | macOS/TUI | P1 |
| DR11 | esengine/DeepSeek-Reasonix | #1122 | macOS M4 stuck on startup | macOS M4 启动卡死 | macOS | P1 |

### obra/superpowers (SP)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| SP1 | obra/superpowers | #87 | Optimize plan generation: Modular task files + orchestrator for 90%+ token reduction | 模块化任务文件+编排器，子代理开发 token 减少 90%+ | token | P0 |
| SP2 | obra/superpowers | #190 | All Skills Preloaded at Startup Consuming 22k+ Tokens (11% of Context) | 启动时预加载全部 skill 消耗 22k+ tokens（占上下文 11%） | token | P1 |
| SP3 | obra/superpowers | #832 | Token optimization: 69% line reduction across all 14 skills | 14 个 skill 减少 69% 行数无行为回退 | token | P1 |
| SP4 | obra/superpowers | #1988 | Codex SDD has no circuit breaker: one task ran ~4h, 120.7M telemetry tokens | 无断路器，单任务跑 4h 累积 120.7M tokens | token/stability | P0 |
| SP5 | obra/superpowers | #750 | Superpowers consume a lot of tokens in Opencode with Codex | Opencode+Codex 下 token 消耗大 | token | P1 |
| SP6 | obra/superpowers | #1940 | Is Superpowers still suitable for the token plan era? cost control difficult | token 计费时代成本控制困难 | token | P1 |
| SP7 | obra/superpowers | #551 | Add a core project memory system for cross-session retrieval | 跨会话项目记忆系统（常用能力） | UX/common | P2 |
| SP8 | obra/superpowers | #351 | Change text search from grep to ripgrep | 文本搜索从 grep 换 ripgrep 提速 | perf | P2 |
| SP9 | obra/superpowers | #100 | Make it more explicit the skill is waiting for user input | 明确提示 skill 正在等待用户输入 | UX | P2 |

### bytedance/deer-flow (DF)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| DF1 | bytedance/deer-flow | #3173 | Raise default summarization trigger to avoid frequent compaction in research runs | 提高默认摘要触发阈值，避免研究任务频繁 compaction | token | P1 |
| DF2 | bytedance/deer-flow | #3125 | display real-time context window usage percentage in chat UI | 实时显示上下文窗口使用百分比 | token/UX | P1 |
| DF3 | bytedance/deer-flow | #3103 | LLM 400 上下文超限 — summary 不触发、无 input_token 限制入口 | 上下文超限 400，摘要不触发且无 input token 上限入口 | token | P1 |
| DF4 | bytedance/deer-flow | #1400 | lsof -nP -iTCP hangs indefinitely on macOS, blocking server startup | macOS 下 lsof 挂起阻塞服务启动 | macOS/perf | P1 |
| DF5 | bytedance/deer-flow | #1602 | SummarizationMiddleware fails in streaming (stream_usage off) -> context overflow | 流式下摘要中间件不触发导致上下文溢出 | token | P1 |
| DF6 | bytedance/deer-flow | #1850 | Unified Persistence Layer: Message History, Event Tracing, Token Tracking & Feedback | 统一持久层：消息历史/事件追踪/token 统计/反馈 | token/UX | P2 |

### warpdotdev/warp (WP)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| WP1 | warpdotdev/warp | #13295 | Agent runs failing to complete with high credit usage and crash/restart loop | Agent 崩溃重启循环，信用消耗高（计费影响） | macOS/token | P0 |
| WP2 | warpdotdev/warp | #7248 | git operations crashes arm macs | ARM Mac 上 git 操作崩溃 | macOS/stability | P1 |
| WP3 | warpdotdev/warp | #9037 | High CPU Usage (>100%) and command hangs on macOS Tahoe and Remote SSH | macOS Tahoe + 远程 SSH 下 CPU>100% 且命令挂起 | macOS/perf | P1 |
| WP4 | warpdotdev/warp | #8205 | Huge memory leak on macOS Tahoe 26.1/26.2 (78GB of memory consumed) | macOS Tahoe 下巨大内存泄漏（78GB） | macOS/perf | P0 |
| WP5 | warpdotdev/warp | #6590 | lag and high CPU usage on macOS 26 when scrolling big scrollback | macOS 26 滚动大 scrollback 卡顿且 CPU 高 | macOS/perf | P1 |
| WP6 | warpdotdev/warp | #9830 | Idle Warp tabs drain GitHub GraphQL API rate limit (~2.4 calls/sec) | 空闲 tab 持续消耗 GitHub GraphQL API 限额 | macOS/token | P1 |
| WP7 | warpdotdev/warp | #5950 | Warp hangs indefinitely on macOS after Feb 27 update | macOS 更新后 Warp 永久挂起 | macOS/stability | P1 |
| WP8 | warpdotdev/warp | #7965 | Terrible performance on M3 Mac | M3 Mac 上性能极差 | macOS/perf | P1 |
| WP9 | warpdotdev/warp | #13040 | Crash on terminal resize with CJK (wide) characters in the prompt | 提示符含 CJK 宽字符时终端 resize 崩溃 | macOS/TUI | P1 |
| WP10 | warpdotdev/warp | #7804 | Process stuck in repetitive 'summarization loop' failing to make progress | 陷入重复摘要循环无进展 | token/stability | P0 |
| WP11 | warpdotdev/warp | #8405 | Feature request: CLI/API to query Warp AI credit usage programmatically | 提供 CLI/API 程序化查询 AI 信用用量 | macOS/token/UX | P1 |
| WP12 | warpdotdev/warp | #8542 | Enhance 'Changes' panel with full Git staging/unstaging/commit (VS Code-like) | 增强 Changes 面板支持完整 Git 暂存/提交 | macOS/UX | P2 |

### bytedance/trae-agent (TA)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| TA1 | bytedance/trae-agent | #228 | Trae-cli always hang and never terminate | CLI 永久挂起不终止 | perf/stability | P1 |
| TA2 | bytedance/trae-agent | #351 | 为什么这个 cli 这么费 token？ | 用户反馈 CLI token 消耗过大 | token | P1 |
| TA3 | bytedance/trae-agent | #233 | 进程吃满 cpu 100%（疑似挖矿） | 进程 CPU 占满 100% | perf | P1 |
| TA4 | bytedance/trae-agent | #364 | AI agent is operating very slowly | 代理运行极慢 | perf | P1 |
| TA5 | bytedance/trae-agent | #195 | trae 目前有做 memory 压缩么（类似 mem0） | 询问是否做 memory 压缩以省 token | token | P1 |
| TA6 | bytedance/trae-agent | #94 | Steps streaming print | 步骤流式打印（实时反馈） | UX | P2 |
| TA7 | bytedance/trae-agent | #14 | Qwen and Deepseek support | 支持更多模型 provider（常用能力） | UX/common | P2 |

### QwenLM/qwen-code (QW)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| QW1 | QwenLM/qwen-code | #6004 | 安装 MCP 过程中任务异常直接闪退 | macOS 下安装 MCP 时异常闪退 | macOS/stability | P1 |
| QW2 | QwenLM/qwen-code | #3264 | ui.statusLine crashes CLI with spawn EBADF on macOS | macOS 下 statusLine 崩溃（spawn EBADF） | macOS/stability | P1 |
| QW3 | QwenLM/qwen-code | #4815 | Severe OOM with --resume and Escape key broken | --resume 严重 OOM 且 Escape 失效 | perf/token | P1 |
| QW4 | QwenLM/qwen-code | #6265 | tool_search invalidates LLM server KV-cache on every deferred-tool load | 每次延迟加载 tool_search 使 KV-cache 失效 | token/perf | P1 |
| QW5 | QwenLM/qwen-code | #6806 | Status line context usage % does not refresh after /compress | /compress 后状态栏上下文百分比不刷新 | token/UX | P1 |
| QW6 | QwenLM/qwen-code | #7831 | Repeated ECONNRESET on streaming when context exceeds ~150k tokens | 上下文超 150k token 时流式重复 ECONNRESET | perf/token | P1 |
| QW7 | QwenLM/qwen-code | #6097 | System prompt fixed overhead reaches ~22k tokens (0.2% signal) | 系统提示固定开销达 22k tokens，信噪比低 | token | P0 |
| QW8 | QwenLM/qwen-code | #5861 | Context compression request should use stream=true to avoid gateway timeout | 上下文压缩应用 stream=true 避免网关超时 | token/perf | P1 |
| QW9 | QwenLM/qwen-code | #5722 | Token speed display bugs: tok/s disappears during thinking | token 速度显示错误（思考/工具调用时 tok/s 消失） | token/UX | P1 |
| QW10 | QwenLM/qwen-code | #5101 | Qwen Code carries repeated large tool results through provider history | 重复大工具结果穿过 provider 历史，膨胀上下文 | token | P1 |
| QW11 | QwenLM/qwen-code | #4695 | Tool-call loop: no client-side circuit breaker | 工具调用循环无客户端断路器，模型陷入重复 tool_call | token/stability | P1 |

### cline/cline (CL)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| CL1 | cline/cline | #484 | MacOS 15 Shell Integration: Unable to Retrieve Terminal Output | macOS Shell Integration 无法获取终端输出 | macOS/TUI | P1 |
| CL2 | cline/cline | #3445 | Terminal output capture failure in Cline v3.15.0/v3.15.1 | 终端输出捕获失败 | macOS/TUI | P1 |
| CL3 | cline/cline | #4356 | Improve Terminal Integration Reliability Across Platforms and Shell Configs | 跨平台/Shell 终端集成可靠性改进（常用能力） | TUI | P1 |
| CL4 | cline/cline | #1404 | Cline hanging after command execution from any tasks | 命令执行后挂起 | perf/stability | P1 |
| CL5 | cline/cline | #1146 | Executing terminal commands hangs cline | 执行终端命令时挂起 | perf/stability | P1 |
| CL6 | cline/cline | #3501 | MCP server data under iCloud-synced Documents -> system-wide slowdowns on macOS | macOS 下 MCP 数据存 iCloud 文档目录导致系统变慢 | macOS/perf | P1 |
| CL7 | cline/cline | #6878 | Intermittent crash leads to loss of visible task history on Apple M3 | Apple M3 上间歇崩溃丢失任务历史 | macOS/stability | P1 |
| CL8 | cline/cline | #4031 | Very high idle CPU on Code Helper (Plugin) MacOS Sequoia (Apple Silicon) | macOS Sequoia 下插件空闲 CPU 极高 | macOS/perf | P1 |
| CL9 | cline/cline | #9660 | prompt is too long: 228307 tokens > 200000 maximum | 提示过长 228k tokens 超 200k 上限 | token | P0 |
| CL10 | cline/cline | #1452 | Excessive Token Consumption loading config files like node_modules on initial load | 初始加载误载 node_modules 等配置文件致 token 浪费 | token | P1 |
| CL11 | cline/cline | #11181 | memorybank sub-agents cause significant token waste | memorybank 子代理造成显著 token 浪费 | token | P1 |
| CL12 | cline/cline | #9323 | Cline CLI hangs when context window is full - no accessible retry button | 上下文满时 CLI 挂起且无重试入口 | token/stability | P1 |

### Aider-AI/aider (AI)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| AI1 | Aider-AI/aider | #276 | Is it possible to set custom timeout? | 支持自定义超时 | perf/UX | P1 |
| AI2 | Aider-AI/aider | #3021 | Either 'Loading' or infinite wait for any request on Mac | macOS 下无限等待/Loading | macOS/perf | P1 |
| AI3 | Aider-AI/aider | #705 | Sonnet 3.5 is using a lot of output tokens, hitting 4k output token limit | 输出 token 过多触及 4k 上限 | token | P1 |
| AI4 | Aider-AI/aider | #863 | Tokens leak? | token 疑似泄漏 | token | P1 |
| AI5 | Aider-AI/aider | #437 | how to optimize costs ? | 成本优化指导需求 | token | P1 |
| AI6 | Aider-AI/aider | #3196 | 100% cpu freezing does not respond to ctrl c | 100% CPU 冻结，ctrl-c 无响应 | perf/stability | P1 |
| AI7 | Aider-AI/aider | #995 | Tab filename completion causes aider to hang | 文件名 Tab 补全导致 aider 挂起 | TUI/perf | P1 |
| AI8 | Aider-AI/aider | #2010 | local file updated but /read cache not refreshed | 本地文件更新后 /read 缓存未刷新 | token/cache | P1 |
| AI9 | Aider-AI/aider | #5447 | Cache unchanged tracked-file results during startup | 启动时缓存未变文件结果 | token/cache | P1 |
| AI10 | Aider-AI/aider | #3104 | Add busy indicator / spinner / progress bar | 增加忙碌指示/进度条 | UX | P2 |
| AI11 | Aider-AI/aider | #4542 | Is Aider suitable for complex and large-scale projects? (models only 4k tokens by default) | 大模型默认仅 4k token，不适合大型项目 | token | P2 |

### pydantic/pydantic-ai-harness (PH)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| PH1 | pydantic/pydantic-ai-harness | #40 | Deferred Tool Loading / Tool Search capability | 延迟工具加载/工具搜索，减少每次全量注入 token | token/common | P1 |
| PH2 | pydantic/pydantic-ai-harness | #84 | Adaptive Reasoning Effort (per-step thinking budget selection) | 逐步自适应推理强度，控制 thinking token | token/common | P1 |
| PH3 | pydantic/pydantic-ai-harness | #93 | Git History as Compressed Context | 用 Git 历史作为压缩上下文，降低 token | token/common | P1 |
| PH4 | pydantic/pydantic-ai-harness | #357 | SubAgents: no per-delegation model selection (cheaper/stronger routing) | 子代理无法按任务选更便宜/更强模型路由 | token/common | P1 |
| PH5 | pydantic/pydantic-ai-harness | #18 | CLI agent loading from spec files | 从 spec 文件加载 CLI agent（命令行能力） | TUI/common | P2 |

### 按优先级汇总（P0/P1 推荐实施）

| 优先级 | 编号 | 来源 | Issue | 标题 | 分类 |
|--------|------|------|-------|------|------|
| P0 | CL9 | cline/cline | #9660 | prompt is too long: 228307 tokens > 200000 maximum | token |
| P0 | CR1 | charmbracelet/crush | #1055 | Missing auto-compaction of context; freezes when starting a new session after context exhausted | token/stability |
| P0 | CR7 | charmbracelet/crush | #555 | Unnecessary Input Token Spend - Full Project Context Sent on Every Request | token |
| P0 | QW7 | QwenLM/qwen-code | #6097 | System prompt fixed overhead reaches ~22k tokens (0.2% signal) | token |
| P0 | SP1 | obra/superpowers | #87 | Optimize plan generation: Modular task files + orchestrator for 90%+ token reduction | token |
| P0 | SP4 | obra/superpowers | #1988 | Codex SDD has no circuit breaker: one task ran ~4h, 120.7M telemetry tokens | token/stability |
| P0 | WP1 | warpdotdev/warp | #13295 | Agent runs failing to complete with high credit usage and crash/restart loop | macOS/token |
| P0 | WP10 | warpdotdev/warp | #7804 | Process stuck in repetitive 'summarization loop' failing to make progress | token/stability |
| P0 | WP4 | warpdotdev/warp | #8205 | Huge memory leak on macOS Tahoe 26.1/26.2 (78GB of memory consumed) | macOS/perf |
| P1 | AI1 | Aider-AI/aider | #276 | Is it possible to set custom timeout? | perf/UX |
| P1 | AI2 | Aider-AI/aider | #3021 | Either 'Loading' or infinite wait for any request on Mac | macOS/perf |
| P1 | AI3 | Aider-AI/aider | #705 | Sonnet 3.5 is using a lot of output tokens, hitting 4k output token limit | token |
| P1 | AI4 | Aider-AI/aider | #863 | Tokens leak? | token |
| P1 | AI5 | Aider-AI/aider | #437 | how to optimize costs ? | token |
| P1 | AI6 | Aider-AI/aider | #3196 | 100% cpu freezing does not respond to ctrl c | perf/stability |
| P1 | AI7 | Aider-AI/aider | #995 | Tab filename completion causes aider to hang | TUI/perf |
| P1 | AI8 | Aider-AI/aider | #2010 | local file updated but /read cache not refreshed | token/cache |
| P1 | AI9 | Aider-AI/aider | #5447 | Cache unchanged tracked-file results during startup | token/cache |
| P1 | CL1 | cline/cline | #484 | MacOS 15 Shell Integration: Unable to Retrieve Terminal Output | macOS/TUI |
| P1 | CL10 | cline/cline | #1452 | Excessive Token Consumption loading config files like node_modules on initial load | token |
| P1 | CL11 | cline/cline | #11181 | memorybank sub-agents cause significant token waste | token |
| P1 | CL12 | cline/cline | #9323 | Cline CLI hangs when context window is full - no accessible retry button | token/stability |
| P1 | CL2 | cline/cline | #3445 | Terminal output capture failure in Cline v3.15.0/v3.15.1 | macOS/TUI |
| P1 | CL3 | cline/cline | #4356 | Improve Terminal Integration Reliability Across Platforms and Shell Configs | TUI |
| P1 | CL4 | cline/cline | #1404 | Cline hanging after command execution from any tasks | perf/stability |
| P1 | CL5 | cline/cline | #1146 | Executing terminal commands hangs cline | perf/stability |
| P1 | CL6 | cline/cline | #3501 | MCP server data under iCloud-synced Documents -> system-wide slowdowns on macOS | macOS/perf |
| P1 | CL7 | cline/cline | #6878 | Intermittent crash leads to loss of visible task history on Apple M3 | macOS/stability |
| P1 | CL8 | cline/cline | #4031 | Very high idle CPU on Code Helper (Plugin) MacOS Sequoia (Apple Silicon) | macOS/perf |
| P1 | CR2 | charmbracelet/crush | #337 | macOS panic: runtime error: invalid memory address or nil pointer dereference | macOS/stability |
| P1 | CR3 | charmbracelet/crush | #2918 | High CPU/RAM usage while streaming long thinking traces in assistant message | perf/token |
| P1 | CR4 | charmbracelet/crush | #3167 | Feature: Display real-time token generation speed in status bar | token/UX |
| P1 | CR5 | charmbracelet/crush | #3373 | URLs in Crush output are not clickable (no OSC 8) — Ghostty & Kitty on macOS | macOS/TUI |
| P1 | CR6 | charmbracelet/crush | #993 | Token & Cost count does not function with a custom provider | token |
| P1 | CW1 | Hmbown/CodeWhale | #549 | Interactive TUI hangs on 'working.' at 100% CPU (macOS ARM64, v0.8.7) | macOS/perf |
| P1 | CW3 | Hmbown/CodeWhale | #3143 | Add prompt source map and context-usage report for rules/tools/memory/skills | token/UX |
| P1 | CW4 | Hmbown/CodeWhale | #3906 | perf(tui): render() re-estimates context tokens over ALL api_messages every frame | token/perf |
| P1 | CW5 | Hmbown/CodeWhale | #3190 | feat(tui): surface token throughput during streaming | token/UX |
| P1 | CW6 | Hmbown/CodeWhale | #166 | UI: real-time cost counter during sub-agent work | token/UX |
| P1 | CW7 | Hmbown/CodeWhale | #1120 | 缓存命中方面似乎还是有些问题 (cache hits still problematic) | token |
| P1 | DF1 | bytedance/deer-flow | #3173 | Raise default summarization trigger to avoid frequent compaction in research runs | token |
| P1 | DF2 | bytedance/deer-flow | #3125 | display real-time context window usage percentage in chat UI | token/UX |
| P1 | DF3 | bytedance/deer-flow | #3103 | LLM 400 上下文超限 — summary 不触发、无 input_token 限制入口 | token |
| P1 | DF4 | bytedance/deer-flow | #1400 | lsof -nP -iTCP hangs indefinitely on macOS, blocking server startup | macOS/perf |
| P1 | DF5 | bytedance/deer-flow | #1602 | SummarizationMiddleware fails in streaming (stream_usage off) -> context overflow | token |
| P1 | DR1 | esengine/DeepSeek-Reasonix | #3999 | Mac command+c 双击两次退出终端会话 | macOS/TUI |
| P1 | DR10 | esengine/DeepSeek-Reasonix | #5324 | Scrolling up to view past messages does not work anymore (CLI) | macOS/TUI |
| P1 | DR11 | esengine/DeepSeek-Reasonix | #1122 | macOS M4 stuck on startup | macOS |
| P1 | DR2 | esengine/DeepSeek-Reasonix | #3734 | macOS: codegraph 进程在 Reasonix 退出后残留，导致系统卡顿 | macOS/perf |
| P1 | DR3 | esengine/DeepSeek-Reasonix | #3655 | TUI can be suspended by tty input and leave terminal modes dirty | macOS/TUI |
| P1 | DR4 | esengine/DeepSeek-Reasonix | #5627 | docs say mouse reporting disabled but TUI enables MouseMode, blocking native copy | macOS/TUI |
| P1 | DR5 | esengine/DeepSeek-Reasonix | #4626 | When waiting for confirmation of input, it will hang up and exit | macOS/stability |
| P1 | DR6 | esengine/DeepSeek-Reasonix | #4211 | cli 1.4.0 之后跑一会儿就自动退出 | macOS/stability |
| P1 | DR7 | esengine/DeepSeek-Reasonix | #6387 | v1.17.11 输入框只能显示一行 | macOS/TUI |
| P1 | DR8 | esengine/DeepSeek-Reasonix | #6603 | Mac 使用 reasonix CLI 无法复制粘贴 | macOS/TUI |
| P1 | DR9 | esengine/DeepSeek-Reasonix | #3511 | 多行输入支持 | macOS/TUI |
| P1 | PH1 | pydantic/pydantic-ai-harness | #40 | Deferred Tool Loading / Tool Search capability | token/common |
| P1 | PH2 | pydantic/pydantic-ai-harness | #84 | Adaptive Reasoning Effort (per-step thinking budget selection) | token/common |
| P1 | PH3 | pydantic/pydantic-ai-harness | #93 | Git History as Compressed Context | token/common |
| P1 | PH4 | pydantic/pydantic-ai-harness | #357 | SubAgents: no per-delegation model selection (cheaper/stronger routing) | token/common |
| P1 | PL1 | plandex-ai/plandex | #324 | Suggestion: HADS format for context files — reduces token waste for loaded docs | token |
| P1 | PL2 | plandex-ai/plandex | #89 | Token limit exceeded before adding conversation | token |
| P1 | QW1 | QwenLM/qwen-code | #6004 | 安装 MCP 过程中任务异常直接闪退 | macOS/stability |
| P1 | QW10 | QwenLM/qwen-code | #5101 | Qwen Code carries repeated large tool results through provider history | token |
| P1 | QW11 | QwenLM/qwen-code | #4695 | Tool-call loop: no client-side circuit breaker | token/stability |
| P1 | QW2 | QwenLM/qwen-code | #3264 | ui.statusLine crashes CLI with spawn EBADF on macOS | macOS/stability |
| P1 | QW3 | QwenLM/qwen-code | #4815 | Severe OOM with --resume and Escape key broken | perf/token |
| P1 | QW4 | QwenLM/qwen-code | #6265 | tool_search invalidates LLM server KV-cache on every deferred-tool load | token/perf |
| P1 | QW5 | QwenLM/qwen-code | #6806 | Status line context usage % does not refresh after /compress | token/UX |
| P1 | QW6 | QwenLM/qwen-code | #7831 | Repeated ECONNRESET on streaming when context exceeds ~150k tokens | perf/token |
| P1 | QW8 | QwenLM/qwen-code | #5861 | Context compression request should use stream=true to avoid gateway timeout | token/perf |
| P1 | QW9 | QwenLM/qwen-code | #5722 | Token speed display bugs: tok/s disappears during thinking | token/UX |
| P1 | SP2 | obra/superpowers | #190 | All Skills Preloaded at Startup Consuming 22k+ Tokens (11% of Context) | token |
| P1 | SP3 | obra/superpowers | #832 | Token optimization: 69% line reduction across all 14 skills | token |
| P1 | SP5 | obra/superpowers | #750 | Superpowers consume a lot of tokens in Opencode with Codex | token |
| P1 | SP6 | obra/superpowers | #1940 | Is Superpowers still suitable for the token plan era? cost control difficult | token |
| P1 | TA1 | bytedance/trae-agent | #228 | Trae-cli always hang and never terminate | perf/stability |
| P1 | TA2 | bytedance/trae-agent | #351 | 为什么这个 cli 这么费 token？ | token |
| P1 | TA3 | bytedance/trae-agent | #233 | 进程吃满 cpu 100%（疑似挖矿） | perf |
| P1 | TA4 | bytedance/trae-agent | #364 | AI agent is operating very slowly | perf |
| P1 | TA5 | bytedance/trae-agent | #195 | trae 目前有做 memory 压缩么（类似 mem0） | token |
| P1 | WP11 | warpdotdev/warp | #8405 | Feature request: CLI/API to query Warp AI credit usage programmatically | macOS/token/UX |
| P1 | WP2 | warpdotdev/warp | #7248 | git operations crashes arm macs | macOS/stability |
| P1 | WP3 | warpdotdev/warp | #9037 | High CPU Usage (>100%) and command hangs on macOS Tahoe and Remote SSH | macOS/perf |
| P1 | WP5 | warpdotdev/warp | #6590 | lag and high CPU usage on macOS 26 when scrolling big scrollback | macOS/perf |
| P1 | WP6 | warpdotdev/warp | #9830 | Idle Warp tabs drain GitHub GraphQL API rate limit (~2.4 calls/sec) | macOS/token |
| P1 | WP7 | warpdotdev/warp | #5950 | Warp hangs indefinitely on macOS after Feb 27 update | macOS/stability |
| P1 | WP8 | warpdotdev/warp | #7965 | Terrible performance on M3 Mac | macOS/perf |
| P1 | WP9 | warpdotdev/warp | #13040 | Crash on terminal resize with CJK (wide) characters in the prompt | macOS/TUI |

---

## 六、新增仓库筛选结果（v3）

> 范围：仅保留 **TUI（终端 UI）+ macOS** 相关、且属于「提升用户体验（更快响应 / 常用能力）/ 降低用户成本（节省 token）」的候选需求。
> 严格过滤：GUI-only 仓库（stepfun-ai/gelab-zero）与浏览器自动化（browser-use/browser-harness）无终端/TUI 场景，已整体排除；conductor / deer-flow / pydantic-harness 的 Web UI 与非终端 issue 已剔除，仅保留 terminal/macOS 相关或可直接迁移到 TUI Agent 的 token/能力项。
> 源仓库（15 个，#5=#4 charmbracelet/crush、#7=#6 conductor-oss/conductor 为重复）：CodeWhale, gelab-zero, plandex, crush, conductor, DeepSeek-Reasonix, superpowers, deer-flow, warp, trae-agent, qwen-code, cline, browser-harness, aider, pydantic-ai-harness。

### Hmbown/CodeWhale (CW)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| CW1 | Hmbown/CodeWhale | #549 | Interactive TUI hangs on 'working.' at 100% CPU (macOS ARM64, v0.8.7) | macOS ARM64 上 TUI 卡在 'working.' 且 CPU 占满，交互完全无响应 | macOS/perf | P1 |
| CW2 | Hmbown/CodeWhale | #2968 | remote-workbench: self-hosted Mac target — workbench in apple/container Linux VM (zero cloud cost) | 支持自托管 Mac 工作节点（Apple/容器 Linux VM），零云端成本 | macOS/cost | P2 |
| CW3 | Hmbown/CodeWhale | #3143 | Add prompt source map and context-usage report for rules/tools/memory/skills | 提供 prompt 来源映射与上下文用量分项报告（rules/tools/memory/skills） | token/UX | P1 |
| CW4 | Hmbown/CodeWhale | #3906 | perf(tui): render() re-estimates context tokens over ALL api_messages every frame | 每帧 render 对全部 api_messages 重新序列化估算 tokens，热路径浪费 CPU | token/perf | P1 |
| CW5 | Hmbown/CodeWhale | #3190 | feat(tui): surface token throughput during streaming | 流式输出时显示 token 吞吐速度 | token/UX | P1 |
| CW6 | Hmbown/CodeWhale | #166 | UI: real-time cost counter during sub-agent work | 子代理工作期间实时显示成本计数 | token/UX | P1 |
| CW7 | Hmbown/CodeWhale | #1120 | 缓存命中方面似乎还是有些问题 (cache hits still problematic) | prompt cache 命中率异常，未稳定命中，每轮重发前缀 | token | P1 |
| CW8 | Hmbown/CodeWhale | #3474 | /model /sessions TUI selector extremely low text contrast on macOS terminal | macOS 终端下选择器文字对比度过低难以辨认 | macOS/TUI | P2 |
| CW9 | Hmbown/CodeWhale | #1670 | theme='system' detects dark on macOS Light mode with Ghostty | macOS Light 模式下 theme=system 误检测为深色 | macOS/TUI | P2 |
| CW10 | Hmbown/CodeWhale | #1556 | deepseek 在 macOS 下的 ghostty 会一直闪屏 (flickering on Ghostty) | macOS Ghostty 终端下 TUI 持续闪烁，iTerm2 正常 | macOS/TUI | P2 |

### plandex-ai/plandex (PL)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| PL1 | plandex-ai/plandex | #324 | Suggestion: HADS format for context files — reduces token waste for loaded docs | 用 HADS 格式组织上下文文件，减少加载文档的 token 浪费 | token | P1 |
| PL2 | plandex-ai/plandex | #89 | Token limit exceeded before adding conversation | 加入对话前即触发 token 上限 | token | P1 |
| PL3 | plandex-ai/plandex | #34 | Stream buffer tokens too high for file 'go.mod' | 单文件流缓冲 token 过高（go.mod） | token/perf | P2 |
| PL4 | plandex-ai/plandex | #349 | nil pointer dereference crash during build of large files | 构建大文件时 nil pointer 崩溃 | perf/stability | P2 |
| PL5 | plandex-ai/plandex | #251 | Average cost to use this? | 用户关心使用成本，需要成本可见与优化指引 | token | P2 |
| PL6 | plandex-ai/plandex | #297 | Add CLI flags --local and --host for non-interactive local mode | 非交互本地模式与自定义 host 的 CLI 参数（常用能力） | UX/common | P2 |

### charmbracelet/crush (CR)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| CR1 | charmbracelet/crush | #1055 | Missing auto-compaction of context; freezes when starting a new session after context exhausted | 缺少自动 compaction，上下文耗尽后开新会话冻结 | token/stability | P0 |
| CR2 | charmbracelet/crush | #337 | macOS panic: runtime error: invalid memory address or nil pointer dereference | macOS 下 nil 指针 panic 崩溃 | macOS/stability | P1 |
| CR3 | charmbracelet/crush | #2918 | High CPU/RAM usage while streaming long thinking traces in assistant message | 流式长思考链路时 CPU/内存占用高 | perf/token | P1 |
| CR4 | charmbracelet/crush | #3167 | Feature: Display real-time token generation speed in status bar | 状态栏显示实时 token 生成速度 | token/UX | P1 |
| CR5 | charmbracelet/crush | #3373 | URLs in Crush output are not clickable (no OSC 8) — Ghostty & Kitty on macOS | macOS Ghostty/Kitty 下输出 URL 不可点击（缺 OSC 8 超链） | macOS/TUI | P1 |
| CR6 | charmbracelet/crush | #993 | Token & Cost count does not function with a custom provider | 自定义 provider 下 token/成本统计失效 | token | P1 |
| CR7 | charmbracelet/crush | #555 | Unnecessary Input Token Spend - Full Project Context Sent on Every Request | 每轮请求发送完整项目上下文，输入 token 浪费 | token | P0 |
| CR8 | charmbracelet/crush | #447 | Local LM Studio/Ollama Custom Providers Support | 支持本地 LM Studio/Ollama 自定义 provider（常用能力） | UX/common | P2 |
| CR9 | charmbracelet/crush | #3136 | Pasting images from WeChat screenshot clipboard fails on macOS | macOS 下从微信截图剪贴板粘贴图片失败 | macOS/TUI | P2 |
| CR10 | charmbracelet/crush | #3429 | No mouse text selection, click-to-position cursor, or copy support (macOS, Warp) | macOS/Warp 下无鼠标文本选择与点击定位光标 | macOS/TUI | P2 |

### conductor-oss/conductor (CO)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| CO1 | conductor-oss/conductor | #1226 | OpenCode providers hang indefinitely in Conductor while native OpenCode UI completes | Conductor 中 OpenCode provider 永久挂起，而原生 OpenCode TUI 正常完成（跨工具 TUI 代理挂起） | perf/TUI | P2 |
| CO2 | conductor-oss/conductor | #1143 | git not found on macOS when launched from GUI (Tauri empty env issue) | macOS 从 GUI(Tauri) 启动时光 git 找不到（空环境变量） | macOS | P2 |

### esengine/DeepSeek-Reasonix (DR)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| DR1 | esengine/DeepSeek-Reasonix | #3999 | Mac command+c 双击两次退出终端会话 | macOS 下 Cmd+C 双击误退出终端会话 | macOS/TUI | P1 |
| DR2 | esengine/DeepSeek-Reasonix | #3734 | macOS: codegraph 进程在 Reasonix 退出后残留，导致系统卡顿 | 退出后 codegraph 进程残留导致 macOS 卡顿 | macOS/perf | P1 |
| DR3 | esengine/DeepSeek-Reasonix | #3655 | TUI can be suspended by tty input and leave terminal modes dirty | TTY 输入可挂起 TUI 并残留终端模式（需清理） | macOS/TUI | P1 |
| DR4 | esengine/DeepSeek-Reasonix | #5627 | docs say mouse reporting disabled but TUI enables MouseMode, blocking native copy | TUI 实际启用 MouseMode 阻止终端原生文本复制 | macOS/TUI | P1 |
| DR5 | esengine/DeepSeek-Reasonix | #4626 | When waiting for confirmation of input, it will hang up and exit | 等待确认输入时挂起退出 | macOS/stability | P1 |
| DR6 | esengine/DeepSeek-Reasonix | #4211 | cli 1.4.0 之后跑一会儿就自动退出 | CLI 运行一段时间后自动退出 | macOS/stability | P1 |
| DR7 | esengine/DeepSeek-Reasonix | #6387 | v1.17.11 输入框只能显示一行 | 输入框仅显示一行，不可多行 | macOS/TUI | P1 |
| DR8 | esengine/DeepSeek-Reasonix | #6603 | Mac 使用 reasonix CLI 无法复制粘贴 | macOS 下无法复制粘贴 | macOS/TUI | P1 |
| DR9 | esengine/DeepSeek-Reasonix | #3511 | 多行输入支持 | 需要多行输入支持 | macOS/TUI | P1 |
| DR10 | esengine/DeepSeek-Reasonix | #5324 | Scrolling up to view past messages does not work anymore (CLI) | CLI 下向上滚动查看历史失效 | macOS/TUI | P1 |
| DR11 | esengine/DeepSeek-Reasonix | #1122 | macOS M4 stuck on startup | macOS M4 启动卡死 | macOS | P1 |

### obra/superpowers (SP)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| SP1 | obra/superpowers | #87 | Optimize plan generation: Modular task files + orchestrator for 90%+ token reduction | 模块化任务文件+编排器，子代理开发 token 减少 90%+ | token | P0 |
| SP2 | obra/superpowers | #190 | All Skills Preloaded at Startup Consuming 22k+ Tokens (11% of Context) | 启动时预加载全部 skill 消耗 22k+ tokens（占上下文 11%） | token | P1 |
| SP3 | obra/superpowers | #832 | Token optimization: 69% line reduction across all 14 skills | 14 个 skill 减少 69% 行数无行为回退 | token | P1 |
| SP4 | obra/superpowers | #1988 | Codex SDD has no circuit breaker: one task ran ~4h, 120.7M telemetry tokens | 无断路器，单任务跑 4h 累积 120.7M tokens | token/stability | P0 |
| SP5 | obra/superpowers | #750 | Superpowers consume a lot of tokens in Opencode with Codex | Opencode+Codex 下 token 消耗大 | token | P1 |
| SP6 | obra/superpowers | #1940 | Is Superpowers still suitable for the token plan era? cost control difficult | token 计费时代成本控制困难 | token | P1 |
| SP7 | obra/superpowers | #551 | Add a core project memory system for cross-session retrieval | 跨会话项目记忆系统（常用能力） | UX/common | P2 |
| SP8 | obra/superpowers | #351 | Change text search from grep to ripgrep | 文本搜索从 grep 换 ripgrep 提速 | perf | P2 |
| SP9 | obra/superpowers | #100 | Make it more explicit the skill is waiting for user input | 明确提示 skill 正在等待用户输入 | UX | P2 |

### bytedance/deer-flow (DF)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| DF1 | bytedance/deer-flow | #3173 | Raise default summarization trigger to avoid frequent compaction in research runs | 提高默认摘要触发阈值，避免研究任务频繁 compaction | token | P1 |
| DF2 | bytedance/deer-flow | #3125 | display real-time context window usage percentage in chat UI | 实时显示上下文窗口使用百分比 | token/UX | P1 |
| DF3 | bytedance/deer-flow | #3103 | LLM 400 上下文超限 — summary 不触发、无 input_token 限制入口 | 上下文超限 400，摘要不触发且无 input token 上限入口 | token | P1 |
| DF4 | bytedance/deer-flow | #1400 | lsof -nP -iTCP hangs indefinitely on macOS, blocking server startup | macOS 下 lsof 挂起阻塞服务启动 | macOS/perf | P1 |
| DF5 | bytedance/deer-flow | #1602 | SummarizationMiddleware fails in streaming (stream_usage off) -> context overflow | 流式下摘要中间件不触发导致上下文溢出 | token | P1 |
| DF6 | bytedance/deer-flow | #1850 | Unified Persistence Layer: Message History, Event Tracing, Token Tracking & Feedback | 统一持久层：消息历史/事件追踪/token 统计/反馈 | token/UX | P2 |

### warpdotdev/warp (WP)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| WP1 | warpdotdev/warp | #13295 | Agent runs failing to complete with high credit usage and crash/restart loop | Agent 崩溃重启循环，信用消耗高（计费影响） | macOS/token | P0 |
| WP2 | warpdotdev/warp | #7248 | git operations crashes arm macs | ARM Mac 上 git 操作崩溃 | macOS/stability | P1 |
| WP3 | warpdotdev/warp | #9037 | High CPU Usage (>100%) and command hangs on macOS Tahoe and Remote SSH | macOS Tahoe + 远程 SSH 下 CPU>100% 且命令挂起 | macOS/perf | P1 |
| WP4 | warpdotdev/warp | #8205 | Huge memory leak on macOS Tahoe 26.1/26.2 (78GB of memory consumed) | macOS Tahoe 下巨大内存泄漏（78GB） | macOS/perf | P0 |
| WP5 | warpdotdev/warp | #6590 | lag and high CPU usage on macOS 26 when scrolling big scrollback | macOS 26 滚动大 scrollback 卡顿且 CPU 高 | macOS/perf | P1 |
| WP6 | warpdotdev/warp | #9830 | Idle Warp tabs drain GitHub GraphQL API rate limit (~2.4 calls/sec) | 空闲 tab 持续消耗 GitHub GraphQL API 限额 | macOS/token | P1 |
| WP7 | warpdotdev/warp | #5950 | Warp hangs indefinitely on macOS after Feb 27 update | macOS 更新后 Warp 永久挂起 | macOS/stability | P1 |
| WP8 | warpdotdev/warp | #7965 | Terrible performance on M3 Mac | M3 Mac 上性能极差 | macOS/perf | P1 |
| WP9 | warpdotdev/warp | #13040 | Crash on terminal resize with CJK (wide) characters in the prompt | 提示符含 CJK 宽字符时终端 resize 崩溃 | macOS/TUI | P1 |
| WP10 | warpdotdev/warp | #7804 | Process stuck in repetitive 'summarization loop' failing to make progress | 陷入重复摘要循环无进展 | token/stability | P0 |
| WP11 | warpdotdev/warp | #8405 | Feature request: CLI/API to query Warp AI credit usage programmatically | 提供 CLI/API 程序化查询 AI 信用用量 | macOS/token/UX | P1 |
| WP12 | warpdotdev/warp | #8542 | Enhance 'Changes' panel with full Git staging/unstaging/commit (VS Code-like) | 增强 Changes 面板支持完整 Git 暂存/提交 | macOS/UX | P2 |

### bytedance/trae-agent (TA)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| TA1 | bytedance/trae-agent | #228 | Trae-cli always hang and never terminate | CLI 永久挂起不终止 | perf/stability | P1 |
| TA2 | bytedance/trae-agent | #351 | 为什么这个 cli 这么费 token？ | 用户反馈 CLI token 消耗过大 | token | P1 |
| TA3 | bytedance/trae-agent | #233 | 进程吃满 cpu 100%（疑似挖矿） | 进程 CPU 占满 100% | perf | P1 |
| TA4 | bytedance/trae-agent | #364 | AI agent is operating very slowly | 代理运行极慢 | perf | P1 |
| TA5 | bytedance/trae-agent | #195 | trae 目前有做 memory 压缩么（类似 mem0） | 询问是否做 memory 压缩以省 token | token | P1 |
| TA6 | bytedance/trae-agent | #94 | Steps streaming print | 步骤流式打印（实时反馈） | UX | P2 |
| TA7 | bytedance/trae-agent | #14 | Qwen and Deepseek support | 支持更多模型 provider（常用能力） | UX/common | P2 |

### QwenLM/qwen-code (QW)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| QW1 | QwenLM/qwen-code | #6004 | 安装 MCP 过程中任务异常直接闪退 | macOS 下安装 MCP 时异常闪退 | macOS/stability | P1 |
| QW2 | QwenLM/qwen-code | #3264 | ui.statusLine crashes CLI with spawn EBADF on macOS | macOS 下 statusLine 崩溃（spawn EBADF） | macOS/stability | P1 |
| QW3 | QwenLM/qwen-code | #4815 | Severe OOM with --resume and Escape key broken | --resume 严重 OOM 且 Escape 失效 | perf/token | P1 |
| QW4 | QwenLM/qwen-code | #6265 | tool_search invalidates LLM server KV-cache on every deferred-tool load | 每次延迟加载 tool_search 使 KV-cache 失效 | token/perf | P1 |
| QW5 | QwenLM/qwen-code | #6806 | Status line context usage % does not refresh after /compress | /compress 后状态栏上下文百分比不刷新 | token/UX | P1 |
| QW6 | QwenLM/qwen-code | #7831 | Repeated ECONNRESET on streaming when context exceeds ~150k tokens | 上下文超 150k token 时流式重复 ECONNRESET | perf/token | P1 |
| QW7 | QwenLM/qwen-code | #6097 | System prompt fixed overhead reaches ~22k tokens (0.2% signal) | 系统提示固定开销达 22k tokens，信噪比低 | token | P0 |
| QW8 | QwenLM/qwen-code | #5861 | Context compression request should use stream=true to avoid gateway timeout | 上下文压缩应用 stream=true 避免网关超时 | token/perf | P1 |
| QW9 | QwenLM/qwen-code | #5722 | Token speed display bugs: tok/s disappears during thinking | token 速度显示错误（思考/工具调用时 tok/s 消失） | token/UX | P1 |
| QW10 | QwenLM/qwen-code | #5101 | Qwen Code carries repeated large tool results through provider history | 重复大工具结果穿过 provider 历史，膨胀上下文 | token | P1 |
| QW11 | QwenLM/qwen-code | #4695 | Tool-call loop: no client-side circuit breaker | 工具调用循环无客户端断路器，模型陷入重复 tool_call | token/stability | P1 |

### cline/cline (CL)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| CL1 | cline/cline | #484 | MacOS 15 Shell Integration: Unable to Retrieve Terminal Output | macOS Shell Integration 无法获取终端输出 | macOS/TUI | P1 |
| CL2 | cline/cline | #3445 | Terminal output capture failure in Cline v3.15.0/v3.15.1 | 终端输出捕获失败 | macOS/TUI | P1 |
| CL3 | cline/cline | #4356 | Improve Terminal Integration Reliability Across Platforms and Shell Configs | 跨平台/Shell 终端集成可靠性改进（常用能力） | TUI | P1 |
| CL4 | cline/cline | #1404 | Cline hanging after command execution from any tasks | 命令执行后挂起 | perf/stability | P1 |
| CL5 | cline/cline | #1146 | Executing terminal commands hangs cline | 执行终端命令时挂起 | perf/stability | P1 |
| CL6 | cline/cline | #3501 | MCP server data under iCloud-synced Documents -> system-wide slowdowns on macOS | macOS 下 MCP 数据存 iCloud 文档目录导致系统变慢 | macOS/perf | P1 |
| CL7 | cline/cline | #6878 | Intermittent crash leads to loss of visible task history on Apple M3 | Apple M3 上间歇崩溃丢失任务历史 | macOS/stability | P1 |
| CL8 | cline/cline | #4031 | Very high idle CPU on Code Helper (Plugin) MacOS Sequoia (Apple Silicon) | macOS Sequoia 下插件空闲 CPU 极高 | macOS/perf | P1 |
| CL9 | cline/cline | #9660 | prompt is too long: 228307 tokens > 200000 maximum | 提示过长 228k tokens 超 200k 上限 | token | P0 |
| CL10 | cline/cline | #1452 | Excessive Token Consumption loading config files like node_modules on initial load | 初始加载误载 node_modules 等配置文件致 token 浪费 | token | P1 |
| CL11 | cline/cline | #11181 | memorybank sub-agents cause significant token waste | memorybank 子代理造成显著 token 浪费 | token | P1 |
| CL12 | cline/cline | #9323 | Cline CLI hangs when context window is full - no accessible retry button | 上下文满时 CLI 挂起且无重试入口 | token/stability | P1 |

### Aider-AI/aider (AI)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| AI1 | Aider-AI/aider | #276 | Is it possible to set custom timeout? | 支持自定义超时 | perf/UX | P1 |
| AI2 | Aider-AI/aider | #3021 | Either 'Loading' or infinite wait for any request on Mac | macOS 下无限等待/Loading | macOS/perf | P1 |
| AI3 | Aider-AI/aider | #705 | Sonnet 3.5 is using a lot of output tokens, hitting 4k output token limit | 输出 token 过多触及 4k 上限 | token | P1 |
| AI4 | Aider-AI/aider | #863 | Tokens leak? | token 疑似泄漏 | token | P1 |
| AI5 | Aider-AI/aider | #437 | how to optimize costs ? | 成本优化指导需求 | token | P1 |
| AI6 | Aider-AI/aider | #3196 | 100% cpu freezing does not respond to ctrl c | 100% CPU 冻结，ctrl-c 无响应 | perf/stability | P1 |
| AI7 | Aider-AI/aider | #995 | Tab filename completion causes aider to hang | 文件名 Tab 补全导致 aider 挂起 | TUI/perf | P1 |
| AI8 | Aider-AI/aider | #2010 | local file updated but /read cache not refreshed | 本地文件更新后 /read 缓存未刷新 | token/cache | P1 |
| AI9 | Aider-AI/aider | #5447 | Cache unchanged tracked-file results during startup | 启动时缓存未变文件结果 | token/cache | P1 |
| AI10 | Aider-AI/aider | #3104 | Add busy indicator / spinner / progress bar | 增加忙碌指示/进度条 | UX | P2 |
| AI11 | Aider-AI/aider | #4542 | Is Aider suitable for complex and large-scale projects? (models only 4k tokens by default) | 大模型默认仅 4k token，不适合大型项目 | token | P2 |

### pydantic/pydantic-ai-harness (PH)

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| PH1 | pydantic/pydantic-ai-harness | #40 | Deferred Tool Loading / Tool Search capability | 延迟工具加载/工具搜索，减少每次全量注入 token | token/common | P1 |
| PH2 | pydantic/pydantic-ai-harness | #84 | Adaptive Reasoning Effort (per-step thinking budget selection) | 逐步自适应推理强度，控制 thinking token | token/common | P1 |
| PH3 | pydantic/pydantic-ai-harness | #93 | Git History as Compressed Context | 用 Git 历史作为压缩上下文，降低 token | token/common | P1 |
| PH4 | pydantic/pydantic-ai-harness | #357 | SubAgents: no per-delegation model selection (cheaper/stronger routing) | 子代理无法按任务选更便宜/更强模型路由 | token/common | P1 |
| PH5 | pydantic/pydantic-ai-harness | #18 | CLI agent loading from spec files | 从 spec 文件加载 CLI agent（命令行能力） | TUI/common | P2 |

### 按优先级汇总（P0/P1 推荐实施）

| 优先级 | 编号 | 来源 | Issue | 标题 | 分类 |
|--------|------|------|-------|------|------|
| P0 | CL9 | cline/cline | #9660 | prompt is too long: 228307 tokens > 200000 maximum | token |
| P0 | CR1 | charmbracelet/crush | #1055 | Missing auto-compaction of context; freezes when starting a new session after context exhausted | token/stability |
| P0 | CR7 | charmbracelet/crush | #555 | Unnecessary Input Token Spend - Full Project Context Sent on Every Request | token |
| P0 | QW7 | QwenLM/qwen-code | #6097 | System prompt fixed overhead reaches ~22k tokens (0.2% signal) | token |
| P0 | SP1 | obra/superpowers | #87 | Optimize plan generation: Modular task files + orchestrator for 90%+ token reduction | token |
| P0 | SP4 | obra/superpowers | #1988 | Codex SDD has no circuit breaker: one task ran ~4h, 120.7M telemetry tokens | token/stability |
| P0 | WP1 | warpdotdev/warp | #13295 | Agent runs failing to complete with high credit usage and crash/restart loop | macOS/token |
| P0 | WP10 | warpdotdev/warp | #7804 | Process stuck in repetitive 'summarization loop' failing to make progress | token/stability |
| P0 | WP4 | warpdotdev/warp | #8205 | Huge memory leak on macOS Tahoe 26.1/26.2 (78GB of memory consumed) | macOS/perf |
| P1 | AI1 | Aider-AI/aider | #276 | Is it possible to set custom timeout? | perf/UX |
| P1 | AI2 | Aider-AI/aider | #3021 | Either 'Loading' or infinite wait for any request on Mac | macOS/perf |
| P1 | AI3 | Aider-AI/aider | #705 | Sonnet 3.5 is using a lot of output tokens, hitting 4k output token limit | token |
| P1 | AI4 | Aider-AI/aider | #863 | Tokens leak? | token |
| P1 | AI5 | Aider-AI/aider | #437 | how to optimize costs ? | token |
| P1 | AI6 | Aider-AI/aider | #3196 | 100% cpu freezing does not respond to ctrl c | perf/stability |
| P1 | AI7 | Aider-AI/aider | #995 | Tab filename completion causes aider to hang | TUI/perf |
| P1 | AI8 | Aider-AI/aider | #2010 | local file updated but /read cache not refreshed | token/cache |
| P1 | AI9 | Aider-AI/aider | #5447 | Cache unchanged tracked-file results during startup | token/cache |
| P1 | CL1 | cline/cline | #484 | MacOS 15 Shell Integration: Unable to Retrieve Terminal Output | macOS/TUI |
| P1 | CL10 | cline/cline | #1452 | Excessive Token Consumption loading config files like node_modules on initial load | token |
| P1 | CL11 | cline/cline | #11181 | memorybank sub-agents cause significant token waste | token |
| P1 | CL12 | cline/cline | #9323 | Cline CLI hangs when context window is full - no accessible retry button | token/stability |
| P1 | CL2 | cline/cline | #3445 | Terminal output capture failure in Cline v3.15.0/v3.15.1 | macOS/TUI |
| P1 | CL3 | cline/cline | #4356 | Improve Terminal Integration Reliability Across Platforms and Shell Configs | TUI |
| P1 | CL4 | cline/cline | #1404 | Cline hanging after command execution from any tasks | perf/stability |
| P1 | CL5 | cline/cline | #1146 | Executing terminal commands hangs cline | perf/stability |
| P1 | CL6 | cline/cline | #3501 | MCP server data under iCloud-synced Documents -> system-wide slowdowns on macOS | macOS/perf |
| P1 | CL7 | cline/cline | #6878 | Intermittent crash leads to loss of visible task history on Apple M3 | macOS/stability |
| P1 | CL8 | cline/cline | #4031 | Very high idle CPU on Code Helper (Plugin) MacOS Sequoia (Apple Silicon) | macOS/perf |
| P1 | CR2 | charmbracelet/crush | #337 | macOS panic: runtime error: invalid memory address or nil pointer dereference | macOS/stability |
| P1 | CR3 | charmbracelet/crush | #2918 | High CPU/RAM usage while streaming long thinking traces in assistant message | perf/token |
| P1 | CR4 | charmbracelet/crush | #3167 | Feature: Display real-time token generation speed in status bar | token/UX |
| P1 | CR5 | charmbracelet/crush | #3373 | URLs in Crush output are not clickable (no OSC 8) — Ghostty & Kitty on macOS | macOS/TUI |
| P1 | CR6 | charmbracelet/crush | #993 | Token & Cost count does not function with a custom provider | token |
| P1 | CW1 | Hmbown/CodeWhale | #549 | Interactive TUI hangs on 'working.' at 100% CPU (macOS ARM64, v0.8.7) | macOS/perf |
| P1 | CW3 | Hmbown/CodeWhale | #3143 | Add prompt source map and context-usage report for rules/tools/memory/skills | token/UX |
| P1 | CW4 | Hmbown/CodeWhale | #3906 | perf(tui): render() re-estimates context tokens over ALL api_messages every frame | token/perf |
| P1 | CW5 | Hmbown/CodeWhale | #3190 | feat(tui): surface token throughput during streaming | token/UX |
| P1 | CW6 | Hmbown/CodeWhale | #166 | UI: real-time cost counter during sub-agent work | token/UX |
| P1 | CW7 | Hmbown/CodeWhale | #1120 | 缓存命中方面似乎还是有些问题 (cache hits still problematic) | token |
| P1 | DF1 | bytedance/deer-flow | #3173 | Raise default summarization trigger to avoid frequent compaction in research runs | token |
| P1 | DF2 | bytedance/deer-flow | #3125 | display real-time context window usage percentage in chat UI | token/UX |
| P1 | DF3 | bytedance/deer-flow | #3103 | LLM 400 上下文超限 — summary 不触发、无 input_token 限制入口 | token |
| P1 | DF4 | bytedance/deer-flow | #1400 | lsof -nP -iTCP hangs indefinitely on macOS, blocking server startup | macOS/perf |
| P1 | DF5 | bytedance/deer-flow | #1602 | SummarizationMiddleware fails in streaming (stream_usage off) -> context overflow | token |
| P1 | DR1 | esengine/DeepSeek-Reasonix | #3999 | Mac command+c 双击两次退出终端会话 | macOS/TUI |
| P1 | DR10 | esengine/DeepSeek-Reasonix | #5324 | Scrolling up to view past messages does not work anymore (CLI) | macOS/TUI |
| P1 | DR11 | esengine/DeepSeek-Reasonix | #1122 | macOS M4 stuck on startup | macOS |
| P1 | DR2 | esengine/DeepSeek-Reasonix | #3734 | macOS: codegraph 进程在 Reasonix 退出后残留，导致系统卡顿 | macOS/perf |
| P1 | DR3 | esengine/DeepSeek-Reasonix | #3655 | TUI can be suspended by tty input and leave terminal modes dirty | macOS/TUI |
| P1 | DR4 | esengine/DeepSeek-Reasonix | #5627 | docs say mouse reporting disabled but TUI enables MouseMode, blocking native copy | macOS/TUI |
| P1 | DR5 | esengine/DeepSeek-Reasonix | #4626 | When waiting for confirmation of input, it will hang up and exit | macOS/stability |
| P1 | DR6 | esengine/DeepSeek-Reasonix | #4211 | cli 1.4.0 之后跑一会儿就自动退出 | macOS/stability |
| P1 | DR7 | esengine/DeepSeek-Reasonix | #6387 | v1.17.11 输入框只能显示一行 | macOS/TUI |
| P1 | DR8 | esengine/DeepSeek-Reasonix | #6603 | Mac 使用 reasonix CLI 无法复制粘贴 | macOS/TUI |
| P1 | DR9 | esengine/DeepSeek-Reasonix | #3511 | 多行输入支持 | macOS/TUI |
| P1 | PH1 | pydantic/pydantic-ai-harness | #40 | Deferred Tool Loading / Tool Search capability | token/common |
| P1 | PH2 | pydantic/pydantic-ai-harness | #84 | Adaptive Reasoning Effort (per-step thinking budget selection) | token/common |
| P1 | PH3 | pydantic/pydantic-ai-harness | #93 | Git History as Compressed Context | token/common |
| P1 | PH4 | pydantic/pydantic-ai-harness | #357 | SubAgents: no per-delegation model selection (cheaper/stronger routing) | token/common |
| P1 | PL1 | plandex-ai/plandex | #324 | Suggestion: HADS format for context files — reduces token waste for loaded docs | token |
| P1 | PL2 | plandex-ai/plandex | #89 | Token limit exceeded before adding conversation | token |
| P1 | QW1 | QwenLM/qwen-code | #6004 | 安装 MCP 过程中任务异常直接闪退 | macOS/stability |
| P1 | QW10 | QwenLM/qwen-code | #5101 | Qwen Code carries repeated large tool results through provider history | token |
| P1 | QW11 | QwenLM/qwen-code | #4695 | Tool-call loop: no client-side circuit breaker | token/stability |
| P1 | QW2 | QwenLM/qwen-code | #3264 | ui.statusLine crashes CLI with spawn EBADF on macOS | macOS/stability |
| P1 | QW3 | QwenLM/qwen-code | #4815 | Severe OOM with --resume and Escape key broken | perf/token |
| P1 | QW4 | QwenLM/qwen-code | #6265 | tool_search invalidates LLM server KV-cache on every deferred-tool load | token/perf |
| P1 | QW5 | QwenLM/qwen-code | #6806 | Status line context usage % does not refresh after /compress | token/UX |
| P1 | QW6 | QwenLM/qwen-code | #7831 | Repeated ECONNRESET on streaming when context exceeds ~150k tokens | perf/token |
| P1 | QW8 | QwenLM/qwen-code | #5861 | Context compression request should use stream=true to avoid gateway timeout | token/perf |
| P1 | QW9 | QwenLM/qwen-code | #5722 | Token speed display bugs: tok/s disappears during thinking | token/UX |
| P1 | SP2 | obra/superpowers | #190 | All Skills Preloaded at Startup Consuming 22k+ Tokens (11% of Context) | token |
| P1 | SP3 | obra/superpowers | #832 | Token optimization: 69% line reduction across all 14 skills | token |
| P1 | SP5 | obra/superpowers | #750 | Superpowers consume a lot of tokens in Opencode with Codex | token |
| P1 | SP6 | obra/superpowers | #1940 | Is Superpowers still suitable for the token plan era? cost control difficult | token |
| P1 | TA1 | bytedance/trae-agent | #228 | Trae-cli always hang and never terminate | perf/stability |
| P1 | TA2 | bytedance/trae-agent | #351 | 为什么这个 cli 这么费 token？ | token |
| P1 | TA3 | bytedance/trae-agent | #233 | 进程吃满 cpu 100%（疑似挖矿） | perf |
| P1 | TA4 | bytedance/trae-agent | #364 | AI agent is operating very slowly | perf |
| P1 | TA5 | bytedance/trae-agent | #195 | trae 目前有做 memory 压缩么（类似 mem0） | token |
| P1 | WP11 | warpdotdev/warp | #8405 | Feature request: CLI/API to query Warp AI credit usage programmatically | macOS/token/UX |
| P1 | WP2 | warpdotdev/warp | #7248 | git operations crashes arm macs | macOS/stability |
| P1 | WP3 | warpdotdev/warp | #9037 | High CPU Usage (>100%) and command hangs on macOS Tahoe and Remote SSH | macOS/perf |
| P1 | WP5 | warpdotdev/warp | #6590 | lag and high CPU usage on macOS 26 when scrolling big scrollback | macOS/perf |
| P1 | WP6 | warpdotdev/warp | #9830 | Idle Warp tabs drain GitHub GraphQL API rate limit (~2.4 calls/sec) | macOS/token |
| P1 | WP7 | warpdotdev/warp | #5950 | Warp hangs indefinitely on macOS after Feb 27 update | macOS/stability |
| P1 | WP8 | warpdotdev/warp | #7965 | Terrible performance on M3 Mac | macOS/perf |
| P1 | WP9 | warpdotdev/warp | #13040 | Crash on terminal resize with CJK (wide) characters in the prompt | macOS/TUI |

---

## 六、竞品 GitHub Issues 候选需求补充 (Roo-Code / Continue / Goose)

> 筛选标准：仅限 macOS + TUI 终端环境，专注于 Token 节省、响应速度提升与常用能力增强

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| RC1 | RooCodeInc/Roo-Code | #12111 | [ ] TOON token-efficient context serialization — plugin/CLI token reduction | TOON 格式压缩上下文序列化，有效减少上下文 Token 占用 | Token | P1 |
| RC2 | RooCodeInc/Roo-Code | #12087 | [ ] Context loss on Provider Error causes agent prompt hallucination | Provider 报错后上下文丢失，导致智能体忘掉上一条提示并幻觉之前任务 | Token/稳定性 | P1 |
| RC3 | RooCodeInc/Roo-Code | #12249 | [ ] Too much contextual content occupying too much memory | 长会话超大上下文导致内存膨胀和 TUI 响应卡顿/白屏 | 性能/TUI | P1 |
| RC4 | RooCodeInc/Roo-Code | #12330 | [ ] Support parallel execution of specialized subagents and context handoff | 支持专业子智能体并行执行与跨 Mode 上下文平滑交接 | 常用能力/子智能体 | P1 |
| RC5 | RooCodeInc/Roo-Code | #12268 | [ ] Auto-approve command execution cannot be stopped / interrupted | TUI 中自动审批模式运行命令时无法及时 Esc 打断或停止 | TUI/交互 | P1 |
| CT1 | continuedev/continue | #12980 | [ ] perf: avoid tokenizing lines that pruning discards | 剪枝丢弃的行避免无效分词与上下文构建，降低 CPU 开销与 Token 浪费 | Token/性能 | P1 |
| CT2 | continuedev/continue | #13038 | [ ] Autocomplete / prompt pruneLength negative budget bug | 当设置的上下文与模型  相等时计算负 budget 导致前缀/后缀被清空 | Token/稳定性 | P1 |
| CT3 | continuedev/continue | #13026 | [ ] /model switch notices stored as system messages replayed mid-conversation | 模型切换通知和错误 Banner 被存为 system 消息并在对话中重放，破坏严格 Chat 模板 | Token/上下文 | P1 |
| CT4 | continuedev/continue | #12925 | [ ] userRules / system prompt displayed twice in context | 系统规则在 CLI 上下文中重复注入两次，造成冗余 Token 浪费 | Token | P1 |
| CT5 | continuedev/continue | #13057 | [ ] Local models return tool calls as plain text instead of executing | 局部模型/Ollama 返回明文 tool call 时未容错解析直接暴露给用户 | TUI/工具调用 | P1 |
| GS1 | aaif-goose/goose | #10706 | [x] <turn-context> refreshes every LLM call breaking prompt-prefix caching | <turn-context> 中的时间戳每轮刷新导致 LLM API 服务端 Prompt Cache 命中率跌零 | Token/缓存 | P0 |
| GS2 | aaif-goose/goose | #10764 | [ ] Uncapped conversation replay on session restore makes long sessions unresumable | 会话恢复时无界重放全量历史，导致长会话 Token 爆炸且无法恢复 | Token/性能 | P1 |
| GS3 | aaif-goose/goose | #10763 | [ ] /clear and /compact context only cleared client-side while token counter lies | /clear 和 /compact 只在客户端清理上下文，未同步至智能体且 Token 计数器不准 | Token/一致性 | P1 |
| GS4 | aaif-goose/goose | #10732 | [x] TUI input editing limited by ink-multiline-input: in-tree editing layer | TUI 多行输入框缺乏多行光标移动与粘贴折叠支持 | TUI/输入 | P1 |
| GS5 | aaif-goose/goose | #10642 | [x] Tool-call input/output folded with no verbosity control in TUI (macOS) | TUI 中工具调用的输入输出折叠缺乏详细程度 (verbosity) 控制 | TUI/交互 | P1 |

---

## 七、竞品 GitHub Issues 候选需求补充 (Open Interpreter / Agent Zero)

> 筛选标准：仅限 macOS + TUI 终端环境，专注于 Token 节省、响应速度提升与常用能力增强

| # | 来源 | Issue | 标题 | 描述 | 分类 | 优先级 |
|---|------|-------|------|------|------|--------|
| OI1 | openinterpreter/openinterpreter | #1851 | [x] System confirmation message serialized between tool call and tool result breaking strict API templates | TUI 自动审批/确认消息在 tool_call 与 tool result 之间被序列化，导致严格 Provider 返回 400 重试卡死并浪费 Token | Token/稳定性 | P0 |
| OI2 | openinterpreter/openinterpreter | #1839 | [x] Harness function-style tools emit no TurnItem — invisible in TUI transcript | TUI 界面中函数工具执行缺失 TurnItem 动态反馈，导致用户感知卡顿 | TUI/状态反馈 | P1 |
| OI3 | openinterpreter/openinterpreter | #1812 | [x] Switching providers reuses previous provider model cache | 在 TUI 中切换 Provider 时误复用上个 Provider 的模型缓存，引发请求报错与 Token 浪费 | Token/缓存 | P1 |
| AZ1 | agent0ai/agent-zero | #1762 | [x] Regression: Severe Context Loss makes long-running tasks nearly unusable | 上下文裁剪过猛导致严重上下文丢失，使长程任务智能体丧失前序状态记忆 | Token/一致性 | P0 |
| AZ2 | agent0ai/agent-zero | #1778 | [x] ChatCompletionsTransport drops tool_calls when non-empty content present | 模型同时输出 reasoning content 与 tool_calls 时工具调用被静默丢弃导致零产出 Token 消耗 | Token/工具调用 | P1 |
| AZ3 | agent0ai/agent-zero | #1750 | [x] history.output_text() serializes AI messages as raw JSON, polluting utility context | 历史记录序列化为原始 JSON 污染辅助模型上下文，增加无用 Token 开销 | Token/上下文 | P1 |
