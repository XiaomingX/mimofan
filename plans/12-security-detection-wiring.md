# 网络安全检测接线 + AVDH 能力补充计划

> 依据：Google Threat Intelligence Group《Staying Ahead of Adversarial AI Through Agentic Source Code Review》(2026-08-19) 的 **Agentic Vulnerability Discovery Harness (AVDH)** 架构。
> 目标：① 盘点并接线当前项目**已实现但未接线**的网络安全检测代码；② 借鉴 AVDH 的 agentic 编排方法论补齐检测能力缺口。

---

## 现状盘点（已实证，2026-08-19）

### 已接线（无需改动）
| 组件 | 位置 | 说明 |
|------|------|------|
| taint/interproc/rules/callgraph/kb_trace/knowledge/auto_gadget/sarif 引擎 | `crates/staticanalysis/src/*.rs` | 被工具层调用 |
| TUI 工具 ast_query/call_graph/gadget_chain/auto_gadget/run_poc/hypothesis | `crates/tui/src/core/engine/tool_setup.rs:94-99` | 主 CLI 路径真实注册 |
| EvalHarness + evaluate.py verifier | `crates/tui/src/eval/mod.rs` + `benchmark/vuln_hunt/` | 完整可运行 |
| 沙箱 Seatbelt/Landlock/OpenSandbox/Container | `crates/tui/src/sandbox/*.rs` | 真实实现，run_poc/disposable 接线 |
| security-audit skill（bundled） | `crates/tui/src/skills/system.rs:90` | 编译期内嵌 |
| security_auditor 人格 | `crates/tui/src/prompts/personalities/security_auditor.md` | 被 prompts/mod.rs:543 引用 |

### 未接线 / 死代码（需处理）
| 组件 | 位置 | 现状 | 证据 |
|------|------|------|------|
| **security_audit.rs（semgrep）** | `crates/tui/src/tools/security_audit.rs` | `build_semgrep_command`/`run_semgrep_scan`/`to_review_issue` **零调用方**，仅 `tools/mod.rs:80` pub mod + 自身单测 | grep 无 TUI 调用 |
| **recon 编排器** | `crates/staticanalysis/src/recon.rs` | `ReconCapability` trait + `run()` 并行编排器完整但 **TUI 零引用** | grep crates/tui 无 recon:: |
| **typestate（协议 FSM）** | `crates/staticanalysis/src/typestate.rs` | `check_sequence` 引擎完整，无 TUI 调用 | grep 无引用 |
| **attack_surface（攻击面枚举）** | `crates/staticanalysis/src/attack_surface.rs` | `scan_attack_surface`/`enumerate_surface` 完整，无 TUI 调用 | grep 无引用 |
| **index.rs（SQLite 符号索引）** | `crates/staticanalysis/src/index.rs` | 默认 feature 关闭，未接线 | lib.rs:43 cfg 门 |
| **vuln-hunt skill** | `crates/tui/assets/skills/vuln-hunt/SKILL.md` | 存在但**未编译期内嵌**（非 bundled），仅文件系统 skill | system.rs 无 include_str! |

### 关键矛盾
`security_auditor.md` 人格明确指示 agent "drive `semgrep`"（L36），但 `security_audit.rs` 从未接线成工具——即**文档宣称存在、实际不可用**的能力。

---

## AVDH 文章 → 能力缺口映射

| AVDH 阶段 | mimofan 现状 | 缺口 |
|-----------|-------------|------|
| **Threat Model**（Explorer→Specialist→Synthesis + **人类审批门**） | 有 `attack_surface`/`recon` 引擎但未接线；**无审批门/可视化** | 需编排 + 审批门 |
| **Entry Point Discovery**（并行 Discovery agent，提取入口点+用户输入源） | 有 `call_graph`/`ast_query`，但无"枚举入口点+提取 source"的编排工具 | 需补 |
| **Context Enrichment**（跨文件聚合上下文，决定走 Access Control 还是 Data Flow） | 有 `interproc` 引擎，无 enrichment 编排 | 需补 |
| **Hypothesis Generation**（Access Control + Data Flow 双 agent + Confidence Filter） | 有 `hypothesis` 工具 + taint(data flow)，但**无 access-control 静态分析器** | 需补 access control 能力 |
| **Multi-Validation + Synthesis**（高温多验证 agent + 综合判定 confirmed/disproven/rejected） | 单 hypothesis + evidence 门，无多验证综合 | 需补 |
| **Distilled Knowledge 层级规则**（domain→language/framework→vulnerability） | rules/ 已有 java.yaml/rust.yaml/nextjs.yaml（语言/框架）+ kb/gadgets.yaml（漏洞），扁平 | 基本匹配，可强化 hierarchy |
| **POC 动态验证** | `run_poc` ✅ 已实现 | 无 |
| **Benchmark + FP triage + 去重 + 人工复核** | `evaluate.py` verifier ✅；无 FP-triage/duplicate-resolution agent | 部分缺口 |

