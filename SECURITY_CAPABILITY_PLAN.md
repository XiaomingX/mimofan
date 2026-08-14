# 网络安全能力提升规划（SAST / 漏洞挖掘 / 静态分析 / 沙箱隔离）

> 本规划从 GitHub issue 中筛选「网络安全能力提升」类 OPEN 待办，经源码权威核验（grep / 读文件）后细化，便于后续直接派工。
> **核对纪律（复用 `ARCHITECTURE_IMPROVEMENT_PLAN.md`）**：本文档所有「已落地」结论均以 `grep` / 读源码亲核 `main` 分支代码为准，不采信二手对标清单。若发现本文档与代码不符，先 `git grep` 复核再改文档。

最后更新：2026-08-14

---

## 0. 已完成前提（不要再派工，仅作下游依赖底座）

这些项已真实落地，相关任务**不要再列进待办**，但其能力是后续任务的依赖或参照：

- **#601 密钥凭据泄漏分析** — `crates/secrets/src/scanner.rs`
  - `SecretKind` 枚举（行 19-51：AWS/GitHub/Google/Slack/SSH/JWT/PrivateKey/Generic）
  - `scan_line`（行 102）、`scan_generic_assignment`（行 203）、`is_sensitive_content`（行 248）
  - `redact_stream`（行 257-303）流式密钥去敏，跨 chunk 合并
  - 调用点：`file.rs` / `edit.rs` 写入前 `is_sensitive_content` 拦截
- **#618 macOS Seatbelt** — `crates/tui/src/sandbox/seatbelt.rs`（`create_seatbelt_args` 行 117）、`mod.rs:189` `MacosSeatbelt` 变体。注意 **Linux Landlock 仍缺失**（见 T-14）。
- **#587 AST 检索 ast_query（部分）** — 工具已注册（`registry.rs:583`）、`tools/ast_query.rs` 完整 schema+execute+单测；但底层 grammar **仅 Rust**（见 T-2 缺口）。

---

## 1. 能力地图（Epic → issue 依赖关系）

```
                    [#711] 容器沙箱+凭据池  ──┐ (PoC 验证硬前置)
                            │                │
  底座层: [#587] AST多语言 ─┬─→ [#589] callgraph工具 ─┐
         [#593] 符号索引   ─┘                        │
                                                    ↓
  分析引擎层: [#588/#606] 污点分析 ─→ [#590] 函数摘要+typestate
             [#591] 外部分析器+SARIF
             [#592] 安全persona+semgrep插件
                                                    │
  编排/验证层: [#595] 并行侦察编排 ←─ [#594] 可达性剪枝+沙箱PoC (依赖[#711])
             [#599] SCA/OSV
             [#610/#611/#612] Gadget链/PoC生成/漏洞知识库
                                                    │
  评测层: [#603] D13 漏洞挖掘评测域
  隔离层: [#618-Linux] Landlock   [#617] 插件沙箱(已落地: plugins.rs 经 SandboxManager 路由)
```

依赖箭头自洽：**T-4 污点分析依赖 T-2 + T-3**；**T-9 沙箱 PoC 依赖 T-1 容器沙箱**；**T-1 排在最前**（无本地强隔离则 PoC 验证不可开工）。

---

## 2. 可执行任务清单

### T-1. [#711] 容器沙箱后端（podman/docker）+ 凭据池  — P1，排最前

- **为什么**：无本地强隔离，不可信代码执行与漏洞 PoC 验证（#594/#611）只能在宿主机裸奔或送出本机的远程 OpenSandbox，多数审计场景不可接受；恶意 skill 可经环境变量读走所有 API key。
- **落点文件**：
  - `crates/tui/src/sandbox/backend.rs` 增 `SandboxKind::Container` 变体 + `create_backend()` 接线
  - 新增 `crates/tui/src/sandbox/container.rs` 实现 `SandboxBackend` trait
  - 凭据池：在 `sandbox/` 内新增 env 白名单 + 临时凭据发放逻辑，与 `crates/secrets` 打通（复用 #601 的 `redact_stream` 防落盘/日志）
- **依赖**：无；被 #594 / #611 / #617 依赖
- **验收标准**（每项均 grep / `cargo test` 可验证）：
  - [ ] `SandboxKind::Container` 已实现并在 `create_backend()` 接线
  - [ ] podman / docker 运行时自动探测；两者皆缺时**显式报错而非静默降级**（单测覆盖此路径）
  - [ ] 默认配置下容器内**无网络**（`--network=none`）、工作区只读、非 root（各有断言测试）
  - [ ] 超时与内存上限生效：构造死循环 / 内存炸弹 fixture，验证被杀且宿主无影响
  - [ ] 子进程默认不继承宿主 env：构造读 `ANTHROPIC_API_KEY` 的脚本，验证读不到；白名单机制生效
  - [ ] 网络与可写挂载需**显式逐项开启**，无 "allow all" 快捷开关
