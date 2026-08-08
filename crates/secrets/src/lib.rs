//! Secret storage for mimofan API keys.
//!
//! Provides a small abstraction (`KeyringStore`) plus a default
//! file-based implementation (`FileKeyringStore`), an opt-in OS keyring
//! implementation (`DefaultKeyringStore`), and an in-memory store for tests
//! (`InMemoryKeyringStore`).
//!
//! Higher-level lookup through [`Secrets::resolve`] checks the secret store first
//! and falls back to environment variables. Config-file precedence lives in the
//! config crate so user-facing commands can keep `config -> secret store -> env`
//! explicit at the call site.
#![deny(missing_docs)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default OS keychain service name. Kept as `deepseek` for compatibility
/// with credentials saved before the mimofan rename. macOS users can verify
/// entries with `security find-generic-password -s deepseek -a <provider>`.
pub const DEFAULT_SERVICE: &str = "deepseek";
/// Select the secret storage backend. Supported values are `file` (default)
/// and `system`/`keyring` for the OS credential store.
pub const SECRET_BACKEND_ENV: &str = "MIMOFAN_SECRET_BACKEND";
/// Human-readable label for the file-based secret backend.
pub const FILE_BACKEND_LABEL: &str = "file-based (~/.mimofan/secrets/)";

/// Errors that may arise from a [`KeyringStore`] backend.
#[derive(Debug, Error)]
pub enum SecretsError {
    /// Underlying OS keyring backend reported an error.
    #[error("keyring backend error: {0}")]
    Keyring(String),
    /// File-backed fallback I/O error.
    #[error("file-backed secret store I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// File-backed fallback JSON (de)serialisation error.
    #[error("file-backed secret store JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Caught when a stored secret on disk has unsafe permissions.
    #[error("file-backed secret store at {path} has insecure permissions {mode:o} (expected 0600)")]
    InsecurePermissions {
        /// Absolute path to the secrets file.
        path: PathBuf,
        /// Observed unix permission mode.
        mode: u32,
    },
}

/// Abstract secret store trait.
///
/// Concrete implementations may use the OS keyring ([`DefaultKeyringStore`]),
/// a JSON file under `~/.mimofan/secrets/` ([`FileKeyringStore`]), or an
/// in-memory map for tests ([`InMemoryKeyringStore`]).
///
/// All implementations must be [`Send`] + [`Sync`] so they can be shared
/// across threads via [`Arc`].
pub trait KeyringStore: Send + Sync {
    /// Read a secret by key.
    ///
    /// Returns `Ok(None)` if no entry exists for the given key. Returns
    /// `Err` only on backend failures (I/O errors, keyring access issues).
    fn get(&self, key: &str) -> Result<Option<String>, SecretsError>;

    /// Write a secret, replacing any existing value for the same key.
    ///
    /// Creates the backing store (e.g. the JSON file) on first write if
    /// it does not yet exist.
    fn set(&self, key: &str, value: &str) -> Result<(), SecretsError>;

    /// Remove a secret by key.
    ///
    /// Implementations should succeed (no-op) if the entry is already absent
    /// rather than returning an error.
    fn delete(&self, key: &str) -> Result<(), SecretsError>;

    /// Short, human-readable label for this backend.
    ///
    /// Used by diagnostic output (e.g. `doctor` command) to indicate which
    /// storage backend is active. Examples: `"file-based (~/.mimofan/secrets/)"`,
    /// `"system keyring"`, `"in-memory (test)"`.
    fn backend_name(&self) -> &'static str;
}

/// OS-native keyring backend.
///
/// Wraps the macOS Keychain (via `security` framework).
///
/// This backend is opt-in -- set the [`SECRET_BACKEND_ENV`] environment
/// variable to `system` or `keyring` to activate it. On platforms other
/// than macOS, [`probe`](DefaultKeyringStore::probe) returns an unsupported
/// error so [`Secrets::auto_detect`] can transparently fall back to
/// [`FileKeyringStore`].
#[derive(Debug, Clone)]
pub struct DefaultKeyringStore {
    /// Keyring service name used to namespace stored credentials.
    /// Defaults to [`DEFAULT_SERVICE`].
    service: String,
}

