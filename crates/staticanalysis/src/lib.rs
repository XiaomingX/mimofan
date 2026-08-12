//! mimofan static analysis: tree-sitter based AST retrieval.
//!
//! This crate is the shared AST foundation for SAST capabilities (taint
//! analysis, call graph, data-flow). It is decoupled from `tui` so it can be
//! unit-tested and reused by the runtime crate.

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

pub mod callgraph;

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
pub fn query_source(
    file: &str,
    source: &str,
    lang: Language,
    query: &str,
) -> Result<Vec<AstHit>, AstError> {
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
        // NOTE: Java presets (`java.sink.runtime_exec`, `java.sink.sql_concat`)
        // were removed: their grammars are not compiled (only `lang-rust` is
        // enabled, see Cargo.toml), so invoking them always returns
        // `AstError::Unsupported`. Leaving them in this table implied they were
        // usable and misled the model. Re-add them once the Java/TS/Kotlin/Swift
        // grammars land (tracked in #586 / #715).
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
    fn unsafe_block_is_captured() {
        let q = named_query("rust.unsound.unsafe_block").unwrap();
        let hits = query_source("demo.rs", RUST_SRC, Language::Rust, q).expect("query ok");
        assert!(!hits.is_empty(), "expected at least one unsafe_block hit");
        assert!(hits[0].file == "demo.rs");
        assert!(hits[0].line > 0, "line should be 1-based");
    }

    #[test]
    fn unsupported_language_is_rejected_without_feature() {
        // ObjectiveC grammar is not compiled in (no feature), must error clearly.
        let res = query_source("x.m", "int main(){}", Language::ObjectiveC, "(_) @n");
        assert!(matches!(res, Err(AstError::Unsupported(_))));
    }
}

