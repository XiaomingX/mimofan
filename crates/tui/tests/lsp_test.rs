//! `lsp` 模块单元测试（禁用/不支持语言降级、call hierarchy 纯逻辑）。
//!
//! 从 `crates/tui/src/lsp/mod.rs` 的内联 `#[cfg(test)] mod tests` 迁出；
//! 相关类型与 trait（`LspManager` / `LspConfig` / `LspTransport` /
//! `CallHierarchyTree` / `CallNode` / `Diagnostic` 等）均已 `pub` 暴露。

use std::collections::HashMap as Map;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use async_trait::async_trait;
use mimofan::lsp::diagnostics::Diagnostic;
use mimofan::lsp::{
    CallHierarchyTree, CallNode, LspConfig, LspLocation, LspManager, LspSymbol, LspTransport,
};
use serde_json::{Value, json};

/// 禁用状态下三个转发方法都应立即返回空结果，且不尝试拉起任何进程。
#[tokio::test]
async fn disabled_manager_returns_empty_for_symbol_queries() {
    let manager = LspManager::disabled();
    let file = Path::new("/tmp/does-not-matter.rs");
    let wait = Duration::from_millis(10);

    assert!(manager.document_symbols_for(file, wait).await.is_empty());
    assert!(
        manager
            .references_for(file, 1, 1, true, wait)
            .await
            .is_empty()
    );
    assert!(manager.definition_for(file, 1, 1, wait).await.is_none());
}

/// 非受支持语言（`Language::Other`）同样短路返回，不去 spawn 服务端。
#[tokio::test]
async fn unsupported_language_returns_empty() {
    let manager = LspManager::new(LspConfig::default(), PathBuf::from("/tmp"));
    let file = Path::new("/tmp/notes.unknown-ext");
    let wait = Duration::from_millis(10);

    assert!(manager.document_symbols_for(file, wait).await.is_empty());
    assert!(manager.definition_for(file, 1, 1, wait).await.is_none());
}

/// `default_wait` 应当反映配置里的 `poll_after_edit_ms`。
#[test]
fn default_wait_follows_config() {
    let manager = LspManager::new(
        LspConfig {
            poll_after_edit_ms: 1_234,
            ..LspConfig::default()
        },
        PathBuf::from("/tmp"),
    );
    assert_eq!(manager.default_wait(), Duration::from_millis(1_234));
}

// --- call hierarchy 纯逻辑（build / limit_depth） ---
//
// 这些测试用一个 in-process fake transport 脚本化 `prepareCallHierarchy`
// 与 `callHierarchy/*Calls`，从而确定地覆盖递归展开与深度裁剪逻辑，
// 不依赖任何外部 LSP 二进制（issue #827）。

/// Fake transport：按 `(item.name, direction)` 返回脚本化的调用边。
struct FakeHierarchyTransport {
    /// `prepareCallHierarchy` 返回的起始 item 数组。
    root: StdMutex<Vec<Value>>,
    /// 按节点名称映射其 outgoing / incoming 边。
    outgoing: StdMutex<Map<String, Vec<Value>>>,
    incoming: StdMutex<Map<String, Vec<Value>>>,
}

impl FakeHierarchyTransport {
    fn new(root: Vec<Value>) -> Self {
        Self {
            root: StdMutex::new(root),
            outgoing: StdMutex::new(Map::new()),
            incoming: StdMutex::new(Map::new()),
        }
    }
    /// 给某名称的节点挂一条 outgoing 边：`to` 为被调用符号。
    fn with_outgoing(self, name: &str, to: &str) -> Self {
        self.outgoing
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .push(json!({
                "to": { "name": to, "kind": 12, "uri": "file:///x.rs",
                        "range": { "start": { "line": 0, "character": 0 } },
                        "selectionRange": { "start": { "line": 0, "character": 0 } } },
                "toRanges": [ { "start": { "line": 4, "character": 1 } } ]
            }));
        self
    }
    /// 给某名称的节点挂一条 incoming 边：`from` 为调用方符号。
    fn with_incoming(self, name: &str, from: &str) -> Self {
        self.incoming
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .push(json!({
                "from": { "name": from, "kind": 12, "uri": "file:///x.rs",
                          "range": { "start": { "line": 0, "character": 0 } },
                          "selectionRange": { "start": { "line": 0, "character": 0 } } },
                "fromRanges": [ { "start": { "line": 9, "character": 3 } } ]
            }));
        self
    }
}

