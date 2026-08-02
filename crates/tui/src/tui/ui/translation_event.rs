//! Translation pipeline events.
//!
//! Carries assistant-message / thinking-block translations from the background
//! i18n thread back to the main render loop. Moved out of `ui/mod.rs` during the
//! god-file slicing refactor.

use super::PendingToolUses;
use anyhow::Result;

pub(crate) enum TranslationEvent {
    AssistantMessage {
        history_index: Option<usize>,
        original_text: String,
        translated: Result<String>,
        thinking: Option<String>,
        tool_uses: PendingToolUses,
    },
    Thinking {
        placeholder: String,
        translated: Result<String>,
    },
}
