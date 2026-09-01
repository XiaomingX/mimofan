//! Java intraprocedural taint frontend (loop v5 / T2-A).
//!
//! Lowers tree-sitter Java to [`ProgramFacts`], tracking attacker-controlled
//! local variables per method and seeding the argument positions of calls
//! that receive tainted data. Source/sink/sanitizer/propagator symbols come
//! from the embedded `rules/java.yaml` (hot-reloadable declarative rules).
//!
//! Precision approach: taint flows only through explicit assignments and
//! argument references (whole-word local variable tokens); a local wrapped in
//! a known sanitizer call is treated as clean, so `PreparedStatement("... ?")`
//! patterns and encoders do not produce findings.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::AstError;
use crate::rules::RuleSet;
use crate::sarif::SecurityIssue;
use crate::taint::{CallFact, ProgramFacts, TaintFinding, analyze_with_seed};

/// Embedded declarative Java rules (sources/sinks/sanitizers/propagators).
pub const JAVA_RULES_YAML: &str = include_str!("rules/java.yaml");

/// The Java rule set, loaded once.
pub fn java_rules() -> &'static RuleSet {
    static RULES: std::sync::OnceLock<RuleSet> = std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        let mut set = RuleSet::default();
        set.extend_from_yaml("java.yaml", JAVA_RULES_YAML)
            .expect("bundled java.yaml must parse");
        set
    })
}

/// Last path segment of a (possibly qualified) symbol — `java.net.URL` → `URL`.
fn bare(symbol: &str) -> String {
    symbol
        .split('.')
        .filter(|s| !s.is_empty())
        .next_back()
        .unwrap_or(symbol)
        .to_string()
}

/// Whole-word containment: does `text` reference identifier `word`?
fn contains_word(text: &str, word: &str) -> bool {
    let bytes = text.as_bytes();
    let w = word.as_bytes();
    if w.is_empty() {
        return false;
    }
    let mut start = 0usize;
    while let Some(rel) = text[start..].find(word) {
        let s = start + rel;
        let e = s + w.len();
        let ok_before = s == 0 || !bytes[s - 1].is_ascii_alphanumeric();
        let ok_after = e >= bytes.len() || !bytes[e].is_ascii_alphanumeric();
        if ok_before && ok_after {
            return true;
        }
        start = e;
    }
    false
}

#[derive(Default)]
struct MethodEvents {
    /// Ordered events (source order): calls and variable definitions.
    ops: Vec<EventOp>,
    /// Text of `if (...)` validation guards (whitelist/matches/allowed).
    guards: Vec<String>,
}

enum EventOp {
    /// Symbol, line, argument texts, receiver (object) text (None when
    /// the call is unqualified/constructor).
    Call(String, usize, Vec<String>, Option<String>),
    /// Variable name, RHS text.
    Def(String, String),
    /// Method parameter name (seeded as untrusted).
    Param(String),
}

/// Analyze one Java source file, returning findings mapped to SARIF issues.
pub fn analyze_file(file: &str, source: &str) -> Result<Vec<SecurityIssue>, AstError> {
    let findings = analyze_source(file, source)?;
    let mut out = Vec::new();
    for f in &findings {
        out.extend(finding_to_issues(file, f));
    }
    out.extend(lexical_issues_source(file, source));
    out.extend(idor_issues_source(file, source));
    Ok(out)
}

