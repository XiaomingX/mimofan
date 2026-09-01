# reports/summary.md — mimofan Loop Engineer 能力汇总

> 本文件随每次迭代（v1 → v100）更新。基线建立于 2026-08-30。
> 门控指标定义见根目录 `loop_plan.md`。

## 能力矩阵（基线）

| 能力 | 状态 | 默认继承 | 验收样本 | 门控通过 |
|---|---|---|---|---|
| SAST（security_audit 等 6 件套） | 线上仅单文件 semgrep；自研 taint/interproc/auto_gadget 引擎未接线 | ✅ | JSEF（`benchmark/jsef/`） | ⏳ 待 T2-SAST 四阶段爬坡 |
| DAST（run_poc） | 代码存在但默认 backend=None → fail-closed 不可用；与 SAST 无联动 | ⚠️（需配置 sandbox） | JSEF 可动态验证子集 | ❌ 待 T2-DAST D0→D2 |
| 轨迹日志（trace.rs） | 部分（缺维度） | ✅(redact) | 导出测试 | ❌ 待 T4 |
| 无 SK MCP 后端 | 部分（未挂安全工具） | ⚠️ | 端到端冒烟 | ❌ 待 T5 |
| 多套冲突归一化 | 未做 | — | grep 唯一真源 | ❌ 待 T3 |
| 死代码/编排缺口 | 未做 | — | cargo test | ❌ 待 T1 |
| A 长期记忆 | consolidation/injector 完整但 injector 未接 tui 运行时；无跨会话召回量化 | ⚠️（需 MEMORY_API_KEY） | 缺标准跨会话样本 | ❌ 待 T7-A |
| B 长程任务 | goal_loop/loop_guard 完整；评分锚点未标准化 | ✅ | benchmark/long_horizon（有样本） | ❌ 待 T7-B |
| C 复杂任务 | decomposer/task_graph 完整；无端到端成功率度量 | ✅ | 缺 SWE-bench 类样本 | ❌ 待 T7-C |
| D 0day 漏洞挖掘 | 已知 CVE 挖掘链完整；**无任何未知漏洞样本，0day 发现率不可验收** | ✅ | JSEF/vuln_hunt（均已知 CVE） | ❌ 待 T7-D（层2范式待建） |
| T9 多维验收（无 SK） | 漂移/trace/性能/内存/token/提示词成本 等 6 子项门控，均经 T5 MCP 由 claude code 驱动，不耗 mimofan SK | — | 各 `reports/*_vN.md` | ❌ 待 T9（v16） |
| T10 横向对比 agent | 10 项量化指标对比 mimofan vs 对照 agent（成功率/token效率/安全合规等），无 SK 下经 MCP 驱动 | — | `reports/agent_compare_vN.md` | ❌ 待 T10（v17） |
| T8 上游 diff patch | 全部门控通过后固化交付物（相对 origin/main = XiaomingX/mimofan） | — | `reports/upstream_patch.diff` | ❌ 待 T1–T7/T9/T10 全达标 |

## 门控通过率

- 总门控项：T1/T3/T4/T5 + T2-SAST(A/B/C/D) + T2-DAST(D0/D1/D2) + T6 共多类
- 已通过：0 / 全部
- 进行中：基线调研完成，待 v1 实现

## 已知剩余缺口（基线）

1. `staticanalysis/src/index.rs` 死代码（feature-gated 无调用方）。
2. access-control 授权 gate 静态分析缺失；hypothesis 单 verdict 无多验证综合。
3. 轨迹日志缺失维度：user_prompt / agent_think / token_usage；默认 redact 毁后训练价值；缺 export CLI。
4. MCP 未挂安全工具白名单；MCP 调用点不补 emit 轨迹。
5. 三处真冲突：hook 双枚举、轨迹三套、沙箱双抽象。
6. **SAST**：线上仅单文件 semgrep（Recall 低根因），自研跨文件引擎已就绪未接线 → 需 T2-SAST A→D 爬坡至 Recall≥0.95/Precision≥0.95（基于 JSEF）。
7. **DAST**：`run_poc` 默认 backend=None 永远 fail-closed，且与 SAST 无联动、判定仅子串 → 需 T2-DAST D0(默认可用)→D1(SAST 联动)→D2(结构化判定) 建设真正可用动态验证链。
8. JSEF 样本已复制到 `benchmark/jsef/`（2026-08-30），待 T2 跑 harness 验收。

## 迭代历史

（从 v1 起追加，见 `loop_plan.md` 迭代记录段）
