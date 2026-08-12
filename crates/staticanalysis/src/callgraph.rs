//! Call graph construction and reachability (L1 analysis foundation).
//!
//! This is the *minimal* call-graph skeleton that the SAST data-flow solver
//! (#598) builds on. It consumes the tree-sitter AST produced by
//! [`crate::query_source`]'s parser and extracts, for a single translation
//! unit:
//!
//! - function definitions (`function_item` in Rust), and
//! - call sites (`call_expression`), resolved *by name* to a conservative
//!   callee within the same file.
//!
//! Resolution is intentionally naive (last path segment, same-file name
//! match). Cross-file resolution, field/type sensitivity, and inter-procedural
//! summaries are later slices of #598 — this skeleton only provides the graph
//! data structure, a worklist reachability query, and cycle safety so the
//! higher layers have something to build on.

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Parser};

use crate::{AstError, Language};

/// Stable identifier for a function within one analyzed unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub usize);

/// A discovered function definition.
#[derive(Debug, Clone)]
pub struct Function {
    pub id: FuncId,
    /// Simple name (last path segment of the definition), e.g. `run_exec`.
    pub name: String,
    /// Source file the function was found in.
    pub file: String,
    /// 1-based line of the definition.
    pub line: usize,
}

/// A call edge: `caller` invokes `callee` (callee resolved by name).
#[derive(Debug, Clone)]
pub struct CallEdge {
    pub caller: FuncId,
    /// Callee name as resolved from the call site (last path segment).
    pub callee_name: String,
    pub callee: Option<FuncId>,
}

/// A call graph for a single translation unit.
#[derive(Debug, Clone, Default)]
pub struct CallGraph {
    functions: Vec<Function>,
    /// FuncId -> function name (mirror for O(1) lookup in tests/reports).
    by_id: HashMap<FuncId, String>,
    /// function name -> first matching FuncId (same-file, first-definition wins).
    name_to_id: HashMap<String, FuncId>,
    /// caller FuncId -> outgoing edges.
    edges: HashMap<FuncId, Vec<CallEdge>>,
}

impl CallGraph {
    /// All functions discovered in the unit.
    pub fn functions(&self) -> &[Function] {
        &self.functions
    }

    /// Outgoing call edges for a function.
    pub fn calls_of(&self, id: FuncId) -> &[CallEdge] {
        self.edges.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Number of functions in the graph.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Whether the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Resolve a function id from its simple name (same-file, first def).
    pub fn function_id_by_name(&self, name: &str) -> Option<FuncId> {
        self.name_to_id.get(name).copied()
    }

    /// Worklist reachability: return the set of function ids reachable from
    /// `entry` following call edges (transitive closure). Cycles are handled
    /// via the visited set, so this terminates even on recursive call graphs.
    pub fn reachable_from(&self, entry: FuncId) -> HashSet<FuncId> {
        let mut visited = HashSet::new();
        let mut worklist = vec![entry];
        while let Some(current) = worklist.pop() {
            if !visited.insert(current) {
                continue;
            }
            for edge in self.calls_of(current) {
                if let Some(callee) = edge.callee {
                    if !visited.contains(&callee) {
                        worklist.push(callee);
                    }
                }
                // Unresolved callees (None) are skipped: they have no node in
                // this unit, matching the conservative same-file resolution.
            }
        }
        visited
    }

    /// Convenience: reachability starting from a function name.
    pub fn reachable_from_name(&self, name: &str) -> Option<HashSet<FuncId>> {
        self.function_id_by_name(name).map(|id| self.reachable_from(id))
    }

    /// Build a call graph from source text using tree-sitter.
    ///
    /// Only Rust is supported in this skeleton (the `lang-rust` grammar is the
    /// only one compiled; see `crate::Cargo.toml`). Other languages return
    /// [`AstError::Unsupported`] rather than misparsing.
    pub fn build(file: &str, source: &str, lang: Language) -> Result<Self, AstError> {
        let mut parser = Parser::new();
        let ts_lang = match lang {
            Language::Rust => {
                #[cfg(feature = "lang-rust")]
                {
                    let l = tree_sitter_rust::LANGUAGE;
                    parser
                        .set_language(&l.into())
                        .map_err(|e| AstError::Parse(e.to_string()))?;
                    l
                }
                #[cfg(not(feature = "lang-rust"))]
                {
                    return Err(AstError::Unsupported(lang));
                }
            }
            other => return Err(AstError::Unsupported(other)),
        };

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| AstError::Parse("tree-sitter returned no tree".into()))?;

        let mut graph = CallGraph::default();
        let root = tree.root_node();
        // First pass: collect all function definitions so call sites can
        // resolve names within the same file.
        collect_functions(&mut graph, file, source, root);
        // Second pass: walk each function body for call expressions.
        collect_calls(&mut graph, source, root);
        let _ = ts_lang;
        Ok(graph)
    }
}