impl Default for DefaultKeyringStore {
    fn default() -> Self {
        Self::new(DEFAULT_SERVICE)
    }
}

impl DefaultKeyringStore {
    /// Build a new store with the given service name.
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Probe the OS keyring without writing anything. Returns `Ok(())` if
    /// a backend is reachable, otherwise an error describing why not.
    ///
    /// Only supported on macOS. On other platforms returns an unsupported error.
    pub fn probe(&self) -> Result<(), SecretsError> {
        #[cfg(target_os = "macos")]
        {
            // `Entry::new` is enough to validate the native macOS Keychain
            // backend path.
            let entry = keyring::Entry::new(&self.service, "__probe__")
                .map_err(|err| SecretsError::Keyring(err.to_string()))?;
            let _ = entry;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = &self.service;
            Err(SecretsError::Keyring(
                "system keyring backend is only supported on macOS".to_string(),
            ))
        }
    }
}

impl KeyringStore for DefaultKeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, SecretsError> {
        #[cfg(target_os = "macos")]
        {
            let entry = keyring::Entry::new(&self.service, key)
                .map_err(|err| SecretsError::Keyring(err.to_string()))?;
            match entry.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(err) => Err(SecretsError::Keyring(err.to_string())),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = key;
            Err(SecretsError::Keyring(
                "system keyring backend is only supported on macOS".to_string(),
            ))
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretsError> {
        #[cfg(target_os = "macos")]
        {
            let entry = keyring::Entry::new(&self.service, key)
                .map_err(|err| SecretsError::Keyring(err.to_string()))?;
            entry
                .set_password(value)
                .map_err(|err| SecretsError::Keyring(err.to_string()))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (key, value);
            Err(SecretsError::Keyring(
                "system keyring backend is only supported on macOS".to_string(),
            ))
        }
    }

    fn delete(&self, key: &str) -> Result<(), SecretsError> {
        #[cfg(target_os = "macos")]
        {
            let entry = keyring::Entry::new(&self.service, key)
                .map_err(|err| SecretsError::Keyring(err.to_string()))?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(err) => Err(SecretsError::Keyring(err.to_string())),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = key;
            Err(SecretsError::Keyring(
                "system keyring backend is only supported on macOS".to_string(),
            ))
        }
    }

    fn backend_name(&self) -> &'static str {
        "system keyring"
    }
}

/// In-memory keyring store for tests.
///
/// Stores secrets in a [`HashMap`] protected by a [`Mutex`]. Not persisted
/// to disk -- all entries are lost when the process exits. This is the
/// preferred store for unit tests because it requires no filesystem setup
/// and is safe to use in parallel test threads.
#[derive(Debug, Default)]
pub struct InMemoryKeyringStore {
    /// Thread-safe map of key-value pairs.
    entries: Mutex<HashMap<String, String>>,
}

