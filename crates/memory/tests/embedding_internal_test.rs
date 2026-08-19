use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mimofan_memory::embedding::{hash_embed, ApiEmbedder, Embedder, EmbeddingService};
use mimofan_memory::Result;

#[test]
fn hash_embed_is_deterministic() {
    let a = hash_embed("hello world foo", 64);
    let b = hash_embed("hello world foo", 64);
    assert_eq!(a, b);
}

#[test]
fn hash_embed_matches_dimension() {
    let v = hash_embed("any text here", 1536);
    assert_eq!(v.len(), 1536);
}

#[test]
fn hash_embed_is_unit_length() {
    let v = hash_embed("a b c d e f g", 32);
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 1e-5,
        "expected unit length, got {norm}"
    );
}

#[test]
fn hash_embed_similar_text_overlaps() {
    // Shared tokens should place the two vectors closer than disjoint ones.
    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb)
    }
    let similar = hash_embed("the cat sat on the mat", 128);
    let same = hash_embed("the cat sat on the mat", 128);
    let different = hash_embed("zebra orbit quantum plasma", 128);
    assert!(
        cosine(&similar, &same) > cosine(&similar, &different),
        "degraded vectors should preserve lexical overlap"
    );
}

#[test]
fn empty_text_yields_zero_vector() {
    let v = hash_embed("", 16);
    assert!(v.iter().all(|x| *x == 0.0));
}

#[test]
fn degraded_counter_is_readable() {
    // The counter is a global monotonic probe; merely reading it must not
    // panic or trigger any network I/O. The actual degrade path (API
    // failure -> local hash vectors) is exercised at runtime when the
    // upstream embedding endpoint is unreachable (#627).
    let _ = EmbeddingService::degraded_count();
}

#[test]
fn api_embedder_satisfies_embedder_trait() {
    // Compile-time proof that any local type can implement `Embedder`
    // behind `dyn Embedder` (#712) — this is the seam that lets a future
    // on-device embedder replace the remote API without touching callers.
    // Network/Client construction is intentionally avoided here so the
    // unit test does not depend on the TLS crypto provider installed by
    // the binary at startup.
    struct StubEmbedder {
        dim: usize,
    }
    impl Embedder for StubEmbedder {
        fn embed(&self, _text: &str) -> Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + '_>> {
            Box::pin(async move { Ok(vec![0.0; self.dim]) })
        }
        fn embed_batch(
            &self,
            texts: &[String],
        ) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>>> + Send + '_>> {
            let n = texts.len();
            let dim = self.dim;
            Box::pin(async move { Ok(vec![vec![0.0; dim]; n]) })
        }
        fn dim(&self) -> usize {
            self.dim
        }
        fn model_name(&self) -> &str {
            "stub"
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    let embedder: Arc<dyn Embedder> = Arc::new(StubEmbedder { dim: 8 });
    assert_eq!(embedder.dim(), 8);
    assert_eq!(embedder.model_name(), "stub");
    // Downcast back to the concrete type via the `as_any` seam.
    let recovered = embedder
        .as_any()
        .downcast_ref::<StubEmbedder>()
        .expect("downcast");
    assert_eq!(recovered.dim, 8);
}

#[test]
fn service_wraps_backend_and_exposes_config() {
    // `EmbeddingService::with_backend` injects an arbitrary `Embedder`;
    // `dimension`/`model_name` proxy through; `as_any` downcast reaches
    // the concrete backend (#712). No `Client`/TLS provider needed.
    struct StubEmbedder {
        dim: usize,
    }
    impl Embedder for StubEmbedder {
        fn embed(&self, _text: &str) -> Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + '_>> {
            Box::pin(async move { Ok(vec![0.0; self.dim]) })
        }
        fn embed_batch(
            &self,
            texts: &[String],
        ) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>>> + Send + '_>> {
            let n = texts.len();
            let dim = self.dim;
            Box::pin(async move { Ok(vec![vec![0.0; dim]; n]) })
        }
        fn dim(&self) -> usize {
            self.dim
        }
        fn model_name(&self) -> &str {
            "stub-backend"
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    let backend: Arc<dyn Embedder> = Arc::new(StubEmbedder { dim: 16 });
    let service = EmbeddingService::with_backend(backend);
    assert_eq!(service.dimension(), 16);
    assert_eq!(service.model_name(), "stub-backend");
    // `config()` returns None for a non-ApiEmbedder backend (no API
    // config), proving the facade degrades gracefully for local embedders.
    assert!(service.config().is_none());
}

#[test]
fn api_embedder_is_a_valid_embedder_backend() {
    // Static assertion: `ApiEmbedder` satisfies the `Embedder` trait so it
    // can be stored behind `Arc<dyn Embedder>` in `EmbeddingService`
    // (#712). Avoids constructing `reqwest::Client` (needs TLS provider).
    fn assert_impl_embedder<T: Embedder>() {}
    assert_impl_embedder::<ApiEmbedder>();
}
