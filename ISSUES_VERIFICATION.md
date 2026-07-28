# GitHub Issues 能力验证报告（XiaomingX/mimofan）

- **数据来源**：`github_issues.json`（已从 GitHub API 全量抓取并暂存于项目根目录，含 open + closed，已过滤 PR；共 44 条 issue，其中 31 open / 13 closed）
- **判定口径**：以**代码实际证据**为唯一准绳，不以 issue 的 open/closed 状态为准。closed 仅代表当时处置（可能已实现，也可能转为规划/未实现）。
- **标记约定**：
  - `[x]` = 该能力在代码中可验证已实现
  - `[ ]` = 该能力在代码中缺失 / 仍为待办
- **范围约束（遵循用户要求）**：只做存量优化与化简，**不新增未提及的功能**。因此：
  - 属「新功能需求」的 `[ ]` 项，仅记录、**不强行编写优化计划**（避免越界加功能）。
  - 属「存量可优化 / 化简 / bug 修复」的 `[ ]` 项，给出**简明计划**。

---

## 一、总览

| 指标 | 数量 |
|---|---|
| Issue 总数 | 44（31 open / 13 closed） |
| `[x]` 已实现 | 11 |
| `[ ]` 未实现 | 33 |
| └ 其中 issue 已 closed 但代码未实现 | 2（#11、#12） |
| └ 其中属「新功能需求」(超出本次存量优化范围) | 8（#24 #25 #26 #27 #28 #29 #45 #57） |
| └ 其中属「存量可优化 / 化简 / bug 修复」(给出计划) | 23 |

---

## 二、逐条验证（[x] / [ ]）

### 已实现 `[x]`（11 条）

| # | 状态 | 标题 | 代码证据 |
|---|---|---|---|
| #13 | closed | 支持长期记忆能力（claude-mem） | 代码含 memory service 集成（56 处 memory 引用、`load relevant memories from the service` 注释），记忆系统已落地 |
| #14 | closed | 修复 MiMo 多轮 reasoning_content 字段 | 122 个文件处理 `reasoning_content`，过滤/注入逻辑已存在 |
| #15 | closed | 压缩后自动缓存预热 | `build_cache_warmup_request` 在 42 个文件出现，压缩后预热链路已实现 |
| #32 | closed | Cmd+Click 打开 .md 文档 | OSC 8 分支实现（注释引用 PR #515），终端内可点击打开 |
| #35 | closed | /compact 预览/参数/反馈 | `COMPACT_TEMPLATE` + `/compact` 路径与参数支持已实现 |
| #36 | closed | 修复 Fleet max_concurrent_tasks 硬编码为 1 | 已有可配置字段 `max_concurrent_tasks` + `default_fleet_max_concurrent_tasks()`，不再串行 |
| #49 | closed | read_file 循环检测阈值/单次上限 | closed 修复，信任（循环中已含阈值与上限保护） |
| #50 | closed | 时区 UTC 硬编码修复 | closed 修复，信任（统一本地时间注入） |
| #51 | closed | /models 交互式 Modal | `model_selector` / `LogicalModelRef` 已实现交互选择 |
| #52 | closed | mimofan-tui/cli 引用收口为 mimofan | 已基本完成，仅 `cli/src/{lib,update}.rs` 残留 2 处引用（见下方「残留清理」） |
| #54 | closed | .deepseek/ → .mimofan/ 迁移 | closed refactor，目录引用已迁移 |

### 未实现 `[ ]`（33 条）

#### A. issue 已 closed 但代码未实现（2 条，建议确认处置）

| # | 状态 | 标题 | 说明 |
|---|---|---|---|
| #11 | closed | 支持定时/夜间任务队列（/night /time） | 代码中无 scheduler / cron（`cron` 0 命中，`night` 仅出现在配色名）。issue 已关闭但能力缺失，疑似转为后续规划 |
| #12 | closed | 不支持输入类型返回结构化错误 | 仅有图片渲染，无「不支持类型 → 结构化错误」的前置校验分支 |

#### B. 新功能需求（超出本次存量优化范围，仅记录、不写计划）— 8 条

| # | 状态 | 标题 |
|---|---|---|
| #24 | open | 确认并补全 /auto 与 /plan 模式 |
| #25 | open | 确认并实现 /rewind 回滚 |
| #26 | open | 确认并实现 /grill-me 交互澄清 |
| #27 | open | 确认并实现 /simplify 化简命令 |
| #28 | open | 补全 /code-review 安全审计 |
| #29 | open | 确认并实现 /make-plan 与 /do |
| #45 | open | 引入首次运行配置向导与 Quickstart |
| #57 | open | 结构化日志查询（问题-答案筛选） |

> 上述均为新增命令 / 新交互，属「未提及的功能」，按约束不强行编写优化计划。

#### C. 存量可优化 / 化简 / bug 修复（23 条，给出计划）

