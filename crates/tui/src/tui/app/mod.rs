//! Application state for the `DeepSeek` TUI.
//!
//! This module provides the core application state and logic for the TUI.
//! The implementation is split across multiple files for maintainability:
//!
//! - `state.rs`: Type definitions (enums, structs, App struct)
//! - `actions.rs`: Action enums and related types
//! - `impl_core.rs`: Core App methods (new, tr, set_mode, etc.)
//! - `impl_history.rs`: History-related methods
//! - `impl_streaming.rs`: Streaming-related methods
//! - `impl_composer.rs`: Composer-related methods
//! - `impl_actions.rs`: Action-related methods
//! - `events.rs`: Event handling and input processing functions
//! - `helpers.rs`: Helper functions

mod actions;
mod events;
mod helpers;
mod impl_actions;
mod impl_composer;
mod impl_core;
mod impl_history;
mod impl_streaming;
mod state;

// Re-export all public types from submodules for backward compatibility
pub use actions::*;
pub use helpers::*;
pub use state::*;

// Re-export types from other modules for backward compatibility
pub use crate::tui::composer::MentionCompletionCache;
pub use crate::tui::history_search::HuntVerdict;
pub use crate::tui::state::{SidebarHoverRow, SidebarHoverSection, SidebarHoverState};
pub use crate::tui::tool_collapse::ToolCollapseMode;
pub use crate::tui::vim::VimMode;
