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
