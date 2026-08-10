# mimofan v0.0.10

本次发布聚焦**能力优化**：补齐漏洞挖掘分析底座、统一 token 计数精度、强化长程任务稳定性，并提升 Provider 配置兼容性。

## 新增能力

- **LSP request/response 基础设施** ([issue #597](https://github.com/XiaomingX/mimofan/issues/597))
  新增 `document_symbols` / `references` / `definition` 请求层与 `serverCapabilities` 解析，是后续跨过程数据流分析（MECE L0）的前置底座。
- **记忆分类索引 + 按需加载** ([PR #583](https://github.com/XiaomingX/mimofan/pull/583))
  对齐 CodeBuddy 记忆机制，按四分类体系建立索引并懒加载，降低内存与启动开销。

## 改进

- **Provider 配置增强**：`anthropic-compatible` 模式新增识别 `ANTHROPIC_AUTH_TOKEN`，并在 `README.md` 与 `config.example.toml` 补充环境变量配置说明。
- **真实 BPE 统一全库 token 计数**：修复中文 token 系统性低估，将 `seam_manager` / `context_inspector` / `injector` 三处估算收敛到共享 BPE 计数，压缩与上下文预算估算更准确。

## 修复

- **编辑工具正确性三项修复**：读范围授权 / `replace_all` / BOM+CRLF 保真。
- **回合内循环检测（loop_guard）**：恢复循环 / 重复 / 停滞检测，缓解长程任务目标漂移。
- **抑制 libring linker_messages 警告**：编译零 warning。

## 文档

- **README 去 emoji 提升专业性** ([PR #584](https://github.com/XiaomingX/mimofan/pull/584))。

## 已知进行中（未含入本版本）

MECE 漏洞挖掘四层重构（[issue #596](https://github.com/XiaomingX/mimofan/issues/596) Epic 及 #586/#598/#599/#600/#601/#602/#603/#604 系列）仍在规划/实现中，本版本仅落地其 L0 前置（#597）。

## 资产

- `mimofan-v0.0.10-x86_64-apple-darwin.tar.gz`：macOS Intel 二进制包。
- 其他平台（macOS arm64 / Linux musl x64+arm64 / Windows x64）由 CI 发布矩阵产出。