- **反模式**：不要提供 "allow all" 开关；不要静默降级为无沙箱；不要重复造 `SandboxBackend` trait（已存在）；凭据不要落盘、不要进日志。

### T-2. [#587] 多语言 AST grammar 补齐  — P0 底座

- **为什么**：当前 `ast_query` 仅 Rust 可用，其余 5 语言 grammar 未编译（`lib.rs:112` 非 Rust 返回 `Unsupported`），漏洞挖掘所需的结构化查询对 Java/TS/Kotlin/Swift/ObjC 全不可达。
- **落点文件**：
  - `crates/staticanalysis/Cargo.toml` 按语言 feature gate 增 `tree-sitter-java/tsx/javascript/kotlin/swift/objc`（避免默认全量编译拖慢 `cargo build`）
  - `crates/staticanalysis/src/lib.rs` 接对应 grammar，补全命名查询库（`java.sink.runtime_exec`、`webext.manifest.broad_host_permissions` 等）
  - `tools/ast_query.rs:103` 描述里写的预设需与 `named_query` 实际实现一致（当前预设已删，会返回 unknown preset 错误）
- **依赖**：无
- **验收标准**：
  - [ ] `ast_query` 对 6 语言均返回命中（构造每种语言最小样例 fixture）
  - [ ] 命名查询库列出的预设均可按名调用
  - [ ] 全量编译时间不因默认 feature 失控（`cargo build` 不显著变慢）
- **反模式**：不要全量无条件编译所有 grammar（构建时间爆炸）；不要把 `ast_query` 放错 crate（真正的工具在 `crates/tui/src/tools/`，不是 `crates/tools`）。

### T-3. [#589] call graph 暴露为模型工具 + 多语言引用解析

- **为什么**：`crates/staticanalysis/src/callgraph.rs` 已有 `CallGraph` 结构 + `reachable_from`，但**未注册为模型工具**（`registry` 零命中）；无 Kotlin/Swift/ObjC；污点分析需要 O(1) 调用可达性查询。
- **落点文件**：
  - 新增 `tools/call_graph.rs` 包装 `CallGraph` 为 `ToolSpec`，在 `registry.rs` 注册
  - LSP 引用查找已独立落地（`lsp_symbols.rs:160/254/93`），可复用其精度做在线兜底；离线场景用 #593 符号索引
  - 多语言引用解析依赖 T-2 的 grammar
- **依赖**：T-2
- **验收标准**：
  - [ ] `call_graph` 工具已注册，模型可查询「从 X 可达的所有调用」
  - [ ] `reachable_from` 对至少 Java/Rust 返回正确可达集（构造 fixture）
  - [ ] 无 Kotlin/Swift/ObjC 时显式提示不支持，而非崩溃
- **反模式**：不要把通用 LSP 引用查找当成 SAST call-graph 工具（精度/离线不同）；不要重复造 `CallGraph` 数据结构。

### T-4. [#588 / #606] 污点分析引擎（source/sink/sanitizer + 跨函数传播）

- **为什么**：漏洞挖掘核心问法是「不可信数据能否在未经净化情况下抵达危险操作」。当前 `staticanalysis/` 无任何 taint/sanitizer/source/sink 规则引擎，只能靠模型「看着像有问题」——无证据链、不可复现。
- **落点文件**：`crates/staticanalysis/src/taint.rs`（新）+ `rules/`（声明式 YAML 规则库）
- **依赖**：T-2（多语言 AST）、T-3（call graph 可达性）
- **验收标准**：
  - [ ] 声明式 YAML 规则三要素（sources / sinks / sanitizers）可加载；sanitizer 支持「部分净化」（`neutralizes: [xss]` 而非布尔开关）
  - [ ] 支持 propagator（拼接/集合读写/模板字符串的污点流动）与 field-sensitivity（至少一层字段区分）
  - [ ] 对 C3P0 gadget chain 输出从 `jndiName` → `InitialContext.lookup()` 完整传播路径并标注每步规则（`#606` 验收）
  - [ ] `isAutoTypeDenyClass` 识别为强净化，DataSource 类型 `@type` 被阻断
- **反模式**：不要把 `tools/schema_sanitize.rs`（JSON schema 清洗）误当安全 sanitizer；不要硬编码规则进 Rust（必须 YAML 可扩展 + 热更新）。

### T-5. [#590] 函数摘要 + typestate 建模

- **为什么**：跨函数组合爆炸与时序类漏洞（如对象生命周期 / 反序列化协议状态）无法仅用点状污点解决，需函数级摘要与状态机建模。
- **落点文件**：`crates/staticanalysis/src/summary.rs` + `typestate.rs`（新）
- **依赖**：T-4
- **验收标准**：
  - [ ] 函数摘要可标注副作用 / 构造器 / getter / 增量更新
  - [ ] typestate 可建模对象生命周期与 Feature 交互 FSM，捕捉时序类漏洞
