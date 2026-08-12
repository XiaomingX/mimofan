//! Stream resumption / breakpoint reconnect for LLM streaming responses.
//!
//! When an SSE stream from an LLM provider is interrupted mid-response (network
//! drop, provider 5xx, timeout), a resilient client should be able to reconnect
//! and continue consuming from where it left off rather than restarting the
//! whole generation. [`StreamResume`] is the pure state tracker that makes this
//! possible: it records how much content has already been received (a byte/token
//! offset cursor) and exposes a reconnect entry point that yields the
//! continuation parameters a provider needs to resume.
//!
//! This module is deliberately free of network and clock dependencies so the
//! cursor bookkeeping can be unit tested deterministically. Wiring it into the
//! production retry loop is a follow-up task.

use std::time::Duration;

/// Status of a resumable stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamResumeState {
    /// Actively receiving tokens.
    Active,
    /// The stream was interrupted and a reconnect is required.
    Interrupted,
    /// The stream completed successfully and is no longer resumable.
    Completed,
}

/// A pure tracker for resuming an interrupted LLM token stream.
///
/// The tracker keeps a monotonic cursor (`received_bytes`) describing how much
/// content has already been delivered to the caller. On interruption the caller
/// calls [`StreamResume::interrupt`], which flips the state to `Interrupted` and
/// records how many reconnect attempts have been made. The next reconnect is
/// obtained via [`StreamResume::resume_stream`], returning the continuation
/// parameters (the byte offset to resume from, plus a suggested backoff).
#[derive(Debug, Clone)]
pub struct StreamResume {
    /// Total bytes/tokens already received and acknowledged.
    received_bytes: u64,
    /// Current status of the stream.
    state: StreamResumeState,
    /// Number of reconnect attempts performed since the last interruption.
    reconnect_attempts: u32,
    /// Base backoff used between reconnect attempts.
    base_backoff: Duration,
    /// Maximum number of reconnect attempts before giving up.
    max_reconnect_attempts: u32,
}

/// Continuation parameters returned when resuming an interrupted stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamResumePoint {
    /// Offset (in bytes) from which the provider should resume delivery.
    pub resume_offset: u64,
    /// How long the caller should wait before issuing the reconnect.
    pub backoff: Duration,
    /// Zero-based index of this reconnect attempt.
    pub attempt: u32,
}

impl StreamResume {
    /// Creates a new tracker for a stream starting from byte zero.
    ///
    /// `base_backoff` seeds the exponential reconnect delay; `max_reconnect_attempts`
    /// bounds how many reconnects will be attempted before the stream is abandoned.
    #[must_use]
    pub fn new(base_backoff: Duration, max_reconnect_attempts: u32) -> Self {
        Self {
            received_bytes: 0,
            state: StreamResumeState::Active,
            reconnect_attempts: 0,
            base_backoff,
            max_reconnect_attempts,
        }
    }

    /// Returns the current state.
    #[must_use]
    pub fn state(&self) -> StreamResumeState {
        self.state
    }

    /// Returns how many bytes/tokens have been received so far.
    #[must_use]
    pub fn received_bytes(&self) -> u64 {
        self.received_bytes
    }

    /// Records newly received content, advancing the cursor.
    ///
    /// A successful delivery also resets the reconnect-attempt counter, since a
    /// healthy delivery implies the connection is good again.
    pub fn record_received(&mut self, bytes: u64) {
        self.received_bytes = self.received_bytes.saturating_add(bytes);
        self.reconnect_attempts = 0;
        self.state = StreamResumeState::Active;
    }

    /// Marks the stream as interrupted, preparing it for a reconnect.
    ///
    /// No-op if the stream has already completed.
    pub fn interrupt(&mut self) {
        if self.state != StreamResumeState::Completed {
            self.state = StreamResumeState::Interrupted;
        }
    }

    /// Marks the stream as completed; further resumes are no longer possible.
    pub fn complete(&mut self) {
        self.state = StreamResumeState::Completed;
    }

    /// Returns `true` if another reconnect attempt should be made.
    #[must_use]
    pub fn can_resume(&self) -> bool {
        self.state == StreamResumeState::Interrupted
            && self.reconnect_attempts < self.max_reconnect_attempts
    }

