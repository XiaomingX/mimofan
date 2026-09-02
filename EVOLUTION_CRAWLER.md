# 演进路线：从「终端 AI 编程助手」到「百亿级 URL 分布式爬虫 + 开源情报监测」

> 面向中国开发者的演进规划。说人话，不画饼。
> 本文档回答三个问题：**我们现在有什么**、**要去哪里**、**怎么一步步走**。
> 原则：**只动底层、不动用户交互层**（TUI/CLI/HTTP 的用法对新老用户恒定）；**不谎报现状**，能复用的明确列出，缺的才立项。
>
> 最后更新：2026-09-03

---

## 1. 先说清楚：我们正在做什么

mimofan 现在是一个**终端里的 AI 编程助手**：自然语言 → 模型思考 → 工具执行 → 结果回灌 → 再思考。它的内核其实是一个**成熟、可复用的智能体运行时**：

- 有一套**工具框架**：`ToolHandler` + `ToolRegistry`（注册/派发/schema）。
- 有一套**并发门禁**：`ToolCallRuntime`（读写锁 + 重入保护），并行工具可重叠、串行工具互斥。
- 有一套**任务/会话编排**：`JobManager`、`ThreadManager`、`Runtime`（无界面 API 核心）。
- 有一套**安全策略**：`ExecPolicyEngine`（审批门禁）、`SandboxBackend`（沙箱，Linux/macOS 各有实现）、网络策略 `NetworkPolicyDecider`。
- 有一套**持久化**：SQLite（`StateStore`）＋ 密钥（`Secrets`）。
- 有一套**HTTP 服务**：`app-server`（axum，已按读写锁拆分并发粒度）。
- 有一整套**提示词分层宪法**与**模型路由**：多 Provider、模型别名、回退。

**关键洞察**：以上这些能力，恰好就是一个"爬虫 + 情报"系统所需要的**执行骨架**。爬虫的本质是"把一个 URL 当成一个任务，交给一个能调度、能并发、能限速、能审批、能落盘的工具去跑"。这跟 mimofan 现在的"把一条指令当成一个任务，交给工具去跑"在**架构上是同构的**。

所以演进的第一性原理是：**不要推翻重来，而是往现有的"智能体运行时"上，长出"采集、清洗、结构化、调度、情报"这几个新领域子域**。

---

## 2. 要去哪里（目标：百亿级 URL + 开源情报 + 多模态标准化）

目标态是这样一个系统：

```
                            ┌──────────────────────────────┐
                            │    规模化采集 / 情报平台        │
                            │  (分布式爬虫 + OSINT 上层服务)   │
                            └──────────────────────────────┘
                                       │
         ┌──────────────────┬───────────┴─────────┬──────────────────┐
         ▼                  ▼                     ▼                  ▼
   ┌────────────┐    ┌────────────┐        ┌────────────┐    ┌────────────┐
   │  URL 调度域  │    │ 采集执行域  │        │ 数据标准化域 │    │ 情报监测域  │
   │ 百亿URL队列  │    │ 抓取/解析    │        │ 清洗/去重/   │    │ 规则/变化/  │
   │ 分片/去重    │    │ 多模态获取   │        │ 结构化解析   │    │ 告警       │
   └────────────┘    └────────────┘        └────────────┘    └────────────┘
         │                  │                     │                  │
         └──────────────────┴─────────┬───────────┴──────────────────┘
                                      │
                            ┌──────────────────────────────┐
                            │   可复用的智能体运行时底座      │
                            │ ToolHandler │ ToolRegistry     │
                            │ ToolCallRuntime │ ExecPolicy    │
                            │ JobManager │ Runtime │ hooks    │
                            │ StateStore │ secrets │ mcp     │
                            └──────────────────────────────┘
```

**一句话**：运行时底座不动，往上长四个新领域子域（URL 调度、采集执行、数据标准化、情报监测）。

---

## 3. 复用清单：现有代码里"白捡"的（无需新建）

> 这条很重要——很多演进项**当前已经具备**，硬写"待办"就是凑数。先列出来。

| 复用点 | 位置 | 说明 |
|--------|------|------|
| 单机抓取工具 | `crates/tui/src/tools/fetch_url.rs`（`FetchUrlTool`）、`web_search.rs` | 已实现 `ToolSpec`；已内建**合规/频率门禁**（`NetworkPolicyDecider`，`fetch_url.rs:394`）、**SSRF 防护 + DNS pinning + 重定向上限 + 超时**（`:330` 起）、**HTML→可读文本**轻量解析（`html_to_text`，`:501`）。**这就是一个能用的爬虫工具。** |
| 并发门禁 | `crates/tools/src/lib.rs:416` `ToolCallRuntime` | 并行/串行按工具声明自动加锁，重入安全。爬虫的"并发抓取上限"可直接复用。 |
| 任务/会话调度 | `crates/core` `Runtime`（headless API）、`JobManager`、`ThreadManager` | "每个 URL=一个 thread/任务"的编排骨架。 |
| HTTP 服务 | `crates/app-server`（axum，RwLock 已拆并发粒度） | "给外部系统调用"的对外契约层。 |
| 审批/合规 | `crates/execpolicy` `ExecPolicyEngine`、`SandboxBackend` | 对外网抓取的**域名/路径/频率白名单/合规**判断可延展为"robots/疆域合规"。 |
| 监控告警 | `crates/hooks` `HookDispatcher`/`HookSink` | "URL 变化 → 告警"天然复用钩子。 |
| 语义去重/向量 | `crates/memory`（experimental）+ `crates/tui/src/vector_memory/` | 多模态内容语义去重的**现成底座**（默认按 `MIMOFAN_MEMORY_API_KEY` 优雅降级）。 |
| 精确去重 | `sha2`（workspace 已依赖） | 内容哈希去重，零成本。 |
| 观测 | `crates/telemetry`（feature-gated）+ `tracing` | 指标/Trace 底座，默认关闭、按需开。 |