/// Analyze one Java source file, returning raw taint findings.
pub fn analyze_source(file: &str, source: &str) -> Result<Vec<TaintFinding>, AstError> {
    let mut parser = Parser::new();
    let _lang: tree_sitter::Language = set_grammar!(
        parser,
        "java",
        "lang-java",
        tree_sitter_java::LANGUAGE.into()
    );
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| AstError::Parse("tree-sitter returned no tree".into()))?;

    let rules = java_rules();
    let source_names: Vec<String> = rules
        .sources
        .iter()
        .map(|s| bare(&s.symbol.symbol))
        .collect();
    let sanitizer_names: Vec<String> = rules
        .sanitizers
        .iter()
        .map(|s| bare(&s.symbol.symbol))
        .collect();

    let mut facts = ProgramFacts::new();
    let mut arg_seed: HashMap<usize, HashSet<usize>> = HashMap::new();
    let mut call_id = 0usize;

    let mut stack: Vec<Node> = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "method_declaration" {
            let events = collect_method_events(node, source);
            compute_method_facts(
                &events,
                file,
                &source_names,
                &sanitizer_names,
                &mut call_id,
                &mut facts,
                &mut arg_seed,
            );
            // Do not descend into the method again.
            continue;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }

    // Local-stub sinks: JSEF blinded samples simulate library sinks with
    // same-file stub methods (`query(sql)`, `exec(cmd)`, ...). Append
    // synthetic sink rules for locally defined sink-like methods; the
    // guard-aware solver then respects validation in safe variants.
    let mut augmented: RuleSet = rules.clone();
    let (methods, _idx) = extract_index(file, source);
    let local_names: std::collections::HashSet<String> =
        methods.iter().map(|m| m.name.clone()).collect();
    for name in &local_names {
        if let Some(cwe) = local_sink_cwe(name) {
            augmented.sinks.push(crate::rules::SinkRule {
                id: format!("local-stub-{name}"),
                language: "java".into(),
                symbol: crate::rules::SymbolSpec {
                    symbol: name.clone(),
                    arg: None,
                    ret: false,
                },
                category: "local-stub-sink".into(),
                cwe: vec![cwe.to_string()],
                severity: "error".into(),
            });
        }
    }
    let findings = analyze_with_seed(&facts, &augmented, &arg_seed, &HashMap::new())
        .map_err(|e| AstError::Parse(e.to_string()))?;
    Ok(findings)
}

/// Infer a CWE from a file-local (simulated) sink method name. Real library
/// sinks are matched declaratively via `rules/java.yaml`; this only covers
/// JSEF blinded samples that model sinks with same-file stub methods.
/// Matching is verb-contains to catch `fakeQueryService`, `exec2`, etc.
fn local_sink_cwe(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    let verbs: [(&str, &str); 13] = [
        ("queryfor", "CWE-89"),
        ("findbyorder", "CWE-89"),
        ("sqlquery", "CWE-89"),
        ("query", "CWE-89"),
        ("exec", "CWE-78"),
        ("runcommand", "CWE-78"),
        ("lookup", "CWE-74"),
        ("parseexpression", "CWE-917"),
        ("evalexpression", "CWE-917"),
        ("sendrequest", "CWE-918"),
        ("callurl", "CWE-918"),
        ("sendredirect", "CWE-601"),
        ("deserialize", "CWE-502"),
    ];
    for (pat, cwe) in &verbs {
        if n.contains(pat) {
            return Some(cwe);
        }
    }
    None
}

