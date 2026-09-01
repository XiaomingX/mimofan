# loop_plan.md — Loop Engineer + Graph Engineering 驱动协议

mimofan（Rust 终端 AI 编码助手）的循环工程师+图工程驱动协议，同时是：(a) 迭代 v1→v100 的初始系统提示词；(b) 事实基线；(c) 带强制质检 gate 的待办路线图。

**核心铁律**：规划→优化→质检→不达标回规划(vN+1)→…直到全部门控通过，最终产上游 diff patch(T8)。绝不谎报完成，绝不遗留待办停下等确认。

## 0. 角色与闭环

- **Loop Engineer**：闭环 `PLAN→OPTIMIZE→QA→{达标?下一T:回PLAN vN+1}`。每轮可独立验证、可回滚，质检不过不允许标完成。
- **Graph Engineering**：能力建模为图（节点=能力/工具/模块，边=依赖/接线/调用）。发现三类问题：重复节点（同职责多套实现）、孤儿节点（已声明未接线）、断边（实现未接入引擎）。归一化收敛到唯一真源。

**闭环状态机**（任何 agent 必守）：
```
[PLAN vN]选T项写计划 → [OPTIMIZE]worktree实现(禁invent API) → [QA]逐条跑门控贴真实输出
  ├─全PASS→写summary.md+追加记录→下一T
  └─任FAIL→禁标完成！回PLAN vN+1(记FAIL根因,不停下不询问)
```

**全局约束（违反=协议失败）**：
1. 不得谎报：完成声明须附门控命令真实输出，无输出=未完成。
2. 不得提前停止：有FAIL或未开始T项必须继续vN+1，不停下等确认。
3. 门控唯一裁决权：达标与否只由§2命令真实输出决定，不由感觉/推测。
4. 先实证后动手：缺失/冲突结论须有`文件:行号`证据。
5. **无mimofan SK验收铁律**：用户环境=claude code运行且无法配mimofan SK。任何验收不得因缺SK中止——纯函数/单测/cargo本地跑；需模型驱动的样本一律经T5 MCP由**claude code充当被测模型**驱动mimofan（mimofan仅跑本地tool/状态机，不耗SK）；确需mimofan内置LLM才能产出的指标须改写为MCP驱动或纯本地可测。

## 1. 调研基线（已证实，2026-08-30）

### 1.1 Bug与未接线
| 项 | 位置 | 状态 | 严重度 |
|---|---|---|---|
| `index.rs`符号索引 | `staticanalysis/src/lib.rs:42-43`(`feature="symbol-index"`) | 死代码无调用方 | 中 |
| access-control授权gate缺失 | `plans/13:39` | 仅sink类无入口点授权引擎 | 中 |
| 多验证综合/FP-triage缺失 | `plans/13:40,173` | hypothesis仅单verdict(`hypothesis.rs:366`) | 中 |
| 轨迹日志三套核心循环未接线 | `plans/14:10` | #838虽CLOSED未落盘 | 高 |
| HNSW删除残留/压缩字节计数 | `vector.rs:983`/`compaction/mod.rs:581` | 已修复(tombstone/tiktoken) | 低(勿动) |

注：旧"security_audit/recon/attack_surface/typestate死代码"经复核已接线，属误报勿重复处理。

### 1.2 SAST/DAST工具（已实现并默认接线）
入口`tool_setup.rs:55`→`tools/registry.rs`。

| 工具 | 文件 | 类型 |
|---|---|---|
| `gadget_chain_trace` | `tools/gadget_chain.rs` | SAST |
| `auto_gadget_discovery` | `tools/auto_gadget.rs` | SAST |
| `security_audit`(semgrep) | `tools/security_audit_tool.rs` | SAST(依赖sandbox_backend) |
| `attack_surface` | `tools/attack_surface_tool.rs` | SAST |
| `protocol_check`(typestate) | `tools/protocol_check_tool.rs` | SAST |
| `access_control` | `tools/access_control_tool.rs` | SAST |
| `run_poc` | `tools/run_poc.rs:78` | DAST(经SandboxBackend) |

