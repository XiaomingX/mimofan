# 更新日志

本项目所有重要变更均记录在此文件中。

格式遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) 规范，
版本号遵循 [语义化版本控制](https://semver.org/spec/v2.0.0.html)。

## [未发布]

## [0.8.65] - 2026-06-24

### 新增

- **Provider/模型/路由解析（EPIC #2608）。** 规范化的 provider、model、offering 和 route 类型，通过统一的 `RouteResolver` 为每次切换生成已解析的 `ReadyRouteCandidate`（端点、线路协议、模型 ID、上下文限制、价格）（#3458、#3084、#3384）。执行客户端现在从已解析的候选构建，而非从配置重新推导（#3384）。提交式、无需网络的 Models.dev 风格目录为模型提供真实的上下文窗口和定价，带有无密钥的实时缓存（#3497、#3498、#3385）。带来源信息的 offering 定价投射到候选上（#3501、#3085），路由限制馈入路由感知的上下文预算服务（#3508、#3523、#3086）。
- **Fleet 执行基底（EPIC #3154）。** Fleet 配置文件类型和配置（#3469），持久化管理器恢复，工作区 agent 配置文件加载解析到 worker 运行时（#3367），任务规格中携带的装载意图（#3512），以及持久化已解析路由供检查的回执（#3154、#3166）。Worker 状态折叠到统一的 `/fleet` 界面，通过 Runtime API 暴露。
- **Provider 界面。** `/provider` 就绪仪表板，包含推理就绪度、实验性/受支持的成熟度标记，以及"查看此 provider 的开放模型"操作（#3083、#2984、#3485）；跨 provider 的 `/model` 搜索，支持滚动和 provider 输入提示（#3484、#3075）；内联 `<think>` 推理流路由，支持 per-provider 覆盖（#3222）；usage 遥测规范化为标准 token 类别，包括 Responses 缓存未命中和 reasoning tokens（#2961、#3509）；以及远程 MCP OAuth 登录，支持 bearer/header 认证优先级（#3527）。
- **更多 provider 和路由。** 通过 `[providers.<name>]` 支持用户自定义的 OpenAI 兼容自定义 provider（#1519）；DeepSeek Anthropic 兼容路由（#2963、#3449）；千帆路由（#3425）；智谱折叠到 Z.ai，模型规范化平等对待（#3539）；DashScope/Together fixtures。
- **本地化的模式选择器和编辑器指示器。** `/mode` 选择器提示、模式名称和提示信息，以及编辑器的 Vim 模式指示器，现在支持全部 7 种已发布语言渲染（面向模型的模式标签保持英文）。来自 @gordonlu 的 #2239。
- **网站和自动化。** 运行时/集成页面、来源和镜像信任说明、事实漂移 CI 门控、已发布的安装脚本，以及 mimofan.net 上的每周社区摘要归档（#3419、#3421、#3415、#3482、#3420）；每个自动化模式/Shell/信任/审批设置（#3467）。

### 变更

- **配置模块化（#3311）。** `ProviderKind`（#3505）、harness 姿态（#3507）和 provider 默认种子（#3503）移入专用模块，`config.rs` 单体拆分为清晰的叶子模块（路径、搜索、模型/基础 URL 常量、子代理限制），通过 `pub use` 门面暴露。`AppMode` 辅助函数集中化（#3510），模式与权限策略现在通过单一的 `base_policy_for_mode` 解析器推导，取代分散的修改（#3386，保留咨询式 review-intent 行为）。
- **精简的工具表面。** 从活跃集中移除 `task_shell_*`，折叠 `tool_search_*`（#3463）；消除了轮内 loop_guard 和编码推理处置（#3462）；向 constitution 添加了 Orchestration 处置。
- **路由。** Provider/模型切换和能力感知回退链通过 `RouteResolver` 解析；reasoning effort 针对*已解析的* provider 规范化；回退链现在跳过缺少认证的 provider（#2574）；上下文窗口和内存压力来自已解析路由（#3086）。
- **用户体验。** 审批模态框增加了分组分隔符和选中行光标（#3515）；选择器滚动/输入提示和选中对比度加固（#3500）；README 重写为架构终篇（#3087）；仓库 agent 指导去除硬编码，改为实时获取。
- **恢复贡献者信用。** 为早期已合入但未署名的工作补充了机器可读的信用信息（`docs/CONTRIBUTORS.md` + `.github/AUTHOR_MAP`），包括 @jieshu666 的 `/jobs cancel-all` 操作和 npm 重试超时提示（#1538），以及 @rockeverm3m 的社区 ACP 适配器参考。

### 修复

- **发布卫生。** 严格的 `cargo clippy --workspace --all-targets --locked -- -D warnings` 门控通过；`npm run build` 不再弄脏生成的 web facts；站点设置了 `metadataBase`；社区摘要页面独立解析每条记录并本地化其 chrome；`cargo audit` 干净，starlark 传递性未维护 advisory 已记录。
- **路由和模式正确性。** 普通提示文本不再被解释为模式切换（#3387、#3491）；模型候选限定在活跃 provider 范围内；Together 拥有的 DeepSeek 路由被接受（#3426）；不安全的 `http://` 自定义端点会发出 advisory 警告（#1519）；Fleet 设置规划器的角色/模型选择现在驱动生成的配置文件。
- **运行时稳定性。** MCP 连接断开变为显式处理（#3524），HTTP API 调用复用共享的 MCP 连接池（#3532），per-agent 子代理邮箱遥测被限流以减少 UI 卡顿（#3454）。
