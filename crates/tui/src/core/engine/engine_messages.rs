//! Engine message/goal event helpers.
//!
//! Extracted from `engine.rs` to keep the core engine module focused.
//! These are inherent `impl Engine` methods; they access `Engine`'s private
//! fields because this submodule is a child of `core::engine`.

use super::*;

impl Engine {
    pub(crate) async fn emit_session_updated(&self) {
        let _ = self
            .tx_event
            .send(Event::SessionUpdated {
                session_id: self.session.id.clone(),
                messages: self.session.messages.clone().into(),
                system_prompt: self.session.system_prompt.clone(),
                model: self.session.model.clone(),
                workspace: self.session.workspace.clone(),
            })
            .await;
    }

    pub(crate) fn goal_snapshot_for_event(&self) -> Option<GoalSnapshot> {
        match self.config.goal_state.lock() {
            Ok(state) => {
                let snapshot = state.snapshot();
                snapshot.objective.is_some().then_some(snapshot)
            }
            Err(err) => {
                tracing::warn!("goal state lock poisoned while emitting goal update: {err}");
                None
            }
        }
    }

    pub(crate) async fn emit_goal_updated(&self) {
        if let Some(snapshot) = self.goal_snapshot_for_event() {
            let _ = self.tx_event.send(Event::GoalUpdated { snapshot }).await;
        }
    }

    pub(crate) fn record_goal_usage_for_turn(&self, usage: &Usage, elapsed: std::time::Duration) {
        let token_delta =
            u64::from(usage.input_tokens).saturating_add(u64::from(usage.output_tokens));
        let time_delta_seconds = elapsed.as_secs();
        if token_delta == 0 && time_delta_seconds == 0 {
            return;
        }
        match self.config.goal_state.lock() {
            Ok(mut state) => state.record_usage(token_delta, time_delta_seconds),
            Err(err) => tracing::warn!("goal state lock poisoned while recording usage: {err}"),
        }
    }

    pub(crate) async fn add_session_message(&mut self, message: Message) {
        self.session.add_message(message);
        self.emit_session_updated().await;
    }

    pub(crate) fn turn_metadata_block(
        &self,
        routed_model: &str,
        auto_model: bool,
        reasoning_effort: Option<&str>,
        reasoning_effort_auto: bool,
        provenance: UserInputProvenance,
    ) -> ContentBlock {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let working_set_summary = self
            .session
            .working_set
            .summary_block(&self.config.workspace)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut lines = vec![
            format!("Current local date: {today}"),
            // Workspace path moved here from the static `## Environment` block so
            // the static system prefix stays byte-stable across sessions (see
            // `render_environment_block` for the prefix-cache rationale).
            format!("Current workspace: {}", self.config.workspace.display()),
            format!("Current model: {routed_model}"),
            format!("Input provenance: {}", provenance.as_str()),
            format!(
                "Input authority: {}",
                if provenance.can_authorize_work() {
                    "external_current_turn"
                } else {
                    "non_authoritative"
                }
            ),
        ];
        if auto_model {
            lines.push(format!("Auto model route: {routed_model}"));
        }
        if reasoning_effort_auto && let Some(reasoning_effort) = reasoning_effort {
            lines.push(format!("Auto reasoning effort: {reasoning_effort}"));
        }
        if let Some(working_set_summary) = working_set_summary {
            lines.push(working_set_summary);
        }
        let summary = lines.join("\n");

        ContentBlock::Text {
            text: format!("<turn_meta>\n{summary}\n</turn_meta>"),
            cache_control: None,
        }
    }

    pub(crate) fn user_text_message_with_turn_metadata(&self, text: String) -> Message {
        self.user_text_message_with_turn_metadata_for_route(
            text,
            &self.session.model,
            self.session.auto_model,
            self.session.reasoning_effort.as_deref(),
            self.session.reasoning_effort_auto,
        )
    }

    pub(crate) fn user_text_message_with_turn_metadata_for_route(
        &self,
        text: String,
        routed_model: &str,
        auto_model: bool,
        reasoning_effort: Option<&str>,
        reasoning_effort_auto: bool,
    ) -> Message {
        self.user_text_message_with_turn_metadata_for_route_and_provenance(
            text,
            routed_model,
            auto_model,
            reasoning_effort,
            reasoning_effort_auto,
            UserInputProvenance::ExternalUser,
        )
    }

    pub(crate) fn runtime_text_message_with_turn_metadata(
        &self,
        text: String,
        provenance: UserInputProvenance,
    ) -> Message {
        self.user_text_message_with_turn_metadata_for_route_and_provenance(
            text,
            &self.session.model,
            self.session.auto_model,
            self.session.reasoning_effort.as_deref(),
            self.session.reasoning_effort_auto,
            provenance,
        )
    }

    pub(crate) fn user_text_message_with_turn_metadata_for_route_and_provenance(
        &self,
        text: String,
        routed_model: &str,
        auto_model: bool,
        reasoning_effort: Option<&str>,
        reasoning_effort_auto: bool,
        provenance: UserInputProvenance,
    ) -> Message {
        // Place the user text first and turn_meta last so that the leading
        // bytes of each user message stay stable across date / model-route /
        // working-set changes. DeepSeek's KV prefix cache matches byte
        // sequences from the start of each message; when turn_meta (which
        // contains the current date) sits at position 0 the entire user
        // message prefix is invalidated at every date boundary. Moving it
        // to the tail preserves the user-input prefix and limits cache
        // invalidation to the trailing metadata block.
        Message {
            role: "user".to_string(),
            content: vec![
                ContentBlock::Text {
                    text,
                    cache_control: None,
                },
                self.turn_metadata_block(
                    routed_model,
                    auto_model,
                    reasoning_effort,
                    reasoning_effort_auto,
                    provenance,
                ),
            ],
        }
    }