impl InMemoryKeyringStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeyringStore for InMemoryKeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, SecretsError> {
        let guard = self.entries.lock().map_err(|e| {
            SecretsError::Keyring(format!("InMemoryKeyringStore mutex poisoned: {e}"))
        })?;
        Ok(guard.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretsError> {
        let mut guard = self.entries.lock().map_err(|e| {
            SecretsError::Keyring(format!("InMemoryKeyringStore mutex poisoned: {e}"))
        })?;
        guard.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecretsError> {
        let mut guard = self.entries.lock().map_err(|e| {
            SecretsError::Keyring(format!("InMemoryKeyringStore mutex poisoned: {e}"))
        })?;
        guard.remove(key);
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "in-memory (test)"
    }
}

/// JSON-on-disk secret store for headless environments.
///
/// This is the default backend. Secrets are serialised as a JSON object
/// at `<home>/.mimofan/secrets/secrets.json` with Unix file mode `0600`
/// (owner read/write only). The parent directory is created with mode `0700`
/// if it does not exist.
///
/// On Unix, the store rejects files whose permissions are more permissive
/// than `0600` (i.e. group or world bits are set). This prevents other
/// users on the system from reading stored credentials. On Windows, the
/// ACL model is too different to enforce programmatically; callers are
/// responsible for placing the file in a per-user directory.
#[derive(Debug, Clone)]
pub struct FileKeyringStore {
    /// Absolute path to the JSON secrets file.
    path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileSecretsBlob {
    #[serde(default)]
    entries: HashMap<String, String>,
}

impl FileKeyringStore {
    /// Build a store backed by the given JSON file path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Default path: `<home>/.mimofan/secrets/secrets.json`. Honours
    /// `MIMOFAN_HOME`, then `HOME`, `USERPROFILE`, and finally the platform
    /// home directory from the `dirs` crate.
    pub fn default_path() -> Result<PathBuf, SecretsError> {
        default_mimofan_secrets_path()
    }

    fn home_dir() -> Result<PathBuf, SecretsError> {
        for var in ["HOME", "USERPROFILE"] {
            if let Ok(value) = std::env::var(var) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Ok(PathBuf::from(trimmed));
                }
            }
        }

        dirs::home_dir().ok_or_else(|| {
            SecretsError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "could not resolve home directory for FileKeyringStore",
            ))
        })
    }

    /// Path used for storage.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load_unlocked(&self) -> Result<FileSecretsBlob, SecretsError> {
        if !self.path.exists() {
            return Ok(FileSecretsBlob::default());
        }
        // Reject files with unsafe permissions on unix. On Windows the
        // ACL model is too different to enforce here; the caller is
        // responsible for placing the file in a per-user directory.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&self.path)?;
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(SecretsError::InsecurePermissions {
                    path: self.path.clone(),
                    mode,
                });
            }
        }
        let raw = fs::read_to_string(&self.path)?;
        if raw.trim().is_empty() {
            return Ok(FileSecretsBlob::default());
        }
        let blob: FileSecretsBlob = serde_json::from_str(&raw)?;
        Ok(blob)
    }

    fn store_unlocked(&self, blob: &FileSecretsBlob) -> Result<(), SecretsError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(parent)?.permissions();
                perms.set_mode(0o700);
                let _ = fs::set_permissions(parent, perms);
            }
        }
        let body = serde_json::to_string_pretty(blob)?;
        write_private_file(&self.path, body.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Best-effort 0o600 — matches the parent-dir chmod above which
            // is also `let _ = ...`. Filesystems that don't support Unix
            // chmod (Docker bind-mounts of NTFS, network shares — #897)
            // would otherwise fail the whole save here even though the
            // blob already wrote successfully. The host's native ACLs
            // are doing access control in those environments.
            if let Ok(meta) = fs::metadata(&self.path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                let _ = fs::set_permissions(&self.path, perms);
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn write_private_file(path: &Path, body: &[u8]) -> Result<(), SecretsError> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(body)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, body: &[u8]) -> Result<(), SecretsError> {
    fs::write(path, body)?;
    Ok(())
}

impl KeyringStore for FileKeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, SecretsError> {
        let blob = self.load_unlocked()?;
        Ok(blob.entries.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretsError> {
        // load_unlocked already returns Ok(default) for a missing file, so the
        // first-write-creates-the-file path is preserved. Any other Err
        // (insecure permissions, corrupt JSON, transient I/O) MUST surface to
        // the caller — propagating it via `unwrap_or_default()` silently
        // wipes every previously stored secret on the next `store_unlocked`.
        let mut blob = self.load_unlocked()?;
        blob.entries.insert(key.to_string(), value.to_string());
        self.store_unlocked(&blob)
    }

    fn delete(&self, key: &str) -> Result<(), SecretsError> {
        // Same invariant as `set`: never fall back to an empty blob on read
        // error, or `delete <one-key>` becomes `delete <every-key>`.
        let mut blob = self.load_unlocked()?;
        blob.entries.remove(key);
        self.store_unlocked(&blob)
    }

    fn backend_name(&self) -> &'static str {
        FILE_BACKEND_LABEL
    }
}

fn default_mimofan_secrets_path() -> Result<PathBuf, SecretsError> {
    if let Ok(value) = std::env::var("MIMOFAN_HOME") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed).join("secrets").join("secrets.json"));
        }
    }
    Ok(FileKeyringStore::home_dir()?
        .join(".mimofan")
        .join("secrets")
        .join("secrets.json"))
}

