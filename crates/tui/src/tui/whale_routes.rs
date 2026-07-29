//! Whale-size route taxonomy for model + thinking-effort combinations (#2026).
//!
//! Maps each `(model, reasoning_effort)` pair to a friendly whale-species label,
//! sorted from largest/deepest to smallest/fastest. The labels share the same
//! species pool as sub-agent nicknames (#2016). These labels are kept as an
//! internal taxonomy for sub-agent routing receipts and related affordances; the
//! main `/model` picker stays neutral and lets users choose model and thinking
//! independently.
//!
//! ## Route ordering (size → speed)
//!
//! 1. Blue Whale   — Pro + max thinking (largest, deepest)
//! 2. Fin Whale    — Pro + high thinking
//! 3. Sperm Whale  — Pro + no thinking
//! 4. Humpback     — Flash + max thinking
//! 5. Minke Whale  — Flash + high thinking
//! 6. Beluga       — Flash + no thinking (smallest, fastest)
//!
//! Unknown or non-DeepSeek models fall back to the raw model id without
//! fake whale labeling.

use crate::tui::app::ReasoningEffort;

/// One whale-sized route: a model + thinking-effort combination with
/// a friendly label, sort order, and descriptive hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhaleRoute {
    /// Whale-species label, e.g. "Blue Whale".
    pub label: &'static str,
    /// Model id, e.g. "deepseek-v4-pro".
    pub model: &'static str,
    /// Reasoning effort tier.
    pub effort: ReasoningEffort,
    /// Sort index (0 = largest / deepest).
    pub sort_order: usize,
    /// Short inline hint, e.g. "Pro + max thinking".
    pub hint: &'static str,
    /// Longer description for tooltips / route receipts.
    pub description: &'static str,
}

#[cfg(test)]
mod tests {}
