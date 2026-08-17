//! Runtime helpers that are not tied to a single subsystem.
//!
//! Currently houses [`idle_drop`], the idle-time-based lazy unload guard used
//! to free LSP/embedding handles after a period of inactivity.

pub mod idle_drop;
