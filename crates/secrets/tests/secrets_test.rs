use mimofan_secrets::*;
use std::fs;
use std::sync::{Arc, Mutex, OnceLock};

/// Serialise env-mutating tests: tests in this module poke
/// `MIMOFAN_API_KEY` etc., which is process-global.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn clear_known_envs() {
    for var in [
        "MIMOFAN_HOME",
        "MIMOFAN_API_KEY",
        "OPENROUTER_API_KEY",
        "NOVITA_API_KEY",
        "NVIDIA_API_KEY",
        "NVIDIA_NIM_API_KEY",
        "FIREWORKS_API_KEY",
        "SILICONFLOW_API_KEY",
        "ARCEE_API_KEY",
        "OPENAI_API_KEY",
        "XIAOMI_MIMO_API_KEY",
        SECRET_BACKEND_ENV,
    ] {
        // Safety: tests serialise on env_lock(); the broader
        // workspace has the same pattern in `crates/config`.
        unsafe { std::env::remove_var(var) };
    }
}

struct EnvVarGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        unsafe { std::env::set_var(name, value) };
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => unsafe { std::env::set_var(self.name, value) },
            None => unsafe { std::env::remove_var(self.name) },
        }
    }
}

#[test]
fn backend_selection_defaults_to_file() {
    assert_eq!(secret_backend_selection(None), SecretBackendSelection::File);
    assert_eq!(
        secret_backend_selection(Some("")),
        SecretBackendSelection::File
    );
    assert_eq!(
        secret_backend_selection(Some("  file  ")),
        SecretBackendSelection::File
    );
}

#[test]
fn backend_selection_accepts_explicit_system_keyring() {
    assert_eq!(
        secret_backend_selection(Some("system")),
        SecretBackendSelection::System
    );
    assert_eq!(
        secret_backend_selection(Some("keyring")),
        SecretBackendSelection::System
    );
    assert_eq!(
        secret_backend_selection(Some("os-keyring")),
        SecretBackendSelection::System
    );
}

#[test]
fn auto_detect_is_file_backed_by_default() {
    let _lock = env_lock();
    clear_known_envs();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let _home = EnvVarGuard::set("HOME", tmp.path());
    let _userprofile = EnvVarGuard::set("USERPROFILE", tmp.path());

    let secrets = Secrets::auto_detect();

    assert_eq!(secrets.backend_name(), FILE_BACKEND_LABEL);
}

#[test]
fn auto_detect_honors_explicit_file_backend() {
    let _lock = env_lock();
    clear_known_envs();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let _home = EnvVarGuard::set("HOME", tmp.path());
    let _userprofile = EnvVarGuard::set("USERPROFILE", tmp.path());
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var(SECRET_BACKEND_ENV, "local") };

    let secrets = Secrets::auto_detect();

    assert_eq!(secrets.backend_name(), FILE_BACKEND_LABEL);
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var(SECRET_BACKEND_ENV) };
}

#[test]
fn file_default_path_uses_mimofan_home() {
    let _lock = env_lock();
    clear_known_envs();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let _home = EnvVarGuard::set("HOME", tmp.path());
    let _userprofile = EnvVarGuard::set("USERPROFILE", tmp.path());

    let path = FileKeyringStore::default_path().expect("resolve default secrets path");

    assert_eq!(
        path,
        tmp.path()
            .join(".mimofan")
            .join("secrets")
            .join("secrets.json")
    );
}

#[test]
fn file_default_path_honors_mimofan_home() {
    let _lock = env_lock();
    clear_known_envs();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let custom = tmp.path().join("custom-mimofan");
    let _home = EnvVarGuard::set("HOME", tmp.path());
    let _userprofile = EnvVarGuard::set("USERPROFILE", tmp.path());
    let _mimofan_home = EnvVarGuard::set("MIMOFAN_HOME", &custom);

    let path = FileKeyringStore::default_path().expect("resolve default secrets path");

    assert_eq!(path, custom.join("secrets").join("secrets.json"));
}

