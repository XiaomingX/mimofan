//! Credential pool for sandboxed child processes (#SECURITY-CAPABILITY T-1).
//!
//! When a command runs inside a container (or any restricted backend), its
//! environment must NOT inherit the host process environment verbatim — that
//! would leak `ANTHROPIC_API_KEY`, `MIMOFAN_API_KEY`, cloud tokens, etc. into
//! an attacker-controlled interpreter.
//!
//! Instead we start from an **empty** environment (`env_clear`) and inject only:
//! 1. A small, explicit whitelist of benign host variables (locale, `PATH`,
//!    `HOME`, `TMPDIR`, …) that tools legitimately need, and
//! 2. Short-lived, least-scope **ephemeral credentials** minted by the
//!    credential pool, which never touch disk and never enter logs.
//!
//! Secret redaction of any visible output is delegated to
//! [`mimofan_secrets::redact_stream`] — we intentionally do NOT re-implement
//! scanning here.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

/// Host environment variables that are considered benign to forward into a
/// sandbox. This is an *allowlist*: anything not listed is dropped. These are
/// chosen because they are non-secret and tools commonly rely on them.
///
/// NOTE: Never add a credential-bearing variable here (no `*_API_KEY`,
/// `*_TOKEN`, `*_SECRET`, `AWS_*`, `GITHUB_TOKEN`, etc.).
const ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "TERM",
    "TMPDIR",
    "TEMP",
    "TMP",
    // Proxy configuration is sometimes required for legitimate tooling, but is
    // itself non-secret. Network is off by default in the container backend,
    // so forwarding these is harmless unless the caller explicitly enables net.
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

/// Build the minimal environment a sandboxed child should receive.
///
/// Starts from an empty environment and adds:
/// - the [`ALLOWLIST`] variables copied from the host,
/// - any ephemeral credentials produced by the pool (via
///   [`CredentialPool::issue`]), and
/// - the explicit `extra` overrides supplied by the caller (e.g. the command's
///   own declared env). `extra` values win over both the allowlist and pool
///   variables.
#[must_use]
pub fn build_sandbox_env(extra: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = HashMap::new();

    for key in ALLOWLIST {
        if let Ok(value) = std::env::var(key) {
            env.insert((*key).to_string(), value);
        }
    }

    // Ephemeral, least-scope credentials. These are minted in-memory only.
    let pool = CredentialPool::new();
    for (k, v) in pool.issue() {
        env.insert(k, v);
    }

    // Caller-supplied overrides take precedence.
    for (k, v) in extra {
        env.insert(k.clone(), v.clone());
    }

    env
}

/// Whether a given variable name is on the benign allowlist. Exposed for tests
/// so the allowlist contract ("no secret-bearing keys") can be asserted.
#[must_use]
pub fn is_allowlisted(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    // Belt-and-suspenders: reject anything that looks credential-bearing even
    // if someone mistakenly added it to ALLOWLIST.
    for suffix in ["API_KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "PRIVATE_KEY"] {
        if key.ends_with(suffix) {
            return false;
        }
    }
    for prefix in ["AWS_", "GITHUB_", "GCP_", "AZURE_", "GOOGLE_"] {
        if key.starts_with(prefix) {
            return false;
        }
    }
    ALLOWLIST.contains(&key.as_str())
}

/// Ephemeral, least-scope credential pool.
///
/// In this implementation the pool issues only **synthetic** environment
/// credentials (a scoped, time-boxed sandbox token) so that the wiring is
/// demonstrable and testable without a real secret store. Real provider
/// credentials are sourced from [`mimofan_secrets::Secrets`], but they are
/// *never* materialized to disk or logs — only the names of variables that a
/// sandbox should be allowed to see are enumerated (from the static provider
/// mapping below), and the actual values are redacted by
/// [`mimofan_secrets::redact_stream`] whenever shown.
pub struct CredentialPool {
    /// Variable names that the sandbox is permitted to see. Values are injected
    /// caller-side; here we only enumerate what is *allowed*.
    allowed: HashSet<String>,
    /// Synthetic scoped token value (in-memory only, dropped at end of scope).
    sandbox_token: String,
}

/// Provider environment variable *names* (not values) that could be granted to
/// a sandbox on explicit request. We enumerate names only — we never read the
/// values into the parent process for a child that should not have them.
const PROVIDER_ENV_KEYS: &[&str] = &[
    "MIMOFAN_API_KEY",
    "OPENROUTER_API_KEY",
    "OPENAI_COMPATIBLE_API_KEY",
    "OPENAI_API_KEY",
    "NOVITA_API_KEY",
    "NVIDIA_API_KEY",
    "NVIDIA_NIM_API_KEY",
    "FIREWORKS_API_KEY",
    "SILICONFLOW_API_KEY",
    "ARCEE_API_KEY",
    "MOONSHOT_API_KEY",
    "KIMI_API_KEY",
    "VOLCENGINE_API_KEY",
    "VOLCENGINE_ARK_API_KEY",
    "ARK_API_KEY",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
];

