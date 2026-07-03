# 死代码清理执行清单

## 总览

- **总警告数**: 465
- **Unreachable pattern**: 273
- **其他警告**: 192
- **预计清理时间**: 3.5 小时

## Phase 1: 低风险删除 (30 分钟)

### 1.1 删除未使用的导入

#### `crates/tui/src/constants/mod.rs`
- [ ] 删除 `pub use limits::*;` (行 62)
- [ ] 删除 `pub use paths::*;` (行 63)
- [ ] 删除 `pub use tokens::*;` (行 64)
- [ ] 删除 `pub use ui::*;` (行 65)

#### `crates/tui/src/errors/mod.rs`
- [ ] 删除 `ErrorCategory`, `ErrorEnvelope`, `ErrorSeverity` 导入 (行 37)

### 1.2 删除未使用的变量

#### 待定位文件
- [ ] 删除 `setting` 变量
- [ ] 删除 `env` 变量

### 1.3 修复命名问题

#### `crates/protocol/src/fleet.rs:275`
- [ ] 将 `mimofanVerifierPrompt` 改为 `MimofanVerifierPrompt`

### 1.4 删除未使用的常量 (分批)

#### 第一批: Provider 配置常量 (50 个)
- [ ] `DEFAULT_ANTHROPIC_MODEL`, `DEFAULT_ANTHROPIC_BASE_URL`
- [ ] `DEFAULT_ARCEE_MODEL`, `DEFAULT_ARCEE_BASE_URL`
- [ ] `DEFAULT_ATLASCLOUD_MODEL`, `DEFAULT_ATLASCLOUD_BASE_URL`
- [ ] `DEFAULT_DEEPINFRA_MODEL`, `DEFAULT_DEEPINFRA_FLASH_MODEL`, `DEFAULT_DEEPINFRA_BASE_URL`
- [ ] `DEFAULT_DEEPSEEK_ANTHROPIC_MODEL`, `DEFAULT_DEEPSEEK_ANTHROPIC_BASE_URL`
- [ ] `DEFAULT_DEEPSEEKCN_BASE_URL`
- [ ] `DEFAULT_FIREWORKS_MODEL`, `DEFAULT_FIREWORKS_BASE_URL`
- [ ] `DEFAULT_HUGGINGFACE_MODEL`, `DEFAULT_HUGGINGFACE_FLASH_MODEL`, `DEFAULT_HUGGINGFACE_BASE_URL`
- [ ] `DEFAULT_KIMI_CODE_MODEL`, `DEFAULT_KIMI_CODE_BASE_URL`
- [ ] `DEFAULT_MINIMAX_MODEL`, `DEFAULT_MINIMAX_BASE_URL`
- [ ] `DEFAULT_MOONSHOT_MODEL`, `DEFAULT_MOONSHOT_BASE_URL`
- [ ] `DEFAULT_NOVITA_MODEL`, `DEFAULT_NOVITA_FLASH_MODEL`, `DEFAULT_NOVITA_BASE_URL`
- [ ] `DEFAULT_NVIDIA_NIM_MODEL`, `DEFAULT_NVIDIA_NIM_FLASH_MODEL`
- [ ] `DEFAULT_OPENAI_MODEL`, `DEFAULT_OPENAI_BASE_URL`
- [ ] `DEFAULT_OPENAI_CODEX_MODEL`, `DEFAULT_OPENAI_CODEX_BASE_URL`
- [ ] `DEFAULT_OPENROUTER_MODEL`, `DEFAULT_OPENROUTER_FLASH_MODEL`, `DEFAULT_OPENROUTER_BASE_URL`
- [ ] `DEFAULT_QIANFAN_MODEL`, `DEFAULT_QIANFAN_BASE_URL`
- [ ] `DEFAULT_SILICONFLOW_MODEL`, `DEFAULT_SILICONFLOW_FLASH_MODEL`, `DEFAULT_SILICONFLOW_BASE_URL`, `DEFAULT_SILICONFLOW_CN_BASE_URL`
- [ ] `DEFAULT_STEPFUN_MODEL`, `DEFAULT_STEPFUN_BASE_URL`
- [ ] `DEFAULT_TOGETHER_MODEL`, `DEFAULT_TOGETHER_FLASH_MODEL`, `DEFAULT_TOGETHER_BASE_URL`
- [ ] `DEFAULT_VOLCENGINE_BASE_URL`
- [ ] `DEFAULT_WANJIE_ARK_MODEL`, `DEFAULT_WANJIE_ARK_BASE_URL`
- [ ] `DEFAULT_ZAI_MODEL`, `DEFAULT_ZAI_BASE_URL`