/// How the secret backend was selected from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretBackendSelection {
    /// Use the file-backed JSON store (default).
    File,
    /// Use the OS credential store (keyring).
    System,
    /// Unrecognised backend value; falls back to file.
    Unknown,
}

/// Map a raw `MIMOFAN_SECRET_BACKEND` value to a [`SecretBackendSelection`].
pub fn secret_backend_selection(value: Option<&str>) -> SecretBackendSelection {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => SecretBackendSelection::File,
        Some(value) => match value.to_ascii_lowercase().as_str() {
            "file" | "local" | "json" => SecretBackendSelection::File,
            "system" | "keyring" | "os" | "os-keyring" => SecretBackendSelection::System,
            _ => SecretBackendSelection::Unknown,
        },
    }
}

fn configured_secret_backend() -> Option<String> {
    std::env::var(SECRET_BACKEND_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// High-level facade combining a [`KeyringStore`] with environment variable fallbacks.
///
/// Lookup precedence: **secret store -> env -> none**. Callers that also
/// have a TOML config layer must wire that themselves at the very end
/// of the chain (the config crate handles this).
///
/// # Examples
///
/// ```no_run
/// use mimofan_secrets::Secrets;
///
/// let secrets = Secrets::auto_detect();
/// if let Some(key) = secrets.resolve("deepseek") {
///     // use the API key
/// }
/// ```
#[derive(Clone)]
pub struct Secrets {
    /// Underlying secret store backend.
    pub store: Arc<dyn KeyringStore>,
    /// Owner identifier within the secret store (typically `"deepseek"`).
    /// The `key` parameter passed to [`resolve`](Secrets::resolve) is
    /// forwarded to the store as-is, while environment variables are
    /// looked up by canonical provider name via [`env_for`].
    service: String,
}

/// Identifies which layer in the resolution chain supplied a secret.
///
/// Returned by [`Secrets::resolve_with_source`] so callers can
/// distinguish whether a value came from the configured store or from
/// a process environment variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSource {
    /// The secret was returned by the configured [`KeyringStore`] backend.
    Keyring,
    /// The secret was found in a process environment variable.
    Env,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets")
            .field("backend", &self.store.backend_name())
            .field("service", &self.service)
            .finish()
    }
}

impl Secrets {
    /// Build a new facade around the given store, using the
    /// [`DEFAULT_SERVICE`] service name.
    #[must_use]
    pub fn new(store: Arc<dyn KeyringStore>) -> Self {
        Self {
            store,
            service: DEFAULT_SERVICE.to_string(),
        }
    }

    /// Auto-detect the best available backend based on the environment.
    ///
    /// Selection logic:
    /// 1. If [`SECRET_BACKEND_ENV`] is set to `system`/`keyring`/`os`/`os-keyring`,
    ///    probe the OS keyring. If the probe succeeds, use it; otherwise
    ///    fall back to the file-based store with a warning.
    /// 2. If the env var is unset, empty, or `file`/`local`/`json`, use
    ///    the file-based store directly.
    /// 3. If the env var is set to an unrecognised value, log a warning
    ///    and use the file-based store.
    pub fn auto_detect() -> Self {
        match secret_backend_selection(configured_secret_backend().as_deref()) {
            SecretBackendSelection::File => Self::file_backed_default(),
            SecretBackendSelection::Unknown => {
                tracing::warn!(
                    "{SECRET_BACKEND_ENV} has an unsupported value; using file-backed secret store"
                );
                Self::file_backed_default()
            }
            SecretBackendSelection::System => {
                let default_store = DefaultKeyringStore::default();
                match default_store.probe() {
                    Ok(()) => Self::new(Arc::new(default_store)),
                    Err(err) => {
                        tracing::warn!(
                            "OS keyring unavailable ({err}); falling back to file-backed secret store"
                        );
                        Self::file_backed_default()
                    }
                }
            }
        }
    }

