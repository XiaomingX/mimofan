//! Automatic gadget discovery engine (W2 / #788, #790, #791, #794).
//!
//! This is the *discovery* counterpart to the curated, reverse-trace
//! [`crate::kb_trace`] gadget-chain knowledge base. Instead of relying on a
//! hand-curated list of known exploit chains, this engine statically scans a
//! Java project for **paradigm-level** dangerous symbols (runtime exec,
//! reflection, JNDI lookup, deserialization entry points, expression
//! evaluation, unsafe class loading, …) driven entirely by the data file
//! `rules/java_auto_gadget.yaml`.
//!
//! The engine is intentionally **library-agnostic**: there is NO
//! `if library == "fastjson"` style special-case branch anywhere. Every
//! gadget class is a plain symbol matched by method name (with an optional
//! type-receiver prefix for stronger matches). Adding a new class is a YAML
//! edit, not a Rust change.
//!
//! Pipeline
//! ---------
//! 1. Parse `rules/java_auto_gadget.yaml` (reusing the dependency-free YAML
//!    subset loader in [`crate::rules`]) into pivot/sink/source symbol sets.
//! 2. Build one merged cross-file [`crate::callgraph::CallGraph`] over the
//!    target directory.
//! 3. Walk every `.java` file for `method_invocation` call sites, resolve each
//!    to a `(receiver.)method` symbol, and match it against the rule sets.
//! 4. For every pivot hit, follow the call graph downward (callee edges,
//!    transitive) to see whether it reaches a sink. A pivot that reaches a
//!    sink is a discovered **gadget chain**; the sinks reached are the
//!    `sinks_hit` list.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use anyhow::Context;

use crate::callgraph::{CallGraph, FuncId};
use crate::rules::{self, Yaml};
use crate::AstError;

/// On-disk rule file (paradigm-level pivot/sink/source symbols).
const RULES_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rules/java_auto_gadget.yaml");

/// One discovered gadget chain: a pivot that transitively reaches a sink.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GadgetChain {
    /// Pivot rule id that seeds the chain (e.g. `runtime-exec`).
    pub pivot_id: String,
    /// Sink rule id the chain reaches (e.g. `runtime-exec-sink`).
    pub sink_id: String,
    /// Human-readable category, copied from the sink rule.
    pub category: String,
    /// CWEs associated with the sink.
    pub cwe: Vec<String>,
    /// The entry function (caller of the pivot-bearing function), if any.
    pub entry_function: Option<String>,
    /// Ordered call path from the pivot-bearing function to the sink-bearing
    /// function (BFS shortest path over callee edges). The first entry is the
    /// function containing the pivot; the last is the function containing the
    /// sink call site.
    pub path: Vec<String>,
    /// Concrete source locations of the pivot and sink hits (file:line).
    pub pivot_hit: SourceLocation,
    pub sink_hit: SourceLocation,
}

/// A concrete source location of a matched symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    /// Enclosing function the call site lives in.
    pub function: String,
    /// Resolved `(receiver.)method` symbol that matched.
    pub symbol: String,
}

/// Full discovery result, serializable so the TUI tool can return it directly.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscoveryResult {
    /// Every discovered gadget chain (pivot reaching a sink).
    pub chains: Vec<GadgetChain>,
    /// Sink rule ids that were reached by at least one pivot.
    pub sinks_hit: Vec<String>,
    /// All pivot rule ids that were observed anywhere in the target (even if
    /// they did not reach a sink within the analyzed scope). Useful signal for
    /// the reverse-trace tool to narrow the KB.
    pub pivots_observed: Vec<String>,
}

impl DiscoveryResult {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// A single rule set entry (pivot, sink, or source).
#[derive(Debug, Clone)]
struct Rule {
    id: String,
    symbol: String,
    /// Optional receiver/type prefix parsed from `symbol` (the part before the
    /// last `.`). Empty means "match by method name only".
    receiver: String,
    /// Method name parsed from `symbol` (the part after the last `.`, or the
    /// whole symbol if no `.`).
    method: String,
    category: String,
    cwe: Vec<String>,
    /// Sink severity (carried from the rule file; informational — DiscoveryResult
    /// surfaces `category`/`cwe` and the KB trace judges satisfaction).
    #[allow(dead_code)]
    severity: String,
}

impl Rule {
    fn parse(symbol: &str, id: &str, category: &str, cwe: Vec<String>, severity: &str) -> Self {
        let (receiver, method) = match symbol.rsplit_once('.') {
            Some((r, m)) => (r.to_string(), m.to_string()),
            None => (String::new(), symbol.to_string()),
        };
        Rule {
            id: id.to_string(),
            symbol: symbol.to_string(),
            receiver,
            method,
            category: category.to_string(),
            cwe,
            severity: severity.to_string(),
        }
    }