#### 第二批: Model ID 常量 (30 个)
- [ ] `ANTHROPIC_OPUS_MODEL`, `ANTHROPIC_HAIKU_MODEL`
- [ ] `ARCEE_TRINITY_LARGE_PREVIEW_MODEL`, `ARCEE_TRINITY_MINI_MODEL`
- [ ] `MINIMAX_M2_MODEL`, `MINIMAX_M2_1_MODEL`, `MINIMAX_M2_1_HIGHSPEED_MODEL`
- [ ] `MINIMAX_M2_5_MODEL`, `MINIMAX_M2_5_HIGHSPEED_MODEL`
- [ ] `MINIMAX_M2_7_MODEL`, `MINIMAX_M2_7_HIGHSPEED_MODEL`
- [ ] `MOONSHOT_KIMI_K2_6_MODEL`
- [ ] `OPENROUTER_ARCEE_TRINITY_LARGE_THINKING_MODEL`
- [ ] `OPENROUTER_GEMMA_4_31B_MODEL`, `OPENROUTER_GEMMA_4_26B_A4B_MODEL`
- [ ] `OPENROUTER_KIMI_K2_7_CODE_MODEL`, `OPENROUTER_KIMI_K2_6_MODEL`
- [ ] `OPENROUTER_MINIMAX_M3_MODEL`, `OPENROUTER_MINIMAX_M2_7_MODEL`
- [ ] `OPENROUTER_NEMOTRON_3_ULTRA_MODEL`, `OPENROUTER_NEMOTRON_3_NANO_OMNI_MODEL`
- [ ] `OPENROUTER_QWEN_3_6_27B_MODEL`, `OPENROUTER_QWEN_3_6_35B_A3B_MODEL`
- [ ] `OPENROUTER_QWEN_3_6_FLASH_MODEL`, `OPENROUTER_QWEN_3_6_PLUS_MODEL`, `OPENROUTER_QWEN_3_6_MAX_PREVIEW_MODEL`
- [ ] `OPENROUTER_QWEN_3_7_MAX_MODEL`
- [ ] `OPENROUTER_TENCENT_HY3_PREVIEW_MODEL`
- [ ] `OPENROUTER_XIAOMI_MIMO_V2_5_MODEL`, `OPENROUTER_XIAOMI_MIMO_V2_5_PRO_MODEL`
- [ ] `ZAI_GLM_5_1_MODEL`

#### 第三批: 路径和文件名常量 (15 个)
- [ ] `APP_DIR`, `PREVIOUS_APP_DIR`, `LEGACY_APP_DIR`
- [ ] `ARTIFACTS_DIR_NAME`
- [ ] `CONFIG_FILE_NAME`, `SETTINGS_JSON_FILE_NAME`
- [ ] `STATE_FILE_NAME`, `HISTORY_FILE_NAME`
- [ ] `PERMISSIONS_FILE_NAME`, `TRUST_FILE_NAME`
- [ ] `DEPRECATED_WHALE_FILENAME`
- [ ] `HANDOFF_RELATIVE_PATH`, `LEGACY_HANDOFF_RELATIVE_PATH`

