//! Access-Control static analysis.
//!
//! Mirrors the AVDH "Access Control agent": for each entry point, decide
//! whether an authorization gate is *missing* or the wrong identity is used.
//! mimofan has no engine for this today; this module adds a self-contained,
//! call-graph-reachability based detector.
//!
//! Core logic: for each entry-point function, determine whether any
//! authorization-gate method is reachable from it (via `CallGraph` same-file
//! reachability). Reachable => protected; unreachable => exposed.
//!
//! All detection here is **suffix-name based**: a function is an entry point
//! when its simple name ends with a configured entry suffix, and a function is
//! a gate when its name ends with a configured gate suffix. This is a
//! heuristic — see the module docs for known limitations.

use crate::callgraph::CallGraph;
use crate::sarif::SecurityIssue;
use crate::{AstError, Language};
use std::sync::OnceLock;

/// A single authorization-gate pattern. Suffix-matched against callable names.
#[derive(Clone)]
pub struct GateSpec {
    pub symbol: String,
    pub severity: String,
    pub category: String,
    pub cwe: Vec<String>,
}

/// A single entry-point pattern. Suffix-matched against function names.
#[derive(Clone)]
pub struct EntrySpec {
    pub symbol: String,
    pub severity: String,
    pub category: String,
    pub cwe: Vec<String>,
}

/// Default gate method suffixes (suffix-match; add reasonable coverage).
///
/// `Vec`/`String` fields are not const-constructible, so the tables are built
/// once behind a `OnceLock` and exposed as `&'static [GateSpec]`.
#[allow(non_snake_case)]
pub fn DEFAULT_GATES() -> &'static [GateSpec] {
    static GATES: OnceLock<Vec<GateSpec>> = OnceLock::new();
    GATES.get_or_init(|| {
        vec![
            GateSpec {
                symbol: "require_role".into(),
                severity: "warning".into(),
                category: "missing-authorization".into(),
                cwe: vec!["CWE-862".into()],
            },
            GateSpec {
                symbol: "check_permission".into(),
                severity: "warning".into(),
                category: "missing-authorization".into(),
                cwe: vec!["CWE-862".into()],
            },
            GateSpec {
                symbol: "require_auth".into(),
                severity: "warning".into(),
                category: "missing-authentication".into(),
                cwe: vec!["CWE-306".into()],
            },
            GateSpec {
                symbol: "require_login".into(),
                severity: "warning".into(),
                category: "missing-authentication".into(),
                cwe: vec!["CWE-306".into()],
            },
            GateSpec {
                symbol: "require_permission".into(),
                severity: "warning".into(),
                category: "missing-authorization".into(),
                cwe: vec!["CWE-862".into()],
            },
            GateSpec {
                symbol: "authorize".into(),
                severity: "warning".into(),
                category: "missing-authorization".into(),
                cwe: vec!["CWE-862".into()],
            },
            GateSpec {
                symbol: "ensure_authenticated".into(),
                severity: "warning".into(),
                category: "missing-authentication".into(),
                cwe: vec!["CWE-306".into()],
            },
            GateSpec {
                symbol: "is_admin".into(),
                severity: "warning".into(),
                category: "missing-authorization".into(),
                cwe: vec!["CWE-862".into()],
            },
            GateSpec {
                symbol: "has_role".into(),
                severity: "warning".into(),
                category: "missing-authorization".into(),
                cwe: vec!["CWE-862".into()],
            },
        ]
    })
}

/// Default entry-point name suffixes.
#[allow(non_snake_case)]
pub fn DEFAULT_ENTRIES() -> &'static [EntrySpec] {
    static ENTRIES: OnceLock<Vec<EntrySpec>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        vec![
            EntrySpec {
                symbol: "handler".into(),
                severity: "high".into(),
                category: "unauthenticated-entry-point".into(),
                cwe: vec!["CWE-306".into()],
            },
            EntrySpec {
                symbol: "endpoint".into(),
                severity: "high".into(),
                category: "unauthenticated-entry-point".into(),
                cwe: vec!["CWE-306".into()],
            },
            EntrySpec {
                symbol: "route".into(),
                severity: "high".into(),
                category: "unauthenticated-entry-point".into(),
                cwe: vec!["CWE-306".into()],
            },
            EntrySpec {
                symbol: "on_request".into(),
                severity: "high".into(),
                category: "unauthenticated-entry-point".into(),
                cwe: vec!["CWE-306".into()],
            },
        ]
    })
}

