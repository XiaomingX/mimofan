# mimofan vs qwen-code 能力对标

- **mimofan**：Rust 终端 AI 编码助手，约 24 万行，主体在 `crates/tui/`。
- **qwen-code**：TypeScript，Gemini CLI 血统的终端 code agent，主体在 `packages/core/`、`packages/cli/`。形态与 mimofan 最接近，参考价值最高。

> 方法论说明：本文中每一条判为「缺失」的能力，均已用 Grep 在 mimofan 代码库检索过对应符号/别名后才写入。多条初判为「缺失」的能力在验证后被**推翻**（如 read-before-edit、@ 文件引用、自定义斜杠命令、非交互模式、MCP resources/prompts、分层上下文），这些已改判为「已具备」并在文中标注，避免重复踩坑。

---

## 总览表

| 能力域 | mimofan | qwen-code | 结论 |
|---|---|---|---|
| 文件读写工具（read/write/edit/patch） | 完备，含行锚点、PDF 分页、模糊匹配 | 完备，含 PDF 分页、replace_all | 各有胜场 |
| read-before-edit + 陈旧检测 | 已具备（`spec.rs:498`） | 已具备（FileReadCache） | 持平 |
| 结构化工具错误码 | 10 类粗粒度（`error_taxonomy/`） | 53 个细粒度 `ToolErrorType` | **mimofan 弱** |
| 批量替换语义（replace_all / 期望次数） | 无 | `replace_all` 参数 | **mimofan 缺** |
| Shell 工具（后台/超时/进程组） | 完备（2033 行） | 完备（5171 行） | 持平，qwen 更细 |
| 循环/重复检测 | **已移除**（`engine_config.rs:200`） | `loopDetectionService.ts` 多维检测 | **mimofan 缺（最高优先级）** |
| 上下文压缩 | 三范式 + 溢出恢复 | 单一 compress | mimofan 强 |
| Token 计数 | ~4 字符/token 启发式 | 服务端 `countTokens` API | 双方都无本地 tokenizer |
| 生命周期 Hook | 13 变体 + EventFrame sink | `core/hooks/` | 持平 |
| 任务/依赖图 | `task_manager/`、`tools/tasks.rs` | `agents/tasks/`、task-* 工具 | 持平 |
| 子智能体 | `tools/subagent/`（mailbox/bus/decomposer） | `subagents/`、`agents/team/` | 持平 |
| 工具延迟加载 | `tool_catalog.rs`（BM25） | `tool-search.ts` | 持平 |
| 记忆系统 | `crates/memory/` + remember 工具 | `core/memory/`（dream/forget/团队记忆） | qwen 更丰富 |
| @ 文件引用 | 已具备（`tui/file_mention.rs`） | `atFileProcessor.ts` | 持平 |
| 自定义斜杠命令 | `.md` + frontmatter（`user_commands.rs`） | `.toml` + `.md`，支持 `!{}` shell 注入 | **mimofan 缺 shell 注入** |
| 分层项目上下文 | `project_context/`（1198 行，含 monorepo） | `memoryDiscovery.ts` + `@import` | **mimofan 缺 @import** |
| MCP（tools/resources/prompts） | 已具备（`mcp.rs`） | 完备 + 连接池/重试/预算 | qwen 更工程化 |
| 认证方式 | API key + keyring + MCP OAuth | 多 AuthType（含 QWEN_OAUTH 设备码） | **mimofan 缺模型侧 OAuth** |
| OpenTelemetry 遥测 | 无（仅本地 audit.log + metrics） | 完整 OTLP（span/metrics/logs） | **mimofan 缺** |
| IDE 伴随模式 | 无 | `core/ide/` + VSCode/Zed 扩展 | **mimofan 缺** |
| 扩展系统 / 市场 | 无（仅 `plugin.rs`） | `core/extension/`（市场/npm/github/zip） | **mimofan 缺** |
| 非交互脚本模式 | 已具备（`cli/exec_agent.rs`） | `--prompt` / stream-json | 持平 |
| Vision / 多模态 | `vision/` + `image_ocr.rs` | image-gen / zoom-image / display-image | **mimofan 缺图像生成与缩放** |
| Notebook (.ipynb) 编辑 | 无 | `notebook-edit.ts`（972 行） | **mimofan 缺** |
| 定时 / 周期任务 | `/loop` 自治循环 + `/night` 一次性定时 | cron 工具族 + `/loop` 墙钟调度 | mimofan 缺 cron 式周期调度 |
| 计划模式 | `exit_plan_mode`（`tools/plan.rs`） | enterPlanMode/exitPlanMode | 持平 |
| 向用户提问 | `request_user_input`（1-3 题） | `askUserQuestion`（1-4 题） | 持平 |
| 编辑前人工修改（外部编辑器） | 无 | `modifiable-tool.ts` | **mimofan 缺** |
| 编码 / BOM / CRLF 保真 | 无（直接 read_to_string + write） | `encoding`/`bom`/`lineEnding` 三元保真 | **mimofan 缺（正确性）** |
| 读后 TOCTOU 复检 | 仅编辑前查一次 | 读入后二次 `checkPriorRead` | **mimofan 缺（正确性）** |
| ignore 文件约束工具 | `.mimoignore` 仅作用于工作集遍历 | `.qwenignore` 在工具参数校验层拦截 | **mimofan 缺（语义不符预期）** |
| shell 内 `sed -i` 拦截 | 仅提示文案，无拦截 | 模拟 + diff 确认流程 | **mimofan 缺** |
| 密钥写入扫描 | `crates/secrets/` 存在但未接入工具 | 扫描替换后全文 | **mimofan 未接线** |

