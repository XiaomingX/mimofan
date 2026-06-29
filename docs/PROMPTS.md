# Prompt 索引

mimofan 的所有 LLM prompt 统一存放在 `crates/tui/src/prompts/` 目录下，
通过 `include_str!` 编译进二进制。修改 `.md` 文件后重新编译即可生效。

## 分层体系（Tier 1-9）

Prompt 按宪法层级组织，优先级从高到低：

| Tier | 层级 | 来源 |
|------|------|------|
| 1 | Constitution | `prompts/constitution.md` |
| 2 | Statutes | `prompts/approvals/*.md` |
| 3 | Regulations | `prompts/modes/*.md` |
| 4 | Project Law | `.mimofan/constitution.json`（用户项目级） |
| 5 | Memory | `prompts/memory_guidance.md` |
| 6 | Live Evidence | 工具返回的实时数据 |
| 7 | Handoffs | `prompts/compact.md` |
| 8 | Personality | `prompts/personalities/*.md` |
| 9 | Continuation | `prompts/continuation.md` |

## 核心 Prompt 文件

### 宪法与身份

| 文件 | 用途 |
|------|------|
| `constitution.md` | 核心系统 prompt — 身份、行为准则、分层宪法 |
| `authority_recap.md` | 权限层级回顾 — 系统 prompt 末尾的权威摘要 |
| `continuation.md` | 目标续行 prompt |
| `compact.md` | 上下文压缩中继模板 |
| `memory_guidance.md` | 记忆卫生规则 |
| `subagent_output_format.md` | 子代理输出格式契约 |
| `subagent_self_report_note.md` | 子代理自报告免责声明 |
| `purge_instructions.md` | 上下文清理指令 |

### 模式（Modes）

| 文件 | 用途 |
|------|------|
| `modes/agent.md` | Agent 模式 — 自主执行，写操作需审批 |
| `modes/plan.md` | Plan 模式 — 只读调查，禁止写操作 |
| `modes/yolo.md` | YOLO 模式 — 自动批准所有操作 |

### 审批策略（Approvals）

| 文件 | 用途 |
|------|------|
| `approvals/auto.md` | 自动批准 — 所有工具调用预批准 |
| `approvals/never.md` | 从不批准 — 只读模式 |
| `approvals/suggest.md` | 建议批准 — 推荐但不自动执行 |

### 人格（Personalities）

| 文件 | 用途 |
|------|------|
| `personalities/calm.md` | 冷静人格 — 克制、精确、空间感 |
| `personalities/playful.md` | 活泼人格 — 轻松、友好 |

### 语言区域（Locale）

| 文件 | 用途 |
|------|------|
| `locale_preamble_zh_hans.md` | 简体中文语言要求（系统 prompt 前置） |
| `locale_closer_zh_hans.md` | 简体中文语言再次提醒（系统 prompt 末尾） |

### 模型特性

| 文件 | 用途 |
|------|------|
| `v4_model_characteristics.md` | DeepSeek V4 架构特性 — 缓存、思考 token 策略 |
| `generic_model_characteristics.md` | 通用模型特性 — 前缀缓存、并行执行 |
| `shell_policy_disabled.md` | Shell 工具不可用时的替代指引 |

### 工具与子系统 Prompt

| 文件 | 用途 | 模板变量 |
|------|------|----------|
| `router_classifier.md` | 自动路由分类器 | `{cheap}`, `{big}` |
| `inventory_router_classifier.md` | 库存路由分类器 | `{inventory}` |
| `translator.md` | 翻译器 | `{target_language}` |
| `compaction_specialist.md` | 上下文压缩专家 | 无 |
| `summarization_specialist.md` | 上下文摘要专家 | 无 |
| `conversation_summary.md` | 对话摘要 | 无 |
| `coding_assistant.md` | 编码助手 | 无 |
| `acp_coding_assistant.md` | ACP 编辑器编码助手 | 无 |
| `synthesis_assistant.md` | 大输出综合助手 | `{tool_name}`, `{estimated_tokens}`, `{raw_output}` |
| `voice_transcription.md` | 语音转录 | 无 |
| `skill_loader.md` | 技能加载器 | `{skill_name}`, `{skill_body}` |
| `code_reviewer.md` | 代码审查 JSON 输出 | 无 |
| `rlm.md` | RLM 递归语言模型 | 无 |

### 子代理角色 Prompt（硬编码）

以下 prompt 硬编码在 `crates/tui/src/tools/subagent/mod.rs` 中：

| 角色 | 用途 |
|------|------|
| `default` | 通用子代理 |
| `explore` | 只读探索 |
| `plan` | 只读规划 |
| `review` | 代码审查 |
| `custom` | 自定义工具集 |
| `implementer` | 实现变更 |
| `verifier` | 验证门禁 |

## 代码中的 Prompt 常量

| 常量 | 文件 | 说明 |
|------|------|------|
| `BASE_PROMPT` | `prompts.rs` | `include_str!("prompts/constitution.md")` |
| `AUTHORITY_RECAP` | `prompts.rs` | `include_str!("prompts/authority_recap.md")` |
| `COMPACT_TEMPLATE` | `prompts.rs` | `include_str!("prompts/compact.md")` |
| `GOAL_CONTINUATION_PROMPT` | `prompts.rs` | `include_str!("prompts/continuation.md")` |
| `MEMORY_GUIDANCE` | `prompts.rs` | `include_str!("prompts/memory_guidance.md")` |
| `SHELL_POLICY_DISABLED` | `prompts.rs` | `include_str!("prompts/shell_policy_disabled.md")` |
| `V4_MODEL_CHARACTERISTICS` | `prompts.rs` | `include_str!("prompts/v4_model_characteristics.md")` |
| `GENERIC_MODEL_CHARACTERISTICS` | `prompts.rs` | `include_str!("prompts/generic_model_characteristics.md")` |
| `LOCALE_PREAMBLE_ZH_HANS` | `prompts.rs` | 简体中文前置语言要求 |
| `LOCALE_CLOSER_ZH_HANS` | `prompts.rs` | 简体中文末尾语言提醒 |
| `PURGE_INSTRUCTIONS` | `purge.rs` | `include_str!("prompts/purge_instructions.md")` |
| `REVIEW_SYSTEM_PROMPT` | `tools/review.rs` | `include_str!("prompts/code_reviewer.md")` |
| `RLM_SYSTEM_PROMPT` | `rlm/prompt.rs` | `include_str!("prompts/rlm.md")` |
| `SUBAGENT_SELF_REPORT_NOTE` | `tools/subagent/mod.rs` | `include_str!("prompts/subagent_self_report_note.md")` |

## 修改指南

1. 编辑对应的 `.md` 文件
2. 运行 `cargo build` 验证编译
3. 运行 `cargo test -p mimofan` 验证测试
4. 模板变量使用 `{variable_name}` 语法，对应 `format!()` 调用