    /// Does this rule match a resolved `(receiver.)method` call symbol?
    /// Strong match requires the full symbol to equal `rule.symbol`; weak match
    /// requires the call's method name to equal `rule.method`. Either suffices
    /// for a paradigm-level discovery hit (conservative by design).
    fn matches(&self, call_symbol: &str, call_method: &str) -> bool {
        if !self.receiver.is_empty() && call_symbol == self.symbol {
            return true; // strong: receiver.method fully matches
        }
        call_method == self.method
    }
}

/// Load and parse the paradigm-level rule file.
fn load_rules() -> anyhow::Result<(Vec<Rule>, Vec<Rule>, Vec<Rule>)> {
    let text = std::fs::read_to_string(RULES_FILE)
        .map_err(|e| anyhow::anyhow!("reading {RULES_FILE}: {e}"))?;
    let doc = rules::parse_yaml(RULES_FILE, &text)?;
    let map = doc.as_map().context("rule document must be a mapping")?;

    let pivots = parse_rules(map.get("pivots"), "");
    let sinks = parse_rules(map.get("sinks"), "error");
    let sources = parse_rules(map.get("sources"), "");
    Ok((pivots, sinks, sources))
}

fn parse_rules(yaml: Option<&Yaml>, default_severity: &str) -> Vec<Rule> {
    let mut out = Vec::new();
    if let Some(Yaml::Seq(items)) = yaml {
        for it in items {
            let m = match it.as_map() {
                Some(m) => m,
                None => continue,
            };
            let id = m.get("id").and_then(Yaml::as_str).unwrap_or("").to_string();
            let symbol = m
                .get("symbol")
                .and_then(Yaml::as_str)
                .unwrap_or("")
                .to_string();
            if symbol.is_empty() {
                continue;
            }
            let category = m
                .get("category")
                .and_then(Yaml::as_str)
                .unwrap_or("")
                .to_string();
            let cwe = match m.get("cwe") {
                Some(Yaml::Seq(s)) => s.iter().filter_map(Yaml::as_str).map(|x| x.to_string()).collect(),
                Some(Yaml::Str(s)) => vec![s.clone()],
                _ => vec![],
            };
            let severity = m
                .get("severity")
                .and_then(Yaml::as_str)
                .unwrap_or(default_severity)
                .to_string();
            out.push(Rule::parse(&symbol, &id, &category, cwe, &severity));
        }
    }
    out
}

/// One resolved call site observed while scanning the target.
#[derive(Debug, Clone)]
struct CallSite {
    file: String,
    line: usize,
    /// Enclosing `Class.method` (or bare `method`).
    function: String,
    /// Resolved `(receiver.)method` symbol.
    symbol: String,
    /// Method name (last segment).
    method: String,
}

/// Scan every `.java` file under `dir` for `method_invocation` call sites,
/// resolving each to a `(receiver.)method` symbol plus its enclosing function.
fn scan_calls(dir: &Path) -> Vec<CallSite> {
    let mut files: Vec<(String, String)> = Vec::new();
    collect_java_files(dir, &mut files);
    let mut sites = Vec::new();
    for (file, source) in files {
        if let Ok(tree) = parse_java(&source) {
            scan_invocations(&tree.root_node(), &source, &file, &mut sites);
        }
    }
    sites
}

fn scan_invocations(node: &Node, source: &str, file: &str, out: &mut Vec<CallSite>) {
    if node.kind() == "method_invocation" {
        let method = node
            .child_by_field_name("name")
            .map(|n| n.utf8_text(source.as_bytes()).unwrap_or("").to_string())
            .unwrap_or_default();
        if !method.is_empty() {
            let receiver = node
                .child_by_field_name("object")
                .map(|o| {
                    let t = o.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                    t.split(['.', '(', ' ']).next().unwrap_or("").trim().to_string()
                })
                .filter(|r| !r.is_empty() && r.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false));
            let symbol = match &receiver {
                Some(r) => format!("{r}.{method}"),
                None => method.clone(),
            };
            let function = enclosing_function_name(*node, source);
            out.push(CallSite {
                file: file.to_string(),
                line: node.start_position().row + 1,
                function,
                symbol,
                method: method.clone(),
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        scan_invocations(&child, source, file, out);
    }
}

/// Walk up to the enclosing `method_declaration` (within its `class`) to label
/// the call site with `Class.method`.
fn enclosing_function_name(node: Node, source: &str) -> String {
    let mut current = node.parent();
    let mut method = None;
    while let Some(n) = current {
        if n.kind() == "method_declaration" {
            if let Some(name_node) = n.child_by_field_name("name") {
                method = Some(name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string());
            }
            // Keep walking up to find the enclosing class for the prefix.
        }
        if n.kind() == "class_declaration"
            || n.kind() == "enum_declaration"
            || n.kind() == "interface_declaration"
        {
            if let (Some(m), Some(name_node)) = (
                &method,
                n.child_by_field_name("name"),
            ) {
                let cls = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                return format!("{cls}.{m}");
            }
        }
        current = n.parent();
    }
    method.unwrap_or_default()
}

/// Discover gadget chains in `target_dir`.
///
/// Returns every pivot that transitively reaches a sink, plus the set of sink
/// rule ids hit. See module docs for the full pipeline.
pub fn discover_gadgets(target_dir: &Path) -> anyhow::Result<DiscoveryResult> {
    let (pivots, sinks, _sources) = load_rules()?;

    // Build the merged cross-file call graph.
    let graph = CallGraph::build_from_dir(target_dir);

    // Scan all call sites and bucket hits by function name.
    let sites = scan_calls(target_dir);

    // Map: function name -> set of sink rule ids hit in that function.
    let mut sink_hits_by_func: HashMap<String, Vec<(String, SourceLocation)>> = HashMap::new();
    // Map: function name -> pivot rule ids hit in that function.
    let mut pivot_hits_by_func: HashMap<String, Vec<(String, SourceLocation)>> = HashMap::new();
    let mut pivots_observed: Vec<String> = Vec::new();

    for site in &sites {
        for rule in &sinks {
            if rule.matches(&site.symbol, &site.method) {
                let loc = SourceLocation {
                    file: site.file.clone(),
                    line: site.line,
                    function: site.function.clone(),
                    symbol: site.symbol.clone(),
                };
                sink_hits_by_func
                    .entry(site.function.clone())
                    .or_default()
                    .push((rule.id.clone(), loc));
            }
        }
        for rule in &pivots {
            if rule.matches(&site.symbol, &site.method) {
                let loc = SourceLocation {
                    file: site.file.clone(),
                    line: site.line,
                    function: site.function.clone(),
                    symbol: site.symbol.clone(),
                };
                pivot_hits_by_func
                    .entry(site.function.clone())
                    .or_default()
                    .push((rule.id.clone(), loc));
                if !pivots_observed.contains(&rule.id) {
                    pivots_observed.push(rule.id.clone());
                }
            }
        }
    }

    let mut result = DiscoveryResult::default();
    let mut seen_sinks: HashSet<String> = HashSet::new();

    // For each pivot hit, follow callee edges to a sink.
    for (pivot_func, pivot_entries) in &pivot_hits_by_func {
        let Some(start_id) = graph.function_id(pivot_func) else {
            continue;
        };
        // BFS shortest path over callee edges toward any function containing a sink.
        if let Some((sink_func, path)) = bfs_to_sink(&graph, start_id, &sink_hits_by_func) {
            let sink_entries = &sink_hits_by_func[&sink_func];
            for (sink_rule_id, sink_loc) in sink_entries {
                let pivot_rule = pivot_entries
                    .first()
                    .map(|(id, _)| id.clone())
                    .unwrap_or_default();
                let sink_rule = sinks
                    .iter()
                    .find(|r| &r.id == sink_rule_id)
                    .cloned();
                let (category, cwe) = sink_rule
                    .map(|r| (r.category, r.cwe))
                    .unwrap_or_else(|| (String::new(), vec![]));
                // Entry function = a caller of pivot_func (if any).
                let entry = graph
                    .callers_of(start_id)
                    .into_iter()
                    .next()
                    .and_then(|fid| graph.function_name(fid).map(|s| s.to_string()));
                result.chains.push(GadgetChain {
                    pivot_id: pivot_rule,
                    sink_id: sink_rule_id.clone(),
                    category,
                    cwe,
                    entry_function: entry,
                    path: path.clone(),
                    pivot_hit: pivot_entries.first().map(|(_, l)| l.clone()).unwrap_or_else(|| SourceLocation {
                        file: pivot_func.clone(),
                        line: 0,
                        function: pivot_func.clone(),
                        symbol: String::new(),
                    }),
                    sink_hit: sink_loc.clone(),
                });
                if seen_sinks.insert(sink_rule_id.clone()) {
                    result.sinks_hit.push(sink_rule_id.clone());
                }
            }
        }
    }

    result.pivots_observed = pivots_observed;
    Ok(result)
}

/// BFS over callee edges from `start` to the nearest function that contains a
/// sink hit. Returns `(sink_function_name, path)` where `path[0] == start`'s
/// function name and `path[last] == sink_function_name`.
fn bfs_to_sink(
    graph: &CallGraph,
    start: FuncId,
    sink_hits_by_func: &HashMap<String, Vec<(String, SourceLocation)>>,
) -> Option<(String, Vec<String>)> {
    if let Some(name) = graph.function_name(start) {
        if sink_hits_by_func.contains_key(name) {
            return Some((name.to_string(), vec![name.to_string()]));
        }
    }
    let mut visited = HashSet::new();
    let mut parent: HashMap<FuncId, FuncId> = HashMap::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start);
    visited.insert(start);
    while let Some(cur) = queue.pop_front() {
        if let Some(name) = graph.function_name(cur) {
            if sink_hits_by_func.contains_key(name) {
                // Reconstruct path.
                let mut path = vec![name.to_string()];
                let mut p = cur;
                while let Some(&prev) = parent.get(&p) {
                    if let Some(pn) = graph.function_name(prev) {
                        path.push(pn.to_string());
                    }
                    p = prev;
                }
                path.reverse();
                return Some((name.to_string(), path));
            }
        }
        for edge in graph.calls_of(cur) {
            if let Some(callee) = edge.callee {
                if visited.insert(callee) {
                    parent.insert(callee, cur);
                    queue.push_back(callee);
                }
            }
        }
    }
    None
}

// --- tree-sitter helpers (mirror of callgraph.rs, kept local to avoid leaking
// private helpers across modules) -----------------------------------------

fn collect_java_files(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_java_files(&path, out);
        } else if path.extension().map(|e| e == "java").unwrap_or(false) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((path.to_string_lossy().to_string(), text));
            }
        }
    }
}

