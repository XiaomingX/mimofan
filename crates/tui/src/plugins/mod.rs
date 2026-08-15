//! Plugin capability assembly (issue #834, plan W1).
//!
//! This module is the seam between the static tool registry built in
//! `tool_setup.rs` and external/optional capability plugins described by a
//! `PluginManifest`. W1 only wires the `tools` slice; `sandbox`/`llm`
//! capabilities are declared here so the type is stable, but are filled by
//! W2/W4 respectively.

// Tools run inside the TUI alt-screen runtime; route logging through tracing.
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

pub mod manifest;
pub mod registry;