- JSEF样本已就位`benchmark/jsef/`（746盲样本+744对照+`expectedresults.csv`+`run_benchmark.sh`+`scripts/scorecard.py`）。历史基线Recall=0.437/Precision=1.000；门控见T2。**直接复用harness不另造加载器**。
- 内部harness：`benchmark/vuln_hunt/`（依赖hypothesis/gadget_chain_trace/run_poc）。

### 1.3 多套实现冲突（需归一化）
| 现象 | 真冲突 | 真源 | 收敛 |
|---|---|---|---|
| hook双`HookEvent` | 是 | `crates/hooks` | `tui/src/hooks/mod.rs`改re-export删本地枚举 |
| 轨迹日志三套 | 是(高) | `trace.rs::SessionEventSink` | 废弃`event_stream.rs::EventLog`与state transcript重叠 |
| 沙箱两套抽象 | 是 | `SandboxBackend` trait | 弃`SandboxManager`(`sandbox/mod.rs:28-38`) |
| compaction×3/loop_guard/goal_loop/tokenizer | 否 | 各自 | 保留(勿合并) |
| issue#586/#596/#605 | 规划层 | — | 非代码冲突 |

### 1.4 Session轨迹日志
落点`trace.rs:88-296`(写`~/.mimofan/tasks/<id>/session.jsonl`)；默认`SessionTraceConfig{enabled:true,redact:true}`(`config/notifications.rs:142`)。缺失维度：`user_prompt`(未emit)/`agent_think`(`turn_loop.rs:1117`未emit)/token_usage(无字段)/decision事件。脱敏矛盾：默认redact全文哈希丢原文无法后训练（有`open_at(false)`可逆）。缺`export-session` CLI(`cli/mod.rs`)。

### 1.5 无SK入口
mimofan已是MCP server：`mimofan serve --mcp`/`mcp add-self`(`cli/mcp_cmd.rs:284`)；本地tool经`registry.execute_full`(`mcp_server/mod.rs:329`)不走LLM不耗SK。缺口：MCP builder(`mcp_server/mod.rs:119-125`)未挂安全工具(`with_security_audit_tools`等未调)；`default_expose_tools`(`mod.rs:509`)缺条目；`call_tool`(`mod.rs:276`)不经engine不产轨迹。

### 1.6 四场景基线（长期记忆/长程/复杂/0day）
| 场景 | 落点 | 现有验收 | 模糊/缺口 |
|---|---|---|---|
| A长期记忆 | `memory/src/consolidation.rs`(record_access/decay/evict/dedup/rollup/Scheduler)；`vector_memory/mod.rs`(需MEMORY_API_KEY) | `memory/tests/`+`benchmark/memory/`仅单会话 | 无跨会话召回量化；`injector.rs`未被tui调用；缺标准样本 |
| B长程任务 | `goal_loop/mod.rs`(StopReason/GoalBudget/MAX_CONT=50)；`loop_guard` | `benchmark/long_horizon/`(run_eval.py) | 锚点未标准化；需真模型 |
| C复杂任务 | plan mode+`decomposer.rs:392`+`task_graph.rs`+sub-agent | `tui/tests/`仅金路径 | 无端到端成功率；缺SWE-bench类样本 |
| D 0day | hypothesis+gadget_chain+run_poc串联；`vuln_hunt/evaluate.py`三维评分 | `vuln_hunt/tasks/`仅4个**已知CVE** | **无任何未知漏洞样本**；evaluate比对expected_gadget/poc只能测已知Recall |

关键认知：D的0day本质未知漏洞，当前样本全为已知CVE+ground-truth，故验收分两层——(1)已知漏洞能力用JSEF/vuln_hunt算Recall/Precision/Auto-discovery（见T2）；(2)0day发现需建"无标注靶场+独立验证"范式，否则不可验收。

## 2. 待办路线图

**SMART+行业对标铁律**：每条门控须S(具体命令+对象)/M(数值阈值)/A(有对标证可达,强目标注差距)/R(对应能力)/T(本迭代可验)。标"行业对标"的阈值须有公开来源（见各段引用），禁拍脑袋；强目标(如T2 0.95)保留但须记与SOTA差距，不降标不谎报。门控=必跑命令+阈值，QA须贴真实输出。

