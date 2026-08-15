# 需求待办（OPEN Issue 梳理，2026-08-14）

> 数据来源：`gh issue list --state open --limit 300`，共 **43** 个 OPEN issue（编号 598–777，跳跃为已关闭项）。逐项 `gh issue view` 取证，代码落地状态以 `git` 工作树 `grep`/读源码核验为准（参考 `SECURITY_CAPABILITY_PLAN.md`、`QUEUE_CAPABILITY_PLAN.md`）。

> **迁移说明（2026-08-15）**：原 43 个 OPEN issue 经 triage 判定均为「抽象需求→需细化可实施步骤+验收口径」（规则3），已全量细化为含「来源/可实施范围/验收口径」的新 issue 并关闭原 issue。原 `#598`→`#790`、`#600`→`#791`、…、`#777`→`#832`（连续一一对应，详见 `/tmp/triage_run.log`）。本文档来源 issue 编号已同步更新为新编号；第三章「疑似已落地」条目对应新编号仍需按原证据复核。

## 一、分层总览

| 层 | 说明 | 需求条数 | P0 | P1 | P2 |
|----|------|---------|----|----|----|
| L0 | 基础底座（编译/稳定性/可观测/测试基建） | 8 | 0 | 6 | 2 |
| L1 | 核心能力（长程任务/记忆/安全漏洞挖掘/模型路由） | 18 | 1 | 12 | 5 |
| L2 | 体验与生态（UX/IDE/插件/多端/可观测接入） | 8 | 0 | 7 | 1 |
| L3 | 评测与对标（benchmark/记分卡/回归基线） | 3 | 0 | 3 | 0 |
| **合计** | | **37** | **1** | **28** | **8** |

## 二、需求明细（按层 L0→L3、层内按 P0→P2 排列）

### L0 基础底座

#### [L0-1] trace_id 跨模块调用链贯穿 `P1` `observability,#54`
- **来源 issue**：#799
- **问题/动机**：engine→tool→mcp→subagent 无统一 trace 上下文，长任务排障无法串联路径。
- **可实施范围**：引入 `trace_id`/`correlation_id` 在回合主循环生成，透传至工具调用、MCP 请求、子 agent 派生；跨进程 MCP 参照 W3C trace context 透传。
- **验收口径**：构造一次跨工具+子 agent 的调用，所有 span 共享同一 `trace_id`（可在日志/结构体断言）；grep `trace_id` 在 engine/tool/mcp 有透传命中。
- **依赖/风险**：涉及主循环与所有工具签名，热路径改造；需与 #796/#830 可观测体系协同。

#### [L0-2] 记忆可观测 Stats（注入 token / last recall / 整合成本） `P1` `memory,#5`
- **来源 issue**：#796
- **问题/动机**：记忆系统无任何维度统计，无法判断召回质量/注入成本/陈旧度。
- **可实施范围**：新增 `MemoryStats`（注入 token 数、last recall 时间、整合次数/成本、条目数），在 status/debug 命令暴露，并供 #830 OTel 指标桥接。
- **验收口径**：debug 命令输出含上述字段；`MemoryStats` 有单测与真实写入点。
- **依赖/风险**：依赖记忆子系统数据埋点；纯增量不破坏检索路径。

#### [L0-3] compaction 事实保留率断言 `P1` `compaction,#11`
- **来源 issue**：#797
- **问题/动机**：`compaction/mod.rs`（~1626 行）无关键事实保留率断言，压缩可能静默丢事实。
- **可实施范围**：构造含已知关键事实的会话，压缩后断言事实仍在；补保留率评测维度，纳入压缩回归测试。
- **验收口径**：新增保留率断言测试，已知事实压缩后命中率 ≥ 阈值（如 0.95）；CI 中可跑。
- **依赖/风险**：需定义"关键事实"提取器（与 #832 评测归因协同）。

#### [L0-4] 核心工具层测试覆盖门禁 `P1` `testing,#85`
- **来源 issue**：#798
- **问题/动机**：625 个 .rs 文件仅 88 个有测试，工具层（git/github/apply_patch/registry）零测试，重构易静默回归。
- **可实施范围**：优先为高频/高风险工具补单测与集成测试；建立覆盖门禁（CI 阈值或报告）。
- **验收口径**：git/github/apply_patch/registry 注册均有测试；全 workspace `cargo test` 绿。
- **依赖/风险**：测试基建，不阻塞功能；与 #802 拆分同属可维护性债。

#### [L0-5] 文件写操作统一 VFS（路径穿越防护） `P1` `security,#35`
- **来源 issue**：#800
- **问题/动机**：`crates/tui/src` 有 148 处直写 `std::fs`，手写路径校验易遗漏，可越权写 workspace 外。
- **可实施范围**：引入沙箱 VFS 抽象，所有写经其校验 workspace 边界+路径规范化；逐步收口 148 处直写（先高危工具，后低风险）。
- **验收口径**：构造 `../../etc/xxx` 写请求被 VFS 拒绝（单测）；高危工具路径全部经 VFS。
- **依赖/风险**：与安全沙箱（#618/#711）强相关；热路径改造，需兼容符号链接等合法场景。

