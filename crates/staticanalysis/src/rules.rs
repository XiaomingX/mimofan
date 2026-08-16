//! Declarative vulnerability rule schema + a minimal, dependency-free YAML
//! loader.
//!
//! Rules are expressed as data (YAML), not Rust code, so they can be edited
//! and **hot-reloaded** at runtime without recompiling the analyzer (T-4/T-12
//! acceptance: "must be YAML extensible + hot-updatable; do NOT hardcode
//! rules into Rust").
//!
//! The staticanalysis crate intentionally does *not* depend on `serde_yaml`
//! (its `Cargo.toml` is owned by the parallel staticanalysis group and must
//! not be touched), so this module ships a small, focused YAML *subset*
//! parser sufficient for the rule format below. The subset supports:
//!
//! - block mappings (`key: value`) with indentation-based nesting,
//! - block sequences (`- item`) and inline sequences (`[a, b]`),
//! - scalars: double/single quoted strings, bare strings, integers,
//!   booleans (`true`/`false`), and `null`,
//! - `#` comments (whole-line and trailing),
//! - block scalars are *not* supported (rules avoid them).
//!
//! The parser returns a [`Yaml`] tree which converts losslessly into a
//! `serde_json::Value`, so the rest of the engine can use familiar `Value`
//! access while keeping the loader dependency-free.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

/// A YAML node produced by the minimal parser.
#[derive(Debug, Clone, PartialEq)]
pub enum Yaml {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Seq(Vec<Yaml>),
    Map(BTreeMap<String, Yaml>),
}