---

## 一、文件工具与编辑健壮性

- [x] **read-before-edit 强制 + 文件陈旧检测**：mimofan 在 `crates/tui/src/tools/spec.rs:498 require_fresh_file_read` 中实现，未读拒绝 + 快照比对（`current != prior` 判定 changed since read），错误信息带 Recovery 指引。**此前易被误判为缺失**，实际与 qwen-code 的 `EDIT_REQUIRES_PRIOR_READ` / `FILE_CHANGED_SINCE_READ` 等价。
- [x] **read_file 分页 + PDF 页范围**：`crates/tui/src/tools/file.rs:99-122`，参数 `path` / `start_line` / `max_lines` / `pages`，默认 200 行、硬上限 1500 行、16KB 可见字节封顶，截断时返回 `next_start_line` 续读提示。对齐 qwen-code `read-file.ts` 的 `offset` / `limit` / `pages`。
- [x] **行内容哈希锚点（mimofan 独有优势）**：`read_file` 每行输出 6 字符内容哈希（`     1│a1b2c3│ line`），`edit_file` 可用 `line_ref` 直接定位替换，无需重打整行。qwen-code 无对应机制，这是 mimofan 在 token 效率上的净胜项。
- [x] **编辑模糊回退链（mimofan 独有优势）**：`file.rs:786-820` 精确匹配失败后依次尝试 ① 缩进容差 `leading_whitespace_fuzzy_matches` ② 排版标点归一化 `punctuation_normalized_matches`（智能引号/长破折号/NBSP）。qwen-code 只有精确匹配 + LLM 纠错，无本地确定性回退。
- [ ] **`replace_all` 批量替换语义**：qwen-code `packages/core/src/tools/edit.ts:101` 提供 `replace_all?: boolean`，未开启时命中多处会以 `Failed to edit. Found N occurrences for old_string ... but replace_all was not enabled.` 明确报错。mimofan 已 grep 确认无 `replace_all` / `expected_replacements` / `expected_occurrences` 任何符号。**价值**：当前 mimofan 遇到多处命中只能报「非唯一」让模型重试，重命名类改动必须逐处编辑，回合数显著增加。
- [ ] **空 `old_string` 表示创建新文件**：qwen-code `edit.ts:279` 对不存在的文件提示 `Use an empty old_string to create a new file`，让 edit 与 create 走同一工具。mimofan `edit_file` 要求 `search` 非空（`file.rs:721`），创建必须切到 `write_file`。**价值**：减少模型的工具选择分叉。
- [ ] **Notebook（.ipynb）单元格编辑**：qwen-code `notebook-edit.ts`（972 行）+ 专属错误码 `NOTEBOOK_INVALID_JSON` / `NOTEBOOK_CELL_NOT_FOUND`。mimofan 全库 grep `ipynb|notebook` 无任何命中。**价值**：数据科学仓库中 .ipynb 是 JSON，用通用 edit 极易破坏结构。
- [ ] **编辑前交给用户外部编辑器修改**：qwen-code `modifiable-tool.ts` + `EditToolParams.modified_by_user` / `ai_proposed_content`（`edit.ts:107-113`），用户可在确认弹窗里改 diff，改动会回传模型。mimofan grep `modifiable_tool|ModifiableTool|modify_with_editor` 无命中。**价值**：把「拒绝→重述→重试」压成一次人工微调。

---

## 二、Agent 循环控制（最关键差距）