#### [L0-6] context engine trait 抽象（检索/组装可插拔） `P1` `architecture,#69`
- **来源 issue**：#801
- **问题/动机**：compaction/context 实现硬编码，无 trait 抽象，替换策略需改核心代码。
- **可实施范围**：抽取 `ContextEngine` trait（compress/retrieve/assemble），内置实现作默认，留插件插槽。
- **验收口径**：内置实现满足现有行为；新增一个示例插件（或单测 mock）证明可插拔。
- **依赖/风险**：重构核心上下文路径，需回归测试护航（依赖 #798）。

#### [L0-7] 拆分超大 TUI 文件 `P2` `refactor,#57`
- **来源 issue**：#802
- **问题/动机**：`ui_event_loop.rs`(4299 行)/`sidebar.rs`(3008 行) 过大，review/重构风险高。
- **可实施范围**：按职责拆分子模块（渲染协调/事件分发/视图），拆分后补测试，零行为变更。
- **验收口径**：单文件行数下降、功能等价、相关单测通过。
- **依赖/风险**：纯可维护性，风险低；可独立 worktree。

#### [L0-8] OpenTelemetry/OTLP + Prometheus 接入（切片 B–D） `P2` `observability,R28`
- **来源 issue**：#830
- **问题/动机**：切片 A（feature-gated OTel 桥接 crate）已合并（PR #772），但 GenAI 语义 span、token metrics、Prometheus `/metrics`、endpoint 配置均未做。
- **可实施范围**：导出 LLM 调用延迟/token 的 GenAI 语义约定 span（接 `cost_status`）；Prometheus exporter 暴露 `/metrics`；OTLP endpoint 配置项+启动接线。
- **验收口径**：配置 endpoint 后 `/metrics` 可抓取核心指标（token/延迟/工具调用/记忆检索）；B–D 切片各自单测/集成测试通过。
- **依赖/风险**：best-effort 不阻断主流程；切片 A 已落地，本项只做上层导出。

### L1 核心能力

#### [L1-1] 经验学习闭环（会话结束自动蒸馏回写记忆） `P1` `memory,A1,#809`
- **来源 issue**：#809（合并 #812 PTC、#811 /learn、#810 Curator、#813 rollout 导出、#814 blueprint 同属 Epic #658 经验/训练闭环族，见合并说明）
- **问题/动机**：写记忆唯一路径是 `remember` 工具（模型主动调用），无会话结束自动总结/回写机制。
- **可实施范围**：会话/任务结束后自动蒸馏经验（成功路径/踩坑/修复模式）回写为可检索记忆（`Observation`）；与 #831 UserProfile 注入协同。
- **验收口径**：跑完一个任务后，相关经验可被跨会话召回（端到端测试）；回写不污染主上下文。
- **依赖/风险**：依赖记忆子系统与 #831；需防回写噪声（与 #829 衰减协同）。

#### [L1-2] 用户建模层 UserProfile（跨会话用户画像） `P1` `memory,#831`
- **来源 issue**：#831
- **问题/动机**：无跨会话用户画像，无法沉淀"用户是谁/如何协作"的稳定信息。切片 A（`user_profile.rs`）已合并（PR #770）。
- **可实施范围**：切片 B 从对话蒸馏画像 + 切片 C 注入系统提示 + 切片 D 衰减豁免；明确写入时机与常驻/检索注入策略。
- **验收口径**：UserProfile 持久化且跨会话可见；用户纠正后 profile 更新而非追加矛盾条目（单测）；衰减对其豁免。
- **依赖/风险**：切片 A 已落地，剩 B–D；注入需 token 预算约束（与 #796 协同）。

#### [L1-3] 记忆巩固与遗忘（重要性/衰减/淘汰/去重/rollup） `P1` `memory,M4,#829`
- **来源 issue**：#829（与 #816 记忆模块合并、#828 混合检索、#796 可观测同属记忆体系）
- **问题/动机**：记忆只增不减，长期运行无限膨胀、噪声累积。切片 A（consolidation.rs 数据结构）已合并（PR #773），但衰减/淘汰/去重合并/rollup 未做。
- **可实施范围**：切片 B 指数时近衰减+容量上限 LRU 淘汰；切片 C 近重复去重合并；切片 D 情景→语义 rollup；切片 E 定时任务+单测。
- **验收口径**：`cargo test -p mimofan-memory` 覆盖 decay/淘汰/去重/rollup；离线 benchmark 容量与召回稳定。
- **依赖/风险**：切片 A 已落地；去重逻辑需收敛到单一实现（与 #816 协同）。

