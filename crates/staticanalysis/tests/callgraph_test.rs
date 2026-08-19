#[cfg(not(feature = "lang-java"))]
use mimofan_staticanalysis::AstError;
use mimofan_staticanalysis::callgraph::CallGraph;
use mimofan_staticanalysis::Language;

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
    assert!(
        names.contains(&"helper"),
        "main -> helper missing: {names:?}"
    );
    assert!(names.contains(&"leaf"), "main -> leaf missing: {names:?}");
    assert!(
        edges.iter().all(|e| e.callee.is_some()),
        "same-file callees must resolve"
    );
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
    assert!(
        edges[0].callee.is_none(),
        "cross-unit callee must not resolve"
    );
    // Reachability must not panic and returns only the entry.
    assert_eq!(g.reachable_from(start).len(), 1);
}

#[cfg(not(feature = "lang-java"))]
#[test]
fn non_rust_language_is_rejected() {
    let res = CallGraph::build("x.java", "class A {}", Language::Java);
    assert!(matches!(res, Err(AstError::Unsupported(_))));
}

#[cfg(feature = "lang-java")]
#[test]
fn java_is_supported_with_feature() {
    let res = CallGraph::build("x.java", "class A {}", Language::Java);
    assert!(
        res.is_ok(),
        "Java should be supported with lang-java feature"
    );
}

#[cfg(feature = "lang-java")]
#[test]
fn java_method_call_graph_resolves() {
    let src = r#"
public class Exploit {
    public void trigger() {
        Runtime.getRuntime().exec("touch /tmp/pwn");
    }
    public void other() {
        trigger();
    }
}
"#;
    let g = CallGraph::build("Exploit.java", src, Language::Java).unwrap();
    // Both methods should be registered (under Class.method and bare name).
    assert!(
        g.function_id_by_name("Exploit.trigger").is_some(),
        "trigger not found"
    );
    // `trigger` calls Runtime.exec — edge must resolve to a known callee.
    let trigger = g.function_id_by_name("Exploit.trigger").unwrap();
    let edges = g.calls_of(trigger);
    let names: Vec<&str> = edges.iter().map(|e| e.callee_name.as_str()).collect();
    assert!(
        names.iter().any(|n| n.contains("exec")),
        "expected a call to exec, got {names:?}"
    );
    // The exec edge should resolve (Runtime.exec is a known sink-ish callee
    // if a method of that name exists; here it may be unresolved since no
    // such method is defined — that is fine; the key is no panic and the
    // trigger->other edge resolves within the file).
    let other = g.function_id_by_name("Exploit.other").unwrap();
    let other_edges = g.calls_of(other);
    assert!(
        other_edges
            .iter()
            .any(|e| e.callee_name == "trigger" && e.callee.is_some()),
        "other -> trigger must resolve within the file"
    );
}

#[cfg(feature = "lang-java")]
#[test]
fn java_cross_file_build_merges_nodes() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("A.java");
    let b = tmp.path().join("B.java");
    let mut fa = std::fs::File::create(&a).unwrap();
    writeln!(
        fa,
        "public class A {{ public void run() {{ new B().helper(); }} }}"
    )
    .unwrap();
    let mut fb = std::fs::File::create(&b).unwrap();
    writeln!(
        fb,
        "public class B {{ public void helper() {{ Runtime.getRuntime().exec(\"x\"); }} }}"
    )
    .unwrap();

    let g = CallGraph::build_from_dir(tmp.path());
    // A.run should resolve a call edge to B.helper across files.
    let run = g.function_id_by_name("A.run").expect("A.run present");
    let edges = g.calls_of(run);
    assert!(
        edges
            .iter()
            .any(|e| e.callee_name == "B.helper" && e.callee.is_some()),
        "cross-file A.run -> B.helper must resolve; edges={edges:?}"
    );
}