---

## Phase 0：文档发现（已完成）

已通读的权威来源：
- `crates/tui/src/tools/security_audit.rs` — semgrep 命令构造/SARIF 归一化 API（签名见上表）
- `crates/staticanalysis/src/recon.rs` — `ReconBudget`/`ReconCapability`/`run()/dedupe()`（L29-99）
- `crates/staticanalysis/src/attack_surface.rs` — `enumerate_surface`/`scan_attack_surface`/`AttackSurfaceKind`
- `crates/staticanalysis/src/typestate.rs` — `ProtocolFsm::check_sequence`/`load_protocols_dir`
- `crates/tui/src/core/engine/tool_setup.rs:94-99` — 现有工具接线范式（`.with_*_tools()`）
- `crates/tui/src/tools/registry.rs` — `ToolRegistryBuilder`/`with_*_tools` 模式
- `crates/tui/src/tools/hypothesis.rs` — hypothesis 工具范式（consistency 门）
- `crates/tui/src/tools/run_poc.rs` — `evaluate()`/`tail()` 纯函数范式
- `crates/tui/src/eval/mod.rs` + `benchmark/vuln_hunt/evaluate.py` — verifier 范式
- `crates/tui/src/skills/system.rs` — bundled skill 注册范式

**Allowed APIs**：见各文件签名。**Anti-pattern**：不要凭空发明新 API；新工具一律实现 `ToolSpec` trait 并通过 `ToolRegistryBuilder::with_*_tools()` 注册（仿 tool_setup.rs:94-99）；沙箱执行一律复用 `SandboxBackend`，禁止直接 shell out。

---

## Phase 1：接线 `security_audit.rs`（semgrep）为 `security_audit` 工具

**What**：把现成 semgrep 辅助库封装成 `ToolSpec` 工具，让 `security_auditor.md` 人格的指令真正可用。

**Task**：新建 `crates/tui/src/tools/security_audit_tool.rs`（或扩展现有文件），实现：
- `SecurityAuditTool` 结构体，`name = "security_audit"`，`ReadOnly` + `Auto` 审批（仿 `crates/tui/src/tools/call_graph.rs`）
- `input`：`{ target, config?, extra_flags? }`
- `execute`：调 `security_audit::run_semgrep_scan(backend, opts)`，把返回的 `Vec<SecurityIssue>` 经 `to_review_issue` 归一化成 `ReviewIssue` 列表输出
- 复用 `context.sandbox_backend`（同 `run_poc.rs:149` 模式）；无 backend 时 fail-closed 返回错误
- 在 `tools/mod.rs` 导出，并在 `tool_setup.rs:94-99` 链上加 `.with_security_audit_tools()`

**Doc ref**：`security_audit.rs:40-101`（API）、`run_poc.rs:21,135-159`（backend 复用+fail-closed 范式）、`call_graph.rs`（ToolSpec 范式）。

**Verification**：
- `cargo build -p mimofan-tui` 零 warning
- `cargo test -p mimofan-tui security_audit`（含既有单测 + 新增注册单测）
- grep 确认 `run_semgrep_scan` 在 TUI 有真实调用方
- `cargo test -p mimofan-tui -- --list` 列出 `security_audit` 相关用例

**Anti-pattern guards**：不新增命令行参数；不直接 `std::process::Command` 调 semgrep（必须走 SandboxBackend）；不改动 `security_audit.rs` 现有签名。

---

## Phase 2：接线 `recon` + `attack_surface` 为 `attack_surface` 工具

**What**：把已就绪的并行侦察编排器与攻击面枚举引擎暴露为工具，实现 AVDH 的 **Threat Model / Entry Point Discovery** 雏形。

**Task**：新建 `crates/tui/src/tools/attack_surface_tool.rs`，实现：
- `AttackSurfaceTool`，`name = "attack_surface"`，`ReadOnly` + `Auto`
- `input`：`{ target_dir, include_implied_autotype? }`
- `execute`：
  1. 构造 `ReconBudget { max_parallel: 4, worktree_root, token_budget }`
  2. 用 `recon::run(budget, caps)` 并行跑 `attack_surface::enumerate_surface` 与 `kb_trace::trace_chains` 两个 capability（封装成 `ReconCapability` 实现）
  3. 输出去重后的 `SecurityIssue` 列表（含 GadgetChain/ImplicitAutoType/VulnerableDependency/SinkPresent 分类）