/// Java web entry-point annotations (Spring MVC, JAX-RS, Spring messaging).
///
/// Loop v2 / T1 (G1.2): the suffix-name entry table misses the dominant Java
/// case — controllers whose methods carry mapping annotations but whose names
/// (`getUser`, `list`) do not end in `handler`/`endpoint`/`route`. These
/// annotations mark HTTP-reachable entry points.
const JAVA_ENTRY_ANNOTATIONS: &[&str] = &[
    "RequestMapping",
    "GetMapping",
    "PostMapping",
    "PutMapping",
    "DeleteMapping",
    "PatchMapping",
    "Path",           // JAX-RS: @Path on a method/class
    "GET",            // JAX-RS: @GET
    "POST",           // JAX-RS: @POST
    "PUT",            // JAX-RS: @PUT
    "DELETE",         // JAX-RS: @DELETE
    "PATCH",          // JAX-RS: @PATCH
    "MessageMapping", // Spring WebSocket/STOMP
];

/// Java authorization-gate annotations (Spring Security, JSR-250, Shiro).
///
/// A method or class carrying any of these enforces authorization before the
/// entry point's body runs.
const JAVA_GATE_ANNOTATIONS: &[&str] = &[
    "PreAuthorize",
    "PostAuthorize",
    "Secured",
    "RolesAllowed",           // JSR-250
    "DenyAll",                // JSR-250: deny everyone (still a gate; safe to skip)
    "RequiresPermissions",    // Apache Shiro
    "RequiresRoles",          // Apache Shiro
    "RequiresAuthentication", // Apache Shiro
    "RequiresUser",           // Apache Shiro
];

/// Servlet entry-point method names (`HttpServlet` overrides).
const SERVLET_ENTRY_METHODS: &[&str] =
    &["doGet", "doPost", "doPut", "doDelete", "doPatch", "service"];

/// Java-style gate method names, matched against callee names reachable from
/// the entry (covers Shiro/Spring Security/Servlet programmatic checks that
/// the snake_case `DEFAULT_GATES` table misses — e.g. `subject.checkPermission`
/// or `request.isUserInRole`).
const JAVA_GATE_METHODS: &[&str] = &[
    "checkPermission",
    "checkRole",
    "isPermitted",
    "hasRole",
    "hasAuthority",
    "isUserInRole",
    "isAuthenticated",
    "isAuthorized",
];

/// Extract the annotation name from a line like `@org.springframework.web...@GetMapping("/x")`.
/// Returns the simple name after the final `.`.
fn annotation_simple_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    let after = t.strip_prefix('@')?;
    // Annotation name = identifier chars / dots up to '(' or whitespace.
    let name: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_')
        .collect();
    let simple = name.rsplit('.').next().unwrap_or(&name).to_string();
    if simple.is_empty() {
        None
    } else {
        Some(simple)
    }
}

/// Collect the annotation block immediately ABOVE 0-based line `decl_idx`:
/// consecutive lines (scanning upward) whose first non-whitespace char is `@`.
/// Multi-line annotation arguments are tolerated because the `@Name` token is
/// always on the first line of the annotation; only simple names are needed.
fn annotations_above(lines: &[&str], decl_idx: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = decl_idx;
    while i > 0 {
        let prev = lines[i - 1].trim();
        if prev.is_empty() {
            break;
        }
        // Skip non-annotation modifiers/comments between the annotation block
        // and the declaration — annotations sit in a contiguous block, so stop
        // at the first non-annotation line.
        if !prev.starts_with('@') {
            break;
        }
        if let Some(n) = annotation_simple_name(prev) {
            out.push(n);
        }
        i -= 1;
    }
    out
}