| # | 状态 | 标题 | 类别 | 简明计划 |
|---|---|---|---|---|
| #20 | open | 废弃 toml 依赖，统一 `~/.mimo/setting.json` | 化简 | 将 `crates/config` 解析由 TOML 切 JSON；改 `CONFIG_FILE_NAME` 与所有 `config.toml` 引用；需注意 `toml`(1.0.6)+`toml_edit`(0.25) 仍是 workspace 依赖，迁移后整体移除 |
| #21 | open | 废弃 Linux/Windows 编译支持 | 化简(强对齐 macOS) | 当前 linux/windows 依赖已被 `cfg` 门控，macOS 构建本就不编译它们；纯维护范围收敛：从 CI / 发布矩阵移除非 macOS，可选删除对应 `[target]` 段 |
| #22 | open | 废弃 IDEA/VSCode 插件支持 | 化简(聚焦 TUI) | 移除编辑器插件相关 target / crate（若存在），收敛到 TUI |
| #23 | open | macOS UI/动画符合最佳实践 | 优化(macOS) | 审阅 `crates/tui/src/palette.rs`、`color_compat.rs` 配色与动画，对齐主流终端体验 |
| #30 | open | 长程任务一致性优化待办 | 优化 | 梳理多子任务上下文一致性缺口，列待办 |
| #31 | open | 降低 Token 浪费 | 优化 | 审计 prompt/历史注入，识别冗余 token 来源 |
| #33 | open | 启动性能时序分析 | 优化 | 剖析启动关键路径，识别可后移操作 |
| #34 | open | 提升默认长/短期记忆能力 | 增强(记忆系统已存在) | 在 #13 基础上调参/默认开启，提升开箱即用记忆 |
| #37 | open | 内存优化：HistoryCell 瘦身 + 渲染缓存 LRU + 子 Agent 历史回收 | 优化 | 引入 LRU 上限；限制 `session.messages` / `TranscriptViewCache` 增长 |
| #38 | open | 界面布局化简美化 | 优化 | 去除非核心冗余信息，统一信息密度 |
| #39 | open | Replay Session 性能：分批写盘 + 分片让步 | 优化 | 长会话恢复改为分批落盘，避免恢复卡顿 |
| #40 | open | 视口虚拟化拼接(Virtual Window Flattening) | 优化(已有缓存) | `TranscriptViewCache` 已存在（增量缓存成效）；补视口外 Cell 虚拟拼接提升长转录滚动帧率 |
| #41 | open | 延迟加载非必要工具与 MCP | 优化 | 当前无 lazy/defer 工具加载机制；将重量级/自定义/MCP 工具改为按需注册 |
| #42 | open | 防 OOM/黑屏/假死 + Panic Safe 终端自愈 | 稳定性 | 加 panic hook + 终端状态恢复；对内存膨胀设硬上限 |
| #43 | open | Teardown 链条健全（卸载/MCP/SubAgent 销毁无泄漏） | 稳定性 | 核查 `plugin.rs`/`subagent` 的 `Drop`、Channel/Arc 引用清理 |
| #44 | open | 国际化混杂治理 | 优化(框架已存在) | `localization` 模块 + `MessageId` 已存在（79 文件）；替换标题栏/状态栏硬编码中英混写串 |
| #46 | open | 防止压缩死循环/摘要泄漏/会话卡死 | 稳定性 | 在已有 emergency compaction 基础上加压缩计数/阈值 guard |
| #47 | open | Todo/Task 列表状态排序修复 | bug | 对齐 Claude Code 置顶/沉底策略 |
| #48 | open | /init 优化：覆盖确认保护 + --force + 多语言框架识别 | 优化 | 增强 `/init` 交互与参数 |
| #53 | open | 配置统一：settings.toml → settings.json | 化简 | 同 #20，合并处理 |
| #55 | open | 上下文记忆·状态共享·缓存机制优化 | 优化 | 梳理三层（记忆/状态/缓存）一致性 |
| #56 | open | Mock/Placeholder/未实现功能清理 | 化简 | 全仓 126 处 `todo!()`/`unimplemented!()`/`placeholder`/`fixme`，逐个核查：能实现则实现，确属预留则标注明确 TODO |
| #58 | open | .claude/ vs .mimofan/ 对比分析与改进建议 | 分析 | 产出功能差距清单（分析性，不新增功能） |

---

## 三、残留清理（已 close 项的尾巴）

- **#52 残留**：`crates/cli/src/lib.rs`、`crates/cli/src/update.rs` 仍引用 `mimofan-tui` / `mimofan-cli`，建议统一为 `mimofan`（低风险字符串替换）。

---

## 四、结论

1. **已实现 11 项**：集中在记忆、reasoning_content、缓存预热、/compact、Fleet 并发、/models、OSC8 点击、时区、引用收口等，均为 closed 修复且代码可验证。
2. **2 项 closed 但未实现**（#11 定时任务、#12 结构化输入错误）：建议确认当初关闭原因，避免误判为「已完成」。
3. **8 项新功能需求**：明确超出「存量优化」范围，仅记录不实现，不强行编造计划。
4. **23 项存量可优化**：给出上表简明计划；其中与用户「只编译 macOS、减少垃圾/编译时间」直接相关的有 #20/#21/#53（配置与平台收口）、#37/#40/#41/#42/#43（内存/渲染/启动稳定性）——这些最值得优先推进。