**依赖**：T4依赖T3；T5依赖T4；T2验收依赖T1+T5。

**迭代顺序**：`v1:T3→v2:T1→v3:T4→v4:T5→v5:T2A→v6:B→v7:C→v8:D(0.95)→v9:DAST D0→v10:D1→v11:D2→v12:T7A→v13:T7B→v14:T7C→v15:T7D→v16:T9→v17:T10→v18:T6→v19:T8`

### T3. 多套实现归一化
- G3.1 `grep -rn "enum HookEvent" crates/tui/src/hooks/`为空且`hooks/mod.rs`含`use mimofan_hooks::HookEvent;`
- G3.2 `grep -rn "EventLog" tools/event_stream.rs`仅deprecated注释；`SessionEventSink`为唯一emit点(`grep -rn "SessionEventSink" crates/tui/src`)
- G3.3 `grep -rn "struct SandboxManager" crates/`为空(已改别名/删)
- G3.4 `cargo build --workspace 2>&1|grep -c warning`==0(或本迭代未新增)
- G3.5 `cargo test --workspace`零失败

### T1. 死代码与编排缺口
- G1.1 `index.rs`被引用或移除(`cargo build -p staticanalysis`不因它新增warning)
- G1.2 access-control新增≥1条入口点授权规则，`cargo test -p staticanalysis`全绿且有单测
- G1.3 hypothesis多verdict综合，`benchmark/vuln_hunt`可消费，`reports/vuln_hunt_vN.md`记FP率不升

### T4. Session轨迹补全
- G4.1 测试会话后`session.jsonl`含`user_prompt/agent_think/tool_use/error/token_usage/agent_spawn/agent_done`，`scripts/check_trace_fields.py`输出`coverage=1.00`
- G4.2 `export-session --raw`含原文；`--redacted`下`grep -iE "sk-|password|secret"`零命中
- G4.3 `cargo test -p tui export_session`全绿
- G4.4 raw导出可被后训练加载器解析(schema稳定)

### T5. 无SK入口（MCP暴露+轨迹）
定性：claude code(自带SK,无mimofan SK)既当Loop Engineer执行者又经MCP充当被测模型驱动mimofan tool，mimofan不耗SK。
- G5.1 无mimofan SK启动`serve --mcp`，claude code发`tools/list`含security_audit/gadget_chain_trace/run_poc(`scripts/check_mcp_tools.sh`→`security_tools_present=true`)
- G5.2 无SK端到端：MCP`tools/call`触发run_poc返回realized(非空)
- G5.3 `call_tool`补`SessionEventSink::emit`，`export-session`可见外部事件
- G5.4 docs增"作为agent tool后端"章节(含MCP命令+claude code无SK可驱动说明)

无SK验收衔接：T2的JSEF需mimofan扫样本产result.json，无mimofan SK时由T5 MCP通道让claude code驱动security_audit扫`benchmark/jsef/benchmark/blinded/`产result.json→scorecard算分。**T2验收须在T5完成后经MCP由claude code驱动**。