/// Recursively find `function_item` nodes and register them.
fn collect_functions(graph: &mut CallGraph, file: &str, source: &str, node: Node) {
    if node.kind() == "function_item" {
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            if !name.is_empty() {
                let id = FuncId(graph.functions.len());
                graph.functions.push(Function {
                    id,
                    name: name.clone(),
                    file: file.to_string(),
                    line: name_node.start_position().row + 1,
                });
                graph.by_id.insert(id, name.clone());
                // First definition wins for same-file name resolution.
                graph.name_to_id.entry(name).or_insert(id);
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_functions(graph, file, source, child);
    }
}

/// Recursively find `call_expression` nodes that live inside a function body
/// and record an edge from the enclosing function to the (name-resolved)
/// callee.
fn collect_calls(graph: &mut CallGraph, source: &str, node: Node) {
    if node.kind() == "function_item" {
        // Determine the enclosing function id for this body.
        let enclosing = node
            .child_by_field_name("name")
            .and_then(|n| {
                let nm = n.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                graph.name_to_id.get(&nm).copied()
            });
        if let Some(caller) = enclosing {
            // Only scan the function body, not nested function items (they get
            // their own enclosing scope in the recursion below).
            if let Some(body) = node.child_by_field_name("body") {
                scan_calls_in_body(graph, source, caller, body);
            }
        }
        // Still descend into nested function items so they register their own
        // edges (do NOT also scan them from the outer function).
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_item" {
                collect_calls(graph, source, child);
            }
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(graph, source, child);
    }
}

/// Scan a function body for call expressions, resolving each callee name to
/// the last path segment (e.g. `a::b::foo` -> `foo`).
fn scan_calls_in_body(graph: &mut CallGraph, source: &str, caller: FuncId, body: Node) {
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        if node.kind() == "call_expression" {
            let callee_name = node
                .child_by_field_name("function")
                .map(|f| resolve_callee_name(f, source))
                .unwrap_or_default();
            if !callee_name.is_empty() {
                let callee = graph.name_to_id.get(&callee_name).copied();
                graph
                    .edges
                    .entry(caller)
                    .or_default()
                    .push(CallEdge {
                        caller,
                        callee_name,
                        callee,
                    });
            }
            // Do not descend into the callee expression itself for more calls
            // here — they are handled by the general walk below.
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

/// Extract the last identifier segment of a call target, e.g.
/// `std::process::Command::new` -> `new`, `obj.do_thing` -> `do_thing`.
/// This is the conservative same-file name used for resolution in this
/// skeleton; type/field-sensitive resolution is a later slice of #598.
fn resolve_callee_name(function_node: Node, source: &str) -> String {
    let text = function_node.utf8_text(source.as_bytes()).unwrap_or("");
    text.rsplit(|c| c == ':' || c == '.')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    const SRC: &str = r#"
fn main() {
    let x = helper();
    helper();
    leaf();
}

fn helper() {
    leaf();
    recursive();
}

fn recursive() {
    recursive();
}

fn leaf() {}
"#;

    #[test]
    fn extracts_all_function_definitions() {
        let g = CallGraph::build("demo.rs", SRC, Language::Rust).unwrap();
        assert_eq!(g.len(), 4);
        assert!(g.function_id_by_name("main").is_some());
        assert!(g.function_id_by_name("helper").is_some());
        assert!(g.function_id_by_name("recursive").is_some());
        assert!(g.function_id_by_name("leaf").is_some());
    }

    #[test]
    fn captures_direct_and_transitive_calls() {
        let g = CallGraph::build("demo.rs", SRC, Language::Rust).unwrap();
        let main = g.function_id_by_name("main").unwrap();
        let edges = g.calls_of(main);
        let names: Vec<&str> = edges.iter().map(|e| e.callee_name.as_str()).collect();
        assert!(names.contains(&"helper"), "main -> helper missing: {names:?}");
        assert!(names.contains(&"leaf"), "main -> leaf missing: {names:?}");
        assert!(edges.iter().all(|e| e.callee.is_some()), "same-file callees must resolve");
    }

    #[test]
    fn reachability_is_transitive_and_cycle_safe() {
        let g = CallGraph::build("demo.rs", SRC, Language::Rust).unwrap();
        // From main: main -> helper -> leaf,recursive; recursive -> recursive.
        let reached = g.reachable_from_name("main").unwrap();
        assert!(reached.contains(&g.function_id_by_name("main").unwrap()));
        assert!(reached.contains(&g.function_id_by_name("helper").unwrap()));
        assert!(reached.contains(&g.function_id_by_name("leaf").unwrap()));
        assert!(reached.contains(&g.function_id_by_name("recursive").unwrap()));
        // Reachable from leaf (no outgoing edges) is just itself.
        let leaf_only = g.reachable_from_name("leaf").unwrap();
        assert_eq!(leaf_only.len(), 1);
    }

    #[test]
    fn unresolved_callee_outside_unit_is_skipped() {
        const SRC2: &str = r#"
fn start() {
    external_crate_fn();
}
"#;
        let g = CallGraph::build("demo.rs", SRC2, Language::Rust).unwrap();
        let start = g.function_id_by_name("start").unwrap();
        let edges = g.calls_of(start);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].callee_name, "external_crate_fn");
        assert!(edges[0].callee.is_none(), "cross-unit callee must not resolve");
        // Reachability must not panic and returns only the entry.
        assert_eq!(g.reachable_from(start).len(), 1);
    }

    #[test]
    fn non_rust_language_is_rejected() {
        let res = CallGraph::build("x.java", "class A {}", Language::Java);
        assert!(matches!(res, Err(AstError::Unsupported(_))));
    }
}
