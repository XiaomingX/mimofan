//! `lsp_symbols` 工具集单元测试（schema / 校验 / 降级 / 纯函数）。
//!
//! 从 `crates/tui/src/tools/lsp_symbols.rs` 的内联 `#[cfg(test)] mod tests` 迁出；
//! 相关私有符号（`SymbolOut` / `MAX_CALL_DEPTH` / `count_nodes` / `node_to_json` /
//! `resolve_wait` / `display_path` / `count_symbols` / `symbol_kind_name`）已 `pub` 暴露。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use mimofan::lsp::{CallHierarchyTree, CallNode, LspManager, LspSymbol};
use mimofan::tools::lsp_symbols::{
    LspCallHierarchyTool, LspDocumentSymbolsTool, LspFindReferencesTool, LspGotoDefinitionTool,
    MAX_CALL_DEPTH, MAX_WAIT_MS, SymbolOut, count_nodes, count_symbols, display_path,
    node_to_json, resolve_wait, symbol_kind_name,
};
use mimofan::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolSpec,
};
use serde_json::{Value, json};
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
    assert_eq!(LspCallHierarchyTool.name(), "lsp_call_hierarchy");
}

#[test]
fn call_hierarchy_schema_shape() {
    let schema = LspCallHierarchyTool.input_schema();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], json!(["file", "line", "character"]));
    assert_eq!(schema["properties"]["file"]["type"], "string");
    assert_eq!(
        schema["properties"]["direction"]["enum"],
        json!(["incoming", "outgoing", "both"])
    );
    assert_eq!(schema["properties"]["max_depth"]["maximum"], MAX_CALL_DEPTH);
}

#[test]
fn call_hierarchy_is_read_only_auto_and_parallel() {
    let caps = LspCallHierarchyTool.capabilities();
    let approval = LspCallHierarchyTool.approval_requirement();
    let parallel = LspCallHierarchyTool.supports_parallel();
    assert_eq!(caps, vec![ToolCapability::ReadOnly]);
    assert_eq!(approval, ApprovalRequirement::Auto);
    assert!(parallel);
    assert!(LspCallHierarchyTool.is_read_only());
}

#[test]
fn count_nodes_includes_nested() {
    let tree = CallHierarchyTree {
        root_name: "main".to_string(),
        direction: "both".to_string(),
        max_depth: 2,
        children: vec![CallNode {
            name: "a".to_string(),
            kind: 12,
            path: Path::new("/x.rs").to_path_buf(),
            line: 1,
            column: 1,
            call_line: 0,
            children: vec![CallNode {
                name: "b".to_string(),
                kind: 12,
                path: Path::new("/x.rs").to_path_buf(),
                line: 2,
                column: 1,
                call_line: 0,
                children: vec![],
            }],
        }],
    };
    assert_eq!(count_nodes(&tree.children), 2);
    let json = node_to_json(&tree.children[0]);
    assert_eq!(json["name"], "a");
    assert_eq!(json["kind_name"], "function");
    assert_eq!(json["children"][0]["name"], "b");
    assert_eq!(json["path"], "/x.rs");
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
