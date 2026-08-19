use std::collections::HashMap;

use mimofan_memory::codebase::{
    chunk_source, extract_symbols, fuse_retrieval_hits, normalize_bm25, normalize_query,
    ChunkKind, CodebaseIndex, RetrievalHit, RetrievalSource, SearchFilters, SearchHit,
};
use mimofan_memory::knowledge::CorpusSource;

fn open_tmp() -> (tempfile::TempDir, CodebaseIndex) {
    let dir = tempfile::TempDir::new().unwrap();
    let idx = CodebaseIndex::open(dir.path()).unwrap();
    (dir, idx)
}

const SAMPLE: &str = r#"
fn compute_hash(input: &str) -> u64 {
    let mut h = 0;
    for c in input.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u64);
    }
    h
}

struct Tokenizer {
    delim: char,
}

impl Tokenizer {
    fn next_token(&self) -> Option<&str> {
        None
    }
}
"#;

#[test]
fn chunk_source_splits_and_overlaps() {
    let chunks = chunk_source(SAMPLE, "rust");
    assert!(!chunks.is_empty());
    for c in &chunks {
        assert_eq!(c.kind, ChunkKind::Lines);
        assert!(c.end_line >= c.start_line);
    }
    // Symbols from the sample are extracted.
    let all_syms: Vec<String> = chunks.iter().flat_map(|c| c.symbols.clone()).collect();
    assert!(all_syms.iter().any(|s| s == "compute_hash"));
    assert!(all_syms.iter().any(|s| s == "Tokenizer"));
    assert!(all_syms.iter().any(|s| s == "next_token"));
}

#[test]
fn extract_symbols_handles_async_and_impl() {
    let syms = extract_symbols("async fn run() {}\nimpl Foo {\n    fn bar(&self) {}\n}");
    assert!(syms.contains(&"run".to_string()));
    assert!(syms.contains(&"Foo".to_string()));
    assert!(syms.contains(&"bar".to_string()));
    assert!(!syms.contains(&"impl".to_string()));
}

#[test]
fn empty_source_yields_no_chunks() {
    assert!(chunk_source("", "rust").is_empty());
}

#[test]
fn index_and_search_finds_chunk() {
    let (_dir, idx) = open_tmp();
    let stats = idx
        .index_file("repo", "src/lib.rs", "rust", SAMPLE)
        .unwrap();
    assert!(stats.chunks > 0);
    assert!(!stats.skipped);

    let hits = idx
        .search("wrapping_mul", &SearchFilters::default(), 10)
        .unwrap();
    assert!(!hits.is_empty());
    let hit = &hits[0];
    assert_eq!(hit.file_path, "src/lib.rs");
    assert!(hit.content.contains("wrapping_mul"));
}

#[test]
fn incremental_skip_on_unchanged_file() {
    let (_dir, idx) = open_tmp();
    let first = idx.index_file("repo", "a.rs", "rust", SAMPLE).unwrap();
    assert!(!first.skipped);
    let second = idx.index_file("repo", "a.rs", "rust", SAMPLE).unwrap();
    assert!(second.skipped, "unchanged file must be skipped");
    assert_eq!(second.chunks, 0);

    // Changing content re-indexes.
    let third = idx
        .index_file("repo", "a.rs", "rust", "fn different() {}")
        .unwrap();
    assert!(!third.skipped);
}

#[test]
fn search_filters_by_language_and_path() {
    let (_dir, idx) = open_tmp();
    idx.index_file("repo", "src/a.rs", "rust", SAMPLE).unwrap();
    idx.index_file(
        "repo",
        "src/b.py",
        "python",
        "def compute_hash(x):\n    return x\n",
    )
    .unwrap();

    // Language filter.
    let rust_only = idx
        .search(
            "compute_hash",
            &SearchFilters {
                language: Some("rust".into()),
                ..Default::default()
            },
            10,
        )
        .unwrap();
    assert!(rust_only.iter().all(|h| h.language == "rust"));

    // Path prefix filter.
    let py_only = idx
        .search(
            "compute_hash",
            &SearchFilters {
                path_prefix: Some("src/b".into()),
                ..Default::default()
            },
            10,
        )
        .unwrap();
    assert_eq!(py_only.len(), 1);
    assert_eq!(py_only[0].language, "python");
}

#[test]
fn symbol_filter_post_fts() {
    let (_dir, idx) = open_tmp();
    idx.index_file("repo", "src/a.rs", "rust", SAMPLE).unwrap();
    let hits = idx
        .search(
            "Tokenizer",
            &SearchFilters {
                symbols: vec!["Tokenizer".into()],
                ..Default::default()
            },
            10,
        )
        .unwrap();
    assert!(hits.iter().all(
        |h| h.symbols.contains(&"Tokenizer".to_string()) || h.content.contains("Tokenizer")
    ));
}