#### [L1-4] 混合检索四路融合（向量+全文+时近+重要性） `P1` `memory,M3,#828`
- **来源 issue**：#828
- **问题/动机**：当前纯向量单路召回，质量受限。切片 A（score_breakdown+FTS5 RRF）与切片 B（RetrievalHit 统一类型）已合并（PR #767/#771），但时近/重要性信号与四路 RRF 融合未做。
- **可实施范围**：切片 C 加 `indexed_at` 时近衰减信号；切片 D 加重要性信号（被引用/修改频次）；切片 E 四路 RRF 融合+端到端单测。
- **验收口径**：四路融合后 recall 在样本集提升（可量化）；`score_breakdown` 跨源可解释。
- **依赖/风险**：切片 A/B 已落地；真实语义 embedding 接入（见 #832 结论）是最大杠杆，但属独立配置项。

#### [L1-5] 合并分散记忆/去重代码到统一 mimofan-memory `P1` `refactor,memory,#816`
- **来源 issue**：#816
- **问题/动机**：记忆/去重逻辑散落 `crates/memory`、`turn_memory.rs`、`vector_memory/mod.rs` 三处，且 `Cargo.toml` 描述过时（称"未集成"，实际已接入）。
- **可实施范围**：修正 Cargo.toml 描述；下沉 `turn_memory.rs` 到 `crates/memory`；收敛 `vector_memory` 封装；统一两套去重算法；更新引用点。
- **验收口径**：`cargo build`（零 warning）+ `cargo test`（全 workspace 零失败）；tui 不再保留两套独立记忆实现。
- **依赖/风险**：多 crate 重构，建议独立 worktree（依 CODEBUDDY.md 约定）。

#### [L1-6] provider 电路熔断（Circuit Breaker）收尾接线 `P0` `reliability,#42,#795`
- **来源 issue**：#795
- **问题/动机**：跨 provider 故障转移已实现（`advance_fallback`），但连续失败 N 次后熔断/冷却某 provider 缺失。`circuit_breaker.rs` 状态机（llm_client + engine 两处）已落地并有单测，但 `advance_fallback` 主流程是否读取熔断状态待确认（impl_composer.rs grep 零命中）。
- **可实施范围**：将熔断状态机挂到 `advance_fallback` 的 readiness 过滤：Open 状态 provider 视为不可选；可恢复错误→对应 provider 失败计数+1，成功→重置；达阈值置 Open 并 cooldown；暴露 `circuit_breaker_status()` 指标（呼应 #799/#796）。
- **验收口径**：某 provider 连续失败达阈值后被暂时摘除，cooldown 后 HalfOpen 探测恢复（端到端/单测）；状态栏可见熔断状态。
- **依赖/风险**：**疑似部分落地**——状态机与引擎 mod 已存在，需先确认 advance_fallback 接线是否完整再判定是否关闭（见第三章）。不改动现有 fallback 语义。

#### [L1-7] 统一次级模型（secondary_model）抽象收敛 `P1` `model-routing,#807`
- **来源 issue**：#807
- **问题/动机**：次级模型功能已有但抽象分散（seam_model / cheap_tier / model_strength 三处），custom_agents `model=fast` 是未兑现承诺。`SecondaryModel` 枚举已存在（model_routing/mod.rs:53）并有单测，收敛进行中。
- **可实施范围**：config 顶层引入统一 `secondary_model` 段作为单一事实源；seam_model 与 cheap tier 回退到它；修复 custom_agents `model: fast/cheap/flash` 映射或删除假承诺。
- **验收口径**：单处配置后摘要与 `model:fast` 子代理都走该模型；custom_agents 的 fast 要么生效要么报错，不再静默透传。
- **依赖/风险**：阻塞 QUEUE 规划 T-Q5（次级模型自动策略）；需确认 `SecondaryModel` 是否已真统一三处。

#### [L1-8] 跨过程数据流求解器（worklist 不动点 + 格抽象） `P1` `sast,#596,#790`
- **来源 issue**：#790（合并 #794 状态机分析、#791 配置分析、#793 二进制分析、#792 密钥分析同属 MECE 漏洞挖掘体系；见合并说明）
- **问题/动机**：污点/typestate/可达性本质是同一 worklist 不动点算法，各写一遍会重叠。当前零可复用 CFG/求解器。
- **可实施范围**：构建 CFG（AST 出发，处理异常边）→ 通用 `solve<L: Lattice>(cfg, transfer, direction)` → 函数摘要（跨过程）。作为 #588/#590/#594 的公共底座。
- **验收口径**：CFG 构建 + worklist 求解器单测（不动点收敛、异常边正确）；上层三种分析复用同一底座。
- **依赖/风险**：分析引擎层核心，工作量大；需多语言 AST（依赖 #587 类 grammar，参考 SECURITY 规划 T-2）。

