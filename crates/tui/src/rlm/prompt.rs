//! RLM system prompt — adapted from the reference implementation
//! (alexzhang13/rlm) and Zhang et al., arXiv:2512.24601.
//!
//! The prompt is deliberately strict: the only way to make progress is
//! through a `repl` block. There is no fall-through prose path.

use crate::models::SystemPrompt;

/// Build the system prompt for a Recursive Language Model (RLM) root call.
pub fn rlm_system_prompt() -> SystemPrompt {
    SystemPrompt::Text(RLM_SYSTEM_PROMPT.trim().to_string())
}

const RLM_SYSTEM_PROMPT: &str = include_str!("../prompts/rlm.md");

#[cfg(test)]
mod tests {}