    fn file_backed_default() -> Self {
        let path = FileKeyringStore::default_path()
            .unwrap_or_else(|_| PathBuf::from(".mimofan-secrets.json"));
        Self::new(Arc::new(FileKeyringStore::new(path)))
    }

    /// Construct the file-backed default backend directly.
    #[must_use]
    pub fn file_backed() -> Self {
        Self::file_backed_default()
    }

    /// Construct the opt-in OS credential backend, falling back to the
    /// file-backed store when the platform backend is unavailable.
    #[must_use]
    pub fn system_keyring() -> Self {
        let default_store = DefaultKeyringStore::default();
        match default_store.probe() {
            Ok(()) => Self::new(Arc::new(default_store)),
            Err(err) => {
                tracing::warn!(
                    "OS keyring unavailable ({err}); falling back to file-backed secret store"
                );
                Self::file_backed_default()
            }
        }
    }

    /// Backend label, suitable for `doctor` output.
    #[must_use]
    pub fn backend_name(&self) -> &'static str {
        self.store.backend_name()
    }

    /// Resolve a secret with `secret store → env → none` precedence.
    ///
    /// `name` is the canonical provider name or a supported provider alias.
    /// Empty strings on either layer are treated as "not set".
    #[must_use]
    pub fn resolve(&self, name: &str) -> Option<String> {
        self.resolve_with_source(name).map(|(value, _)| value)
    }

    /// Resolve a secret and report which layer supplied it.
    #[must_use]
    pub fn resolve_with_source(&self, name: &str) -> Option<(String, SecretSource)> {
        if let Ok(Some(v)) = self.store.get(name)
            && !v.trim().is_empty()
        {
            return Some((v, SecretSource::Keyring));
        }
        env_for(name).map(|value| (value, SecretSource::Env))
    }

    /// Convenience: write a secret through the underlying store.
    pub fn set(&self, name: &str, value: &str) -> Result<(), SecretsError> {
        self.store.set(name, value)
    }

    /// Convenience: delete a secret through the underlying store.
    pub fn delete(&self, name: &str) -> Result<(), SecretsError> {
        self.store.delete(name)
    }

    /// Convenience: read a secret directly (no env fallback).
    pub fn get(&self, name: &str) -> Result<Option<String>, SecretsError> {
        self.store.get(name)
    }

    /// Resolve a secret by key name with an optional source constraint.
    ///
    /// This is the fleet-worker secret resolution path. Unlike
    /// [`resolve`](Secrets::resolve), this does NOT map provider names
    /// to their canonical env vars — the caller controls the exact key
    /// and resolution order.
    ///
    /// `source_hint` controls the resolution order:
    /// - `Some("env")` — only check environment variables
    /// - `Some("keyring")` — only check the keyring/file store
    /// - `None` — try the store first, then fall back to environment
    #[must_use]
    pub fn resolve_direct(&self, key: &str, source_hint: Option<&str>) -> Option<String> {
        match source_hint {
            Some("env") => {
                // Only check process environment — skip the store entirely.
                std::env::var(key).ok().filter(|v| !v.trim().is_empty())
            }
            Some("keyring") | Some("file") => {
                // Only check the store backend.
                self.store
                    .get(key)
                    .ok()
                    .flatten()
                    .filter(|v| !v.trim().is_empty())
            }
            Some(_) | None => {
                // Default: store first, then env fallback.
                if let Ok(Some(v)) = self.store.get(key)
                    && !v.trim().is_empty()
                {
                    return Some(v);
                }
                std::env::var(key).ok().filter(|v| !v.trim().is_empty())
            }
        }
    }
}