#### 第四批: UI 常量 (12 个)
- [ ] `WHALE_BG_RGB`, `WHALE_PANEL_RGB`, `WHALE_ELEVATED_RGB`, `WHALE_SELECTION_RGB`
- [ ] `WHALE_ACCENT_ACTION_RGB`, `WHALE_ACCENT_PRIMARY_RGB`, `WHALE_ACCENT_SECONDARY_RGB`
- [ ] `WHALE_TEXT_BODY_RGB`, `WHALE_TEXT_DIM_RGB`, `WHALE_TEXT_HINT_RGB`, `WHALE_TEXT_MUTED_RGB`, `WHALE_TEXT_SOFT_RGB`
- [ ] `HOTBAR_SLOT_COUNT`

#### 第五批: 上下文和会话常量 (15 个)
- [ ] `CONTEXT_HEADROOM_TOKENS`
- [ ] `CURRENT_SESSION_SCHEMA_VERSION`, `CURRENT_QUEUE_SCHEMA_VERSION`
- [ ] `DEFAULT_COMPACTION_TRIGGER_PERCENT`, `DEFAULT_COMPACTION_TOKEN_THRESHOLD`
- [ ] `DEFAULT_SPAWN_DEPTH`, `MAX_SPAWN_DEPTH_CEILING`
- [ ] `FILE_INDEX_MAX_ENTRIES`, `LOCAL_REFERENCE_SCAN_LIMIT`
- [ ] `IGNORED_ROOT_DIRS`
- [ ] `INSTRUCTIONS_FILE_MAX_BYTES`
- [ ] `MAX_MEMORY_SIZE`, `MAX_SESSIONS`
- [ ] `MIN_INPUT_BUDGET_TOKENS`

#### 第六批: 压力阈值常量 (3 个)
- [ ] `CRITICAL_PRESSURE_PERCENT`, `HIGH_PRESSURE_PERCENT`, `MEDIUM_PRESSURE_PERCENT`

#### 第七批: 其他常量 (10 个)
- [ ] `DEEPSEEK_ALIAS_RETIREMENT_DATE`, `DEEPSEEK_ALIAS_RETIREMENT_UTC`, `DEEPSEEK_ALIAS_REPLACEMENT`
- [ ] `LEGACY_DEEPSEEK_CONTEXT_WINDOW_TOKENS`
- [ ] `OFFICIAL_DEEPSEEK_MODELS`
- [ ] `OPENAI_CODEX_EFFECTIVE_CONTEXT_WINDOW_TOKENS`
- [ ] `RECENT_OPENROUTER_LARGE_MODELS`
- [ ] `CODEX_CLIENT_ID`, `KIMI_CODE_CLIENT_ID`, `TOKEN_URL`

#### 第八批: 关联常量 (1 个)
- [ ] `KIND_LOOKUP` (在 `crates/tui/src/tui/ui.rs`)

---

## Phase 2: 中等风险删除 (1 小时)

### 2.1 删除未使用的函数

#### `crates/tui/src/config/mod.rs` (9 个)
- [ ] `canonical_arcee_model_id`
- [ ] `canonical_minimax_model_id`
- [ ] `canonical_moonshot_model_id`
- [ ] `canonical_official_deepseek_model_id`
- [ ] `canonical_openrouter_recent_model_id`
- [ ] `canonical_zai_model_id`
- [ ] `provider_entry_uses_custom_base_url`
- [ ] `root_deepseek_model_is_foreign_to_direct_provider`
- [ ] `siliconflow_base_url_is_official`

#### `crates/tui/src/kimi_oauth.rs` (15 个)
- [ ] `auth_mode_uses_kimi_oauth`
- [ ] `extract_account_id_from_id_token`
- [ ] `jwt_expiry_seconds`
- [ ] `kimi_cli_oauth_access_token`
- [ ] `kimi_oauth_access_token_is_fresh`
- [ ] `moonshot_base_url_uses_kimi_code`
- [ ] `normalize_auth_mode`
- [ ] `now_unix_secs`
- [ ] `provider_config_uses_kimi_oauth`
- [ ] `refresh_access_token`
- [ ] `refresh_kimi_oauth_token`
- [ ] `save_credentials`
- [ ] `token_is_expired`
- [ ] `write_kimi_oauth_credential`
- [ ] `get_credentials`

