//! mimofan static analysis: tree-sitter based AST retrieval.
//!
//! This crate is the shared AST foundation for SAST capabilities (taint
//! analysis, call graph, data-flow). It is decoupled from `tui` so it can be
//! unit-tested and reused by the runtime crate.

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

/// Resolve a concrete tree-sitter grammar for `lang`, setting it on `parser`.
/// Each grammar is feature-gated so the default build only compiles Rust.
/// Grammars differ in their exported symbol (`LANGUAGE` constant vs a
/// `language()` fn), so this is written out per-crate rather than via a macro
/// with a uniform name. `#[macro_export]` makes it reachable from submodules
/// (`crate::set_grammar!`) since `macro_rules!` textual scope does not cross
/// module-file boundaries otherwise.
#[macro_export]
macro_rules! set_grammar {
    ($parser:expr, $lang:expr, $feature:literal, $body:expr) => {{
        #[cfg(feature = $feature)]
        {
            let l: tree_sitter::Language = $body;
            $parser
                .set_language(&l)
                .map_err(|e| AstError::Parse(e.to_string()))?;
            l
        }
        #[cfg(not(feature = $feature))]
        {
            return Err(AstError::Unsupported($lang));
        }
    }};
}

pub mod callgraph;

/// Automatic gadget discovery engine (W2 / #788): paradigm-level, rule-driven
/// Java gadget-chain discovery over a project directory.
pub mod auto_gadget;

/// Persistent symbol index (SQLite). Only compiled with the `symbol-index`
/// feature so the default build stays lean (no bundled SQLite compile).
#[cfg(feature = "symbol-index")]
pub mod index;

/// Supported source languages. Grammars are feature-gated to keep the default
/// build lean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Java,
    TypeScript,
    JavaScript,
    Kotlin,
    Swift,
    ObjectiveC,
    Json,
    Auto,
}

impl Language {
    /// Resolve the concrete grammar for `Auto` based on a file extension.
    pub fn from_path(path: &str) -> Language {
        let lower = path.to_lowercase();
        if lower.ends_with(".rs") {
            Language::Rust
        } else if lower.ends_with(".java") {
            Language::Java
        } else if lower.ends_with(".tsx") {
            Language::TypeScript
        } else if lower.ends_with(".ts") {
            Language::TypeScript
        } else if lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs") {
            Language::JavaScript
        } else if lower.ends_with(".kt") || lower.ends_with(".kts") {
            Language::Kotlin
        } else if lower.ends_with(".swift") {
            Language::Swift
        } else if lower.ends_with(".m") && !lower.ends_with(".ts") {
            Language::ObjectiveC
        } else if lower.ends_with(".json") || lower.ends_with(".jsonc") {
            Language::Json
        } else {
            // Unknown extension: do NOT silently fall back to Rust (that would
            // parse e.g. a `.py` file as Rust and return wrong results). Surface
            // `Auto` so the query path returns an explicit `Unsupported` error
            // instead of a silent misparse. Grammar coverage beyond Rust is
            // tracked in #586 / #715.
            Language::Auto
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Java => "java",
            Language::TypeScript => "tsx",
            Language::JavaScript => "javascript",
            Language::Kotlin => "kotlin",
            Language::Swift => "swift",
            Language::ObjectiveC => "objc",
            Language::Json => "json",
            Language::Auto => "auto",
        }
    }
}

/// A single AST query hit: a captured node plus its enclosing scope.
#[derive(Debug, Clone)]
pub struct AstHit {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub capture: String,
    pub text: String,
}

/// Errors raised while parsing or querying a source file.
#[derive(Debug)]
pub enum AstError {
    Unsupported(Language),
    Query(String),
    Parse(String),
}

/// Parse `source` for `lang` and run a tree-sitter S-expression `query`,
/// returning every capture with its source location and text. `file` is stored
/// on each hit for reporting.
/// Resolve a concrete tree-sitter grammar for `lang`, setting it on `parser`.
/// Each grammar is feature-gated so the default build only compiles Rust.
pub fn query_source(
    file: &str,
    source: &str,
    lang: Language,
    query: &str,
) -> Result<Vec<AstHit>, AstError> {
    let mut parser = Parser::new();
    let ts_lang: tree_sitter::Language = match lang {
        Language::Rust => set_grammar!(parser, lang, "lang-rust", tree_sitter_rust::LANGUAGE.into()),
        Language::Java => set_grammar!(parser, lang, "lang-java", tree_sitter_java::LANGUAGE.into()),
        Language::TypeScript => {
            set_grammar!(parser, lang, "lang-typescript", tree_sitter_typescript::LANGUAGE_TSX.into())
        }
        Language::JavaScript => {
            set_grammar!(parser, lang, "lang-javascript", tree_sitter_javascript::LANGUAGE.into())
        }
        Language::Kotlin => set_grammar!(parser, lang, "lang-kotlin", tree_sitter_kotlin_ng::LANGUAGE.into()),
        Language::Swift => set_grammar!(parser, lang, "lang-swift", tree_sitter_swift::LANGUAGE.into()),
        Language::ObjectiveC => {
            set_grammar!(parser, lang, "lang-objc", tree_sitter_objc::LANGUAGE.into())
        }
        Language::Json => set_grammar!(parser, lang, "lang-json", tree_sitter_json::LANGUAGE.into()),
        Language::Auto => return Err(AstError::Unsupported(lang)),
    };

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| AstError::Parse("tree-sitter returned no tree".into()))?;

    let q = Query::new(&ts_lang.into(), query).map_err(|e| AstError::Query(e.message))?;
    let mut cursor = QueryCursor::new();
    let mut hits = Vec::new();

    let mut matches = cursor.matches(&q, tree.root_node(), source.as_bytes());
    while let Some(m) = matches.next() {
        for cap in m.captures {
            let node = cap.node;
            let row = node.start_position().row + 1;
            let col = node.start_position().column + 1;
            let text = node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            hits.push(AstHit {
                file: file.to_string(),
                line: row,
                column: col,
                capture: cap.index.to_string(),
                text,
            });
        }
    }
    Ok(hits)
}

