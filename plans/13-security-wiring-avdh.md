# 网络安全检测接线 + AVDH 能力补充计划（核验修订版）

> 依据：Google Threat Intelligence Group《Staying Ahead of Adversarial AI Through Agentic Source Code Review》(2026-08-19) 的 **Agentic Vulnerability Discovery Harness (AVDH)** 架构。
> 目标：① 盘点并接线当前项目**已实现但未接线**的网络安全检测代码（真实死代码）；② 借鉴 AVDH 的 agentic 编排方法论补齐检测能力缺口。
> 本计划为 `plans/12-security-detection-wiring.md`（初稿，2026-08-19）的**实测修订版**，修正了初稿中的工具名/类型名偏差，并据最新代码状态（2026-08-20）重新核实。

---

## 现状盘点（已实证，2026-08-20）

### 已接线（无需改动）
| 组件 | 位置 | 说明 |
|------|------|------|
| taint/interproc/rules/callgraph/kb_trace/knowledge/auto_gadget/sarif 引擎 | `crates/staticanalysis/src/*.rs` | 被 TUI 工具层调用 |
| ast_query / call_graph / gadget_chain_trace / auto_gadget_discovery / run_poc | `crates/tui/src/tools/{ast_query,call_graph,gadget_chain,auto_gadget,run_poc}.rs` | 已在 `tool_setup.rs:94-99` 生产链注册 |
| hypothesis 工具 | `crates/tui/src/tools/hypothesis.rs` | 经 `with_agent_tools_policy`（`registry.rs:1162`）注册，生产路径可用（**不在 tool_setup 链上，而是 agent policy 注册**） |
| EvalHarness + evaluate.py verifier | `crates/tui/src/eval/mod.rs` + `benchmark/vuln_hunt/evaluate.py` | 完整可运行，4 维评分（consistency/trace/reproduce/auto_discovery） |
| 沙箱 Seatbelt/Landlock/OpenSandbox/Container | `crates/tui/src/sandbox/*.rs` | 真实实现，run_poc/disposable 接线 |
| security-audit skill（bundled） | `crates/tui/src/skills/system.rs:20,89-93` | `include_str!` 编译期内嵌 |
| security_auditor 人格 | `crates/tui/src/prompts/personalities/security_auditor.md` | 被 prompts/mod.rs:543 引用 |

> ⚠️ **工具名修正**：初稿称 `gadget_chain`/`auto_gadget`，实际模型-facing 名为 **`gadget_chain_trace`**（gadget_chain.rs:26）与 **`auto_gadget_discovery`**（auto_gadget.rs:25）。任何给模型的提示词/SKILL/评测 harness 引用工具名时**必须用全名**（eval/mod.rs:232,263,369 已用 `gadget_chain_trace`）。

### 未接线 / 死代码（需处理）—— 本计划核心
| 组件 | 位置 | 现状 | 证据 |
|------|------|------|------|
| **security_audit.rs（semgrep）** | `crates/tui/src/tools/security_audit.rs` | `build_semgrep_command`(:40)/`run_semgrep_scan`(:58)/`to_review_issue`(:84) 是**纯辅助函数，不实现 ToolSpec**，全 TUI **零调用方** | grep 全 crates/tui 仅命中自身+单测 |
| **recon 编排器** | `crates/staticanalysis/src/recon.rs` | `ReconBudget`(:28)/`ReconCapability`(:51)/`run`(:59)/`dedupe`(:89) 完整，`lib.rs:321 pub mod`（无 feature 门），**TUI 零引用** | grep 仅 tests/recon_test.rs |
| **attack_surface（攻击面枚举）** | `crates/staticanalysis/src/attack_surface.rs` | `AttackSurfaceKind`(:52)/`enumerate_surface`(:65)/`scan_attack_surface`(:171) 完整，`lib.rs:318 pub mod`，**TUI 零引用** | grep 仅 tests/attack_surface_test.rs |
| **typestate（协议 FSM）** | `crates/staticanalysis/src/typestate.rs` | `ProtocolFsm`(:43)/`from_yaml`(:80)/`check_sequence`(:158)/`load_protocols_dir`(:208) 完整，`lib.rs:327 pub mod`，**TUI 零引用** | grep 仅 tests/typestate_test.rs |
| **index.rs（SQLite 符号索引）** | `crates/staticanalysis/src/index.rs` | `SymbolIndex`(:56)（**注意：不是 `IndexDb`**）。`lib.rs:42-43 #[cfg(feature="symbol-index")]` 门控，默认 feature 不含 → **默认构建根本不编译** | Cargo.toml:26 default 不含 symbol-index |
| **vuln-hunt skill** | `crates/tui/assets/skills/vuln-hunt/SKILL.md` | 存在（2095B）但**不在 `BUNDLED_SKILLS`**（system.rs:28-94 共 13 个，无 vuln-hunt），`is_bundled_skill_name("vuln-hunt")==false`，仅文件系统 skill | system.rs 无 include_str! |
| **benchmark run.sh** | `benchmark/vuln_hunt/run.sh` | README.md:15,45 引用但**文件不存在** | ls 确认缺失 |

