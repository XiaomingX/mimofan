//! MCP configuration types.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Full MCP configuration from mcp.json
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct McpConfig {
    #[serde(default)]
    pub timeouts: McpTimeouts,
    #[serde(default, alias = "mcpServers")]
    pub servers: HashMap<String, McpServerConfig>,
}

/// Global timeout configuration
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct McpTimeouts {
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: u64,
    #[serde(default = "default_execute_timeout")]
    pub execute_timeout: u64,
    #[serde(default = "default_read_timeout")]
    pub read_timeout: u64,
}

fn default_connect_timeout() -> u64 {
    10
}
fn default_execute_timeout() -> u64 {
    60
}
fn default_read_timeout() -> u64 {
    120
}

impl Default for McpTimeouts {
    fn default() -> Self {
        Self {
            connect_timeout: default_connect_timeout(),
            execute_timeout: default_execute_timeout(),
            read_timeout: default_read_timeout(),
        }
    }
}

/// Configuration for a single MCP server
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    pub url: Option<String>,
    /// Optional explicit HTTP transport override.
    ///
    /// By default URL-based MCP servers use Streamable HTTP first and fall
    /// back to legacy SSE only when the server rejects Streamable HTTP with
    /// a known incompatible status. Set this to `"sse"` for legacy SSE
    /// endpoints that must start with a long-lived GET endpoint discovery
    /// stream and cannot accept an initial POST to the configured URL.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default)]
    pub connect_timeout: Option<u64>,
    #[serde(default)]
    pub execute_timeout: Option<u64>,
    #[serde(default)]
    pub read_timeout: Option<u64>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    /// Extra HTTP headers sent with every request to this MCP server.
    /// Only the HTTP transports (streamable HTTP today; SSE in a
    /// follow-up) honor this — `command`-based stdio servers ignore it.
    ///
    /// Mirrors the `headers` field that Claude Code, Codex, and
    /// OpenCode already accept in their MCP config formats. Use it to
    /// authenticate against gateways that require a Bearer token or
    /// API key, e.g.:
    ///
    /// ```jsonc
    /// "huggingface": {
    ///     "url": "https://huggingface.co/api/mcp",
    ///     "headers": { "Authorization": "Bearer ${HF_TOKEN}" }
    /// }
    /// ```
    ///
    /// Header keys and values are passed through as-is, except that values
    /// may reference environment variables using `${VAR}` or `${VAR:-default}`
    /// syntax (variable name `[A-Z_][A-Z0-9_]*`); these are expanded at config
    /// load time. This keeps bearer/API-token integrations out of plain-text
    /// `mcp.json`, e.g.:
    ///
    /// ```jsonc
    /// "huggingface": {
    ///     "url": "https://huggingface.co/api/mcp",
    ///     "headers": { "Authorization": "Bearer ${HF_TOKEN}" }
    /// }
    /// ```
    ///
    /// A missing variable expands to an empty string (matching CodeBuddy's
    /// "warn, don't abort" behavior); `${VAR:-default}` falls back to
    /// `default` when `VAR` is unset or empty.
    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// HTTP headers whose values are read from environment variables at request
    /// time. This keeps common bearer/API-token integrations out of mcp.json.
    #[serde(default, alias = "env_http_headers")]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub env_headers: HashMap<String, String>,
    /// Environment variable containing a bearer token. When present and set,
    /// mimofan sends `Authorization: Bearer <value>` for URL-based servers.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_token_env_var: Option<String>,
    /// OAuth scopes requested during `mimofan mcp login`.
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    /// OAuth client override for MCP servers that require a pre-registered
    /// public client instead of dynamic registration.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpServerOAuthConfig>,
    /// Optional RFC 8707 resource parameter appended to the authorization URL.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_resource: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct McpServerOAuthConfig {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

fn default_enabled() -> bool {
    true
}