impl Yaml {
    /// Convert into a `serde_json::Value` for downstream convenience.
    pub fn into_json(self) -> serde_json::Value {
        match self {
            Yaml::Null => serde_json::Value::Null,
            Yaml::Bool(b) => serde_json::Value::Bool(b),
            Yaml::Int(i) => serde_json::Value::Number(i.into()),
            Yaml::Str(s) => serde_json::Value::String(s),
            Yaml::Seq(items) => {
                serde_json::Value::Array(items.into_iter().map(Yaml::into_json).collect())
            }
            Yaml::Map(m) => {
                let obj = m
                    .into_iter()
                    .map(|(k, v)| (k, v.into_json()))
                    .collect::<serde_json::Map<_, _>>();
                serde_json::Value::Object(obj)
            }
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Yaml::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&BTreeMap<String, Yaml>> {
        match self {
            Yaml::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<&[Yaml]> {
        match self {
            Yaml::Seq(s) => Some(s),
            _ => None,
        }
    }

    /// Get a required string field, with a readable error.
    pub fn get_str(&self, key: &str) -> Result<&str> {
        self.as_map()
            .and_then(|m| m.get(key))
            .and_then(Yaml::as_str)
            .with_context(|| format!("rule field `{key}` missing or not a string"))
    }
}

/// Parse a YAML document (subset) into a [`Yaml`] tree.
///
/// `file` is only used for error context.
pub fn parse_yaml(file: &str, text: &str) -> Result<Yaml> {
    let lines = tokenize_lines(text);
    let root = parse_block(file, &lines, 0, 0)?;
    Ok(root)
}

/// A preprocessed, non-empty YAML line: its indentation and trimmed content.
#[derive(Clone)]
struct Line {
    indent: usize,
    content: String,
}

/// Strip comments and blank lines, recording indentation of significant content.
fn tokenize_lines(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let (content, _) = strip_comment(raw);
        let trimmed = content.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        out.push(Line {
            indent,
            // `content` is fully trimmed; `indent` carries the original
            // indentation so block nesting is computed from `indent`, not from
            // leading spaces in `content` (otherwise `starts_with("- ")` checks
            // on indented sequence items would misfire).
            content: trimmed.trim().to_string(),
        });
    }
    out
}

/// Remove a trailing `#` comment that is not inside quotes. Whole-line comments
/// (leading `#`) collapse to empty.
fn strip_comment(line: &str) -> (String, bool) {
    let mut in_single = false;
    let mut in_double = false;
    let mut result = String::new();
    let chars = line.chars().peekable();
    for c in chars {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                result.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                result.push(c);
            }
            '#' if !in_single && !in_double => {
                // Comment starts here; keep leading whitespace for indent calc.
                return (result, true);
            }
            _ => result.push(c),
        }
    }
    (result, false)
}

/// Parse a block of `lines` starting at `idx`, all belonging to indentation
/// `min_indent` or deeper. Returns the parsed value and the next index to
/// process in the parent.
fn parse_block(file: &str, lines: &[Line], mut idx: usize, min_indent: usize) -> Result<Yaml> {
    // Determine whether this block is a sequence (first significant line is a
    // `- ` item) or a mapping.
    if idx >= lines.len() {
        return Ok(Yaml::Null);
    }
    let first = &lines[idx];
    if first.content.starts_with("- ") || first.content == "-" {
        return parse_seq(file, lines, idx, first.indent);
    }
    // Otherwise a mapping.
    let mut map = BTreeMap::new();
    while idx < lines.len() {
        let line = &lines[idx];
        if line.indent < min_indent {
            break;
        }
        if line.indent > min_indent {
            // Shouldn't happen at mapping top-level; skip defensively.
            idx += 1;
            continue;
        }
        let (key, inline_val) = split_mapping_key(&line.content)
            .with_context(|| format!("{}: malformed mapping line `{}`", file, line.content))?;
        if let Some(v) = inline_val {
            // `key: value` on one line.
            map.insert(key, parse_scalar_or_inline(&v));
            idx += 1;
        } else {
            // `key:` followed by a nested block (deeper indent).
            map.insert(key.clone(), Yaml::Null); // placeholder
            // Consume nested lines.
            let child_indent = line.indent + 1;
            let mut child_idx = idx + 1;
            let mut nested = Vec::new();
            while child_idx < lines.len() && lines[child_idx].indent >= child_indent {
                nested.push(lines[child_idx].clone());
                child_idx += 1;
            }
            if nested.is_empty() {
                // Empty value -> null.
                idx += 1;
            } else {
                // The nested block may itself start with a `- ` (seq) at the
                // deeper indent, or be a mapping at that indent.
                let nested_block = parse_block(file, &nested, 0, nested[0].indent)?;
                map.insert(key, nested_block);
                idx = child_idx;
            }
        }
    }
    Ok(Yaml::Map(map))
}

/// Split `key: value` into `(key, Some(value))` or `(key, None)` when the value
/// is on the next (deeper) line.
fn split_mapping_key(content: &str) -> Result<(String, Option<String>)> {
    // Find the first ':' that is not inside quotes and not part of `://`.
    let mut in_single = false;
    let mut in_double = false;
    let chars = content.char_indices().peekable();
    for (i, c) in chars {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ':' if !in_single && !in_double => {
                // Peek next char: if '//' it's a URL scheme, skip.
                let rest = &content[i + 1..];
                if rest.starts_with("//") {
                    continue;
                }
                let key = content[..i].trim().to_string();
                let val = content[i + 1..].trim().to_string();
                return Ok((key, if val.is_empty() { None } else { Some(val) }));
            }
            _ => {}
        }
    }
    bail!("expected `key: value` mapping, got `{}`", content)
}

