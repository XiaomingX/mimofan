//! Session state and related types for the TUI.

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use ratatui::layout::Rect;

use crate::client::{CacheWarmupKey, PromptInspection};
use crate::models::Tool;
use crate::resource_telemetry::TokenThroughput;

use super::app::TurnCacheRecord;

/// Session cost and token telemetry state.
#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_cost: f64,
    pub session_cost_cny: f64,
    pub subagent_cost: f64,
    pub subagent_cost_cny: f64,
    pub subagent_cost_event_seqs: HashSet<u64>,
    pub displayed_cost_high_water: f64,
    pub displayed_cost_high_water_cny: f64,
    pub last_prompt_tokens: Option<u32>,
    pub last_completion_tokens: Option<u32>,
    pub last_output_throughput: Option<TokenThroughput>,
    /// Time-to-first-token: wall-clock from `TurnStarted` to the first
    /// content `MessageDelta`. Stored here for the footer chip.
    pub last_ttft: Option<Duration>,
    pub last_prompt_cache_hit_tokens: Option<u32>,
    pub last_prompt_cache_miss_tokens: Option<u32>,
    pub last_reasoning_replay_tokens: Option<u32>,
    pub total_tokens: u32,
    pub total_conversation_tokens: u32,
    /// Accumulated token breakdown for the session.
    pub total_input_tokens: u32,
    pub total_cache_hit_tokens: u32,
    pub total_cache_miss_tokens: u32,
    pub total_output_tokens: u32,
    pub turn_cache_history: VecDeque<TurnCacheRecord>,
    pub(crate) last_cache_inspection: Option<PromptInspection>,
    pub(crate) last_warmup_key: Option<CacheWarmupKey>,
    /// Tool catalog from the most recent model request.
    ///
    /// `/cache inspect` uses this to inspect the same tool schema bytes
    /// that were eligible for the provider's prefix cache.
    pub last_tool_catalog: Option<Vec<Tool>>,
    /// API base URL used by the most recent model request or cache warmup.
    pub last_base_url: Option<String>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            session_cost: 0.0,
            session_cost_cny: 0.0,
            subagent_cost: 0.0,
            subagent_cost_cny: 0.0,
            subagent_cost_event_seqs: HashSet::new(),
            displayed_cost_high_water: 0.0,
            displayed_cost_high_water_cny: 0.0,
            last_prompt_tokens: None,
            last_completion_tokens: None,
            last_output_throughput: None,
            last_ttft: None,
            last_prompt_cache_hit_tokens: None,
            last_prompt_cache_miss_tokens: None,
            last_reasoning_replay_tokens: None,
            total_tokens: 0,
            total_conversation_tokens: 0,
            total_input_tokens: 0,
            total_cache_hit_tokens: 0,
            total_cache_miss_tokens: 0,
            total_output_tokens: 0,
            turn_cache_history: VecDeque::new(),
            last_cache_inspection: None,
            last_warmup_key: None,
            last_tool_catalog: None,
            last_base_url: None,
        }
    }
}

impl SessionState {
    /// Reset the accumulated token breakdown fields to zero.
    pub fn reset_token_breakdown(&mut self) {
        self.total_input_tokens = 0;
        self.total_cache_hit_tokens = 0;
        self.total_cache_miss_tokens = 0;
        self.total_output_tokens = 0;
        self.last_output_throughput = None;
        self.last_ttft = None;
    }
}

/// Sidebar hover state for mouse tooltip support.
#[derive(Debug, Clone, Default)]
pub struct SidebarHoverState {
    /// Rendered sections with their areas and full-text lines.
    pub sections: Vec<SidebarHoverSection>,
}

/// Per-row metadata for sidebar detail popovers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarHoverRow {
    /// Absolute row position in the terminal.
    pub row_y: u16,
    /// Text shown in the compact sidebar row.
    pub display_text: String,
    /// Full untruncated text for the popover.
    pub full_text: String,
    /// Optional additional detail line.
    pub detail: Option<String>,
    /// Whether the compact row lost information.
    pub is_truncated: bool,
    /// Slash command to execute when this row is clicked (#3028).
    /// `shell_*` job ids route through `/jobs` (e.g. `/jobs cancel
    /// shell_abc123`); task-manager ids route through `/task` (e.g.
    /// `/task show task_abc123`).
    pub click_action: Option<String>,
    /// Optional narrower stop target for rows that show an inline `[x]`.
    pub stop_action: Option<String>,
    pub stop_zone_start_col: Option<u16>,
    pub stop_zone_end_col: Option<u16>,
}

/// Per-section metadata for sidebar hover detection.
#[derive(Debug, Clone)]
pub struct SidebarHoverSection {
    /// Content area within the section (inside border + padding).
    pub content_area: Rect,
    /// Full original text for each content line rendered.
    pub lines: Vec<String>,
    /// Per-row metadata for rich hover popovers.
    pub rows: Vec<SidebarHoverRow>,
}