impl McpServerConfig {
    /// Whether this server is enabled (not explicitly disabled).
    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.disabled
    }

    /// Whether a specific tool is enabled on this server.
    ///
    /// A tool is enabled when:
    /// 1. The server itself is enabled, AND
    /// 2. The tool is not in `disabled_tools`, AND
    /// 3. Either `enabled_tools` is empty (all tools allowed) or the tool is in `enabled_tools`.
    pub fn is_tool_enabled(&self, tool_name: &str) -> bool {
        if !self.is_enabled() {
            return false;
        }
        if self.disabled_tools.iter().any(|t| t == tool_name) {
            return false;
        }
        if self.enabled_tools.is_empty() {
            return true;
        }
        self.enabled_tools.iter().any(|t| t == tool_name)
    }

    pub fn effective_connect_timeout(&self, global: &McpTimeouts) -> u64 {
        self.connect_timeout.unwrap_or(global.connect_timeout)
    }

    pub fn effective_execute_timeout(&self, global: &McpTimeouts) -> u64 {
        self.execute_timeout.unwrap_or(global.execute_timeout)
    }

    pub fn effective_read_timeout(&self, global: &McpTimeouts) -> u64 {
        self.read_timeout.unwrap_or(global.read_timeout)
    }

    /// Return a copy of this config with all string fields expanded for
    /// `${VAR}` / `${VAR:-default}` environment-variable references.
    ///
    /// Expansion happens at config-load time so every downstream consumer
    /// (stdio command/args/env, HTTP url/headers, cwd) sees resolved values.
    /// The original config is unchanged; calling this on an already-expanded
    /// config is a no-op (idempotent).
    #[must_use]
    pub fn expand_env_vars(&self) -> McpServerConfig {
        let mut expanded = self.clone();
        expanded.command = expanded.command.take().map(|s| expand_env_in_string(&s));
        expanded.args = expanded.args.iter().map(|a| expand_env_in_string(a)).collect();
        for (_, v) in expanded.env.iter_mut() {
            *v = expand_env_in_string(v);
        }
        expanded.url = expanded.url.take().map(|s| expand_env_in_string(&s));
        expanded.cwd = expanded.cwd.take().map(|p| {
            let s = expand_env_in_string(&p.to_string_lossy());
            PathBuf::from(s)
        });
        for (_, v) in expanded.headers.iter_mut() {
            *v = expand_env_in_string(v);
        }
        // NOTE: `bearer_token_env_var` is the *name* of an env var read at
        // request time (like `env_headers` values), so it is intentionally NOT
        // expanded here.
        expanded.oauth_resource = expanded.oauth_resource.take().map(|s| expand_env_in_string(&s));
        expanded.scopes = expanded.scopes.iter().map(|s| expand_env_in_string(s)).collect();
        expanded
    }
}

/// Expand `${VAR}` and `${VAR:-default}` references in `input`.
///
/// - `${VAR}`: replaced by `VAR`'s value, or an empty string when unset.
/// - `${VAR:-default}`: replaced by `VAR`'s value, or `default` when `VAR` is
///   unset or empty. `default` may itself contain `${...}` (expanded once).
///
/// Variable names must match `[A-Z_][A-Z0-9_]*` (upper-case, matching
/// CodeBuddy's convention). Unterminated `${` and malformed placeholders are
/// left untouched. Non-matching text is preserved verbatim.
pub fn expand_env_in_string(input: &str) -> String {
    const OPEN: &str = "${";
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open_idx) = rest.find(OPEN) {
        out.push_str(&rest[..open_idx]);
        let after = &rest[open_idx + OPEN.len()..];
        let Some(close_idx) = after.find('}') else {
            // Unterminated placeholder: keep the rest verbatim.
            out.push_str(&rest[open_idx..]);
            return out;
        };
        let inner = &after[..close_idx];
        let value = resolve_env_var(inner);
        out.push_str(&value);
        rest = &after[close_idx + 1..];
    }
    out.push_str(rest);
    out
}

/// Resolve the contents of a `${...}` placeholder (without the braces).
///
/// Returns `None` when the placeholder is not a valid env-reference (so the
/// caller can leave it untouched).
fn resolve_env_var(inner: &str) -> String {
    if inner.is_empty() {
        return String::new();
    }
    // `${VAR:-default}` form.
    if let Some((name, default)) = inner.split_once(":-") {
        if is_valid_env_name(name) {
            match std::env::var(name) {
                Ok(v) if !v.is_empty() => return v,
                _ => return expand_env_in_string(default),
            }
        }
    }
    if is_valid_env_name(inner) {
        return std::env::var(inner).unwrap_or_default();
    }
    // Malformed placeholder: leave the original `${...}` text in place.
    format!("${{{inner}}}")
}

