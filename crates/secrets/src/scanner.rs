//! Content-level secret scanning and redaction primitives.
//!
//! These helpers operate on *content* (free text, tool output, memory
//! observations) rather than on stored credentials. They implement a small,
//! dependency-free subset of the gitleaks rule families so callers can:
//!
//! * block writes that would persist a secret ([`is_sensitive_content`]), and
//! * strip secrets from streamed tool output on the fly ([`redact_stream`]).
//!
//! The patterns are intentionally conservative (high-precision, lower-recall):
//! we would rather let a borderline value through than corrupt legitimate
//! text. This is defence-in-depth — the authoritative secret store still
//! encrypts/permissions its backend separately.

use std::path::{Component, Path, PathBuf};

/// A category of secret the scanner recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// Generic `api_key`/`apikey`/`token`/`secret` assignment with a
    /// sufficiently long alphanumeric value.
    GenericCredential,
    /// AWS access key id (`AKIA...`) plus the matching secret key pattern.
    AwsKey,
    /// Private key PEM block header (`-----BEGIN ... PRIVATE KEY-----`).
    PrivateKey,
    /// GitHub personal access / OAuth token (`ghp_`, `gho_`, ...).
    GithubToken,
    /// Google API key (`AIza...`).
    GoogleApiKey,
    /// Slack token (`xox[baprs]-...`).
    SlackToken,
    /// OpenSSH / PuTTY private key file markers.
    SshKey,
    /// JSON web token (`eyJ...`).
    Jwt,
}

impl SecretKind {
    /// Stable, human-readable label used in diagnostics and redaction tags.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SecretKind::GenericCredential => "generic-credential",
            SecretKind::AwsKey => "aws-key",
            SecretKind::PrivateKey => "private-key",
            SecretKind::GithubToken => "github-token",
            SecretKind::GoogleApiKey => "google-api-key",
            SecretKind::SlackToken => "slack-token",
            SecretKind::SshKey => "ssh-key",
            SecretKind::Jwt => "jwt",
        }
    }
}

/// One secret occurrence found inside a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretMatch {
    /// Which rule family matched.
    pub kind: SecretKind,
    /// Inclusive start byte offset of the match within the input string.
    pub start: usize,
    /// Exclusive end byte offset of the match within the input string.
    pub end: usize,
}

impl SecretMatch {
    /// The matched substring (for callers that need to report or redact it).
    #[must_use]
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        source.get(self.start..self.end).unwrap_or("")
    }
}

/// Minimum length for a generic credential value to avoid flagging short
/// placeholder strings like `token = "x"`.
const MIN_GENERIC_LEN: usize = 16;

/// A line is scanned as `key = value` or `key: value`; these prefixes signal
/// a credential-bearing key.
const CREDENTIAL_KEYWORDS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "access_token",
    "accesstoken",
    "secret",
    "token",
    "password",
    "passwd",
    "private_key",
    "client_secret",
    "auth",
];

/// Scan a single line for secrets. Returns every match found (may be empty).
///
/// `line` should already be a single newline-terminated-free chunk; multi-line
/// scans should call this per line.
#[must_use]
pub fn scan_line(line: &str) -> Vec<SecretMatch> {
    let mut matches = Vec::new();

    // 1. PEM / key-block headers — whole-line structural markers.
    if line.contains("-----BEGIN") && line.contains("PRIVATE KEY-----") {
        // The header itself is the marker; approximate the span to the line.
        if let Some(start) = line.find("-----BEGIN") {
            if let Some(end) = line[start..].find("-----") {
                matches.push(SecretMatch {
                    kind: SecretKind::PrivateKey,
                    start,
                    end: start + end + 5,
                });
            }
        }
    }

    // 2. SSH / PuTTY key file markers.
    if line.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
        || line.starts_with("PuTTY-User-Key-File-")
    {
        matches.push(SecretMatch {
            kind: SecretKind::SshKey,
            start: 0,
            end: line.len(),
        });
    }

    // 3. AWS access key id.
    if let Some(pos) = line.find("AKIA") {
        let tail = &line[pos..];
        let token_len = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .count();
        if (20..=24).contains(&token_len) {
            matches.push(SecretMatch {
                kind: SecretKind::AwsKey,
                start: pos,
                end: pos + token_len,
            });
        }
    }

    // 4. Provider-specific token prefixes.
    for (prefix, kind) in [
        ("ghp_", SecretKind::GithubToken),
        ("gho_", SecretKind::GithubToken),
        ("ghu_", SecretKind::GithubToken),
        ("ghs_", SecretKind::GithubToken),
        ("ghr_", SecretKind::GithubToken),
        ("AIza", SecretKind::GoogleApiKey),
        ("xoxb-", SecretKind::SlackToken),
        ("xoxp-", SecretKind::SlackToken),
        ("xoxa-", SecretKind::SlackToken),
        ("xoxr-", SecretKind::SlackToken),
    ] {
        if let Some(pos) = line.find(prefix) {
            let tail = &line[pos..];
            let token_len = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count();
            if token_len >= prefix.len() + 8 {
                matches.push(SecretMatch {
                    kind,
                    start: pos,
                    end: pos + token_len,
                });
            }
        }
    }

    // 5. JWT — three dot-separated base64url segments, first starts with `eyJ`.
    if let Some(pos) = line.find("eyJ") {
        let tail = &line[pos..];
        let token_len = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
            .count();
        let segment_count = tail[..token_len].matches('.').count();
        if segment_count == 2 && token_len >= 32 {
            matches.push(SecretMatch {
                kind: SecretKind::Jwt,
                start: pos,
                end: pos + token_len,
            });
        }
    }

    // 6. Generic `key = value` assignments with a long value.
    if let Some(m) = scan_generic_assignment(line) {
        matches.push(m);
    }

    matches
}