#### [L1-9] 状态机分析（对象生命周期/反序列化协议/Feature 交互 FSM） `P1` `sast,#C,#794`
- **来源 issue**：#794
- **问题/动机**：无法表达时序类漏洞（fastjson2 字段顺序依赖、`$ref` 绕过 setter 链）。C1 对象生命周期机制移交 #590 Part B，本 issue 保留 C2 反序列化协议 FSM + C3 Feature 交互矩阵 + C1 的 Java 场景验收。
- **可实施范围**：建模 fastjson2 token 流状态机、类型解析路径、`$ref` 时机；Feature 组合矩阵探索；端到端验收「setLoginTimeout 触发 JNDI 需 jndiName!=null」。
- **验收口径**：自动识别该前置条件；能表达「字段顺序不同导致利用成败不同」的时序性质。
- **依赖/风险**：阻塞于 #790 求解器 + #590 typestate；CFG 异常边对 CFG 正确性极敏感（Swift defer/Rust ? 易画错）。

#### [L1-10] 配置文件漏洞分析（AndroidManifest/Info.plist/manifest.json/CI） `P1` `sast,#791`
- **来源 issue**：#791
- **问题/动机**：大量高危漏洞在配置而非代码（exported 组件、过宽权限、ATS 例外），当前零能力。
- **可实施范围**：建声明式规则库覆盖 Android/iOS/浏览器插件/CI 配置；输出结构化发现（复用现有 security_issues 结构）。
- **验收口径**：对样本 manifest 检出 `exported=true` 无保护组件、`<all_urls>` 过宽权限等；规则可扩展。
- **依赖/风险**：纯规则匹配，成本可控；与 #792 密钥规则、#793 二进制解包复用。

#### [L1-11] LSP callHierarchy 补齐（递归调用链展开） `P1` `sast,ide,#827`
- **来源 issue**：#827
- **问题/动机**：find_references 已有，缺递归调用链展开（callHierarchy incoming/outgoing），阻塞 #589 可达性分析，也影响 IDE 体验。
- **可实施范围**：实现 callHierarchy 的 incomingCalls/outgoingCalls 递归展开；与现有 LSP 客户端集成；复用 find_references 基础设施。
- **验收口径**：对一个方法查询其完整调用链（调用者+被调用者递归）正确返回；单测覆盖递归边界与环。
- **依赖/风险**：依赖 LSP 客户端能力；自闭环，风险中等。

#### [L1-12] 密钥凭据泄漏分析生产化 `P2` `sast,#792`（疑似可关闭，见第三章）
- **来源 issue**：#792
- **问题/动机**：密钥泄漏检测原缺失，已落地 `crates/secrets/src/scanner.rs`（`scan_line`/`is_sensitive_content`/`redact_stream` + 单测），但 issue 仍 OPEN。
- **可实施范围**：若确认已落地则关闭；若需补 git 历史扫描/熵分析则单列。
- **验收口径**：见第三章证据。
- **依赖/风险**：`crates/secrets` 不可被其他分析模块复用（issue 已注明）。

#### [L1-13] 已编译产物分析（APK/IPA 外部工具编排） `P2` `sast,#793`
- **来源 issue**：#793
- **问题/动机**：移动端黑盒审计只有 APK/IPA，无源码，当前零能力。
- **可实施范围**：外部编排 apktool/jadx/otool 等解包 → 提取 manifest/strings/so → 喂给 #791 配置规则 + #792 密钥规则 + #588 污点（降级）。不自研反编译器。
- **验收口径**：APK 解包后 AndroidManifest 命中 #791 规则、strings 命中 #792 规则；聚合报告。
- **依赖/风险**：P2，依赖 #791/#792/#588 先落地；编排层成本可控。

#### [L1-14] 运行时插桩 + 运行时污点（MCP 外接） `P2` `sast,runtime,#805`
- **来源 issue**：#805
- **问题/动机**：现有漏洞挖掘全为静态，无运行时观测通道（反射/类加载/JNDI 只能间接推断）。
- **可实施范围**：做成可编排外部工具/MCP server（JVM `-javaagent` 基于 ByteBuddy/ASM，通用侧 strace/eBPF）；新增 `runtime_trace(pid|cmd, trace=[...])` 工具；运行时污点确认 sink 达。
- **验收口径**：attach fastjson 进程输出「实际调用 setLoginTimeout 而非 getter」；确认污点流入 `InitialContext.lookup`。
- **依赖/风险**：必须在隔离环境执行（依赖 #618 Linux 沙箱 / #711 容器）；建议 MCP 外接非内置。

#### [L1-15] 覆盖率反馈 + 语法感知模糊测试 `P2` `sast,fuzzing,#806`
- **来源 issue**：#806
- **问题/动机**：PoC/测试运行不采集覆盖率，无法指导补测。
- **可实施范围**：C3 先做（JaCoCo/llvm-cov 外部编排，`coverage_report(run_cmd)` 输出覆盖矩阵）；C4 探索性（接 Jazzer/AFL++ 做 coverage-guided fuzz）。
- **验收口径**：C3 跑 PoC 后输出「覆盖/未覆盖分支」；C4 可选产出触达新分支输入。
- **依赖/风险**：C3 性价比高优先；C4 标注探索性、命中不确定。

