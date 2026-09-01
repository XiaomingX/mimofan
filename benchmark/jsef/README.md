# Java Security Education Framework (JSEF) - Spring Boot 安全实践平台
[![GitHub Stars](https://img.shields.io/github/stars/XiaomingX/JSEF?style=social&label=Star%20This%20Repo)](https://github.com/XiaomingX/JSEF)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![PRs Welcome](https://img.shields.io/badge/PRs-Welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Java Version](https://img.shields.io/badge/Java-17%2B-blue.svg)](https://www.oracle.com/java/technologies/downloads/#java17)
[![Spring Boot](https://img.shields.io/badge/Spring%20Boot-3.x-orange.svg)](https://spring.io/projects/spring-boot)
[![Docker Ready](https://img.shields.io/badge/Docker-Supported-blue.svg)](docs/docker-deployment.md)

> 一款**可复现、可实操、可学习**的Spring Boot Web安全实验框架，助力开发者快速掌握Web安全漏洞原理与防御方案。


## 项目简介
**Java Security Education Framework (JSEF)** 是基于Spring Boot 3.x构建的Web安全实践平台，专为**开发者、安全研究员、高校学生及企业培训**设计。通过**35+种真实业务场景下的安全漏洞实例**（含注入攻击、越权访问、敏感信息泄露等核心类型），提供“**原理讲解→漏洞复现→代码对比→修复验证**”的完整学习闭环，帮助学习者从“理论”到“实战”快速掌握Web安全核心能力。

本项目不依赖复杂环境，支持本地一键启动与Docker部署，所有漏洞案例均基于真实业务逻辑设计，避免“为了漏洞而漏洞”的演示性代码，更贴近实际开发场景。

**新结构说明：** 项目代码已重构，所有漏洞相关控制器现在位于 `com.freedom.securitysamples.vulnerability` 包下。每个漏洞类别内部进一步细分为 `vuln` (包含不安全/脆弱实现) 和 `sec` (包含安全/修复实现) 子包，便于直接对比学习。API 路由也已统一为 `/api/v1/{vulnerability-type}/unsafe/{scenario}` 和 `/api/v1/{vulnerability-type}/safe/{scenario}` 格式。


## 核心优势（为什么选择JSEF？）
| 优势                | 具体说明                                                                 |
|---------------------|--------------------------------------------------------------------------|
| **漏洞实例真实可复现** | 35+漏洞覆盖OWASP Top 10全类型，每个案例均模拟真实业务场景（如用户登录、数据查询、文件上传）。 |
| **学习闭环完整**     | 每个漏洞配套：原理文档+复现步骤+不安全代码+安全代码对比+防御最佳实践。         |
| **部署零门槛**       | 支持`mvn`一键启动、Docker容器化部署，无需手动配置数据库/中间件。             |
| **代码规范清晰**     | 采用Spring Boot最佳实践编码，漏洞代码与安全代码已按 `vuln`/`sec` 目录分离，便于对比学习。       |
| **资源生态丰富**     | 内置API文档、漏洞复现手册、安全编码规范，持续更新CVE最新漏洞案例。           |
| **高度可扩展**       | 提供插件化漏洞案例接口，支持开发者自定义新增漏洞场景或扩展防御方案。         |


## 快速开始
### 环境要求
- JDK 17 或更高版本
- Maven 3.6+ 或 Gradle 8.0+
- Git（可选，用于克隆仓库）
- Docker（可选，用于容器化部署）

### 方式1：本地Maven启动（推荐新手）
```bash
# 1. 克隆仓库（或直接下载ZIP包）
git clone --depth 1 https://github.com/XiaomingX/JSEF.git
cd JSEF

# 2. 构建项目（跳过测试加速构建）
mvn clean package -DskipTests

# 3. 启动服务
java -jar target/java-sec-code-plus-1.2.0.jar
```

### 方式2：Docker一键部署
```bash
# 1. 构建镜像
docker build -t jsef-security-sample:latest .

# 2. 启动容器
docker run -d -p 8080:8080 --name jsef-demo jsef-security-sample:latest
```

### 验证部署成功
启动后访问以下地址：
- 项目首页：`http://localhost:8080`（查看项目导航与漏洞列表）
- API文档（Swagger）：`http://localhost:8080/swagger-ui/index.html`（查看所有漏洞接口详情）
- 漏洞手册：`http://localhost:8080/docs`（查看在线漏洞复现指南）


## 漏洞案例分类（35+全列表）
关于所有已实现的漏洞案例的详细列表，请参阅 [VULNERABILITIES.md](VULNERABILITIES.md)。


## 适用场景
| 用户类型               | 适用场景                                                                 |
|------------------------|--------------------------------------------------------------------------|
| **开发工程师**         | 学习安全编码规范，避免在项目中写出存在漏洞的代码。                         |
| **安全研究员**         | 复现漏洞原理，验证防御方案有效性，开发安全工具的测试环境。                 |
| **高校师生**           | 信息安全/网络安全课程实验平台，替代传统演示性实验。                       |
| **企业培训**           | 开发团队安全编码培训、渗透测试团队入门实战练习。                           |
| **CTF选手**            | 基础漏洞实战练习，熟悉常见漏洞利用姿势。                                   |


## SAST 能力与多模型漏洞挖掘 Benchmark

JSEF 不只是教学平台，还内置了一套用于**验收 SAST 基础能力**与**对比多个大模型漏洞挖掘能力差异**的 benchmark。设计基于 SAST 第一性原理（从 source 到 sink 的不可信数据可达性证明），样本带有区分度梯度，便于交叉对比误报、漏报、平均耗时、超时样本、报告简洁度与能力完备程度。

### 核心能力

| 能力维度 | 说明 |
|---------|------|
| 污点传播（变量无断点） | 单跳 / 多跳 / 间接（Map/字段）梯度，检验中间变量是否丢污点 |
| 状态机 / 调用链追踪 | 跨方法 / 跨文件 / gadget chain，检验可达性分析深度 |
| 框架语义理解 | Spring 参数绑定、SpEL、@RequestParam 驱动的隐式 source/sink |
| 误报抑制 | OWASP 式真假混淆样本，检验对"看似危险但安全"代码的判别 |

### 样本与区分度分级

样本按 **L0-L5** 分级（逐级加大推理距离与语义依赖，以拉开不同工具/模型的能力档次；L0 为能力基准，所有工具/模型都应命中）：

| 级别 | 含义 | 示例 |
|------|------|------|
| L0 | 能力基准（显式直连） | source 直接传入 sink，无中间变量 |
| L1 | 单跳直连 | `Runtime.exec(userInput)` |
| L2 | 多跳（变量无断点） | source -> 中间变量 -> builder -> sink |
| L3 | 间接 / 跨方法 | 污点经 Map/字段传递；经方法返回值跨函数 |
| L4 | 跨文件 / 框架语义 | Controller -> ServiceA -> ServiceB -> sink；Spring4Shell SpEL 语义 |
| L5 | gadget chain | 多个安全类组合成危险可达性（CC 反序列化链抽象） |

除基础分级外，另有两类"长程/复杂任务"样本族，专用于验收大模型的**规划能力**与**一致性**：
- **长程任务（LT 系列）**：跨文件追踪 / 框架状态机 / gadget chain 还原 / 多跳拼接 / 版本门控，详见 [`benchmark/README.md`](benchmark/README.md) §3。
- **代码质量 / 性能 DoS + LGTM 缺口（PERF/TB/REFLECT/FMT/HOST/XSLT/FWD/SEED 系列）**：慢 SQL、资源泄漏、反射注入、信任边界、格式串注入等，对标 LGTM/CodeQL Java 规则库。

### 当前样本规模

> 数据来源：`benchmark/expectedresults.csv`（事实源，与源码 `// [CHECKPOINT]` 标注双向一致，经 `validate_checkpoints.py` 校验退出码 0）

- **782 条**机器可读 checkpoint 标注（覆盖 `src/main` 现有漏洞 + `benchmark/cases` 梯度样本 + 长程任务 + 代码质量/性能 DoS + LGTM 缺口 + 逻辑漏洞样本 + **原子范式样本族 TCM/SBM/DBG/STR** + **高区分度数据流变形/防护语义陷阱样本族** + **场景化编排样本族（检测压力/级联/多漏洞链/活分支截断）**）
- **414 个 VULN**（应报）+ **368 个 SAFE**（不应报，用于算 TN/FP）
- 难度分布：L0 x 18、L1 x 165、L2 x 184、L3 x 181、L4 x 141、L5 x 93（完整 L0-L5 梯度）
- CWE 覆盖：**86 类**（仅计 VULN）。高频：表达式注入(917)、反序列化(502)、SQLi(89)、命令注入(78)、授权失效(285)、硬编码凭证/密钥(798)、业务逻辑(840)、SSRF(918)、IDOR(639)、路径穿越(22)、ReDoS(1333)、性能 DoS(400) 等
- 覆盖 **189 个 category**（slug），含 OWASP Top 10 2021 全类；**139 条**样本带 `trace=` 路径节点（支持 `--check-trace` 路径正确性评测）
- 专项样本族：长程任务(LT) x 16、代码质量/性能 DoS(PERF) x 15、信任边界(TB)/反射(REFLECT)/格式串(FMT)/hostname(HOST)/XSLT(XSLT)/forward(FWD)/种子(SEED) 各 x 2
- **原子范式样本族（TCM/SBM/DBG/STR）** x 64：从 Fastjson、Spring Boot、Dubbo、Struts2 的真实 0day/1day 中抽象出**与具体库无关**的底层危险组合，用纯 Java 标准库自包含复现。详见下方「原子范式样本族」章节。
- **场景化编排样本族（DE/OS/DEAD）** x 18：检测压力（危险 sink 可达但会被监控，`detection-pressure`）、跨服务污点（RestTemplate 回传，`cross-svc-taint`）、级联信任（系统 A 配置决定系统 B 权限，`cascade-trust`）、多漏洞组合链（信息泄露→越权串链，`multi-vuln-chain`）、活分支截断（某活分支消毒截断不可达，`branch-dead-end`）。对标 CyScenarioBench / FrontierCyber / Kimi K3 评测。详见 `plans/09-scenario-benchmark-orchestration-samples.md`。
- **高区分度数据流变形 / 防护语义陷阱样本族（NV / TV）**：针对大模型与弱 SAST 的失分点补充。聚焦"污点经框架/状态/类型系统隐式传播"（getter 序列化、绑定、二次加载、Stream/Optional/异步 lambda、Map 间接、静态字段跨类）与"表面有防护实则可绕"（黑名单单次替换、清洗用错变量、前缀/弱正则校验、防护语序错误、配置默认危险）两类陷阱，每条配 SAFE 对照制造误报压力。含文件上传(434)、JWT RS256→HS256 算法混淆(347)、jku/x5u 不可信 JWKS(347/918)、XMLDecoder(502)、XStream 白名单语序(502)、Hessian2(502)、JEXL/Velocity SSTI(917/1336)、SVG/XXE 错误工厂(611)、路径规范化绕过(22)、双重解码(22)、重定向前缀绕过(601)、整数溢出/金额语义(190/682)、HPP 提权(915)、OAuth 缺 state CSRF(352)、资源 DoS(Zip Bomb/无界队列/400/409/776)、CompletableFuture 异步 SpEL(917)、三元/catch/循环入命令(78)、静态字段跨类污点(89)、SSRF 302 跟随/DNS 重绑定(918)、正则嵌套绕过 XSS(79)、Spring Cloud Function/@Query SpEL(917)、GraphQL 别名爆破(307) 等 **34 个新类别**。详见 `plans/06-new-vuln-gap-supplement.md`。

### 原子范式样本族（TCM / SBM / DBG / STR）

为满足「评估大模型 / harness 对**同类原理**漏洞的检测能力」这一需求，JSEF 从近年高危框架（Fastjson、Spring Boot、Dubbo、Struts2）的 0day/1day 中抽象出**去库化**的原子级危险范式，构造了一批与具体框架解耦、但原理同源的复杂样本。每个范式族含 `vuln` + `sec` 对照（算 FP/TN），按 L1–L5 分级，全部带 `// [CHECKPOINT]` 标注且不出现原框架类名（纯标准库语义）。

| 命名空间 | 抽象自 | 原子范式维度（MECE，互不重叠） | 样本数 |
|---------|--------|-------------------------------|--------|
| **TCM** | Fastjson 反序列化 | TCM-1 直接类型选择 · TCM-2 继承绕过白名单 · TCM-3 缓存/二次解析绕过 · TCM-4 私有字段可控 · TCM-5 属性即代码（getter/setter 危险） | 20 |
| **SBM** | Spring Boot | SBM-1 属性绑定穿越（Binder Traversal）· SBM-2 声明式配置被求值 · SBM-3 高权限端点暴露 · SBM-4 授权短路绕过 | 16 |
| **DBG** | Dubbo RPC | DBG-1 解析器/格式协商切换 · DBG-2 跨信任域隐式信任（attachment）· DBG-3 类名黑名单编码变形绕过 | 16 |
| **STR** | Struts2/OGNL | STR-1 双层求值（Double Evaluation）· STR-2 协议层字段注入 · STR-3 表达式排除列表/沙箱绕过 | 12 |

**设计要点**：
- 抽象原则：剥离「某 JSON 库 autotype」「某 Web 框架 SpEL」等具体机制，只保留「攻击者控制类型/数据 + 系统自动调用隐式方法 + 隐式方法链抵达危险 sink」等跨框架不变危险组合。
- 与既有样本不重叠：刻意避开仓库已有的 `JSEF-OGNL-*`/`JSEF-SPEL-*` 单层表达式注入、`JSEF-DESER-*` 直接反序列化等场景，只覆盖上述框架**独有**且未被建模的原子维度（如 OGNL 双层求值、Spring4Shell 绑定穿越、Dubbo 解析器协商）。
- 高区分度：含 L4 跨文件、L5 gadget chain、跨方法链等难例，专用于拉开不同工具/模型的能力档次。
- 安全底线：所有危险调用仅 localhost 演示语义、占位字符串，不提供真实利用脚本。

样本位置：`benchmark/cases/{vuln,sec}/{tcm,sbm,dbg,str}/`；设计文档：`plans/02-~05-*.md`。

样本组织：
- `benchmark/cases/vuln/` 与 `benchmark/cases/sec/`：有区分度的梯度样本（含安全对照）
- `benchmark/cases/vuln/longtask/` 与 `benchmark/cases/vuln/perf/` 等：长程任务与代码质量/性能 DoS 专项样本
- `benchmark/cases/vendor/`：从 OWASP Benchmark / Juliet / PrimeVul / CVEfixes 抽象留存的高质量竞品样本，含来源 URL 溯源

### 如何运行与交叉对比

1. 启动 JSEF：`mvn clean package -DskipTests && java -jar target/*.jar`
2. 选定被测对象：SAST 工具（CodeQL/SonarQube/Snyk）+ 大模型（在 Claude Code 中切换模型，使用相同提示词 `benchmark/prompts/vuln_hunt.md`）
3. 各对象对 `benchmark/cases/` 跑一遍，产出 SARIF 或 `id -> {hit,file,line}` 结果，记录耗时
4. 跑评分脚本得到交叉对比指标（在仓库根目录执行）：
   ```bash
   python3 benchmark/scripts/scorecard.py --expected benchmark/expectedresults.csv --result <结果文件.json|.sarif> --name <被测对象名>
   ```
   输出 Recall / Precision / **Youden Score (TPR - FPR)** / 平均耗时 / 超时数 / 报告简洁度 / 能力完备度，并按 CWE 与 level 分组。

详细设计与协议见 [`benchmark/README.md`](benchmark/README.md) 与 [`MY_PLAN.md`](MY_PLAN.md)。


## 验收 LLM 安全能力（误报率 / 漏报率评测）

> 本节聚焦「完整验收一个 LLM / SAST 的误报率与漏报率」：从防标签泄漏的盲化分发，到驱动模型产出结果，再到回连评分与横向报告。样本分级（L0–L5）、样本规模与单对象 scorecard 用法见上一节，本节不再重复。

### 1. 评测指标与定义

所有指标由 `benchmark/scripts/scorecard.py` 的 `compute_metrics` 产出（单对象 `--result` 与交叉矩阵 `--results-dir` 共用同一口径）。对齐规则（`align` 语义）：

| 期望标注（`expect`） | 被测对象上报 | 判定 | 含义 |
|---|---|---|---|
| `expect=VULN` | 报 | **TP** | 该报的报了（真阳性） |
| `expect=VULN` | 未报 | **FN** | 该报的漏了（漏报） |
| `expect=SAFE` | 报 | **FP** | 不该报的报了（误报） |
| `expect=SAFE` | 未报 | **TN** | 不该报的没报（真阴性） |

核心指标：

| 指标 | 公式 | 说明 |
|---|---|---|
| **漏报率（FNR）** | FN / (TP + FN) = 1 − Recall | 越低越好，漏报直接对应安全风险 |
| **误报率（FPR）** | FP / (FP + TN) | 越低越好，误报对应无效复核人力 |
| Recall（召回率） | TP / (TP + FN) | 漏报率的反向指标 |
| Precision（精确率） | TP / (TP + FP) | 上报结果里真漏洞的占比 |
| **Youden Score** | (Recall − FPR) × 100 | 0–100，OWASP Benchmark 口径，综合「召回且不误报」 |
| F1 | 2·P·R / (P + R) | Recall 与 Precision 的调和平均 |
| MCC | (TP·TN − FP·FN) / √((TP+FP)(TP+FN)(TN+FP)(TN+FN)) | 平衡类别不平衡的相关系数，-1~+1 |
| 定位精确率 | exact_hit_rate / near_hit_rate | TP 中行号完全命中 / 容差（`--line-tolerance`）内命中的占比 |
| CWE 精确度 | cwe_accuracy | TP 中被测对象上报 CWE 与 expected 一致的占比 |
| 路径证据链召回 | trace_recall | 开启 `--check-trace` 后，CSV `trace=` 节点被被测结果覆盖的比例 |

> `exact_hit_rate` 默认容差为 1：`// [CHECKPOINT]` 注解行位于 sink 上方一行，工具通常按 sink 行（N+1）上报，容差 1 消除该系统性偏移。详见 [`benchmark/README.md`](benchmark/README.md) §5.1。

### 2. 推荐流程（三步）

#### 第 1 步：生成双盲样本（防标签泄漏）

```bash
python3 benchmark/scripts/blind.py --cases-dir benchmark/cases --out benchmark/blinded
```

- 产物：`benchmark/blinded/B0001.java…`（盲化语料：移除 `// [CHECKPOINT]` / `// [VULN]` / Javadoc，类名与文件名中性化）+ 私有 `manifest.json`（`files` 与 `anchors` 映射，如 `B0001.java:ANCHOR_1 → JSEF-XXX-001`）。
- 盲化语料只是**评测分发形式**，不进入 `expectedresults.csv` 的 `file` 列；评分方持 manifest 才能把盲化上报回连到真实 checkpoint id。详见 [`benchmark/README.md`](benchmark/README.md) §5.3。

#### 第 2 步：驱动 LLM 产出结果

`run_llm_benchmark.py`（项目根目录，模型无关，HTTP 直连 OpenAI / Anthropic 兼容端点）：

- **identify 模式**（默认，判别式：列出 sample id 逐条判定）→ 产 `benchmark/results/<name>/result.json`（简化 JSON，scorecard 可直接消费）：

```bash
python3 run_llm_benchmark.py \
    --provider openai \
    --base-url https://<你的OpenAI兼容端点>/v1 \
    --model <模型ID> \
    --api-key $OPENAI_API_KEY \
    --name my-model
```

- **blind 模式**（盲挖式：不泄 ground truth，让模型自行报漏洞）→ 每文件产出一个 `.sarif`（`benchmark/results/<name>/*.sarif`），配合第 3 步的 `eval_blind.py` 回连：

```bash
python3 run_llm_benchmark.py \
    --provider anthropic \
    --base-url https://<你的Anthropic兼容端点> \
    --model <模型ID> \
    --api-key $ANTHROPIC_API_KEY \
    --name my-model --mode blind
```

- 稳定性评测（可选）：`--trials N` 对同一批文件跑 N 次独立试验，结果写入 `benchmark/results/<name>/trial_i/result.json`，`compare_models.py` 会聚合为 `sample_pass@1`（N 次全过才算过，DeepSWE 语义）。
- **`--require-complete` 默认开启**：覆盖率不足 100% 时以 **exit 2** 中止。被跳过 / 截断的文件不会被写入结果，其 vuln 样本会被 scorecard 误判为 FN——必须加 `--resume` 补跑至覆盖率 100%，再进入评分。

#### 第 3 步：评分 + 报告

- **单对象评分**（identify 结果）——得 Recall / Precision / FPR / Youden / F1 / MCC：

```bash
python3 benchmark/scripts/scorecard.py --expected benchmark/expectedresults.csv --result benchmark/results/my-model/result.json --name my-model
```

- **盲化回连评分**（blind 结果；`--line-tolerance` 默认 1：锚点在 sink 上方一行，LLM 常报 sink 行）：

```bash
python3 benchmark/scripts/eval_blind.py \
    --expected benchmark/expectedresults.csv \
    --manifest benchmark/blinded/manifest.json \
    --result <blind结果>.sarif
```

> `--result` 支持单个 `.sarif` / 简化 JSON，也支持**结果目录**（自动合并其下 `*.sarif` 与 `result.json`），直接传入盲挖模式输出目录即可。eval_blind 按 `B*.java` 文件名基名 + 行号回连到 manifest 锚点。

- **横向对比报告**——得 `compare.md` 排行 + `ranking.png` + `radar.png`：

```bash
python3 compare_models.py --results-dir benchmark/results --expected benchmark/expectedresults.csv
```

- **OWASP 行业标准报告**（`--cross-matrix` 消费上一步产出的 `cross_matrix.json`）——得 `report.md` + `report.json`：

```bash
python3 benchmark/reports/generate_report.py \
    --cross-matrix benchmark/results/compare/cross_matrix.json \
    --expected benchmark/expectedresults.csv \
    --out benchmark/results/compare/report.md
```

详见 [`benchmark/README.md`](benchmark/README.md) §8。

### 3. 环境变量表

评测脚本用到的环境变量（散落在各脚本 docstring，集中列出）：

| 环境变量 | 适用脚本 | 用途 | 缺省行为 |
|---|---|---|---|
| `OPENAI_API_KEY` | `run_llm_benchmark.py`（`--provider openai`） | OpenAI 兼容端点鉴权 | 未给 `--api-key` 时读取；仍为空则 FATAL |
| `ANTHROPIC_API_KEY` | `run_llm_benchmark.py`（`--provider anthropic`） | Anthropic 兼容端点鉴权 | 同上 |
| `MIMOFAN_PROVIDER` | `run_mimofan_benchmark.py` | mimofan 提供商 | 默认 `anthropic-compatible` |
| `ANTHROPIC_BASE_URL` | `run_mimofan_benchmark.py` | mimofan 端点 URL | 默认 `https://api.xiaomimimo.com/anthropic` |
| `ANTHROPIC_MODEL` | `run_mimofan_benchmark.py` | mimofan 模型标识 | 默认 `mimo-v2.5` |
| `ANTHROPIC_AUTH_TOKEN` | `run_mimofan_benchmark.py` | mimo 鉴权令牌（源码不硬编码密钥） | **缺省 FATAL** |

`run_llm_benchmark.py` 可用 `--api-key` 直传替代环境变量；`run_mimofan_benchmark.py` 必须走 `ANTHROPIC_AUTH_TOKEN`。

### 4. 盲化评测注意点

- 盲化评测的 **ground truth = 盲化语料中的锚点集合**（`/*ANCHOR_N*/`，vuln 与 safe 样本都有锚点），而非 `expectedresults.csv` 的原始行。
- 被测对象上报若落在锚点容差（`--line-tolerance`，默认 1）之外，`eval_blind.py` 无法关联任何 checkpoint → **告警并忽略**（不计分，也不会误判为 FP）。
- **只有落在锚点上的上报才能计入 TP/FP**；未上报的样本按期望类型计 FN/TN。因此盲挖式评测的指标语义与 identify 式完全一致，但杜绝了标签泄漏（模型无法从 `// [VULN]` 标记、类名、Javadoc 猜答案）。


## 官方文档
- [部署指南](docs/deployment.md)：本地/Mac/Linux/Windows/Docker部署全方案
- [ 漏洞复现手册](docs/vulnerability-guide.md)：每个漏洞的详细复现步骤（含Payload示例）
- [ API文档](docs/api-reference.md)：所有接口的请求参数、响应格式说明（支持Swagger在线调试）
- [ 安全编码规范](docs/secure-coding-guide.md)：基于Spring Boot的安全编码最佳实践
- [ 新增漏洞指南](docs/contribute-vulnerability.md)：如何为项目新增漏洞案例
- [Benchmark 设计与协议](benchmark/README.md)：SAST/LLM 漏洞挖掘验收 benchmark 的使用与扩展
- [Benchmark 实施计划](MY_PLAN.md)：能力模型、样本分级与待办进度
- [ 视频教程](https://github.com/XiaomingX/JSEF/wiki/Video-Tutorials)：B站配套漏洞复现视频（持续更新）


## 如何贡献
本项目欢迎所有形式的贡献，无论是**漏洞案例新增、文档完善、代码修复还是功能建议**，都能帮助更多人学习Web安全！

### 贡献方式
1. **提交Issue**：反馈漏洞、建议功能或报告Bug（推荐先搜索是否已有同类Issue）
2. **提交PR**：
   - 修复代码问题（如拼写错误、逻辑优化）
   - 新增漏洞案例（需遵循[新增漏洞指南](docs/contribute-vulnerability.md)）
   - 完善文档（如补充复现步骤、翻译英文文档）
3. **分享推广**：Star本项目、在技术社区分享使用体验，帮助更多人发现JSEF

### 新手友好贡献
- [Good First Issues](https://github.com/XiaomingX/JSEF/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22)：适合新手的入门级任务（如文档补充、代码注释完善）


## 开源许可
本项目基于 **MIT License** 开源，允许：
- 免费用于个人学习、企业培训及商业产品测试
- 修改、分发项目代码（需保留原作者版权声明）
- 基于本项目二次开发（需注明来源）

**禁止**：将本项目用于未经授权的渗透测试、恶意攻击等违法活动。


## Star 历史
[![Star History Chart](https://api.star-history.com/chart?repos=xiaomingx%2Fjsef&type=date&legend=top-left)](https://star-history.com/#XiaomingX/JSEF&Date)


## 致谢
- 感谢[OWASP](https://owasp.org/)提供的Web安全标准与漏洞分类框架
- 感谢Spring社区提供的Spring Boot生态支持
- 感谢所有贡献者的代码提交与反馈（[Contributors](https://github.com/XiaomingX/JSEF/graphs/contributors)）
- 感谢安全社区技术博主的漏洞原理分享


## 免责声明
本项目仅用于**学习、研究及企业内部安全培训**，请勿用于任何未经授权的测试、攻击或破坏活动。使用本项目产生的一切法律责任，由使用者自行承担。