#[test]
fn file_default_path_migrates_legacy_entries_to_mimofan() {
    let _lock = env_lock();
    clear_known_envs();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let _home = EnvVarGuard::set("HOME", tmp.path());
    let _userprofile = EnvVarGuard::set("USERPROFILE", tmp.path());
    let legacy = tmp
        .path()
        .join(".mimofan")
        .join("secrets")
        .join("secrets.json");
    FileKeyringStore::new(legacy.clone())
        .set("xiaomi-mimo", "legacy-mimo")
        .expect("write legacy xiaomi-mimo secret");

    let primary = FileKeyringStore::default_path().expect("resolve default secrets path");
    let primary_store = FileKeyringStore::new(primary.clone());

    assert_eq!(
        primary,
        tmp.path()
            .join(".mimofan")
            .join("secrets")
            .join("secrets.json")
    );
    assert_eq!(
        primary_store.get("xiaomi-mimo").expect("get xiaomi-mimo secret from primary store").as_deref(),
        Some("legacy-mimo")
    );
    assert!(
        legacy.exists(),
        "migration copies; it does not delete legacy data"
    );
}

#[test]
fn file_default_path_migration_preserves_primary_values() {
    let _lock = env_lock();
    clear_known_envs();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let _home = EnvVarGuard::set("HOME", tmp.path());
    let _userprofile = EnvVarGuard::set("USERPROFILE", tmp.path());
    let legacy = tmp
        .path()
        .join(".mimofan")
        .join("secrets")
        .join("secrets.json");
    let primary = tmp
        .path()
        .join(".mimofan")
        .join("secrets")
        .join("secrets.json");
    FileKeyringStore::new(legacy)
        .set("openrouter", "legacy-openrouter")
        .expect("write legacy openrouter secret");
    let primary_store = FileKeyringStore::new(primary.clone());
    primary_store
        .set("openrouter", "primary-openrouter")
        .expect("write primary openrouter secret");

    let resolved = FileKeyringStore::default_path().expect("resolve default secrets path");

    assert_eq!(resolved, primary);
    assert_eq!(
        primary_store.get("openrouter").expect("get openrouter secret from primary store").as_deref(),
        Some("primary-openrouter")
    );
}

#[test]
fn in_memory_store_round_trips() {
    let store = InMemoryKeyringStore::new();
    assert_eq!(store.get("deepseek").expect("get deepseek secret from in-memory store"), None);
    store.set("deepseek", "sk-test").expect("write deepseek test secret");
    assert_eq!(store.get("deepseek").expect("get deepseek secret from in-memory store"), Some("sk-test".to_string()));
    store.set("deepseek", "sk-replaced").expect("overwrite deepseek secret");
    assert_eq!(
        store.get("deepseek").expect("get deepseek secret from in-memory store"),
        Some("sk-replaced".to_string())
    );
    store.delete("deepseek").expect("delete deepseek secret");
    assert_eq!(store.get("deepseek").expect("get deepseek secret from in-memory store"), None);
    // Deleting an absent key is a no-op.
    store.delete("missing").expect("delete absent secret (no-op)");
}

#[test]
fn resolve_prefers_keyring_over_env() {
    let _lock = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("MIMOFAN_API_KEY", "env-key") };

    let store = Arc::new(InMemoryKeyringStore::new());
    store.set("deepseek", "ring-key").expect("write deepseek ring-key secret");
    let secrets = Secrets::new(store);

    assert_eq!(secrets.resolve("deepseek").as_deref(), Some("ring-key"));
    assert_eq!(
        secrets.resolve_with_source("deepseek"),
        Some(("ring-key".to_string(), SecretSource::Keyring))
    );
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var("MIMOFAN_API_KEY") };
}

#[test]
fn resolve_falls_back_to_env_when_keyring_empty() {
    let _lock = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("MIMOFAN_API_KEY", "env-fallback") };

    let secrets = Secrets::new(Arc::new(InMemoryKeyringStore::new()));
    assert_eq!(secrets.resolve("deepseek").as_deref(), Some("env-fallback"));
    assert_eq!(
        secrets.resolve_with_source("deepseek"),
        Some(("env-fallback".to_string(), SecretSource::Env))
    );
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var("MIMOFAN_API_KEY") };
}

#[test]
fn resolve_returns_none_when_both_layers_empty() {
    let _lock = env_lock();
    clear_known_envs();
    let secrets = Secrets::new(Arc::new(InMemoryKeyringStore::new()));
    assert_eq!(secrets.resolve("deepseek"), None);
}

#[test]
fn resolve_treats_blank_keyring_value_as_unset() {
    let _lock = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("MIMOFAN_API_KEY", "env-real") };

    let store = Arc::new(InMemoryKeyringStore::new());
    store.set("deepseek", "   ").expect("write blank deepseek secret");
    let secrets = Secrets::new(store);
    assert_eq!(secrets.resolve("deepseek").as_deref(), Some("env-real"));
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var("MIMOFAN_API_KEY") };
}

