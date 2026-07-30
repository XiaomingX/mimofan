//! Command matching helpers for execpolicy rules.

use regex::Regex;

/// Normalize a command string by shlex parsing and re-joining tokens.
///
/// Strips heredoc bodies first (#419) so a command like
/// `cat <<EOF > file.txt\nbody\nEOF` collapses to `cat > file.txt`
/// before pattern matching. Without this, an `auto_allow` pattern
/// of `cat > file.txt` would fail to match because shlex would
/// tokenize the body lines into the command.
pub fn normalize_command(command: &str) -> String {
    let stripped = strip_heredoc_bodies(command);
    if let Some(tokens) = shlex::split(&stripped) {
        tokens.join(" ")
    } else {
        stripped
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Strip heredoc bodies from a multi-line command string.
///
/// Recognises the common forms:
///
/// * `<<DELIM` — body until line equal to `DELIM`.
/// * `<<-DELIM` — body until line equal to `DELIM` (tabs stripped
///   in real shell; we keep the delimiter match the same).
/// * `<<'DELIM'` / `<<"DELIM"` — quoted delimiter; quotes peeled
///   for the closing match.
///
/// The here-string operator `<<<` is intentionally not stripped —
/// its body is the next token on the same line, not separate lines,
/// and shlex tokenizes it correctly.
fn strip_heredoc_bodies(command: &str) -> String {
    if !command.contains("<<") {
        return command.to_string();
    }
    // Sidestep the here-string operator (`<<<`) by replacing it
    // with a placeholder before running the heredoc regex, then
    // restoring it after. Rust's `regex` crate doesn't support
    // lookbehind, so we can't write "match `<<` only when not
    // preceded by `<`" directly; this preprocessing achieves the
    // same outcome.
    const HERESTRING_PLACEHOLDER: &str = "\u{0001}HERESTRING\u{0001}";
    let command_owned: String = command.replace("<<<", HERESTRING_PLACEHOLDER);
    let command: &str = &command_owned;

    // Lazy-init the heredoc-start regex. Allows whitespace / `-`
    // between `<<` and the delimiter, accepts optional `'` / `"`
    // around the delimiter name. The delimiter is a typical
    // shell identifier (alphanumeric + underscore).
    static HEREDOC_RE_INIT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = HEREDOC_RE_INIT.get_or_init(|| {
        Regex::new(r#"<<-?\s*(?:['"]?)([A-Za-z_][A-Za-z0-9_]*)(?:['"]?)"#)
            .expect("heredoc regex compiles")
    });

    let mut out = String::with_capacity(command.len());
    let mut lines = command.lines();
    while let Some(line) = lines.next() {
        // Detect heredoc on this line, capture the delimiter, and
        // strip the `<<DELIM` operator from the line so downstream
        // tokenizers don't see it in the pattern. A single line can
        // have multiple heredocs (rare but legal: `cmd <<A <<B`);
        // we strip every match on the line and consume until the
        // *last* delimiter (the matching shell behavior is to stack
        // them, but for pattern-match purposes they all collapse).
        let mut delim: Option<String> = None;
        let mut redacted = line.to_string();
        for cap in re.captures_iter(line) {
            // Strip the entire `<<DELIM` text from the line.
            let whole = cap.get(0).map_or("", |m| m.as_str());
            redacted = redacted.replace(whole, "");
            // Track the last-seen delimiter for body consumption.
            delim = cap.get(1).map(|m| m.as_str().to_string());
        }
        // Trim any double-spaces left after stripping.
        let cleaned = redacted
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&cleaned);
        out.push('\n');
        if let Some(d) = delim {
            // Skip body lines until we hit the matching delimiter.
            for body_line in lines.by_ref() {
                if body_line.trim() == d {
                    break;
                }
            }
        }
    }
    // Restore the here-string operator we hid before regex matching.
    out.replace(HERESTRING_PLACEHOLDER, "<<<")
}

/// Return true if the pattern matches the command.
///
/// Patterns support `*` wildcards that match any substring. The command is
/// first reduced to its canonical executable form (stripping wrappers like
/// `sudo` / `command` / `env FOO=`, and replacing the executable with its
/// basename) so a `deny = ["rm *"]` rule also blocks `/bin/rm -rf /` or
/// `sudo rm -rf /` without the caller having to canonicalise first.
pub fn pattern_matches(pattern: &str, command: &str) -> bool {
    let pattern = normalize_command(pattern);
    let command = canonical_executable_form(&normalize_command(command));

    if pattern == "*" {
        return true;
    }

    let escaped = regex::escape(&pattern).replace("\\*", ".*");
    let Ok(re) = Regex::new(&format!("^{escaped}$")) else {
        return false;
    };
    re.is_match(&command)
}

/// Reduce a command to a canonical executable form for deny-rule matching:
/// strip common wrapper prefixes (`sudo`, `command`, `env VAR=`, …) and replace
/// the executable with its filesystem basename, so a `deny = ["rm *"]` rule also
/// blocks `/bin/rm -rf /` or `sudo rm -rf /`.
///
/// Case-preserving, matching the convention of [`normalize_command`] in this
/// module. `bash -c "rm -rf /"` is intentionally *not* flattened — parsing the
/// `-c` argument would risk mis-classifying unrelated commands.
pub fn canonical_executable_form(command: &str) -> String {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let mut idx = 0;
    while idx < tokens.len() {
        let t = tokens[idx];
        if matches!(
            t,
            "command" | "sudo" | "time" | "nohup" | "doas" | "setsid" | "env"
        ) {
            idx += 1;
            continue;
        }
        if t.contains('=') && !t.starts_with('-') {
            idx += 1;
            continue;
        }
        break;
    }
    let positional: &[&str] = &tokens[idx..];
    if positional.is_empty() {
        return command.to_string();
    }
    let first = std::path::Path::new(positional[0])
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(positional[0]);
    let mut out: Vec<&str> = Vec::with_capacity(positional.len());
    out.push(first);
    out.extend_from_slice(&positional[1..]);
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_bypass_closed_by_canonical_form() {
        // `rm *` must match the path/wrapper forms via the canonical variant.
        assert!(pattern_matches("rm *", "/bin/rm -rf /"));
        assert!(pattern_matches("rm *", "sudo rm -rf /"));
        // Bare `rm` (no wildcard) still requires exact match of the executable.
        assert!(!pattern_matches("rm", "rm -rf /"));
    }
}
