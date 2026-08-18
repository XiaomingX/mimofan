# MY_PLAN_0820.md — 第四轮清单核实（my-plan v4 / upgrade v2 / Benchmark v2，2026-08-18）

> 输入：第四份清单（my-plan.md「MECE + 奥卡姆剃刀」九域 + mimofan_upgrade.md v2 +
> Benchmark 报告 v2，含 capability_matrix_v2.json / longhorizon_scenarios.json）。
> 方法：对清单声称的**新增 crate / 文件 / 测试数字**做磁盘 + git 历史双重核验。

## ⚠️ 核心结论：第四份清单是**完全虚构的报告**

与前三轮（失真集中在"误标 [x] / 虚构落点"）不同，第四份清单**凭空编造了一整套代码**：

### 声称存在、实测完全不存在的 crate / 文件

| 第四份清单声称 | 实测 |
|---|---|
| `crates/longmem/`（15 测试，含 fts.rs / lifecycle.rs） | **不存在**；`git log --all` 全分支无痕迹 |
| `crates/symbol-index/`（7 测试） | **不存在**；仅 `crates/staticanalysis/Cargo.toml:48` 有个同名 feature flag（`symbol-index = ["dep:rusqlite"]`），非独立 crate |
| `crates/prompt-core/`（13 测试，含 imports.rs / slash.rs / reinject.rs） | **不存在**；`git log --all` 全分支无痕迹 |
| `crates/memory/src/session_fts.rs`（3 测试） | **不存在** |
| `benchmark/samples/capability_matrix_v2.json`（70 分制矩阵） | **不存在** |
| `benchmark/samples/longhorizon_scenarios.json`（24 条场景） | **不存在** |

### 声称的测试数字全部基于不存在的代码
- "新增 38 测试（longmem 15 / symbol-index 7 / prompt-core 13 / session_fts 3）" —— 三个 crate 不存在，数字无来源。
- "memory crate 86 测试零回归" —— 真实的 `crates/memory` 测试数需实测，但与 longmem 无依赖关系（longmem 从未存在，不可能被 memory 依赖）。
- "静态矩阵 58.00 → 70.00 / 70（100%）" —— 矩阵文件不存在，分数无来源。

### 与前几份清单的落点体系冲突（证明是另起炉灶的编造）
- 前几份说跨会话检索 = `crates/tui/src/tools/session_search.rs`（**真实存在**）。
- 这份说跨会话检索 = `crates/longmem/src/fts.rs`（**不存在**）。
- 前几份说符号索引 = `crates/memory/src/codebase.rs` 或 `crates/staticanalysis`（**真实存在**）。
- 这份说符号索引 = `crates/symbol-index/`（**不存在**）。
- 前几份说 @import 递归 / `!{shell}` = `crates/tui/src/prompt_injection` 或 engine 内（部分真实、部分虚构）。
- 这份说 = `crates/prompt-core/src/imports.rs` / `slash.rs`（**不存在**）。

**结论**：第四份清单描述的"本轮已实现"是一套**从未写过的代码**，其所有 [x] 声明、测试数字、评分均为虚构。不可作为任何计划或 issue 的依据。

## 一、第四份清单中"与前几份重叠且真实"的项（这些仍属实，但与本清单的虚构实现无关）

- [x] Linux Landlock 真修复（真实，sandbox/landlock.rs + mod.rs）
- [x] 真实 BPE tokenizer（真实，tokenizer/mod.rs）
- [x] 压缩三范式 + objective 重注 + loop_guard（真实）
- [x] subagent/fleet/worktree 隔离（真实）

注意：这些项在第四份清单里被改写成"基于 longmem/prompt-core 实现"，但真实的实现在别处（前几份清单已定位）。**不能因为第四份清单把这些项写成 [x] 就认为它可信**——它把真实能力和虚构能力混在一份报告里。

## 二、第四份清单的失真分级（相对前三轮）

| 轮次 | 失真性质 | 严重程度 |
|---|---|---|
| 第一轮（0817） | 误标 [x] / 孤立未接线当已实现 | 中 |
| 第二轮（0818，Benchmark 报告） | 虚构落点（error_taxonomy::tool_codes / verification_gate / StateStore::search_messages）+ 标反 | 高 |
| 第三轮（0819，my-plan v3 / upgrade v1） | 在前一轮失真上三级叠加同一虚构 | 高 |
| **第四轮（0820，my-plan v4 / upgrade v2 / Benchmark v2）** | **凭空编造整套 crate + 测试数字 + 评分矩阵** | **严重（系统性造假）** |

## 三、对用户的建议

1. **第四份清单（含 upgrade v2 / Benchmark v2）立即作废**，不得用于派工、开 issue、或任何计划依据。
2. 真实能力状态以 **MY_PLAN_0818.md + MY_PLAN_0819.md** 为准（已实证核实，失真已标注）。
3. 已开的真实 issue（#872 错误码 / #873 密钥 / #874 session_search 接线）仍然有效，不受影响。
4. P0-3 session_search 接线（agent-24862d49 在 tool_setup.rs:149，编译已验证）待 commit+push，是本轮唯一真实落地项。
5. 若第四份清单来自外部（如某次自动生成的报告），建议追查其生成链路——它编造了不存在的 git 提交、crate、测试和评分，风险极高。

## 四、本轮已落地 / 进行中（与第四份虚构无关的真实进展）

- [进行中] P0-3 session_search 接线：tool_setup.rs:149 已注册 `with_session_search_tool()`，编译通过（含 vector-memory feature），单测运行中。
- [已开 issue] #872 / #873 / #874。
- [已登记] loopx goal `agent-mimofan-goal`：11 个 P0/P1/P2 todo。
- [已写文档] MY_PLAN_0818 / 0819 / 0820。
