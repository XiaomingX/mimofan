//! LSP integration: post-edit diagnostics injection (#136).
//!
//! After the agent performs a successful file edit (`edit_file`,
//! `apply_patch`, or `write_file`) the engine asks the [`LspManager`] for
//! diagnostics on that file. The manager spawns the appropriate LSP server
//! lazily on first use, sends `didOpen`/`didChange`, waits up to a bounded
//! timeout for `publishDiagnostics`, normalizes the result, and returns it
//! to the engine.
//!
//! Failure modes are non-blocking by design: a missing LSP binary, a
//! crashed server, or a timeout all degrade to "no diagnostics this turn"
//! rather than stalling the agent. We log a one-time warning per language
//! when the binary is missing.
//!
//! # Wiring
//!
//! ```text
//! Engine  ── after successful edit ──▶  LspManager.diagnostics_for(path, seq)
//!                                              │
//!                                              ▼
//!                                       per-language LspClient
//!                                              │
//!                                              ▼
//!                                      LspTransport (stdio)
//! ```
//!
//! # Configuration
//!
//! The `[lsp]` table in `~/.mimofan/config.toml` controls behavior:
//! `enabled`, `poll_after_edit_ms`, `max_diagnostics_per_file`,
//! `include_warnings`, and an optional `servers` override. See
//! [`LspConfig`] for defaults and `config.example.toml` for documentation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::timeout;

pub mod client;
pub mod diagnostics;
pub mod registry;

pub use client::{LspLocation, LspSymbol, LspTransport, StdioLspTransport};
pub use diagnostics::{Diagnostic, DiagnosticBlock, Severity, render_blocks};
pub use registry::Language;

/// `[lsp]` config schema. Mirrors the TOML keys documented in
/// `config.example.toml`. Unknown keys are ignored.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LspConfig {
    /// Master switch. When `false`, the manager skips every operation and
    /// returns an empty diagnostics list.
    pub enabled: bool,
    /// Maximum time in milliseconds to wait for the LSP server to publish
    /// diagnostics after a `didOpen`/`didChange`. Default 5000 ms.
    pub poll_after_edit_ms: u64,
    /// Maximum diagnostics to keep per file. Excess items are dropped after
    /// sorting by severity. Default 20.
    pub max_diagnostics_per_file: usize,
    /// When `true`, warnings (severity 2) are kept in the output. When
    /// `false` (default), only errors (severity 1) are surfaced.
    pub include_warnings: bool,
    /// Optional override for the `Language -> (cmd, args)` table. Keys use
    /// [`Language::as_key`] (e.g. `"rust"`).
    pub servers: HashMap<String, Vec<String>>,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_after_edit_ms: 5_000,
            max_diagnostics_per_file: 20,
            include_warnings: false,
            servers: HashMap::new(),
        }
    }
}

impl LspConfig {
    /// Resolve `(command, args)` for `lang`. User-supplied overrides take
    /// precedence over the built-in registry.
    fn resolve_command(&self, lang: Language) -> Option<(String, Vec<String>)> {
        if let Some(parts) = self.servers.get(lang.as_key())
            && let Some((first, rest)) = parts.split_first()
        {
            return Some((first.clone(), rest.to_vec()));
        }
        let (cmd, args) = registry::server_for(lang)?;
        Some((
            cmd.to_string(),
            args.iter().map(|a| (*a).to_string()).collect(),
        ))
    }
}

/// The LspManager holds a lazily populated map of `Language -> Transport`.
/// One transport is reused across files of the same language for the
/// session's lifetime.
pub struct LspManager {
    config: LspConfig,
    workspace: PathBuf,
    /// Per-language transports. Wrapped in `Arc` so we can release the outer
    /// lock before driving I/O on a single transport.
    transports: AsyncMutex<HashMap<Language, Arc<dyn LspTransport>>>,
    /// Per-language "we already warned the user that the binary is missing"
    /// guard so we do not spam the audit log on every edit.
    missing_warned: AsyncMutex<HashSet<Language>>,
    /// Test seam: when set, `diagnostics_for` uses these instead of spawning
    /// real LSP processes. Keyed by language.
    test_transports: AsyncMutex<HashMap<Language, Arc<dyn LspTransport>>>,
}

