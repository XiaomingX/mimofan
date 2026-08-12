//! Retrieval / assembly trait abstractions (#645).
//!
//! Issue #645 observed that the context engine's retrieval and assembly
//! strategies were hardcoded, making the memory recall path non-pluggable.
//! This module introduces two small traits so that different backends
//! (vector store, codebase FTS, UserProfile, hybrid) can be composed behind a
//! stable interface without touching the engine wiring.
//!
//! These are *pure interfaces* — no behavior change is implied. Existing code
//! continues to work; new code can implement `Retriever`/`Assembler` to swap
//! strategies (e.g. for testing or for a different memory backend).

use crate::Result;
use crate::vector::SearchFilters;

/// A retriever produces candidate texts for a query under optional filters.
///
/// Implementors: `VectorStore` adapter, codebase FTS adapter, `UserProfile`
/// recall adapter. The engine depends on this trait, not on concrete stores.
/// Retrieval is synchronous here because the memory backends (`VectorStore`,
/// FTS) expose blocking APIs; async callers wrap with `block_on` if needed.
pub trait Retriever: Send + Sync {
    /// Return ranked candidate texts (most relevant first).
    fn retrieve(&self, query: &str, filters: &SearchFilters, limit: usize) -> Result<Vec<String>>;
}

/// An assembler composes retrieved candidates + profile into a final
/// injectable system-prompt fragment.
///
/// Implementors decide ordering, deduplication, token budgeting, and how the
/// `UserProfile` block is merged. Centralizing this behind a trait lets the
/// engine swap assembly strategies (e.g. compact vs. verbose) without
/// re-implementing the merge each call site.
pub trait Assembler: Send + Sync {
    /// Assemble the final injection text from candidates and a profile block.
    fn assemble(&self, candidates: &[String], profile_block: &str, token_budget: usize) -> String;
}

/// Default assembler: concatenate candidates (already ranked) and append the
/// profile block, truncating to `token_budget` (estimated by char count / 4).
///
/// This mirrors the pre-#645 inline behavior so existing callers can adopt the
/// trait without changing output shape.
pub struct ConcatAssembler;

impl Assembler for ConcatAssembler {
    fn assemble(&self, candidates: &[String], profile_block: &str, token_budget: usize) -> String {
        let mut out = String::new();
        for c in candidates {
            if out.len() + c.len() > token_budget * 4 {
                break;
            }
            out.push_str(c);
            out.push('\n');
        }
        if !profile_block.is_empty() {
            out.push_str(profile_block);
        }
        out
    }
}