#### [L1-16] 假设→实证闭环（Hypothesis/Evidence/Verdict 一等公民） `P1` `harness,meta,#803`
- **来源 issue**：#803（与 #804 对抗验证同属 harness 元能力）
- **问题/动机**：无假设-验证结构化追踪，单链路自证易幻觉，跨会话丢失。
- **可实施范围**：新增 `Hypothesis{id,statement,status,evidence[]}` 一等类型 + 工具（create/add_evidence/resolve/list）；复用 task/goal 持久化层跨会话召回；证伪强制落库；可选与向量记忆联动。
- **验收口径**：agent 可登记假设、追加证据、给 verdict，`hypothesis_list` 按状态过滤；跨会话恢复可读。
- **依赖/风险**：纯结构化新增，复用现有持久化；不破坏核心循环。

#### [L1-17] 多代理对抗验证（find vs refute） `P1` `harness,meta,#804`
- **来源 issue**：#804
- **问题/动机**：现有 subagent 仅并行协作，无「一方提结论、另方独立证伪」的对抗语义。
- **可实施范围**：复用 subagent spawn+bus，新增 `adversarial_verify(claim, evidence_refs)`；反驳者子代理不共享提出者推理链；分歧时升级人工/表决；受 `max_spawn_depth` 约束。
- **验收口径**：对结论调用得独立反驳者裁决+理由；分歧正确升级/表决，不静默采信单方。
- **依赖/风险**：复用现有 subagent 编排，非从零；防递归爆炸。

#### [L1-18] 三个低成本工具壳（create_sub_session / record_artifact / 主会话 worktree） `P1` `tooling,#823`
- **来源 issue**：#823
- **问题/动机**：三项底座（ThreadRequest::Create/Fork、artifact 记录、git worktree 派生）基础设施均已就绪，但 59 个工具中无一暴露给模型，模型不可达。
- **可实施范围**：`create_sub_session`（模型可调用、fire-and-forget 与等首轮结果两种模式派生兄弟会话，区别于人类 `/fork`）；`record_artifact`（把产物登记为可检索 artifact，复用现有 artifact 结构）；`主会话 worktree`（模型触发为当前任务开独立 worktree）。
- **验收口径**：模型调用三者均能成功产生对应副作用（新会话/artifact 登记/worktree 创建）；端到端单测覆盖三种模式。
- **依赖/风险**：**实现成本低**（issue 自评"只差工具层暴露"）；注意与 `/fork` 语义区分，避免重复。

#### [L1-19] Arena 交互式多模型同题对决 `P2` `benchmark,qw,#825`
- **来源 issue**：#825
- **问题/动机**：对标 qwen `agents/arena/`：`/arena --models a,b,c` 每模型在独立 git worktree 并行跑同一任务，比 diff/轮次/token/耗时选胜者合并。现有 `evals/` 仅打裸 `/chat/completions` 端点（horizontal model comparison），**不跑 agent loop、不碰 git、不产 diff**，源码 `arena|best_of|tournament` 零命中。
- **可实施范围**：新增 `/arena` 命令：为各候选模型拉起独立 worktree + 跑真实 agent loop 完成同一任务；收集 diff/turns/token/time；生成 approach 摘要并选优合并。
- **验收口径**：`/arena --models a,b` 能并行跑完并产出可比报告（含 diff 与胜者建议）；不污染主工作区（每模型独立 worktree）。
- **依赖/风险**：需在 agent loop 之上编排多实例 + worktree 生命周期；与 #654 GoalQueue 复用；P2 因非主干能力。

### L2 体验与生态

#### [L2-1] IDE 上下文感知（visibleFiles/openTabs/光标位置） `P1` `ide,#70,#818`
- **来源 issue**：#818
- **问题/动机**：LSP 自拉起（仅注入自身编辑诊断）、ACP 仅 4 方法基线，完全无编辑器焦点感知。
- **可实施范围**：扩展 ACP 新增 editor-context 方法（visibleFiles/openTabs/diagnostics/selection），任何 ACP 宿主可接入；不必先做 VS Code 扩展。
- **验收口径**：ACP 实现 editor-context 方法且有 host 可消费；注入后模型无需靠检索猜「用户说哪个文件」。
- **依赖/风险**：需 ACP server 扩容（与 #820 协同）；不依赖特定 IDE。

