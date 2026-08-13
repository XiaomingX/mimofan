//! LSP 符号导航工具集：`lsp_document_symbols` / `lsp_find_references` /
//! `lsp_goto_definition`。
//!
//! 这三个工具把 [`crate::lsp::LspManager`] 已有的符号查询能力暴露给 agent，
//! 让模型能直接问「这个文件里有哪些符号」「谁引用了它」「它定义在哪」，
//! 而不是退化成全仓库 grep。
//!
//! 设计约定：
//!
//! - **只读**：三个工具都不修改任何文件，`capabilities()` 返回
//!   [`ToolCapability::ReadOnly`]，审批策略为 [`ApprovalRequirement::Auto`]，
//!   并且允许并行执行。
//! - **行列 1-based**：对外暴露的 `line`/`column` 与编辑器显示一致（从 1 开
//!   始），内部由 [`crate::lsp::client`] 转换为 LSP 的 0-based 坐标。
//! - **尽力而为**：LSP 服务端缺失、不支持某个方法或查询超时，统一降级为空
//!   结果而不是报错——这与 `LspManager` 的既有语义保持一致。真正的错误只有
//!   两类：LSP 未配置（`lsp_manager` 为 `None`）和输入非法（文件不存在等）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};
use crate::lsp::{LspLocation, LspManager, LspSymbol};

/// `wait_ms` 缺省时的兜底等待时长（毫秒）。实际优先取
/// `[lsp] poll_after_edit_ms`，两者都拿不到时用这个值。
const DEFAULT_WAIT_MS: u64 = 2_000;

/// `wait_ms` 的上限，避免模型传入一个超大值把整个回合挂住。
const MAX_WAIT_MS: u64 = 60_000;

// === 输出结构 ===

/// 序列化后的符号节点，保留嵌套结构以便模型看清文件大纲。
#[derive(Debug, Clone, Serialize)]
struct SymbolOut {
    name: String,
    /// LSP `SymbolKind` 数字码。
    kind: u64,
    /// `kind` 对应的可读名称，便于模型理解（未知码为 `null`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    kind_name: Option<&'static str>,
    /// 1-based 行号。
    line: u32,
    /// 1-based 列号。
    column: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<SymbolOut>,
}

impl From<&LspSymbol> for SymbolOut {
    fn from(sym: &LspSymbol) -> Self {
        Self {
            name: sym.name.clone(),
            kind: sym.kind,
            kind_name: symbol_kind_name(sym.kind),
            line: sym.line,
            column: sym.column,
            children: sym.children.iter().map(SymbolOut::from).collect(),
        }
    }
}

/// 序列化后的位置节点。`path` 尽量相对 workspace 展示，减少 token 占用。
#[derive(Debug, Clone, Serialize)]
struct LocationOut {
    path: String,
    /// 1-based 行号。
    line: u32,
    /// 1-based 列号。
    column: u32,
}

impl LocationOut {
    fn new(loc: &LspLocation, workspace: &Path) -> Self {
        Self {
            path: display_path(&loc.path, workspace),
            line: loc.line,
            column: loc.column,
        }
    }
}

// === lsp_document_symbols ===

/// 列出单个文件内定义的符号（函数/类型/方法……）。
pub struct LspDocumentSymbolsTool;