> **结论**：阶段 0 并发底座、阶段 1 单机抓取，**当前已具备能力**，不需要新建一个"爬虫 crate"来重复造轮子。

---

## 4. 分步演进计划（每步可独立交付）

### 阶段 0：并发底座（部分已具备）

- [x] app-server 已从 `Arc<Mutex<Runtime>>` 改为 `Arc<RwLock<Runtime>>`（`crates/app-server/src/lib.rs:75`），只读路径可并发，消除队头阻塞。这是分布式前的基础。
- [ ] **状态外置抽象**：把"会话/任务状态"从 `Runtime`（单进程内存态）抽出一个 `StateRepository` 接口（trait），当前用 SQLite 实现，将来可换分布式。**这是百亿级的关键切口**——现在不做，分布式阶段就要返工。`crates/state` 的 `StateStore` 已天然面向"状态存取"，可在此之上加 trait 层。**优先级：中（演进红线，见 §6）。**

### 阶段 1：单机爬虫工具（已具备，复用现有）

- [x] `FetchUrlTool` + `WebSearchTool` 已实现 `ToolSpec` 并注册（`registry.rs:716` 附近），内建 SSRF 防护、重定向上限、超时、网络策略门禁、HTML→text。
- [x] 无需新建 `crates/fetcher`——现有工具即等价能力。**重复造并行 crate 是浪费。**
- [ ] **补齐抓取指纹**（可选增强）：加一个 `User-Agent`/`robots.txt` 合规读取与限速（节流）选项，复用 `ExecPolicyEngine` 做"合法抓取"门槛。**低优先级**。

### 阶段 2：网页结构化解耦（部分具备）

- [x] `fetch_url` 已有 `html_to_text`（轻量 HTML→文本）。
- [ ] **新增 `crates/parser`（结构化抽取算子）**：把"字段级结构化抽取"做成独立、可单测的模块。两条路二选一：
  - 强模型路由：用 LLM 从页面/文本抽字段（如融资事件 JSON），走 mimofan 的提示词 + 模型路由；
  - 弱模型/规则路由：CSS selector + 启发式 + schema 校验。
  - **建议**：先做"schema 驱动的抽取算子"（输入：HTML/文本 + 期望 schema；输出：结构化 JSON），把抽取能力和抓取解耦，便于后续复用。**优先级：高（阶段 3 的上游）。**

### 阶段 3：多模态清洗、去重、标准化、结构化解析（新建子域）

> 这是"采集数据标准化"的核心，也是与现有 AI 能力结合最紧的一步。

- [ ] **新增 `crates/multimodal`**：接纳并归一化多模态输入（文本/HTML/图片/PDF/音视频转写）。统一为**标准化中间表示（NormalizedRecord）**：`{content_hash, canonical_url, title, text, image_meta, audio_transcript, entity_tags, source, fetched_at, charset}`。
- [ ] **新增 `crates/dedup`**：去重分两层——
  - 精确去重：`sha2` 内容哈希（URL + 内容指纹）；
  - 语义去重：复用 `crates/memory`（embedding 向量，按 `MIMOFAN_MEMORY_API_KEY` 启用）做近重复检测。**这正好让 memory 从"实验性"变成"实用"。**
- [ ] **清洗规则**：正文提取、HTML 噪音剔除、字符集检测、语言识别、脱敏（复用轨迹脱敏逻辑）。
- [ ] **结构化解析**：接入阶段 2 的抽取算子，产出字段级 JSON。
- [ ] **标准化字典**：统一时间/地点/实体/作者/货币等字段（可复用 `crates/config` 与 `localization` 的思路）。
- **优先级：中。** 多模态清洗/去重标准化，是"开源情报"质量的地基；但必须先有阶段 2 的抽取算子。

### 阶段 4：百亿级 URL 调度（依赖外部中间件，需立项）

> 这是**从"单机"跨到"分布式"的分水岭**。SQLite 单文件模型在此失效。

- [ ] **新增 `crates/crawl-scheduler`**：
  - **URL 队列**：用 Kafka / NATS 做百万级待抓队列（替代内存队列）；
  - **分片**：按域名哈希分片，天然解决"同一站点限速"与"热点域名"；
  - **去重 Frontier**：布隆过滤器（Bloom Filter）+ 倒排索引，百亿 URL 去重；
  - **调度策略**：robots 合规、频率门禁、优先级、重试/退避。
