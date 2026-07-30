//! Multi-agent result aggregation.
//!
//! Provides strategies for combining results from multiple sub-agents
//! into a single coherent output, with conflict detection and resolution.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Aggregation strategy for combining sub-agent results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AggregationStrategy {
    /// Merge results by key-value fields, preferring later values on conflict.
    Merge,
    /// Take only the first completed result, discard the rest.
    First,
    /// Require a quorum of agents to agree on a value.
    Vote {
        /// Minimum number of agreeing agents required.
        quorum: usize,
    },
    /// Concatenate all results with a separator.
    Concatenate {
        /// Separator string between results.
        separator: String,
    },
    /// Use an LLM prompt to synthesize results.
    LlmAggregate {
        /// The aggregation prompt template.
        prompt: String,
    },
}

/// A conflict between values from different agents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Conflict {
    /// The field or key where the conflict occurs.
    pub field: String,
    /// List of (agent_name, value) pairs that conflict.
    pub values: Vec<(String, String)>,
    /// The resolved value, if resolution has been attempted.
    pub resolution: Option<String>,
}

/// Aggregated result from multiple sub-agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedResult {
    /// The strategy used for aggregation.
    pub strategy: AggregationStrategy,
    /// Input results as (agent_name, result_text) pairs.
    pub inputs: Vec<(String, String)>,
    /// The final aggregated output text.
    pub output: String,
    /// Conflicts detected during aggregation.
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
}

/// Detects conflicts across a set of key-value parsed results.
pub struct ConflictDetector;

impl ConflictDetector {
    /// Detect conflicts across agent results.
    ///
    /// Each result is split into lines of `key: value` form. Lines whose
    /// values differ across agents are reported as conflicts.
    #[must_use]
    pub fn detect(results: &[(String, String)]) -> Vec<Conflict> {
        let mut field_values: HashMap<String, Vec<(String, String)>> = HashMap::new();

        for (agent_name, result) in results {
            for line in result.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim().to_lowercase();
                    let value = value.trim().to_string();
                    if !key.is_empty() && !value.is_empty() {
                        field_values
                            .entry(key)
                            .or_default()
                            .push((agent_name.clone(), value));
                    }
                }
            }
        }

        let mut conflicts = Vec::new();
        for (field, mut entries) in field_values {
            // Deduplicate: only report if there are 2+ distinct values.
            entries.sort_by(|a, b| a.1.cmp(&b.1));
            entries.dedup_by(|a, b| a.1 == b.1);
            if entries.len() > 1 {
                conflicts.push(Conflict {
                    field,
                    values: entries,
                    resolution: None,
                });
            }
        }

        conflicts.sort_by(|a, b| a.field.cmp(&b.field));
        conflicts
    }
}

/// Aggregates results from multiple sub-agents using a configurable strategy.
pub struct ResultAggregator;

impl ResultAggregator {
    /// Aggregate results from multiple sub-agents.
    ///
    /// Returns an [`AggregatedResult`] containing the combined output,
    /// any detected conflicts, and the strategy that was used.
    pub fn aggregate(
        strategy: &AggregationStrategy,
        results: &[(String, String)],
    ) -> AggregatedResult {
        let conflicts = ConflictDetector::detect(results);

        let output = match strategy {
            AggregationStrategy::Merge => Self::merge(results),
            AggregationStrategy::First => Self::first(results),
            AggregationStrategy::Vote { quorum } => Self::vote(results, *quorum),
            AggregationStrategy::Concatenate { separator } => {
                Self::concatenate(results, separator)
            }
            AggregationStrategy::LlmAggregate { .. } => {
                // LLM aggregation requires an external LLM call; the caller
                // is responsible for invoking it and passing the prompt here
                // as a marker. The actual synthesis is done upstream.
                Self::concatenate(results, "\n---\n")
            }
        };

        AggregatedResult {
            strategy: strategy.clone(),
            inputs: results.to_vec(),
            output,
            conflicts,
        }
    }

    /// Merge results by collecting unique non-empty lines from all inputs.
    /// When lines share the same key, the last value wins.
    fn merge(results: &[(String, String)]) -> String {
        let mut merged: Vec<String> = Vec::new();
        for (_name, result) in results {
            for line in result.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Replace existing line with same key prefix.
                if let Some((key, _)) = trimmed.split_once(':') {
                    let key_prefix = format!("{}:", key.trim());
                    merged.retain(|l| !l.starts_with(&key_prefix));
                }
                merged.push(trimmed.to_string());
            }
        }
        merged.join("\n")
    }

    /// Return the first non-empty result.
    fn first(results: &[(String, String)]) -> String {
        results
            .iter()
            .find(|(_, r)| !r.trim().is_empty())
            .map(|(_, r)| r.clone())
            .unwrap_or_default()
    }

    /// Return a result if at least `quorum` agents agree on it.
    ///
    /// Groups exact-match results and picks the largest group that meets
    /// the quorum threshold. Returns an error message if no group qualifies.
    fn vote(results: &[(String, String)], quorum: usize) -> String {
        if results.is_empty() {
            return String::new();
        }

        let mut buckets: HashMap<String, Vec<&str>> = HashMap::new();
        for (_name, result) in results {
            let trimmed = result.trim().to_string();
            if !trimmed.is_empty() {
                buckets.entry(trimmed).or_default().push(_name);
            }
        }

        let mut best: Option<(usize, String)> = None;
        for (value, voters) in &buckets {
            if voters.len() >= quorum {
                let dominated = best.as_ref().is_none_or(|(count, _)| voters.len() > *count);
                if dominated {
                    best = Some((voters.len(), value.clone()));
                }
            }
        }

        match best {
            Some((_, value)) => value,
            None => format!(
                "No consensus reached (quorum={quorum}, agents={})",
                results.len()
            ),
        }
    }

    /// Concatenate all non-empty results with the given separator.
    fn concatenate(results: &[(String, String)], separator: &str) -> String {
        results
            .iter()
            .filter(|(_, r)| !r.trim().is_empty())
            .map(|(_, r)| r.trim().to_string())
            .collect::<Vec<_>>()
            .join(separator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let agg = ResultAggregator::aggregate(
            &AggregationStrategy::Vote { quorum: 2 },
            &results,
        );
        assert_eq!(agg.output, "approve");
    }

    #[test]
    fn test_aggregate_vote_no_consensus() {
        let results = vec![
            ("a".into(), "approve".into()),
            ("b".into(), "reject".into()),
        ];
        let agg = ResultAggregator::aggregate(
            &AggregationStrategy::Vote { quorum: 2 },
            &results,
        );
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
}
