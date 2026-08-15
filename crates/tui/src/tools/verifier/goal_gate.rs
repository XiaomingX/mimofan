//! Objective goal-gate verifier (#849).
//!
//! `GoalGate` decides whether an objective is actually met using *observable*
//! signals, not the model's self-report. The engine populates a
//! [`GoalEvidence`] struct with what it can actually observe (files changed
//! vs. expected, tests passing, a user-supplied success predicate command that
//! must exit 0, a required output substring) and `GoalGate::evaluate` folds
//! those signals into a [`GateVerdict`].
//!
//! This module is deliberately pure and side-effect free except for the
//! optional predicate command, which is run through `std::process::Command`
//! directly (no shell, no LLM call). Every gate behind the [`GoalVerifier`]
//! trait is unit-testable.

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Outcome of evaluating a goal against objective evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateVerdict {
    /// Whether the objective is judged met by the available evidence.
    pub met: bool,
    /// Confidence in the verdict, in `[0.0, 1.0]`.
    ///
    /// 1.0 means the evidence fully and deterministically establishes the
    /// outcome (e.g. a passing predicate command); lower values reflect
    /// partial or circumstantial signals.
    pub confidence: f32,
    /// Human-readable reason, suitable for surfacing to the model/user.
    pub reason: String,
}

impl GateVerdict {
    /// A verdict that the objective is met with full confidence.
    #[must_use]
    pub fn met(reason: impl Into<String>) -> Self {
        Self {
            met: true,
            confidence: 1.0,
            reason: reason.into(),
        }
    }

    /// A verdict that the objective is not met with full confidence.
    #[must_use]
    pub fn unmet(reason: impl Into<String>) -> Self {
        Self {
            met: false,
            confidence: 1.0,
            reason: reason.into(),
        }
    }

    /// A verdict with an explicit confidence, used for partial signals.
    #[must_use]
    pub fn with_confidence(met: bool, confidence: f32, reason: impl Into<String>) -> Self {
        Self {
            met,
            confidence: confidence.clamp(0.0, 1.0),
            reason: reason.into(),
        }
    }
}

/// Observable signals the engine can populate to judge a goal.
///
/// Every field is optional: a gate only requires the signals it cares about.
/// The engine is expected to fill in whatever it can observe; gates that
/// depend on absent signals report low-confidence or `unmet` rather than
/// guessing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalEvidence {
    /// Files the engine observed as changed (paths, relative or absolute).
    #[serde(default)]
    pub files_changed: Vec<PathBuf>,
    /// Files the objective *expected* to change. When non-empty, a goal is
    /// only satisfied if every expected file appears in `files_changed`.
    #[serde(default)]
    pub expected_files: Vec<PathBuf>,
    /// Whether the relevant test suite passed. `None` when unknown.
    #[serde(default)]
    pub tests_passing: Option<bool>,
    /// A user-supplied predicate command that must exit 0 for the goal to be
    /// met. Run directly (no shell) by [`PredicateGate`].
    #[serde(default)]
    pub success_predicate: Option<String>,
    /// A substring that must be present in the objective's produced output.
    #[serde(default)]
    pub required_output_substring: Option<String>,
    /// The observed output text the objective produced (e.g. tool stdout).
    #[serde(default)]
    pub observed_output: Option<String>,
}

impl GoalEvidence {
    /// Whether every `expected_files` entry is present in `files_changed`.
    #[must_use]
    pub fn all_expected_files_changed(&self) -> bool {
        self.expected_files
            .iter()
            .all(|expected| self.files_changed.contains(expected))
    }

    /// Count of `expected_files` entries missing from `files_changed`.
    #[must_use]
    pub fn missing_expected_files(&self) -> Vec<PathBuf> {
        self.expected_files
            .iter()
            .filter(|expected| !self.files_changed.contains(expected))
            .cloned()
            .collect()
    }
}

/// A single objective verifier behind the goal gate.
///
/// Each implementation inspects the [`GoalEvidence`] and returns a
/// [`GateVerdict`]. `GoalGate` combines them. Implementations must be pure
/// except where explicitly documented (e.g. [`PredicateGate`] runs a command).
pub trait GoalVerifier {
    /// Human-readable name of this verifier (used in reasons).
    fn name(&self) -> &str;

    /// Evaluate the objective against the evidence.
    fn evaluate(&self, objective: &str, evidence: &GoalEvidence) -> GateVerdict;
}

/// Runs a user-supplied command and requires it to exit 0 for the goal to be
/// met.
///
/// The command is executed directly (`std::process::Command`) without a shell,
/// so it should be a full program path or name plus args. The working
/// directory is the engine-provided `cwd` (default: current dir). A non-zero
/// exit, spawn failure, or missing predicate all produce an `unmet` verdict.
pub struct PredicateGate {
    cwd: Option<PathBuf>,
}

