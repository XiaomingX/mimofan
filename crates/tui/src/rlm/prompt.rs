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
mod tests {
    use super::*;

    fn body() -> String {
        match rlm_system_prompt() {
            SystemPrompt::Text(t) => t,
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn rlm_prompt_is_not_empty() {
        assert!(!body().is_empty());
    }

    #[test]
    fn rlm_prompt_uses_repl_fence() {
        assert!(body().contains("```repl"));
    }

    #[test]
    fn rlm_prompt_uses_five_phase_skeleton() {
        let s = body();
        for phase in ["Load", "Orient", "Compute", "Recurse", "Converge"] {
            assert!(s.contains(phase), "system prompt missing phase: {phase}");
        }
    }

    #[test]
    fn rlm_prompt_mentions_all_helpers() {
        let s = body();
        for name in [
            "peek",
            "search",
            "chunk",
            "chunk_coverage",
            "context_meta",
            "sub_query",
            "sub_query_batch",
            "sub_query_map",
            "sub_query_sequence",
            "sub_rlm",
            "finalize",
            "evaluate_progress",
            "SHOW_VARS",
        ] {
            assert!(s.contains(name), "system prompt missing helper: {name}");
        }
    }

    #[test]
    fn rlm_prompt_does_not_publicize_context_variables() {
        let s = body();
        assert!(s.contains("`_ctx` and `content` are compatibility aliases"));
        assert!(s.contains("There is no `context` or `ctx` variable"));
        assert!(!s.contains("len(context)"));
        assert!(!s.contains("chunk_context"));
        assert!(!s.contains("llm_query"));
        assert!(!s.contains("rlm_query"));
    }

    #[test]
    fn rlm_prompt_is_finalize_only() {
        let s = body();
        assert!(s.contains("finalize(value"));
        assert!(!s.contains("FINAL_VAR"));
        assert!(!s.contains("FINAL(value)"));
        assert!(!s.contains("FINAL("));
    }

    #[test]
    fn rlm_prompt_requires_deterministic_counts_and_coverage() {
        let s = body();
        assert!(s.contains("compute with Python"));
        assert!(s.contains("include coverage"));
        assert!(s.contains("chunks processed"));
    }

    #[test]
    fn rlm_prompt_requires_batch_dependency_safety() {
        let s = body();
        assert!(s.contains("dependency_mode=\"independent\""));
        assert!(s.contains("sub_query_sequence"));
        assert!(s.contains("database or schema migrations"));
        assert!(s.contains("rollback-sensitive"));
    }

    #[test]
    fn rlm_prompt_mentions_symbolic_state_contract() {
        let s = body();
        assert!(s.contains("symbolic recursion"));
        assert!(s.contains("REPL variables"));
        assert!(s.contains("Do not copy the whole input"));
    }
}
