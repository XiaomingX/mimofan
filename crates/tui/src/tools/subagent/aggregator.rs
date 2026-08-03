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
            AggregationStrategy::Concatenate { separator } => Self::concatenate(results, separator),
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
