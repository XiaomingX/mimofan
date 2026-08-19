#[cfg(not(feature = "lang-objc"))]
use mimofan_staticanalysis::AstError;
use mimofan_staticanalysis::Language;
use mimofan_staticanalysis::{named_query, query_source};

const RUST_SRC: &str = r#"
fn main() {
    let cmd = std::process::Command::new("ls");
    unsafe {
        do_thing();
    }
    run_exec();
}
"#;

#[test]
fn parses_and_queries_rust_source() {
    let q = named_query("rust.sink.process_exec").unwrap();
    let hits = query_source("demo.rs", RUST_SRC, Language::Rust, q).expect("query ok");
    // run_exec() matches the `exec$` pattern.
    assert!(
        hits.iter().any(|h| h.text.contains("run_exec")),
        "expected run_exec call captured, got {hits:?}"
    );
}

#[test]
fn all_named_query_presets_resolve() {
    // Every preset key must return a non-empty S-expression. This guards
    // against a typo'd or dangling preset name the model might call.
    let keys = [
        "rust.sink.process_exec",
        "rust.unsound.unsafe_block",
        "java.sink.runtime_exec",
        "java.sink.sql_concat",
        "java.sink.deserialization",
        "js.sink.eval",
        "js.sink.child_process_exec",
        "js.sink.sql_concat",
        "kotlin.sink.runtime_exec",
        "swift.sink.process_run",
        "objc.sink.system",
        "webext.manifest.broad_host_permissions",
    ];
    for k in keys {
        let q = named_query(k);
        assert!(q.is_some(), "preset {k} must resolve to a query");
        assert!(
            !q.unwrap().trim().is_empty(),
            "preset {k} must have a non-empty query body"
        );
    }
    // Unknown preset must not resolve.
    assert!(named_query("no.such.preset").is_none());
}

#[test]
fn unsafe_block_is_captured() {
    let q = named_query("rust.unsound.unsafe_block").unwrap();
    let hits = query_source("demo.rs", RUST_SRC, Language::Rust, q).expect("query ok");
    assert!(!hits.is_empty(), "expected at least one unsafe_block hit");
    assert!(hits[0].file == "demo.rs");
    assert!(hits[0].line > 0, "line should be 1-based");
}

#[cfg(not(feature = "lang-objc"))]
#[test]
fn unsupported_language_is_rejected_without_feature() {
    // ObjectiveC grammar is not compiled in (no feature), must error clearly.
    let res = query_source("x.m", "int main(){}", Language::ObjectiveC, "(_) @n");
    assert!(matches!(res, Err(AstError::Unsupported(_))));
}

#[cfg(feature = "lang-objc")]
#[test]
fn objc_is_supported_with_feature() {
    // With the `lang-objc` feature the grammar is compiled in, so the query
    // must succeed rather than error out as Unsupported.
    let res = query_source("x.m", "int main(){}", Language::ObjectiveC, "(_) @n");
    assert!(
        res.is_ok(),
        "ObjectiveC should be supported with lang-objc feature"
    );
}

// ── Multi-language preset validation. These only compile/run when the
// matching grammar feature is enabled, so `cargo test --features lang-all`
// exercises every preset against a minimal fixture. ──────────────────────
#[cfg(feature = "lang-java")]
#[test]
fn java_presets_hit() {
    let src = "class A { void f() { Runtime.getRuntime().exec(\"x\"); st.execute(\"SELECT \" + y); new ObjectInputStream(in); } }";
    for key in [
        "java.sink.runtime_exec",
        "java.sink.sql_concat",
        "java.sink.deserialization",
    ] {
        let q = named_query(key).unwrap();
        let hits = query_source("A.java", src, Language::Java, q).expect("query ok");
        assert!(!hits.is_empty(), "preset {key} should hit");
    }
}

#[cfg(feature = "lang-javascript")]
#[test]
fn js_presets_hit() {
    let src =
        "eval(x); new Function(\"y\"); child_process.exec(\"ls\"); db.query(\"SELECT \" + z);";
    for key in [
        "js.sink.eval",
        "js.sink.child_process_exec",
        "js.sink.sql_concat",
    ] {
        let q = named_query(key).unwrap();
        let hits = query_source("a.js", src, Language::JavaScript, q).expect("query ok");
        assert!(!hits.is_empty(), "preset {key} should hit");
    }
}

#[cfg(feature = "lang-kotlin")]
#[test]
fn kotlin_preset_hit() {
    let src = "fun f() { Runtime.getRuntime().exec(cmd) }";
    let q = named_query("kotlin.sink.runtime_exec").unwrap();
    let hits = query_source("a.kt", src, Language::Kotlin, q).expect("query ok");
    assert!(!hits.is_empty(), "kotlin preset should hit");
}

#[cfg(feature = "lang-swift")]
#[test]
fn swift_preset_hit() {
    let src = "func f() { system(\"ls\") }";
    let q = named_query("swift.sink.process_run").unwrap();
    let hits = query_source("a.swift", src, Language::Swift, q).expect("query ok");
    assert!(!hits.is_empty(), "swift preset should hit");
}

#[cfg(feature = "lang-objc")]
#[test]
fn objc_preset_hit() {
    let src = "void f() { system(\"ls\"); }";
    let q = named_query("objc.sink.system").unwrap();
    let hits = query_source("a.m", src, Language::ObjectiveC, q).expect("query ok");
    assert!(!hits.is_empty(), "objc preset should hit");
}

#[cfg(feature = "lang-json")]
#[test]
fn webext_preset_hit() {
    let src = "{\"host_permissions\": [\"<all_urls>\"]}";
    let q = named_query("webext.manifest.broad_host_permissions").unwrap();
    let hits = query_source("manifest.json", src, Language::Json, q).expect("query ok");
    assert!(!hits.is_empty(), "webext preset should hit");
}