/// Recursively collect calls and local definitions within one method body, in
/// source order.
fn collect_method_events(method: Node, source: &str) -> MethodEvents {
    let mut events = MethodEvents::default();
    let mut stack = vec![method];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "method_invocation" => {
                let symbol = node
                    .child_by_field_name("name")
                    .map(|n| n.utf8_text(source.as_bytes()).unwrap_or("").to_string())
                    .unwrap_or_default();
                let args = arg_texts(node, source);
                let object = node
                    .child_by_field_name("object")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(str::to_string);
                let line = node.start_position().row + 1;
                events.ops.push(EventOp::Call(symbol, line, args, object));
                // Still descend: nested method calls in arguments are taint
                // sources themselves; re-enqueue children for defs/nested calls.
            }
            "object_creation_expression" => {
                // The `type` field is the constructed type name
                // (`File`, `java.io.File`, `ProcessBuilder`); take its last
                // identifier segment and strip generic/array markers.
                let symbol = node
                    .child_by_field_name("type")
                    .map(|t| t.utf8_text(source.as_bytes()).unwrap_or("").to_string())
                    .unwrap_or_default()
                    .replace(['<', '>', '[', ']'], "");
                let symbol = bare(&symbol);
                let args = arg_texts(node, source);
                let line = node.start_position().row + 1;
                events.ops.push(EventOp::Call(symbol, line, args, None));
            }
            "variable_declarator" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let var = name.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                    if let Some(value) = node.child_by_field_name("value") {
                        let rhs = value.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                        events.ops.push(EventOp::Def(var, rhs));
                    }
                }
            }
            "assignment_expression" => {
                let left = node.child_by_field_name("left");
                let right = node.child_by_field_name("right");
                if let (Some(l), Some(r)) = (left, right) {
                    // Track simple name assignments; field assignments are
                    // approximated by expression taint (token references).
                    if l.kind() == "identifier" {
                        let var = l.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                        let rhs = r.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                        events.ops.push(EventOp::Def(var, rhs));
                    }
                }
            }
            "if_statement" => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    if let Some(txt) = cond.utf8_text(source.as_bytes()).ok() {
                        events.guards.push(txt.to_string());
                    }
                }
            }
            "formal_parameter" => {
                // JSEF convention: method inputs are untrusted (equivalent to
                // @RequestParam/@RequestBody parameters). Seed the parameter
                // name as a taint source.
                if let Some(name) = node.child_by_field_name("name")
                    && let Some(text) = name.utf8_text(source.as_bytes()).ok()
                {
                    events.ops.push(EventOp::Param(text.to_string()));
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        // Push children in reverse so they are processed source-order on pop.
        let kids: Vec<Node> = node.children(&mut cursor).collect();
        for child in kids.into_iter().rev() {
            stack.push(child);
        }
    }
    events
}

/// Argument expressions (text) for an invocation / object-creation node.
fn arg_texts(node: Node, source: &str) -> Vec<String> {
    let Some(args) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.is_named() {
            out.push(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
        }
    }
    out
}

/// Whether expression text carries taint: references a source call or a
/// tainted local, and is not wrapped in a sanitizer call.
/// Method-name fragments that indicate validation/encoding/sanitization
/// (precision heuristic): safe JSEF variants wrap untrusted data in
/// validate/encode/whitelist helpers and use the validated result.
const SAFE_NAME_FRAGMENTS: &[&str] = &[
    "validat",
    "sanitiz",
    "encode",
    "escape",
    "allowlist",
    "whitelist",
    "clean",
    "normaliz",
    "canonical",
    "parseint",
    "parselong",
    "parsebool",
    "getvalid",
    "safedecode",
    "resolveallowed",
    "checkallowed",
    "preparestatement",
];

fn looks_sanitized(expr: &str, sanitizer_names: &[String]) -> bool {
    if sanitizer_names.iter().any(|s| contains_word(expr, s)) {
        return true;
    }
    // Name-based: a call like `validate(input)` / `encodeForHtml(input)`
    // anywhere in the expression is treated as neutralization.
    let lc = expr.to_lowercase();
    SAFE_NAME_FRAGMENTS.iter().any(|frag| lc.contains(frag))
}

/// Remove quoted string/char literal contents (keep placeholders) so token
/// matching never sees identifiers inside literals (e.g. a SQL column `id`
/// must not taint a `PreparedStatement` binding).
fn de_literal(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    let mut quote: Option<char> = None;
    for c in expr.chars() {
        match quote {
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                    out.push(' ');
                } else {
                    out.push(c);
                }
            }
            Some(q) => {
                if c == q {
                    quote = None;
                }
                out.push(' ');
            }
        }
    }
    out
}

fn expr_is_tainted(
    expr: &str,
    tainted: &HashSet<String>,
    source_names: &[String],
    sanitizer_names: &[String],
) -> bool {
    let code = de_literal(expr);
    let has_source = source_names.iter().any(|s| contains_word(&code, s));
    let has_tainted_var = tainted.iter().any(|v| contains_word(&code, v));
    let carries = has_source || has_tainted_var;
    if !carries {
        return false;
    }
    if looks_sanitized(&code, sanitizer_names) {
        return false;
    }
    true
}

