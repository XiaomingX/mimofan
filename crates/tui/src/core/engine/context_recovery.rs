//! Context-budget recovery for the engine (MECE split of `engine.rs`).
//!
//! This module owns the single, cohesive responsibility of **bringing an
//! over-budget conversation back under its token ceiling**. It is decoupled
//! from message sending, tool orchestration, and compaction *summary* assembly
//! (see `crate::compaction`), and only decides *how* to shrink the live
//! message window: emergency LLM compaction followed by a local oldest-message
//! trim. The four methods here are the only places that mutate the session
//! window for budget reasons, so the "what shrinks the context" logic lives in
//! exactly one file.

// Inherit every import the moved methods relied on from the parent engine
// module, then re-bind the few `context`-module symbols they need by name.
use super::*;
use super::context::{
    MIN_RECENT_MESSAGES_TO_KEEP, context_input_budget_for_route, route_output_reservation_for_route,
};

impl Engine {
    /// Memoized estimate of the current request input tokens.
    ///
    /// Cached on `(messages_revision, system-prompt fingerprint)`; the cache
    /// invalidates as soon as either input changes. Repeated callers (capacity
    /// checkpoints, `/status`, context inspector, TUI footer) all hit the
    /// cached value instead of re-tokenizing the whole window.
    pub(super) fn estimated_input_tokens(&mut self) -> usize {
        self.token_estimate_cache.lookup_or_compute(
            self.session.messages_revision,
            self.session.system_prompt.as_ref(),
            &self.session.messages,
        )
    }

    /// Budget snapshot for the live route, with the compaction trigger taken
    /// from the *configured* threshold rather than the module default.
    ///
    /// `compaction.token_threshold` is itself derived from the user's
    /// `compact_threshold` percentage of the route window (see
    /// `route_budget::compaction_threshold_for_route_at_percent`), so routing
    /// it through [`ContextBudget::with_trigger`] keeps the engine decision and
    /// the UI percentage reading off the same number instead of two parallel
    /// rules that can drift apart.
    pub(super) fn route_compaction_budget(&mut self) -> Option<ContextBudget> {
        let input_tokens = self.estimated_input_tokens();
        let window = crate::route_budget::route_context_window_tokens(
            self.api_provider,
            &self.session.model,
            self.active_route_limits,
        );
        // A zero threshold means "no token budget configured" in
        // `compaction::should_compact`, which then falls back to a message-count
        // rule. Pass `None` so the budget gate uses its percent-of-window
        // default instead of a clamped 1-token trigger that would fire on every
        // turn and leave the structural check as the only real gate.
        let configured_trigger = match self.config.compaction.token_threshold {
            0 => None,
            threshold => u64::try_from(threshold).ok(),
        };
        let output_cap = route_output_reservation_for_route(
            &self.session.model,
            window,
            self.active_route_limits,
        );
        Some(ContextBudget::with_trigger(
            u64::from(window),
            input_tokens as u64,
            u64::from(output_cap),
            configured_trigger,
        ))
    }

    /// Trim the oldest messages until the window is at or below
    /// `target_input_budget`, always keeping at least
    /// `MIN_RECENT_MESSAGES_TO_KEEP` messages so the tail is never emptied.
    pub(super) fn trim_oldest_messages_to_budget(&mut self, target_input_budget: usize) -> usize {
        let mut removed = 0usize;
        while self.session.messages.len() > MIN_RECENT_MESSAGES_TO_KEEP
            && self.estimated_input_tokens() > target_input_budget
        {
            self.session.messages.trim_front(1);
            self.session.bump_messages_revision();
            removed = removed.saturating_add(1);
        }
        removed
    }

    /// Emergency recovery when the context is (about to be) rejected by the
    /// provider: run LLM compaction with a forced-below-budget trigger, then
    /// locally trim the oldest messages until the window fits. Returns `true`
    /// if the request was brought under `target_budget`.
    pub(super) async fn recover_context_overflow(&mut self, client: &ApiClient, reason: &str) -> bool {
        let Some(target_budget) = context_input_budget_for_route(
            self.api_provider,
            &self.session.model,
            self.active_route_limits,
            0,
        ) else {
            return false;
        };

        let id = format!("compact_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let start_message = format!("Emergency context compaction started ({reason})");
        self.emit_compaction_started(id.clone(), true, start_message)
            .await;

        let before_tokens = self.estimated_input_tokens();
        let before_count = self.session.messages.len();

        let mut retries_used = 0u32;
        let mut summary_prompt = None;
        let mut compacted_messages: Vec<Message> = self.session.messages.clone().into();

        // Emergency path: the provider already rejected (or is about to reject)
        // this context, so the trigger must sit strictly below the spendable
        // budget regardless of what the user configured. Build the same
        // `ContextBudget` the auto path uses, but force the trigger down to
        // `target_budget - 1` so `should_compact` is unambiguously true — this
        // keeps the "what counts as too full" arithmetic in one module instead
        // of a bespoke min/max here.
        let forced_trigger = self
            .config
            .compaction
            .token_threshold
            .min(target_budget.saturating_sub(1))
            .max(1);
        let mut forced_config = self.config.compaction.clone();
        forced_config.enabled = true;
        forced_config.token_threshold = forced_trigger;

        match compact_messages_safe_with_objective(
            client,
            &self.session.messages,
            &forced_config,
            Some(&self.session.workspace),
            None,
            None,
            None,
        )
        .await
        {
            Ok(result) => {
                retries_used = result.retries_used;
                compacted_messages = result.messages;
                summary_prompt = result.summary_prompt;
            }
            Err(err) => {
                let _ = self
                    .tx_event
                    .send(Event::status(format!(
                        "Emergency compaction API pass failed: {err}. Falling back to local trim."
                    )))
                    .await;
            }
        }

        if !compacted_messages.is_empty() || self.session.messages.is_empty() {
            self.session.messages = compacted_messages.into();
        }
        self.merge_compaction_summary(summary_prompt);

        let trimmed = self.trim_oldest_messages_to_budget(target_budget);
        self.emit_session_updated().await;
        let after_tokens = self.estimated_input_tokens();
        let after_count = self.session.messages.len();
        let recovered = after_tokens <= target_budget
            && (after_tokens < before_tokens || after_count < before_count || trimmed > 0);

        if recovered {
            let removed = before_count.saturating_sub(after_count);
            let mut details = format!(
                "Emergency compaction complete: {before_count} → {after_count} messages ({removed} removed), ~{before_tokens} → ~{after_tokens} tokens"
            );
            if retries_used > 0 {
                details.push_str(&format!(" ({retries_used} retries)"));
            }
            if trimmed > 0 {
                details.push_str(&format!(", trimmed {trimmed} oldest"));
            }
            self.emit_compaction_completed(
                id,
                true,
                details.clone(),
                Some(before_count),
                Some(after_count),
            )
            .await;
            let _ = self.tx_event.send(Event::status(details)).await;
            return true;
        }

        let message = format!(
            "Emergency context compaction failed to reduce request below model limit \
             (estimate ~{after_tokens} tokens, budget ~{target_budget})."
        );
        self.emit_compaction_failed(id, true, message.clone()).await;
        let _ = self.tx_event.send(Event::status(message)).await;
        false
    }
}