impl PredicateGate {
    /// Create a predicate gate that runs commands in the current directory.
    #[must_use]
    pub fn new() -> Self {
        Self { cwd: None }
    }

    /// Create a predicate gate that runs commands in `cwd`.
    #[must_use]
    pub fn with_cwd(cwd: PathBuf) -> Self {
        Self { cwd: Some(cwd) }
    }
}

impl Default for PredicateGate {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalVerifier for PredicateGate {
    fn name(&self) -> &str {
        "predicate"
    }

    fn evaluate(&self, objective: &str, evidence: &GoalEvidence) -> GateVerdict {
        let Some(predicate) = &evidence.success_predicate else {
            return GateVerdict::with_confidence(
                false,
                0.0,
                format!(
                    "predicate gate for objective '{}': no success_predicate supplied",
                    objective
                ),
            );
        };

        // Split the predicate into program + args on ASCII whitespace. The
        // engine hands us a single command line; we deliberately do NOT use a
        // shell so the predicate cannot inject redirects/operators.
        let parts: Vec<&str> = predicate.split_whitespace().collect();
        if parts.is_empty() {
            return GateVerdict::unmet(format!(
                "predicate gate for objective '{}': success_predicate is empty",
                objective
            ));
        }

        let mut command = Command::new(parts[0]);
        command.args(&parts[1..]);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }

        match command.status() {
            Ok(status) if status.success() => GateVerdict::met(format!(
                "predicate gate passed: '{}' exited 0",
                predicate
            )),
            Ok(status) => GateVerdict::unmet(format!(
                "predicate gate failed: '{}' exited with status {}",
                predicate,
                status
            )),
            Err(err) => GateVerdict::unmet(format!(
                "predicate gate could not run '{}': {}",
                predicate, err
            )),
        }
    }
}

/// Requires a specific substring to be present in the observed output.
///
/// Pure: only inspects `evidence.required_output_substring` and
/// `evidence.observed_output`. Substring matching is case-sensitive; if a
/// case-insensitive match is needed the engine should normalise before
/// populating the evidence.
pub struct SubstringGate;

impl GoalVerifier for SubstringGate {
    fn name(&self) -> &str {
        "substring"
    }

    fn evaluate(&self, objective: &str, evidence: &GoalEvidence) -> GateVerdict {
        let Some(needle) = &evidence.required_output_substring else {
            return GateVerdict::with_confidence(
                false,
                0.0,
                format!(
                    "substring gate for objective '{}': no required_output_substring supplied",
                    objective
                ),
            );
        };

        match &evidence.observed_output {
            Some(haystack) if haystack.contains(needle) => {
                GateVerdict::met(format!("substring '{}' found in observed output", needle))
            }
            Some(_) => GateVerdict::unmet(format!(
                "substring '{}' not found in observed output",
                needle
            )),
            None => GateVerdict::with_confidence(
                false,
                0.0,
                format!(
                    "substring gate for objective '{}': no observed_output to search",
                    objective
                ),
            ),
        }
    }
}

/// The objective goal gate.
///
/// Combines a set of [`GoalVerifier`]s into a single verdict. `GoalGate` is
/// constructed with the gates it should run; `evaluate` folds their verdicts
/// with the following policy:
///
/// - If any gate returns a *deterministic* `met` verdict (`confidence == 1.0`),
///   the goal is judged met (the strongest single objective signal wins).
/// - Otherwise, if any gate returns a deterministic `unmet` verdict, the goal
///   is not met.
/// - Otherwise (only partial/low-confidence signals, or no gates), the gate
///   reports `unmet` with the highest-confidence partial reason, since absence
///   of objective proof means the goal cannot be trusted as met.
pub struct GoalGate {
    verifiers: Vec<Box<dyn GoalVerifier>>,
}

impl GoalGate {
    /// Create a goal gate with the default verifier set: [`PredicateGate`]
    /// and [`SubstringGate`].
    #[must_use]
    pub fn default_set() -> Self {
        Self {
            verifiers: vec![Box::new(PredicateGate::new()), Box::new(SubstringGate)],
        }
    }