/// Detect `key = value` / `key: value` style assignments whose key looks
/// credential-bearing and whose value is long enough to be a real secret.
#[must_use]
pub fn scan_generic_assignment(line: &str) -> Option<SecretMatch> {
    let lower = line.to_ascii_lowercase();
    let eq = lower.find('=').or_else(|| lower.find(':'))?;
    let key_part = lower[..eq].trim();
    let value_part = line[eq + 1..].trim();

    let is_cred_key = CREDENTIAL_KEYWORDS.iter().any(|kw| key_part.contains(kw));
    if !is_cred_key {
        return None;
    }

    // Strip surrounding quotes from the value.
    let value = value_part
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| {
            value_part
                .strip_prefix('\'')
                .and_then(|v| v.strip_suffix('\''))
        })
        .unwrap_or(value_part);

    // The value must be a single alphanumeric/url-safe token, not prose, and
    // long enough to be an actual secret rather than a flag name.
    let is_token = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/+=".contains(c))
        && value.len() >= MIN_GENERIC_LEN;
    if !is_token {
        return None;
    }

    // Locate the value span within the original line, accounting for the
    // optional surrounding quotes so the reported span excludes them and
    // `text()` returns just the bare credential value.
    let value_start = line[eq + 1..]
        .char_indices()
        .find(|(i, c)| !c.is_whitespace() && *c != '"' && *c != '\'')
        .map(|(i, _)| eq + 1 + i)
        .unwrap_or(eq + 1);
    let value_end = value_start + value.len();
    Some(SecretMatch {
        kind: SecretKind::GenericCredential,
        start: value_start,
        end: value_end,
    })
}

/// Returns `true` if the text contains any secret recognised by [`scan_line`].
///
/// Splits on newlines so multi-line PEM blocks and assignments are each
/// evaluated per line.
#[must_use]
pub fn is_sensitive_content(text: &str) -> bool {
    text.lines().any(|line| !scan_line(line).is_empty())
}

/// Redact every secret found in a single line, replacing each match with a
/// masked tag like `[REDACTED:aws-key]`.
///
/// Returns the redacted line. Offsets are processed right-to-left so earlier
/// replacements don't invalidate later span indices. Overlapping matches
/// (e.g. a generic `access_key` assignment that also matches the AWS-key
/// rule) are merged, keeping the leftmost classification.
#[must_use]
pub fn redact_line(line: &str) -> String {
    let mut matches = scan_line(line);
    if matches.is_empty() {
        return line.to_string();
    }
    // Merge overlapping matches so we redact each secret exactly once.
    matches.sort_by_key(|m| m.start);
    let mut merged: Vec<SecretMatch> = Vec::new();
    for m in matches {
        if let Some(last) = merged.last_mut() {
            if m.start <= last.end {
                // Overlap: extend the span, keep the earlier kind.
                last.end = last.end.max(m.end);
                continue;
            }
        }
        merged.push(m);
    }

    let mut out = String::with_capacity(line.len());
    let mut cursor = 0;
    for m in merged.iter().rev() {
        out.insert_str(0, &line[cursor..m.start]);
        out.insert_str(0, &format!("[REDACTED:{}]", m.kind.label()));
        cursor = m.end;
    }
    out.insert_str(0, &line[cursor..]);
    out
}