impl LspManager {
    /// Build a new manager. Does not spawn any LSP servers — that is lazy.
    #[must_use]
    pub fn new(config: LspConfig, workspace: PathBuf) -> Self {
        Self {
            config,
            workspace,
            transports: AsyncMutex::new(HashMap::new()),
            missing_warned: AsyncMutex::new(HashSet::new()),
            test_transports: AsyncMutex::new(HashMap::new()),
        }
    }

    /// Read-only access to the resolved config. Used by the engine to skip
    /// the post-edit hook entirely when `enabled = false`.
    #[must_use]
    pub fn config(&self) -> &LspConfig {
        &self.config
    }

    /// Poll the LSP server for diagnostics on `file`. Returns the rendered
    /// [`DiagnosticBlock`] (already truncated to the configured per-file
    /// max) or `None` when the manager is disabled / has no server / the
    /// poll times out.
    ///
    /// The `_edit_seq` argument is currently a no-op; it exists in the
    /// signature so the engine can correlate diagnostics back to a specific
    /// edit when we add request batching in v0.7.x.
    pub async fn diagnostics_for(&self, file: &Path, _edit_seq: u64) -> Option<DiagnosticBlock> {
        if !self.config.enabled {
            return None;
        }
        let lang = registry::detect_language(file);
        if lang == Language::Other {
            return None;
        }

        let text = match tokio::fs::read_to_string(file).await {
            Ok(text) => text,
            Err(err) => {
                tracing::debug!(?err, file = %file.display(), "lsp: read file failed");
                return None;
            }
        };

        let transport = match self.transport_for(lang).await {
            Some(t) => t,
            None => return None,
        };

        let wait = Duration::from_millis(self.config.poll_after_edit_ms);
        let inner_wait = wait;
        let raw = match timeout(wait, transport.diagnostics_for(file, &text, inner_wait)).await {
            Ok(Ok(items)) => items,
            Ok(Err(err)) => {
                tracing::debug!(?err, file = %file.display(), "lsp: diagnostics call failed");
                return None;
            }
            Err(_) => {
                tracing::debug!(file = %file.display(), "lsp: diagnostics timed out");
                return None;
            }
        };

        // Filter, sort, and truncate.
        let include_warnings = self.config.include_warnings;
        let mut items: Vec<Diagnostic> = raw
            .into_iter()
            .filter(|d| match d.severity {
                Severity::Error => true,
                Severity::Warning => include_warnings,
                _ => false,
            })
            .collect();
        items.sort_by_key(|d| match d.severity {
            Severity::Error => 0u8,
            Severity::Warning => 1u8,
            Severity::Information => 2u8,
            Severity::Hint => 3u8,
        });
        let mut block = DiagnosticBlock {
            file: relative_to_workspace(&self.workspace, file),
            items,
        };
        block.truncate(self.config.max_diagnostics_per_file);
        if block.items.is_empty() {
            None
        } else {
            Some(block)
        }
    }

    /// Close `file` in the corresponding LSP server transport and send `didClose`.
    pub async fn close_file(&self, file: &Path) {
        if !self.config.enabled {
            return;
        }
        let lang = registry::detect_language(file);
        if lang == Language::Other {
            return;
        }
        if let Some(transport) = self.transports.lock().await.get(&lang) {
            let _ = transport.close_file(file).await;
        }
    }

    /// Resolve (and lazily spawn) the transport for `lang`. Tests can
    /// short-circuit this via `install_test_transport` (cfg-test only).
    async fn transport_for(&self, lang: Language) -> Option<Arc<dyn LspTransport>> {
        if let Some(t) = self.test_transports.lock().await.get(&lang) {
            return Some(t.clone());
        }

        if let Some(t) = self.transports.lock().await.get(&lang) {
            return Some(t.clone());
        }

        let (cmd, args) = self.config.resolve_command(lang)?;
        match StdioLspTransport::spawn(&cmd, &args, lang, self.workspace.clone()).await {
            Ok(transport) => {
                let arc: Arc<dyn LspTransport> = Arc::new(transport);
                self.transports.lock().await.insert(lang, arc.clone());
                Some(arc)
            }
            Err(err) => {
                self.warn_missing_once(lang, &cmd, &err).await;
                None
            }
        }
    }