- **反模式**：不要试图一次性全量函数摘要（先覆盖高危 sink 周边）。

### T-6. [#591] 外部分析器接入 + SARIF 归一化

- **为什么**：需把 semgrep / 各类外部 SAST 工具输出统一为内部格式，消除命令白名单 / Seatbelt / execpolicy 摩擦。
- **落点文件**：`crates/staticanalysis/src/sarif.rs`（新，SARIF 解析/归一化）
- **依赖**：无（命令白名单已在 `crates/execpolicy/` 独立存在，可复用）
- **验收标准**：
  - [ ] 能解析外部分析器 SARIF 输出并归一化为内部 `security_issues` 结构
  - [ ] grep `sarif` 在 `staticanalysis` 有实现命中（当前零命中）
- **反模式**：不要把 execpolicy 命令白名单当成 SARIF 归一化（二者不同）。

### T-7. [#592] 安全审计 persona + skills + semgrep 插件

- **为什么**：零改码验证路线需要先有安全审计 persona 与可加载的 semgrep 规则插件，建立效果基线。
- **落点文件**：`crates/tui/src/prompts/`（安全审计 persona，参照现有 skill 结构）+ skills 目录新增 security-audit
- **依赖**：T-6（SARIF 接口）
- **验收标准**：
  - [ ] grep `semgrep` 全仓有真实调用（当前除注释外零命中）
  - [ ] 安全审计 persona 可独立运行并产出结构化发现
- **反模式**：不要重复造已有的 `reviewer` 通用评审模块，应复用其结构。

### T-8. [#593] SAST 专用符号索引持久化

- **为什么**：污点 / call graph 每次全库重解析成本不可接受；需 symbols/imports/refs/files 四表持久化 + 增量失效。**注意 `crates/memory` 的 embedding 只服务对话记忆，不是代码索引**（#593 易误判点）。
- **落点文件**：`crates/staticanalysis/src/index.rs`（新）+ SQLite 存储
- **依赖**：T-2
- **验收标准**：
  - [ ] symbols / imports / refs / files 四表持久化
  - [ ] 以 (内容 hash, mtime) 判定增量更新；单文件变更只重建该文件符号及反向引用
  - [ ] 后台增量更新不阻塞回合
- **反模式**：不要复用 memory 的 RAG 索引当成代码索引；不要全量重扫（必须有增量失效）。

### T-9. [#594] 可达性剪枝 + 沙箱 PoC + review.security_issues 升级

- **为什么**：去误报必须实际触发 payload 验证真阳性；当前无可达性剪枝、无沙箱 PoC、无 `review.security_issues` 升级。
- **落点文件**：`crates/tui/src/reviewer/` 升级 `security_issues` 字段；新增 `run_in_disposable_sandbox()` 入口（复用 T-1）
- **依赖**：T-1（容器沙箱）、T-4（污点）、T-3（可达性）
- **验收标准**：
  - [ ] 可达性剪枝后误报率下降（有量化基线）
  - [ ] 可在可丢弃隔离环境触发 payload（`run_in_disposable_sandbox` 接线 #611 PoC）
  - [ ] `review.security_issues` 能承载结构化证据链而非 LLM 自由输出
- **反模式**：绝对不要在宿主机跑攻击 payload；不要跳过 T-1 直接开工。

### T-10. [#595] 多攻击面并行侦察编排

- **为什么**：无侦察编排器，多攻击面只能串行手工；需编排器调度 T-2~T-9 各能力并行侦察。
- **落点文件**：新增 `tools/recon.rs` 编排工具（复用 `subagent/manager.rs` 的 budget 与 worktree 隔离）
- **依赖**：T-4~T-9 各能力
- **验收标准**：
  - [ ] 可并行调度多个攻击面侦察任务，结果聚合
  - [ ] 修复工具注册双轨制（与 #604 协同）
- **反模式**：不要重新造 workflow-budget（已存在，见 QUEUE 文档 T-Q1）。

### T-11. [#599] 依赖清单 SCA（OSV 比对 + 可达性判定）

- **为什么**：无 OSV 客户端、无依赖树解析比对，无法做软件成分分析与可达性剪枝。
- **落点文件**：`crates/staticanalysis/src/sca.rs`（新，OSV 客户端 + lock 解析）
- **依赖**：无
- **验收标准**：
  - [ ] grep `osv|advisory` 有实现命中（当前零命中）
  - [ ] 解析 lock 文件 → 比对 OSV → 输出可达性判定
- **反模式**：不要只做版本号比对不做可达性（误报爆炸）。