#[async_trait]
impl ToolSpec for LspDocumentSymbolsTool {
    fn name(&self) -> &'static str {
        "lsp_document_symbols"
    }

    fn description(&self) -> &'static str {
        "List symbols (functions, types, methods) defined in a source file via the language server. \
         Returns a nested outline with 1-based line/column positions. \
         Returns an empty list when no language server is available for the file's language."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "Path to the source file, absolute or relative to the workspace root."
                },
                "wait_ms": {
                    "type": "number",
                    "description": "Max milliseconds to wait for the language server. Defaults to the configured LSP poll timeout.",
                    "minimum": 0,
                    "maximum": MAX_WAIT_MS
                }
            },
            "required": ["file"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let manager = require_manager(context)?;
        let file = resolve_existing_file(&input, context)?;
        let wait = resolve_wait(&input, manager)?;

        let symbols = manager.document_symbols_for(&file, wait).await;
        let out: Vec<SymbolOut> = symbols.iter().map(SymbolOut::from).collect();
        let total = count_symbols(&out);

        ToolResult::json(&json!({
            "file": display_path(&file, &context.workspace),
            "symbol_count": total,
            "symbols": out,
        }))
        .map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

// === lsp_find_references ===

/// 查找某个位置上符号的全部引用。
pub struct LspFindReferencesTool;

#[async_trait]
impl ToolSpec for LspFindReferencesTool {
    fn name(&self) -> &'static str {
        "lsp_find_references"
    }

    fn description(&self) -> &'static str {
        "Find all references to the symbol at a given 1-based line/column via the language server. \
         Use this instead of a text search when you need semantic accuracy. \
         Returns an empty list when no language server is available."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "Path to the source file, absolute or relative to the workspace root."
                },
                "line": {
                    "type": "number",
                    "description": "1-based line number of the symbol.",
                    "minimum": 1
                },
                "column": {
                    "type": "number",
                    "description": "1-based column number of the symbol.",
                    "minimum": 1
                },
                "include_declaration": {
                    "type": "boolean",
                    "description": "Include the symbol's own declaration in the results. Defaults to true."
                },
                "wait_ms": {
                    "type": "number",
                    "description": "Max milliseconds to wait for the language server. Defaults to the configured LSP poll timeout.",
                    "minimum": 0,
                    "maximum": MAX_WAIT_MS
                }
            },
            "required": ["file", "line", "column"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let manager = require_manager(context)?;
        let file = resolve_existing_file(&input, context)?;
        let line = required_position(&input, "line")?;
        let column = required_position(&input, "column")?;
        let include_declaration = input
            .get("include_declaration")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let wait = resolve_wait(&input, manager)?;

        let locations = manager
            .references_for(&file, line, column, include_declaration, wait)
            .await;
        let out: Vec<LocationOut> = locations
            .iter()
            .map(|loc| LocationOut::new(loc, &context.workspace))
            .collect();

        ToolResult::json(&json!({
            "file": display_path(&file, &context.workspace),
            "line": line,
            "column": column,
            "include_declaration": include_declaration,
            "reference_count": out.len(),
            "references": out,
        }))
        .map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

// === lsp_goto_definition ===

/// 跳转到某个位置上符号的定义处。
pub struct LspGotoDefinitionTool;

#[async_trait]
impl ToolSpec for LspGotoDefinitionTool {
    fn name(&self) -> &'static str {
        "lsp_goto_definition"
    }

    fn description(&self) -> &'static str {
        "Resolve the definition site of the symbol at a given 1-based line/column via the language \
         server. Returns null when the symbol has no definition or no language server is available."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "Path to the source file, absolute or relative to the workspace root."
                },
                "line": {
                    "type": "number",
                    "description": "1-based line number of the symbol.",
                    "minimum": 1
                },
                "column": {
                    "type": "number",
                    "description": "1-based column number of the symbol.",
                    "minimum": 1
                },
                "wait_ms": {
                    "type": "number",
                    "description": "Max milliseconds to wait for the language server. Defaults to the configured LSP poll timeout.",
                    "minimum": 0,
                    "maximum": MAX_WAIT_MS
                }
            },
            "required": ["file", "line", "column"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let manager = require_manager(context)?;
        let file = resolve_existing_file(&input, context)?;
        let line = required_position(&input, "line")?;
        let column = required_position(&input, "column")?;
        let wait = resolve_wait(&input, manager)?;

        let definition = manager.definition_for(&file, line, column, wait).await;
        let out = definition
            .as_ref()
            .map(|loc| LocationOut::new(loc, &context.workspace));

        ToolResult::json(&json!({
            "file": display_path(&file, &context.workspace),
            "line": line,
            "column": column,
            "found": out.is_some(),
            "definition": out,
        }))
        .map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

// === 共用辅助函数 ===

/// 取出 `lsp_manager`，未配置时给出明确错误而不是静默返回空结果——否则模型
/// 会把「LSP 没开」误读为「这个符号没有引用」。
fn require_manager(context: &ToolContext) -> Result<&LspManager, ToolError> {
    context
        .lsp_manager
        .as_deref()
        .ok_or_else(|| ToolError::execution_failed("LSP is not configured for this workspace"))
}

/// 解析并校验 `file` 参数：必须存在且是普通文件。
fn resolve_existing_file(input: &Value, context: &ToolContext) -> Result<PathBuf, ToolError> {
    let raw = input
        .get("file")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::missing_field("file"))?;
    if raw.trim().is_empty() {
        return Err(ToolError::invalid_input("'file' must not be empty"));
    }

    let path = context.resolve_path(raw)?;
    if !path.exists() {
        return Err(ToolError::invalid_input(format!(
            "File does not exist: {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(ToolError::invalid_input(format!(
            "Not a regular file: {}",
            path.display()
        )));
    }
    Ok(path)
}