#### [L2-2] Diff 逐行评论回灌（人→模型反馈通道） `P1` `ux,#68,#817`
- **来源 issue**：#817
- **问题/动机**：`diff_render.rs`（978 行）渲染完善，但缺人对 diff 逐行加评论并回灌模型的通道。
- **可实施范围**：diff 视图选行加批注，汇总为结构化反馈注入下一回合（复用现有 review 数据结构）。
- **验收口径**：用户在 diff 加行注后，下一回合模型收到结构化反馈并据此修改；端到端测试。
- **依赖/风险**：渲染已存在，仅补反馈通道；不重复 model→人 review（tools/review.rs）。

#### [L2-3] Fleet 告警生产接线 + IM 渠道扩展 `P1` `fleet,#96,#821`
- **来源 issue**：#821（与 #97 跨端审批已落地区分开）
- **问题/动机**：`FleetAlertAdapterConfig` 三种适配器（Slack/Webhook/PagerDuty）仅 dry-run 可达，生产投递从未接线；TG/钉钉/飞书/企微/Discord 无代码。
- **可实施范围**：把 `FleetAlertDispatcher` 接真实投递路径，与 #671 automation 完成事件共用 sink 抽象；补 IM 渠道适配器。
- **验收口径**：配置 Slack/Webhook/PagerDuty 后真实投递成功（非 dry-run）；至少新增一种 IM 渠道适配器。
- **依赖/风险**：与 #671 共用 sink；`WebhookHookSink` 死代码可复用。

#### [L2-4] daemon WebSocket + ACP 扩容 + 多工作区注册表 `P1` `daemon,#93,#94,#820`
- **来源 issue**：#820
- **问题/动机**：HTTP+SSE 已可用，但无 WebSocket、ACP 仅 baseline（4 方法、仅 stdio）、无多工作区注册表（多仓需起多 daemon 占多端口）。
- **可实施范围**：加 WebSocket 承载（tokio-tungstenite/axum ws）；ACP 扩展权限请求/文件系统方法；新增多工作区注册表（区别于 `workspace_discovery` 的 @-mention 黑白名单）。
- **验收口径**：WS 端点可双向低延迟通信；ACP 新增方法可注册调用；单 daemon 可服务多 workspace。
- **依赖/风险**：SSE 已覆盖多数流式场景，WS 收益在双向低延迟；多工作区可暂用多进程绕过。

#### [L2-5] Provider OAuth + Bedrock/Vertex/Copilot 接入 `P1` `provider,#59,#61,#819`
- **来源 issue**：#819
- **问题/动机**：Login 仅 `--api-key`；Provider 全集仅 3 个（OpenAI/Anthropic/Gemini 兼容），缺 OAuth 订阅制与 Bedrock/Vertex/Copilot。
- **可实施范围**：加 OAuth device-code 登录流（Claude Pro/Copilot）；Bedrock（SigV4）/Vertex（GCP 凭据链）走各自 wire format。
- **验收口径**：OAuth 登录可完成并持久化凭据；Bedrock/Vertex 可作为 provider 配置并真实调用。
- **依赖/风险**：注意区分 `mcp/oauth.rs`（MCP 侧）与 provider 登录；企业合规场景价值高。

#### [L2-6] 渐进式工具披露 select_tools 收尾（模型主动申请工具） `P1` `ux,prefix-cache,#826`
- **来源 issue**：#826（#725 已作为重复关闭）
- **问题/动机**：能力已有（每步重算工具目录），但每步重写请求前缀击穿 prefix cache。`tool_catalog.rs` 已有 `select_tools` 相关代码，但「模型主动调用 select_tools 工具申请工具」入口未注册到工具表（registry 无对应 ToolSpec）。
- **可实施范围**：`active_tools_for_step` 改为返回「基线+增量」；引擎接线增量公告，不再每步重写目录；注册 `select_tools` ToolSpec 供模型主动申请。
- **验收口径**：连续 10 回合工具集变化≥3 次，请求前缀 token 序列零变化（用 #646 cached_tokens 验证）；模型申请后下一回合工具可用；端到端单测（申请→调用→成功）。
- **依赖/风险**：建议 #646 prefix cache 命中率指标先落地以度量收益；切片 A（公告式实现）已合（db4d5d8），剩 registry 注册收尾。

#### [L2-7] BTW 侧边对话（独立消息栈临时旁路问答） `P2` `ux,#808`
- **来源 issue**：#808
- **问题/动机**：写代码时「顺便问一句」会污染主上下文/占 KV cache，无「问答不入主历史」机制。
- **可实施范围**：新增 `/btw <question>`，开共享同模型/同工具但独立消息栈的临时子对话，答完即弃，默认不写回主线（可选采纳）。
- **验收口径**：`/btw` 问答后主对话 token 与历史不增加；可选把侧边回答采纳进主线。
- **依赖/风险**：复用 subagent 隔离运行时；对软缝压缩无影响；P2 体验增强。