/// Collect annotations that gate the whole file via a class/interface
/// declaration (Spring/Shiro allow class-level `@PreAuthorize`,
/// `@RequiresPermissions`, etc.). Unioned across all types in the file.
fn class_level_annotations(lines: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        // Match `class X`, `interface X`, `enum X` (modifiers may precede).
        let is_type_decl = t
            .strip_prefix("public ")
            .unwrap_or(t)
            .strip_prefix("abstract ")
            .unwrap_or(t)
            .strip_prefix("final ")
            .unwrap_or(t)
            .starts_with("class ")
            || t.starts_with("interface ")
            || t.starts_with("public interface ")
            || t.starts_with("public class ")
            || t.starts_with("final class ");
        if is_type_decl {
            out.extend(annotations_above(lines, idx));
        }
    }
    out
}

/// Whether a callable name hits a [`GateSpec`] (suffix match) or the Java
/// programmatic-check method table (exact suffix match).
fn name_is_gate(name: &str, gates: &[GateSpec]) -> bool {
    gates.iter().any(|g| name.ends_with(g.symbol.as_str()))
        || JAVA_GATE_METHODS
            .iter()
            .any(|m| name == *m || name.ends_with(&format!(".{m}")) || name.ends_with(*m))
}

/// Whether any reachable function (by node name) or reachable call edge
/// (callee name, including external/undefined callees) is an authorization
/// gate. Edges matter because Shiro/Spring checks are usually calls into
/// library code with no in-project function node.
fn reaches_gate(graph: &CallGraph, entry: crate::callgraph::FuncId, gates: &[GateSpec]) -> bool {
    let reachable = graph.reachable_from(entry);
    for id in &reachable {
        if let Some(name) = graph.function_name(*id)
            && name_is_gate(name, gates)
        {
            return true;
        }
        for edge in graph.calls_of(*id) {
            if name_is_gate(&edge.callee_name, gates) {
                return true;
            }
        }
    }
    false
}

/// Annotation-aware entry-point authorization detection (Java/Kotlin).
///
/// Reports web entry points (mapping/JAX-RS annotations or `HttpServlet`
/// method overrides) that are not protected by:
/// - a gate annotation on the method or the enclosing class, or
/// - a reachable gate call (suffix gates + Java programmatic checks).
fn analyze_annotations(
    file: &str,
    source: &str,
    graph: &CallGraph,
    gates: &[GateSpec],
) -> Vec<SecurityIssue> {
    let lines: Vec<&str> = source.lines().collect();
    let class_gates: Vec<String> = class_level_annotations(&lines)
        .into_iter()
        .filter(|a| JAVA_GATE_ANNOTATIONS.contains(&a.as_str()))
        .collect();

    // The Java collector registers each method twice (qualified
    // `Class.method` with call edges and bare `method` without) at the SAME
    // source line. Group functions by line so an entry is analyzed once and
    // counts as protected when ANY of its records reaches a gate.
    let mut by_line: std::collections::BTreeMap<usize, Vec<&crate::callgraph::Function>> =
        std::collections::BTreeMap::new();
    for func in graph.functions() {
        if func.file == file {
            by_line.entry(func.line).or_default().push(func);
        }
    }

    let mut issues = Vec::new();
    for (line, funcs) in &by_line {
        let decl_idx = line.saturating_sub(1);
        let anns = annotations_above(&lines, decl_idx);

        let is_annotation_entry = anns
            .iter()
            .any(|a| JAVA_ENTRY_ANNOTATIONS.contains(&a.as_str()));
        let is_servlet_entry = funcs
            .iter()
            .any(|f| SERVLET_ENTRY_METHODS.contains(&f.name.as_str()));
        if !is_annotation_entry && !is_servlet_entry {
            continue;
        }

        // Method-level gate annotation?
        let method_gated = anns
            .iter()
            .any(|a| JAVA_GATE_ANNOTATIONS.contains(&a.as_str()));
        // Class-level gate annotation? (Spring Security/Shiro honor it on all
        // methods of the controller.)
        let class_gated = !class_gates.is_empty();
        // Reachable programmatic gate? Check every record for this line — the
        // call edges live on the qualified record.
        let call_gated = funcs.iter().any(|f| reaches_gate(graph, f.id, gates));

        if method_gated || class_gated || call_gated {
            continue;
        }

        // Prefer the qualified name for the report; fall back to the bare one.
        let display_name = funcs
            .iter()
            .map(|f| f.name.as_str())
            .find(|n| n.contains('.'))
            .unwrap_or_else(|| funcs[0].name.as_str());

        let entry_kind = if is_annotation_entry {
            anns.iter()
                .find(|a| JAVA_ENTRY_ANNOTATIONS.contains(&a.as_str()))
                .cloned()
                .unwrap_or_else(|| "mapped".to_string())
        } else {
            "servlet-method".to_string()
        };

        issues.push(SecurityIssue {
            tool: "access-control".into(),
            rule_id: "access-control.missing-annotation-authz".into(),
            severity: "high".into(),
            category: "missing-authorization".into(),
            title: format!(
                "Web entry point '{display_name}' (@{entry_kind}) has no authorization gate"
            ),
            description: format!(
                "{display_name} at {file}:{line} is an HTTP-reachable entry point (@{entry_kind}) \
                 but neither the method nor its enclosing class carries an \
                 authorization annotation (@PreAuthorize/@Secured/@RolesAllowed/\
                 @RequiresPermissions/...) and no authorization check \
                 (checkPermission/isUserInRole/...) is reachable. Every \
                 authenticated-or-anonymous request can reach this code; confirm \
                 it is not a missing-access-control finding (CWE-862/CWE-863)."
            ),
            cwe: vec!["CWE-862".into(), "CWE-863".into(), "CWE-285".into()],
            path: Some(file.to_string()),
            line: Some(*line as u32),
            evidence: vec![format!(
                "entry={display_name} line={line} kind=@{entry_kind} -> no authz annotation, no reachable gate"
            )],
            automated: true,
        });
    }
    issues
}