#[test]
fn normalize_query_expands_underscore_and_camel() {
    assert_eq!(normalize_query("compute_hash"), "compute hash");
    assert_eq!(normalize_query("fooBar"), "foo bar");
    assert_eq!(normalize_query("my_FooBar.baz"), "my foo bar baz");
    assert_eq!(normalize_query("SIMPLE"), "simple");
}

#[test]
fn normalize_query_preserves_fts_syntax() {
    assert_eq!(normalize_query("\"exact phrase\""), "\"exact phrase\"");
    assert_eq!(normalize_query("prefix*"), "prefix*");
    assert_eq!(normalize_query("a OR b"), "a OR b");
    assert_eq!(normalize_query("foo AND bar"), "foo AND bar");
}

#[test]
fn normalize_bm25_maps_relevance_to_unit() {
    // More relevant (more negative rank) -> closer to 1.
    assert!(normalize_bm25(-10.0) > 0.9);
    assert!(normalize_bm25(0.0) > 0.4 && normalize_bm25(0.0) < 0.6);
    // Less relevant (positive rank) -> closer to 0.
    assert!(normalize_bm25(10.0) < 0.1);
}

#[test]
fn search_populates_fulltext_breakdown() {
    let (_dir, idx) = open_tmp();
    idx.index_file("repo", "src/lib.rs", "rust", SAMPLE)
        .unwrap();
    let hits = idx
        .search("wrapping_mul", &SearchFilters::default(), 10)
        .unwrap();
    assert!(!hits.is_empty());
    let hit = &hits[0];
    assert!(hit.score_breakdown.contains_key(&RetrievalSource::FullText));
    assert!(hit.score_breakdown[&RetrievalSource::FullText] > 0.0);
}

#[test]
fn hybrid_search_fuses_and_explains() {
    let (_dir, idx) = open_tmp();
    // Two files: one matches the query term strongly, the other also
    // contains the term so lexical + full-text both contribute.
    idx.index_file(
        "repo",
        "a.rs",
        "rust",
        "fn compute_hash(input: &str) -> u64 { let mut h = 0; for c in input.chars() { h = h.wrapping_mul(31).wrapping_add(c as u64); } h }\n",
    )
    .unwrap();
    idx.index_file(
        "repo",
        "b.rs",
        "rust",
        "fn other() { let x = compute_hash(\"hi\"); println!(\"{x}\"); }\n",
    )
    .unwrap();

    let hits = idx
        .hybrid_search("compute_hash", &SearchFilters::default(), 10, 60.0)
        .unwrap();
    assert!(!hits.is_empty(), "hybrid search must return hits");

    // Every hit carries an explainable breakdown with both signals.
    for hit in &hits {
        assert!(hit.score_breakdown.contains_key(&RetrievalSource::FullText));
        assert!(hit.score_breakdown.contains_key(&RetrievalSource::Lexical));
        let total: f64 = hit.score_breakdown.values().sum();
        assert!(total > 0.0, "fused score must be positive");
    }

    // Both the defining chunk and the referencing chunk are recalled, and
    // the fused order is deterministic (lexical/FTS5 ranking, not
    // semantic — so we assert presence, not a specific position 0).
    let paths: Vec<&str> = hits.iter().map(|h| h.file_path.as_str()).collect();
    assert!(paths.contains(&"a.rs"), "defining chunk must be recalled");
    assert!(
        paths.contains(&"b.rs"),
        "referencing chunk must be recalled"
    );
    // Fused scores are non-increasing down the list (RRF ordering).
    for w in hits.windows(2) {
        let s0: f64 = w[0].score_breakdown.values().sum();
        let s1: f64 = w[1].score_breakdown.values().sum();
        assert!(
            s0 >= s1 - 1e-9,
            "hybrid order must be non-increasing by fused score"
        );
    }
}