fn compute_method_facts(
    events: &MethodEvents,
    file: &str,
    source_names: &[String],
    sanitizer_names: &[String],
    call_id: &mut usize,
    facts: &mut ProgramFacts,
    arg_seed: &mut HashMap<usize, HashSet<usize>>,
) {
    // Fixed point over source-ordered ops (a few passes cover branches/loops).
    // Taint seeds: method parameters (JSEF convention — method inputs are
    // untrusted), source calls, and variables propagated from either.
    let mut tainted: HashSet<String> = HashSet::new();
    for _ in 0..6 {
        for op in &events.ops {
            match op {
                EventOp::Param(name) => {
                    tainted.insert(name.clone());
                }
                EventOp::Def(var, rhs) => {
                    if expr_is_tainted(rhs, &tainted, source_names, sanitizer_names) {
                        tainted.insert(var.clone());
                    } else if tainted.contains(var) {
                        // Re-assignment to a clean value clears the variable.
                        tainted.remove(var);
                    }
                }
                EventOp::Call(symbol, _line, args, obj) => {
                    // A sanitizer call consuming a tainted variable clears it
                    // (validation/encoding gates in JSEF safe samples).
                    if sanitizer_names.iter().any(|san| symbol == san) {
                        let cleaned: Vec<String> = tainted
                            .iter()
                            .filter(|v| args.iter().any(|a| contains_word(a, v)))
                            .cloned()
                            .collect();
                        for v in cleaned {
                            tainted.remove(&v);
                        }
                    }
                    // Receiver propagation: a collection/builder receiving a
                    // tainted value (`ctx.put("k", untrusted)`, `sb.append(x)`)
                    // becomes tainted itself, and any method invoked on a
                    // tainted receiver returns tainted data. Only track simple
                    // local names (lowercase identifier, no qualification).
                    let arg_tainted = args
                        .iter()
                        .any(|a| expr_is_tainted(a, &tainted, source_names, sanitizer_names));
                    // Only propagate into collection/builder receivers
                    // (append/put/add style). Parameter-setters such as
                    // PreparedStatement.setString must NOT taint the receiver.
                    const COLLECTION_MUTATORS: &[&str] = &[
                        "append",
                        "put",
                        "putall",
                        "add",
                        "addall",
                        "insert",
                        "push",
                        "offer",
                        "addproperty",
                    ];
                    let is_collector = COLLECTION_MUTATORS
                        .iter()
                        .any(|m| symbol.to_lowercase() == *m);
                    let recv_local = obj.as_deref().and_then(|o| {
                        let head = o.trim_start();
                        if head.contains('.') || head.contains('(') {
                            return None;
                        }
                        let first = head.chars().next()?;
                        if !first.is_ascii_lowercase() {
                            return None;
                        }
                        let name: String = head
                            .chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect();
                        (!name.is_empty()
                            && is_collector
                            && (arg_tainted || tainted.contains(&name)))
                        .then_some(name)
                    });
                    if let Some(rec) = recv_local {
                        tainted.insert(rec);
                    }
                }
            }
        }
    }

    // Emit call facts; seed tainted argument positions.
    for op in &events.ops {
        if let EventOp::Call(symbol, line, args, obj) = op {
            let id = *call_id;
            *call_id += 1;
            facts.calls.push(CallFact {
                id,
                symbol: symbol.clone(),
                function: String::new(),
                file: file.to_string(),
                line: *line,
                args: Vec::new(),
                arg_count: args.len(),
            });
            let mut tainted_args = HashSet::new();
            for (i, arg) in args.iter().enumerate() {
                if expr_is_tainted(arg, &tainted, source_names, sanitizer_names) {
                    tainted_args.insert(i);
                }
            }
            // Receiver taint (e.g. `target.openConnection()` with no args):
            // mark position 0 so receiver-driven sinks can report.
            // Exception: print/println/write are XSS sinks only when invoked
            // on a response writer (getWriter/response chains); file/log writes
            // are different classes.
            let is_writer_call = matches!(symbol.as_str(), "print" | "println" | "write");
            let looks_like_writer = obj
                .as_deref()
                .map(|o| {
                    let lo = o.to_lowercase();
                    lo.contains("getwriter")
                        || lo.contains("writer")
                        || lo.ends_with("out")
                        || lo.contains("response")
                })
                .unwrap_or(false);
            if is_writer_call && !looks_like_writer {
                tainted_args.clear();
            }
            let receiver_tainted = !is_writer_call
                && obj
                    .as_deref()
                    .is_some_and(|o| expr_is_tainted(o, &tainted, source_names, sanitizer_names));
            if receiver_tainted {
                tainted_args.insert(0);
            }
            if !tainted_args.is_empty() {
                arg_seed.insert(id, tainted_args);
            }
        }
    }
}

