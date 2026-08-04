use std::path::Path;
use std::time::{Duration, Instant};

use crate::tui::clipboard::ClipboardContent;
use crate::tui::history::TranscriptRenderOptions;
use crate::tui::paste_burst::FlushResult;
use crate::tui::scrolling::TranscriptScroll;
use crate::tui::transcript::TranscriptViewCache;
use crate::tui::vim::VimMode;

use super::events::strip_raw_mouse_report_runs;
use super::helpers::{
    byte_index_at_char, char_count, media_attachment_reference, normalize_paste_text,
    remove_char_at, sanitize_api_key_text,
};
use super::state::{App, StatusToast};

// === Constants ===

const MAX_SUBMITTED_INPUT_CHARS: usize = 16_000;
const MAX_COMPOSER_DISPLAY_CHARS: usize = 4_000;
const MAX_DRAFT_HISTORY: usize = 50;

impl App {
    pub fn active_status_toast(&mut self) -> Option<StatusToast> {
        self.sync_status_message_to_toasts();
        let now = Instant::now();
        let mut removed = false;

        while self
            .status_toasts
            .front()
            .is_some_and(|toast| toast.is_expired(now))
        {
            self.status_toasts.pop_front();
            removed = true;
        }

        if self
            .sticky_status
            .as_ref()
            .is_some_and(|toast| toast.is_expired(now))
        {
            self.sticky_status = None;
            removed = true;
        }

        if removed {
            self.needs_redraw = true;
        }

        self.sticky_status
            .clone()
            .or_else(|| self.status_toasts.back().cloned())
    }

    pub fn transcript_render_options(&self) -> TranscriptRenderOptions {
        TranscriptRenderOptions {
            show_thinking: self.show_thinking,
            verbose: self.verbose_transcript,
            show_tool_details: self.show_tool_details,
            calm_mode: self.calm_mode,
            low_motion: self.low_motion,
            spacing: self.transcript_spacing,
        }
    }

    /// Handle terminal resize event.
    pub fn handle_resize(&mut self, _width: u16, _height: u16) {
        let preserved_scroll = (!self.viewport.transcript_scroll.is_at_tail())
            .then_some(self.viewport.last_transcript_top);
        self.viewport.transcript_cache = TranscriptViewCache::new();

        if let Some(top) = preserved_scroll {
            self.viewport.transcript_scroll = TranscriptScroll::at_line(top);
        }

        self.viewport.pending_scroll_delta = 0;
        self.viewport.transcript_selection.clear();

        self.viewport.last_transcript_area = None;
        self.viewport.last_transcript_top = 0;
        // Seed visible height from the resize event so paging keys use a
        // useful page size immediately, before the next render updates it.
        self.viewport.last_transcript_visible = (_height as usize).saturating_sub(2).max(1);
        self.viewport.last_transcript_total = 0;
        self.viewport.last_transcript_padding_top = 0;
        self.viewport.jump_to_latest_button_area = None;

        self.mark_history_updated();
    }

    pub fn cursor_byte_index(&self) -> usize {
        byte_index_at_char(&self.input, self.cursor_position)
    }