#### [L2-8] headless 结构化输出（--json-schema 终态约束 + 合成终结工具） `P1` `headless,ux,#824`
- **来源 issue**：#824
- **问题/动机**：对标 qwen `tools/syntheticOutput.ts`：headless/CI 下模型产出自由文本，调用方须自写解析器且随模型措辞漂移。`structured_output` 仅是能力标记位（models_dev.rs / model_profile），无任何 schema 校验或投递逻辑；`ExecOutputFormat` 仅 `Text`/`StreamJson`，stream-json 不约束最终结构。
- **可实施范围**：headless 下注册一个**合成的终结工具**，模型必须调用它提交符合指定 JSON Schema 的最终结果，首个合法调用即结束会话；CLI 加 `--json-schema` 参数驱动该约束。
- **验收口径**：`mimofan -p --json-schema <schema>` 下，会话仅在模型提交符合 schema 的终结工具调用后结束；非法调用被拒；端到端单测。
- **依赖/风险**：纯 headless/CI 增强，不触及交互主路径；需与现有 stream-json 输出格式共存。

### L3 评测与对标

#### [L3-1] 离线 mock 驱动 agent 行为评分基准（27 指标记分卡） `P1` `benchmark,#C8,#815`
- **来源 issue**：#815
- **问题/动机**：离线设施有但 mock 驱动 agent 行为评分缺失（`MockLlmClient` 仅注释），无法可复现量化 A1 端到端成功率等核心能力。
- **可实施范围**：新增 `benchmark/agent_harness/`，真正落地 `MockLlmClient`（record/replay fixture → canned StreamEvent 喂真实 turn_loop）；固定样本断言工具名/参数 schema/计划覆盖/修复轮数/最终仓库状态；27 指标 0–10 记分卡，before/after 同卷对比。
- **验收口径**：无网络无 key 跑出完整 27 指标记分卡；同改动 before/after 同卷可对比。
- **依赖/风险**：与 #822 Edit Apply 基准、#797 compaction 断言互补；纯 Python+Rust 集成测试。

#### [L3-2] Edit Apply 一次成功率回归基准 `P1` `benchmark,#822`
- **来源 issue**：#822（关联 #815、#556）
- **问题/动机**：edit_file/apply_patch 有 preflight 但无一次成功率回归基准，无法量化 diff/old_string 一次命中率。
- **可实施范围**：新增 `benchmark/edit_apply/` 多语言样本集（空格变更/重复串/嵌套括号等易失败 case）；跑 edit_file/apply_patch 统计一次成功率输出记分卡；失败 case 反哺 preflight。
- **验收口径**：样本集可重跑输出成功率；零 warning；失败 case 反馈到 preflight 改进。
- **依赖/风险**：依赖 #815 mock 基础设施或独立 harness；样本集需覆盖边界。

#### [L3-3] 公开记忆基准验证（LongMemEval）与能力提升追踪 `P1` `benchmark,memory,#832`
- **来源 issue**：#832
- **问题/动机**：记忆评测仅规则子串匹配，无 LLM-as-judge、无公开基准对接。已跑出 LongMemEval 100 条 judge 召回率 0.14（根因是本地哈希 embedding 无语义能力，非模型）。
- **可实施范围**：`longmemeval_harness.py` 已存在，扩展跑 500 条全量并产出 5 维度 judge 分数+归因；制定能力提升切片（真实语义 embedding 接入为最大杠杆，依赖独立 embedding key）；持续追踪。
- **验收口径**：可复现跑通并产出 judge 分数；`LONGMEMEVAL_REPORT.md` 含归因；≥3 条提升 todo 落入 loopx。
- **依赖/风险**：**最大杠杆是接入真实语义 embedding**（MiMo 端点不提供 /v1/embeddings，需独立 key）；当前 0.14 低估上限。

## 三、疑似已落地（建议复核后关闭或收窄）

以下 OPEN issue 经代码核验疑似已交付，建议复核证据后关闭或收窄范围，**未擅自改动 issue 状态**：

- **#792 密钥凭据泄漏分析** — 证据：`crates/secrets/src/scanner.rs` 已含 `scan_line`(L102)、`is_sensitive_content`(L253)、`redact_stream`(L308) 实现及单测（L465+）；调用点 `file.rs`/`edit.rs` 写入前拦截。SECURITY_CAPABILITY_PLAN.md 已将其列为「已完成前提」。建议复核是否还需补 git 历史扫描/熵分析，否则关闭。

- **#795 provider 故障转移/熔断** — 证据：`crates/tui/src/llm_client/circuit_breaker.rs`（纯状态机）+ `crates/tui/src/core/engine/circuit_breaker.rs`（引擎接线，`engine.rs:3104 mod circuit_breaker;`）+ `record_failure` 调用已存在并有单测。跨 provider 故障转移（`advance_fallback`）issue 评论已确认实现。**但** `impl_composer.rs`（advance_fallback 主体）grep 零命中 `CircuitBreaker` 引用，即熔断状态机已落地但「挂到 advance_fallback readiness 过滤」的收尾接线待确认。建议核实 advance_fallback 是否读取熔断状态——若已接线则关闭，若未接线则保留并收窄为「熔断收尾接线」（即 L1-6）。

