use std::collections::HashSet;
use std::time::Instant;

use mimofan_config::ProviderChain;
use mimofan_config::route::RouteLimits;

use crate::compaction::CompactionConfig;
use crate::config::{ApiProvider, DEFAULT_TEXT_MODEL};
use crate::tui::history::HistoryCell;
use crate::tui::history_search::{ComposerHistorySearch, InputHistoryDraft};

use super::helpers::{
    byte_index_at_char, char_count, looks_like_slash_command_input, normalize_paste_text,
};
use super::state::{App, QueuedMessage, ReasoningEffort, StatusToastLevel, SubmitDisposition};

// === Constants ===

const MAX_SUBMITTED_INPUT_CHARS: usize = 16_000;
const MAX_COMPOSER_DISPLAY_CHARS: usize = 4_000;
const MAX_DRAFT_HISTORY: usize = 50;

impl App {
    /// Move the cursor down one logical line within the buffer (vim `j`).
    /// Falls back to history-down when already on the last line.
    pub fn vim_move_down(&mut self) {
        let text = self.input.clone();
        let total = char_count(&text);
        if self.cursor_position >= total {
            self.history_down();
            return;
        }
        let cursor_byte = byte_index_at_char(&text, self.cursor_position);
        let rest = &text[cursor_byte..];
        if let Some(rel_nl) = rest.find('\n') {
            // Column offset on the current line.
            let line_start_byte = text[..cursor_byte].rfind('\n').map_or(0, |i| i + 1);
            let col = char_count(&text[line_start_byte..cursor_byte]);
            let next_line_start = cursor_byte + rel_nl + 1;
            let next_line = &text[next_line_start..];
            let next_line_len = next_line.find('\n').unwrap_or(next_line.len());
            let next_line_char_len =
                char_count(&text[next_line_start..next_line_start + next_line_len]);
            let target_col = col.min(next_line_char_len);
            self.cursor_position = char_count(&text[..next_line_start]) + target_col;
            self.needs_redraw = true;
        } else {
            self.history_down();
        }
    }

    /// Move the cursor up one logical line within the buffer (vim `k`).
    /// Falls back to history-up when already on the first line.
    pub fn vim_move_up(&mut self) {
        let text = self.input.clone();
        let cursor_byte = byte_index_at_char(&text, self.cursor_position);
        if let Some(prev_nl) = text[..cursor_byte].rfind('\n') {
            // Column on the current line.
            let line_start_byte = prev_nl + 1;
            let col = char_count(&text[line_start_byte..cursor_byte]);
            // Find start of the previous line.
            let prev_line_end = prev_nl; // byte of the newline itself
            let prev_start = text[..prev_line_end].rfind('\n').map_or(0, |i| i + 1);
            let prev_line_len = char_count(&text[prev_start..prev_line_end]);
            let target_col = col.min(prev_line_len);
            self.cursor_position = char_count(&text[..prev_start]) + target_col;
            self.needs_redraw = true;
        } else {
            self.history_up();
        }
    }

    pub fn clear_input(&mut self) {
        self.clear_input_history_navigation();
        self.input.clear();
        self.cursor_position = 0;
        // Prevent stale oversized-paste state from leaking when the user
        // clears the composer or navigates to a different input (#3263).
        self.pending_paste_reference = None;
        self.oversized_paste_full_text = None;
        self.selection_anchor = None;
        self.selected_attachment_index = None;
        self.slash_menu_selected = 0;
        self.slash_menu_hidden = false;
        self.paste_burst.clear_after_explicit_paste();
        self.needs_redraw = true;
    }

    pub fn clear_input_recoverable(&mut self) {
        self.stash_current_input_for_recovery();
        self.clear_input();
    }

    pub fn stash_current_input_for_recovery(&mut self) {
        // Before stashing, expand any truncated paste so the saved draft
        // contains the full text, not the truncated preview (#3263).
        self.auto_expand_oversized_paste();
        let draft = self.input.clone();
        if draft.trim().is_empty() {
            self.clear_undo_buffer = None;
            return;
        }
        self.clear_undo_buffer = Some(draft.clone());
        self.remember_draft_for_recovery(draft);
    }

