//! Transcript viewport scroll state.

use ratatui::layout::Rect;

use crate::tui::scrolling::{MouseScrollState, TranscriptScroll};
use crate::tui::selection::{SelectionAutoscroll, TranscriptSelection};
use crate::tui::transcript::TranscriptViewCache;

/// Viewport/scroll state — fields related to transcript scrolling and caching.
pub struct ViewportState {
    pub transcript_scroll: TranscriptScroll,
    pub pending_scroll_delta: i32,
    pub mouse_scroll: MouseScrollState,
    pub transcript_cache: TranscriptViewCache,
    pub transcript_selection: TranscriptSelection,
    pub selection_autoscroll: Option<SelectionAutoscroll>,
    pub transcript_scrollbar_dragging: bool,
    pub last_transcript_area: Option<Rect>,
    pub last_composer_area: Option<Rect>,
    /// Outer rect of the right-hand sidebar (when visible), stored at render
    /// time so mouse hit-testing can keep scroll events over the sidebar from
    /// leaking into the transcript viewport.
    pub last_sidebar_area: Option<Rect>,
    pub last_transcript_top: usize,
    pub last_transcript_visible: usize,
    pub last_transcript_total: usize,
    pub last_transcript_padding_top: usize,
    pub jump_to_latest_button_area: Option<Rect>,
    /// Inner content rect of the composer (excluding border/padding),
    /// stored at render time for mouse coordinate mapping.
    pub last_composer_content: Option<Rect>,
    /// Number of rendered text lines scrolled off the top of the composer,
    /// stored at render time for mouse coordinate mapping.
    pub last_composer_scroll_offset: usize,
    /// Vertical padding above the first text line in the composer,
    /// stored at render time for mouse coordinate mapping.
    pub last_composer_top_padding: usize,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            transcript_scroll: TranscriptScroll::to_bottom(),
            pending_scroll_delta: 0,
            mouse_scroll: MouseScrollState::new(),
            transcript_cache: TranscriptViewCache::new(),
            transcript_selection: TranscriptSelection::default(),
            selection_autoscroll: None,
            transcript_scrollbar_dragging: false,
            last_transcript_area: None,
            last_composer_area: None,
            last_sidebar_area: None,
            last_transcript_top: 0,
            last_transcript_visible: 0,
            last_transcript_total: 0,
            last_transcript_padding_top: 0,
            jump_to_latest_button_area: None,
            last_composer_content: None,
            last_composer_scroll_offset: 0,
            last_composer_top_padding: 0,
        }
    }
}
