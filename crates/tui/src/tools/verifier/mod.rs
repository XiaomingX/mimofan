//! Objective verifier machinery for goal completion and tool output.
//!
//! This module hosts the *objective* completion/verification primitives that
//! the engine can use to judge whether a goal was actually met without
//! trusting the model's self-report (#849), plus a runtime `assert_key`
//! mechanism that lets tools declare the keys their output MUST contain and
//! have those expectations checked at execution time (#852).
//!
//! - `goal_gate`: [`GoalGate`] and the [`GoalVerifier`] trait with two
//!   concrete implementations ([`PredicateGate`], [`SubstringGate`]).
//! - `assert_key`: the [`AssertKey`] spec and [`verify_output`] / the
//!   [`ToolVerifier`] trait for per-tool runtime validation.
//! - `run_verifiers`: the agent-facing `run_verifiers` tool (pre-existing)
//!   lives alongside these so all verifier-related code shares one module.

pub mod assert_key;
pub mod goal_gate;
pub mod run_verifiers;

// Re-export so legacy path-based imports (e.g. `super::verifier::RunVerifiersTool`
// in registry.rs) keep resolving after `verifier` became a directory module.
pub use run_verifiers::RunVerifiersTool;