/// Parse a block sequence beginning at `lines[idx]` (a `- ...` item) with the
/// given `seq_indent`.
fn parse_seq(file: &str, lines: &[Line], mut idx: usize, seq_indent: usize) -> Result<Yaml> {
    let mut seq = Vec::new();
    while idx < lines.len() {
        let line = &lines[idx];
        if line.indent < seq_indent {
            break;
        }
        if line.indent != seq_indent {
            idx += 1;
            continue;
        }
        if !(line.content.starts_with("- ") || line.content == "-") {
            break;
        }
        let after_dash = if line.content == "-" {
            String::new()
        } else {
            line.content[2..].trim().to_string()
        };
        if after_dash.is_empty() {
            // Nested block under the dash.
            let child_indent = seq_indent + 1;
            let mut child_idx = idx + 1;
            let mut nested = Vec::new();
            while child_idx < lines.len() && lines[child_idx].indent >= child_indent {
                nested.push(lines[child_idx].clone());
                child_idx += 1;
            }
            if nested.is_empty() {
                seq.push(Yaml::Null);
                idx += 1;
            } else {
                seq.push(parse_block(file, &nested, 0, nested[0].indent)?);
                idx = child_idx;
            }
        } else if let Some((k, inline)) = try_split_inline_mapping(&after_dash) {
            // A sequence item that is itself a mapping, e.g.
            //   - name: foo
            //     sink: bar
            // The first key is on this line; subsequent keys are indented to
            // align with `name` (seq_indent + 2).
            let mut map = BTreeMap::new();
            map.insert(k, parse_scalar_or_inline(&inline.unwrap_or_default()));
            // Gather continuation lines (indent == seq_indent + 2).
            let cont_indent = seq_indent + 2;
            let mut child_idx = idx + 1;
            let mut nested = Vec::new();
            while child_idx < lines.len() && lines[child_idx].indent >= cont_indent {
                nested.push(lines[child_idx].clone());
                child_idx += 1;
            }
            if !nested.is_empty() {
                let sub = parse_block(file, &nested, 0, nested[0].indent)?;
                if let Yaml::Map(m) = sub {
                    for (kk, vv) in m {
                        map.insert(kk, vv);
                    }
                }
            }
            seq.push(Yaml::Map(map));
            idx = child_idx;
        } else {
            seq.push(parse_scalar_or_inline(&after_dash));
            idx += 1;
        }
    }
    Ok(Yaml::Seq(seq))
}

/// If `content` looks like `key: value` (single pair, inline) return it.
fn try_split_inline_mapping(content: &str) -> Option<(String, Option<String>)> {
    let (k, v) = split_mapping_key(content).ok()?;
    Some((k, v))
}

/// Parse a scalar (could be an inline `[a, b]` sequence).
fn parse_scalar_or_inline(s: &str) -> Yaml {
    let t = s.trim();
    if t.starts_with('[') && t.ends_with(']') {
        let inner = &t[1..t.len() - 1];
        if inner.trim().is_empty() {
            return Yaml::Seq(vec![]);
        }
        let items = inner
            .split(',')
            .map(|x| parse_scalar_or_inline(x.trim()))
            .collect();
        return Yaml::Seq(items);
    }
    if t.starts_with('{') && t.ends_with('}') {
        // Inline mapping (rare in rules); parse simply as string fallback.
        return Yaml::Str(t.to_string());
    }
    parse_scalar(t)
}

/// Parse a single scalar token.
fn parse_scalar(t: &str) -> Yaml {
    let t = t.trim();
    if t.is_empty() {
        return Yaml::Null;
    }
    if t == "null" || t == "~" || t == "Null" || t == "NULL" {
        return Yaml::Null;
    }
    if t == "true" || t == "True" || t == "TRUE" {
        return Yaml::Bool(true);
    }
    if t == "false" || t == "False" || t == "FALSE" {
        return Yaml::Bool(false);
    }
    if (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
        || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
    {
        return Yaml::Str(unquote(t));
    }
    if let Ok(i) = t.parse::<i64>() {
        return Yaml::Int(i);
    }
    Yaml::Str(t.to_string())
}

fn unquote(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        // Minimal unescape for common cases.
        inner
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"")
    } else {
        s.to_string()
    }
}

// ==========================================================================
// Rule schema
// ==========================================================================