    fn remember_draft_for_recovery(&mut self, draft: String) {
        if draft.trim().is_empty() {
            return;
        }
        self.draft_history.retain(|existing| existing != &draft);
        self.draft_history.push_back(draft);
        while self.draft_history.len() > MAX_DRAFT_HISTORY {
            let _ = self.draft_history.pop_front();
        }
    }

    pub fn start_history_search(&mut self) {
        if self.composer_history_search.is_some() {
            return;
        }
        // Expand any truncated paste first so the history search seed
        // contains the full text, not the truncated preview (#3263).
        self.auto_expand_oversized_paste();
        self.composer_history_search = Some(ComposerHistorySearch::new(
            self.input.clone(),
            self.cursor_position,
        ));
        self.slash_menu_hidden = true;
        self.mention_menu_hidden = true;
        self.paste_burst.clear_after_explicit_paste();
        self.status_message = Some("History search: type to filter, Enter accepts".to_string());
        self.needs_redraw = true;
    }

    pub fn is_history_search_active(&self) -> bool {
        self.composer_history_search.is_some()
    }

    pub fn history_search_query(&self) -> Option<&str> {
        self.composer_history_search
            .as_ref()
            .map(|search| search.query.as_str())
    }

    pub fn history_search_selected_index(&self) -> usize {
        self.composer_history_search
            .as_ref()
            .map_or(0, |search| search.selected)
    }

    pub fn composer_display_input(&self) -> &str {
        self.history_search_query().unwrap_or(&self.input)
    }

    pub fn composer_display_cursor(&self) -> usize {
        self.composer_history_search
            .as_ref()
            .map_or(self.cursor_position, |search| char_count(&search.query))
    }

    pub fn history_search_matches(&self) -> Vec<String> {
        let Some(query) = self.history_search_query() else {
            return Vec::new();
        };
        self.history_search_matches_for_query(query)
    }

    fn history_search_matches_for_query(&self, query: &str) -> Vec<String> {
        let normalized_query = query.trim().to_lowercase();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut matches = Vec::new();

        for candidate in self
            .draft_history
            .iter()
            .rev()
            .chain(self.input_history.iter().rev())
        {
            if candidate.trim().is_empty() || !seen.insert(candidate.as_str()) {
                continue;
            }
            if normalized_query.is_empty() || candidate.to_lowercase().contains(&normalized_query) {
                matches.push(candidate.clone());
            }
        }

        matches
    }

    fn clamp_history_search_selection(&mut self) {
        let Some(search) = self.composer_history_search.as_ref() else {
            return;
        };
        let selected = search.selected;
        let query = search.query.clone();
        let match_count = self.history_search_matches_for_query(&query).len();
        if let Some(search) = self.composer_history_search.as_mut() {
            search.selected = if match_count == 0 {
                0
            } else {
                selected.min(match_count.saturating_sub(1))
            };
        }
    }