- [ ] **循环 / 重复 / 停滞检测**：**这是本次对标发现的最严重差距**。
  - mimofan 证据：`crates/tui/src/core/engine/engine_config.rs:198-204` 注释明写 —— *"the in-turn loop_guard that used to brake repetition is gone, so this only exists to terminate a pathological runaway turn via `at_max_steps()`"*，`max_steps: 1000` 被明确描述为「high backstop rather than a working ceiling」。全库 grep `loop_detect|LoopDetect|loop_detection|repetition|repeated_tool|same_tool_call|oscillat` 均无有效实现。唯一相近的是 `turn_loop.rs:2755` 的 anti-drift 断路器，但它只看「连续多轮无文件改动」，且仅作用于 goal 模式。
  - qwen-code 落点：`packages/core/src/services/loopDetectionService.ts`，同时覆盖 **6 个维度**：① 同名同参连续重复 `toolCallRepetitionCount` ② 流式内容句子级重复 `contentStats`（且 `inCodeBlock` 时豁免）③ 同名不同参的「参数抖动」停滞 `sameNameStreak` ④ shell 巡检类命令停滞 `shellInspectionStreak` ⑤ 跨回合全局 (tool,args) 去重计数 `globalToolCallCounts` + `GLOBAL_DUPLICATE_THRESHOLD` ⑥ ABABAB 交替模式 `recentToolCallKeys`。另有 READ_FILE_LOOP 的冷启动豁免（`hasSeenNonReadTool`，开局密集读文件不算循环）与自适应软/硬上限 `checkTurnToolCallCap`。
  - **价值**：mimofan 目前唯一的兜底是 1000 步硬上限，意味着一个陷入 ABAB 循环的回合会烧掉上千次工具调用和大量 token 才停下。这是长程任务里最直接的成本与体验杀手，且 qwen-code 的实现是纯启发式、无需模型参与，移植成本低。

---

## 三、错误处理与可诊断性

- [x] **统一错误信封**：mimofan `crates/tui/src/error_taxonomy/mod.rs` 提供 `ErrorCategory`（Network/Authentication/Authorization/RateLimit/Timeout/InvalidInput/Parse/Tool/State/Internal 共 10 类）+ `ErrorSeverity` + `recoverable` 标志 + `code`/`message`。
- [x] **错误信息内嵌 Recovery 指引（mimofan 独有优势）**：mimofan 的工具错误几乎都带明确下一步，例如 `file.rs:747` 锚点失效时提示 "Recovery: call read_file to get fresh anchors"、`file.rs:805` 搜索未命中时提示带上具体 `path=` 的重试命令。qwen-code 的 `raw` 信息也有引导，但覆盖面不如 mimofan 一致。
- [ ] **细粒度工具错误码枚举**：qwen-code `packages/core/src/tools/tool-error.ts` 定义 53 个 `ToolErrorType`，按子系统分组（File System / Edit / Notebook / Glob / Grep / Ls / MCP / Memory / Shell / WebFetch…），并区分了 `EDIT_REQUIRES_PRIOR_READ`、`FILE_CHANGED_SINCE_READ`、`PRIOR_READ_VERIFICATION_FAILED`（stat 失败无法判定，区别于「确定没读」）、`TARGET_NOT_REGULAR_FILE`（FIFO/socket/设备文件，重读也没用，避免读-编辑死循环）等极细的边界情形。mimofan 的 10 类分类无法表达这些区分，工具侧只能靠 `execution_failed` + 自由文本。**价值**：① 让引擎能按错误码做差异化重试策略（可重试 vs 结构性死路）② 遥测/告警可按码路由 ③ 避免模型对「重读能解决」和「重读没用」的情形采取同一策略而空转。
- [ ] **`EDIT_NO_CHANGE` 类语义化零改动判定**：qwen-code 区分 `EDIT_NO_CHANGE` 与 `EDIT_NO_CHANGE_LLM_JUDGEMENT`。mimofan 在 `file.rs:779` 只对 `search == replace` 做了字面相等检查。**价值**：捕捉「改了但等价」的空转编辑。

---

## 四、上下文、Token 与压缩

