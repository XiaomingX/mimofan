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

#[cfg(test)]
mod tests {
    use super::*;

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
}
