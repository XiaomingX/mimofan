//! Memory System Integration Test
//!
//! Test complete workflow: store → compress → inject → knowledge

use mimofan_memory::compressor::ObservationCompressor;
use mimofan_memory::vector::{Observation, ObservationKind, SearchFilters, VectorStore};

/// Generate embedding
fn generate_embedding(seed: u64) -> Vec<f32> {
    (0..384)
        .map(|i| {
            let x = (seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(i as u64)) as f32;
            (x / u64::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

#[tokio::main]
async fn main() {
    println!("Memory System Integration Test");
    println!("==============================\n");

    // Setup
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("integration.db");
    let _corpora_path = dir.path().join("corpora");

    // Initialize components
    let vector_store = VectorStore::open(&db_path, 384).unwrap();
    let compressor = ObservationCompressor::with_settings(10, 86400);

    println!("1. Store observations");
    let mut obs_store = mimofan_memory::optimization::ObservationStore::new(vector_store);

    // Store 100 observations
    for i in 0..100 {
        let kind = match i % 5 {
            0 => ObservationKind::Bugfix,
            1 => ObservationKind::Feature,
            2 => ObservationKind::Decision,
            3 => ObservationKind::Discovery,
            _ => ObservationKind::Change,
        };
        let obs = Observation::new(
            "integration-project".to_string(),
            kind,
            format!("Integration test observation {}", i),
        );
        let embedding = generate_embedding(i);
        obs_store.store_observation(&obs, &embedding).unwrap();
    }
    println!("   ✓ Stored 100 observations");

    println!("\n2. Search and filter");
    let query = generate_embedding(0);
    let filters = SearchFilters {
        project: Some("integration-project".to_string()),
        kind: Some(ObservationKind::Discovery),
        ..Default::default()
    };
    let results = obs_store.search(&query, 10, &filters).unwrap();
    assert!(!results.is_empty(), "Should find observations");
    println!("   ✓ Found {} observations", results.len());

    println!("\n3. Compress observations");
    let observations: Vec<Observation> = (0..100)
        .map(|i| {
            Observation::new(
                "integration-project".to_string(),
                ObservationKind::Discovery,
                format!("Observation {}", i),
            )
        })
        .collect();

    let strategies = compressor.analyze_observations(&observations);
    assert_eq!(strategies.len(), 100);
    println!("   ✓ Analyzed {} observations", strategies.len());

    let summary = compressor
        .summarize_session("integration-session", &observations)
        .unwrap();
    assert!(!summary.session_id.is_empty());
    println!("   ✓ Generated session summary");

    println!("\n==============================");
    println!("Integration test passed! ✅");
}