    /// Create an empty goal gate (caller adds verifiers via [`GoalGate::add`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            verifiers: Vec::new(),
        }
    }

    /// Add a verifier to the gate.
    pub fn add(&mut self, verifier: Box<dyn GoalVerifier>) {
        self.verifiers.push(verifier);
    }

    /// Evaluate the objective, folding each verifier's verdict.
    #[must_use]
    pub fn evaluate(&self, objective: &str, evidence: &GoalEvidence) -> GateVerdict {
        if self.verifiers.is_empty() {
            return GateVerdict::with_confidence(
                false,
                0.0,
                format!(
                    "goal gate for objective '{}': no verifiers configured; cannot judge objectively",
                    objective
                ),
            );
        }

        let mut best_partial: Option<GateVerdict> = None;

        for verifier in &self.verifiers {
            let verdict = verifier.evaluate(objective, evidence);
            if verdict.confidence >= 1.0 {
                if verdict.met {
                    return GateVerdict::met(format!(
                        "[{}] {}",
                        verifier.name(),
                        verdict.reason
                    ));
                }
                // A deterministic "not met" still needs to lose to a later
                // deterministic "met", so we remember it but keep scanning.
                if best_partial.as_ref().map_or(true, |p| p.confidence < 1.0) {
                    best_partial = Some(GateVerdict::unmet(format!(
                        "[{}] {}",
                        verifier.name(),
                        verdict.reason
                    )));
                }
            } else if best_partial.as_ref().map_or(true, |p| {
                p.confidence < verdict.confidence
            }) {
                best_partial = Some(GateVerdict::with_confidence(
                    verdict.met,
                    verdict.confidence,
                    format!("[{}] {}", verifier.name(), verdict.reason),
                ));
            }
        }

        best_partial.unwrap_or_else(|| {
            GateVerdict::with_confidence(
                false,
                0.0,
                format!(
                    "goal gate for objective '{}': no objective signal established completion",
                    objective
                ),
            )
        })
    }
}

impl Default for GoalGate {
    fn default() -> Self {
        Self::default_set()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_gate_passes_on_exit_zero() {
        // A command that exits 0 must yield a met verdict. `true` is portable.
        let gate = PredicateGate::new();
        let mut evidence = GoalEvidence::default();
        evidence.success_predicate = Some("true".to_string());
        let verdict = gate.evaluate("objective-x", &evidence);
        assert!(verdict.met, "exit-0 predicate must be met: {verdict:?}");
        assert_eq!(verdict.confidence, 1.0);
    }

    #[test]
    fn predicate_gate_fails_on_exit_nonzero() {
        let gate = PredicateGate::new();
        let mut evidence = GoalEvidence::default();
        // `false` exits 1 on every platform.
        evidence.success_predicate = Some("false".to_string());
        let verdict = gate.evaluate("objective-x", &evidence);
        assert!(!verdict.met, "exit-1 predicate must be unmet: {verdict:?}");
        assert_eq!(verdict.confidence, 1.0);
    }

    #[test]
    fn predicate_gate_unmet_when_missing() {
        let gate = PredicateGate::new();
        let evidence = GoalEvidence::default();
        let verdict = gate.evaluate("objective-x", &evidence);
        assert!(!verdict.met);
        assert_eq!(verdict.confidence, 0.0);
    }

    #[test]
    fn substring_gate_matches_present_substring() {
        let gate = SubstringGate;
        let mut evidence = GoalEvidence::default();
        evidence.required_output_substring = Some("All tests passed".to_string());
        evidence.observed_output = Some("running 12 tests\nAll tests passed\n".to_string());
        let verdict = gate.evaluate("objective-x", &evidence);
        assert!(verdict.met, "present substring must be met: {verdict:?}");
    }

    #[test]
    fn substring_gate_unmet_when_absent() {
        let gate = SubstringGate;
        let mut evidence = GoalEvidence::default();
        evidence.required_output_substring = Some("All tests passed".to_string());
        evidence.observed_output = Some("1 test failed".to_string());
        let verdict = gate.evaluate("objective-x", &evidence);
        assert!(!verdict.met);
    }

    #[test]
    fn substring_gate_case_sensitive() {
        let gate = SubstringGate;
        let mut evidence = GoalEvidence::default();
        evidence.required_output_substring = Some("ALL TESTS PASSED".to_string());
        evidence.observed_output = Some("all tests passed".to_string());
        let verdict = gate.evaluate("objective-x", &evidence);
        assert!(!verdict.met, "substring match must be case-sensitive");
    }

    #[test]
    fn goal_gate_prefers_deterministic_met_over_partial() {
        let mut gate = GoalGate::new();
        gate.add(Box::new(SubstringGate));
        let mut evidence = GoalEvidence::default();
        evidence.required_output_substring = Some("done".to_string());
        evidence.observed_output = Some("job done".to_string());
        let verdict = gate.evaluate("ship feature", &evidence);
        assert!(verdict.met);
        assert_eq!(verdict.confidence, 1.0);
        assert!(verdict.reason.contains("substring"));
    }

    #[test]
    fn goal_gate_unmet_without_objective_signal() {
        let gate = GoalGate::default_set();
        let evidence = GoalEvidence::default();
        let verdict = gate.evaluate("vague objective", &evidence);
        assert!(!verdict.met, "no objective signal must not be trusted as met");
    }

    #[test]
    fn goal_gate_empty_verifier_set_is_unmet() {
        let gate = GoalGate::new();
        let evidence = GoalEvidence::default();
        let verdict = gate.evaluate("objective-x", &evidence);
        assert!(!verdict.met);
        assert_eq!(verdict.confidence, 0.0);
    }
}