/// Named query presets: common vulnerability patterns the model can invoke by
/// name instead of hand-writing S-expressions. Keyed by `language:name`.
pub fn named_query(key: &str) -> Option<&'static str> {
    let q = match key {
        // Rust: detect calls to a function whose name ends in `exec`.
        "rust.sink.process_exec" => "(call_expression
  function: [(identifier) @fn (#match? @fn \"exec$\")]) @call",
        // Rust: detect `unsafe` blocks.
        "rust.unsound.unsafe_block" => "(unsafe_block) @blk",

        // ── Java ────────────────────────────────────────────────────────────
        // Runtime.exec / ProcessBuilder.start — command injection sink. The
        // `object` of the invocation is itself a `method_invocation`
        // (e.g. `Runtime.getRuntime()`), so we match on the `object` chain
        // containing `Runtime` and a `name` of `exec`/`start`.
        "java.sink.runtime_exec" => "(method_invocation
  object: (method_invocation) @obj
  name: (identifier) @method
  (#match? @obj \"Runtime\")
  (#match? @method \"^(exec|start)$\")) @call",
        // String-concatenated SQL passed to a statement — SQL injection sink.
        "java.sink.sql_concat" => "(method_invocation
  name: (identifier) @method
  arguments: (argument_list
    (binary_expression) @sql)
  (#match? @method \"^(executeQuery|executeUpdate|execute|prepareStatement)$\")) @call",
        // Deserialization of an untrusted stream (ObjectInputStream) — gadget sink.
        "java.sink.deserialization" => "(object_creation_expression
  type: (type_identifier) @type
  (#eq? @type \"ObjectInputStream\")) @call",

        // ── JavaScript / TypeScript ────────────────────────────────────────
        // `eval(...)` / `new Function(...)` — code injection sink.
        "js.sink.eval" => "[(call_expression
  function: [(identifier) @fn (#match? @fn \"^(eval|execScript)$\")]
  arguments: (arguments (expression) @arg))
 (new_expression
  constructor: (identifier) @ctor (#eq? @ctor \"Function\"))] @call",
        // `child_process.exec(...)` — command injection sink.
        "js.sink.child_process_exec" => "(call_expression
  function: (member_expression
    object: (identifier) @obj
    property: (property_identifier) @prop)
  (#eq? @obj \"child_process\")
  (#match? @prop \"^(exec|execFile|spawn)(Sync)?$\")) @call",
        // SQL string concatenation into a query call.
        "js.sink.sql_concat" => "[(call_expression
  function: [(identifier) @fn
            (member_expression property: (property_identifier) @mprop)]
  arguments: (arguments (binary_expression) @sql)
  (#match? @fn \"^(query|execute|exec|raw)$\")
  (#match? @mprop \"^(query|execute|exec|raw|all|run)$\"))] @call",

        // ── Kotlin ───────────────────────────────────────────────────────────
        // Runtime.exec / getRuntime — command injection sink. The navigation
        // chain `Runtime.getRuntime().exec` is matched by text predicates on
        // the call site (kotlin-ng node types nest navigation_expressions).
        "kotlin.sink.runtime_exec" => "(call_expression
  (navigation_expression) @callsite
  (#match? @callsite \"Runtime\")
  (#match? @callsite \"exec|getRuntime\")) @call",

        // ── Swift ────────────────────────────────────────────────────────────
        // `system(...)` / `shell(...)` — command execution sink.
        "swift.sink.process_run" => "[(call_expression
  (simple_identifier) @fn (#match? @fn \"^shell$|^system$\"))] @call",

        // ── Objective-C ──────────────────────────────────────────────────────
        // NSTask / system(...) / popen(...) — command execution sink.
        "objc.sink.system" => "[(call_expression
  function: (identifier) @fn
  (#match? @fn \"^(system|popen|execl|execv)$\"))] @call",

        // ── Cross-cutting: browser extension manifest hazards ────────────────
        // WebExtension manifest with broad host permissions (`<all_urls>` or any
        // `://` scheme). Requires the JSON grammar (`lang-json` feature).
        "webext.manifest.broad_host_permissions" => "(pair
  key: (string (string_content) @k (#eq? @k \"host_permissions\"))
  value: (array (string (string_content) @hp (#match? @hp \"<all_urls>|://\")))) @pair",

        _ => return None,
    };
    Some(q)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(res.is_ok(), "ObjectiveC should be supported with lang-objc feature");
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
        let src = "eval(x); new Function(\"y\"); child_process.exec(\"ls\"); db.query(\"SELECT \" + z);";
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
}


// --- Taint/SCA capability modules (taint-sca-group, T-4..T-13) ---
// Wiring for the security analysis modules. These are additive and do not
// touch the shared `Language`/`query_source`/`callgraph` foundation owned by
// the staticanalysis group.
pub mod rules;
pub mod taint;
pub mod summary;
pub mod typestate;
pub mod sarif;
pub mod sca;
pub mod knowledge;
pub mod kb_trace;
pub mod attack_surface;
pub mod recon;