fn parse_java(source: &str) -> Result<tree_sitter::Tree, AstError> {
    let mut parser = Parser::new();
    let ts_lang: tree_sitter::Language = set_grammar!(
        parser,
        crate::Language::Java,
        "lang-java",
        tree_sitter_java::LANGUAGE.into()
    );
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| AstError::Parse("tree-sitter returned no tree".into()))?;
    let _ = ts_lang;
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "lang-java")]
    #[test]
    fn discovers_runtime_exec_chain() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("Vuln.java");
        let mut fh = std::fs::File::create(&f).unwrap();
        writeln!(
            fh,
            "public class Vuln {{ public void handle(String cmd) {{ Runtime.getRuntime().exec(cmd); }} }}"
        )
        .unwrap();

        let res = discover_gadgets(tmp.path()).expect("discovery ok");
        // The pivot (Runtime.exec) and the sink (Runtime.exec) are the same
        // symbol here, so a chain must be reported and the sink hit recorded.
        assert!(
            res.sinks_hit.iter().any(|s| s == "runtime-exec-sink"),
            "expected runtime-exec-sink in sinks_hit; got {res:?}"
        );
        assert!(
            !res.chains.is_empty(),
            "expected at least one gadget chain; got {res:?}"
        );
        assert!(
            res.chains.iter().any(|c| c.pivot_id == "runtime-exec" && c.sink_id == "runtime-exec-sink"),
            "expected a runtime-exec pivot->sink chain; chains={:?}",
            res.chains
        );
    }

    #[cfg(feature = "lang-java")]
    #[test]
    fn discovers_jndi_chain_across_calls() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("Svc.java");
        let mut fh = std::fs::File::create(&f).unwrap();
        // pivot-bearing method calls a sink-bearing method transitively.
        writeln!(
            fh,
            "public class Svc {{ \
               public void outer() {{ inner(); }} \
               public void inner() {{ javax.naming.InitialContext ctx = null; ctx.lookup(\"ldap://x\"); }} \
             }}"
        )
        .unwrap();

        let res = discover_gadgets(tmp.path()).expect("discovery ok");
        assert!(
            res.sinks_hit.iter().any(|s| s == "jndi-lookup-sink"),
            "expected jndi-lookup-sink; got {res:?}"
        );
        assert!(
            res.chains
                .iter()
                .any(|c| c.pivot_id == "jndi-lookup" && c.sink_id == "jndi-lookup-sink"),
            "expected jndi-lookup pivot->sink chain; chains={:?}",
            res.chains
        );
    }

    #[cfg(feature = "lang-java")]
    #[test]
    fn no_chain_for_benign_code() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("Benign.java");
        let mut fh = std::fs::File::create(&f).unwrap();
        // A perfectly ordinary method with no dangerous pivot/sink symbol.
        writeln!(
            fh,
            "public class Benign {{ public void only() {{ java.io.File f = new java.io.File(\"x\"); f.listFiles(); }} }}"
        )
        .unwrap();
        let res = discover_gadgets(tmp.path()).expect("discovery ok");
        // No dangerous symbol => no pivot observed, no chain formed.
        assert!(
            res.pivots_observed.is_empty(),
            "benign code must not observe any pivot; got {:?}",
            res.pivots_observed
        );
        assert!(
            res.chains.is_empty(),
            "benign code must not yield gadget chains; chains={:?}",
            res.chains
        );
    }
}