    /// Produces the continuation point for a reconnect.
    ///
    /// Returns `None` if the stream is not in an interruptible state or the
    /// maximum number of reconnect attempts has been exhausted. Each call
    /// increments the attempt counter and uses exponential backoff
    /// (`base_backoff * 2^attempt`) capped at 60s.
    #[must_use]
    pub fn resume_stream(&mut self) -> Option<StreamResumePoint> {
        if !self.can_resume() {
            return None;
        }
        let attempt = self.reconnect_attempts;
        let backoff = compute_backoff(self.base_backoff, attempt);
        self.reconnect_attempts += 1;
        Some(StreamResumePoint {
            resume_offset: self.received_bytes,
            backoff,
            attempt,
        })
    }
}

/// Computes an exponential backoff capped at 60 seconds.
fn compute_backoff(base: Duration, attempt: u32) -> Duration {
    let factor = 2u64.saturating_pow(attempt);
    let micros = base.as_micros().saturating_mul(u128::from(factor));
    let capped = micros.min(60 * 1_000_000);
    Duration::from_micros(capped as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stream_is_active_with_zero_cursor() {
        let sr = StreamResume::new(Duration::from_millis(100), 3);
        assert_eq!(sr.state(), StreamResumeState::Active);
        assert_eq!(sr.received_bytes(), 0);
        assert!(!sr.can_resume());
    }

    #[test]
    fn record_received_advances_cursor_and_resets_attempts() {
        let mut sr = StreamResume::new(Duration::from_millis(100), 3);
        sr.record_received(10);
        assert_eq!(sr.received_bytes(), 10);
        sr.interrupt();
        // After an interruption we attempted a reconnect.
        let _ = sr.resume_stream();
        assert_eq!(sr.reconnect_attempts, 1);
        // A successful delivery resets the attempt counter.
        sr.record_received(5);
        assert_eq!(sr.received_bytes(), 15);
        assert_eq!(sr.reconnect_attempts, 0);
        assert_eq!(sr.state(), StreamResumeState::Active);
    }

    #[test]
    fn resume_stream_reports_offset_and_backoff() {
        let mut sr = StreamResume::new(Duration::from_millis(100), 3);
        sr.record_received(42);
        sr.interrupt();
        let point = sr.resume_stream().expect("should produce a resume point");
        assert_eq!(point.resume_offset, 42);
        assert_eq!(point.attempt, 0);
        assert_eq!(point.backoff, Duration::from_millis(100));
    }

    #[test]
    fn backoff_grows_exponentially_and_is_capped() {
        let mut sr = StreamResume::new(Duration::from_secs(10), 10);
        sr.interrupt();
        let p0 = sr.resume_stream().unwrap();
        let p1 = sr.resume_stream().unwrap();
        let p2 = sr.resume_stream().unwrap();
        assert_eq!(p0.backoff, Duration::from_secs(10));
        assert_eq!(p1.backoff, Duration::from_secs(20));
        assert_eq!(p2.backoff, Duration::from_secs(40));
        // Far past the cap, still bounded at 60s.
        for _ in 0..6 {
            let _ = sr.resume_stream();
        }
        let capped = sr.resume_stream().unwrap();
        assert_eq!(capped.backoff, Duration::from_secs(60));
    }

    #[test]
    fn cannot_resume_past_max_attempts() {
        let mut sr = StreamResume::new(Duration::from_millis(100), 2);
        sr.interrupt();
        assert!(sr.can_resume());
        let _ = sr.resume_stream(); // attempt 0
        let _ = sr.resume_stream(); // attempt 1
        assert!(!sr.can_resume());
        assert!(sr.resume_stream().is_none());
    }

    #[test]
    fn completed_stream_is_not_resumable() {
        let mut sr = StreamResume::new(Duration::from_millis(100), 3);
        sr.record_received(7);
        sr.complete();
        sr.interrupt(); // no-op after completion
        assert_eq!(sr.state(), StreamResumeState::Completed);
        assert!(!sr.can_resume());
        assert!(sr.resume_stream().is_none());
    }
}