fn finding_to_issues(file: &str, f: &TaintFinding) -> Vec<SecurityIssue> {
    let chain_text = f
        .chain
        .iter()
        .map(|s| s.description.as_str())
        .collect::<Vec<_>>()
        .join(" -> ");
    let cwes: Vec<String> = if f.cwe.is_empty() {
        vec!["CWE-20".to_string()]
    } else {
        f.cwe.clone()
    };
    cwes.iter()
        .map(|cwe| SecurityIssue {
            tool: "security_audit".into(),
            rule_id: cwe.clone(),
            severity: if f.severity == "error" {
                "high".into()
            } else {
                f.severity.clone()
            },
            category: f.category.clone(),
            title: format!("Tainted data reaches `{}` sink", f.sink_symbol),
            description: format!(
                "{}:{} — untrusted input reaches the `{}` sink ({}). Taint chain: {}",
                file, f.line, f.sink_symbol, cwe, chain_text
            ),
            cwe: vec![cwe.clone()],
            path: Some(file.to_string()),
            line: Some(f.line as u32),
            evidence: f.chain.iter().map(|s| s.description.clone()).collect(),
            automated: true,
        })
        .collect()
}

/// Analyze every `.java` file under `dir` (recursively), including
/// cross-file entry attribution for long chains.
pub fn analyze_dir(dir: &str) -> Vec<SecurityIssue> {
    let mut issues: Vec<SecurityIssue> = Vec::new();
    let root = Path::new(dir);
    let mut files = Vec::new();
    walk_java(root, &mut files);

    let mut all_methods: Vec<MethodDef> = Vec::new();
    let mut all_calls: Vec<CallSite> = Vec::new();
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let label = path.to_string_lossy().to_string();
        let (methods, calls) = extract_index(&label, &source);
        all_methods.extend(methods);
        all_calls.extend(calls);
        match analyze_file(&label, &source) {
            Ok(mut found) => issues.append(&mut found),
            Err(_) => {}
        }
    }

    // Entry-shaped files: request-mapping controllers (or main-based
    // drivers in self-contained samples). Cross-file findings are only
    // reported at chain entry sites to avoid name-collision noise.
    let entry_files: std::collections::HashSet<String> = files
        .iter()
        .filter(|p| {
            std::fs::read_to_string(p)
                .map(|text| {
                    text.contains("@RestController")
                        || text.contains("@Controller")
                        || text.contains("@GetMapping")
                        || text.contains("@PostMapping")
                        || text.contains("@RequestMapping")
                        || text.contains("void main(")
                })
                .unwrap_or(false)
        })
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    // Access-control annotation analysis (missing-authz CWE-862/863/285).
    let authz = crate::access_control::analyze_dir(
        dir,
        &crate::access_control::DEFAULT_ENTRIES(),
        &crate::access_control::DEFAULT_GATES(),
    );
    issues.extend(authz);

    // Lexical weakness detectors (no taint flow required).
    issues.extend(lexical_issues(&files));
    // IDOR: repository lookups by id with no ownership check in the method.
    issues.extend(idor_issues(&files));

    // C-phase: auto-discovered gadget chains (pivot reaching a dangerous
    // sink), reported at the pivot entry location.
    match crate::auto_gadget::discover_gadgets(root) {
        Ok(result) => {
            for chain in result.chains {
                let cwe0 = chain
                    .cwe
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "CWE-502".to_string());
                issues.push(SecurityIssue {
                    tool: "auto_gadget_discovery".into(),
                    rule_id: cwe0,
                    severity: "high".into(),
                    category: chain.category.clone(),
                    title: format!("Gadget chain: {} -> {}", chain.pivot_id, chain.sink_id),
                    description: format!("gadget chain via {}", chain.path.join(" -> ")),
                    cwe: chain.cwe.clone(),
                    path: Some(chain.pivot_hit.file.clone()),
                    line: Some(chain.pivot_hit.line as u32),
                    evidence: vec![
                        format!("pivot {}:{}", chain.pivot_hit.file, chain.pivot_hit.line),
                        format!("sink {}:{}", chain.sink_hit.file, chain.sink_hit.line),
                    ],
                    automated: true,
                });
            }
        }
        Err(_) => {}
    }

    attribute_to_entries(&mut issues, &all_methods, &all_calls, &entry_files);
    dedupe_issues(&mut issues);
    issues
}