/// Where a taint source/sink/propagator lives and how to match it.
///
/// Matching is by fully-qualified *symbol* name (last path segment or full
/// path) plus an optional argument/return selector. This keeps the engine
/// grammar-agnostic: concrete AST extraction is done by the caller using
/// `staticanalysis::query_source`, and only symbol names flow into the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSpec {
    /// Symbol name to match (e.g. `getParameter`, `InitialContext.lookup`).
    /// Matched as a suffix (last path segment) so `javax.naming.InitialContext.lookup`
    /// matches `lookup`.
    pub symbol: String,
    /// Optional: only the Nth argument (0-based) carries/consumes taint.
    pub arg: Option<usize>,
    /// Optional: the return value carries taint (for sources/propagators).
    pub ret: bool,
}

impl SymbolSpec {
    /// Parse a spec from a YAML scalar or mapping.
    ///
    /// Accepts either a bare string `"foo"` or a mapping
    /// `{ symbol: foo, arg: 0, ret: true }`.
    pub fn from_yaml(y: &Yaml) -> Result<SymbolSpec> {
        match y {
            Yaml::Str(s) => Ok(SymbolSpec {
                symbol: s.clone(),
                arg: None,
                ret: false,
            }),
            Yaml::Map(m) => {
                let symbol = m
                    .get("symbol")
                    .and_then(Yaml::as_str)
                    .context("SymbolSpec mapping requires `symbol`")?
                    .to_string();
                let arg = m.get("arg").and_then(|v| match v {
                    Yaml::Int(i) => Some(*i as usize),
                    _ => None,
                });
                let ret = matches!(m.get("ret"), Some(Yaml::Bool(true)));
                Ok(SymbolSpec { symbol, arg, ret })
            }
            _ => bail!("SymbolSpec must be a string or mapping"),
        }
    }

    /// Does this spec match a concrete call symbol + argument position?
    pub fn matches(&self, call_symbol: &str, arg_pos: Option<usize>) -> bool {
        let suffix_matches = call_symbol.ends_with(&self.symbol)
            || call_symbol == self.symbol
            || call_symbol.ends_with(&format!(".{}", self.symbol));
        if !suffix_matches {
            return false;
        }
        match (self.arg, arg_pos) {
            (Some(required), Some(got)) => required == got,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }
}

/// A taint *source*: introduces attacker-controlled data.
#[derive(Debug, Clone)]
pub struct SourceRule {
    pub id: String,
    pub language: String,
    pub symbol: SymbolSpec,
    pub category: String,
    pub cwe: Vec<String>,
}

impl SourceRule {
    /// The vulnerability classes this source can realize. We use the CWE ids
    /// directly as class tags so sinks/partial sanitizers can match them.
    pub fn cwe_to_classes(&self) -> Vec<String> {
        self.cwe.clone()
    }
}

/// A taint *sink*: where tainted data causes damage.
#[derive(Debug, Clone)]
pub struct SinkRule {
    pub id: String,
    pub language: String,
    pub symbol: SymbolSpec,
    pub category: String,
    pub cwe: Vec<String>,
    /// Minimum confidence when this sink is reached by taint.
    pub severity: String,
}

/// A *sanitizer*: partially or fully neutralizes taint.
///
/// `neutralizes` lists the vulnerability classes it defeats (e.g. `[xss]`).
/// An **empty** list means it is a *strong* (full) sanitizer that clears all
/// taint. A *non-empty* list means **partial** sanitization: taint tagged with
/// a class NOT in the list survives (T-4 acceptance: "partial sanitization via
/// `neutralizes: [xss]` rather than a boolean").
#[derive(Debug, Clone)]
pub struct SanitizerRule {
    pub id: String,
    pub language: String,
    pub symbol: SymbolSpec,
    pub neutralizes: Vec<String>,
}

/// A *propagator*: passes taint from one argument to the return value / another
/// argument (e.g. `String.concat`, `StringBuilder.append`).
#[derive(Debug, Clone)]
pub struct PropagatorRule {
    pub id: String,
    pub language: String,
    pub symbol: SymbolSpec,
    /// Argument index whose taint flows to the return value.
    pub from_arg: Option<usize>,
    /// If true, taint also flows into the receiver (builder pattern).
    pub to_receiver: bool,
}

/// A full rule set loaded from one or more YAML documents.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    pub sources: Vec<SourceRule>,
    pub sinks: Vec<SinkRule>,
    pub sanitizers: Vec<SanitizerRule>,
    pub propagators: Vec<PropagatorRule>,
}