- [x] **三套压缩范式并存**：`crates/tui/src/compaction/`（摘要式）、`crates/tui/src/seam_manager/`（接缝管理）、`crates/tui/src/purge/`（清除式），加上 `core/engine.rs recover_context_overflow` 的溢出恢复。qwen-code 只有单一 compress 路径。**mimofan 明显更强**。
- [x] **上下文预算与报告**：`context_budget/`、`context_report/`、`prompt_zones/`、`route_budget/`、`resource_telemetry/`。
- [x] **分层项目上下文**：`crates/tui/src/project_context/mod.rs`（1198 行），优先级 `AGENTS.md` > `CLAUDE.md` > `*/instructions.md`，支持全局 `~/.mimofan/AGENTS.md` 与厂商中立 `~/.agents/AGENTS.md`，并在 `:706` 明确支持 monorepo 场景下根 AGENTS.md 适用于所有子目录。**此前易被误判为缺失**（只 grep `project_doc/` 会看到一个仅 19 行的 git-root 工具模块，真正实现在 `project_context/`）。
- [ ] **上下文文件的 `@import` 递归导入**：qwen-code `packages/core/src/utils/memoryImportProcessor.ts` 支持在 QWEN.md 内用 `@path/to/file.md` 递归引入其他文档，带循环检测与深度上限（`:277` 处理 limit 边界）。mimofan `project_context/mod.rs` grep `import` 无命中，只能靠单文件平铺。**价值**：大型仓库可把上下文拆成「总纲 + 各模块细则」，避免单个 AGENTS.md 无限膨胀。
- [ ] **真实 tokenizer**：mimofan `crates/tui/src/compaction/mod.rs:576-598` 的 `estimate_tokens_for_message` 全部用 `text.len() / 4` 启发式；Cargo.toml grep `tiktoken|tokenizers` 无任何依赖。需要说明的是 **qwen-code 同样没有本地 tokenizer**，它走服务端 `countTokens` API（`contentGenerator.countTokens`）拿精确值。**价值**：`len()/4` 对中文（UTF-8 每字 3 字节）会严重高估、对代码会低估，直接影响压缩触发时机与预算判断。可选路径是接 `tiktoken-rs`，或像 qwen-code 一样优先采信 API 返回的 usage（mimofan `models/mod.rs:215` 已有 `input_tokens` 字段，可作为校准锚点）。

---

## 五、命令、扩展与集成

- [x] **自定义斜杠命令**：mimofan `crates/tui/src/commands/user_commands.rs`，扫描 `~/.mimofan/commands/<name>.md` 与 `<workspace>/.mimofan/commands/<name>.md`，支持 YAML frontmatter（`description` / `argument-hint` / `allowed-tools` / `pausable`）与 `$1` / `$2` / `$ARGUMENTS` 占位符替换（`:157`）。**此前易被误判为缺失**（mimofan 用 `.md`，qwen-code 用 `.toml`，grep `.toml` 会落空）。qwen-code 两种格式都支持并提供 `command-migration-tool.ts` 做 TOML→MD 迁移。
- [x] **@ 文件引用**：mimofan `crates/tui/src/tui/file_mention.rs`，含 Tab 补全（前缀→子串排序）与发送前展开（每条消息最多 8 个 mention、单文件 128KB、目录列举 80 条上限）。对齐 qwen-code `atFileProcessor.ts`。
- [x] **MCP 完整三件套**：`crates/tui/src/mcp.rs` 已实现 `resources/list`（:538）、`resources/read`（:687）、`prompts/list`（:630）、`prompts/get`（:704），另有 `mcp/oauth.rs`、`mcp/transport.rs`、`mcp/headers.rs` 与 `mcp_server/`（对外提供服务）。**此前易被误判为缺失**。
- [x] **非交互 / 脚本模式**：`crates/tui/src/cli/exec_agent.rs` 提供 `run_exec_agent` / `run_one_shot` / `run_one_shot_json`，以及 `ExecOutputFormat`、stdin 读取、`mimofan apply` 打补丁子命令。
- [ ] **斜杠命令内的 `!{...}` shell 注入**：qwen-code `packages/cli/src/services/prompt-processors/shellProcessor.ts` 允许命令模板内嵌 `!{git diff --staged}` 这类子命令，执行结果内联进 prompt；且做了两级转义 —— `{{args}}` 在 `!{}` **外**用原文替换、在 `!{}` **内**用 `escapeShellArg` 转义（`:51-52`, `:101-102`），并在安全配置未加载时直接 abort（`:83`）。mimofan `user_commands.rs` / `user_registry.rs` grep `!{|shell_inject|run_shell` 无命中。**价值**：这是「/review 自动带上当前 diff」这类高频命令的关键能力，且 qwen-code 的转义设计可直接借鉴以避免命令注入。
- [ ] **扩展系统与市场**：qwen-code `packages/core/src/extension/`（约 30 个模块）支持从 npm / GitHub / zip 安装扩展，含 `marketplace.ts`、`archive-safety.ts`（zip slip 防护）、`network-policy.ts`、`redaction.ts`、`variables.ts` 变量插值、`claude-converter.ts` / `gemini-converter.ts` 跨生态格式转换。mimofan grep `marketplace|extension_manager|ExtensionManager` 仅命中一个技能文档和 `version_check.rs`，`tools/plugin.rs` 是运行期工具插件而非分发体系。**价值**：决定生态能否由社区扩张，而非全部内置。
- [ ] **IDE 伴随模式**：qwen-code `packages/core/src/ide/`（`detect-ide.ts` / `ide-client.ts` / `ide-installer.ts` / `ideContext.ts`）+ 独立的 `packages/vscode-ide-companion`、`packages/zed-extension`，可感知 IDE 当前打开文件与选区。mimofan grep `ide_client|IdeClient|ide_companion|detect_ide` 零命中；已有的 `acp_server/` 与 `lsp/` 解决的是另一层问题（Agent 协议与语言服务），不等价。**价值**：让 CLI agent 知道用户「正在看哪一段代码」，大幅减少定位类对话。
- [x] **自治循环与一次性定时**：mimofan 已有 `/loop`（`commands/groups/core/loop_cmd.rs`，复用 `/goal` 续跑管线，由模型自判停止条件 + 显式轮数上限）与 `/night`（`commands/groups/core/schedule.rs`，`--schedule HH:MM` 在指定时刻发送一次 prompt，含 `/time list|cancel`）。**注意不要误判为完全缺失定时能力。**
- [ ] **cron 式周期调度（工具化）**：qwen-code 把调度做成了**模型可调用的工具** —— `cron-create.ts` / `cron-list.ts` / `cron-delete.ts` + `loop-wakeup.ts`，`/loop 5m <prompt>` 会解析间隔并注册 cron 作业，还支持 `/loop 20m /review-pr 1234`（周期性执行另一个斜杠命令）。mimofan grep `cron|Cron` 全库零命中：`/night` 只能定时**一次**、`/loop` 是回合驱动而非墙钟驱动，两者都无法表达「每 30 分钟轮询一次」。**价值**：无人值守的周期巡检 / CI 轮询 / PR 看护场景。属于窄场景，优先级低。

