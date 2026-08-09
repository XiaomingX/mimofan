# mimofan 升级规划（mimofan_upgrade.md）

> 综合 `vs_openclaw.md` / `vs_kilocode.md` / `vs_qwen.md` / `vs_kimicode.md` / `vs_hermes.md` 五份对标产出。
> 目标：**长程记忆能力 + 复杂工程化处理能力 + 吸收竞品全部优点 + 性能极佳**。
> 评分体系见 `EVAL_METRICS.md`，评测脚本在 `benchmark/agentbench/`。
>
> 标记约定：`- [x]` = 已实现并经验证；`- [ ]` = 待实现。
> **所有「缺失」结论均已 grep 亲自复核**（五份对标共推翻 16 处 subagent 误判，见各文档附表）。

---

## 0. 改动前基线（2026-08-08，commit `2b0ca30`）

| 维度 | 分值 |
|---|---|
| A 类静态能力矩阵 | **55.00 / 60（91.7%）** |
| B1 构建健康度 | 18 warnings → **1.50 / 6** |
| B4 Tokenizer 精度 | 平均误差 26.6%、纯中文 34.8% → **3.64 / 7** |
| B2/B3/B5/B6 | 见 `benchmark/agentbench/results/baseline_*.json` |

mimofan 的**净领先项**（五份文档一致确认，不需补齐）：压缩三范式（含 append-only 软缝护 prefix cache）、
子智能体编排（mailbox/bus/task_claim/decomposer/aggregator/worktree 隔离）、任务依赖图、
工具 BM25 延迟加载、行内容哈希锚点 `line_ref`、编辑双重模糊回退、`rlm/` 递归 REPL、
本机沙箱（seatbelt/opensandbox）、MCP client+server 双形态。

---

## 1. P0 — 地基与正确性（本轮必做）

这批的共同点：**成本低、影响面大、是其他能力的地基**，且多份对标独立指向同一处。

### 1.1 真实 tokenizer 统一计数 ★ 五份文档中 3 份列为 Top1
- [ ] 引入 `tiktoken-rs`，新建 `crates/tui/src/tokenizer/` 作为**唯一权威计数入口**
- [ ] 收敛现有 6 处不一致实现（详见下表，字节/字符单位混用是主 bug）
- [ ] 保留字符启发式作为 fallback（tokenizer 不可用时降级，不 panic）
- [ ] 按模型族选择编码（cl100k_base / o200k_base），未知模型走默认
- [ ] 为 tokenizer 补单元测试，覆盖中/英/代码/JSON/混合

**待收敛的落点**（已 grep 定位）：

| 落点 | 原实现 | 单位 | 问题 |
|---|---|---|---|
| `compaction/mod.rs:581,584,587,589` | `text.len() / 4` | **字节** | 中文 3 字节/字 → 系统性低估 |
| `compaction/mod.rs:616` | `chars().div_ceil(3)` | 字符 | 与上面同文件不一致 |
| `tools/large_output_router.rs:75` | `chars().div_ceil(3)` | 字符 | 中文误差 69% |
| `tool_output_receipts/mod.rs:339` | `chars.div_ceil(4)` | 字符 | 中文误差 76% |
| `resource_telemetry/mod.rs:223` | `(chars+3)/4` | 字符 | 同上 |
| `utils/mod.rs:500-503` | `text.len()` | **字节** | 仅字符数，被上层当 token 用 |

> 实测误差（本机 tiktoken cl100k_base 为真值，12 条样本）：
> bytes/4 平均 **26.6%**、纯中文 **34.8%**；chars/3 中文 **69.2%**；chars/4 中文 **76.4%**。

### 1.2 循环 / 重复 / 停滞检测 ★ vs_qwen 判定为「最严重差距」
- [ ] 新建 `crates/tui/src/loop_guard/`，恢复被移除的循环刹车
- [ ] 同名同参工具调用重复检测（参数指纹哈希）
- [ ] ABAB 交替模式检测（A→B→A→B 循环）
- [ ] 连续无进展检测（工具调用无状态变化）
- [ ] 冷启动豁免 + 阈值可配，避免误伤正常重试
- [ ] 接入 `turn_loop`，触发后注入有界提示而非硬中断

> 现状：全库 240k 行中 `loop_guard` 仅剩 `engine_config.rs:200` 一句注释自陈
> 「the in-turn loop_guard that used to brake repetition is gone」，仅靠 `max_steps: 1000` 兜底。
> qwen-code `loopDetectionService.ts` 有 6 维检测。ABAB 循环在 mimofan 会烧上千次调用才停。

### 1.3 部分读取不应授权全文编辑（正确性隐患）
- [ ] `FileReadSnapshot`（`tools/spec.rs:127`）增加已读行范围记录
- [ ] `require_fresh_file_read` 校验编辑目标行是否落在已读范围内
- [ ] 越界时返回明确可操作的错误（提示先读取对应区间）

> 现状：snapshot 只存 `len` + `modified`，模型读前 200 行即可编辑第 800 行。
> qwen-code `read-file.ts:154` 明确只有无 offset/limit 才算 full read。