impl RuleSet {
    /// Parse a single YAML rule document into this set (appending).
    pub fn extend_from_yaml(&mut self, file: &str, text: &str) -> Result<()> {
        let doc = parse_yaml(file, text)?;
        let map = doc.as_map().context("rule document must be a mapping")?;

        if let Some(Yaml::Seq(items)) = map.get("sources") {
            for it in items {
                self.sources.push(parse_source(it)?);
            }
        }
        if let Some(Yaml::Seq(items)) = map.get("sinks") {
            for it in items {
                self.sinks.push(parse_sink(it)?);
            }
        }
        if let Some(Yaml::Seq(items)) = map.get("sanitizers") {
            for it in items {
                self.sanitizers.push(parse_sanitizer(it)?);
            }
        }
        if let Some(Yaml::Seq(items)) = map.get("propagators") {
            for it in items {
                self.propagators.push(parse_propagator(it)?);
            }
        }
        Ok(())
    }

    /// Merge another rule set into this one.
    pub fn merge(&mut self, other: RuleSet) {
        self.sources.extend(other.sources);
        self.sinks.extend(other.sinks);
        self.sanitizers.extend(other.sanitizers);
        self.propagators.extend(other.propagators);
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
            && self.sinks.is_empty()
            && self.sanitizers.is_empty()
            && self.propagators.is_empty()
    }
}

/// Load every `*.yaml` rule file from a directory into one [`RuleSet`].
pub fn load_rules_dir(dir: &str) -> Result<RuleSet> {
    let mut set = RuleSet::default();
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading rules dir {dir}"))?;
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|x| x == "yaml" || x == "yml")
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    for path in files {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        set.extend_from_yaml(&name, &text)?;
    }
    Ok(set)
}

fn parse_source(y: &Yaml) -> Result<SourceRule> {
    let m = y.as_map().context("source must be a mapping")?;
    Ok(SourceRule {
        id: m.get("id").and_then(Yaml::as_str).unwrap_or("").to_string(),
        language: m
            .get("language")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string(),
        symbol: SymbolSpec::from_yaml(m.get("symbol").context("source.symbol")?)?,
        category: m
            .get("category")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string(),
        cwe: string_list(m.get("cwe")),
    })
}

fn parse_sink(y: &Yaml) -> Result<SinkRule> {
    let m = y.as_map().context("sink must be a mapping")?;
    Ok(SinkRule {
        id: m.get("id").and_then(Yaml::as_str).unwrap_or("").to_string(),
        language: m
            .get("language")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string(),
        symbol: SymbolSpec::from_yaml(m.get("symbol").context("sink.symbol")?)?,
        category: m
            .get("category")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string(),
        cwe: string_list(m.get("cwe")),
        severity: m
            .get("severity")
            .and_then(Yaml::as_str)
            .unwrap_or("error")
            .to_string(),
    })
}

fn parse_sanitizer(y: &Yaml) -> Result<SanitizerRule> {
    let m = y.as_map().context("sanitizer must be a mapping")?;
    Ok(SanitizerRule {
        id: m.get("id").and_then(Yaml::as_str).unwrap_or("").to_string(),
        language: m
            .get("language")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string(),
        symbol: SymbolSpec::from_yaml(m.get("symbol").context("sanitizer.symbol")?)?,
        neutralizes: string_list(m.get("neutralizes")),
    })
}