    pub(crate) async fn handle_idle_subagent_completion(&mut self, first: SubAgentCompletion) {
        let mut completions = vec![first];
        while let Ok(completion) = self.rx_subagent_completion.try_recv() {
            completions.push(completion);
        }

        let count = completions.len();
        let content = completions
            .iter()
            .map(|completion| turn_loop::subagent_completion_runtime_text(&completion.payload))
            .collect::<Vec<_>>()
            .join("\n\n");

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "Resuming turn with {count} idle sub-agent completion(s)"
            )))
            .await;

        self.handle_send_message(
            content,
            self.current_mode,
            Some(self.api_provider),
            self.session.model.clone(),
            self.config.goal_objective.clone(),
            self.config.goal_token_budget,
            self.config.goal_status,
            self.session.reasoning_effort.clone(),
            self.session.reasoning_effort_auto,
            self.session.response_format.clone(),
            self.session.auto_model,
            self.session.allow_shell,
            self.session.trust_mode,
            self.session.auto_approve,
            self.session.approval_mode,
            self.config.translation_enabled,
            self.config.show_thinking,
            self.config.allowed_tools.clone(),
            Vec::new(),
            self.config.hook_executor.clone(),
            self.config.verbosity.clone(),
            UserInputProvenance::SubAgentHandoff,
        )
        .await;
    }

    /// Handle a send message operation
    #[allow(clippy::too_many_arguments)]
    /// After a turn completes, check whether an active goal should keep going.
    /// Returns a continuation message to re-dispatch as a new turn, or `None`
    /// if the goal is complete, blocked, paused, or over an optional budget.
    ///
    /// There is no continuation cap — a goal runs until the model self-reports
    /// done/blocked, the user pauses or clears, or an optional token/time
    /// budget is exhausted. The loop is "until done," not "until N turns."
    pub(crate) fn goal_continuation_if_active(&self) -> Option<String> {
        let snapshot = self.config.goal_state.lock().ok()?.snapshot();
        if !snapshot.is_active() {
            return None;
        }

        // The snapshot status is a string ("active", "paused", "complete",
        // "blocked"). Map it to the goal-loop decision core's status enum.
        let status = match snapshot.status.as_str() {
            "active" => crate::goal_loop::GoalRunStatus::Active,
            "complete" => crate::goal_loop::GoalRunStatus::Completed,
            // Paused / Blocked / unknown → no continuation.
            _ => return None,
        };

        let decision = crate::goal_loop::decide_continuation(
            status,
            crate::goal_loop::GoalProgress {
                tokens_used: snapshot.tokens_used,
                time_used_seconds: snapshot.time_used_seconds,
                continuations: snapshot.continuation_count,
            },
            crate::goal_loop::GoalBudget {
                token_budget: snapshot.token_budget.map(u64::from),
                time_budget_seconds: None,
                max_continuations: Some(crate::goal_loop::DEFAULT_MAX_CONTINUATIONS),
            },
        );

        match decision {
            crate::goal_loop::ContinuationDecision::Continue => {
                Some(crate::tools::goal::render_continuation_prompt(
                    &snapshot,
                    snapshot.continuation_count,
                ))
            }
            // All stop reasons → no continuation. The caller (the async turn
            // completion path) emits a status message for budget-exhaustion.
            crate::goal_loop::ContinuationDecision::Stop(reason) => {
                tracing::info!(?reason, "goal continuation stopped");
                None
            }
        }
    }

    /// Handle `/goal pause|resume|clear|complete|blocked` by writing the new
    /// status to `SharedGoalState` so the cross-turn continuation loop respects
    /// it. This does NOT dispatch a model turn — it's a control-plane update.
    pub(crate) async fn handle_set_goal_status(&mut self, status: GoalStatus, clear: bool) {
        match self.config.goal_state.lock() {
            Ok(mut state) => {
                if clear {
                    // `/goal clear` — wipe the objective entirely.
                    state.sync_from_host_status(None, None, GoalStatus::Active);
                } else {
                    // Update only the status; keep the objective and budget.
                    // `sync_from_host_status` resets usage when the objective
                    // changes, but here we pass the existing objective so usage
                    // is preserved (pause/resume shouldn't reset the counter).
                    let objective = state.objective().map(str::to_string);
                    let budget = state.token_budget();
                    state.sync_from_host_status(objective.as_deref(), budget, status);
                }
            }
            Err(err) => {
                tracing::warn!("goal state lock poisoned during SetGoalStatus: {err}");
            }
        }
        let label = if clear {
            "cleared"
        } else {
            match status {
                GoalStatus::Active => "resumed",
                GoalStatus::Paused => "paused",
                GoalStatus::Complete => "complete",
                GoalStatus::Blocked => "blocked",
            }
        };
        let _ = self
            .tx_event
            .send(Event::status(format!("Goal {label}.")))
            .await;
        self.emit_goal_updated().await;
    }
}
