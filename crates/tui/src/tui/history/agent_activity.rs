//! Compact transcript rendering for agent and activity metadata cells.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::palette;

use super::{
    GenericToolCell, render_tool_header_with_family_and_summary, tool_status_label, truncate_text,
};

/// Render `agent` as a single compact summary line for live mode. The
/// companion `DelegateCard` already carries the live action tree, status, and
/// final summary; this line is just the pointer that says "a spawn happened,
/// here's the agent id".
///
/// Output shape (header):
///   `◐ delegate · agent  agent-abc12  [running]`
/// Falls back to a placeholder when the spawn is still pending and no agent id
/// has been assigned yet.
pub(super) fn render_agent_compact(cell: &GenericToolCell, low_motion: bool) -> Vec<Line<'static>> {
    let family = crate::tui::widgets::tool_card::ToolFamily::Delegate;
    let agent_id = cell
        .output
        .as_deref()
        .and_then(extract_agent_id)
        .unwrap_or("…");
    vec![render_tool_header_with_family_and_summary(
        family,
        Some(agent_id),
        tool_status_label(cell.status),
        cell.status,
        None,
        low_motion,
    )]
}

pub(super) fn render_activity_group(cell: &GenericToolCell, width: u16) -> Vec<Line<'static>> {
    let summary = cell.input_summary.as_deref().unwrap_or("Updated metadata");
    let budget = usize::from(width).max(1);
    vec![Line::from(Span::styled(
        truncate_text(summary, budget),
        Style::default().fg(palette::TEXT_MUTED),
    ))]
}

/// Pull the `agent_id` field out of a sub-agent open tool output. The tool
/// emits structured JSON shaped like
/// `{"agent_id": "agent-abc12", "nickname": "...", "model": "..."}` so we
/// look for the `agent_id` key and return its string value.
///
/// Returns `None` for outputs we can't parse as JSON or that lack the expected
/// key — the caller falls back to a placeholder so a still-pending spawn
/// renders cleanly.
pub(super) fn extract_agent_id(output: &str) -> Option<&str> {
    // Cheap, deterministic, no allocations: scan for the literal key.
    // Avoids dragging serde_json into a render hot path on every frame.
    let key = "\"agent_id\"";
    let key_idx = output.find(key)?;
    let rest = &output[key_idx + key.len()..];
    let colon = rest.find(':')?;
    let after_colon = rest[colon + 1..].trim_start();
    let after_colon = after_colon.strip_prefix('"')?;
    let end = after_colon.find('"')?;
    let id = &after_colon[..end];
    (!id.is_empty()).then_some(id)
}