#[async_trait]
impl LspTransport for FakeHierarchyTransport {
    async fn diagnostics_for(
        &self,
        _path: &Path,
        _text: &str,
        _wait: Duration,
    ) -> anyhow::Result<Vec<Diagnostic>> {
        Ok(Vec::new())
    }
    async fn close_file(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn request(
        &self,
        _method: &str,
        _params: Value,
        _wait: Duration,
    ) -> anyhow::Result<Value> {
        Ok(Value::Null)
    }
    async fn document_symbols(&self, _path: &Path, _wait: Duration) -> Vec<LspSymbol> {
        Vec::new()
    }
    async fn references(
        &self,
        _path: &Path,
        _line: u32,
        _column: u32,
        _include_declaration: bool,
        _wait: Duration,
    ) -> Vec<LspLocation> {
        Vec::new()
    }
    async fn definition(
        &self,
        _path: &Path,
        _line: u32,
        _column: u32,
        _wait: Duration,
    ) -> Option<LspLocation> {
        None
    }
    async fn prepare_call_hierarchy(
        &self,
        _path: &Path,
        _line: u32,
        _column: u32,
        _wait: Duration,
    ) -> Vec<Value> {
        self.root.lock().unwrap().clone()
    }
    async fn call_hierarchy_calls(
        &self,
        item: Value,
        direction: &str,
        _wait: Duration,
    ) -> Vec<Value> {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        match direction {
            "incoming" => self
                .incoming
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .unwrap_or_default(),
            _ => self
                .outgoing
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

#[test]
fn empty_tree_when_no_root_item() {
    let t = CallHierarchyTree::empty("both", 2);
    assert!(t.is_empty());
    assert_eq!(t.direction, "both");
    assert_eq!(t.max_depth, 2);
}

#[test]
fn limit_depth_drops_deep_subtrees() {
    // 构造一棵三层深的树，裁剪到 1 层后应只保留第一层 children。
    let deep = CallNode {
        name: "a".into(),
        kind: 12,
        path: PathBuf::from("/x.rs"),
        line: 1,
        column: 1,
        call_line: 0,
        children: vec![CallNode {
            name: "b".into(),
            kind: 12,
            path: PathBuf::from("/x.rs"),
            line: 2,
            column: 1,
            call_line: 0,
            children: vec![CallNode {
                name: "c".into(),
                kind: 12,
                path: PathBuf::from("/x.rs"),
                line: 3,
                column: 1,
                call_line: 0,
                children: vec![],
            }],
        }],
    };
    let clipped = deep.limit_depth(1);
    assert_eq!(clipped.children.len(), 1);
    assert_eq!(clipped.children[0].name, "b");
    // 第 2 层被剪掉。
    assert!(clipped.children[0].children.is_empty());
}

#[tokio::test]
async fn build_expands_outgoing_to_depth() {
    let fake = FakeHierarchyTransport::new(vec![json!({
        "name": "main", "kind": 12, "uri": "file:///x.rs",
        "range": { "start": { "line": 0, "character": 0 } },
        "selectionRange": { "start": { "line": 0, "character": 0 } }
    })])
    .with_outgoing("main", "helper")
    .with_outgoing("helper", "leaf");

    let root = fake
        .prepare_call_hierarchy(Path::new("/x.rs"), 1, 1, Duration::from_millis(10))
        .await
        .into_iter()
        .next()
        .unwrap();

    let tree =
        CallHierarchyTree::build(&fake, root, "outgoing", 2, Duration::from_millis(10)).await;

    assert_eq!(tree.root_name, "main");
    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].name, "helper");
    // helper 还有 outgoing 边 leaf，深度 2 足够把它展开出来。
    assert_eq!(tree.children[0].children.len(), 1);
    assert_eq!(tree.children[0].children[0].name, "leaf");
    // leaf 没有进一步边，其 children 为空。
    assert!(tree.children[0].children[0].children.is_empty());
}

#[tokio::test]
async fn build_respects_max_depth() {
    let fake = FakeHierarchyTransport::new(vec![json!({
        "name": "main", "kind": 12, "uri": "file:///x.rs",
        "range": { "start": { "line": 0, "character": 0 } },
        "selectionRange": { "start": { "line": 0, "character": 0 } }
    })])
    .with_outgoing("main", "helper")
    .with_outgoing("helper", "leaf");

    let root = fake
        .prepare_call_hierarchy(Path::new("/x.rs"), 1, 1, Duration::from_millis(10))
        .await
        .into_iter()
        .next()
        .unwrap();

    // 深度 1：只能展开到 helper，leaf 看不到。
    let tree =
        CallHierarchyTree::build(&fake, root, "outgoing", 1, Duration::from_millis(10)).await;
    assert_eq!(tree.children[0].name, "helper");
    assert!(tree.children[0].children.is_empty());
}

#[tokio::test]
async fn build_expands_incoming_edges() {
    let fake = FakeHierarchyTransport::new(vec![json!({
        "name": "target", "kind": 12, "uri": "file:///x.rs",
        "range": { "start": { "line": 0, "character": 0 } },
        "selectionRange": { "start": { "line": 0, "character": 0 } }
    })])
    .with_incoming("target", "caller");

    let root = fake
        .prepare_call_hierarchy(Path::new("/x.rs"), 1, 1, Duration::from_millis(10))
        .await
        .into_iter()
        .next()
        .unwrap();

    let tree =
        CallHierarchyTree::build(&fake, root, "incoming", 2, Duration::from_millis(10)).await;
    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].name, "caller");
    // 入边用 fromRanges 的第一个作为 call_line（1-based）。
    assert_eq!(tree.children[0].call_line, 10);
}

#[test]
fn limit_depth_on_tree_keeps_root_level() {
    let tree = CallHierarchyTree {
        root_name: "main".into(),
        direction: "outgoing".into(),
        max_depth: 5,
        children: vec![CallNode {
            name: "a".into(),
            kind: 12,
            path: PathBuf::from("/x.rs"),
            line: 1,
            column: 1,
            call_line: 0,
            children: vec![CallNode {
                name: "b".into(),
                kind: 12,
                path: PathBuf::from("/x.rs"),
                line: 2,
                column: 1,
                call_line: 0,
                children: vec![],
            }],
        }],
    };
    let clipped = tree.limit_depth(0);
    assert!(clipped.children.is_empty());
    assert_eq!(clipped.root_name, "main");
}