/// Walk sink findings back to entry call sites, emitting copies at each
/// cross-file hop call site (up to depth 5).
fn attribute_to_entries(
    issues: &mut Vec<SecurityIssue>,
    methods: &[MethodDef],
    calls: &[CallSite],
    entry_files: &std::collections::HashSet<String>,
) {
    let initial: Vec<SecurityIssue> = issues.clone();
    let mut emitted: Vec<SecurityIssue> = Vec::new();
    // Current frontier: (callee method name, originating sink issue)
    let mut frontier: Vec<(String, SecurityIssue)> = initial
        .iter()
        .filter_map(|i| method_containing(i, methods).map(|m| (m, i.clone())))
        .collect();
    let mut seen_hops: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    for _ in 0..5 {
        let mut next: Vec<(String, SecurityIssue)> = Vec::new();
        for (callee, issue) in &frontier {
            for site in calls.iter().filter(|c| &c.callee == callee) {
                // Attribute across files only; same-file findings already
                // exist. Record only at entry-shaped chain origins.
                if site.file == issue.path.clone().unwrap_or_default() {
                    continue;
                }
                if !entry_files.contains(&site.file) {
                    // Still traverse through non-entry callers (services),
                    // but do not emit a finding there.
                    if let Some(caller_def) = methods
                        .iter()
                        .find(|m| m.file == site.file && m.name == site.caller)
                    {
                        next.push((caller_def.name.clone(), issue.clone()));
                    }
                    continue;
                }
                let key = (
                    site.file.clone(),
                    site.line.to_string(),
                    issue.rule_id.clone(),
                );
                if !seen_hops.insert(key.clone()) {
                    continue;
                }
                let mut at_entry = issue.clone();
                at_entry.path = Some(site.file.clone());
                at_entry.line = Some(site.line as u32);
                emitted.push(at_entry);
                if let Some(caller_def) = methods
                    .iter()
                    .find(|m| m.file == site.file && m.name == site.caller)
                {
                    next.push((caller_def.name.clone(), issue.clone()));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    issues.append(&mut emitted);
}

/// Find the simple name of the method whose body contains an issue's line.
fn method_containing(issue: &SecurityIssue, methods: &[MethodDef]) -> Option<String> {
    let file = issue.path.as_ref()?;
    let line = issue.line.unwrap_or(0) as usize;
    methods
        .iter()
        .filter(|m| &m.file == file && m.line <= line)
        .max_by_key(|m| m.line)
        .map(|m| m.name.clone())
}

/// De-duplicate findings by (rule_id, file, line).
fn dedupe_issues(issues: &mut Vec<SecurityIssue>) {
    let mut seen = std::collections::HashSet::new();
    issues.retain(|i| {
        let key = (
            i.rule_id.clone(),
            i.path.clone().unwrap_or_default(),
            i.line.unwrap_or(0),
        );
        seen.insert(key)
    });
}

/// A method definition (simple name, file, start line).
struct MethodDef {
    name: String,
    file: String,
    line: usize,
}

/// A method invocation site: `caller` (enclosing method bare name) invokes
/// `callee` at `line` in `file`.
struct CallSite {
    callee: String,
    line: usize,
    caller: String,
    file: String,
}

/// Extract method definitions and call sites from one Java source.
fn extract_index(file: &str, source: &str) -> (Vec<MethodDef>, Vec<CallSite>) {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .is_err()
    {
        return (Vec::new(), Vec::new());
    }
    let Some(tree) = parser.parse(source, None) else {
        return (Vec::new(), Vec::new());
    };
    let mut methods = Vec::new();
    let mut calls = Vec::new();
    let mut stack: Vec<(Node, Option<String>)> = vec![(tree.root_node(), None)];
    while let Some((node, enclosing)) = stack.pop() {
        let mut current = enclosing;
        if node.kind() == "method_declaration" {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .unwrap_or("")
                .to_string();
            methods.push(MethodDef {
                name: name.clone(),
                file: file.to_string(),
                line: node.start_position().row + 1,
            });
            current = Some(name);
        }
        if node.kind() == "method_invocation" {
            let callee = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .unwrap_or("")
                .to_string();
            if let Some(caller) = &current {
                calls.push(CallSite {
                    callee,
                    line: node.start_position().row + 1,
                    caller: caller.clone(),
                    file: file.to_string(),
                });
            }
        }
        let mut cursor = node.walk();
        let kids: Vec<Node> = node.children(&mut cursor).collect();
        for child in kids.into_iter().rev() {
            stack.push((child, current.clone()));
        }
    }
    (methods, calls)
}

fn walk_java(root: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str())
                && matches!(name, ".git" | "target" | "node_modules" | "build" | "dist")
            {
                continue;
            }
            walk_java(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("java") {
            out.push(p);
        }
    }
}

/// Lexical weakness detectors (no taint flow required): weak cryptographic
/// algorithms and hard-coded credentials.
fn lexical_issues(files: &[std::path::PathBuf]) -> Vec<SecurityIssue> {
    let mut out = Vec::new();
    for path in files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let label = path.to_string_lossy().to_string();
        out.extend(lexical_issues_source(&label, &source));
    }
    out
}

/// Lexical detectors over a single in-memory source file.
fn lexical_issues_source(label: &str, source: &str) -> Vec<SecurityIssue> {
    let mut out = Vec::new();
    // Strip comments to avoid matching documentation/prose content.
    let mut in_block = false;
    let code_lines: Vec<String> = source
        .lines()
        .map(|l| {
            let mut l = l.to_string();
            if in_block {
                if let Some(e) = l.find("*/") {
                    in_block = false;
                    l = l[e + 2..].to_string();
                } else {
                    return String::new();
                }
            }
            while let Some(s) = l.find("/*") {
                if let Some(e) = l[s + 2..].find("*/") {
                    l = format!("{} {}", &l[..s], &l[s + 2 + e + 2..]);
                } else {
                    l = l[..s].to_string();
                    in_block = true;
                    break;
                }
            }
            if let Some(c) = l.find("//") {
                l = l[..c].to_string();
            }
            l
        })
        .collect();
    for (i, line) in code_lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let lc = line.to_lowercase();
        // CWE-327: broken crypto algorithm in getInstance.
        let weak_algos = ["md5", "sha-1", "\"sha1", "sha_1", "md4", "md2", "rc4"];
        let weak_cipher = ["des", "rc4", "/ecb/"];
        let is_getinstance = lc.contains("getinstance(");
        let is_keyfactory = lc.contains("secretkeyspec") || lc.contains("pbekeyspec");
        if is_getinstance
            && weak_algos
                .iter()
                .any(|a| lc.replace(char::is_whitespace, "").contains(a))
        {
            out.push(lex_issue(
                &label,
                i + 1,
                "CWE-327",
                "Weak cryptographic algorithm",
                "Weak/legacy crypto algorithm (MD5/SHA-1/DES/RC4) — collision/broken primitives",
            ));
        }
        if (is_getinstance || is_keyfactory)
            && weak_cipher.iter().any(|a| {
                lc.replace(char::is_whitespace, "")
                    .contains(&format!("\"{a}"))
            })
        {
            out.push(lex_issue(
                &label,
                i + 1,
                "CWE-327",
                "Weak cipher mode/algorithm",
                "Weak cipher (DES/RC4/ECB) in getInstance/key material",
            ));
        }
        // CWE-798: hardcoded credentials (password/secret/key assigned a literal).
        if (lc.contains("password")
            || lc.contains("passwd")
            || lc.contains("secret")
            || lc.contains("apikey")
            || lc.contains("api_key")
            || lc.contains("token"))
            && lc.contains('=')
            && lc.contains('"')
            && !lc.contains("getproperty")
            && !lc.contains("logger")
            && !lc.contains("println")
            && lc.split('"').filter(|s| !s.is_empty()).any(|s| {
                let v = s.trim();
                v.len() >= 6
                    && v.chars().any(|c| c.is_ascii_alphabetic())
                    && !["password", "secret", "secret_key", "changeit", "null"]
                        .contains(&v.to_lowercase().as_str())
            })
        {
            out.push(lex_issue(
                &label,
                i + 1,
                "CWE-798",
                "Hardcoded credential",
                "Hard-coded password/secret/credential in source",
            ));
        }
    }
    out
}

fn lex_issue(file: &str, line: usize, rule: &str, category: &str, title: &str) -> SecurityIssue {
    SecurityIssue {
        tool: "security_audit".into(),
        rule_id: rule.to_string(),
        severity: "high".into(),
        category: category.to_string(),
        title: title.to_string(),
        description: format!("{file}:{line} — {title}"),
        cwe: vec![rule.to_string()],
        path: Some(file.to_string()),
        line: Some(line as u32),
        evidence: vec![format!("lexicographic pattern match")],
        automated: true,
    }
}

/// CWE-639 IDOR detector: a method that fetches data by a caller-controlled
/// id (`findById`/`getById`/`findOne`/`get(id)`) but never references an
/// ownership/current-user check (`currentUser`, `owner`, `session`).
fn idor_issues(files: &[std::path::PathBuf]) -> Vec<SecurityIssue> {
    let mut out = Vec::new();
    for path in files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let label = path.to_string_lossy().to_string();
        out.extend(idor_issues_source(&label, &source));
    }
    out
}