    /// When the user starts editing a truncated oversized paste, restore the
    /// full text so they can see and edit the complete content (#3263).
    pub(crate) fn auto_expand_oversized_paste(&mut self) {
        if let Some(full) = self.oversized_paste_full_text.take() {
            self.input = full;
            // Clamp cursor to the new length instead of resetting to 0,
            // so the user's position in the truncated preview is preserved.
            self.cursor_position = self.cursor_position.min(char_count(&self.input));
        }
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.auto_expand_oversized_paste();
        self.delete_selection();
        self.selected_attachment_index = None;
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert_str(byte_index, text);
        self.cursor_position = cursor + char_count(text);
        self.strip_raw_mouse_reports_from_input();
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    pub fn insert_paste_text(&mut self, text: &str) {
        if let Some(pending) = self.paste_burst.flush_before_modified_input() {
            self.insert_str(&pending);
        }
        let normalized = normalize_paste_text(text);
        if !normalized.is_empty() {
            self.insert_str(&normalized);
        }
        self.paste_burst.clear_after_explicit_paste();
        // Large pasted input stays editable and visible until submit. The
        // submit-time safety net consolidates oversized composer content into
        // an @paste-...md mention before dispatch, so no path silently
        // truncates user input.
        // self.consolidate_large_input_if_oversized(); // deferred to submit time
    }

    pub fn insert_media_attachment(&mut self, kind: &str, path: &Path, description: Option<&str>) {
        let reference = media_attachment_reference(kind, path, description);
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        let needs_prefix_newline = self.input[..byte_index]
            .chars()
            .last()
            .is_some_and(|ch| !ch.is_whitespace());
        let needs_suffix_newline = self.input[byte_index..]
            .chars()
            .next()
            .is_some_and(|ch| !ch.is_whitespace());

        let mut inserted = String::new();
        if needs_prefix_newline {
            inserted.push('\n');
        }
        inserted.push_str(&reference);
        if needs_suffix_newline || self.input[byte_index..].is_empty() {
            inserted.push('\n');
        }
        self.insert_str(&inserted);
        self.paste_burst.clear_after_explicit_paste();
    }

    pub fn composer_attachment_count(&self) -> usize {
        crate::tui::file_mention::media_attachment_references(&self.input).len()
    }

    pub fn selected_composer_attachment_index(&self) -> Option<usize> {
        let count = self.composer_attachment_count();
        self.selected_attachment_index
            .filter(|index| *index < count)
    }

    pub fn select_previous_composer_attachment(&mut self) -> bool {
        let count = self.composer_attachment_count();
        if count == 0 {
            self.selected_attachment_index = None;
            return false;
        }

        let next = self
            .selected_composer_attachment_index()
            .map_or(count.saturating_sub(1), |index| index.saturating_sub(1));
        self.selected_attachment_index = Some(next);
        self.cursor_position = 0;
        self.status_message = Some("Attachment selected - Backspace/Delete removes it".to_string());
        self.needs_redraw = true;
        true
    }

    pub fn select_next_composer_attachment(&mut self) -> bool {
        let count = self.composer_attachment_count();
        let Some(index) = self.selected_composer_attachment_index() else {
            return false;
        };
        if index + 1 < count {
            self.selected_attachment_index = Some(index + 1);
            self.status_message =
                Some("Attachment selected - Backspace/Delete removes it".to_string());
        } else {
            self.selected_attachment_index = None;
            self.status_message = Some("Composer focused".to_string());
        }
        self.needs_redraw = true;
        true
    }

    pub fn clear_composer_attachment_selection(&mut self) -> bool {
        if self.selected_attachment_index.take().is_some() {
            self.status_message = Some("Composer focused".to_string());
            self.needs_redraw = true;
            true
        } else {
            false
        }
    }

    pub fn remove_selected_composer_attachment(&mut self) -> bool {
        let references = crate::tui::file_mention::media_attachment_references(&self.input);
        let Some(index) = self
            .selected_composer_attachment_index()
            .filter(|index| *index < references.len())
        else {
            self.selected_attachment_index = None;
            return false;
        };
        let reference = references[index].clone();
        let cursor_byte = byte_index_at_char(&self.input, self.cursor_position);
        let new_cursor_byte = if cursor_byte <= reference.start_byte {
            cursor_byte
        } else if cursor_byte >= reference.end_byte {
            cursor_byte.saturating_sub(reference.end_byte - reference.start_byte)
        } else {
            reference.start_byte
        };

        self.input
            .replace_range(reference.start_byte..reference.end_byte, "");
        self.cursor_position = self.input[..new_cursor_byte.min(self.input.len())]
            .chars()
            .count();
        let remaining = self.composer_attachment_count();
        self.selected_attachment_index = if remaining == 0 {
            None
        } else {
            Some(index.min(remaining.saturating_sub(1)))
        };
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.status_message = Some(format!("Removed attachment: {}", reference.path));
        self.needs_redraw = true;
        true
    }

    pub fn flush_paste_burst_if_due(&mut self, now: Instant) -> bool {
        match self.paste_burst.flush_if_due(now) {
            FlushResult::Paste(text) => {
                self.insert_str(&text);
                true
            }
            FlushResult::Typed(ch) => {
                self.insert_char(ch);
                true
            }
            FlushResult::None => false,
        }
    }

    pub fn flush_paste_burst_if_enabled(&mut self, now: Instant) -> bool {
        self.use_paste_burst_detection && self.flush_paste_burst_if_due(now)
    }

    pub fn paste_burst_next_flush_delay_if_enabled(&self, now: Instant) -> Option<Duration> {
        if self.use_paste_burst_detection {
            self.paste_burst.next_flush_delay(now)
        } else {
            None
        }
    }

    pub fn flush_paste_burst_before_modified_input_if_enabled(&mut self) -> Option<String> {
        if self.use_paste_burst_detection {
            self.paste_burst.flush_before_modified_input()
        } else {
            None
        }
    }

    pub fn insert_api_key_char(&mut self, c: char) {
        let cursor = self.api_key_cursor.min(char_count(&self.api_key_input));
        let byte_index = byte_index_at_char(&self.api_key_input, cursor);
        self.api_key_input.insert(byte_index, c);
        self.api_key_cursor = cursor + 1;
    }

    pub fn insert_api_key_str(&mut self, text: &str) {
        let sanitized = sanitize_api_key_text(text);
        if sanitized.is_empty() {
            return;
        }
        let cursor = self.api_key_cursor.min(char_count(&self.api_key_input));
        let byte_index = byte_index_at_char(&self.api_key_input, cursor);
        self.api_key_input.insert_str(byte_index, &sanitized);
        self.api_key_cursor = cursor + char_count(&sanitized);
    }

    pub fn delete_api_key_char(&mut self) {
        if self.api_key_cursor == 0 {
            return;
        }
        let target = self.api_key_cursor.saturating_sub(1);
        if remove_char_at(&mut self.api_key_input, target) {
            self.api_key_cursor = target;
        }
    }

    /// Paste from clipboard into input
    pub fn paste_from_clipboard(&mut self) {
        if let Some(content) = self.clipboard.read(self.workspace.as_path()) {
            self.apply_clipboard_content(content);
        }
    }

    pub fn apply_clipboard_content(&mut self, content: ClipboardContent) {
        match content {
            ClipboardContent::Text(text) => {
                self.insert_paste_text(&text);
            }
            ClipboardContent::Image(pasted) => {
                let description = format!("{} ({})", pasted.short_label(), pasted.size_label());
                self.insert_media_attachment("image", &pasted.path, Some(&description));
                self.status_message = Some(format!("Attached image: {description}"));
            }
        }
    }

    pub fn paste_api_key_from_clipboard(&mut self) {
        if let Some(ClipboardContent::Text(text)) = self.clipboard.read(self.workspace.as_path()) {
            self.insert_api_key_str(&text);
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        self.viewport.pending_scroll_delta =
            self.viewport.pending_scroll_delta.saturating_sub(delta);
        self.user_scrolled_during_stream = true;
        self.needs_redraw = true;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        self.viewport.pending_scroll_delta =
            self.viewport.pending_scroll_delta.saturating_add(delta);
        self.user_scrolled_during_stream = true;
        self.needs_redraw = true;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.viewport.transcript_scroll = TranscriptScroll::to_bottom();
        self.viewport.pending_scroll_delta = 0;
        self.viewport.jump_to_latest_button_area = None;
        self.user_scrolled_during_stream = false;
        self.needs_redraw = true;
    }

    pub fn insert_char(&mut self, c: char) {
        self.clear_input_history_navigation();
        self.auto_expand_oversized_paste();
        self.delete_selection();
        self.selected_attachment_index = None;
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert(byte_index, c);
        self.cursor_position = cursor + 1;
        self.strip_raw_mouse_reports_from_input();
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    fn strip_raw_mouse_reports_from_input(&mut self) {
        if let Some((input, cursor_position)) =
            strip_raw_mouse_report_runs(&self.input, self.cursor_position)
        {
            self.input = input;
            self.cursor_position = cursor_position;
        }
    }

    pub fn delete_char(&mut self) {
        self.clear_input_history_navigation();
        self.auto_expand_oversized_paste();
        if self.delete_selection() {
            return;
        }
        self.selected_attachment_index = None;
        if self.cursor_position == 0 {
            return;
        }
        let target = self.cursor_position.saturating_sub(1);
        let removed = remove_char_at(&mut self.input, target);
        if removed {
            self.cursor_position = target;
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    pub fn delete_char_forward(&mut self) {
        self.clear_input_history_navigation();
        self.auto_expand_oversized_paste();
        if self.delete_selection() {
            return;
        }
        self.selected_attachment_index = None;
        if self.input.is_empty() {
            return;
        }
        let target = self.cursor_position;
        let removed = remove_char_at(&mut self.input, target);
        if !removed {
            self.cursor_position = char_count(&self.input);
        }
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    /// Delete the word before the cursor.
    pub fn delete_word_backward(&mut self) {
        self.clear_input_history_navigation();
        if self.delete_selection() {
            return;
        }
        self.selected_attachment_index = None;
        if self.cursor_position == 0 {
            return;
        }

        let cursor_byte = byte_index_at_char(&self.input, self.cursor_position);
        let mut word_start = cursor_byte;

        while word_start > 0 {
            let Some((prev, ch)) = self.input[..word_start].char_indices().next_back() else {
                break;
            };
            if !ch.is_whitespace() {
                break;
            }
            word_start = prev;
        }

        while word_start > 0 {
            let Some((prev, ch)) = self.input[..word_start].char_indices().next_back() else {
                break;
            };
            if ch.is_whitespace() {
                break;
            }
            word_start = prev;
        }

        if word_start < cursor_byte {
            self.input.replace_range(word_start..cursor_byte, "");
            self.cursor_position = char_count(&self.input[..word_start]);
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    /// Delete from the cursor to the start of the line.
    pub fn delete_to_start_of_line(&mut self) {
        self.clear_input_history_navigation();
        if self.delete_selection() {
            return;
        }
        self.selected_attachment_index = None;
        if self.cursor_position == 0 {
            return;
        }

        let cursor_byte = byte_index_at_char(&self.input, self.cursor_position);
        // Find the start of the current line (last newline or start of string)
        let line_start = self.input[..cursor_byte]
            .rfind('\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);

        if line_start < cursor_byte {
            self.input.replace_range(line_start..cursor_byte, "");
            self.cursor_position = char_count(&self.input[..line_start]);
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    /// Delete the word after the cursor.
    pub fn delete_word_forward(&mut self) {
        self.clear_input_history_navigation();
        if self.delete_selection() {
            return;
        }
        self.selected_attachment_index = None;
        let cursor_byte = byte_index_at_char(&self.input, self.cursor_position);
        if cursor_byte >= self.input.len() {
            return;
        }

        let mut word_end = cursor_byte;
        while word_end < self.input.len() {
            let Some(ch) = self.input[word_end..].chars().next() else {
                break;
            };
            if !ch.is_whitespace() {
                break;
            }
            word_end += ch.len_utf8();
        }

        while word_end < self.input.len() {
            let Some(ch) = self.input[word_end..].chars().next() else {
                break;
            };
            if ch.is_whitespace() {
                break;
            }
            word_end += ch.len_utf8();
        }

        if cursor_byte < word_end {
            self.input.replace_range(cursor_byte..word_end, "");
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    /// Cut from the cursor to the end of the current logical line into the
    /// kill buffer. If the cursor is already at end-of-line and a trailing
    /// newline exists, that newline is consumed so repeated invocations
    /// continue to make progress (matching emacs/codex semantics).
    ///
    /// Returns `true` when bytes were moved into the kill buffer.
    pub fn kill_to_end_of_line(&mut self) -> bool {
        self.clear_input_history_navigation();
        if let Some((start, end)) = self.selection_range() {
            let sb = byte_index_at_char(&self.input, start);
            let eb = byte_index_at_char(&self.input, end);
            self.kill_buffer = self.input[sb..eb].to_string();
            self.delete_selection();
            return true;
        }
        let total_chars = char_count(&self.input);
        let cursor = self.cursor_position.min(total_chars);
        let start_byte = byte_index_at_char(&self.input, cursor);

        // Find the byte offset of the next '\n' (relative to the whole string)
        // or the end of the buffer if no newline exists at/after the cursor.
        let eol_byte = self.input[start_byte..]
            .find('\n')
            .map(|rel| start_byte + rel)
            .unwrap_or_else(|| self.input.len());

        let end_byte = if start_byte == eol_byte {
            // Cursor is at EOL — consume the newline itself if one is there.
            if eol_byte < self.input.len() {
                eol_byte + 1
            } else {
                return false;
            }
        } else {
            eol_byte
        };

        let removed: String = self.input[start_byte..end_byte].to_string();
        if removed.is_empty() {
            return false;
        }

        self.kill_buffer = removed;
        self.input.replace_range(start_byte..end_byte, "");
        // Cursor stays at the same character index (start of removed range).
        self.cursor_position = cursor;
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
        true
    }

    /// Insert the contents of the kill buffer at the cursor, advancing it.
    /// The kill buffer is left intact so multiple yanks duplicate the text.
    /// Returns `true` if any text was inserted.
    pub fn yank(&mut self) -> bool {
        if self.kill_buffer.is_empty() {
            return false;
        }
        self.delete_selection();
        self.clear_input_history_navigation();
        let text = self.kill_buffer.clone();
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert_str(byte_index, &text);
        self.cursor_position = cursor + char_count(&text);
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
        true
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
        self.needs_redraw = true;
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < char_count(&self.input) {
            self.cursor_position += 1;
            self.needs_redraw = true;
        }
    }

    pub fn move_cursor_start(&mut self) {
        self.cursor_position = 0;
        self.needs_redraw = true;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = char_count(&self.input);
        self.needs_redraw = true;
    }

    /// In a multiline composer, jump to the start of the current line.
    /// On single-line input this is equivalent to `move_cursor_start`.
    pub fn move_cursor_line_start(&mut self) {
        let byte_pos = byte_index_at_char(&self.input, self.cursor_position);
        let before = &self.input[..byte_pos];
        if let Some(last_nl_byte) = before.rfind('\n') {
            // Position after the '\n' (start of the current line).
            self.cursor_position = char_count(&self.input[..=last_nl_byte]);
        } else {
            self.cursor_position = 0;
        }
        self.needs_redraw = true;
    }

    /// In a multiline composer, jump to the end of the current line
    /// (just before the next `\n` or at the end of input).
    /// On single-line input this is equivalent to `move_cursor_end`.
    pub fn move_cursor_line_end(&mut self) {
        let search_start = byte_index_at_char(&self.input, self.cursor_position);
        if let Some(offset) = self.input[search_start..].find('\n') {
            self.cursor_position = char_count(&self.input[..search_start + offset]);
        } else {
            self.cursor_position = char_count(&self.input);
        }
        self.needs_redraw = true;
    }

    /// Move forward one word. Skips over the current word then any trailing
    /// whitespace to land on the first character of the next word.
    pub fn move_cursor_word_forward(&mut self) {
        let text = self.input.clone();
        let total = char_count(&text);
        let mut pos = self.cursor_position;
        if pos >= total {
            return;
        }
        // Skip non-whitespace (current word).
        while pos < total {
            let byte = byte_index_at_char(&text, pos);
            let ch = text[byte..].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            pos += 1;
        }
        // Skip whitespace.
        while pos < total {
            let byte = byte_index_at_char(&text, pos);
            let ch = text[byte..].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            pos += 1;
        }
        self.cursor_position = pos;
        self.needs_redraw = true;
    }

    /// Move backward one word. Skips leading whitespace then the preceding
    /// word to land on its first character.
    pub fn move_cursor_word_backward(&mut self) {
        let text = self.input.clone();
        let mut pos = self.cursor_position;
        if pos == 0 {
            return;
        }
        // Step back one so we're not already at the word start.
        pos -= 1;
        // Skip whitespace.
        while pos > 0 {
            let byte = byte_index_at_char(&text, pos);
            let ch = text[byte..].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            pos -= 1;
        }
        // Skip non-whitespace.
        while pos > 0 {
            let byte = byte_index_at_char(&text, pos - 1);
            let ch = text[byte..].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            pos -= 1;
        }
        self.cursor_position = pos;
        self.needs_redraw = true;
    }

    // === Selection helpers ===

    /// Return the (start, end) of the active selection, or `None`.
    /// `start` is inclusive, `end` is exclusive; both are char indices.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let total = char_count(&self.input);
        let anchor = self.selection_anchor?.min(total);
        let cursor = self.cursor_position.min(total);
        if anchor == cursor {
            return None;
        }
        Some(if anchor < cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        })
    }

    /// Return the selected text, or empty string if no selection.
    pub fn selected_text(&self) -> String {
        self.selection_range()
            .map(|(s, e)| {
                let sb = byte_index_at_char(&self.input, s);
                let eb = byte_index_at_char(&self.input, e);
                self.input[sb..eb].to_string()
            })
            .unwrap_or_default()
    }

    /// Delete the selected text, place cursor at the start of the deleted range.
    /// Returns true if a selection was deleted.
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        let sb = byte_index_at_char(&self.input, start);
        let eb = byte_index_at_char(&self.input, end);
        self.input.replace_range(sb..eb, "");
        self.cursor_position = start;
        self.selection_anchor = None;
        self.clear_input_history_navigation();
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
        true
    }

    /// Clear the selection without moving the cursor.
    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    // === Vim composer mode helpers ===

    /// Move the cursor to the start of the current logical line (vim `0`).
    pub fn vim_move_line_start(&mut self) {
        let text = self.input.clone();
        let cursor_byte = byte_index_at_char(&text, self.cursor_position);
        // Walk backward until we find a newline or the start of the string.
        let line_start_byte = text[..cursor_byte].rfind('\n').map_or(0, |idx| idx + 1);
        self.cursor_position = char_count(&text[..line_start_byte]);
        self.needs_redraw = true;
    }

    /// Move the cursor to the end of the current logical line (vim `$`).
    pub fn vim_move_line_end(&mut self) {
        let text = self.input.clone();
        let cursor_byte = byte_index_at_char(&text, self.cursor_position);
        // Walk forward to the next newline or end-of-string.
        let line_end_char = text[cursor_byte..].find('\n').map_or_else(
            || char_count(&text),
            |rel| char_count(&text[..cursor_byte + rel]),
        );
        self.cursor_position = line_end_char;
        self.needs_redraw = true;
    }

    /// Move forward one word (vim `w`).  Skips over the current word then any
    /// trailing whitespace to land on the first character of the next word.
    pub fn vim_move_word_forward(&mut self) {
        self.move_cursor_word_forward();
    }

    /// Move backward one word (vim `b`).  Skips leading whitespace then the
    /// preceding word to land on its first character.
    pub fn vim_move_word_backward(&mut self) {
        self.move_cursor_word_backward();
    }

    /// Delete the character under the cursor (vim `x`).
    pub fn vim_delete_char_under_cursor(&mut self) {
        self.auto_expand_oversized_paste();
        let total = char_count(&self.input);
        if self.cursor_position >= total {
            return;
        }
        let pos = self.cursor_position;
        remove_char_at(&mut self.input, pos);
        // Keep cursor in bounds after deletion.
        let new_total = char_count(&self.input);
        if self.cursor_position > 0 && self.cursor_position >= new_total {
            self.cursor_position = new_total.saturating_sub(1);
        }
        self.needs_redraw = true;
    }

    /// Delete the entire current logical line (vim `dd`).
    pub fn vim_delete_line(&mut self) {
        let text = self.input.clone();
        let cursor_byte = byte_index_at_char(&text, self.cursor_position);
        let line_start_byte = text[..cursor_byte].rfind('\n').map_or(0, |idx| idx + 1);
        let line_end_byte = text[cursor_byte..]
            .find('\n')
            .map_or(text.len(), |rel| cursor_byte + rel);

        // Include the trailing newline if present, or the leading newline for the
        // very last non-terminated line to avoid leaving a dangling newline.
        let (remove_start, remove_end) = if line_end_byte < text.len() {
            // There is a newline after the line — remove it too.
            (line_start_byte, line_end_byte + 1)
        } else if line_start_byte > 0 {
            // Last line without trailing newline — remove the preceding newline.
            (line_start_byte - 1, line_end_byte)
        } else {
            // Only line in the buffer.
            (line_start_byte, line_end_byte)
        };

        self.input.replace_range(remove_start..remove_end, "");
        self.cursor_position = char_count(&self.input[..remove_start]);
        self.needs_redraw = true;
    }

    /// Enter insert mode at the cursor (vim `i`).
    pub fn vim_enter_insert(&mut self) {
        self.vim_mode = VimMode::Insert;
        self.needs_redraw = true;
    }

    /// Enter insert mode after the cursor (vim `a`).
    pub fn vim_enter_append(&mut self) {
        let total = char_count(&self.input);
        if self.cursor_position < total {
            self.cursor_position += 1;
        }
        self.vim_mode = VimMode::Insert;
        self.needs_redraw = true;
    }

    /// Open a new line below and enter insert mode (vim `o`).
    pub fn vim_open_line_below(&mut self) {
        // Move to end of line, then insert a newline.
        self.vim_move_line_end();
        self.insert_char('\n');
        self.vim_mode = VimMode::Insert;
    }

    /// Return to Normal mode from Insert or Visual (vim `Esc`).
    pub fn vim_enter_normal(&mut self) {
        self.vim_mode = VimMode::Normal;
        self.vim_pending_d = false;
        // In Normal mode the cursor sits on a character, not after the last one.
        let total = char_count(&self.input);
        if self.cursor_position > 0 && self.cursor_position >= total {
            self.cursor_position = total.saturating_sub(1);
        }
        self.needs_redraw = true;
    }

    /// Returns `true` when vim mode is active and the composer is in Normal
    /// mode, which means character keys should NOT be inserted as text.
    #[must_use]
    pub fn vim_is_normal_mode(&self) -> bool {
        self.composer.vim_enabled && self.composer.vim_mode == VimMode::Normal
    }

    /// Returns `true` when vim mode is active and the composer is in Visual mode.
    #[must_use]
    pub fn vim_is_visual_mode(&self) -> bool {
        self.composer.vim_enabled && self.composer.vim_mode == VimMode::Visual
    }
}