---

## 六、遥测与可观测性

- [x] **本地审计与用量统计**：mimofan `crates/tui/src/audit/`（`~/.mimofan/audit.log`，每事件一行 JSON，记录审批与凭证事件）、`crates/tui/src/cli_commands/metrics.rs`（`mimofan metrics` 聚合 audit.log + sessions/ + tasks/runtime/events/ 输出用量汇总）、`resource_telemetry/`（token/时长/预算压力三档）、`cost_status/`、`pricing/`。
- [ ] **OpenTelemetry / OTLP 导出**：qwen-code `packages/core/src/telemetry/` 有约 40 个模块，覆盖 OTLP span/metrics/logs 导出（`otlp-urls.ts`、`file-exporters.ts`、`log-to-span-processor.ts`）、GenAI 语义约定（`gen-ai-request.ts` / `gen-ai-usage.ts` / `gen-ai-content.ts`）、事件循环延迟指标（`event-loop-lag-metrics.ts`）、资源属性与脱敏（`resource-attributes.ts` / `sanitize.ts`）。mimofan Cargo.toml grep `opentelemetry|tracing-opentelemetry` 无任何依赖。**价值**：团队/企业部署时无法接入现有可观测性栈，只能靠本地日志人工排查。

---

## 七、认证、安全与沙箱

- [x] **凭证安全存储**：mimofan `crates/tui/src/config/credential.rs`，同时写 OS keyring 与配置文件以规避 `keyring → env → config` 解析顺序造成的陈旧条目遮蔽（issue #593），配置文件走 `write_config_file_secure`，并记录 `log_sensitive_event` 审计。
- [x] **沙箱**：`crates/tui/src/sandbox/`（seatbelt / opensandbox）、`execpolicy/`、`command_safety/`、`network_policy/`、`workspace_trust/`。
- [x] **MCP OAuth**：`crates/tui/src/mcp/oauth.rs`（`OAuthState::Authorized` / `Unauthorized`）。
- [ ] **模型侧多认证方式（含设备码 OAuth）**：qwen-code `packages/core/src/core/contentGenerator.ts:56 enum AuthType` 支持 `QWEN_OAUTH`、`USE_ANTHROPIC` 等多种模式，配套 `packages/core/src/qwen/qwenOAuth2.ts` 与 `sharedTokenManager.ts`（多进程共享 token 与自动刷新）。mimofan 模型侧只有 API key 路径，`credential.rs` grep `enum AuthType|OAuth|device_code` 零命中。**价值**：免 API key 的登录式接入门槛更低，且共享 token 管理能避免多实例并发刷新冲突。