/// 读取 1-based 的行或列。缺失、非数字或小于 1 都视为输入错误。
fn required_position(input: &Value, field: &str) -> Result<u32, ToolError> {
    let raw = input
        .get(field)
        .ok_or_else(|| ToolError::missing_field(field))?;
    let value = raw
        .as_u64()
        .ok_or_else(|| ToolError::invalid_input(format!("'{field}' must be a positive integer")))?;
    if value < 1 {
        return Err(ToolError::invalid_input(format!(
            "'{field}' is 1-based and must be >= 1"
        )));
    }
    u32::try_from(value)
        .map_err(|_| ToolError::invalid_input(format!("'{field}' is out of range: {value}")))
}

/// 解析 `wait_ms`：缺省用配置里的 `poll_after_edit_ms`，并夹在
/// `[0, MAX_WAIT_MS]` 内防止模型传入超大值挂住回合。
fn resolve_wait(input: &Value, manager: &LspManager) -> Result<Duration, ToolError> {
    let Some(raw) = input.get("wait_ms") else {
        let configured = manager.default_wait();
        return Ok(if configured.is_zero() {
            Duration::from_millis(DEFAULT_WAIT_MS)
        } else {
            configured
        });
    };
    let ms = raw
        .as_u64()
        .ok_or_else(|| ToolError::invalid_input("'wait_ms' must be a non-negative integer"))?;
    Ok(Duration::from_millis(ms.min(MAX_WAIT_MS)))
}

/// 优先展示相对 workspace 的路径，落在 workspace 外时退回绝对路径。
fn display_path(path: &Path, workspace: &Path) -> String {
    path.strip_prefix(workspace)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// 递归统计符号总数（含嵌套子符号）。
fn count_symbols(symbols: &[SymbolOut]) -> usize {
    symbols.iter().map(|s| 1 + count_symbols(&s.children)).sum()
}

/// LSP `SymbolKind` 数字码到可读名称的映射（LSP 规范 3.17）。
fn symbol_kind_name(kind: u64) -> Option<&'static str> {
    Some(match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum_member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type_parameter",
        _ => return None,
    })
}

