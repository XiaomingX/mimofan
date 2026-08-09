# mimofan MECE 1000 题能力基准评测结论

> 评测引擎：`benchmark/agentbench/mece_bench.py`（分层评分：T1 静态 grep / T2 结构断言 / T3 真实执行）
> 条目集：`benchmark/agentbench/samples/mece_1000/`（1006 条，MECE 合规 ERROR=0）
> 评分口径：A 类静态能力矩阵（60 分）+ B 类动态指标（40 分），总分 100
> 反作弊：T1 单独系数 0.5，仅当同 `assert_key` 存在通过的 T3 时升 1.0；T2/T3 系数 1.0
> 评分器加固：`test_passes` 断言 `passed >= 1`，封堵用例名写错导致的静默假阳性（最坏虚高 8.90% 已消除）

## 一、总览

| 轮次 | Commit | 总分 | 通过/总条目 | A 类 | B 类 |
|---|---|---|---|---|---|
| 基线 | ccce132 | **待填** | 待填 | 待填 | 待填 |
| 改进 | f4cf663 | **待填** | 待填 | 待填 | 待填 |
| 变化 | Δ | **待填** | — | 待填 | 待填 |

（分数由双轮评测实跑得出，见 `/tmp/baseline.json` 与 `/tmp/after.json`。）

## 二、优势范畴（mimofan 已具备且质量较高的能力）

### D04 压缩与 tokenizer
- **真实 BPE tokenizer**（`crates/tui/src/tokenizer/`）：采用 tiktoken 风格 BPE，含 12 条冻结真值测试，对中文系统性低估已修复（全库收敛到统一入口）。
- **压缩三范式并存**：语义压缩（ObservationCompressor）、结构化裁剪（seam/summary）、预算压力分级（context_budget）三者齐备，优于多数对标产品仅单一 compaction。

### D06 循环/停滞守卫与显式状态机
- `loop_guard`（`turn_loop.rs:156` 重建，`2450/2539` 调用 `observe`）已接线，提供回合内重复/停滞三维检测，属近期高质量实现（commit `6801a72`）。
- `goal_loop::decide_continuation` 在 `turn_loop.rs:2805` 与 `engine_messages.rs:259` 两处真实接线，计划驱动闭环成立。

### D01 文件编辑保真
- `FileFidelity`（`2284d7a`）已实现 BOM 剥离/还原、CRLF 归一化；`replace_all` 语义已落地。

## 三、不足范畴（能力缺口，已诚实写成会失败的条目）

### D05 记忆生命周期治理（最大空洞）
- `promote / decay / prune / consolidate / TTL / evict` 在 `crates/memory/` + vector_memory 中**零命中**：记忆只增不减，无淘汰入口；`Observation` 无重要性/置信度字段；检索打分纯 L2 距离不含时间衰减（十个月前与昨天的记忆等权）；无短期→长期晋升；HNSW 硬编码 `max_elements=20000` 无淘汰策略。
- **双存储静默失步**：`vector.rs:445` `count()` 只查 SQLite，sled 侧失步不可见；`delete_observation` 留 HNSW 残影（源码 `// TODO` 自陈）；多表写入无事务包裹。
- **写入策略空白**：无内容去重、无新旧事实冲突合并、无「何时值得记」的自动萃取。

### D04 计数统一未收尾
- `seam_manager/mod.rs:213/:324` 仍是 `summary.len()/4` **字节**除法（正是 tokenizer 文档批判的 bug 原型，中文低估约 4 倍）；`context_budget` 的 chars/3 与 chars/4 两套比值并存；`memory/injector.rs:231` 自复制 `chars().count()/4` 与全库唯一入口脱节。

### D06 任务依赖图缺失
- `TaskRecord` 无 `blocked_by/depends_on` 字段，无环检测；`tools/tasks.rs` 仅有 `"blocked": true` 输出字面量；`approved_plan` 存了但无偏离检测闸门，步骤完成与否纯靠模型自报。

### D12 沙箱仅 macOS Seatbelt
- Landlock/Windows 仅存在于注释，实际 `SandboxType` 只有 `None + MacosSeatbelt`。跨平台沙箱能力缺口明显。

### 测试覆盖缺口（评测意外暴露）
- `list_jobs` / `SPILLOVER_HEAD_BYTES` / `seatbelt::is_available` / `is_allowed_parent_env_key` **四项能力零测试覆盖**（符号存在但四文件仅 truncate.rs 有 `cfg(test)`）。

## 四、评测有效性说明

- 评分器已加固 `passed >= 1`，T3 用例名写错不再静默假通过。
- 4 条 `--list` 型 T3 条目考察「测试存在性」，报红为语义正确的真阳性（对应上述零覆盖缺口），保持不降级。
- 双轮评测各用独立 `CARGO_TARGET_DIR`（`/tmp/mece_base_target`、改进轮独立目录），避开 cargo 锁竞争；环境 `--remap-cwd-prefix` bug 已修复，未加 RUSTFLAGS 前缀。

## 五、结论

（待双轮评测分数填充：mimofan 在 D04/D06/D01 静态与执行层面表现扎实；主要短板集中在 D05 记忆生命周期治理与 D04 计数收敛尾部，建议作为下一阶段 P0 投入方向。）