/// Stream redaction primitive.
///
/// Takes an arbitrary chunk of streamed text (which may split a line in the
/// middle) and returns the redacted text with secrets masked, plus the number
/// of secrets replaced in this chunk.
///
/// `carry` is an optional trailing fragment from the previous call that did
/// not end in a newline — pass it back so cross-chunk secrets (e.g. a long
/// `api_key = ...` value split across two writes) are still caught. The
/// returned `String` is the (possibly redacted) portion of `chunk` up to the
/// last newline; the leftover after the last newline is returned as the new
/// `carry` for the next call.
#[must_use]
pub fn redact_stream(chunk: &str, carry: Option<&str>) -> (String, usize, String) {
    let mut blocked = 0usize;
    let mut pending = String::new();
    if let Some(c) = carry {
        pending.push_str(c);
    }
    pending.push_str(chunk);

    // Split into completed lines (everything up to the final newline) and a
    // trailing carry fragment.
    let last_nl = pending.rfind('\n');
    let (completed, new_carry) = match last_nl {
        Some(pos) => {
            let split = pos + 1;
            (pending[..split].to_string(), pending[split..].to_string())
        }
        None => (String::new(), pending.clone()),
    };

    let mut out = String::with_capacity(completed.len());
    for line in completed.lines() {
        let redacted = redact_line(line);
        blocked += scan_line(line).len();
        out.push_str(&redacted);
        out.push('\n');
    }

    (out, blocked, new_carry)
}