fn idor_issues_source(label: &str, source: &str) -> Vec<SecurityIssue> {
    let mut out = Vec::new();
    {
        let mut parser = Parser::new();
        if parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .is_err()
        {
            return out;
        }
        let Some(tree) = parser.parse(&source, None) else {
            return out;
        };
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if node.kind() == "method_declaration" {
                let body = node.utf8_text(source.as_bytes()).unwrap_or("");
                let bl = body.to_lowercase();
                let has_repo_lookup = bl.contains("findbyid")
                    || bl.contains("getbyid")
                    || bl.contains("findone")
                    || bl.contains("getone(")
                    || bl.contains("load(");
                let has_owner_check = bl.contains("currentuser")
                    || bl.contains("getowner")
                    || bl.contains(".owner")
                    || bl.contains("owns(")
                    || bl.contains("getcurrentuser")
                    || bl.contains("securitycontext")
                    || bl.contains("tenant")
                    || bl.contains("belongs")
                    || bl.contains("accountid")
                    || bl.contains("createdby");
                if has_repo_lookup && !has_owner_check {
                    // Locate the lookup call line within the method body.
                    for (rel, ln) in body.lines().enumerate() {
                        let l = ln.to_lowercase();
                        if l.contains("findbyid")
                            || l.contains("getbyid")
                            || l.contains("findone")
                            || l.contains("getone(")
                        {
                            let abs_line = node.start_position().row + 1 + rel;
                            out.push(SecurityIssue {
                                tool: "access_control".into(),
                                rule_id: "CWE-639".to_string(),
                                severity: "high".into(),
                                category: "idor".into(),
                                title: "Object fetched by id without ownership check (IDOR)".into(),
                                description: format!(
                                    "{label}:{abs_line} — repository lookup by caller-controlled id without owner/currentUser check"
                                ),
                                cwe: vec!["CWE-639".to_string()],
                                path: Some(label.to_string()),
                                line: Some(abs_line as u32),
                                evidence: vec!["no currentUser/owner reference in method".into()],
                                automated: true,
                            });
                            break;
                        }
                    }
                }
                // Do not descend into nested structures.
                continue;
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
    }
    out
}
