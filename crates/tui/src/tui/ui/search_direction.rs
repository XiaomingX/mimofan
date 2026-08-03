//! search direction 子系统（从 ui 上帝文件切片）
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchDirection {
    Forward,
    Backward,
}

pub(crate) fn jump_to_adjacent_tool_cell(app: &mut App, direction: SearchDirection) -> bool {
    let line_meta = app.viewport.transcript_cache.line_meta();
    if line_meta.is_empty() {
        return false;
    }

    let top = app
        .viewport
        .last_transcript_top
        .min(line_meta.len().saturating_sub(1));
    let current_cell = line_meta
        .get(top)
        .and_then(crate::tui::scrolling::TranscriptLineMeta::cell_line)
        .map(|(cell_index, _)| app.original_cell_index_for_rendered(cell_index));

    let mut scan_indices = Vec::new();
    match direction {
        SearchDirection::Forward => {
            scan_indices.extend((top.saturating_add(1))..line_meta.len());
        }
        SearchDirection::Backward => {
            scan_indices.extend((0..top).rev());
        }
    }

    for idx in scan_indices {
        let Some((cell_index, _)) = line_meta[idx].cell_line() else {
            continue;
        };
        let cell_index = app.original_cell_index_for_rendered(cell_index);
        if current_cell.is_some_and(|current| current == cell_index) {
            continue;
        }
        if !matches!(app.history.get(cell_index), Some(HistoryCell::Tool(_))) {
            continue;
        }
        if let Some(anchor) = TranscriptScroll::anchor_for(line_meta, idx) {
            app.viewport.transcript_scroll = anchor;
            app.viewport.pending_scroll_delta = 0;
            app.needs_redraw = true;
            return true;
        }
    }

    false
}