fn parse_propagator(y: &Yaml) -> Result<PropagatorRule> {
    let m = y.as_map().context("propagator must be a mapping")?;
    Ok(PropagatorRule {
        id: m.get("id").and_then(Yaml::as_str).unwrap_or("").to_string(),
        language: m
            .get("language")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string(),
        symbol: SymbolSpec::from_yaml(m.get("symbol").context("propagator.symbol")?)?,
        from_arg: m.get("from_arg").and_then(|v| match v {
            Yaml::Int(i) => Some(*i as usize),
            _ => None,
        }),
        to_receiver: matches!(m.get("to_receiver"), Some(Yaml::Bool(true))),
    })
}

fn string_list(y: Option<&Yaml>) -> Vec<String> {
    match y {
        Some(Yaml::Seq(items)) => items
            .iter()
            .filter_map(Yaml::as_str)
            .map(|s| s.to_string())
            .collect(),
        Some(Yaml::Str(s)) => vec![s.clone()],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
sources:
  - id: java-servlet-request
    language: java
    symbol: getParameter
    category: untrusted-input
    cwe: [CWE-20, CWE-79]
sinks:
  - id: java-jndi-lookup
    language: java
    symbol: InitialContext.lookup
    category: jndi-injection
    cwe: [CWE-74]
    severity: error
sanitizers:
  - id: java-esapi-encode
    language: java
    symbol: ESAPI.encoder.encodeForHTML
    neutralizes: [xss]
propagators:
  - id: java-string-concat
    language: java
    symbol: concat
    from_arg: 0
    to_receiver: false
"#;

    #[test]
    fn parses_rule_document() {
        let mut set = RuleSet::default();
        set.extend_from_yaml("sample.yaml", SAMPLE).unwrap();
        assert_eq!(set.sources.len(), 1);
        assert_eq!(set.sinks.len(), 1);
        assert_eq!(set.sanitizers.len(), 1);
        assert_eq!(set.propagators.len(), 1);

        let src = &set.sources[0];
        assert_eq!(src.id, "java-servlet-request");
        assert_eq!(src.cwe, vec!["CWE-20", "CWE-79"]);

        let san = &set.sanitizers[0];
        assert_eq!(san.neutralizes, vec!["xss"]); // partial sanitizer

        let prop = &set.propagators[0];
        assert_eq!(prop.from_arg, Some(0));
        assert!(!prop.to_receiver);
    }

    #[test]
    fn symbol_suffix_matching() {
        let spec = SymbolSpec {
            symbol: "lookup".into(),
            arg: None,
            ret: false,
        };
        assert!(spec.matches("javax.naming.InitialContext.lookup", None));
        assert!(spec.matches("InitialContext.lookup", None));
        assert!(!spec.matches("foo.look", None));

        let arg_spec = SymbolSpec {
            symbol: "getParameter".into(),
            arg: Some(0),
            ret: false,
        };
        assert!(arg_spec.matches("getParameter", Some(0)));
        assert!(!arg_spec.matches("getParameter", Some(1)));
    }

    #[test]
    fn loads_real_rule_files_from_disk() {
        // Prove the shipped YAML rule files are not empty shells: they must
        // parse and yield actual rules. Path is relative to the crate root.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rules");
        let set = load_rules_dir(dir).expect("load rules dir");
        assert!(
            !set.sources.is_empty(),
            "expected sources from on-disk rules"
        );
        assert!(!set.sinks.is_empty(), "expected sinks from on-disk rules");
    }

    #[test]
    fn yaml_subset_handles_nested_and_inline() {
        let y = parse_yaml(
            "t.yaml",
            r#"
a: 1
b: hello
c:
  - x
  - y
d: [p, q, r]
e:
  f: nested
  g: 2
"#,
        )
        .unwrap();
        let j = y.into_json();
        assert_eq!(j["a"], serde_json::json!(1));
        assert_eq!(j["b"], serde_json::json!("hello"));
        assert_eq!(j["c"], serde_json::json!(["x", "y"]));
        assert_eq!(j["d"], serde_json::json!(["p", "q", "r"]));
        assert_eq!(j["e"]["f"], serde_json::json!("nested"));
        assert_eq!(j["e"]["g"], serde_json::json!(2));
    }
}