    /// 打开 `file` 并返回其对应语言的 transport。
    ///
    /// 符号类查询（documentSymbol/references/definition）都要求服务端已经
    /// 通过 `didOpen` 见过该文件，否则会返回空结果或报 "unknown document"。
    /// 这里复用 `transport.diagnostics_for`：它的主要副作用正是发送
    /// `didOpen`/`didChange`，顺带在 `wait` 内收集诊断（诊断结果我们丢弃）。
    ///
    /// 返回 `None` 表示：LSP 被禁用、文件语言不受支持、文件读不到，或
    /// 服务端不可用。调用方据此降级为空结果，绝不 panic。
    async fn open_and_get_transport(
        &self,
        file: &Path,
        wait: Duration,
    ) -> Option<Arc<dyn LspTransport>> {
        if !self.config.enabled {
            return None;
        }
        let lang = registry::detect_language(file);
        if lang == Language::Other {
            return None;
        }

        let text = match tokio::fs::read_to_string(file).await {
            Ok(text) => text,
            Err(err) => {
                tracing::debug!(?err, file = %file.display(), "lsp: read file failed");
                return None;
            }
        };

        let transport = self.transport_for(lang).await?;

        // 先同步文件内容；超时或失败都不阻断后续查询——服务端可能已经
        // 通过 workspace 扫描索引到了该文件。
        match timeout(wait, transport.diagnostics_for(file, &text, wait)).await {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                tracing::debug!(?err, file = %file.display(), "lsp: didOpen sync failed");
            }
            Err(_) => {
                tracing::debug!(file = %file.display(), "lsp: didOpen sync timed out");
            }
        }

        Some(transport)
    }

    /// 默认等待时长，取自 `[lsp] poll_after_edit_ms`。
    #[must_use]
    pub fn default_wait(&self) -> Duration {
        Duration::from_millis(self.config.poll_after_edit_ms)
    }

    /// 列出 `file` 中定义的符号（`textDocument/documentSymbol`）。
    ///
    /// 尽力而为：LSP 被禁用、语言不支持、服务端缺失或查询失败时返回空列表。
    pub async fn document_symbols_for(&self, file: &Path, wait: Duration) -> Vec<LspSymbol> {
        let Some(transport) = self.open_and_get_transport(file, wait).await else {
            return Vec::new();
        };
        transport.document_symbols(file, wait).await
    }

    /// 查找 `file` 中 `(line, column)` 处符号的所有引用
    /// （`textDocument/references`）。行列均为 1-based。
    ///
    /// 尽力而为：不可用时返回空列表。
    pub async fn references_for(
        &self,
        file: &Path,
        line: u32,
        column: u32,
        include_declaration: bool,
        wait: Duration,
    ) -> Vec<LspLocation> {
        let Some(transport) = self.open_and_get_transport(file, wait).await else {
            return Vec::new();
        };
        transport
            .references(file, line, column, include_declaration, wait)
            .await
    }

    /// 解析 `file` 中 `(line, column)` 处符号的定义位置
    /// （`textDocument/definition`）。行列均为 1-based。
    ///
    /// 尽力而为：不可用或无定义时返回 `None`。
    pub async fn definition_for(
        &self,
        file: &Path,
        line: u32,
        column: u32,
        wait: Duration,
    ) -> Option<LspLocation> {
        let transport = self.open_and_get_transport(file, wait).await?;
        transport.definition(file, line, column, wait).await
    }

    /// 计算 `file` 中 `(line, column)` 处符号的调用层级（call hierarchy）。
    ///
    /// 通过 `textDocument/prepareCallHierarchy` 取起点 item，再按
    /// `direction` 递归展开 `callHierarchy/incomingCalls` /
    /// `callHierarchy/outgoingCalls`，深度上限为 `max_depth`。返回的
    /// [`CallHierarchyTree`] 是嵌套的调用节点树，每个叶子/分支都带
    /// `name` / `kind` / 位置，便于模型做影响分析（谁调用它 / 它调用谁）。
    ///
    /// 尽力而为：LSP 被禁用、语言不支持、服务端缺失、不支持该方法或查询失败
    /// 时，返回空的 `CallHierarchyTree`（根节点无 children），绝不 panic。
    ///
    /// `direction` 取值 `"incoming"` / `"outgoing"` / `"both"`。
    pub async fn call_hierarchy_for(
        &self,
        file: &Path,
        line: u32,
        column: u32,
        direction: &str,
        max_depth: u32,
        wait: Duration,
    ) -> CallHierarchyTree {
        let transport = self.open_and_get_transport(file, wait).await;
        let Some(transport) = transport else {
            return CallHierarchyTree::empty(direction, max_depth);
        };

        let items = transport.prepare_call_hierarchy(file, line, column, wait).await;
        let Some(root_item) = items.into_iter().next() else {
            return CallHierarchyTree::empty(direction, max_depth);
        };

        CallHierarchyTree::build(
            &*transport,
            root_item,
            direction,
            max_depth,
            wait,
        )
        .await
    }

    async fn warn_missing_once(&self, lang: Language, cmd: &str, err: &anyhow::Error) {
        let mut warned = self.missing_warned.lock().await;
        if warned.insert(lang) {
            tracing::warn!(
                language = %lang.as_key(),
                command = %cmd,
                error = %err,
                "lsp: server unavailable; diagnostics disabled for this language"
            );
        }
    }
}

