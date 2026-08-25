// The symbol index module is only compiled with the `symbol-index` feature.
// Under the default feature set this test file is not compiled; run with:
//   cargo test -p mimofan-staticanalysis --features symbol-index --test index_test
#[cfg(feature = "symbol-index")]
mod symbol_index_tests {
    use std::path::Path;

    use mimofan_staticanalysis::Language;
    use mimofan_staticanalysis::index::SymbolIndex;

    const RUST_SRC: &str = r#"
use std::process;
use std::collections::HashMap;

fn main() {
    helper();
}

fn helper() {
    leaf();
}

fn leaf() {}
"#;

    #[test]
    fn index_and_query_roundtrip() {
        let mut idx = SymbolIndex::open(Path::new(":memory:")).expect("open");
        let tmp = std::env::temp_dir().join(format!("mimofan_idx_test_{}.rs", std::process::id()));
        std::fs::write(&tmp, RUST_SRC).expect("write");

        let changed = idx
            .index_file(&tmp, Language::Rust, Some(RUST_SRC))
            .expect("index");
        assert!(changed, "first index must report changed");

        // Second index with identical content/mtime is a no-op.
        let changed2 = idx
            .index_file(&tmp, Language::Rust, Some(RUST_SRC))
            .expect("index2");
        assert!(!changed2, "unchanged file must be skipped (incremental)");

        let syms = idx.find_symbols("helper").expect("find");
        assert!(syms.iter().any(|(_, n, _)| n == "helper"));

        let importers = idx.find_importers("collections").expect("importers");
        assert!(
            importers
                .iter()
                .any(|p| p == tmp.to_string_lossy().as_ref())
        );

        let refs = idx.find_references("leaf").expect("refs");
        assert!(!refs.is_empty(), "leaf must be referenced from helper");

        std::fs::remove_file(&tmp).ok();
    }
}
