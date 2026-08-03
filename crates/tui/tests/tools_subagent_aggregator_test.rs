// Tests relocated from src/tools/subagent/aggregator.rs (issue #547 Phase 3).

use mimofan::tools::subagent::aggregator::*;

// ── ConflictDetector tests ──────────────────────────────────────

#[test]
fn test_conflict_detect_no_conflict() {
    let results = vec![
        ("agent_a".into(), "status: ok\ncount: 1".into()),
        ("agent_b".into(), "status: ok\ncount: 1".into()),
    ];
    let conflicts = ConflictDetector::detect(&results);
    assert!(conflicts.is_empty());
}

#[test]
fn test_conflict_detect_with_conflict() {
    let results = vec![
        ("agent_a".into(), "status: pass\nscore: 10".into()),
        ("agent_b".into(), "status: fail\nscore: 10".into()),
    ];
    let conflicts = ConflictDetector::detect(&results);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].field, "status");
    assert_eq!(conflicts[0].values.len(), 2);
    assert!(conflicts[0].resolution.is_none());
}

#[test]
fn test_conflict_detect_multiple_conflicts() {
    let results = vec![
        ("a".into(), "x: 1\ny: 2".into()),
        ("b".into(), "x: 1\ny: 3".into()),
        ("c".into(), "x: 4\ny: 2".into()),
    ];
    let conflicts = ConflictDetector::detect(&results);
    // x has values 1 and 4; y has values 2 and 3.
    assert_eq!(conflicts.len(), 2);
    let fields: Vec<&str> = conflicts.iter().map(|c| c.field.as_str()).collect();
    assert!(fields.contains(&"x"));
    assert!(fields.contains(&"y"));
}

#[test]
fn test_conflict_detect_empty_results() {
    let conflicts = ConflictDetector::detect(&[]);
    assert!(conflicts.is_empty());
}

// ── ResultAggregator tests ──────────────────────────────────────

#[test]
fn test_aggregate_merge() {
    let results = vec![
        ("a".into(), "name: alice\nstatus: done".into()),
        ("b".into(), "name: bob\nstatus: running".into()),
    ];
    let agg = ResultAggregator::aggregate(&AggregationStrategy::Merge, &results);
    // Merge keeps last-wins for duplicate keys.
    assert!(agg.output.contains("name: bob"));
    assert!(agg.output.contains("status: running"));
    assert_eq!(agg.strategy, AggregationStrategy::Merge);
    assert_eq!(agg.inputs.len(), 2);
    // status differs => conflict detected
    assert!(!agg.conflicts.is_empty());
}

#[test]
fn test_aggregate_first() {
    let results = vec![
        ("a".into(), "first result".into()),
        ("b".into(), "second result".into()),
    ];
    let agg = ResultAggregator::aggregate(&AggregationStrategy::First, &results);
    assert_eq!(agg.output, "first result");
}

#[test]
fn test_aggregate_first_skips_empty() {
    let results = vec![
        ("a".into(), "  ".into()),
        ("b".into(), "real result".into()),
    ];
    let agg = ResultAggregator::aggregate(&AggregationStrategy::First, &results);
    assert_eq!(agg.output, "real result");
}

#[test]
fn test_aggregate_vote_consensus() {
    let results = vec![
        ("a".into(), "approve".into()),
        ("b".into(), "approve".into()),
        ("c".into(), "reject".into()),
    ];
    let agg = ResultAggregator::aggregate(&AggregationStrategy::Vote { quorum: 2 }, &results);
    assert_eq!(agg.output, "approve");
}

#[test]
fn test_aggregate_vote_no_consensus() {
    let results = vec![
        ("a".into(), "approve".into()),
        ("b".into(), "reject".into()),
    ];
    let agg = ResultAggregator::aggregate(&AggregationStrategy::Vote { quorum: 2 }, &results);
    assert!(agg.output.contains("No consensus"));
}

#[test]
fn test_aggregate_concatenate() {
    let results = vec![
        ("a".into(), "line one".into()),
        ("b".into(), "line two".into()),
    ];
    let agg = ResultAggregator::aggregate(
        &AggregationStrategy::Concatenate {
            separator: " | ".into(),
        },
        &results,
    );
    assert_eq!(agg.output, "line one | line two");
}

#[test]
fn test_aggregate_concatenate_skips_empty() {
    let results = vec![
        ("a".into(), "hello".into()),
        ("b".into(), "  ".into()),
        ("c".into(), "world".into()),
    ];
    let agg = ResultAggregator::aggregate(
        &AggregationStrategy::Concatenate {
            separator: ", ".into(),
        },
        &results,
    );
    assert_eq!(agg.output, "hello, world");
}

#[test]
fn test_aggregate_empty_results() {
    let agg = ResultAggregator::aggregate(&AggregationStrategy::Merge, &[]);
    assert!(agg.output.is_empty());
    assert!(agg.conflicts.is_empty());
    assert!(agg.inputs.is_empty());
}

#[test]
fn test_aggregate_llm_fallback() {
    let results = vec![
        ("a".into(), "result A".into()),
        ("b".into(), "result B".into()),
    ];
    let agg = ResultAggregator::aggregate(
        &AggregationStrategy::LlmAggregate {
            prompt: "summarize".into(),
        },
        &results,
    );
    // Falls back to concatenation with default separator.
    assert_eq!(agg.output, "result A\n---\nresult B");
}
