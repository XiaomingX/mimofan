//! Newline-boundary gate for streaming text.
//!
//! `LineBuffer` is an upstream-of-the-chunker safety layer that holds back any
//! text after the LAST `\n` until the next newline arrives. This prevents
//! partial multi-character markdown — most importantly partial code fences
//! (` ``` `) whose meaning flips depending on what follows on the same line —
//! from ever becoming visible state in the renderer.
//!
//! Mental model:
//! - `push(delta)`  appends raw stream text to an internal pending buffer.
//! - `take_committable()` returns only the prefix up to and including the
//!   LAST `\n` and clears that prefix. Whatever follows the last `\n` stays
//!   in the buffer for the next push.
//! - `flush()` returns whatever is left, used at end-of-stream when the model
//!   signals the turn is done. (The contract upstream of the chunker is that
//!   only complete-line text is committed; `flush()` is the explicit escape
//!   hatch when we know no more text will arrive.)
//!
//! See `cx5_chx5_newline_gate.md` in the task brief for full rationale.

/// Holds streaming text until a newline boundary is reached.
///
/// This is upstream of [`StreamChunker`](super::commit_tick::StreamChunker)
/// in the streaming pipeline:
///
/// ```text
/// raw delta -> LineBuffer.push -> take_committable -> StreamChunker.push_delta -> commit tick
/// ```
///
/// The chunker also enforces a "drain-up-to-last-newline" rule on its pending
/// buffer, but `LineBuffer` exists as a *separate* layer so that:
/// 1. The contract is explicit and locally testable.
/// 2. Future downstream consumers (e.g. live preview that renders queued lines
///    optimistically) cannot accidentally see a partial fence.
/// 3. End-of-turn flush semantics are owned by the gate, not the policy.
#[derive(Debug, Default, Clone)]
pub struct LineBuffer {
    /// Pending text not yet released because no terminating `\n` has been seen
    /// since the last commit.
    pending: String,
}

impl LineBuffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a raw delta.
    pub fn push(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.pending.push_str(delta);
    }

    /// Return the prefix of the pending buffer up to and including the LAST
    /// `\n`. Whatever follows that newline (if anything) stays buffered.
    ///
    /// Returns an empty string when the buffer is empty or contains no
    /// newline yet — callers can treat the empty-string case as "nothing
    /// committable on this push".
    pub fn take_committable(&mut self) -> String {
        let Some(last_nl) = self.pending.rfind('\n') else {
            return String::new();
        };
        // Drain everything up to and including the last newline. The remaining
        // tail (post-newline) stays in `pending` and is concatenated with the
        // next `push` before the next commit decision is made.
        self.pending.drain(..=last_nl).collect()
    }

    /// Return whatever is left in the buffer, even if it is not newline
    /// terminated. Used when the stream ends so we don't strand the final
    /// partial line.
    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.pending)
    }

    /// Whether the buffer holds any uncommitted text.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Length of the pending tail in bytes (testing/observability).
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Reset the buffer (e.g. on stream restart).
    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {}
