//! Call graph construction and reachability (L1 analysis foundation).
//!
//! This is the *minimal* call-graph skeleton that the SAST data-flow solver
//! (#598) builds on. It consumes the tree-sitter AST produced by
//! [`crate::query_source`]'s parser and extracts, for a single translation
//! unit:
//!
//! - function definitions (`function_item` in Rust, `method_declaration`
//!   nested in `class_declaration` in Java), and
//! - call sites (`call_expression` in Rust, `method_invocation` in Java),
//!   resolved *by name* to a conservative callee within the same file.
//!
//! Resolution is intentionally naive (last path segment, same-file name
//! match). Cross-file resolution, field/type sensitivity, and inter-procedural
//! summaries are later slices of #598 — this skeleton only provides the graph
//! data structure, a worklist reachability query, and cycle safety so the
//! higher layers have something to build on.
//!
//! As of #788 / W2, [`CallGraph::build_from_dir`] additionally supports
//! cross-file Java projects: every `.java` file in a directory is parsed and
//! its nodes/edges are merged into one graph, so a gadget chain spanning
//! multiple source files can be traced end-to-end.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::{AstError, Language};

/// Stable identifier for a function within one analyzed unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub usize);

/// A discovered function definition.
#[derive(Debug, Clone)]
pub struct Function {
    pub id: FuncId,
    /// Simple name (last path segment of the definition), e.g. `run_exec`
    /// for Rust or `run_exec` within `Foo` for Java.
    pub name: String,
    /// Source file the function was found in.
    pub file: String,
    /// 1-based line of the definition.
    pub line: usize,
    /// Enclosing class/type name, if the grammar models one (Java
    /// `class_declaration`). `None` for Rust and free functions.
    pub class: Option<String>,
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

    /// Look up the function *name* for a `FuncId` (mirror of `by_id`).
    pub fn function_name(&self, id: FuncId) -> Option<&str> {
        self.by_id.get(&id).map(|s| s.as_str())
    }