### T-12. [#610 / #611 / #612] Gadget 链枚举 / PoC 生成+E2E 沙箱评级 / 漏洞知识库+Gadget 模式库

- **为什么**：攻击面枚举、可利用性验证、知识沉淀是漏洞挖掘闭环的三块，当前全缺失。
- **落点文件**：
  - `#610` 攻击面枚举：`crates/staticanalysis/src/attack_surface.rs`
  - `#611` PoC 生成 + E2E 沙箱（LDAP/RMI/DNS canary）：依赖 T-1 + T-9
  - `#612` 漏洞知识库 / Gadget 模式库 / 报告自动生成：新增知识存储
- **依赖**：T-9（沙箱 PoC）、T-4（污点）、T-11（SCA）
- **验收标准**：
  - [ ] Gadget 链发现 + 依赖指纹 + 隐式 autoType 识别（#610）
  - [ ] PoC 生成 + canary 评级（#611）
  - [ ] 漏洞知识库可检索 + 报告自动生成（#612）
  - [ ] grep `gadget|vuln.*db` 有实现命中（当前零命中）
- **反模式**：不要在没有隔离的情况下生成并执行 PoC（依赖 T-1）。

### T-13. [#603] D13 漏洞挖掘评测域

- **为什么**：缺量化评测则无法证明能力提升；需挂载现有 agentbench + 扩展 `check.kind` 支持 TP/FP 度量。
- **落点文件**：`benchmark/agentbench/` 扩 D13 域 + `check.kind` 扩展
- **依赖**：T-4~T-12（需能力先落地才能评）
- **验收标准**：
  - [ ] D13 域挂载现有 agentbench 样本
  - [ ] `check.kind` 支持 TP/FP 度量，输出误报率/漏报率
- **反模式**：不要在能力未落地前虚报评测分数。

### T-14. [#618-Linux] Landlock / bwrap 实现

- **为什么**：`sandbox/mod.rs:10` 注释声称 "Linux: Uses Landlock"，但目录无 `landlock.rs` 实现文件（`grep "landlock"` 非注释行零命中）；`prefer_bwrap` 仅有偏好开关无路由执行代码。macOS Seatbelt 已落地，Linux 缺位。
- **落点文件**：新增 `crates/tui/src/sandbox/landlock.rs`（或 `bwrap.rs`）实现 `SandboxBackend`
- **依赖**：无
- **验收标准**：
  - [ ] Linux 上 Landlock/bwrap 实际生效（非仅注释）
  - [ ] 与 T-1 容器沙箱互补（本地轻隔离 vs 强隔离）
- **反模式**：不要只改注释宣称已实现；不要与 Seatbelt 逻辑混淆。

### T-15. [#617] 插件执行路径切到沙箱  — ✅ 已落地

- **复核结论（实施期）**：`commands/plugins.rs:300` 已通过 `SandboxManager::new()` → `prepare(&spec)` 将命令转成受限制 `ExecEnv`（清空宿主环境 + 临时凭据 + 可用时 Landlock 预执行），随后才 `std::process::Command::new(exec_env.program())`。原规划担心的"裸 spawn"已不存在，T-15 视为完成。
- **为什么**：从 GitHub 安装的 skill 解压期有防护（zip-slip / 符号链接 / gzip bomb），执行期现已隔离；`tools/js_execution.rs` 同样在沙箱可用时走 `SandboxManager` 由 T-1/T-14 覆盖。
- **落点文件**：`commands/plugins.rs:294-343`、`tools/js_execution.rs`
- **依赖**：T-1 / T-14
- **验收标准**：
  - [ ] 插件解释器与 JS 执行在沙箱可用时经 `SandboxManager` 启动
  - [ ] 凭据不泄漏给插件子进程（复用 T-1 凭据池）
- **反模式**：不要把 execpolicy 校验当成沙箱隔离（前者只管命令策略，不管隔离）。

---

## 3. 风险与顺序建议

1. **T-1 容器沙箱必须最先做** —— 没有本地强隔离，T-9/T-11/T-12 的 PoC 验证不可开工，且 T-15 的插件隔离也依赖它。
2. **底座优先** —— T-2/T-3/T-8（AST 多语言 + call graph + 符号索引）是 T-4 污点分析的前置，应先于分析引擎层。
3. **状态与磁盘不符风险** —— T-15(#617) 标 CLOSED 但裸 spawn，实施前先复核；T-14(#618) 注释宣称 Landlock 实现但无代码，勿被注释误导。
4. **勿重复造轮子** —— `SandboxBackend` trait、`workflow-budget`、通用 `reviewer`、memory RAG 索引均已存在，对应任务应接线/复用而非重建。
5. **评测殿后** —— T-13(D13 评测域) 依赖全部能力落地，放最后。
