# 编译配置审查（macOS 目标）

> 范围：仅 macOS（`x86_64-apple-darwin` / `aarch64-apple-darwin`）。不考虑 Windows / Linux。
> 目标：排查「长期一致等待」、减少编译垃圾文件、减少编译时间、更快得到结果。
> 约束：只做存量优化与化简，**不新增实体**（如 sccache 这类外部工具默认不引入），不强行写可有可无的改动。

---

## 一、现状盘点

| 位置 | 内容 | 评价 |
|---|---|---|
| `.cargo/config.toml` | **仅注释，无实际配置**（注释明确：profile 只在 workspace `Cargo.toml` 定义，避免历史冲突） | ✅ 良好，保持 |
| `Cargo.toml` `[profile.dev]` | `debug = "line-tables-only"`、`split-debuginfo = "unpacked"`、`codegen-units = 16` | ✅ 已是 macOS 最佳实践 |
| `Cargo.toml` `[profile.dev.package."*"]` | `opt-level = 3`（所有依赖在 dev 也按 3 优化） | ⚠️ 见下文「唯一显著 lever」 |
| `Cargo.toml` `[profile.release]` | `strip = true`、`lto = "thin"`、`codegen-units = 16`、`split-debuginfo = "unpacked"` | ✅ 良好 |
| `crates/tui/build.rs` / `crates/cli/build.rs` | 存在 build script | 常规，非等待主因 |
| `Cargo.lock` | 已提交，锁定依赖 | ✅ 保证可重现、避免重复解析等待 |

**结论**：调试信息量与链接速度这两个「垃圾文件 / 编译时间」主因，当前配置**已符合 macOS 最佳实践**，无需为「已经做对的事」再写改动。

---

## 二、「长期一致等待」的真正根因

不是 profile 配置，而是**构建入口脚本跑全量 gauntlet**：

`redeploy.sh` 默认流程：`cargo fmt --check` → `cargo clippy --workspace --all-features --locked -D warnings` → `cargo test --workspace --locked` → `cargo build --release`。
在 x86 Mac 上，整条链路对 600+ crate 重写一遍，单次往往数十分钟，这就是你感知到的「长期等待」。

> 注：`opt-level = 3` 让所有 dev 依赖也走满优化，进一步拉长 dev 编译（见下）。

---

## 三、存量优化建议（按性价比排序，均为配置/流程层，无新实体）

### 1. 本地快速构建：绕开全量 gauntlet【立竿见影】
- 日常迭代用：`cargo build -p mimofan`（debug，仅编默认成员）或
  `./redeploy.sh --debug --skip-fmt --skip-clippy --skip-test`
- 只在发布 / CI 才跑完整 `redeploy.sh`。
- 效果：把「数十分钟」降到「仅编改动 crate + 其依赖」。

### 2. dev 依赖优化级别：3 → 1【唯一显著缩短 dev 编译的 lever，属权衡】
- 当前 `opt-level = 3` 让 dev 构建也优化全部依赖，是其慢的主因之一。
- 改为 `opt-level = 1`：dev 编译时间显著下降，依赖运行时略慢（开发期可接受）；**release 不受影响**（release 单独设了 `lto`/`strip`）。
- 这是既定「release 体验优先」取向与「dev 编译速度」之间的权衡，**不强行改**，需你拍板。若要动，只改 `[profile.dev.package."*"]` 与 `[profile.test.package."*"]` 两处。

### 3. `target/` 垃圾清理：用精准清理替代全量 `cargo clean`【避免下次全量重编】
- `cargo clean` 会清空整个 `target/`，下次 100% 全量重编 = 又一轮长期等待。
- 只清某个 crate：`cargo clean -p <crate_name>`。
- 把 `target/` 放在本地 SSD（默认即在项目内，OK）。

### 4. 注释笔误修正【化简，非功能变更】
- `[profile.dev]` 注释写「Fewer codegen-units = fewer .o files」，但取值是 `16`（高并行、文件更多）。两行自相矛盾，易误导。建议改为「Higher codegen-units = more parallelism, faster compile（更多 .o 文件，但更快）」，或直接删掉误导性注释。

### 5. 不引入 sccache【遵守「不增实体」约束】
- sccache 是削减重编时间的正统手段，但它是一个**新增外部工具/实体**，与你的约束冲突。当前不引入；若后续仍痛，可单独评估（那时需你明确同意）。

---

## 四、关于「只编译 macOS」与 #21（弃用 Linux/Windows）

- 代码中 linux/windows 依赖**已被 `cfg` 门控**（`crates/cli`、`crates/secrets`、`crates/tui` 的 `[target.'cfg(...)'` 段）。在 macOS 上构建时，这些目标依赖**不会被编译**——因此 #21「弃用多平台」**不影响 macOS 编译速度/垃圾量**，它纯属「官方维护范围」收敛。
- 若按 #21 推进，建议只从 CI / 发布矩阵移除非 macOS，可选删除对应 `[target]` 段；对本地 macOS 编译无收益，故此处不做。

---

## 五、收口结论

- 编译配置**已达 macOS 最佳实践**，不为「已做对的部分」编造待办。
- 真正能减少等待的是**构建入口用法**（建议 1）与可选的 **dev 依赖 opt-level 权衡**（建议 2）。
- 未做强制改动：因当前配置本身合理，强行改反而是「可有可无的待办」，违反你的要求。上述建议均为可选、可逆、配置/流程层。