### 关键矛盾（文档谎言）
`security_auditor.md` 人格 L36 明确指示 agent "drive `semgrep`"（`## Tooling you may drive` 段落），但 `security_audit.rs` 从未接线成工具——**文档宣称存在、实际不可用**。这是最高优先级接线项。

### 结构性缺口（AVDH 映射）
- **Access-Control 静态分析缺失**：全 staticanalysis 规则集均为 sink 类（RCE/注入/JNDI/反序列化），无任何引擎判断"入口点是否有授权 gate"。grep `authz|authorization|access.control` 全命中 HTTP header/错误分类/沙箱 ACL/secrets，无一属授权 gate 判定。
- **Multi-Validation Synthesis 缺失**：`hypothesis` 的 `Resolve`（hypothesis.rs:366）只接受**单一** `verdict`，仅有零证据一致性门（:375-384），无多验证 agent + Synthesis 综合判定。`subagent/aggregator.rs`（Vote/quorum/LlmAggregate）存在但与 hypothesis 零耦合。

---

## AVDH 文章 → 能力缺口映射

| AVDH 阶段 | mimofan 现状 | 缺口 |
|-----------|-------------|------|
| **Threat Model**（Explorer→Specialist→Synthesis + 人类审批门） | 有 `attack_surface`/`recon` 引擎但未接线 | 需接线编排 |
| **Entry Point Discovery**（并行发现入口点+source 提取） | 有 call_graph/ast_query，无"枚举入口点"编排工具 | 需补 |
| **Context Enrichment**（跨文件聚合） | 有 interproc 引擎，无 enrichment 编排 | 需补 |
| **Hypothesis Generation**（Access Control + Data Flow 双 agent） | 有 hypothesis 工具 + taint(data flow)；**无 access-control** | 需补 access-control |
| **Multi-Validation + Synthesis** | 单 hypothesis + 证据门，无多验证综合 | 需补 |
| **Distilled Knowledge 层级规则** | rules/ 已有语言/框架/漏洞/kb/协议 5 类，扁平 | 基本匹配，可强化 |
| **POC 动态验证** | `run_poc` ✅ | 无 |
| **Benchmark + FP triage + 去重 + 人工复核** | evaluate.py verifier ✅；无 FP-triage agent | 部分缺口 |

---

## Phase 0：文档发现（已完成，本计划含实测签名）

**Allowed APIs（实测签名，禁止发明）：**
- `security_audit.rs`: `build_semgrep_command(&SemgrepOptions)->String`(L40)、`run_semgrep_scan(&dyn SandboxBackend,&SemgrepOptions)->Result<Vec<SecurityIssue>>`(L58)、`to_review_issue(&SecurityIssue)->ReviewIssue`(L84)
- `recon.rs`: `ReconBudget{max_parallel,worktree_root,token_budget}`(L28)、`trait ReconCapability{fn name();fn run(&ReconBudget)->Vec<SecurityIssue>}`(L51)、`run(&ReconBudget,Vec<Box<dyn ReconCapability>>)->Result<Vec<SecurityIssue>>`(L59)、`dedupe(Vec<SecurityIssue>)`(L89)
- `attack_surface.rs`: `enum AttackSurfaceKind{GadgetChain,ImplicitAutoType,VulnerableDependency,SinkPresent}`(L52)、`enumerate_surface(&KnowledgeBase,&[Dependency],&[(Dependency,Advisory)])->Vec<AttackSurfaceEntry>`(L65)、`scan_attack_surface(&KnowledgeBase,&str,&str,&dyn OsvClient)->Result<Vec<AttackSurfaceEntry>>`(L171)
- `typestate.rs`: `ProtocolFsm{...}`(L43)、`from_yaml(&str,&str)`(L80)、`check_sequence(&[(String,usize)])->Vec<ProtocolViolation>`(L158)、`load_protocols_dir(&str)->Result<Vec<ProtocolFsm>>`(L208)
- `index.rs`: `SymbolIndex`(L56)（**非 `IndexDb`**）：`open(&Path)`(L63)/`index_file`(L142)/`index_tree`(L223)/`find_symbols`(L238)/`find_importers`(L256)/`find_references`(L267)/`forget_file`(L281)
- `sarif.rs`: `SecurityIssue{tool,rule_id,severity,category,title,description,cwe,path,line,evidence,automated}`(L19-34)
- `sca.rs`: `parse_lockfile(&str,&str)`(L62)/`scan(&str,&str,&dyn OsvClient)`(L145)/`InMemoryOsv`(L186)
- Tool 注册范式：`ToolSpec` trait + `ToolRegistryBuilder::with_*_tools()`（仿 run_poc.rs:135-159 fail-closed 沙箱复用；tool_setup.rs:94-99 链）
- Bundled skill 范式：`system.rs` `BUNDLED_SKILLS` 数组 + `include_str!`