/// Render `path` relative to the workspace when possible. Falls back to
/// `path.file_name()` (per the issue's hard rule about not using
/// `display().to_string()` on the bare path) when relativization fails.
fn relative_to_workspace(workspace: &Path, path: &Path) -> PathBuf {
    if let Ok(rel) = path.strip_prefix(workspace) {
        return rel.to_path_buf();
    }
    PathBuf::from(
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("unknown")),
    )
}

/// Used for tests / no-op runs. Builds an empty manager that always returns
/// `None`. Needed because the engine constructs an `LspManager` even when
/// the user has disabled LSP, so the field is always present.
impl LspManager {
    #[must_use]
    pub fn disabled() -> Self {
        Self::new(
            LspConfig {
                enabled: false,
                ..LspConfig::default()
            },
            PathBuf::new(),
        )
    }
}

// === Call hierarchy ===

/// 一次 `callHierarchy/*Calls` 递归展开后的结果树。
///
/// 根节点表示被查询的符号本身；`children` 为按 `direction` 展开得到的
/// 入/出调用边。整棵树由 [`LspManager::call_hierarchy_for`] 构造，是纯粹的
/// 数据产物——其 `build`/`limit_depth` 逻辑被单测覆盖（见本文件测试模块）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHierarchyTree {
    /// 根符号名称。
    pub root_name: String,
    /// 查询方向：`"incoming"` / `"outgoing"` / `"both"`。
    pub direction: String,
    /// 递归深度上限。
    pub max_depth: u32,
    /// 嵌套调用节点。
    pub children: Vec<CallNode>,
}

impl CallHierarchyTree {
    /// 空树——用于降级路径（LSP 不可用、符号无 call-hierarchy item）。
    #[must_use]
    pub fn empty(direction: &str, max_depth: u32) -> Self {
        Self {
            root_name: String::new(),
            direction: direction.to_string(),
            max_depth,
            children: Vec::new(),
        }
    }