/// Recursively collect `.java`/`.kt` source files under `root`.
fn walk_source_entries(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(mut entries) = std::fs::read_dir(root) else {
        return;
    };
    while let Some(Ok(e)) = entries.next() {
        let p = e.path();
        if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str())
                && matches!(name, ".git" | "target" | "node_modules" | "build" | "dist")
            {
                continue;
            }
            walk_source_entries(&p, out);
        } else {
            let s = p.to_string_lossy();
            if s.ends_with(".java") || s.ends_with(".kt") {
                out.push(p);
            }
        }
    }
}

/// Analyze a single source unit: report entry points with no reachable
/// authorization gate.
pub fn analyze_file(
    file: &str,
    source: &str,
    lang: Language,
    entries: &[EntrySpec],
    gates: &[GateSpec],
) -> Result<Vec<SecurityIssue>, AstError> {
    let graph = CallGraph::build(file, source, lang)?;
    let mut issues = analyze_graph(&graph, file, entries, gates);

    // Annotation-aware pass for Java-family sources (JSEF's main surface).
    if matches!(lang, Language::Java | Language::Kotlin) {
        let before: std::collections::HashSet<u32> = issues.iter().filter_map(|i| i.line).collect();
        for ann in analyze_annotations(file, source, &graph, gates) {
            // Dedupe against a suffix-based finding on the same line.
            if ann.line.is_none_or(|l| !before.contains(&l)) {
                issues.push(ann);
            }
        }
    }
    Ok(issues)
}