#### `crates/tui/src/error_taxonomy.rs` (7 个)
- [ ] `all_error_type_metadata`
- [ ] `all_state_machine_metadata`
- [ ] `all_status_metadata`
- [ ] `error_types_in_file`
- [ ] `find_error_type`
- [ ] `find_state_machine`
- [ ] `state_machines_in_file`

#### `crates/tui/src/localization.rs` (7 个)
- [ ] `english`
- [ ] `japanese`
- [ ] `portuguese_brazil`
- [ ] `spanish_latin_america`
- [ ] `traditional_chinese`
- [ ] `vietnamese`
- [ ] `chrono_humanize_if_available`

### 2.2 删除未使用的结构体

#### `crates/tui/src/error_taxonomy.rs` (3 个)
- [ ] `ErrorTypeMetadata`
- [ ] `StateMachineMetadata`
- [ ] `StatusMetadata`

### 2.3 删除未使用的枚举变体

#### 待定位文件 (2 个枚举)
- [ ] 删除 `Optional` 和 `Local` 变体
- [ ] 删除 `Turn`, `Agent`, `Task`, `Connection`, `Ui`, `Other` 变体

### 2.4 删除未读取的字段

#### `crates/tui/src/kimi_oauth.rs` (3 个)
- [ ] `access_token` 字段
- [ ] `refresh_token` 字段
- [ ] `exp` 字段

---

## Phase 3: 高风险删除 (2 小时)

### 3.1 修复不可达模式

#### `crates/tui/src/tui/ui.rs` (273 个)
- [ ] 审查行 6581: `ApiProvider::XiaomiMimo | ApiProvider::XiaomiMimo` 重复
- [ ] 审查行 6811: `ApiProvider::XiaomiMimo | ApiProvider::XiaomiMimo` 重复
- [ ] 审查其他 271 个重复模式
- [ ] 删除重复的模式匹配分支
- [ ] 确保保留的分支逻辑正确

---

## 执行命令

### 验证命令
```bash
# 格式化代码
cargo fmt

# 检查警告
cargo clippy --workspace --all-features --locked

# 编译
cargo build -p mimofan --bin mimofan-tui --locked

# 测试
cargo test -p mimofan-tui --locked
```

### Git 操作
```bash
# 创建备份分支
git checkout -b backup/dead-code-cleanup-YYYYMMDD

# 提交每个阶段
git add -A
git commit -m "chore: remove unused imports and variables (Phase 1.1-1.2)"
git commit -m "chore: remove unused constants (Phase 1.4)"
git commit -m "chore: remove unused functions and structs (Phase 2)"
git commit -m "fix: remove unreachable patterns (Phase 3)"
```

---

## 风险评估

### 低风险 (Phase 1)
- 未使用的导入、变量、常量
- 命名问题
- **风险**: 极低，这些代码完全无用

### 中等风险 (Phase 2)
- 未使用的函数、结构体、枚举变体
- **风险**: 中等，可能存在间接引用或测试依赖

### 高风险 (Phase 3)
- 不可达模式
- **风险**: 高，可能影响业务逻辑

---

## 预期收益

1. **编译警告**: 从 465 个减少到 0 个
2. **代码行数**: 预计减少 2000-3000 行
3. **二进制大小**: 预计减少 5-10%
4. **编译速度**: 预计提升 5-10%
5. **可维护性**: 显著提高

---

## 注意事项

1. **分批执行**: 每完成一批，验证编译和测试
2. **保留备份**: 使用 git 分支备份
3. **文档更新**: 删除代码后更新相关文档
4. **测试覆盖**: 确保删除的代码没有测试依赖
5. **团队沟通**: 通知团队成员正在进行的清理工作