    /// 判断整棵树是否为空（无任何调用边）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.root_name.is_empty() && self.children.is_empty()
    }

    /// 从一个已 prepare 好的根 item 递归构造调用树。
    ///
    /// 该方法是 `LspManager::call_hierarchy_for` 的实际实现：它把递归展开
    /// 逻辑封装在这里（与 transport 解耦），便于对纯的 `expand_node` /
    /// `limit_depth` 做确定性单测。`direction` 缺失或非法时退化为 `"both"`。
    pub async fn build(
        transport: &(dyn LspTransport),
        root_item: Value,
        direction: &str,
        max_depth: u32,
        wait: Duration,
    ) -> Self {
        let dir = match direction {
            "incoming" | "outgoing" | "both" => direction.to_string(),
            _ => "both".to_string(),
        };
        let root_name = root_item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut tree = CallHierarchyTree {
            root_name,
            direction: dir.clone(),
            max_depth,
            children: Vec::new(),
        };

        if max_depth == 0 {
            return tree;
        }

        // 展开根节点的入/出边。`both` 时两条都展开。
        let mut root_nodes: Vec<CallNode> = Vec::new();
        if dir == "incoming" || dir == "both" {
            let edges = transport.call_hierarchy_calls(root_item.clone(), "incoming", wait).await;
            for edge in edges {
                if let Some(node) = CallNode::from_incoming(&edge) {
                    root_nodes.push(node);
                }
            }
        }
        if dir == "outgoing" || dir == "both" {
            let edges = transport.call_hierarchy_calls(root_item.clone(), "outgoing", wait).await;
            for edge in edges {
                if let Some(node) = CallNode::from_outgoing(&edge) {
                    root_nodes.push(node);
                }
            }
        }

        // 逐节点递归到剩余深度。
        let remaining = max_depth.saturating_sub(1);
        for node in &mut root_nodes {
            node.expand(transport, &dir, remaining, wait).await;
        }
        tree.children = root_nodes;
        tree
    }

    /// 纯函数：把树裁剪到 `max_depth` 层。超过深度的子树被丢弃（根算第 0 层，
    /// `children` 为第 1 层）。返回裁剪后的新树，不修改原树。
    #[must_use]
    pub fn limit_depth(&self, max_depth: u32) -> CallHierarchyTree {
        let children = if max_depth == 0 {
            Vec::new()
        } else {
            self.children
                .iter()
                .map(|c| c.limit_depth(max_depth.saturating_sub(1)))
                .collect()
        };
        CallHierarchyTree {
            root_name: self.root_name.clone(),
            direction: self.direction.clone(),
            max_depth,
            children,
        }
    }
}

/// 调用树中的一个节点：代表一次 `callHierarchy/*Calls` 边所指向的符号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallNode {
    /// 被调用/调用方符号名称。
    pub name: String,
    /// LSP `SymbolKind` 数字码。
    pub kind: u64,
    /// 文件绝对路径。
    pub path: PathBuf,
    /// 1-based 行号。
    pub line: u32,
    /// 1-based 列号。
    pub column: u32,
    /// 与父节点的连接位置（edge 的 `fromRanges`/`toRanges` 第一个），1-based，
    /// 用于溯源调用发生点；缺失时为 0。
    pub call_line: u32,
    /// 嵌套调用节点。
    pub children: Vec<CallNode>,
}

impl CallNode {
    /// 从一条 `callHierarchy/incomingCalls` 边解析节点。`from` 是被查询符号
    /// 的调用方，`to` 是被查询符号本身。这里要的是调用方（`from`）。
    fn from_incoming(edge: &Value) -> Option<Self> {
        let item = edge.get("from")?;
        Self::from_item(item, edge.get("fromRanges"))
    }

    /// 从一条 `callHierarchy/outgoingCalls` 边解析节点。`to` 是被调用符号。
    fn from_outgoing(edge: &Value) -> Option<Self> {
        let item = edge.get("to")?;
        Self::from_item(item, edge.get("toRanges"))
    }

    fn from_item(item: &Value, ranges: Option<&Value>) -> Option<Self> {
        let name = item.get("name")?.as_str()?.to_string();
        let kind = item.get("kind").and_then(Value::as_u64).unwrap_or(0);
        let uri = item.get("uri")?.as_str()?;
        let path = path_from_uri_owned(uri)?;
        let range = item.get("range")?;
        let start = range.get("start")?;
        let line = start.get("line")?.as_u64()? as u32 + 1;
        let column = start.get("character")?.as_u64()? as u32 + 1;
        // 连接位置取 edge 第一个 range 的起始行（1-based），缺失时归零。
        let call_line = ranges
            .and_then(|r| r.as_array())
            .and_then(|arr| arr.first())
            .and_then(|r| r.get("start"))
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .map(|l| l as u32 + 1)
            .unwrap_or(0);
        Some(Self {
            name,
            kind,
            path,
            line,
            column,
            call_line,
            children: Vec::new(),
        })
    }