**Anti-pattern（禁止）：** 不发明不存在的 API；新工具一律实现 `ToolSpec` 并通过 `with_*_tools()` 注册；沙箱执行一律复用 `context.sandbox_backend`（fail-closed），禁止 `std::process::Command` 直接调 semgrep；不硬接 `index.rs`（symbol-index feature 默认关闭，属独立工作）；工具名必须用实测全名（`gadget_chain_trace`/`auto_gadget_discovery`）。

---

## Phase 1：接线 `security_audit.rs`（semgrep）为 `security_audit` 工具 ⭐必做

**What**：把现成 semgrep 辅助库封装成 `ToolSpec` 工具，让 `security_auditor.md` 人格的"drive semgrep"指令真正可用（消除文档谎言）。

**Task**：新建 `crates/tui/src/tools/security_audit_tool.rs`：
- `SecurityAuditTool` 结构体，`name = "security_audit"`，审批 `ReadOnly`+`Auto`（仿 `call_graph.rs`）
- `input`：`{ target, config?, extra_flags? }`
- `execute`：`context.sandbox_backend`（同 `run_poc.rs:135-159` fail-closed 模式，无 backend 报 `ToolError::not_available`）→ `security_audit::run_semgrep_scan(backend, opts)` → 将 `Vec<SecurityIssue>` 经 `to_review_issue` 归一化输出 `ReviewIssue` 列表
- `tools/mod.rs` 导出 `pub mod security_audit_tool;`，在 `tool_setup.rs:94-99` 链上加 `.with_security_audit_tools()`（需在 `registry.rs` 新增 builder 方法，仿 `with_run_poc_tools` L718）

**Doc ref**：`security_audit.rs:40-101`（API）、`run_poc.rs:135-159`（fail-closed 沙箱范式）、`call_graph.rs:24-37`（ToolSpec 范式）、`registry.rs:718`（with_*_tools 方法范式）。

**Verification**：
- `cargo build -p mimofan-tui` 零 warning
- grep 确认 `run_semgrep_scan` 在 TUI 有真实调用方（排除 security_audit.rs 自身与单测）
- `cargo test -p mimofan-tui security_audit` 含既有单测 + 新增注册单测

**Anti-pattern guards**：不新增命令行参数；不直接 `Command` 调 semgrep（必须走 SandboxBackend）；**不改动 `security_audit.rs` 现有三个函数签名**；工具名用 `security_audit`。

---

## Phase 2：接线 `recon` + `attack_surface` 为 `attack_surface` 工具 ⭐推荐

**What**：把已就绪的并行侦察编排器与攻击面枚举引擎暴露为工具，实现 AVDH **Threat Model / Entry Point Discovery** 雏形。

**Task**：新建 `crates/tui/src/tools/attack_surface_tool.rs`：
- `AttackSurfaceTool`，`name = "attack_surface"`，`ReadOnly`+`Auto`
- `input`：`{ target_dir, include_implied_autotype? }`
- `execute`：
  1. 构造 `ReconBudget { max_parallel: 4, worktree_root: Some(target_dir), token_budget: None }`
  2. 用 `recon::run(budget, caps)` 并行跑两个 `ReconCapability`：`attack_surface::enumerate_surface` 与 `kb_trace::trace_chains`（封装成 impl `ReconCapability`）
  3. `recon::dedupe()` 去重后输出 `SecurityIssue` 列表（含 `AttackSurfaceKind` 分类）
- 注册 `.with_attack_surface_tools()`

