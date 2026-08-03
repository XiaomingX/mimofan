//! Vim mode and input state for the composer.

use crate::localization::{Locale, MessageId, tr};

/// Vim editing mode for the composer input.
///
/// Currently supports Normal and Insert modes. Visual mode is reserved for
/// future selection support and currently behaves like `Normal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    /// Normal / command mode — motions and operators, no text insertion.
    #[default]
    Normal,
    /// Insert mode — characters are appended at the cursor as typed.
    Insert,
    /// Visual mode — reserved for future selection support.
    Visual,
}

impl VimMode {
    /// Localized status-bar label shown in the composer border (user-facing).
    #[must_use]
    pub fn label_localized(self, locale: Locale) -> &'static str {
        tr(
            locale,
            match self {
                Self::Normal => MessageId::VimModeNormal,
                Self::Insert => MessageId::VimModeInsert,
                Self::Visual => MessageId::VimModeVisual,
            },
        )
    }
}