### 1.4 edit_file 的 replace_all 语义
- [ ] `edit_file` 增加 `replace_all: bool` 参数
- [ ] 未开启且多处命中时，报错信息包含命中次数与建议
- [ ] 补测试覆盖单处/多处/replace_all 三种路径

> 现状：mimofan 有完善的 non-unique 检测与归一化回退，但**只能逐处编辑**，重命名类改动回合数翻倍。

### 1.5 编辑保真度（正确性问题，非能力缺失）
- [ ] 编辑保留 BOM / CRLF / 原文件编码（当前会把 Windows 仓库整文件变成伪 diff）
- [ ] `require_fresh_file_read` 补读后 TOCTOU 复检（写入前二次校验 mtime+len）
- [ ] `.mimoignore` 下沉到工具层拦截（当前只管工作集遍历，工具可绕过）
- [ ] `crates/secrets/` 接入写入路径（模块已存在但未挂载，密钥可能被直接写盘）

> 这批是 vs_qwen 单独挖出的**正确性隐患**，修复成本低于新增能力，且不修会持续产生脏 diff。

### 1.6 清零构建 warning（工程化基线）
- [ ] 修复 18 个 clippy warning（含 `memory.rs` 3 处 `unused Result`）
- [ ] 保证 `cargo clippy --workspace --all-targets` 零 warning

---

## 2. P1 — 长程记忆闭环（用户明确要求的核心能力）

五份对标一致指出：mimofan 的记忆是**被动存取**，缺「主动沉淀 + 高效召回」的闭环。

### 2.1 跨会话全文检索 ★ hermes/qwen 双方指向
- [ ] 会话内容建全文索引（FTS），而非仅标题
- [ ] 新增 `session_search` 工具，让模型能检索自己的历史对话
- [ ] 命中结果按相关度排序并附会话/时间定位

> 现状：`session_manager/mod.rs:605 search_sessions` 只做
> `s.title.to_lowercase().contains(query)`；全库 grep `fts|full_text` **零命中**。

### 2.2 记忆生命周期治理（写入侧完全缺失，负复利风险）
- [ ] 记忆条目增加时间戳与陈旧性标记（stale / ttl）
- [ ] 召回时对陈旧记忆降权并提示需要复核
- [ ] 支持按主题归档，永不静默删除
- [ ] **短期 → 长期晋升（promotion）**：高频命中 / 被显式确认的记忆升级为长期
- [ ] **离线整合（consolidation）**：空闲时合并同主题重复条目，去冗余
- [ ] **衰减与 prune（decay）**：长期未命中条目降权直至归档，控制索引规模

> 现状（grep 复核）：`crates/memory/src/` 与 `crates/tui/src/vector_memory/` 中
> `promot|consolidat|decay|dream|prune` **零命中**——写入侧生命周期完全缺失。
> 向量记忆**默认开启**，条目只增不减：随使用时长增加召回质量单调下降（负复利）。
> 这是「长程记忆」目标下优先级最高的一项。

### 2.3 情景记忆（episodic）
- [ ] 支持按时间 / 会话维度检索「当时发生了什么」
- [ ] 与现有语义向量记忆互补（前者管「何时」，后者管「关于什么」）

### 2.4 周期性记忆 nudge（hermes 独有，成本近乎为零）
- [ ] 每 N 轮提醒模型「这轮有什么值得沉淀的知识？」
- [ ] 与现有 `turn_memory.rs` 正交互补——后者是正则挖掘「系统替模型记」，
      nudge 是「提醒模型自己提炼」，能捞到需理解才能得出的知识
- [ ] 可配置轮次间隔，默认关闭以免打扰

---

## 3. P1 — 复杂工程化处理能力

### 3.1 验证停机守卫 ★ hermes 评为「价值最高成本最低」
- [ ] 模型在**编辑代码后**试图直接结束回合、且验证账本无新鲜证据时，注入有界 nudge
- [ ] 复用已有积木：`tools/verifier.rs` + `evidence/`，只差接成闸门
- [ ] 假阳性抑制：本轮仅改 `.md/.txt` 等非代码文件时不触发

### 3.2 细粒度工具错误码
- [ ] 扩展错误分类，区分「可重试」与「结构性死路」
- [ ] 关键区分：`EDIT_REQUIRES_PRIOR_READ` / `FILE_CHANGED_SINCE_READ` /
      `TARGET_NOT_REGULAR_FILE`（FIFO/socket，重读也没用，避免读-编辑死循环）
- [ ] 引擎可按错误码差异化重试

> 现状：mimofan 10 类分类无法表达这些区分，工具侧只能 `execution_failed` + 自由文本。
> qwen-code `tool-error.ts` 有 60+ 个 `ToolErrorType`。

### 3.3 上下文 @import 递归导入
- [ ] AGENTS.md 支持 `@path/to/file.md` 递归引入
- [ ] 循环检测 + 深度上限
> 让大型仓库可拆成「总纲 + 各模块细则」，避免单文件无限膨胀。

