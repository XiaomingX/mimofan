use mimofan_memory::*;

#[test]
fn test_knowledge_corpus_serialization() {
    let corpus = KnowledgeCorpus {
        name: "test-corpus".to_string(),
        description: "Test corpus".to_string(),
        project: "test-project".to_string(),
        observation_count: 10,
        concepts: vec!["test".to_string()],
        created_at: 1000,
        updated_at: 2000,
    };

    let json = serde_json::to_string(&corpus).expect("serialize knowledge corpus");
    let deserialized: KnowledgeCorpus =
        serde_json::from_str(&json).expect("parse knowledge corpus json");

    assert_eq!(deserialized.name, corpus.name);
    assert_eq!(deserialized.observation_count, corpus.observation_count);
}

#[test]
fn test_corpus_answer_serialization() {
    let answer = CorpusAnswer {
        answer: "Test answer".to_string(),
        sources: vec![CorpusSource {
            observation_id: 1,
            content: "Test content".to_string(),
            relevance_score: 0.95,
            created_at: 1000,
        }],
        confidence: 0.95,
    };

    let json = serde_json::to_string(&answer).expect("serialize corpus answer");
    let deserialized: CorpusAnswer =
        serde_json::from_str(&json).expect("parse corpus answer json");

    assert_eq!(deserialized.answer, answer.answer);
    assert_eq!(deserialized.confidence, answer.confidence);
}