- 在 `tool_setup.rs` 注册 `.with_attack_surface_tools()`

> **实现偏差（2026-08-19 已落地）**：未用 `recon::run` 线程编排。因为 `enumerate_surface`
> 的入参（`KnowledgeBase` + `Dependency[]` + OSV advisories）全部来自**同一份 lockfile 扫描**
> （`scan_attack_surface` 已封装），并行 capability 在此无独立数据源，强行拆线程是伪并行。
> 故直接：① `load_kb_dir` 载入捆绑 KB；② 自动发现/显式指定 lockfile → `scan_attack_surface`
> 产出 `AttackSurfaceEntry[]`；③ 归一化为 `SecurityIssue` 再进统一 `security_issues` 通道
> （复用 `security_audit::to_review_issue`）。OSV 用 `InMemoryOsv`（离线、无网络），保持只读。
> 与 `auto_gadget_discovery`（源码级 sink/pivot）互补：本工具是**依赖驱动**攻击面。

**Doc ref**：`recon.rs:29-99`（ReconBudget/ReconCapability/run）、`attack_surface.rs:40-171`（AttackSurfaceKind/enumerate_surface/scan_attack_surface）、`kb_trace.rs:95-110`（trace_chains）。

**Verification**：
- `cargo build -p mimofan-tui` 零 warning（注意：若 `recon`/`attack_surface` 依赖缺失 feature，需在 `staticanalysis/Cargo.toml` 确认默认 feature 覆盖，必要时补 feature 依赖）
- `cargo test` 零失败
- 手工：对 `benchmark/vuln_hunt/fixtures/bad/` 运行工具，确认能枚举出 sink/依赖告警

**Anti-pattern guards**：`recon::run` 是同步 `std::thread` 实现（无 async runtime in staticanalysis crate），TUI 侧注意用 `spawn_blocking` 包裹避免阻塞 executor；不把 `index.rs` 硬接（SQLite feature 默认关闭，属独立工作）。

---

## Phase 3：接线 `typestate` 为 `protocol_check` 工具

**What**：暴露协议状态机（FSM）检测，覆盖反序列化/会话类"非法状态序列"漏洞（如 `safeMode → readObject` 守卫绕过）。

**Task**：新建 `crates/tui/src/tools/protocol_check_tool.rs`，实现：
- `ProtocolCheckTool`，`name = "protocol_check"`，`ReadOnly` + `Auto`
- `input`：`{ target_dir }`
- `execute`：`typestate::load_protocols_dir(dir)` → 对扫描到的调用序列跑 `check_sequence`，输出违规序列告警
- 注册 `.with_protocol_check_tools()`

> **实现偏差（2026-08-19 已落地）**：`input` 从 `{ target_dir }` 改为
> `{ call_sequence: [{ method, line }] }`。原因：`callgraph::CallEdge` 只记 callee
> **方法名**、不记 receiver 对象（无法定位"某受追踪对象上的调用序列"），从源码自动提取序列
> 会发明不存在的 receiver 追踪。故模型/上游分析器负责抽取有序调用序列，本工具只做
> `check_sequence` 校验（引擎的价值点）。`load_protocols_dir` 载入捆绑 `rules/protocols/*.yaml`。

**Doc ref**：`typestate.rs:80,158,208`（ProtocolFsm::from_yaml/check_sequence/load_protocols_dir）、`rules/protocols/deserialization.yaml`。

**Verification**：build 零 warning；test 零失败；对 deserialization.yaml 语义做一次冒烟。

**Anti-pattern guards**：不新增 YAML 协议 schema；`check_sequence` 的入参是调用序列，调用侧需自行从 callgraph 收集序列（仿 `crates/tui/src/tools/call_graph.rs` 的图遍历）。

---

## Phase 4：把 `vuln-hunt` skill 升级为 bundled skill

**What**：让 `vuln-hunt/SKILL.md`（六步长程漏洞挖掘工作流）成为编译期内嵌的 bundled skill，与 `security-audit` 同级暴露。

**Task**：
1. `crates/tui/src/skills/system.rs`：加 `const VULN_HUNT_BODY: &str = include_str!("../../assets/skills/vuln-hunt/SKILL.md");`，在 `BUNDLED_SKILLS` 数组加条目 `{ name: "vuln-hunt", body: VULN_HUNT_BODY, introduced_in: 5 }`（当前 BUNDLED_SKILL_VERSION 为 "5"，需同步递增到 "6" 触发重新安装）
2. 确认 SKILL.md 中提到的工具名与工具实现一致（hypothesis/gadget_chain_trace/run_poc/call_graph/ast_query/security_audit/attack_surface/protocol_check）