    /// Return the ids of functions that call `id` (reverse edges). Built on
    /// demand from the forward `edges` map; cheap for typical unit sizes.
    pub fn callers_of(&self, id: FuncId) -> Vec<FuncId> {
        let mut callers = Vec::new();
        for (caller, edges) in &self.edges {
            if edges.iter().any(|e| e.callee == Some(id)) {
                callers.push(*caller);
            }
        }
        callers
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
                if let Some(callee) = edge.callee
                    && !visited.contains(&callee)
                {
                    worklist.push(callee);
                }
                // Unresolved callees (None) are skipped: they have no node in
                // this unit, matching the conservative same-file resolution.
            }
        }
        visited
    }

    /// Convenience: reachability starting from a function name.
    pub fn reachable_from_name(&self, name: &str) -> Option<HashSet<FuncId>> {
        self.function_id_by_name(name)
            .map(|id| self.reachable_from(id))
    }

    /// Build a call graph from source text using tree-sitter.
    ///
    /// Multi-language resolution is feature-gated: a language whose grammar is
    /// not compiled in this build returns [`AstError::Unsupported`] rather than
    /// misparsing. See `crate::Cargo.toml` for the `lang-*` feature flags.
    pub fn build(file: &str, source: &str, lang: Language) -> Result<Self, AstError> {
        let mut parser = Parser::new();
        let ts_lang: tree_sitter::Language = match lang {
            Language::Rust => {
                crate::set_grammar!(parser, lang, "lang-rust", tree_sitter_rust::LANGUAGE.into())
            }
            Language::Java => {
                crate::set_grammar!(parser, lang, "lang-java", tree_sitter_java::LANGUAGE.into())
            }
            Language::TypeScript => crate::set_grammar!(
                parser,
                lang,
                "lang-typescript",
                tree_sitter_typescript::LANGUAGE_TSX.into()
            ),
            Language::JavaScript => crate::set_grammar!(
                parser,
                lang,
                "lang-javascript",
                tree_sitter_javascript::LANGUAGE.into()
            ),
            Language::Kotlin => crate::set_grammar!(
                parser,
                lang,
                "lang-kotlin",
                tree_sitter_kotlin_ng::LANGUAGE.into()
            ),
            Language::Swift => crate::set_grammar!(
                parser,
                lang,
                "lang-swift",
                tree_sitter_swift::LANGUAGE.into()
            ),
            Language::ObjectiveC => {
                crate::set_grammar!(parser, lang, "lang-objc", tree_sitter_objc::LANGUAGE.into())
            }
            // JSON has no function-call graph; the call_graph tool reports it as
            // unsupported rather than misparsing.
            Language::Json => return Err(AstError::Unsupported(lang)),
            Language::Auto => return Err(AstError::Unsupported(lang)),
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

    /// Build a single merged call graph over every `.java` file under
    /// `dir`, enabling cross-file gadget-chain tracing (W2 / #788).
    ///
    /// Both passes (definition collection, then call-edge resolution) run over
    /// *all* files before edges are resolved, so a `callee` in one file can
    /// resolve to a `method` definition in another. Resolution stays
    /// conservative (suffix/last-segment match via [`CallGraph::function_id`]).
    ///
    /// Non-`.java` files and files that fail to parse are silently skipped so a
    /// mixed-language tree still yields a usable Java graph.
    pub fn build_from_dir(dir: &Path) -> Self {
        let mut files: Vec<(String, String)> = Vec::new();
        collect_java_files(dir, &mut files);
        // Sort for deterministic "first definition wins" name resolution.
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let mut graph = CallGraph::default();
        // Pass 1: register every method definition across all files.
        for (file, source) in &files {
            if let Ok(tree) = parse_java(source) {
                collect_functions(&mut graph, file, source, tree.root_node());
            }
        }
        // Pass 2: resolve call edges now that all method names are known.
        for (_file, source) in &files {
            if let Ok(tree) = parse_java(source) {
                collect_calls(&mut graph, source, tree.root_node());
            }
        }
        graph
    }

    /// Programmatically register a function (used by tests and by the
    /// inter-procedural builder when the graph is assembled from facts rather
    /// than parsed source). First definition under a name wins, mirroring the
    /// parse-based registration.
    pub fn add_function(&mut self, name: &str, file: &str, line: usize) {
        register_function(self, name.to_string(), None, file, line);
    }

    /// Add a call edge `caller -> callee` by name. Both names are resolved via
    /// [`CallGraph::function_id`] (exact + suffix), so this works for
    /// cross-file edges where the callee lives in another file. If the callee
    /// has no registered definition it is recorded as an unresolved edge
    /// (`callee: None`) — conservative and still traceable by name.
    pub fn add_call(&mut self, caller: &str, callee: &str) {
        let caller_id = match self.function_id(caller) {
            Some(id) => id,
            None => return,
        };
        let callee_id = self.function_id(callee);
        self.edges.entry(caller_id).or_default().push(CallEdge {
            caller: caller_id,
            callee_name: callee.to_string(),
            callee: callee_id,
        });
    }

    /// Resolve a function id by exact name or by suffix (last path segment /
    /// `Class.method` tail). Used for conservative cross-file call resolution
    /// so `Foo.bar` resolves `bar` and vice versa when only one def exists.
    pub fn function_id(&self, name: &str) -> Option<FuncId> {
        if let Some(id) = self.function_id_by_name(name) {
            return Some(id);
        }
        // Suffix match: last segment of `name` equals a registered name's tail.
        let needle = name.rsplit(['.', ':']).next().unwrap_or(name);
        self.functions
            .iter()
            .find(|f| {
                f.name == needle
                    || f.name.ends_with(&format!(".{needle}"))
                    || f.name.ends_with(&format!(":{needle}"))
            })
            .map(|f| f.id)
    }
}

/// Recursively find `function_item` (Rust) and `method_declaration`
/// (Java) nodes and register them. Java methods are recorded with their
/// enclosing `class_declaration` name as a `Class.method` prefix so call sites
/// can resolve across files.
fn collect_functions(graph: &mut CallGraph, file: &str, source: &str, node: Node) {
    if node.kind() == "function_item" {
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = name_node
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                register_function(graph, name, None, file, name_node.start_position().row + 1);
            }
        }
    } else if node.kind() == "method_declaration"
        && let Some(name_node) = node.child_by_field_name("name")
    {
        let method = name_node
            .utf8_text(source.as_bytes())
            .unwrap_or("")
            .to_string();
        if !method.is_empty() {
            let class = enclosing_class_name(node, source);
            // Register under both `Class.method` and the bare `method`
            // (first-wins) so same-file short-name calls also resolve.
            if let Some(cls) = &class {
                register_function(
                    graph,
                    format!("{cls}.{method}"),
                    Some(cls.clone()),
                    file,
                    name_node.start_position().row + 1,
                );
            }
            register_function(
                graph,
                method.clone(),
                class,
                file,
                name_node.start_position().row + 1,
            );
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_functions(graph, file, source, child);
    }
}

/// Register a single function definition, assigning the next `FuncId`.
/// The first definition of a given name wins for `name_to_id` resolution.
fn register_function(
    graph: &mut CallGraph,
    name: String,
    class: Option<String>,
    file: &str,
    line: usize,
) {
    // Skip if already registered under this exact name (first-wins across files).
    if graph.name_to_id.contains_key(&name) {
        return;
    }
    let id = FuncId(graph.functions.len());
    graph.functions.push(Function {
        id,
        name: name.clone(),
        file: file.to_string(),
        line,
        class: class.clone(),
    });
    graph.by_id.insert(id, name.clone());
    graph.name_to_id.entry(name).or_insert(id);
}

/// Walk up the parent chain to find the nearest `class_declaration` name for a
/// Java method/field node.
fn enclosing_class_name(node: Node, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(n) = current {
        if (n.kind() == "class_declaration"
            || n.kind() == "enum_declaration"
            || n.kind() == "interface_declaration")
            && let Some(name_node) = n.child_by_field_name("name")
        {
            return Some(
                name_node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .to_string(),
            );
        }
        current = n.parent();
    }
    None
}