#[test]
fn nvidia_env_aliases_resolve() {
    let _lock = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("NVIDIA_NIM_API_KEY", "nim-key") };
    let secrets = Secrets::new(Arc::new(InMemoryKeyringStore::new()));
    assert_eq!(secrets.resolve("nvidia-nim").as_deref(), Some("nim-key"));
    assert_eq!(secrets.resolve("nvidia").as_deref(), Some("nim-key"));
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var("NVIDIA_NIM_API_KEY") };
}

#[test]
fn xiaomi_mimo_env_aliases_resolve() {
    let _guard = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("XIAOMI_MIMO_API_KEY", "mimo-key") };

    assert_eq!(env_for("xiaomi-mimo").as_deref(), Some("mimo-key"));
    assert_eq!(env_for("xiaomimimo").as_deref(), Some("mimo-key"));
    assert_eq!(env_for("mimo").as_deref(), Some("mimo-key"));
    assert_eq!(env_for("xiaomi").as_deref(), Some("mimo-key"));

    clear_known_envs();
}

#[test]
fn fireworks_env_aliases_resolve() {
    let _lock = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("FIREWORKS_API_KEY", "fw-key") };

    assert_eq!(env_for("fireworks").as_deref(), Some("fw-key"));
    assert_eq!(env_for("fireworks-ai").as_deref(), Some("fw-key"));
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var("FIREWORKS_API_KEY") };
}

#[test]
fn siliconflow_env_aliases_resolve() {
    let _lock = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("SILICONFLOW_API_KEY", "sf-key") };

    assert_eq!(env_for("siliconflow").as_deref(), Some("sf-key"));
    assert_eq!(env_for("silicon-flow").as_deref(), Some("sf-key"));
    assert_eq!(env_for("silicon_flow").as_deref(), Some("sf-key"));
    assert_eq!(env_for("siliconflow-cn").as_deref(), Some("sf-key"));
    assert_eq!(env_for("silicon_flow_cn").as_deref(), Some("sf-key"));
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var("SILICONFLOW_API_KEY") };
}

#[test]
fn arcee_env_aliases_resolve() {
    let _lock = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("ARCEE_API_KEY", "arcee-key") };

    assert_eq!(env_for("arcee").as_deref(), Some("arcee-key"));
    assert_eq!(env_for("arcee-ai").as_deref(), Some("arcee-key"));
    assert_eq!(env_for("arcee_ai").as_deref(), Some("arcee-key"));
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var("ARCEE_API_KEY") };
}

#[test]
fn moonshot_kimi_env_aliases_resolve() {
    let _lock = env_lock();
    clear_known_envs();
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::set_var("KIMI_API_KEY", "kimi-key") };

    assert_eq!(env_for("moonshot").as_deref(), Some("kimi-key"));
    assert_eq!(env_for("moonshot-ai").as_deref(), Some("kimi-key"));
    assert_eq!(env_for("kimi").as_deref(), Some("kimi-key"));
    assert_eq!(env_for("kimi-k2").as_deref(), Some("kimi-key"));
    // Safety: env mutation guarded by env_lock().
    unsafe { std::env::remove_var("KIMI_API_KEY") };
}

#[cfg(unix)]
#[test]
fn file_store_round_trips_with_secure_perms() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join("nested").join("secrets.json");
    let store = FileKeyringStore::new(path.clone());
    assert_eq!(store.get("deepseek").expect("get deepseek secret from in-memory store"), None);
    store.set("deepseek", "sk-disk").expect("write deepseek secret to disk");
    assert_eq!(store.get("deepseek").expect("get deepseek secret from in-memory store"), Some("sk-disk".to_string()));

    let mode = fs::metadata(&path).expect("read secrets file metadata").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");

    store.set("openrouter", "or-disk").expect("write openrouter secret to disk");
    assert_eq!(
        store.get("openrouter").expect("get openrouter secret from disk store"),
        Some("or-disk".to_string())
    );
    // First entry must still be intact.
    assert_eq!(store.get("deepseek").expect("get deepseek secret from in-memory store"), Some("sk-disk".to_string()));

    store.delete("deepseek").expect("delete deepseek secret");
    assert_eq!(store.get("deepseek").expect("get deepseek secret from in-memory store"), None);
}

