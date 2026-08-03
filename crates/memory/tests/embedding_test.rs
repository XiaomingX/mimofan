use mimofan_memory::*;

#[test]
fn test_embedding_config_default() {
    let config = EmbeddingConfig::default();
    assert_eq!(config.api_base_url, "https://api.openai.com/v1");
    assert_eq!(config.model, "text-embedding-3-small");
    assert_eq!(config.dimension, 1536);
}