/// Map a canonical provider name to its environment variable(s), returning
/// the first non-empty value found.
///
/// Provider names are case-insensitive. Supported providers and their
/// environment variables:
///
/// | Provider | Env var(s) |
/// |---|---|
/// | `deepseek` | `MIMOFAN_API_KEY` |
/// | `openrouter` | `OPENROUTER_API_KEY` |
/// | `openai-compatible` | `OPENAI_COMPATIBLE_API_KEY`, `OPENAI_API_KEY` |
/// | `novita` | `NOVITA_API_KEY` |
/// | `nvidia` / `nvidia-nim` / `nim` | `NVIDIA_API_KEY`, `NVIDIA_NIM_API_KEY`, `MIMOFAN_API_KEY` |
/// | `fireworks` | `FIREWORKS_API_KEY` |
/// | `siliconflow` / `siliconflow-cn` | `SILICONFLOW_API_KEY` |
/// | `arcee` / `arcee-ai` | `ARCEE_API_KEY` |
/// | `moonshot` / `kimi` | `MOONSHOT_API_KEY`, `KIMI_API_KEY` |
/// | `openai` | `OPENAI_API_KEY` |
/// | `volcengine` / `ark` | `VOLCENGINE_API_KEY`, `VOLCENGINE_ARK_API_KEY`, `ARK_API_KEY` |
///
/// # Deprecated aliases
///
/// The retired `xiaomi-mimo` / `mimo` / `xiaomi` product names are still
/// accepted and route to the generic OpenAI-compatible provider. Their
/// legacy env keys (`XIAOMI_MIMO_API_KEY`, `MIMO_API_KEY`) remain readable
/// as a fallback for existing user setups, but new configurations should
/// use `OPENAI_COMPATIBLE_API_KEY` (or `OPENAI_API_KEY`).
///
/// Returns `None` if the provider is not recognised or none of its
/// candidate environment variables are set to a non-empty value.
#[must_use]
pub fn env_for(name: &str) -> Option<String> {
    let candidates: &[&str] = match name.to_ascii_lowercase().as_str() {
        "deepseek" => &["MIMOFAN_API_KEY"],
        "openrouter" => &["OPENROUTER_API_KEY"],
        // Canonical OpenAI-compatible provider. The `xiaomi-mimo` family are
        // retired product aliases kept for backwards compatibility; their
        // legacy env keys stay as a deprecated fallback.
        "openai-compatible"
        | "openai_compatible"
        | "xiaomi-mimo"
        | "xiaomi_mimo"
        | "xiaomimimo"
        | "mimo"
        | "xiaomi" => &[
            "OPENAI_COMPATIBLE_API_KEY",
            // Deprecated: legacy product-specific keys.
            "XIAOMI_MIMO_API_KEY",
            "MIMO_API_KEY",
            "OPENAI_API_KEY",
        ],
        "novita" => &["NOVITA_API_KEY"],
        "nvidia" | "nvidia-nim" | "nvidia_nim" | "nim" => {
            &["NVIDIA_API_KEY", "NVIDIA_NIM_API_KEY", "MIMOFAN_API_KEY"]
        }
        "fireworks" | "fireworks-ai" => &["FIREWORKS_API_KEY"],
        "siliconflow" | "silicon-flow" | "silicon_flow" | "siliconflow-cn" | "siliconflow_cn"
        | "silicon-flow-cn" | "silicon_flow_cn" | "siliconflow-china" => &["SILICONFLOW_API_KEY"],
        "arcee" | "arcee-ai" | "arcee_ai" => &["ARCEE_API_KEY"],
        "moonshot" | "moonshot-ai" | "kimi" | "kimi-k2" => &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
        "openai" => &["OPENAI_API_KEY"],
        "anthropic" | "claude" => &["ANTHROPIC_API_KEY"],
        "volcengine" | "volcengine-ark" | "volcengine_ark" | "ark" | "volc-ark"
        | "volcengineark" => &[
            "VOLCENGINE_API_KEY",
            "VOLCENGINE_ARK_API_KEY",
            "ARK_API_KEY",
        ],
        _ => return None,
    };
    for var in candidates {
        if let Ok(value) = std::env::var(var)
            && !value.trim().is_empty()
        {
            return Some(value);
        }
    }
    None
}