/// Variable names are `[A-Z_][A-Z0-9_]*` (upper-case, matching CodeBuddy).
fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_uppercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to set an env var in tests (unsafe on recent toolchains).
    fn set_var(k: &str, v: &str) {
        unsafe { std::env::set_var(k, v) };
    }
    /// Helper to remove an env var in tests.
    fn remove_var(k: &str) {
        unsafe { std::env::remove_var(k) };
    }

    /// Build a `McpServerConfig` with sensible defaults for tests, avoiding the
    /// public `Default` (whose `enabled` would disagree with serde's
    /// `default_enabled`).
    fn test_config() -> McpServerConfig {
        McpServerConfig {
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            url: None,
            transport: None,
            connect_timeout: None,
            execute_timeout: None,
            read_timeout: None,
            disabled: false,
            enabled: true,
            required: false,
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            headers: HashMap::new(),
            env_headers: HashMap::new(),
            bearer_token_env_var: None,
            scopes: Vec::new(),
            oauth: None,
            oauth_resource: None,
        }
    }

    #[test]
    fn expand_basic_var() {
        set_var("MIMOFAN_TEST_TOKEN", "s3cr3t");
        assert_eq!(
            expand_env_in_string("Bearer ${MIMOFAN_TEST_TOKEN}"),
            "Bearer s3cr3t"
        );
        remove_var("MIMOFAN_TEST_TOKEN");
    }

    #[test]
    fn expand_missing_var_becomes_empty() {
        remove_var("MIMOFAN_TEST_UNSET_VAR");
        assert_eq!(expand_env_in_string("x${MIMOFAN_TEST_UNSET_VAR}y"), "xy");
    }

    #[test]
    fn expand_default_when_unset() {
        remove_var("MIMOFAN_TEST_DEFAULT_UNSET");
        assert_eq!(
            expand_env_in_string("${MIMOFAN_TEST_DEFAULT_UNSET:-fallback}"),
            "fallback"
        );
    }

    #[test]
    fn expand_default_when_empty() {
        set_var("MIMOFAN_TEST_EMPTY", "");
        assert_eq!(
            expand_env_in_string("${MIMOFAN_TEST_EMPTY:-fallback}"),
            "fallback"
        );
        remove_var("MIMOFAN_TEST_EMPTY");
    }

    #[test]
    fn expand_default_not_used_when_set() {
        set_var("MIMOFAN_TEST_SET", "real");
        assert_eq!(expand_env_in_string("${MIMOFAN_TEST_SET:-fallback}"), "real");
        remove_var("MIMOFAN_TEST_SET");
    }

    #[test]
    fn expand_leaves_plain_text_and_unterminated_placeholder() {
        assert_eq!(expand_env_in_string("no vars here"), "no vars here");
        // Unterminated `${` is left verbatim.
        assert_eq!(expand_env_in_string("bad ${VAR text"), "bad ${VAR text");
        // Lower-case / invalid name is left verbatim.
        assert_eq!(expand_env_in_string("${var}"), "${var}");
    }

    #[test]
    fn expand_multiple_placeholders() {
        set_var("MIMOFAN_A", "1");
        set_var("MIMOFAN_B", "2");
        assert_eq!(expand_env_in_string("${MIMOFAN_A}-${MIMOFAN_B}"), "1-2");
        remove_var("MIMOFAN_A");
        remove_var("MIMOFAN_B");
    }

    #[test]
    fn server_config_expands_all_string_fields() {
        set_var("MIMOFAN_SRV_TOKEN", "tok");
        set_var("MIMOFAN_SRV_URL", "https://example.com/mcp");
        set_var("MIMOFAN_SRV_HOME", "/opt/srv");
        let mut cfg = test_config();
        cfg.command = Some("${MIMOFAN_SRV_HOME}/bin/run".to_string());
        cfg.args = vec!["--token".into(), "${MIMOFAN_SRV_TOKEN}".into()];
        cfg.env
            .insert("API_KEY".into(), "${MIMOFAN_SRV_TOKEN}".into());
        cfg.url = Some("${MIMOFAN_SRV_URL}".to_string());
        cfg.cwd = Some(std::path::PathBuf::from("${MIMOFAN_SRV_HOME}/work"));
        cfg.headers
            .insert("Authorization".into(), "Bearer ${MIMOFAN_SRV_TOKEN}".into());
        cfg.oauth_resource = Some("${MIMOFAN_SRV_URL}/resource".to_string());
        cfg.scopes = vec!["${MIMOFAN_SRV_TOKEN:-default_scope}".into()];

        let expanded = cfg.expand_env_vars();
        assert_eq!(expanded.command.as_deref(), Some("/opt/srv/bin/run"));
        assert_eq!(expanded.args, vec!["--token", "tok"]);
        assert_eq!(expanded.env.get("API_KEY").map(String::as_str), Some("tok"));
        assert_eq!(expanded.url.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(
            expanded.cwd.as_deref(),
            Some(std::path::Path::new("/opt/srv/work"))
        );
        assert_eq!(
            expanded.headers.get("Authorization").map(String::as_str),
            Some("Bearer tok")
        );
        assert_eq!(
            expanded.oauth_resource.as_deref(),
            Some("https://example.com/mcp/resource")
        );
        assert_eq!(expanded.scopes, vec!["tok"]);
        // Original is unchanged (clone semantics).
        assert_eq!(cfg.url.as_deref(), Some("${MIMOFAN_SRV_URL}"));
        remove_var("MIMOFAN_SRV_TOKEN");
        remove_var("MIMOFAN_SRV_URL");
        remove_var("MIMOFAN_SRV_HOME");
    }

    #[test]
    fn expand_is_idempotent_on_constant_config() {
        let mut cfg = test_config();
        cfg.command = Some("fixed-cmd".to_string());
        cfg.url = Some("https://fixed.example.com/mcp".to_string());
        cfg.headers.insert("X".into(), "plain".into());
        let once = cfg.expand_env_vars();
        let twice = once.expand_env_vars();
        assert_eq!(once, twice);
    }
}