### 3.4 斜杠命令内嵌 shell（`!{...}`）
- [ ] 自定义命令模板支持内嵌 `!{git diff --staged}`，结果内联进 prompt
- [ ] 两级转义：`{{args}}` 在 `!{}` 外原文替换、在内用 shell 转义（防注入）
- [ ] 安全配置未加载时直接 abort
> 这是「/review 自动带上当前 diff」类高频命令的关键能力。

---

## 4. P2 — 自我改进闭环（hermes 唯一真正领先项）

> mimofan 技能体系经 grep 确认是**纯消费型**：`tools/skill.rs` 只有只读 `load_skill`，
> `skill_state` 仅 enabled/disabled 二态，模型**无法生产技能**。这是闭环的卡点。

- [ ] agent 可写的技能管理工具（创建 / 编辑 / 归档），带 provenance 隔离（区分用户技能与 agent 自建）
- [ ] 技能四态生命周期（active / stale / archived / pinned）与自动流转
- [ ] 空闲时 Curator 后台策展：只动 agent 自建技能、永不删除只归档、pinned 豁免
- [ ] 回合后台自评分叉：fork 受限 agent（工具白名单）反思沉淀，不污染主会话与 prefix cache

---

## 5. P2 — 代码库语义索引（kilocode 判定为最大结构性缺口）

- [ ] 用 tree-sitter 对代码文件分块，复用**已有**的 `crates/memory/src/vector.rs`
      （hnsw-rs + sled）与 `EmbeddingService` —— 这是**组装而非从零**
- [ ] 新增 `codebase_search` 语义检索工具
- [ ] 增量 watcher，避免全量重建
- [ ] 强化 `symbol_index`（现仅 104 行行首前缀匹配 `strip_prefix("fn ")`，召回很弱）

> 注意：`tool_catalog.rs:1058` 的 `semantic_search` 只是**测试夹具字符串**，不是实现。

---

## 6. P2 — 其他竞品优点吸收

- [ ] SWE-Pruner 式**事前**工具输出裁剪（模型自声明 context_focus_question）
      —— 与现有 `large_output_router` 的**事后**综述正交，不重复
- [ ] 不可信外部内容包裹 + 提示注入检测（执行侧防护已扎实，**输入侧完全敞开**，木桶短板）
- [ ] `ask_user` 交互提问工具
- [ ] Notebook（.ipynb）单元格编辑
- [ ] 预留输出预算触发压缩（reservedContextSize）
- [ ] `/context` 展示 Autocompact buffer 预留量
- [ ] 压缩后「可重建内容重新注入」二分法（项目记忆/rules 压缩后重读，只对真正对话历史做有损摘要）
- [ ] 协议违规兜底 nudge（模型口头说下一步却 `finish_reason=stop` 退出；**对 MiMo/DeepSeek 国产模型相关性高**）
- [ ] 子智能体断点续跑（resume）
- [ ] 定时 / 周期任务（cron）
- [ ] IDE 伴随模式（感知用户当前打开文件与选区）
- [ ] 扩展市场与远程安装（含 zip slip 防护）
- [ ] OpenTelemetry / OTLP 导出
- [ ] 设备码 OAuth 等多认证方式
- [ ] `zoom_image` 局部放大（UI 调试类任务关键动作）

---

## 7. 明确不做（形态不适用 / 有意克制）

- 多平台 IM 网关（Telegram/Discord/Slack/WhatsApp/Signal）、TTS 语音唤醒、智能家居 —— hermes 的个人助理形态
- serverless 终端后端（Modal/Daytona/Vercel Sandbox）—— 与本地 CLI 定位不符
- VS Code / JetBrains GUI 独有能力 —— 依赖编辑器 API
- 图像 / 视频生成 —— 非 code agent 目标域
- **微压缩（micro-compaction）**：会破坏 prefix cache，hermes 自己默认关闭；
  mimofan 有 `prefix_cache/` 与软缝设计，若做必须是可选项 —— 本轮不做
- **轨迹压缩（trajectory compression）**：面向训练数据生产，与运行时压缩优化目标正交，勿混为一谈

---

## 8. 风险与约束

1. **一切围绕 prefix cache**：seam_manager 的 append-only、摘要块 `cache_control: ephemeral`、
   工具裁剪「从新往旧」都是为了少破坏缓存。改压缩相关代码前先问「会不会让缓存前缀失效」。
2. **记忆注入位置风险**：`compose_index_block` 在 `engine.rs:2660` 位于回合路径，
   中途 `remember` 若重建 system prompt 会打爆 prefix cache —— 实施 2.x 时必须验证。
3. **压缩阈值仍分散 4 处**（`turn_loop.rs` / `context_usage.rs` / `engine.rs recover_context_overflow` /
   `seam_manager`），`context_budget` 只收敛了 UI 侧。改阈值前先确认改的是哪一处。
4. **测试成本高**：`cargo test -p mimofan --lib` 需 20 分钟以上，务必后台运行。
   `cargo check` 不编译 `#[cfg(test)]`，新增测试后要用 `--all-targets` 先验证。