- **#830 OpenTelemetry/OTLP 切片 A** — 证据：`crates/telemetry/` crate 已存在（`Cargo.toml`+`src/lib.rs`+`tests`），issue 评论确认 PR #772（`03c50f4`）已合并 feature-gated OTel 桥接。切片 B–D（GenAI span/Prometheus/endpoint 配置）未做。建议将 issue 收窄为「B–D 上层导出」（即 L0-8），或拆分后关闭切片 A 部分。

- **#829 记忆巩固与遗忘 切片 A** — 证据：`crates/memory/src/consolidation.rs` 已存在（`MemoryEntry{importance,last_accessed_at,access_count}` + `record_access`），PR #773（`a1f0320`）合并，46 passed。切片 B–E 未做。建议保留 issue 但更新状态，切片 A 标记已交付。

- **#828 混合检索 切片 A/B** — 证据：`crates/memory/src/codebase.rs` 已有 `RetrievalHit`(L57) + `score_breakdown`；PR #767/#771 已合并。切片 C/D/E（时近/重要性/四路融合）未做。建议保留并收窄为「切片 C–E」。

- **#831 用户建模 UserProfile 切片 A** — 证据：`crates/memory/src/user_profile.rs` 已有 `struct UserProfile`(L59)+`impl`(L88)，PR #770（`ce64497`）合并。切片 B–D 未做。建议保留并收窄为「切片 B–D」。

- **#807 统一次级模型 部分** — 证据：`crates/tui/src/model_routing/mod.rs:53` 已有 `enum SecondaryModel` + 解析逻辑 + 单测（L690+）。收敛进行中，但 seam_model/cheap_tier 三处是否真统一待确认。建议核实是否已统一，若已统一则关闭，否则保留。

## 四、合并说明

- **Epic #658 经验/训练闭环族**：#809（经验学习闭环）、#812（PTC 程序化工具调用）、#811（/learn 蒸馏 skill）、#810（Skill Curator）、#813（rollout 导出）、#814（Blueprint 导出）语义独立但同属「self-improvement / 训练数据生产线」主题。合并为 L1-1 一条需求项（主条目 #809），其余作为子能力在来源中并列标注，避免 6 条分散冲淡主线。PTC（#812）实为「脚本内 RPC 调工具」独立能力，若实施优先级高可单列。

- **MECE 漏洞挖掘体系**：#790（数据流求解器）、#794（状态机）、#791（配置分析）、#793（二进制分析）、#792（密钥分析）同属 `SECURITY_CAPABILITY_PLAN.md` 的 SAST 族，且 #791/#793 是 #790 求解器的上层应用、#792 已落地。合并为 L1-8~L1-12 系列（#790 作底座主条目，#791/#793/#792 各自独立需求项但共享依赖说明），#794 独立为 L1-9（C2/C3 专属）。

- **记忆体系**：#829（巩固遗忘）、#828（混合检索）、#831（UserProfile）、#816（模块合并）、#796（可观测）全部围绕 `crates/memory`，合并为 L1-2~L1-5 + L0-2 系列，明确切片 A 已交付状态，避免重复派工。

- **评测体系**：#815（agent 行为基准）、#822（Edit Apply 基准）、#832（LongMemEval）合并为 L3 三件套，分别覆盖「行为记分卡 / 编辑成功率 / 记忆质量」三个正交维度。

- **Provider 可靠性**：#795 故障转移/熔断经核验故障转移已实现、熔断部分落地，与相关 #419 类不在列表；保留为 L1-6 单条（P0，因影响主干长任务可用性）。

- **QUEUE/长程任务族**（#654/#700/#631/#665/#693/#724）经核查**不在 43 个 OPEN 列表中**（已关闭或并入他处），其能力地图在 `QUEUE_CAPABILITY_PLAN.md` 已闭环描述，本梳理不重复列入，仅作为背景依赖（如 #807 阻塞 T-Q5）。

- **重复关闭项**：#725（select_tools 重复）已关闭指向 #826；#662（MemoryBackend 重复）已关闭拆出 #831；#623（记忆巩固重复）已关闭并入 #829。均不重复计。

---

**共梳理 43 个 OPEN issue，归并为 37 条需求项（L0:8 / L1:18 / L2:8 / L3:3）。这 43 个原 issue 已于 2026-08-15 全量 triage 迁移为 #790–#832 并关闭，来源编号见上方迁移说明与各条目「来源 issue」。**

> 注：第三章「疑似已落地」的 #601/#619/#726/#716/#714/#732/#653 已同步更新为新编号（#792/#795/#830/#829/#828/#831/#807），其「建议复核后关闭或收窄」的判断仍有效，复核依据见各条证据。