// === 单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use tempfile::TempDir;

    /// 构造一个 workspace 内含 `main.rs` 的最小 context，默认 `lsp_manager`
    /// 为 `None`。
    fn context_with_file() -> (TempDir, ToolContext, String) {
        let dir = TempDir::new().expect("create tempdir");
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() {}\n").expect("write file");
        let ctx = ToolContext::new(dir.path());
        (dir, ctx, "main.rs".to_string())
    }

    fn ctx_with_disabled_lsp(dir: &TempDir) -> ToolContext {
        let mut ctx = ToolContext::new(dir.path());
        ctx.lsp_manager = Some(Arc::new(LspManager::disabled()));
        ctx
    }

    // --- schema ---

    #[test]
    fn document_symbols_schema_shape() {
        let schema = LspDocumentSymbolsTool.input_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["file"]));
        assert_eq!(schema["properties"]["file"]["type"], "string");
        assert_eq!(schema["properties"]["wait_ms"]["type"], "number");
    }

    #[test]
    fn find_references_schema_shape() {
        let schema = LspFindReferencesTool.input_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["file", "line", "column"]));
        assert_eq!(
            schema["properties"]["include_declaration"]["type"],
            "boolean"
        );
    }

    #[test]
    fn goto_definition_schema_shape() {
        let schema = LspGotoDefinitionTool.input_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["file", "line", "column"]));
        // definition 不接受 include_declaration。
        assert!(schema["properties"].get("include_declaration").is_none());
    }

    #[test]
    fn tools_are_read_only_auto_and_parallel() {
        for (name, caps, approval, parallel, read_only) in [
            (
                LspDocumentSymbolsTool.name(),
                LspDocumentSymbolsTool.capabilities(),
                LspDocumentSymbolsTool.approval_requirement(),
                LspDocumentSymbolsTool.supports_parallel(),
                LspDocumentSymbolsTool.is_read_only(),
            ),
            (
                LspFindReferencesTool.name(),
                LspFindReferencesTool.capabilities(),
                LspFindReferencesTool.approval_requirement(),
                LspFindReferencesTool.supports_parallel(),
                LspFindReferencesTool.is_read_only(),
            ),
            (
                LspGotoDefinitionTool.name(),
                LspGotoDefinitionTool.capabilities(),
                LspGotoDefinitionTool.approval_requirement(),
                LspGotoDefinitionTool.supports_parallel(),
                LspGotoDefinitionTool.is_read_only(),
            ),
        ] {
            assert_eq!(caps, vec![ToolCapability::ReadOnly], "{name}");
            assert_eq!(approval, ApprovalRequirement::Auto, "{name}");
            assert!(parallel, "{name} should support parallel execution");
            assert!(read_only, "{name} should be read-only");
        }
    }

    #[test]
    fn tool_names_are_stable() {
        assert_eq!(LspDocumentSymbolsTool.name(), "lsp_document_symbols");
        assert_eq!(LspFindReferencesTool.name(), "lsp_find_references");
        assert_eq!(LspGotoDefinitionTool.name(), "lsp_goto_definition");
    }

    // --- lsp_manager 为 None ---

    #[tokio::test]
    async fn missing_manager_is_a_clear_error() {
        let (_dir, ctx, file) = context_with_file();
        let input = json!({ "file": file, "line": 1, "column": 4 });

        for err in [
            LspDocumentSymbolsTool
                .execute(input.clone(), &ctx)
                .await
                .unwrap_err(),
            LspFindReferencesTool
                .execute(input.clone(), &ctx)
                .await
                .unwrap_err(),
            LspGotoDefinitionTool
                .execute(input.clone(), &ctx)
                .await
                .unwrap_err(),
        ] {
            assert!(
                err.to_string().contains("LSP is not configured"),
                "unexpected error: {err}"
            );
        }
    }

    // --- 输入校验 ---

    #[tokio::test]
    async fn missing_file_field_is_rejected() {
        let dir = TempDir::new().expect("create tempdir");
        let ctx = ctx_with_disabled_lsp(&dir);
        let err = LspDocumentSymbolsTool
            .execute(json!({}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::MissingField { .. }), "{err}");
    }

    #[tokio::test]
    async fn nonexistent_file_is_rejected_without_panic() {
        let dir = TempDir::new().expect("create tempdir");
        let ctx = ctx_with_disabled_lsp(&dir);
        let err = LspDocumentSymbolsTool
            .execute(json!({ "file": "nope.rs" }), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[tokio::test]
    async fn directory_path_is_rejected() {
        let dir = TempDir::new().expect("create tempdir");
        std::fs::create_dir(dir.path().join("src")).expect("mkdir");
        let ctx = ctx_with_disabled_lsp(&dir);
        let err = LspDocumentSymbolsTool
            .execute(json!({ "file": "src" }), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Not a regular file"), "{err}");
    }

    #[tokio::test]
    async fn zero_line_is_rejected_as_one_based() {
        let dir = TempDir::new().expect("create tempdir");
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").expect("write");
        let ctx = ctx_with_disabled_lsp(&dir);
        let err = LspFindReferencesTool
            .execute(json!({ "file": "main.rs", "line": 0, "column": 1 }), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("1-based"), "{err}");
    }

    #[tokio::test]
    async fn missing_position_field_is_rejected() {
        let dir = TempDir::new().expect("create tempdir");
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").expect("write");
        let ctx = ctx_with_disabled_lsp(&dir);
        let err = LspGotoDefinitionTool
            .execute(json!({ "file": "main.rs", "line": 1 }), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::MissingField { field } if field == "column"));
    }

    // --- 禁用的 manager 走降级路径而非报错 ---

    #[tokio::test]
    async fn disabled_manager_returns_empty_results() {
        let dir = TempDir::new().expect("create tempdir");
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").expect("write");
        let ctx = ctx_with_disabled_lsp(&dir);

        let result = LspDocumentSymbolsTool
            .execute(json!({ "file": "main.rs" }), &ctx)
            .await
            .expect("disabled LSP should degrade, not error");
        let parsed: Value = serde_json::from_str(&result.content).expect("valid json");
        assert_eq!(parsed["symbol_count"], 0);
        assert_eq!(parsed["symbols"], json!([]));

        let result = LspGotoDefinitionTool
            .execute(json!({ "file": "main.rs", "line": 1, "column": 4 }), &ctx)
            .await
            .expect("disabled LSP should degrade");
        let parsed: Value = serde_json::from_str(&result.content).expect("valid json");
        assert_eq!(parsed["found"], false);
        assert_eq!(parsed["definition"], Value::Null);

        let result = LspFindReferencesTool
            .execute(json!({ "file": "main.rs", "line": 1, "column": 4 }), &ctx)
            .await
            .expect("disabled LSP should degrade");
        let parsed: Value = serde_json::from_str(&result.content).expect("valid json");
        assert_eq!(parsed["reference_count"], 0);
        // 未显式传入时默认包含声明。
        assert_eq!(parsed["include_declaration"], true);
    }

    // --- 纯函数 ---

    #[test]
    fn wait_is_clamped_to_max() {
        let manager = LspManager::disabled();
        let wait = resolve_wait(&json!({ "wait_ms": 10_000_000_u64 }), &manager).expect("clamped");
        assert_eq!(wait, Duration::from_millis(MAX_WAIT_MS));
    }

    #[test]
    fn wait_falls_back_to_manager_default() {
        let manager = LspManager::disabled();
        let wait = resolve_wait(&json!({}), &manager).expect("default");
        assert_eq!(wait, manager.default_wait());
    }

    #[test]
    fn non_numeric_wait_is_rejected() {
        let manager = LspManager::disabled();
        assert!(resolve_wait(&json!({ "wait_ms": "soon" }), &manager).is_err());
    }

    #[test]
    fn symbol_count_includes_nested_children() {
        let sym = LspSymbol {
            name: "Foo".to_string(),
            kind: 23,
            line: 1,
            column: 1,
            children: vec![LspSymbol {
                name: "bar".to_string(),
                kind: 6,
                line: 2,
                column: 5,
                children: vec![],
            }],
        };
        let out = vec![SymbolOut::from(&sym)];
        assert_eq!(count_symbols(&out), 2);
        assert_eq!(out[0].kind_name, Some("struct"));
        assert_eq!(out[0].children[0].kind_name, Some("method"));
    }

    #[test]
    fn unknown_symbol_kind_maps_to_none() {
        assert_eq!(symbol_kind_name(999), None);
    }

    #[test]
    fn display_path_prefers_workspace_relative() {
        let workspace = Path::new("/ws");
        assert_eq!(
            display_path(Path::new("/ws/src/main.rs"), workspace),
            "src/main.rs"
        );
        assert_eq!(
            display_path(Path::new("/elsewhere/main.rs"), workspace),
            "/elsewhere/main.rs"
        );
    }
}