---

## 八、多模态

- [x] **视觉输入与 OCR**：mimofan `crates/tui/src/vision/`（`mod.rs` + `tools.rs`）、`tools/image_ocr.rs`、`tools/pandoc.rs`、`tools/speech.rs`，`read_file` 也会对截图做本地 OCR 提取。
- [ ] **图像生成 / 缩放 / 展示工具**：qwen-code 有 `image-gen.ts`、`zoom-image.ts`（含 sharp 失败降级测试）、`display-image.ts`。mimofan grep `image_gen|zoom_image|ImageGen` 零命中。**价值**：`zoom_image` 尤其实用 —— 让模型对长截图的局部区域放大再看一次，是 UI 调试类任务的关键动作。

---

## 九、工具级细节差异

这一节聚焦「参数设计、错误处理、边界用例」这类不改变能力清单、但直接决定实际效果的差异。

### 9.1 `edit_file` / `edit` 参数对照

| 维度 | mimofan `edit_file` (`tools/file.rs:650`) | qwen-code `edit` (`tools/edit.ts`) |
|---|---|---|
| 定位方式 | `line_ref`（内容哈希锚点）**或** `search` | 仅 `old_string` |
| 必填 | `path` + `replace`，`anyOf` 约束 line_ref/search | `file_path` + `old_string` + `new_string` |
| 多处命中 | 报错「非唯一」，无批量选项 | `replace_all: true` 可全替 |
| 未命中回退 | 缩进容差 → 标点归一化，两级本地模糊 | 无本地回退，直接报错 |
| 创建新文件 | 不支持，须用 `write_file` | `old_string: ""` 即创建 |
| 换行边界处理 | 锚点模式显式处理原行有无尾随 `\n`（`file.rs:756-763`） | 未见等价处理 |
| 无改动检测 | `search == replace` 字面相等 | `EDIT_NO_CHANGE` + LLM 判定两级 |
| 人工介入 | 无 | `modified_by_user` / `ai_proposed_content` |
| 密钥泄漏扫描 | 未见 | 扫描替换后**全文**而非仅 new_string（`edit.ts:344` 注释说明这是为防止密钥被拆分绕过） |

要点：mimofan 在**定位精度与容错**上更强（锚点 + 双重模糊回退是净胜项），qwen-code 在**批量语义与人机协同**上更强。两者可以互补，不冲突。

### 9.2 `read_file` / `read-file` 参数对照

| 维度 | mimofan | qwen-code |
|---|---|---|
| 分页参数 | `start_line`（1-based）/ `max_lines` | `offset`（0-based）/ `limit` |
| 默认窗口 | 200 行，16KB 可见字节 | 由配置决定 |
| 硬上限 | 1500 行（`HARD_MAX_READ_LINES`） | 有上限 |
| 越界行为 | `start_line > total_lines` **不报错**，返回 `[NO CONTENT]` 说明（`file.rs:211-215`） | 报错 |
| 续读提示 | 返回 `truncated="true"` + `next_start_line` + 完整重试命令 | 返回截断标记 |
| PDF | `pages="1-5"`，纯 Rust 提取器，无需 Poppler | `pages="1-5"`，明确不支持 `"3-"` 开放区间 |
| 图片 | 本地 OCR 提取 | 作为结构化多模态值返回 |
| 「完整读取」标记 | 记录快照供 edit 校验 | 仅当无 offset/limit/pages 时才标记为 full read（`read-file.ts:154-160`），部分读取不足以授权 edit |

要点：mimofan 的越界不报错 + 自带重试命令的截断提示，对模型更友好。但 qwen-code 有一个 mimofan 没有的关键约束 —— **部分读取（ranged read）不算「读过」，不足以授权后续编辑**。mimofan 的 `require_fresh_file_read` 只校验「读过且未变更」，不区分是全量读还是只读了 200 行的窗口，这意味着模型读了文件开头 200 行就可以去编辑第 800 行的内容，属于真实的正确性隐患。

### 9.3 其他工具细节

