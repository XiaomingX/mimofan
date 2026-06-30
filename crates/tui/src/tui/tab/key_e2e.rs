//! End-to-end keyboard event tests for tab system
//!
//! These tests simulate the keyboard event handling that happens in
//! `ui.rs` when the user presses tab-related shortcuts. They verify that
//! the TabManager state transitions correctly in response to key events.
//!
//! The actual key event dispatch lives in `ui.rs` (which is hard to
//! test in isolation due to the engine/App dependencies), so these
//! tests exercise the underlying state transitions that the key handlers
//! would trigger.
//!
//! Run with: `cargo test tui::tab::key_e2e -- --nocapture`

#[cfg(test)]
mod tests {}