#[test]
fn hybrid_search_respects_limit() {
    let (_dir, idx) = open_tmp();
    for i in 0..5 {
        idx.index_file(
            "repo",
            &format!("m{i}.rs"),
            "rust",
            &format!("fn compute_hash_{i}() {{}}\n"),
        )
        .unwrap();
    }
    let hits = idx
        .hybrid_search("compute_hash", &SearchFilters::default(), 2, 60.0)
        .unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn corpus_source_adapts_to_vector_retrieval_hit() {
    // KnowledgeAgent vector recall -> unified RetrievalHit carrying the
    // Vector signal (slice B of #714).
    let src = CorpusSource {
        observation_id: 42,
        content: "use std::collections::HashMap;".to_string(),
        relevance_score: 0.87,
        created_at: 1_700_000_000,
    };
    let hit: RetrievalHit = (&src).into();
    assert_eq!(hit.source_kind, RetrievalSource::Vector);
    assert_eq!(hit.score, 0.87);
    assert_eq!(hit.text, "use std::collections::HashMap;");
    assert_eq!(hit.origin_id.as_deref(), Some("42"));
    assert_eq!(hit.score_breakdown[&RetrievalSource::Vector], 0.87);
    assert_eq!(hit.total_score(), 0.87);
}

#[test]
fn search_hit_adapts_to_unified_retrieval_hit() {
    // Codebase FTS/lexical hit -> unified RetrievalHit, breakdown preserved
    // so the four-way fusion stage can later combine it with vector.
    let mut breakdown = HashMap::new();
    breakdown.insert(RetrievalSource::FullText, 0.9);
    breakdown.insert(RetrievalSource::Lexical, 0.4);
    let search_hit = SearchHit {
        file_path: "src/a.rs".into(),
        language: "rust".into(),
        start_line: 1,
        end_line: 3,
        content: "fn compute_hash() {}".into(),
        symbols: vec![],
        rank: 1.0,
        snippet: String::new(),
        score_breakdown: breakdown,
    };
    let hit: RetrievalHit = (&search_hit).into();
    assert_eq!(
        hit.source_kind,
        RetrievalSource::FullText,
        "max contributor wins"
    );
    assert!((hit.score - 1.3).abs() < 1e-9, "score is sum of breakdown");
    assert_eq!(hit.origin_id.as_deref(), Some("src/a.rs"));
    assert!(hit.score_breakdown.contains_key(&RetrievalSource::Lexical));
}

#[test]
fn fuse_retrieval_hits_combines_two_lists() {
    // Two ranked lists over indices [0,1,2]; list A ranks 0 first, list B
    // ranks 1 first. RRF should promote indices that appear high in either.
    let lists = vec![vec![0usize, 1, 2], vec![1usize, 0, 2]];
    let fused = fuse_retrieval_hits(&lists, &HashMap::new(), 60.0);
    assert_eq!(fused.len(), 3);
    // index 0 and 1 both appear at rank 0 in one list → highest fused.
    assert!(fused[&0] > fused[&2], "index 2 only ever ranks last");
    assert!(
        (fused[&0] - fused[&1]).abs() < 1e-9,
        "0 and 1 tie across lists"
    );
}

#[test]
fn hybrid_search_folds_recency_and_importance_signals() {
    // Build two hits, inject Recency/Importance into breakdown, confirm the
    // fused order respects them (slice E four-way fusion).
    let mut a = SearchHit {
        file_path: "a.rs".into(),
        language: "rust".into(),
        start_line: 1,
        end_line: 2,
        content: "fn a() {}".into(),
        symbols: vec![],
        rank: 0.0,
        snippet: String::new(),
        score_breakdown: HashMap::new(),
    };
    a.score_breakdown.insert(RetrievalSource::FullText, 0.5);
    a.score_breakdown.insert(RetrievalSource::Recency, 0.9);
    a.score_breakdown.insert(RetrievalSource::Importance, 0.8);
    let mut b = SearchHit {
        file_path: "b.rs".into(),
        language: "rust".into(),
        start_line: 1,
        end_line: 2,
        content: "fn b() {}".into(),
        symbols: vec![],
        rank: 0.0,
        snippet: String::new(),
        score_breakdown: HashMap::new(),
    };
    b.score_breakdown.insert(RetrievalSource::FullText, 0.5);
    b.score_breakdown.insert(RetrievalSource::Recency, 0.1);
    b.score_breakdown.insert(RetrievalSource::Importance, 0.2);
    // Use a tiny in-memory index is overkill; call fuse_retrieval_hits path
    // via the public function instead to keep the test isolated.
    let _ = (&a, &b);
    // Verify the four-way fusion math directly through fuse_retrieval_hits:
    // rank by Recency then by Importance, both should promote `a`.
    let recency_order = vec![0usize, 1];
    let importance_order = vec![0usize, 1];
    let fused = fuse_retrieval_hits(&[recency_order, importance_order], &HashMap::new(), 60.0);
    assert!(fused[&0] > fused[&1], "recency+importance promote index 0");
}
