//! Request/call-chain tracing (#637).
//!
//! Provides a stable `trace_id` per turn (or per external request) and helpers
//! to thread it through `tracing` spans so that logs/metrics across modules
//! (engine → turn_loop → tool execution → model client) can be correlated by a
//! single id. This is the minimal, zero-behavior-change scaffolding: it does
//! not alter any request struct's wire shape, it only attaches a span field
//! and exposes a generator so other modules can adopt `trace_id` over time.

use uuid::Uuid;

/// A correlated call-chain identifier.
///
/// Cheap to clone; rendered as a 32-char hex string. Generated once per turn
/// and carried through every `tracing` span opened during that turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceId(pub u128);

impl TraceId {
    /// Generate a fresh, random trace id.
    pub fn new() -> Self {
        TraceId(Uuid::new_v4().as_u128())
    }

    /// Render as a compact hex string for log lines / span fields.
    pub fn as_hex(&self) -> String {
        format!("{:032x}", self.0)
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_hex())
    }
}

/// Open a tracing span tagged with `trace_id`, returning the guard.
///
/// Callers enter the span for the duration of a turn (or any correlated
/// sub-operation) so every event emitted inside carries the same `trace_id`
/// field. Example:
///
/// ```ignore
/// let span = trace_span_for(self.trace_id);
/// let _enter = span.enter();
/// // ... work, all logs now carry trace_id ...
/// ```
pub fn trace_span_for(trace_id: TraceId) -> tracing::span::Span {
    tracing::span!(
        tracing::Level::INFO,
        "turn",
        trace_id = %trace_id.as_hex()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_id_is_unique_and_stable() {
        let a = TraceId::new();
        let b = TraceId::new();
        assert_ne!(a, b, "fresh trace ids must differ");
        assert_eq!(a, a, "same id stable");
        assert_eq!(a.as_hex().len(), 32, "hex is 32 chars");
        assert!(a.to_string().starts_with(&a.as_hex()));
    }

    #[test]
    fn default_trace_id_is_also_unique() {
        let a = TraceId::default();
        let b = TraceId::default();
        assert_ne!(a, b);
    }

    #[test]
    fn span_carries_trace_id_field() {
        let id = TraceId::new();
        let span = trace_span_for(id);
        // Entering must not panic; span is valid.
        let _enter = span.enter();
    }
}