**Doc ref**：`system.rs:7-93`（BUNDLED_SKILL_VERSION/BUNDLED_SKILLS 范式）。

**Verification**：
- build 零 warning
- 单测：`is_bundled_skill_name("vuln-hunt") == true`
- `/skills` 列表出现 vuln-hunt

**Anti-pattern guards**：不要把 skill 正文重复硬编码；只做 include_str 接线，不改 SKILL.md 工作流内容（除非工具名对不上）。

---

## Phase 5：AVDH 启发的新能力（选做，按优先级）

借鉴 AVDH 但**不重复造已存在能力**，按价值排序建议新增：

### 5a. Access-Control 假设分析（高价值，填补结构性缺口）
- AVDH 的 **Access Control agent**（判断权限检查是否缺失/用错身份）当前无静态分析器对应。
- 提议：在 `staticanalysis` 新增 `access_control.rs`，基于 `callgraph` 分析某入口点是否经过授权 gate（仿 fixture 里 h3 "No auth gate protects the endpoint" 语义），输出 `SecurityIssue{category:"Access-Control"}`。
- 该能力可接入 `recon` capability 与 Phase 2 的 `attack_surface` 工具。

### 5b. Multi-Validation Synthesis（AVDH 高温多验证 → 综合判定）
- 当前 `hypothesis` 是单证据门。AVDH 用多个高温 Validation agent + 1 个 Synthesis agent 产出 confirmed/disproven/rejected。
- 提议：为 `hypothesis resolve` 增加可选 `synthesis` 字段，或新增 `validate_hypothesis` 工具，接受多条 evidence/verdict，综合给出三态判定。
- 评估：与现有一致性门（零证据禁止 resolve）兼容，作为增强而非替换。

### 5c. Threat-Model 审批门 + 可视化
- AVDH 在 Threat Model 阶段设**人类审批门**，并提供文本+可视化。
- 提议：`attack_surface` 工具输出结构化威胁模型 + 用 `#[derive]` 或 mermaid/ASCII 图呈现组件暴露面；评估是否加审批门（依赖 TUI 交互，成本较高，建议仅输出结构化结果，审批门延后）。

### 5d. 强化规则层级（domain→language/framework→vulnerability）
- 现有 rules/ 已按语言/框架（java/rust/nextjs）与漏洞（kb/gadgets.yaml）分文件，基本匹配 AVDH 三级规则。可补充 **domain 级**入口点定义规则（如 web 路由、IPC listener 识别），供 Entry Point Discovery 使用。

### 5e. FP-triage / Duplicate-resolution agent（Benchmark 侧）
- `evaluate.py` 现为确定性评分。AVDH 用 agent 做 FP-triage 与去重判定。可在 benchmark 侧补一个 LLM 判卷 agent（可选）。

---

## Phase 6：最终验证

1. **全量构建**：`cargo build --workspace` 零 error、零 warning
2. **全量测试**：`cargo test --workspace` 零失败（注意 memory 提示：并行 agent 下用 `--no-run` + 直跑测试二进制规避 file lock）
3. **接线自检 grep**：
   - `run_semgrep_scan` / `scan_attack_surface` / `check_sequence` 在 `crates/tui` 均有真实调用方
   - `is_bundled_skill_name("vuln-hunt") == true`
4. **Anti-pattern grep**：确认无 `std::process::Command` 直接调 semgrep / 无凭空 `security_audit` 之外的重复 semgrep 封装
5. **冒烟**：对新工具的 input/output 结构做一次 `cargo test` 驱动验证

---

## 结论要点（供评审）

- **必做**（当前明确死代码/文档谎言，风险高）：Phase 1（semgrep 工具接线）✅、Phase 4（vuln-hunt skill 接线）✅
- **推荐**（引擎已就绪、零新增引擎成本）：Phase 2（recon+attack_surface）✅、Phase 3（typestate）✅
- **选做**（AVDH 启发、需新引擎）：Phase 5a/5b/5d（Access-Control、Multi-Validation、domain 入口点规则）
- **延后**：Phase 5c 审批门可视化（依赖 TUI 交互）、5e（benchmark 判卷 agent）

> 状态：Phase 1-4 已在 `feat/security-detection-wiring` 落地（Phase 2/3 有实现偏差见上），
> Phase 5 各子项未做。
