//! Composer input state for the TUI.

use std::collections::VecDeque;
use std::path::PathBuf;

use crate::tui::history_search::{ComposerHistorySearch, InputHistoryDraft};
use crate::tui::paste_burst::PasteBurst;
use crate::tui::vim::VimMode;

/// Cached @-mention completion results to avoid re-walking the filesystem when
/// the cursor moves inside the same mention token.
#[derive(Debug, Clone)]
pub struct MentionCompletionCache {
    /// Workspace root used for this completion walk.
    pub workspace: PathBuf,
    /// Process cwd captured for cwd-relative completion entries.
    pub cwd: Option<PathBuf>,
    /// The partial text after `@` that triggered this completion.
    pub partial: String,
    /// Candidate limit used for this completion walk.
    pub limit: usize,
    /// Workspace depth limit used for this completion walk. Included so live
    /// config changes invalidate cached popup results.
    pub walk_depth: usize,
    /// Completion behavior used for this walk. Included so live config changes
    /// invalidate cached popup results.
    pub behavior: String,
    /// Whether symlink following was enabled for this completion walk.
    /// Included so live config changes invalidate cached popup results.
    pub follow_links: bool,
    /// Cached completion entries.
    pub entries: Vec<String>,
}

/// Composer input state — grouped fields for the text input area.
pub struct ComposerState {
    /// Current composer text content.
    pub input: String,
    /// Cursor position within `input` (in characters).
    pub cursor_position: usize,
    /// Single-entry kill buffer for emacs-style `Ctrl+K` cut / `Ctrl+Y` yank.
    pub kill_buffer: String,
    pub(crate) paste_burst: PasteBurst,
    /// When a large paste is consolidated at submit time, the file @mention
    /// is stored here so it can be appended to the submitted text without
    /// replacing the visible composer content (#3263).
    pub(crate) pending_paste_reference: Option<String>,
    /// When composer content is oversized, the full text is stored here
    /// while `self.input` shows a truncated preview. At submit time the
    /// full text is restored for model submission (#3263).
    pub(crate) oversized_paste_full_text: Option<String>,
    pub input_history: Vec<String>,
    pub draft_history: VecDeque<String>,
    pub clear_undo_buffer: Option<String>,
    pub history_index: Option<usize>,
    pub(crate) history_navigation_draft: Option<InputHistoryDraft>,
    pub composer_history_search: Option<ComposerHistorySearch>,
    pub selected_attachment_index: Option<usize>,
    pub slash_menu_selected: usize,
    pub slash_menu_hidden: bool,
    pub mention_menu_selected: usize,
    pub mention_menu_hidden: bool,
    /// Cached @-mention completions to avoid re-walking the filesystem when
    /// the cursor moves inside the same mention token.
    pub mention_completion_cache: Option<MentionCompletionCache>,
    /// Whether vim modal editing is enabled for this composer.
    /// Sourced from `Settings::vim_mode` at startup.
    pub vim_enabled: bool,
    /// Current vim editing mode.  Only meaningful when `vim_enabled` is true.
    pub vim_mode: VimMode,
    /// Pending `d` prefix for the `dd` delete-line operator.  Set when the
    /// user presses `d` in Normal mode; cleared on the next key (either `d`
    /// to complete `dd`, or any other key to cancel).
    pub vim_pending_d: bool,
    /// When set, the cursor is the active end of a text selection and
    /// `selection_anchor` is the fixed end.  Both are char-indexed.
    /// `None` means no selection is active.
    pub selection_anchor: Option<usize>,
}

impl Default for ComposerState {
    fn default() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
            kill_buffer: String::new(),
            paste_burst: PasteBurst::default(),
            pending_paste_reference: None,
            oversized_paste_full_text: None,
            input_history: Vec::new(),
            draft_history: VecDeque::new(),
            clear_undo_buffer: None,
            history_index: None,
            history_navigation_draft: None,
            composer_history_search: None,
            selected_attachment_index: None,
            slash_menu_selected: 0,
            slash_menu_hidden: false,
            mention_menu_selected: 0,
            mention_menu_hidden: false,
            mention_completion_cache: None,
            vim_enabled: false,
            vim_mode: VimMode::Normal,
            vim_pending_d: false,
            selection_anchor: None,
        }
    }
}