/// Validate that a secrets-relative path does not escape the store directory.
///
/// Rejects absolute paths and any `..` component (path traversal). Returns the
/// normalised path on success, or an error string describing the rejection.
///
/// This backs `FileKeyringStore::new` so a caller cannot point the store at
/// `/etc/passwd` or `../../id_rsa` (#648).
pub fn sanitize_secrets_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Err(format!(
            "secret store path must be relative, got absolute path {}",
            path.display()
        ));
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(format!(
                    "secret store path must not contain '..' (path traversal): {}",
                    path.display()
                ));
            }
            Component::RootDir => {
                return Err(format!(
                    "secret store path must not contain a root component: {}",
                    path.display()
                ));
            }
            Component::CurDir => { /* skip '.' — redundant */ }
            other => out.push(other.as_os_str()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_key() {
        // Assembled at runtime: AWS's documented example key, never a
        // contiguous literal in source (avoids secret-scanning false-positives).
        let key = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let line = format!("aws_access_key_id = {key}");
        let m = scan_line(&line);
        assert!(!m.is_empty(), "expected AWS key match");
        assert_eq!(m[0].kind, SecretKind::AwsKey);
        assert_eq!(m[0].text(&line), key);
    }

    #[test]
    fn detects_github_token() {
        let body = format!("ghp_{}", "1234567890abcdef1234567890abcdef1234");
        let line = format!("token: {body}");
        let m = scan_line(&line);
        assert!(!m.is_empty());
        assert_eq!(m[0].kind, SecretKind::GithubToken);
        assert!(m[0].text(&line).starts_with("ghp_"));
    }

    #[test]
    fn detects_google_api_key() {
        let body = format!("AIza{}", "SyA1234567890abcdef1234567890abcdef");
        let line = format!("key={body}");
        let m = scan_line(&line);
        assert!(!m.is_empty());
        assert_eq!(m[0].kind, SecretKind::GoogleApiKey);
    }

    #[test]
    fn detects_jwt() {
        // Assembled from fragments so no `eyJ...` JWT-shaped substring is
        // contiguous in source (GitHub secret-scanning false-positive guard).
        let head = format!("{}{}", "ey", "JhbGciOiJIUzI1NiJ9");
        let mid = format!("{}{}{}", ".", "ey", "JzdWIiOiIxMjM0NTY3ODkwIn0");
        let tail = format!("{}{}", ".", "dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U");
        let body = format!("{head}{mid}{tail}");
        let line = format!("Authorization: Bearer {body}");
        let m = scan_line(&line);
        assert!(!m.is_empty(), "expected JWT match");
        assert_eq!(m[0].kind, SecretKind::Jwt);
    }

    #[test]
    fn detects_private_key_header() {
        let line = "-----BEGIN RSA PRIVATE KEY-----";
        let m = scan_line(line);
        assert!(!m.is_empty());
        assert_eq!(m[0].kind, SecretKind::PrivateKey);
    }

    #[test]
    fn detects_slack_token() {
        // Assemble the token at runtime so the literal never appears
        // contiguously in source (GitHub secret-scanning false-positive on the
        // `xoxb-` test fixture).
        let prefix = "xoxb-";
        let token = format!("{prefix}1234567890-1234567890-abcdefghijklmnop");
        let line = format!("SLACK_TOKEN={token}");
        let m = scan_line(&line);
        assert!(!m.is_empty());
        assert_eq!(m[0].kind, SecretKind::SlackToken);
    }

    #[test]
    fn detects_generic_assignment() {
        let line = "api_key = \"abcdef0123456789abcdef0123456789\"";
        let m = scan_generic_assignment(line).expect("expected generic match");
        assert_eq!(m.kind, SecretKind::GenericCredential);
        assert_eq!(m.text(line), "abcdef0123456789abcdef0123456789");
    }

    #[test]
    fn rejects_short_placeholder_values() {
        let line = "token = \"short\"";
        assert!(scan_generic_assignment(line).is_none());
    }

    #[test]
    fn ignores_non_credential_keys() {
        let line = "model = abcdef0123456789abcdef0123456789";
        assert!(scan_generic_assignment(line).is_none());
    }

    #[test]
    fn is_sensitive_content_multiline() {
        let text = "log line ok\npassword=supersecretvalue1234567890abcdef\nanother ok line";
        assert!(is_sensitive_content(text));
    }

    #[test]
    fn is_sensitive_content_clean() {
        let text = "just some normal log output\nno secrets here";
        assert!(!is_sensitive_content(text));
    }

    #[test]
    fn redact_line_masks_secret() {
        let key = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let line = format!("aws_access_key_id = {key}");
        let redacted = redact_line(&line);
        assert!(redacted.contains("[REDACTED:aws-key]"));
        assert!(!redacted.contains(&key));
    }

    #[test]
    fn redact_line_passthrough_clean() {
        let line = "this is a benign line";
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn redact_stream_splits_lines_and_counts() {
        let (out, blocked, carry) =
            redact_stream("ok line\napi_key=abcdef0123456789abcdef0123456789\n", None);
        assert!(out.contains("ok line"));
        assert!(out.contains("[REDACTED:generic-credential]"));
        assert_eq!(blocked, 1);
        // All lines terminated, so nothing should remain in the carry.
        assert!(carry.is_empty());
    }

    #[test]
    fn redact_stream_carry_completes_on_newline() {
        let (out1, b1, carry1) = redact_stream("api_key=abcdef0123456789abcdef0123456789", None);
        assert_eq!(b1, 0);
        assert!(!carry1.is_empty());
        let (out2, b2, carry2) = redact_stream("\nmore text\n", Some(&carry1));
        assert_eq!(b2, 1);
        assert!(out2.contains("[REDACTED:generic-credential]"));
        assert!(carry2.is_empty());
        let _ = out1;
    }

    #[test]
    fn redact_stream_partial_line_deferred() {
        // A partial line (no trailing newline) must not be redacted until a
        // newline arrives, and the carry must preserve it unchanged.
        let (out, blocked, carry) = redact_stream("api_key=abcdef0123456789abcdef0123456789", None);
        assert!(out.is_empty());
        assert_eq!(blocked, 0);
        assert_eq!(carry, "api_key=abcdef0123456789abcdef0123456789");
    }

    #[test]
    fn sanitize_path_rejects_traversal() {
        assert!(sanitize_secrets_path(Path::new("../escape.json")).is_err());
        assert!(sanitize_secrets_path(Path::new("a/../../b.json")).is_err());
        assert!(sanitize_secrets_path(Path::new("/abs/secrets.json")).is_err());
    }

    #[test]
    fn sanitize_path_accepts_relative() {
        let ok = sanitize_secrets_path(Path::new("secrets/secrets.json"));
        assert!(ok.is_ok());
        assert_eq!(ok.unwrap(), PathBuf::from("secrets/secrets.json"));
        // '.' components are collapsed.
        let dot = sanitize_secrets_path(Path::new("./secrets.json"));
        assert_eq!(dot.unwrap(), PathBuf::from("secrets.json"));
    }
}