    /// 递归展开该节点的入/出调用边，深度上限 `remaining`。`direction` 为
    /// `"both"` 时同时展开两条方向；否则只展开指定方向。
    async fn expand(&mut self, transport: &(dyn LspTransport), direction: &str, remaining: u32, wait: Duration) {
        if remaining == 0 {
            return;
        }
        // 把当前节点还原成 prepareCallHierarchy 形态的 item（取回 uri/range/selectionRange）。
        let item = json!({
            "name": self.name,
            "kind": self.kind,
            "uri": uri_from_path_string(&self.path),
            "range": {
                "start": { "line": self.line.saturating_sub(1), "character": self.column.saturating_sub(1) },
                "end": { "line": self.line.saturating_sub(1), "character": self.column.saturating_sub(1) }
            },
            "selectionRange": {
                "start": { "line": self.line.saturating_sub(1), "character": self.column.saturating_sub(1) },
                "end": { "line": self.line.saturating_sub(1), "character": self.column.saturating_sub(1) }
            }
        });

        let mut kids: Vec<CallNode> = Vec::new();
        if direction == "incoming" || direction == "both" {
            let edges = transport.call_hierarchy_calls(item.clone(), "incoming", wait).await;
            for edge in edges {
                if let Some(mut node) = CallNode::from_incoming(&edge) {
                    // 递归展开需要 `Box::pin` 引入间接层，避免无限大小的 future。
                    Box::pin(node.expand(transport, direction, remaining.saturating_sub(1), wait)).await;
                    kids.push(node);
                }
            }
        }
        if direction == "outgoing" || direction == "both" {
            let edges = transport.call_hierarchy_calls(item.clone(), "outgoing", wait).await;
            for edge in edges {
                if let Some(mut node) = CallNode::from_outgoing(&edge) {
                    Box::pin(node.expand(transport, direction, remaining.saturating_sub(1), wait)).await;
                    kids.push(node);
                }
            }
        }
        self.children = kids;
    }

    /// 纯函数：裁剪当前子树到 `max_depth` 层（本节点算第 0 层，children 为第 1 层）。
    #[must_use]
    pub fn limit_depth(&self, max_depth: u32) -> CallNode {
        let children = if max_depth == 0 {
            Vec::new()
        } else {
            self.children
                .iter()
                .map(|c| c.limit_depth(max_depth.saturating_sub(1)))
                .collect()
        };
        CallNode {
            name: self.name.clone(),
            kind: self.kind,
            path: self.path.clone(),
            line: self.line,
            column: self.column,
            call_line: self.call_line,
            children,
        }
    }
}

/// `file://` URI 解码为 `PathBuf` 的 owned 版本（供 `CallNode::from_item` 使用）。
fn path_from_uri_owned(uri: &str) -> Option<PathBuf> {
    let stripped = uri.strip_prefix("file://")?;
    Some(PathBuf::from(stripped))
}

/// 路径转 `file://` URI 的 string 版本（供 `CallNode::expand` 还原 item 时用）。
fn uri_from_path_string(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy();
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{}", s.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

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

    use std::collections::HashMap as Map;
    use std::sync::Mutex as StdMutex;

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
        fn with_outgoing(mut self, name: &str, to: &str) -> Self {
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
        fn with_incoming(mut self, name: &str, from: &str) -> Self {
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
        ) -> anyhow::Result<Vec<crate::lsp::diagnostics::Diagnostic>> {
            Ok(Vec::new())
        }
        async fn close_file(&self, _path: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn request(&self, _method: &str, _params: Value, _wait: Duration) -> anyhow::Result<Value> {
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

        let tree = CallHierarchyTree::build(
            &fake,
            root,
            "outgoing",
            2,
            Duration::from_millis(10),
        )
        .await;

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
        let tree = CallHierarchyTree::build(
            &fake,
            root,
            "outgoing",
            1,
            Duration::from_millis(10),
        )
        .await;
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

        let tree = CallHierarchyTree::build(
            &fake,
            root,
            "incoming",
            2,
            Duration::from_millis(10),
        )
        .await;
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
}