#[cfg(unix)]
#[test]
fn file_store_rejects_world_readable_file() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join("secrets.json");
    fs::write(&path, "{\"entries\":{\"deepseek\":\"leak\"}}").expect("write world-readable fixture secrets file");
    let mut perms = fs::metadata(&path).expect("read secrets file metadata").permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&path, perms).expect("chmod secrets file to 0600");

    let store = FileKeyringStore::new(path);
    let err = store.get("deepseek").unwrap_err();
    assert!(
        matches!(err, SecretsError::InsecurePermissions { .. }),
        "unexpected error: {err}"
    );
}

// Regression for #281: `set` and `delete` used to call
// `load_unlocked().unwrap_or_default()`, which silently wiped every
// existing secret whenever the read failed (insecure permissions,
// corrupt JSON, or any other I/O error).

#[cfg(unix)]
#[test]
fn file_store_set_does_not_clobber_secrets_when_perms_are_bad() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join("secrets.json");
    let original = "{\"entries\":{\"deepseek\":\"sk-keep\",\"nvidia\":\"nv-keep\"}}";
    fs::write(&path, original).expect("write fixture secrets file");
    let mut perms = fs::metadata(&path).expect("read secrets file metadata").permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&path, perms).expect("chmod secrets file to 0600");

    let store = FileKeyringStore::new(path.clone());
    let err = store.set("openrouter", "or-new").unwrap_err();
    assert!(
        matches!(err, SecretsError::InsecurePermissions { .. }),
        "set must surface the read error rather than overwriting; got: {err}"
    );

    let on_disk = fs::read_to_string(&path).expect("read secrets file back");
    assert_eq!(
        on_disk, original,
        "set must not modify the file when load_unlocked errored"
    );
}

#[cfg(unix)]
#[test]
fn file_store_delete_does_not_clobber_secrets_when_perms_are_bad() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join("secrets.json");
    let original = "{\"entries\":{\"deepseek\":\"sk-keep\",\"nvidia\":\"nv-keep\"}}";
    fs::write(&path, original).expect("write fixture secrets file");
    let mut perms = fs::metadata(&path).expect("read secrets file metadata").permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&path, perms).expect("chmod secrets file to 0600");

    let store = FileKeyringStore::new(path.clone());
    let err = store.delete("nvidia").unwrap_err();
    assert!(
        matches!(err, SecretsError::InsecurePermissions { .. }),
        "delete must surface the read error rather than wiping the file; got: {err}"
    );
    let on_disk = fs::read_to_string(&path).expect("read secrets file back");
    assert_eq!(on_disk, original);
}

#[test]
fn file_store_set_does_not_clobber_secrets_when_json_is_corrupt() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join("secrets.json");
    // Corrupt JSON. Permissions ok where unix; on Windows the perm-check
    // doesn't run so we exercise the json-error path directly.
    fs::write(&path, "{ this is not valid json").expect("write corrupt-json fixture secrets file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).expect("read secrets file metadata").permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms).expect("chmod secrets file to 0600");
    }

    let store = FileKeyringStore::new(path.clone());
    let err = store.set("deepseek", "sk-new").unwrap_err();
    assert!(
        matches!(err, SecretsError::Json(_)),
        "set must surface the parse error rather than wiping the file; got: {err}"
    );
    let on_disk = fs::read_to_string(&path).expect("read secrets file back");
    assert_eq!(on_disk, "{ this is not valid json");
}

#[test]
fn file_store_set_still_creates_file_when_missing() {
    // Regression guard: the #281 fix removed `unwrap_or_default()` from
    // the load call. Make sure the original first-write-creates-the-file
    // ergonomic still works — `load_unlocked` returns `Ok(default)` for
    // a missing file, so the `?` should pass through cleanly.
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join("nested").join("secrets.json");
    let store = FileKeyringStore::new(path.clone());

    store.set("deepseek", "sk-fresh").expect("write fresh deepseek secret to new file");
    assert_eq!(store.get("deepseek").expect("get deepseek secret from in-memory store"), Some("sk-fresh".to_string()));
}

#[test]
fn file_store_default_path_uses_home() {
    let _lock = env_lock();
    clear_known_envs();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let _home = EnvVarGuard::set("HOME", tmp.path());
    let _userprofile = EnvVarGuard::set("USERPROFILE", tmp.path());

    let path = FileKeyringStore::default_path().expect("resolve default secrets path");
    assert_eq!(
        path,
        tmp.path()
            .join(".mimofan")
            .join("secrets")
            .join("secrets.json")
    );
}