- [ ] **`StateRepository` 换分布式实现**：会话/任务状态从 SQLite 迁移到"对象存储 + 分布式 KV（如 etcd/Redis）"，工作节点无状态化。
- [ ] **采集节点**：把阶段 1 的抓取工具包装成**无状态 worker**（从队列取 URL → 抓取 → 产出 NormalizedRecord → 入结果队列），可水平扩容。
- **依赖**：Kafka/NATS、Redis/etcd、对象存储。**需外部基建，单独立项。**

### 阶段 5：开源情报监测（依赖阶段 3–4）

- [ ] **规则引擎**：把情报线索（关键词/实体/代码样本/CVE 指纹/威胁指标 IoC）组织成可配置规则。
- [ ] **变化检测**：对已入库的实体/URL 做快照 diff，生成"变化事件"。
- [ ] **告警**：复用 `crates/hooks`（`HookDispatcher`/`HookSink`）把变化事件推给通知渠道（Webhook/JSONL/Stdout——现有 sink 已够）。
- [ ] **情报检索**：复用 `crates/state` + 向量召回（memory）做"语义检索 + 图谱关联"。
- **依赖阶段 3–4**，需立项。

### 阶段 6：集群化（依赖阶段 4）

- [ ] 节点**无状态化** + 服务发现（如 etcd/K8s）。
- [ ] 用**分布式锁**替代 `std::sync::Mutex`（多进程下 Mutex 无意义）。
- [ ] 全局观测：`crates/telemetry`（OTel 桥）+ `tracing` + Prometheus 拉取。
- **依赖阶段 4**，需立项。

---

## 5. 演进红线（来自稳定性文档 §7 —— 提前埋雷，防止返工）

| 红线 | 原因 |
|------|------|
| 集群**绝不**照搬"单 `Mutex<Runtime>`"模式 | 分布式下必须"无状态计算节点 + 共享状态存储"，`RwLock` 也只在单进程有意义 |
| SQLite 仅做本地元数据/缓存 | 百亿 URL 必须分片存储 + 倒排索引，SQLite 单文件撑不住 |
| 多进程下 `std::sync::Mutex` 无效 | 全面转向消息通道 / 分布式锁（etcd/Redis） |
| 采集合规先行 | 爬虫是持续对外行为，`ExecPolicyEngine` 的审批/沙箱 + `NetworkPolicyDecider` 必须延展为"域名/robots/频率"合规，否则有法律风险 |

---

## 6. 与当前架构的呼应（DDD 视角）

按 DDD，演进是把"接口上下文"（TUI/CLI/HTTP，**冻结不变**）下面的应用核心，再向横向长出几个**新限界上下文**：

```
接口上下文（TUI / CLI / HTTP）── 对外契约，对新老用户恒定不变
        │
应用核心（Engine 交互循环 / Runtime 无界面 API）
        │
子域： 采集域 │ 解析域 │ 清洗标准化域 │ URL调度域 │ 情报域   ← 本次演进新增
        │
基础设施：protocol │ state │ secrets │ mcp │ hooks │ config  ← 复用 + 部分分布式化
```

- 采集域 = 复用 `fetch_url`/`web_search` + `ToolCallRuntime` 并发门禁。
- 解析域 = 新增 `crates/parser`（阶段 2）。
- 清洗标准化域 = 新增 `crates/multimodal` + `crates/dedup`（阶段 3）。
- URL 调度域 = 新增 `crates/crawl-scheduler`（阶段 4）。
- 情报域 = 规则引擎 + 变化检测 + `crates/hooks` 告警（阶段 5）。

**依赖方向**保持严格向下 DAG（接口 → 应用核心 → 子域 → 基础设施），不允许反向。这与当前 19-crate 无环 DAG 的工程纪律一致。

---

## 7. 诚实的边界判断（不画饼）

- **阶段 0、1**：当前已具备或只需小改，**低风险，可即刻开工**。
- **阶段 2、3**：需要新建 `crates/parser` / `crates/multimodal` / `crates/dedup`，但**不需要外部中间件**，属纯 Rust 能力，可在本仓库内推进（前提是先有 parser）。**中风险。**
- **阶段 4–6**：**必须引入外部中间件**（Kafka/NATS、Redis/etcd、对象存储、K8s），这不是本仓库现在的基建，**不能假装一次干完**。应**单独立项评估**，且会改动服务端/基础设施层（不触碰 TUI/CLI/HTTP 用户交互层）。
- 本文档刻意**不列**任何"可有可无"的待办。凡已具备的用 `[x]` 承认；缺外部基建的用 `[ ]` 立项，**不做空 crate 框架凑数**。

---

> 相关文档：`ARCHITECTURE_CN.md`（当前架构）、`ARCHITECTURE_IMPROVEMENT_PLAN.md`（DDD 改进清单）、`ARCHITECTURE_STABILITY.md`（稳定性红线）、`USER_GUIDE_CN.md`（使用说明）。