/// Recursively find `call_expression` (Rust) and `method_invocation` (Java)
/// nodes that live inside a function body and record an edge from the enclosing
/// function to the (name-resolved) callee.
fn collect_calls(graph: &mut CallGraph, source: &str, node: Node) {
    if node.kind() == "function_item" {
        // Determine the enclosing function id for this body.
        let enclosing = node.child_by_field_name("name").and_then(|n| {
            let nm = n.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            graph.function_id(&nm)
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
    if node.kind() == "method_declaration" {
        // Java: resolve the enclosing method id (try `Class.method` then bare).
        let method_name = node
            .child_by_field_name("name")
            .map(|n| n.utf8_text(source.as_bytes()).unwrap_or("").to_string())
            .unwrap_or_default();
        let class = enclosing_class_name(node, source);
        let lookup = class
            .as_ref()
            .map(|c| format!("{c}.{method_name}"))
            .unwrap_or_else(|| method_name.clone());
        let enclosing = graph
            .function_id(&lookup)
            .or_else(|| graph.function_id(&method_name));
        if let Some(caller) = enclosing
            && let Some(body) = node.child_by_field_name("body")
        {
            scan_calls_in_body(graph, source, caller, body);
        }
        // Descend so nested methods (lambdas, local classes) register too.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "method_declaration" {
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
                let callee = graph.function_id(&callee_name);
                graph.edges.entry(caller).or_default().push(CallEdge {
                    caller,
                    callee_name,
                    callee,
                });
            }
            // Do not descend into the callee expression itself for more calls
            // here — they are handled by the general walk below.
        } else if node.kind() == "method_invocation" {
            // Java: `obj.method(args)` or `obj.inner().method(args)`.
            let callee_name = resolve_java_invocation(node, source);
            if !callee_name.is_empty() {
                let callee = graph.function_id(&callee_name);
                graph.edges.entry(caller).or_default().push(CallEdge {
                    caller,
                    callee_name,
                    callee,
                });
            }
            // Descend: the `object` of this invocation may itself be a nested
            // `method_invocation` (e.g. `Runtime.getRuntime().exec`).
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

/// Resolve a Java `method_invocation` to a callee symbol. For
/// `Runtime.getRuntime().exec(...)` we want `Runtime.exec` (the receiver type
/// plus the invoked method) so it can match a `Class.method` definition. The
/// receiver is taken from the last identifier segment of the `object` chain
/// (handles both `Foo.bar()` and `foo.bar()` receiver forms conservatively).
fn resolve_java_invocation(node: Node, source: &str) -> String {
    let method = node
        .child_by_field_name("name")
        .map(|n| n.utf8_text(source.as_bytes()).unwrap_or("").to_string())
        .unwrap_or_default();
    if method.is_empty() {
        return String::new();
    }
    let object = node.child_by_field_name("object");
    if let Some(obj) = object {
        // For a constructor call `new Foo().bar()`, the receiver type is the
        // constructed class `Foo` (the `type` field of the
        // `object_creation_expression`), not the literal token `new`.
        let receiver = if obj.kind() == "object_creation_expression" {
            obj.child_by_field_name("type")
                .map(|t| t.utf8_text(source.as_bytes()).unwrap_or("").to_string())
        } else {
            let obj_text = obj.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            let lead = obj_text
                .split(['.', '(', ' '])
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if lead
                .chars()
                .next()
                .map(|c| c.is_alphabetic())
                .unwrap_or(false)
            {
                Some(lead)
            } else {
                None
            }
        };
        if let Some(r) = receiver.filter(|r| !r.is_empty()) {
            return format!("{r}.{method}");
        }
    }
    method
}

/// Extract the last identifier segment of a call target, e.g.
/// `std::process::Command::new` -> `new`, `obj.do_thing` -> `do_thing`.
/// This is the conservative same-file name used for resolution in this
/// skeleton; type/field-sensitive resolution is a later slice of #598.
fn resolve_callee_name(function_node: Node, source: &str) -> String {
    let text = function_node.utf8_text(source.as_bytes()).unwrap_or("");
    text.rsplit([':', '.'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Recursively collect `(file, source)` pairs for every `.java` file under
/// `dir`. Symlinks are not followed; parse errors are left to the caller.
fn collect_java_files(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_java_files(&path, out);
        } else if path.extension().map(|e| e == "java").unwrap_or(false)
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            out.push((path.to_string_lossy().to_string(), text));
        }
    }
}

/// Parse Java `source` with the tree-sitter-java grammar. Returns an error
/// (rather than panicking) when the grammar is not compiled in, so
/// [`CallGraph::build_from_dir`] can skip unparseable units gracefully.
fn parse_java(source: &str) -> Result<tree_sitter::Tree, AstError> {
    let mut parser = Parser::new();
    let ts_lang: tree_sitter::Language = set_grammar!(
        parser,
        Language::Java,
        "lang-java",
        tree_sitter_java::LANGUAGE.into()
    );
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| AstError::Parse("tree-sitter returned no tree".into()))?;
    let _ = ts_lang;
    Ok(tree)
}