/// Analyze an already-built call graph (also used for Java cross-file).
pub fn analyze_graph(
    graph: &CallGraph,
    file: &str,
    entries: &[EntrySpec],
    gates: &[GateSpec],
) -> Vec<SecurityIssue> {
    let mut issues = Vec::new();
    for func in graph.functions() {
        // Only consider functions whose name matches an entry pattern.
        let Some(entry_spec) = entries.iter().find(|e| func.name.ends_with(&e.symbol)) else {
            continue;
        };
        // Skip if the entry function itself is a gate (defensive).
        if gates.iter().any(|g| func.name.ends_with(&g.symbol)) {
            continue;
        }
        let reachable = graph.reachable_from(func.id);
        // Is any reachable function (incl. the entry) a gate?
        let has_gate = reachable.iter().any(|&id| {
            graph
                .function_name(id)
                .map(|n| gates.iter().any(|g| n.ends_with(&g.symbol)))
                .unwrap_or(false)
        });
        if !has_gate {
            issues.push(SecurityIssue {
                tool: "access-control".into(),
                rule_id: "access-control.missing-gate".into(),
                severity: entry_spec.severity.clone(),
                category: entry_spec.category.clone(),
                title: format!(
                    "Entry point '{}' has no reachable authorization gate",
                    func.name
                ),
                description: format!(
                    "{} at {}:{} does not reach any of the authorization gates ({}) \
                     before doing work. Confirm privileged functionality is not exposed \
                     to unauthenticated/unauthorized users.",
                    func.name,
                    file,
                    func.line,
                    gates
                        .iter()
                        .map(|g| g.symbol.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                cwe: entry_spec.cwe.clone(),
                path: Some(file.to_string()),
                line: Some(func.line as u32),
                evidence: vec![format!(
                    "entry={} line={} -> no gate reachable",
                    func.name, func.line
                )],
                automated: true,
            });
        }
    }
    issues
}

/// Java cross-file: analyze all .java files under `dir` with a merged graph.
///
/// The suffix reachability pass runs over the merged call graph; the
/// annotation-aware pass (G1.2) then runs per source file, with reachability
/// also resolved against the merged graph so a gate call in another file
/// counts.
pub fn analyze_dir(dir: &str, entries: &[EntrySpec], gates: &[GateSpec]) -> Vec<SecurityIssue> {
    let root = std::path::Path::new(dir);
    let graph = CallGraph::build_from_dir(root);
    let mut issues = analyze_graph(&graph, dir, entries, gates);

    let mut sources = Vec::new();
    walk_source_entries(root, &mut sources);
    for path in sources {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let file_label = path.to_string_lossy().to_string();
        let before: std::collections::HashSet<u32> = issues.iter().filter_map(|i| i.line).collect();
        for ann in analyze_annotations(&file_label, &source, &graph, gates) {
            if ann.line.is_none_or(|l| !before.contains(&l)) {
                issues.push(ann);
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze_rust(source: &str) -> Vec<SecurityIssue> {
        analyze_file(
            "test.rs",
            source,
            Language::Rust,
            DEFAULT_ENTRIES(),
            DEFAULT_GATES(),
        )
        .expect("analyze_file should succeed on valid Rust")
    }

    #[test]
    fn exposed_handler_flagged() {
        let source = r#"
fn db_insert(user: &str) {
    // persists user
}

fn create_user_handler(req: &str) -> String {
    db_insert(req);
    "ok".to_string()
}
"#;
        let issues = analyze_rust(source);
        assert_eq!(issues.len(), 1, "expected exactly one access-control issue");
        assert_eq!(issues[0].tool, "access-control");
        assert_eq!(issues[0].rule_id, "access-control.missing-gate");
        assert_eq!(issues[0].severity, "high");
        assert!(issues[0].description.contains("create_user_handler"));
    }

    #[test]
    fn protected_handler_ok() {
        let source = r#"
fn require_role(role: &str) -> bool {
    role == "admin"
}

fn create_user_handler(req: &str) -> String {
    if !require_role("admin") {
        return "forbidden".to_string();
    }
    "ok".to_string()
}
"#;
        let issues = analyze_rust(source);
        assert_eq!(issues.len(), 0, "protected handler should not be flagged");
    }

    #[test]
    fn default_gates_nonempty() {
        assert!(!DEFAULT_GATES().is_empty());
        assert!(!DEFAULT_ENTRIES().is_empty());
        // Each default gate should carry a CWE id.
        for g in DEFAULT_GATES() {
            assert!(!g.cwe.is_empty(), "gate '{}' should have a CWE", g.symbol);
        }
    }

    fn analyze_java(source: &str) -> Vec<SecurityIssue> {
        analyze_file(
            "Test.java",
            source,
            Language::Java,
            DEFAULT_ENTRIES(),
            DEFAULT_GATES(),
        )
        .expect("analyze_file should succeed on valid Java")
    }

    #[test]
    fn spring_mapping_without_authorization_is_flagged() {
        let source = r#"
@RestController
@RequestMapping("/api/users")
public class UserController {
    @PostMapping("/create")
    public String create(String req) {
        dbInsert(req);
        return "ok";
    }

    private void dbInsert(String s) {}
}
"#;
        let issues = analyze_java(source);
        assert!(
            issues
                .iter()
                .any(|i| i.rule_id == "access-control.missing-annotation-authz"),
            "expected annotation-authz finding, got: {issues:?}"
        );
        let ann = issues
            .iter()
            .find(|i| i.rule_id == "access-control.missing-annotation-authz")
            .unwrap();
        assert_eq!(ann.severity, "high");
        assert!(ann.cwe.iter().any(|c| c == "CWE-862"));
    }

    #[test]
    fn preauthorize_on_method_protects_entry() {
        let source = r#"
@RestController
@RequestMapping("/api/users")
public class UserController {
    @PreAuthorize("hasRole('ADMIN')")
    @PostMapping("/create")
    public String create(String req) {
        dbInsert(req);
        return "ok";
    }

    private void dbInsert(String s) {}
}
"#;
        let issues = analyze_java(source);
        assert!(
            !issues
                .iter()
                .any(|i| i.rule_id == "access-control.missing-annotation-authz"),
            "@PreAuthorize must protect the mapping, got: {issues:?}"
        );
    }

    #[test]
    fn class_level_preauthorize_protects_all_mappings() {
        let source = r#"
@RestController
@PreAuthorize("hasRole('ADMIN')")
@RequestMapping("/api/users")
public class UserController {
    @GetMapping("/{id}")
    public String get(String id) {
        return dbLookup(id);
    }

    private String dbLookup(String id) { return id; }
}
"#;
        let issues = analyze_java(source);
        assert!(
            !issues
                .iter()
                .any(|i| i.rule_id == "access-control.missing-annotation-authz"),
            "class-level @PreAuthorize must protect mappings, got: {issues:?}"
        );
    }

    #[test]
    fn jaxrs_get_without_gate_is_flagged() {
        let source = r#"
@Path("/orders")
public class OrderResource {
    @GET
    @Path("/{id}")
    public Order show(@PathParam("id") String id) {
        return load(id);
    }

    private Order load(String id) { return new Order(); }
}
"#;
        let issues = analyze_java(source);
        assert!(
            issues
                .iter()
                .any(|i| i.rule_id == "access-control.missing-annotation-authz"),
            "JAX-RS @GET entry without authz must be flagged, got: {issues:?}"
        );
    }

    #[test]
    fn shiro_programmatic_check_protects_entry() {
        let source = r#"
@RestController
public class AdminController {
    @DeleteMapping("/purge")
    public void purge() {
        Subject subject = SecurityUtils.getSubject();
        subject.checkPermission("admin:purge");
        deleteEverything();
    }

    private void deleteEverything() {}
}
"#;
        let issues = analyze_java(source);
        assert!(
            !issues
                .iter()
                .any(|i| i.rule_id == "access-control.missing-annotation-authz"),
            "reachable checkPermission must protect the entry, got: {issues:?}"
        );
    }

    #[test]
    fn servlet_doget_without_gate_is_flagged() {
        let source = r#"
public class ReportServlet extends HttpServlet {
    protected void doGet(HttpServletRequest req, HttpServletResponse resp) {
        writeReport(resp);
    }

    private void writeReport(HttpServletResponse resp) {}
}
"#;
        let issues = analyze_java(source);
        assert!(
            issues
                .iter()
                .any(|i| i.rule_id == "access-control.missing-annotation-authz"),
            "unguarded doGet must be flagged, got: {issues:?}"
        );
    }
}