**⚠️ 阻塞点（需先解决）**：`attack_surface::enumerate_surface` 依赖 `sca::Dependency/Advisory`，`scan_attack_surface` 依赖 `sca::OsvClient`。需确认：a) `sca.rs` 在 TUI 侧是否已接线（agent 1 报告称 `sca` 引擎未在 TUI 接线）；b) `enumerate_surface` 的调用方从哪拿 `deps`/`advisories`。**Phase 2 实施时先读 `tests/attack_surface_test.rs` 看测试如何构造这些输入**，据此设计工具的入参（可能需接受 `lockfile_path` 让工具内部调 `parse_lockfile` + `scan`）。

**Doc ref**：`recon.rs:28-99`、`attack_surface.rs:52-184`、`sca.rs:62-186`、`kb_trace.rs:95-110`、`tests/attack_surface_test.rs`（输入构造范例）。

**Verification**：
- `cargo build -p mimofan-tui` 零 warning（若 recon/attack_surface 依赖缺失 feature，确认 `staticanalysis/Cargo.toml` 默认 feature 覆盖）
- `cargo test` 零失败
- 冒烟：对 `benchmark/vuln_hunt/fixtures/bad/` 运行工具，确认能枚举出 sink/依赖告警

**Anti-pattern guards**：`recon::run` 是同步 `std::thread` 实现（staticanalysis 无 async runtime），TUI 侧用 `spawn_blocking` 包裹避免阻塞 executor；不硬接 `index.rs`。

---

## Phase 3：接线 `typestate` 为 `protocol_check` 工具 ⭐推荐

**What**：暴露协议状态机（FSM）检测，覆盖反序列化/会话类"非法状态序列"漏洞（如 `safeMode→readObject` 守卫绕过）。

**Task**：新建 `crates/tui/src/tools/protocol_check_tool.rs`：
- `ProtocolCheckTool`，`name = "protocol_check"`，`ReadOnly`+`Auto`
- `input`：`{ target_dir }`
- `execute`：`typestate::load_protocols_dir(dir)` → 从 callgraph 收集调用序列（仿 `call_graph.rs` 图遍历）→ 对每个 `ProtocolFsm` 跑 `check_sequence(&calls)` → 输出违规序列告警（`ProtocolViolation`）
- 注册 `.with_protocol_check_tools()`

**Doc ref**：`typestate.rs:43,80,158,208`、`rules/protocols/deserialization.yaml`、`call_graph.rs`（图遍历范式）。

**Verification**：build 零 warning；test 零失败；对 deserialization.yaml 语义做一次冒烟。

**Anti-pattern guards**：不新增 YAML 协议 schema；`check_sequence` 入参是 `&[(String, usize)]` 调用序列，调用侧需自行从 callgraph 收集。

---

## Phase 4：`vuln-hunt` skill 升级为 bundled + 补 `run.sh` ⭐必做

**What**：a) 让 `vuln-hunt/SKILL.md`（六步长程漏洞挖掘工作流）成为编译期内嵌 bundled skill；b) 补缺失的 `benchmark/vuln_hunt/run.sh`。

**Task**：
1. `system.rs`：加 `const VULN_HUNT_BODY: &str = include_str!("../../assets/skills/vuln-hunt/SKILL.md");`，`BUNDLED_SKILLS` 数组（L28-94）加 `{ name: "vuln-hunt", body: VULN_HUNT_BODY, introduced_in: 5 }`；`BUNDLED_SKILL_VERSION` 从 "5" 递增到 "6" 触发重新安装
2. 核对 SKILL.md 引用的工具名与实现一致（用全名：`hypothesis`/`gadget_chain_trace`/`run_poc`/`call_graph`/`ast_query`/`security_audit`/`attack_surface`/`protocol_check`）
3. 新建 `benchmark/vuln_hunt/run.sh`：`--selftest` + 批量跑 `tasks/*/task.json`（对齐 README.md:15,45 描述，仿 evaluate.py:320-347）

**Doc ref**：`system.rs:7-94`（BUNDLED_SKILL_VERSION/BUNDLED_SKILLS/is_bundled_skill_name）、`benchmark/vuln_hunt/README.md:15,45`（run.sh 契约）、`evaluate.py:320-347`（批量模式）。

**Verification**：
- build 零 warning
- 单测：`is_bundled_skill_name("vuln-hunt") == true`
- `/skills` 列表出现 vuln-hunt
- `bash benchmark/vuln_hunt/run.sh --selftest` 通过

**Anti-pattern guards**：不把 skill 正文重复硬编码，只 `include_str!` 接线；不改 SKILL.md 工作流内容（除非工具名对不上）。

---