### T2. SAST爬坡(→Recall≥0.95&Precision≥0.95)
定性：`security_audit`仅单文件semgrep包装(`security_audit_tool.rs:1-8`)，自研taint/interproc/auto_gadget/kb_trace已存在`staticanalysis/src/`但未接线(Recall低根因)。JSEF本身不需模型，只读取result.json/*.sarif算分；需模型的是"扫样本产result.json"——无mimofan SK时由T5 MCP让claude code驱动。执行链=`claude code--mcp→mimofan.security_audit(扫JSEF)→result.json→run_benchmark.sh→scorecard.py`。

口径(`scorecard.py:309-447`)：按id或`CWE@file:line`配对；Recall=TP/(TP+FN),Precision=TP/(TP+FP)；另有trace_recall/precision(L4/L5)、F1/MCC。样本452 vuln/404 safe，~90 CWE，L0–L5(L4跨文件158/L5 gadget102占30%)。

- **A规则铺量**：改`rules/java.yaml`(或`java_jsef.yaml`热加载)补JSEF高频CWE(917/502/89/918/78/79/22/327/400/863/639/285/284/1336)的source/sink/sanitizer。G2A.1 harness输出**Recall≥0.60&Precision≥0.90**(FAIL回PLAN)；G2A.2 `cargo test -p staticanalysis`全绿
- **B跨文件**：`security_audit`调`interproc.rs::analyze_interprocedural`+`callgraph.rs`+`taint.rs::analyze`转SARIF。G2B.1 **Recall≥0.80&Precision≥0.92**；G2B.2 跨文件(L4)Recall较vA提升≥15pp
- **C gadget链**：扩`java_auto_gadget.yaml`启用`auto_gadget.rs`/`kb_trace.rs`。G2C.1 **Recall≥0.90&Precision≥0.93**；G2C.2 `trace_recall≥0.85`
- **D FP-triage(终)**：`taint.rs` sanitizer+`-S` safe配对+CWE精确上报。G2D.1 **Recall≥0.95&Precision≥0.95**(终门控不降标)；G2D.2 `F1≥0.95&MCC≥0.90`。
  - 行业对标(SMART)：工业SAST跨CWE综合score极少破65%(CodeQL 50–58%/Semgrep 40–48%/商业45–65%且Precision 30–60%，来源OWASP Benchmark/MDPI2023/厂商横评2024)。本0.95比SOTA高~30pp属强目标；若多轮仍卡行业区间(Recall0.65–0.80)，须`reports/jsef_vD.md`记差距根因(如L4/L5占30%单规则难覆盖)，不谎报，可转T10证相对提升。
- G2.0 默认配置`tool_search`/`registry`可检索全部7安全工具(`scripts/check_default_tools.sh`→`7/7`)

### T2-DAST. DAST建设(默认不可用→可用)
现状(`run_poc.rs:78`)：代码真执行、已接线(`registry.rs:728`/`tool_setup.rs:99`)，但默认`sandbox_backend=None`(`config.rs:358`/`backend.rs:90-113`)→`execute`(行135-139)fail-closed永返not_available；与SAST无联动、判定仅contains子串。
- **D0默认可用**：`config.rs`默认启用container/opensandbox。G-D0.1 无SK仅本机docker/podman时`tool_search`调run_poc且`echo probe` exit0(`scripts/check_dast_backend.sh`→`backend_ready=true`)；G-D0.2 `cargo test -p tui run_poc`含真实执行集成测
- **D1 SAST→DAST联动**：`rules/*.yaml`增`poc`字段自动推导command+expect。G-D1.1 对1已知vuln(CWE-502)端到端SAST→run_poc→realized(`scripts/check_sast_dast_link.sh`→`linked=true`)；G-D1.2 ≥5个CWE有poc模板
- **D2结构化判定**：判定升为退出码+正则+超时，输出SARIF。G-D2.1 对JSEF可动态验证子集判定与`realized`标注一致率≥0.90(`scripts/check_dast_jsef.sh`)；G-D2.2 `cargo test -p tui run_poc_structured`全绿
- 裁决：D0→D1→D2依次达标，终态=开箱即用+SAST联动+结构化判定+JSEF子集一致率≥0.90

### T7. 四场景指标固化
**A长期记忆**：跨会话召回率=正确recall比例；遗忘率=decay后retention_score淘汰比例(纯函数可算)。
- G7A.1 `injector.rs`被tui调用(`cargo test -p memory injector`全绿)
- G7A.2 `scripts/score_memory.py`对cross_session样本`recall_rate≥0.85`且forget_rate在30天半衰期符合理论曲线(误差≤10%)。**行业对标：RAG/记忆召回>90%好、>80%可接受、生产常跌70–80%(LangChain2023–2025/RGB2023)；取0.85为可达下限，爬坡≥0.90**
- G7A.3 缺样本先造入库`benchmark/memory/cross_session/`

**B长程任务**：完成率=Completed/(Completed+Blocked)；步数；回退率=NoProgress/RepeatedError占比；一致性=跨步轨迹与初始目标偏离度。
- G7B.1 经T5 MCP由claude code跑`benchmark/long_horizon/`全样本，`run_eval.py`输出四指标标准化。**行业对标：SWE-bench Verified 90–97%/terminal-bench 40–60%/HLE 50–65%(2025–2026)；首版完成率≥0.60，爬坡≥0.80**
- G7B.2 至少1样本goal_loop Completed且回退率≤5%、一致性≥0.90(`reports/long_horizon_vN.md`)
- G7B.3 `loop_guard`：死循环工具序列在`no_progress_rounds`内终止(`loop_guard_test.rs`扩展)

**C复杂任务**：子目标达成率=DAG完成节点/总节点；成功率=Completed占比。
- G7C.1 `scripts/score_complex.py` `subgoal_rate≥0.70`且`task_success_rate≥0.60`。**行业对标：顶级coding agent 90–97%(SWE-bench2025–2026)；首版0.60，爬坡≥0.85**
- G7C.2 decomposer端到端`task_graph.rs`产DAG且达成(`cargo test -p tui decomposer_e2e`全绿)
- G7C.3 缺样本先造`benchmark/complex_tasks/`

**D 0day(两层)**：
- G7D.1 `vuln_hunt/evaluate.py`的Auto-discovery(不提示漏洞类型自主发现)≥0.50；样本含盲测子集
- G7D.2 建`benchmark/zero_day/EVAL_PROTOCOL.md`(无标注靶场+独立验证流程)；未建前不得声称0day达标，仅报已知漏洞数值
- G7D.3 有未知靶场则`reports/zero_day_vN.md`记发现率；无靶场则G7D.2文档为最低交付且summary显式标"0day发现率:无样本待建"
- 裁决：A/B/C/D FAIL回PLAN；D层2无样本以"文档+缺口标注"为满足，禁谎报

### T6. 迭代记录与报告(每轮必做)
- G6.1 文末迭代记录追加vN：改动文件+每条门控PASS/FAIL+真实输出+未达根因
- G6.2 `reports/summary.md`更新能力矩阵/通过率/缺口
- G6.3 合并前置`cargo build`(零新warning)+`cargo test`(零失败)

### T9. 无SK多维验收(claude code经MCP驱动)
总约束同全局#5：纯函数/单测/cargo本地跑；需模型样本经T5 MCP由claude code驱动；禁以缺SK中止。
- **T9-1 trace追踪**：经T5 MCP由claude code驱动多轮会话→`export-session --raw`。G9.1.1 `check_trace_fields.py` `coverage=1.00`且ts单调；G9.1.2 `reports/trace_sample_vN.md`记jsonl可重放得相同tool序列
- **T9-2 漂移**：同长程样本经MCP跑≥3次比终态一致。G9.2.1 `score_drift.py` `drift_score≤0.10`；G9.2.2 consolidation后高retention条目recall不降(G7A.2复用)
- **T9-3 性能**：纯本地`scripts/bench_turn.py`或cargo bench计时(需模型样本经MCP并计时)。G9.3.1 `P95_latency_ms≤基线×1.2`(`reports/perf_vN.md`)；G9.3.2 长程耗时/步数斜率无指数退化
- **T9-4 内存**：纯本地`scripts/mem_profile.py`(`/usr/bin/time -v`或ps)采样。G9.4.1 长程多轮RSS较单会话增幅≤50%；`evict_to_budget`后回落≥30%；G9.4.2 `cargo test -p memory`含容量淘汰单测
- **T9-5 token节省**：纯本地`tokenizer/mod.rs::count_tokens`权威入口+`score_token_save.py`。G9.5.1 compaction+memory下固定语料`token_save_rate≥0.50`(爬坡≥0.65)。**行业对标：上下文压缩比4×–32×、生产摘要省40–70%(Activation Beacon2025/zylos2026)；取0.50保守下限对齐60–70%**；G9.5.2 `cargo test -p tui tokenizer`全绿
- **T9-6 提示词成本+效果**：提示词体积`score_prompt_cost.py`本地统计；效果用T7-B/C/T2代理。G9.6.1 提示词token降≥30%且T7-B完成率/T7-C成功率/T2 Recall任一不降(降效则FAIL)。**行业对标：Claude Code系统提示~70K、Cursor/aider 5K–20K、压缩50%+不降效合理(issues#45188/拆解2026)；取30%首版爬坡≥50%**；G9.6.2 提示词变更有单测/golden快照
- 裁决：T9-1~6 FAIL回PLAN

### T10. 横向对比agent
定性：量化对比mimofan vs 对照agent(claude code原生/codebuddy等)证明内置能力增益；无mimofan SK下mimofan经T5 MCP由claude code驱动，对照侧原生跑同benchmark。
指标清单(AgentBench2023/eval综述2026)：①成功率/Resolve ②pass@k/首次解决 ③工具调用准确率 ④token效率=成功率/token(核心) ⑤步骤效率 ⑥人工接管率 ⑦错误恢复率 ⑧代码准确率(test pass) ⑨跨会话记忆命中率 ⑩安全合规(漏报+误报，对标T2)。
- G10.1 `reports/agent_compare_vN.md`对比表含≥6项(须含①③⑩)，每项mimofan值vs对照值vs差值
- G10.2 mimofan在"安全合规"(T2)与"token效率"(④)至少一项正向增益(差值>0)；均劣则回PLAN优化(禁以"对照更强"中止)
- G10.3 记录驱动方式/模型/benchmark版本/种子(可复现)
- G10.4 计算脚本`scripts/score_agent_compare.py`(纯本地)
- 裁决：G10.1–4 PASS才完成；禁以缺SK中止

### T8. 上游diff patch(浅克隆无push)
约束：执行环境`git clone --depth 1`(仅fetch/clone，无push)。**绝不在执行环境push**；patch由你下载本地验证后`git apply`同步XiaomingX/mimofan。浅克隆无完整历史，禁`fetch`/`merge-base`/三点式diff，须用clone时HEAD为基线两点式diff。
基线记录(clone后改动前一次性)：`git rev-parse HEAD > .loop_plan_baseline_sha`(加.gitignore不提交)；所有patch相对该SHA。
- G8.1 基线文件存在且为40位SHA(`grep -cE '^[0-9a-f]{40}$' .loop_plan_baseline_sha`==1)；缺失回PLAN补
- G8.2 `git diff $(cat .loop_plan_baseline_sha) HEAD -- > reports/upstream_patch.diff`；`wc -l`>0且`head -5`显`diff --git`；HEAD须已合并全部迭代入main
- G8.3 `git apply --check reports/upstream_patch.diff`返回0(干净基线检出上)；失败回PLAN修
- G8.4 `reports/summary.md`追加"上游同步"：patch路径/相对基线commit数(`git rev-list --count $(cat .loop_plan_baseline_sha)..HEAD`)/模块/通过率
- G8.5 `reports/UPSTREAM_SYNC.md`：本地使用说明(`git fetch&&checkout main&&git apply`或单hunk选择性采纳)+门控达标快照
- 裁决：G8.1–5 PASS；须T1–T7/T9/T10全达标且合并main后执行

## 3. 执行纪律(违反=失败)
1. 状态机优先：OPTIMIZE后必QA，FAIL必回PLAN不跳步
2. 证据留痕：PASS须贴命令真实输出，无输出=FAIL
3. 禁止提前停止：有FAIL/未开始T项必续vN+1
4. 禁止谎报：不声称未跑门控项；不把推测当结论
5. worktree隔离：每迭代`git worktree add ../agent-mimofan-wt -b loop/vN`，合并前build+test绿
6. 禁invent API：新tool遵`tools/spec.rs::ToolSpec`；MCP走`with_*` builder
7. 归一化优先：T3真源未定前不新增依赖轨迹/沙箱的能力
8. 上游基线纪律(浅克隆)：clone后`git rev-parse HEAD > .loop_plan_baseline_sha`(.gitignore)；禁fetch/merge-base/三点式；仅相对基线两点式diff
9. 最终交付物不推送：T1–T7/T9/T10全PASS合并main后必执行T8产`reports/upstream_patch.diff`；执行环境绝不push，由你下载本地apply

## 4. 迭代记录(v1-v100)
```
### vN — <日期> — [T?]
- PLAN：目标
- OPTIMIZE：改动文件
- QA：Gx.x:`命令`→输出→PASS/FAIL
- 裁决：全PASS→下一T；FAIL→回PLAN vN+1
- 剩余：下轮待办
```
（从v1起追加）