    pub fn history_search_insert_char(&mut self, ch: char) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.push(ch);
            search.selected = 0;
            self.status_message = Some("History search: Enter accepts, Esc restores".to_string());
            self.needs_redraw = true;
        }
    }

    pub fn history_search_insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.push_str(&normalize_paste_text(text));
            search.selected = 0;
            self.status_message = Some("History search: Enter accepts, Esc restores".to_string());
            self.needs_redraw = true;
        }
    }

    pub fn history_search_backspace(&mut self) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.pop();
            search.selected = 0;
            self.needs_redraw = true;
        }
        self.clamp_history_search_selection();
    }

    pub fn history_search_select_previous(&mut self) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.selected = search.selected.saturating_sub(1);
            self.needs_redraw = true;
        }
    }

    pub fn history_search_select_next(&mut self) {
        let Some(search) = self.composer_history_search.as_ref() else {
            return;
        };
        let query = search.query.clone();
        let selected = search.selected;
        let match_count = self.history_search_matches_for_query(&query).len();
        if let Some(search) = self.composer_history_search.as_mut()
            && match_count > 0
        {
            search.selected = (selected + 1).min(match_count.saturating_sub(1));
            self.needs_redraw = true;
        }
    }

    pub fn accept_history_search(&mut self) -> bool {
        let Some(search) = self.composer_history_search.take() else {
            return false;
        };
        let matches = self.history_search_matches_for_query(&search.query);
        if let Some(selected) = matches
            .get(search.selected.min(matches.len().saturating_sub(1)))
            .cloned()
        {
            self.input = selected;
            self.cursor_position = char_count(&self.input);
            self.history_index = None;
            self.status_message = Some("History match inserted into composer".to_string());
            self.needs_redraw = true;
            true
        } else {
            self.composer_history_search = Some(search);
            self.status_message = Some("No history matches".to_string());
            self.needs_redraw = true;
            false
        }
    }

    pub fn cancel_history_search(&mut self) {
        let Some(search) = self.composer_history_search.take() else {
            return;
        };
        self.input = search.pre_search_input;
        self.cursor_position = search.pre_search_cursor.min(char_count(&self.input));
        self.status_message = Some("History search canceled".to_string());
        self.needs_redraw = true;
    }

    pub fn submit_input(&mut self) -> Option<String> {
        if self.input.trim().is_empty() {
            self.paste_burst.clear_after_explicit_paste();
            return None;
        }
        // Safety net: if any earlier path filled the buffer above the
        // safety cap without going through `insert_paste_text`, fold it
        // into a workspace paste file now (#553). Bracketed pastes hit
        // the consolidation in `insert_paste_text` first, so the user
        // sees the @mention in the composer before submission.
        self.consolidate_large_input_if_oversized();
        // If consolidation created a paste file, restore the full text and
        // append the @mention so the model can read the complete content
        // while the composer stays editable (#3263).
        let mut input = self
            .oversized_paste_full_text
            .take()
            .unwrap_or_else(|| self.input.clone());
        if let Some(reference) = self.pending_paste_reference.take() {
            if !input.is_empty() && !input.ends_with('\n') {
                input.push('\n');
            }
            input.push_str(&reference);
        }
        if !looks_like_slash_command_input(&input) {
            self.input_history.push(input.clone());
            if self.max_input_history == 0 {
                self.input_history.clear();
            } else if self.input_history.len() > self.max_input_history {
                let excess = self.input_history.len() - self.max_input_history;
                self.input_history.drain(0..excess);
            }
            // Mirror to the persisted cross-session history (#366) so
            // arrow-up recall works across restarts. Best-effort write —
            // see `composer_history::append_history` for failure modes.
            crate::composer_history::append_history(&input);
        }
        self.history_index = None;
        self.history_navigation_draft = None;
        self.clear_input();
        Some(input)
    }

    pub fn restore_last_submitted_prompt_if_empty(&mut self) -> bool {
        if !self.input.is_empty() {
            return false;
        }
        let Some(prompt) = self
            .last_submitted_prompt
            .as_deref()
            .filter(|prompt| !prompt.is_empty())
        else {
            return false;
        };

        self.input = prompt.to_string();
        self.cursor_position = char_count(&self.input);
        self.history_index = None;
        self.history_navigation_draft = None;
        self.selected_attachment_index = None;
        self.needs_redraw = true;
        true
    }

    /// Restore the last cleared input if the composer is empty.
    /// Returns `true` if the input was restored.
    pub fn restore_last_cleared_input_if_empty(&mut self) -> bool {
        if !self.input.is_empty() {
            return false;
        }
        let Some(saved) = self.clear_undo_buffer.take().filter(|s| !s.is_empty()) else {
            return false;
        };

        self.input = saved;
        self.cursor_position = char_count(&self.input);
        self.history_index = None;
        self.history_navigation_draft = None;
        self.selected_attachment_index = None;
        self.slash_menu_selected = 0;
        self.slash_menu_hidden = false;
        self.needs_redraw = true;
        self.clear_undo_buffer = None;
        true
    }

    /// Composer-Enter dispatch. Returns `Some(input)` when the press should
    /// fire a submit; `None` when Enter was absorbed (paste-burst Enter
    /// suppression — see #1073).
    ///
    /// Two suppression cases are handled here. Both are silent: nothing
    /// visible happens beyond the text gaining a newline.
    ///
    /// 1. **Burst active.** A paste burst is currently being assembled in
    ///    `paste_burst.buffer`. The Enter is part of the paste content;
    ///    append `\n` to the buffer so the next flush includes it, do not
    ///    submit, and extend the suppression window so a follow-on Enter
    ///    (i.e. the *next* line of a multi-line paste) is also absorbed.
    /// 2. **Window open after flush.** A burst just flushed into
    ///    `self.input`, but the suppression window is still alive. The
    ///    Enter is the trailing newline of that paste, not a submit gesture
    ///    by the user. Insert `\n` directly into the composer text and
    ///    re-arm the window.
    ///
    /// Outside both cases the call falls through to [`Self::submit_input`]
    /// unchanged so normal Enter-to-send behaviour is preserved.
    pub fn handle_composer_enter(&mut self) -> Option<String> {
        if self.use_paste_burst_detection {
            let now = Instant::now();
            if self
                .paste_burst
                .newline_should_insert_instead_of_submit(now)
            {
                if !self.paste_burst.append_newline_if_active(now) {
                    self.insert_char('\n');
                    self.paste_burst.extend_window(now);
                }
                self.needs_redraw = true;
                return None;
            }
        }
        self.submit_input()
    }

    /// Public wrapper around [`Self::consolidate_large_input`] that no-ops
    /// when the current input fits inside the safety cap. Both the paste-
    /// insert path (visible-before-submit) and the submit-time safety net
    /// route through here, so the cap is enforced exactly once even when
    /// both paths fire on the same buffer.
    fn consolidate_large_input_if_oversized(&mut self) {
        if char_count(&self.input) > MAX_SUBMITTED_INPUT_CHARS {
            self.consolidate_large_input();
        }
    }

    /// When the composer input exceeds [`MAX_SUBMITTED_INPUT_CHARS`], write
    /// the full content to a timestamped paste file under
    /// `.mimofan/pastes/` and replace `self.input` with an `@`-mention
    /// pointing at it so the model can read the full content via the
    /// normal file-mention resolution path (#553).
    fn consolidate_large_input(&mut self) {
        let full_input = std::mem::take(&mut self.input);
        self.cursor_position = 0;

        let now = chrono::Local::now();
        let suffix = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let filename = format!("paste-{}-{}.md", now.format("%Y-%m-%d-%H%M%S"), suffix);
        let rel_path = format!(".mimofan/pastes/{filename}");

        let pastes_dir = self.workspace.join(".mimofan/pastes");
        if let Err(e) = std::fs::create_dir_all(&pastes_dir) {
            // Fallback: keep a truncated version so we don't lose the
            // user's input entirely when the filesystem is unhappy.
            self.input = full_input.chars().take(MAX_SUBMITTED_INPUT_CHARS).collect();
            self.cursor_position = char_count(&self.input);
            self.push_status_toast(
                format!("Failed to create paste directory: {e}"),
                StatusToastLevel::Error,
                Some(8_000),
            );
            return;
        }

        let file_path = self.workspace.join(&rel_path);
        if let Err(e) = std::fs::write(&file_path, &full_input) {
            self.input = full_input.chars().take(MAX_SUBMITTED_INPUT_CHARS).collect();
            self.cursor_position = char_count(&self.input);
            self.push_status_toast(
                format!("Failed to write paste file: {e}"),
                StatusToastLevel::Error,
                Some(8_000),
            );
            return;
        }

        // Keep a truncated preview in the composer so the user can still
        // select, copy, and edit it, while the full text is stored for
        // model submission. The @mention is appended at submit time (#3263).
        self.pending_paste_reference = Some(format!("@{rel_path}"));
        self.oversized_paste_full_text = Some(full_input.clone());
        let display_chars = char_count(&full_input).min(MAX_COMPOSER_DISPLAY_CHARS);
        let mut truncated: String = full_input.chars().take(display_chars).collect();
        if char_count(&full_input) > MAX_COMPOSER_DISPLAY_CHARS {
            truncated.push_str("\n\n---\n(content truncated for display — start typing to expand; full text sent to model)");
        }
        self.input = truncated;
        self.cursor_position = 0;
        self.push_status_toast(
            "Large paste backed up to file — the model will receive the full content.",
            StatusToastLevel::Info,
            Some(5_000),
        );
    }

    pub fn queue_message(&mut self, message: QueuedMessage) {
        self.queued_messages.push_back(message);
    }

    pub fn pop_queued_message(&mut self) -> Option<QueuedMessage> {
        self.queued_messages.pop_front()
    }

    pub fn remove_queued_message(&mut self, index: usize) -> Option<QueuedMessage> {
        self.queued_messages.remove(index)
    }

    pub fn queued_message_count(&self) -> usize {
        self.queued_messages.len()
    }

    /// Pop the most-recently queued message back into the composer for editing
    /// (issue #85 — ↑ affordance). The popped message is parked in
    /// [`Self::queued_draft`] so the next Enter re-queues it carrying its
    /// original skill instruction. No-op if the composer already has typed
    /// content or a draft is already being edited — surfacing the affordance
    /// would be ambiguous in either case.
    ///
    /// Returns `true` when the composer state was mutated.
    pub fn pop_last_queued_into_draft(&mut self) -> bool {
        if !self.input.is_empty() || self.queued_draft.is_some() {
            return false;
        }
        let Some(msg) = self.queued_messages.pop_back() else {
            return false;
        };
        self.input = msg.display.clone();
        self.cursor_position = char_count(&self.input);
        self.selected_attachment_index = None;
        self.queued_draft = Some(msg);
        self.needs_redraw = true;
        true
    }

    /// Stop editing a queued follow-up and put the original queued message back
    /// at the tail where [`Self::pop_last_queued_into_draft`] took it from.
    pub fn cancel_queued_draft_edit(&mut self) -> bool {
        let Some(draft) = self.queued_draft.take() else {
            return false;
        };
        self.queued_messages.push_back(draft);
        self.clear_input_recoverable();
        self.needs_redraw = true;
        true
    }

    /// Park a legacy pending steer. New keyboard handling routes running-turn
    /// drafts through Enter (same-turn steer) or Tab (next-turn follow-up).
    pub fn push_pending_steer(&mut self, message: QueuedMessage) {
        self.pending_steers.push_back(message);
        self.submit_pending_steers_after_interrupt = true;
        self.needs_redraw = true;
    }

    /// Drain the pending-steer queue and clear the resend flag. Returns the
    /// messages in submit order (oldest first).
    pub fn drain_pending_steers(&mut self) -> Vec<QueuedMessage> {
        self.submit_pending_steers_after_interrupt = false;
        if self.pending_steers.is_empty() {
            return Vec::new();
        }
        self.needs_redraw = true;
        self.pending_steers.drain(..).collect()
    }

    /// Decide how to route a fresh composer submit.
    ///
    /// #382 / v0.8.44: when the model is busy but not actively streaming
    /// (waiting on tool results, sub-agents, or shell commands), Enter tries
    /// to steer into the current turn. If steering fails, the message queues.
    /// During active streaming, Enter always queues to avoid interrupting
    /// in-flight reasoning. Ctrl+Enter forces Steer in all busy states.
    ///
    /// Truth table:
    ///   offline=F, busy=F           → Immediate
    ///   offline=F, busy=T+streaming → Queue
    ///   offline=F, busy=T+waiting   → Steer (fallback Queue)
    ///   offline=T, busy=*           → Queue
    #[must_use]
    pub fn decide_submit_disposition(&self) -> SubmitDisposition {
        if self.offline_mode {
            return SubmitDisposition::Queue;
        }
        if !self.is_loading {
            return SubmitDisposition::Immediate;
        }
        // Busy but not streaming text: model is waiting on tool results or
        // sub-agents — steer so the new message reaches the engine promptly
        // instead of sitting in the queue until the current turn finishes.
        if self.streaming_message_index.is_none() {
            return SubmitDisposition::Steer;
        }
        // Actively streaming: queue to avoid interrupting in-flight reasoning.
        SubmitDisposition::Queue
    }

    /// Mark the in-flight streaming Assistant cell as interrupted: prepend
    /// `[interrupted]` to whatever streamed so far (so the user can see what
    /// was salvaged) and flip `streaming` off so the spinner halts. No-op if
    /// no Assistant cell is currently streaming.
    ///
    /// Deliberate divergence from openai/codex which discards partial output
    /// on abort — V4 thinking is expensive and the user usually wants to see
    /// what the model produced before steering.
    pub fn finalize_streaming_assistant_as_interrupted(&mut self) {
        let Some(index) = self.streaming_message_index.take() else {
            return;
        };
        if let Some(HistoryCell::Assistant { content, streaming }) = self.history.get_mut(index) {
            *streaming = false;
            if content.is_empty() {
                *content = "[interrupted]".to_string();
            } else if !content.starts_with("[interrupted]") {
                content.insert_str(0, "[interrupted] ");
            }
        }
        self.bump_history_cell(index);
    }

    pub fn history_up(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        if self.history_index.is_none() {
            // Expand truncated paste first so the saved draft contains the
            // full text instead of the truncated preview (#3263).
            self.auto_expand_oversized_paste();
            self.history_navigation_draft = Some(InputHistoryDraft {
                input: self.input.clone(),
                cursor: self.cursor_position,
            });
        }
        let new_index = match self.history_index {
            None => self.input_history.len().saturating_sub(1),
            Some(i) => i.saturating_sub(1),
        };
        self.history_index = Some(new_index);
        self.input = self.input_history[new_index].clone();
        self.cursor_position = char_count(&self.input);
        self.selection_anchor = None;
        self.selected_attachment_index = None;
        self.slash_menu_hidden = false;
        self.paste_burst.clear_after_explicit_paste();
    }

    pub fn history_down(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        match self.history_index {
            None => {}
            Some(i) => {
                if i + 1 < self.input_history.len() {
                    self.history_index = Some(i + 1);
                    self.input = self.input_history[i + 1].clone();
                    self.cursor_position = char_count(&self.input);
                    self.selection_anchor = None;
                    self.selected_attachment_index = None;
                    self.slash_menu_hidden = false;
                    self.paste_burst.clear_after_explicit_paste();
                } else {
                    self.history_index = None;
                    if let Some(draft) = self.history_navigation_draft.take() {
                        self.input = draft.input;
                        self.cursor_position = draft.cursor.min(char_count(&self.input));
                        self.selection_anchor = None;
                        self.selected_attachment_index = None;
                        self.slash_menu_hidden = false;
                        self.paste_burst.clear_after_explicit_paste();
                        self.needs_redraw = true;
                    } else {
                        self.clear_input();
                    }
                }
            }
        }
    }

    pub(crate) fn clear_input_history_navigation(&mut self) {
        self.history_index = None;
        self.history_navigation_draft = None;
    }

    /// Retry a `try_lock` up to `retries` times with a 1ms pause between
    /// attempts. Returns `Some(guard)` on success, `None` if the lock
    /// remains contended after all retries.
    fn retry_lock<T>(
        mutex: &tokio::sync::Mutex<T>,
        retries: u32,
    ) -> Option<tokio::sync::MutexGuard<'_, T>> {
        for _ in 0..retries {
            if let Ok(guard) = mutex.try_lock() {
                return Some(guard);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        None
    }

    pub fn clear_todos(&mut self) -> bool {
        // Clear the todo list (the sidebar checklist). Retry with try_lock
        // so /clear always resets todos even when the engine briefly holds
        // the mutex during tool execution.
        let todos_cleared = if let Some(mut todos) = Self::retry_lock(&self.todos, 100) {
            todos.clear();
            true
        } else {
            false
        };
        // Also clear the plan state — /clear means a full reset.
        if let Some(mut plan) = Self::retry_lock(&self.plan_state, 100) {
            *plan = crate::tools::plan::PlanState::default();
        }
        todos_cleared
    }

    pub fn update_model_compaction_budget(&mut self) {
        let model = self.effective_model_for_budget().to_string();
        self.compact_threshold = crate::route_budget::compaction_threshold_for_route_at_percent(
            self.api_provider,
            &model,
            self.active_route_limits,
            self.compact_threshold_percent,
        );
        if !self.auto_compact_user_configured {
            self.auto_compact = crate::route_budget::auto_compact_default_for_route(
                self.api_provider,
                &model,
                self.active_route_limits,
            );
        }
    }

    pub fn set_active_route_limits(&mut self, limits: RouteLimits) {
        self.active_route_limits = crate::route_budget::known_route_limits(limits);
    }

    pub fn set_model_selection(&mut self, model: String) {
        let auto_model = model.trim().eq_ignore_ascii_case("auto");
        self.model = if auto_model {
            "auto".to_string()
        } else {
            model
        };
        self.auto_model = auto_model;
        self.last_effective_model = None;
        self.last_effective_reasoning_effort = None;
        if auto_model {
            self.reasoning_effort = ReasoningEffort::Auto;
        } else {
            self.reasoning_effort = self
                .reasoning_effort
                .normalize_for_provider(self.api_provider);
        }
    }

    pub fn model_selection_for_persistence(&self) -> String {
        if self.auto_model || self.model.trim().eq_ignore_ascii_case("auto") {
            "auto".to_string()
        } else {
            self.model.clone()
        }
    }

    pub fn accepts_custom_model_ids(&self) -> bool {
        self.model_ids_passthrough
            || crate::config::provider_passes_model_through(self.api_provider)
    }

    pub fn effective_model_for_budget(&self) -> &str {
        if self.auto_model {
            return self
                .last_effective_model
                .as_deref()
                .filter(|model| *model != "auto")
                .unwrap_or(DEFAULT_TEXT_MODEL);
        }
        &self.model
    }

    pub fn model_display_label(&self) -> String {
        if self.auto_model {
            if let Some(effective) = self.last_effective_model.as_deref()
                && effective != "auto"
            {
                return format!("auto: {effective}");
            }
            return "auto".to_string();
        }
        self.model.clone()
    }

    pub fn reasoning_effort_display_label(&self) -> String {
        if self.auto_model || self.reasoning_effort == ReasoningEffort::Auto {
            if let Some(effective) = self.last_effective_reasoning_effort {
                return format!(
                    "auto: {}",
                    effective.display_label_for_provider(self.api_provider)
                );
            }
            return "auto".to_string();
        }
        self.reasoning_effort
            .display_label_for_provider(self.api_provider)
            .to_string()
    }

    pub fn compaction_config(&self) -> CompactionConfig {
        CompactionConfig {
            enabled: self.auto_compact,
            token_threshold: self.compact_threshold,
            model: self.effective_model_for_budget().to_string(),
            custom_instructions: self.compact_instructions.clone(),
            ..Default::default()
        }
    }

    pub fn fallback_chain_entries(&self) -> Vec<(usize, ApiProvider, bool)> {
        let Some(chain) = &self.provider_chain else {
            return Vec::new();
        };
        let position = chain.position();
        chain
            .providers()
            .iter()
            .enumerate()
            .map(|(index, provider)| (index, ApiProvider::from_kind(*provider), index == position))
            .collect()
    }

    pub fn fallback_chain_position(&self) -> Option<usize> {
        self.provider_chain.as_ref().map(ProviderChain::position)
    }

    pub fn fallback_chain_len(&self) -> usize {
        self.provider_chain
            .as_ref()
            .map_or(0, |chain| chain.providers().len())
    }

    /// Whether a fallback chain entry can serve a turn right now (#2574).
    ///
    /// Mirrors the provider picker's eligibility: hosted providers need a key
    /// (`has_api_key_for`, captured into `provider_readiness` at startup) while
    /// self-hosted providers (Ollama/vLLM/SGLang) are always ready. Providers
    /// absent from the snapshot default to ready so an unknown entry is tried
    /// rather than silently skipped.
    fn fallback_provider_is_ready(&self, provider: ApiProvider) -> bool {
        // Circuit breaker (#795): a provider that has tripped open after
        // repeated recoverable failures is skipped during its cooldown window
        // so the fallback chain probes a healthier candidate instead.
        if let Ok(mut breaker) = self.provider_breaker.lock() {
            if !breaker.allow_request(provider.as_str(), std::time::Instant::now()) {
                return false;
            }
        }
        self.provider_readiness
            .iter()
            .find_map(|(candidate, ready)| (*candidate == provider).then_some(*ready))
            .unwrap_or(true)
    }

    /// Advance to the next *eligible* provider in the fallback chain (#2574).
    ///
    /// Walks the chain from the current position, skipping entries that are not
    /// ready (hosted providers missing auth) and recording a clear note for each
    /// skip. Local providers are always eligible. Returns the first ready
    /// provider, or `None` (with an exhaustion reason) when every remaining entry
    /// is unready or the end of the chain is reached. `ProviderChain::advance`
    /// stays pure — the readiness filtering lives here at the App level.
    ///
    /// Note: auth-rejection (401) failures never reach this path; the caller
    /// excludes them from fallback so a bad key does not silently rotate
    /// providers (see `apply_engine_error_to_app`).
    ///
    /// Local/private policy (#2574): when the chain's primary provider is a
    /// self-hosted / local runtime, cloud candidates are skipped with a clear
    /// note so a local/private route never silently falls back out to a hosted
    /// provider. Self-hosted siblings remain eligible. The policy is anchored
    /// to the original primary; a cloud primary may still hop through a local
    /// runtime and then back to another cloud fallback.
    pub fn advance_fallback(&mut self, reason: impl Into<String>) -> Option<ApiProvider> {
        let reason = reason.into();
        self.provider_chain.as_ref()?;

        let origin_is_local = self
            .provider_chain
            .as_ref()
            .and_then(|chain| chain.providers().first().copied())
            .map(ApiProvider::from_kind)
            .is_some_and(ApiProvider::is_self_hosted);

        let mut skip_notes: Vec<String> = Vec::new();
        let mut chosen: Option<ApiProvider> = None;
        while let Some(next_kind) = self
            .provider_chain
            .as_mut()
            .and_then(ProviderChain::advance)
        {
            let candidate = ApiProvider::from_kind(next_kind);
            if origin_is_local && !candidate.is_self_hosted() {
                skip_notes.push(format!(
                    "skipped {}: local/private policy (no local->cloud fallback)",
                    candidate.as_str()
                ));
                continue;
            }
            if self.fallback_provider_is_ready(candidate) {
                chosen = Some(candidate);
                break;
            }
            skip_notes.push(format!("skipped {}: needs auth", candidate.as_str()));
        }

        let skipped = if skip_notes.is_empty() {
            String::new()
        } else {
            format!(" ({})", skip_notes.join("; "))
        };

        let Some(next_provider) = chosen else {
            let total = self
                .provider_chain
                .as_ref()
                .map_or(0, |chain| chain.providers().len());
            self.last_fallback_reason = Some(format!(
                "Fallback chain exhausted after {total} provider(s): {reason}{skipped}"
            ));
            return None;
        };

        self.api_provider = next_provider;
        self.last_fallback_reason = Some(format!(
            "Fell back to {} after recoverable provider error: {reason}{skipped}",
            next_provider.as_str()
        ));
        Some(next_provider)
    }

    pub fn is_fallback_active(&self) -> bool {
        self.provider_chain
            .as_ref()
            .is_some_and(ProviderChain::is_fallback_active)
    }
}