## Phase 5：AVDH 启发的新能力（按优先级，避免重复造已存在能力）

### 5a. Access-Control 假设分析 ⭐高价值（填补结构性缺口）
- AVDH 的 **Access Control agent**（判断权限检查缺失/用错身份）当前**无静态分析器对应**。
- 提议：在 `staticanalysis` 新增 `access_control.rs`，基于 `callgraph` 分析某入口点是否经过授权 gate（`role`/`permission`/`admin`/`auth` 相关调用），输出 `SecurityIssue{category:"Access-Control"}`。
- **约束**：规则层目前全是 sink 类（java/rust/nextjs.yaml），grep `auth|permission|role` 仅命中 fastjson autotype deny（非授权 gate）。需新增规则 schema 或硬编码 gate 模式。接入 `recon` capability 与 Phase 2 `attack_surface` 工具。

### 5b. Multi-Validation Synthesis（AVDH 高温多验证 → 综合判定）
- 当前 `hypothesis` 单证据门（hypothesis.rs:366 单一 verdict + :375-384 零证据拒绝），无多验证综合。
- 提议：为 `hypothesis resolve` 增加可选 `synthesis` 字段，接受多条 evidence/verdict，综合给出 confirmed/disproven/rejected 三态（仿 `subagent/aggregator.rs` 的 Vote/quorum 策略，但需在 hypothesis.rs 内实现，不硬耦合 subagent）。
- 评估：与现有一致性门（零证据禁止 resolve）兼容，作为增强而非替换。

### 5c. Distilled Knowledge 层级强化（domain 入口点规则）
- 现有 rules/ 已按语言/框架（java/rust/nextjs）+ 漏洞（kb/gadgets.yaml）+ 协议（protocols/deserialization.yaml）分层，基本匹配 AVDH 三级规则。
- 提议：补充 **domain 级入口点定义规则**（web 路由、IPC listener 识别），供 Entry Point Discovery 编排使用。新增 `rules/entrypoints/web.yaml` 等，schema 复用 `rules.rs::load_rules_dir` 或新建。

### 5d.（延后）Threat-Model 审批门 + 可视化
- AVDH 在 Threat Model 阶段设人类审批门 + 文本/可视化。依赖 TUI 交互，成本高。建议仅输出结构化威胁模型，审批门延后。

### 5e.（延后）Benchmark FP-triage / Duplicate-resolution agent
- evaluate.py 现为确定性评分。可在 benchmark 侧补 LLM 判卷 agent（可选，价值较低）。

---

## Phase 6：最终验证

1. **全量构建**：`cargo build --workspace` 零 error、零 warning
2. **全量测试**：`cargo test --workspace` 零失败（memory 提示：并行 agent 下用 `--no-run` + 直跑测试二进制规避 file lock；`CARGO_TARGET_DIR` 隔离避免抢锁）
3. **接线自检 grep**：
   - `run_semgrep_scan` / `scan_attack_surface`（或 `enumerate_surface`）/ `check_sequence` 在 `crates/tui` 均有真实调用方
   - `is_bundled_skill_name("vuln-hunt") == true`
   - `benchmark/vuln_hunt/run.sh` 存在
4. **Anti-pattern grep**：无 `std::process::Command` 直接调 semgrep；无重复 semgrep 封装；工具名全用 `gadget_chain_trace`/`auto_gadget_discovery` 全名
5. **冒烟**：对新工具 input/output 结构做 `cargo test` 驱动验证

---

## 结论要点（供评审）

- **必做**（明确死代码/文档谎言/缺口，风险高）：
  - Phase 1（semgrep 工具接线，消文档谎言）
  - Phase 4（vuln-hunt skill 接线 + run.sh 补齐）
- **推荐**（引擎已就绪、零新增引擎成本）：
  - Phase 2（recon+attack_surface，注意 sca 依赖阻塞点）
  - Phase 3（typestate）
- **选做**（AVDH 启发、需新引擎）：
  - Phase 5a（Access-Control）、5b（Multi-Validation）、5c（domain 入口点规则）
- **延后**：5d（审批门可视化，依赖 TUI 交互）、5e（benchmark 判卷 agent）

> **初稿修正摘要**：工具名 `gadget_chain`→`gadget_chain_trace`、`auto_gadget`→`auto_gadget_discovery`；`IndexDb`→`SymbolIndex`；hypothesis 不在 tool_setup 链而是在 agent policy 注册；新增缺失的 `benchmark/vuln_hunt/run.sh`；Phase 2 补 sca 依赖阻塞点说明。