- **grep 的默认权限**：qwen-code `grep.ts:90 getDefaultPermission()` 在未指定 `path` 时直接返回 `'allow'`（默认工作区内搜索无需审批），指定路径才校验。mimofan `tools/search.rs` 走统一审批路径，未见按参数分级的默认权限。这类「按参数决定是否需要审批」的粒度能显著减少打断。
- **shell 的后台化引导**：qwen-code `shell.ts:1129-1154` 在达到「有效超时的一半」时向模型发出长时运行警告，并在临近超时时提示 `press Ctrl+B to keep it running in the background`；还会剥离命令尾部裸 `&`（`:106`）、拒绝管道/子 shell 中的后台化（`:1229`）、对超过 10 分钟的前台 sleep 直接拒绝并引导用 `is_background`（`:1302`）。mimofan `tools/shell.rs` 有完整的后台作业追踪与进程组 kill（`kill_child_process_group` 用 `libc::kill(-pgid, SIGKILL)`，比 qwen-code 单纯 kill 子进程更彻底），但缺少这套**主动引导模型正确使用后台模式**的提示层。
- **askUserQuestion 题量**：qwen-code 允许 1–4 题、每题带选项 label + description；mimofan `tools/user_input.rs:52` 限制 1–3 题、选项 2–4 个，校验更严格且错误信息清晰。这是 mimofan 略优的一处。
- **工具参数修复**：mimofan 有 `tools/arg_repair.rs`、`schema_sanitize.rs`、`schema_canonicalize.rs` 三层参数纠错，qwen-code 无等价物。这是 mimofan 应对弱模型输出的独有优势。
- **文件编码 / BOM / 换行风格保真**：qwen-code `edit.ts:116-128` 的 `CalculatedEdit` 显式携带 `encoding`（如 gbk）、`bom`、`lineEnding`（`detectLineEnding`），读入时把 CRLF 归一为 LF 处理、写回时按原样还原。mimofan 已 grep 确认 `crates/tui/src/tools/` 下无 `BOM|feff|encoding` 命中，且无 `\r\n` 处理，`file.rs` 直接 `read_to_string` + `fs::write`。**后果**：编辑带 BOM 的文件会丢 BOM、编辑 CRLF 文件会把改动行悄悄变成 LF，在 Windows 仓库或 .NET 项目里会产生大量伪 diff。属于低成本可修的正确性问题。
- **编辑的读后二次 TOCTOU 复检**：qwen-code `edit.ts:222-262` 在「预检查已读」与「实际读入内容」之间再跑一次 `checkPriorRead(..., { expectExisting: true })`，因为这是两次独立 syscall，中间被改写就会让编辑作用在模型没见过的字节上；命中时记 `debugLogger.warn('post-read TOCTOU rejection')` 留取证痕迹。mimofan `require_fresh_file_read` 只在编辑前查一次（`spec.rs:498`），之后才 `read_to_string`，同样的窗口是敞开的。**价值**：并发写入场景下的静默错误编辑，排查成本极高。
- **ignore 文件对工具的约束力**：qwen-code `read-file.ts:633-636` 在参数校验阶段就调用 `fileService.shouldQwenIgnoreFile()`，被 `.qwenignore` 命中的路径直接拒读并说明是哪条 pattern。mimofan 的 `.mimoignore` 经 grep 确认**只出现在 `working_set/mod.rs`**（影响工作集遍历），read/edit/write 工具不校验，模型仍可直接读写被忽略的文件。**价值**：`.mimoignore` 目前给不了「敏感目录不许碰」的保证，语义与用户预期不符。
- **shell 内 `sed -i` 的拦截与 diff 预览**：qwen-code `shell.ts` 有 `PreparedSedEdit` / `SedEditSimulationError`，识别出 `sed -i` 类原地改写后先模拟出新内容、走和 edit 一样的 diff 确认流程。mimofan grep `sed -i|sed_edit|SedEdit` 仅在 `file.rs` 的工具描述文案里命中（提示模型别用 sed），无实际拦截。**价值**：绕过 edit 审批的「影子写入」路径。
- **shell 的 cwd 语义追踪与 git 归因**：qwen-code `shell.ts` 解析 `git -C` / `env -C` / `sudo -D` / `cd` 来判断命令真实工作目录与是否切换了 repo（`parseGitInvocation`、`cdTargetMayChangeRepo`、`GIT_GLOBAL_FLAGS_SHIFTS_CWD`），并对 `git commit` / `gh pr create` 做提交归因（`CommitAttributionService`、`findAttributableCommitSegment`、`isAmendCommit`）。mimofan grep `Co-authored-by|CommitAttribution` 零命中。**价值**：前者关乎沙箱/工作区边界判定的准确性，后者是可选的署名策略。
- **`web_fetch` 的 `prompt` 二次加工**：qwen-code `web-fetch.ts:759` 把 `prompt` 设为**必填**，抓取后先用模型按 prompt 提炼再返回，避免整页 HTML 灌进上下文。mimofan `fetch_url` grep `"prompt"|summarize` 无命中，靠 `max_bytes`（默认 1MB）+ `fields` JSONPath 投影控量。两种思路各有取舍：mimofan 对结构化 API 更省（JSONPath 是净胜项），qwen-code 对长网页更省。
- **密钥写入扫描**：qwen-code `edit.ts:344` 对团队记忆文件扫描**替换后的全文**（而非仅 new_string），理由是密钥可能被拆成多次编辑逐段写入；若密钥已在磁盘原文中，还会额外提示「光从本次编辑里删掉不够」。mimofan 有独立的 `crates/secrets/` crate，但 grep 确认它**未接入** `crates/tui/src/tools/` 下的任何写入路径。**价值**：现成能力没接线，接入成本低。

