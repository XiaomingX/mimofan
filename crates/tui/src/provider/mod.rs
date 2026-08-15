//! Provider-side capability modules.
//!
//! #844: Batch API offline channel. The engine wires it through
//! `EngineConfig::batch_mode`; this module only provides the client plumbing
//! and types.

pub mod batch;