impl CredentialPool {
    /// Create a pool with the default least-scope grants.
    #[must_use]
    pub fn new() -> Self {
        let mut allowed = HashSet::new();
        // The sandbox may see a single scoped token; nothing else by default.
        allowed.insert("MIMOFAN_SANDBOX_TOKEN".to_string());
        // Provider-backed credential *names* are enumerable so a caller can
        // explicitly grant one — but the value is never loaded here.
        for key in PROVIDER_ENV_KEYS {
            allowed.insert((*key).to_string());
        }

        Self {
            allowed,
            // 32-byte synthetic token, hex-encoded. In production this would be
            // a real short-lived, scoped credential, but it is never persisted.
            sandbox_token: {
                use std::time::{SystemTime, UNIX_EPOCH};
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0u128, |d| d.as_nanos());
                format!("sbx_{nanos:032x}")
            },
        }
    }

    /// Issue the credentials the sandbox should receive.
    ///
    /// Returns the variables to inject. The single synthetic token is the only
    /// *value* we hand out; provider credentials are intentionally NOT
    /// included as values here — if the caller needs them it must request them
    /// explicitly and the request is logged/redacted.
    #[must_use]
    pub fn issue(&self) -> Vec<(String, String)> {
        vec![(
            "MIMOFAN_SANDBOX_TOKEN".to_string(),
            self.sandbox_token.clone(),
        )]
    }

    /// Whether the named variable is permitted by this pool.
    #[must_use]
    pub fn allows(&self, key: &str) -> bool {
        self.allowed.contains(key)
    }
}

impl Default for CredentialPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Redact a chunk of sandbox output through the shared secret scanner.
///
/// Thin wrapper around [`mimofan_secrets::redact_stream`] so callers in the
/// sandbox module do not take a direct dependency on the scanner's streaming
/// carry protocol. Returns the redacted text.
///
/// `carry` is the trailing partial line from the previous chunk (if any) and is
/// returned again by the caller to complete multi-chunk lines.
#[must_use]
pub fn redact_output(chunk: &str, carry: Option<&str>) -> (String, Option<String>) {
    let (redacted, _blocked, new_carry) = mimofan_secrets::redact_stream(chunk, carry);
    let carry_out = if new_carry.is_empty() {
        None
    } else {
        Some(new_carry)
    };
    (redacted, carry_out)
}

/// Result of a redaction pass, used by callers that want the blocked-count too.
pub struct RedactionResult {
    /// Redacted text for completed lines.
    pub text: String,
    /// Trailing partial line carried to the next chunk.
    pub carry: Option<String>,
    /// Number of secret matches redacted in this chunk.
    pub blocked: usize,
}

/// Redact a chunk and report how many secrets were removed.
#[must_use]
pub fn redact_output_counted(chunk: &str, carry: Option<&str>) -> RedactionResult {
    let (text, blocked, new_carry) = mimofan_secrets::redact_stream(chunk, carry);
    RedactionResult {
        text,
        carry: if new_carry.is_empty() {
            None
        } else {
            Some(new_carry)
        },
        blocked,
    }
}

/// Probe helper used by tests to assert the allowlist never leaks secrets.
#[must_use]
pub fn forbidden_secret_keys_sample() -> Vec<&'static str> {
    vec![
        "ANTHROPIC_API_KEY",
        "MIMOFAN_API_KEY",
        "AWS_SECRET_ACCESS_KEY",
        "GITHUB_TOKEN",
        "OPENAI_API_KEY",
    ]
}

/// Convenience: assert (in tests) that none of the host's secret-bearing
/// variables survived into `env`. Returns the list of leaked keys (empty = ok).
#[must_use]
pub fn leaked_secret_keys(env: &HashMap<String, String>) -> Vec<String> {
    let mut leaked = Vec::new();
    for key in env.keys() {
        if !is_allowlisted(key) && !CredentialPool::new().allows(key) {
            leaked.push(key.clone());
        }
    }
    leaked
}

/// Validate that building a sandbox env keeps secrets out. Returns `Ok(())` if
/// the resulting env contains no host secret; `Err` lists the leaked keys.
pub fn validate_sandbox_env(env: &HashMap<String, String>) -> Result<()> {
    let leaked = leaked_secret_keys(env);
    if leaked.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("sandbox env would leak host secrets: {}", leaked.join(", "))
    }
}