---

## 十、建议优先补齐的 Top 10

按「对实际效果影响 ÷ 实现成本」排序。其中 1–3、9 属于**正确性问题**（不是能力缺失），建议最先处理。

1. **循环 / 重复 / 停滞检测**（第二章）—— 唯一被标记为「最严重」的差距。mimofan 的 loop_guard 是被主动移除的，现在只剩 `max_steps: 1000` 兜底。建议先移植 qwen-code 最见效的三个维度：同名同参连续重复、ABAB 交替模式、跨回合全局 (tool,args) 重复计数，并保留其冷启动豁免设计（开局密集读文件不算循环）。纯启发式，无需模型参与，成本低收益极高。

2. **`replace_all` 批量替换**（9.1）—— 单参数改动，直接把重命名类任务的回合数从 N 次压到 1 次。同时补上「命中 N 处但未开启 replace_all」的明确错误文案。

3. **部分读取不足以授权编辑**（9.2）—— 正确性缺陷而非能力缺失。在 `require_fresh_file_read` 的快照里增加「是否全量读取」标记，ranged read 只授权其覆盖的行范围。改动集中在 `tools/spec.rs`，成本很低。

4. **细粒度工具错误码**（第三章）—— 把 `error_taxonomy` 的 10 类扩展出工具子系统层级，重点先补 qwen-code 已验证有价值的几个区分：「确定没读」vs「无法确认是否读过」vs「重读也没用的非常规文件」。这是后续做差异化重试策略和遥测路由的前置依赖。

5. **斜杠命令 `!{...}` shell 注入**（第五章）—— mimofan 的 `.md` 命令体系已有 frontmatter 与 `$ARGUMENTS`，加注入是增量改动。务必照搬 qwen-code 的两级转义（`{{args}}` 在注入内外用不同转义）与「安全配置未加载则 abort」的防线。

6. **上下文 `@import` 递归导入**（第四章）—— `project_context/` 已有 1198 行成熟实现，加一个带循环检测和深度上限的导入处理器即可，能直接缓解大仓 AGENTS.md 膨胀。

7. **Token 计数校准**（第四章）—— 不必急着上 tokenizer。先做低成本的一步：优先采信 API 返回的 usage（`models/mod.rs:215` 已有 `input_tokens`）反推校准系数，替代无差别的 `len()/4`；中文场景收益最明显。

8. **OpenTelemetry 导出**（第六章）—— 本地 audit.log + metrics 已覆盖单机排查，OTLP 主要解决团队/企业部署的接入问题。工作量较大但可增量：先导出 GenAI 语义约定的 span 与 token usage metrics 两类即可覆盖多数需求。

9. **编码 / BOM / CRLF 保真 + 读后 TOCTOU 复检**（9.3）—— 两项都是低成本正确性修复，集中在 `tools/file.rs` 与 `tools/spec.rs`。CRLF 丢失在 Windows 仓库会制造整文件级伪 diff，危害比看上去大；TOCTOU 复检只需在 `read_to_string` 之后再调一次 `require_fresh_file_read`。

10. **`.mimoignore` 接入工具层 + secrets crate 接入写入路径**（9.3）—— 两个「现成能力没接线」的问题。`.mimoignore` 目前只影响工作集遍历，给不了用户预期的「别碰这些文件」保证；`crates/secrets/` 已存在但未接入 `tools/` 写入路径。都属于接线工作，无需新造轮子。

**明确不建议优先做**：IDE 伴随模式、扩展市场、cron 周期调度、notebook 编辑、图像生成。这几项要么工程量大（IDE、市场），要么场景相对窄（cron、notebook），在循环检测这类核心健壮性问题解决前不应占用资源。其中 `zoom_image` 是个例外 —— 单个工具、成本低，如果 UI 调试是重要场景可以顺手补上。
