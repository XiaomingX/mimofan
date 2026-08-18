//! Main streaming turn loop for the engine.
//!
//! Extracted from `core/engine.rs` for issue #74. This module keeps the
//! existing per-turn orchestration intact: request construction, streaming
//! event handling, tool planning/execution, LSP post-edit hooks, capacity
//! checkpoints, and loop termination.

use super::*;
use crate::core::ops::UserInputProvenance;
use crate::prompt_zones::PinnedPrefix;

const MAX_APPROVAL_INTENT_SUMMARY_CHARS: usize = 2_000;

fn approval_intent_summary(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut chars = trimmed.chars();
    let mut summary = chars
        .by_ref()
        .take(MAX_APPROVAL_INTENT_SUMMARY_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        summary.push_str("...");
    }
    Some(summary)
}

pub(super) fn registered_tool_approval_required(
    tool_name: &str,
    requirement: ApprovalRequirement,
    auto_approve: bool,
) -> bool {
    if requirement == ApprovalRequirement::Auto {
        return false;
    }
    if registered_tool_requires_non_bypassable_approval(tool_name) {
        return true;
    }
    !auto_approve
}

fn registered_tool_requires_non_bypassable_approval(tool_name: &str) -> bool {
    matches!(tool_name, "rlm_eval")
}

/// Build a structured self-heal hint appended to a failed tool result so the
/// model retries (or pivots) *within the same step* instead of waiting for the
/// next turn. Mirrors Claude Code's "on error, analyze and retry" behavior.
///
/// `category` is the `ErrorEnvelope::category` label (or `"tool"` for an
/// `Ok` result that reported `success: false`). It is surfaced so the model
/// can decide between retrying the same call and switching approaches.
fn self_heal_hint(tool_name: &str, category: &str) -> String {
    format!(
        "[Self-heal] Tool '{tool_name}' failed (category: {category}). Before issuing the next tool call, do NOT blindly repeat the identical call. Instead:\n\
         - Read the error above carefully; identify the root cause (e.g. missing path, wrong argument, non-zero command exit).\n\
         - If it names a missing file/path, verify it exists or correct the path; if it is a command failure, inspect the output and fix the underlying cause.\n\
         - Prefer a smaller, verifiable step, or choose a different tool/approach if the current one cannot succeed."
    )
}

impl Engine {
    fn drain_shell_completion_events(&self) -> Vec<crate::tools::shell::ShellCompletionEvent> {
        self.shell_manager
            .lock()
            .map(|mut manager| manager.drain_finished_jobs())
            .unwrap_or_default()
    }

    async fn drain_subagent_completion_events(&mut self, status_label: &str) -> usize {
        let mut completions: Vec<crate::tools::subagent::SubAgentCompletion> = Vec::new();
        while let Ok(completion) = self.rx_subagent_completion.try_recv() {
            if self
                .delivered_subagent_completion_ids
                .insert(completion.agent_id.clone())
            {
                completions.push(completion);
            }
        }

        let synthesized = {
            let manager = self.subagent_manager.read().await;
            manager.terminal_results_excluding(&self.delivered_subagent_completion_ids)
        };
        for result in synthesized {
            if self
                .delivered_subagent_completion_ids
                .insert(result.agent_id.clone())
            {
                completions.push(crate::tools::subagent::subagent_completion_from_result(
                    &result,
                ));
            }
        }

        let count = completions.len();
        if count == 0 {
            return 0;
        }

        for completion in completions {
            self.add_session_message(subagent_completion_runtime_message(&completion.payload))
                .await;
        }
        let prefix = if status_label.is_empty() {
            String::new()
        } else {
            format!("{status_label} ")
        };
        let _ = self
            .tx_event
            .send(Event::status(format!(
                "Resuming turn with {count} {prefix}sub-agent completion(s)"
            )))
            .await;
        count
    }

    pub(super) async fn handle_deepseek_turn(
        &mut self,
        turn: &mut TurnContext,
        tool_registry: Option<&crate::tools::ToolRegistry>,
        tools: Option<Vec<Tool>>,
        mode: AppMode,
        force_update_plan_first: bool,
        dynamic_active_tools: Vec<&'static str>,
    ) -> (TurnOutcomeStatus, Option<String>) {
        // Signal to the terminal / taskbar that a turn is in progress
        // (OSC 9 ; 4 indeterminate progress + title spinner).
        crate::tui::notifications::set_taskbar_progress_busy();
        crate::tui::notifications::start_title_animation("mimofan");

        // #637: every turn gets a stable trace_id threaded through its spans so
        // that engine → tool execution → model client logs correlate by id.
        let trace_id = crate::core::engine::trace::TraceId::new();
        let _turn_span = crate::core::engine::trace::trace_span_for(trace_id);
        let _turn_enter = _turn_span.enter();
        tracing::debug!(trace_id = %trace_id.as_hex(), "turn started");

        let client = self
            .deepseek_client
            .clone()
            .expect("DeepSeek client should be configured");

        let mut consecutive_tool_error_steps = 0u32;
        let mut turn_error: Option<String> = None;
        // Tracks whether this turn produced a successful file-write tool call,
        // used to feed the goal anti-drift (NoProgress) circuit breaker.
        let mut turn_had_write = false;
        // Last tool-error text in this step, used as a stable fingerprint for
        // the repeated-error circuit breaker.
        let mut last_tool_error_text = String::new();
        // De-duplicates auto-captured memory signals within a single turn.
        let mut seen_auto_memory: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // In-turn repetition / oscillation / stall brake. The guard is held in
        // a `SharedLoopGuard` (Arc<Mutex<LoopGuard>>) so the *same* guard
        // instance continues across turns within a session — accumulated loop
        // suspicion is not wiped clean by a new user message. Its durable
        // portion is also persisted to disk (loop_guard_state) so it survives
        // process restarts; a new user message is still a fresh intent for
        // per-call evidence, but the suspicion counters and nudge budgets
        // carry over. Detections inject an advisory nudge (see below) rather
        // than terminating, so the user never loses in-flight progress to a
        // false positive.
        let loop_guard = self.load_shared_loop_guard(&self.session.id);
        // Nudges detected during result folding, injected after the batch so
        // they land after the tool results the model is about to read.
        let mut pending_loop_nudges: Vec<String> = Vec::new();
        let mut context_recovery_attempts = 0u8;
        let mut tool_catalog = tools.unwrap_or_default();
        if !tool_catalog.is_empty() {
            ensure_advanced_tooling(&mut tool_catalog, mode, &self.config.tools_always_load);
        }
        if let Some(registry) = tool_registry {
            let issues = tool_catalog_consistency_issues(&tool_catalog, registry);
            if !issues.is_empty() {
                tracing::warn!(
                    target: "engine.tool_catalog",
                    ?issues,
                    "model/search tool catalog is inconsistent with the runtime registry"
                );
            }
        }
        let mut active_tool_names = initial_active_tools(&tool_catalog);
        active_tool_names.extend(
            dynamic_active_tools
                .into_iter()
                .map(std::string::ToString::to_string),
        );
        let mut goal_continuations_this_turn = 0u32;

        // Outer stream-retry counter: when the chunked-transfer connection
        // dies mid-stream and either nothing useful was streamed (#103
        // Phase 3) or the host slept mid-turn (#2990), we silently re-issue
        // the SAME request up to MAX_STREAM_RETRIES times before surfacing
        // the failure to the user.
        let mut stream_retry_attempts: u32 = 0;

        loop {
            if self.cancel_token.is_cancelled() {
                let _ = self.tx_event.send(Event::status("Request cancelled")).await;
                self.persist_loop_guard_state(&self.session.id, &loop_guard)
                    .await;
                return (TurnOutcomeStatus::Interrupted, None);
            }

            // W3 (#836): notify interceptors a step is about to begin before
            // draining the steering channel.
            for ic in &self.interceptors {
                ic.pre_step(&self.session.workspace.to_string_lossy());
            }

            while let Ok(steer) = self.rx_steer.try_recv() {
                let steer = steer.trim().to_string();
                if steer.is_empty() {
                    continue;
                }
                self.session
                    .working_set
                    .observe_user_message(&steer, &self.session.workspace);
                self.add_session_message(self.user_text_message_with_turn_metadata(steer.clone()))
                    .await;
                let _ = self
                    .tx_event
                    .send(Event::status(format!(
                        "Steer input accepted: {}",
                        summarize_text(&steer, 120)
                    )))
                    .await;
            }

            // Child agents can finish while the parent model is still taking
            // tool steps. Surface queued completions before the next provider
            // request so the parent can use them immediately instead of
            // discovering them only when it eventually emits no more tools or
            // the idle handler starts a separate follow-up turn.
            self.drain_subagent_completion_events("queued").await;

            // Ensure system prompt is up to date with latest session states
            self.refresh_system_prompt();

            if turn.at_max_steps() {
                let _ = self
                    .tx_event
                    .send(Event::status("Reached maximum steps"))
                    .await;
                break;
            }

            let compaction_pins = self
                .session
                .working_set
                .pinned_message_indices(&self.session.messages, &self.session.workspace);
            let compaction_paths = self.session.working_set.top_paths(24);

            // Compaction gate. `ContextBudget` owns the *budget* question
            // ("is the window full enough to warrant compacting?"), using the
            // same configured threshold the UI renders its percentage against.
            // `compaction::should_compact` still owns the *structural*
            // question ("is there actually a summarizable span, once pins and
            // the keep-recent window are accounted for?"). Both must agree, so
            // a full window with nothing safe to summarize does not spin.
            let compaction_enabled = self.config.compaction.enabled;
            let budget_says_compact = compaction_enabled
                && self
                    .route_compaction_budget()
                    .and_then(|budget| {
                        crate::context_budget::compaction_decision(&budget, compaction_enabled)
                    })
                    .is_some();

            if budget_says_compact
                && should_compact(
                    &self.session.messages,
                    &self.config.compaction,
                    Some(&self.session.workspace),
                    Some(&compaction_pins),
                    Some(&compaction_paths),
                )
            {
                // #855 — mark compaction in progress so the periodic
                // consolidation scheduler skips its own pass and avoids
                // contending for the storage layer.
                self.compaction_in_progress = true;
                let compaction_id = format!("compact_{}", &uuid::Uuid::new_v4().to_string()[..8]);
                self.emit_compaction_started(
                    compaction_id.clone(),
                    true,
                    "Auto context compaction started".to_string(),
                )
                .await;
                let _ = self
                    .tx_event
                    .send(Event::status("Auto-compacting context...".to_string()))
                    .await;
                let auto_messages_before = self.session.messages.len();

                // Fire the PreCompact lifecycle hook so external hooks can
                // inspect (and optionally deny via exit code 2) the imminent
                // summarization. Runs off the worker thread like other hooks.
                if let Some(hook_executor) = self.config.hook_executor.as_ref()
                    && hook_executor.has_hooks_for_event(crate::hooks::HookEvent::PreCompact)
                {
                    let hook_context = crate::hooks::HookContext::new()
                        .with_message("Auto context compaction")
                        .with_workspace(self.session.workspace.clone())
                        .with_model(&self.config.model)
                        .with_session_id(&self.session.id)
                        .with_tokens(auto_messages_before as u32);
                    let executor = hook_executor.clone();
                    let results = tokio::task::spawn_blocking(move || {
                        executor.execute(crate::hooks::HookEvent::PreCompact, &hook_context)
                    })
                    .await
                    .unwrap_or_else(|join_err| {
                        tracing::error!("PreCompact hook task panicked: {join_err}");
                        Vec::new()
                    });
                    // A PreCompact hook may deny compaction to preserve
                    // critical context (exit code 2, like ToolCallBefore).
                    if results.iter().any(|r| r.exit_code == Some(2)) {
                        let _ = self
                            .tx_event
                            .send(Event::status(
                                "Auto-compaction skipped: denied by PreCompact hook".to_string(),
                            ))
                            .await;
                        self.emit_compaction_failed(
                            compaction_id.clone(),
                            true,
                            "Auto-compaction denied by PreCompact hook".to_string(),
                        )
                        .await;
                        continue;
                    }
                }

                // Re-inject the active objective into the compaction so long-horizon
                // tasks don't drift after multiple context compactions (#841).
                // The `goal_queue` objective is a plain string; bridge it into
                // the structured `Objective` type used by the drift check (W1).
                let active_objective = self.config.goal_queue.lock().ok().and_then(|g| {
                    g.active_id()
                        .and_then(|id| g.get(id))
                        .and_then(|e| e.goal.objective().map(str::to_owned))
                });
                let objective_ref = active_objective
                    .as_ref()
                    .map(|s| crate::compaction::objective::Objective::from(s.as_str()));

                match compact_messages_safe_with_objective(
                    &client,
                    &self.session.messages,
                    &self.config.compaction,
                    Some(&self.session.workspace),
                    Some(&compaction_pins),
                    Some(&compaction_paths),
                    objective_ref.as_ref(),
                    self.config.goal_self_check_after_compact,
                )
                .await
                {
                    Ok(result) => {
                        // Only update if we got valid messages (never corrupt state)
                        if !result.messages.is_empty() || self.session.messages.is_empty() {
                            let auto_messages_after = result.messages.len();
                            self.session.messages = result.messages.into();
                            // #871 — inject the captured decision trail into the
                            // compaction system prompt so long-horizon tasks keep
                            // a clean decision history across compactions.
                            self.merge_compaction_summary(
                                self.with_decision_trail(result.summary_prompt),
                            );

                            // Post-compaction goal self-check nudge (#selfcheck).
                            // Injected via the *system* prompt channel so it never
                            // enters the user conversation history — it only asks
                            // the model to confirm the original objective is still
                            // consistent after the context was compressed. Opt-out
                            // via `config.goal_self_check_after_compact`.
                            if let Some(loop_break) = result.self_check_nudge {
                                let nudge_block = crate::models::SystemBlock {
                                    block_type: "text".to_string(),
                                    text: loop_break.nudge,
                                    cache_control: None,
                                };
                                let merged = crate::compaction::merge_system_prompts(
                                    self.session.system_prompt.as_ref(),
                                    Some(crate::models::SystemPrompt::Blocks(vec![nudge_block])),
                                );
                                self.session.system_prompt = merged;
                                self.session.last_system_prompt_hash = Some(
                                    super::system_prompt_hash(self.session.system_prompt.as_ref()),
                                );
                            }

                            // Fire PostCompact after a successful summarization
                            // so hooks can re-inject disk-backed context or emit
                            // telemetry on the compaction outcome.
                            if let Some(hook_executor) = self.config.hook_executor.as_ref()
                                && hook_executor
                                    .has_hooks_for_event(crate::hooks::HookEvent::PostCompact)
                            {
                                let removed =
                                    auto_messages_before.saturating_sub(auto_messages_after);
                                let hook_context = crate::hooks::HookContext::new()
                                    .with_message(&format!(
                                        "Auto compaction: {auto_messages_before} → {auto_messages_after} ({removed} removed)"
                                    ))
                                    .with_workspace(self.session.workspace.clone())
                                    .with_model(&self.config.model)
                                    .with_session_id(&self.session.id)
                                    .with_tokens(auto_messages_after as u32);
                                let executor = hook_executor.clone();
                                let _ = tokio::task::spawn_blocking(move || {
                                    executor.execute(
                                        crate::hooks::HookEvent::PostCompact,
                                        &hook_context,
                                    )
                                })
                                .await;
                            }

                            self.emit_session_updated().await;
                            let removed = auto_messages_before.saturating_sub(auto_messages_after);
                            let status = if result.retries_used > 0 {
                                format!(
                                    "Auto-compaction complete: {auto_messages_before} → {auto_messages_after} messages ({removed} removed, {} retries)",
                                    result.retries_used
                                )
                            } else {
                                format!(
                                    "Auto-compaction complete: {auto_messages_before} → {auto_messages_after} messages ({removed} removed)"
                                )
                            };
                            self.emit_compaction_completed(
                                compaction_id.clone(),
                                true,
                                status.clone(),
                                Some(auto_messages_before),
                                Some(auto_messages_after),
                            )
                            .await;
                            let _ = self.tx_event.send(Event::status(status)).await;
                            // Warm up prefix cache after auto-compaction.
                            self.spawn_cache_warmup_after_compaction(
                                &client,
                                &self.session.messages,
                                self.session.system_prompt.as_ref(),
                                None,
                                &self.session.model,
                                None,
                            );
                        } else {
                            let message = "Auto-compaction skipped: empty result".to_string();
                            self.emit_compaction_failed(
                                compaction_id.clone(),
                                true,
                                message.clone(),
                            )
                            .await;
                            let _ = self.tx_event.send(Event::status(message)).await;
                        }
                    }
                    Err(err) => {
                        // Log error but continue with original messages (never corrupt)
                        let message = format!("Auto-compaction failed: {err}");
                        self.emit_compaction_failed(compaction_id, true, message.clone())
                            .await;
                        let _ = self.tx_event.send(Event::status(message)).await;
                    }
                }
                // #855 — compaction finished; clear the in-progress latch so the
                // periodic consolidation scheduler can run again on the next
                // turn boundary.
                self.compaction_in_progress = false;
            }

            if let Some(input_budget) = context_input_budget_for_route(
                self.api_provider,
                &self.session.model,
                self.active_route_limits,
                0,
            ) {
                let estimated_input = self.estimated_input_tokens();
                if estimated_input > input_budget {
                    if context_recovery_attempts >= MAX_CONTEXT_RECOVERY_ATTEMPTS {
                        let message = format!(
                            "Context remains above model limit after {MAX_CONTEXT_RECOVERY_ATTEMPTS} recovery attempts \
                             (~{estimated_input} token estimate, ~{input_budget} budget). Please run /compact or /clear."
                        );
                        turn_error = Some(message.clone());
                        let _ = self
                            .tx_event
                            .send(Event::error(ErrorEnvelope::context_overflow(message)))
                            .await;
                        self.persist_loop_guard_state(&self.session.id, &loop_guard)
                            .await;
                        return (TurnOutcomeStatus::Failed, turn_error);
                    }

                    if self
                        .recover_context_overflow(&client, "preflight token budget")
                        .await
                    {
                        crate::core::engine::recovery_stats::record_attempt();
                        crate::core::engine::recovery_stats::record_success();
                        context_recovery_attempts = context_recovery_attempts.saturating_add(1);
                        continue;
                    } else {
                        // A failure we tried to recover from but could not.
                        crate::core::engine::recovery_stats::record_attempt();
                    }
                }
            }

            // #136: drain any LSP diagnostics collected since the last
            // request and inject them as a synthetic user message so the
            // model sees compile errors before its next reasoning step.
            self.flush_pending_lsp_diagnostics().await;

            // #159: layered context seam checkpoint. This is opt-in for
            // v0.7.5 while #200 audits cache-hit behavior; when enabled it
            // appends <archived_context> blocks rather than replacing history.
            self.layered_context_checkpoint().await;

            // Build the request
            let force_update_plan_this_step = force_update_plan_first && !turn.has_tool_calls();
            let mut active_tools = if tool_catalog.is_empty() {
                None
            } else {
                Some(active_tools_for_step(
                    &tool_catalog,
                    &active_tool_names,
                    force_update_plan_this_step,
                ))
            };
            if self.config.strict_tool_mode
                && let Some(tools) = active_tools.as_mut()
            {
                crate::tools::schema_sanitize::prepare_tools_for_strict_mode(tools);
            }

            // Resolve `auto` reasoning_effort to a concrete tier (#663).
            let effective_reasoning_effort = resolve_auto_effort(
                self.session.reasoning_effort.as_deref(),
                &self.session.messages,
                self.api_provider,
            );

            // Check prefix-cache stability before building the request.
            // This detects system-prompt or tool-set drift that would
            // invalidate DeepSeek's KV prefix cache for this turn.
            // Sends an event on EVERY check so the TUI can maintain
            // its own counter for the stable-checks tally.
            if let Some(pm) = self.session.prefix_stability.as_mut() {
                let system_text =
                    crate::prefix_cache::system_prompt_text(self.session.system_prompt.as_ref());
                let tools_ref: Option<&[crate::models::Tool]> = active_tools.as_deref();
                match pm.check_and_update(&system_text, tools_ref) {
                    Err(change) => {
                        let pinned_hash = pm
                            .pinned_fingerprint()
                            .map(|fp| fp.combined_sha256.clone())
                            .unwrap_or_default();
                        tracing::debug!(
                            target: "prefix_cache",
                            "{}",
                            change.description()
                        );
                        let _ = self
                            .tx_event
                            .send(Event::PrefixCacheChange {
                                description: change.description(),
                                system_prompt_changed: change.system_changed,
                                tools_changed: change.tools_changed,
                                stability_pct: (pm.stability_ratio() * 100.0).round() as u32,
                                changed: true,
                                pinned_combined_hash: pinned_hash,
                            })
                            .await;
                    }
                    Ok(_) => {
                        let pinned_hash = pm
                            .pinned_fingerprint()
                            .map(|fp| fp.combined_sha256.clone())
                            .unwrap_or_default();
                        // Stable check — keep the TUI counter in sync.
                        let _ = self
                            .tx_event
                            .send(Event::PrefixCacheChange {
                                description: String::new(),
                                system_prompt_changed: false,
                                tools_changed: false,
                                stability_pct: (pm.stability_ratio() * 100.0).round() as u32,
                                changed: false,
                                pinned_combined_hash: pinned_hash,
                            })
                            .await;
                    }
                }
            }

            // Three-zone prefix contract (#2264): freeze baseline on first
            // turn, verify against it on subsequent turns. Operates alongside
            // PrefixStabilityManager as an independent diagnostic layer.
            // Phase 3: emit a one-shot 'frozen' event on first turn.
            // Drift is logged (tracing::debug!) but not re-emitted —
            // PrefixStabilityManager already reports the change above.
            let system_text =
                crate::prefix_cache::system_prompt_text(self.session.system_prompt.as_ref());
            let current_tools: &[crate::models::Tool] = active_tools.as_deref().unwrap_or_default();

            match &self.session.frozen_prefix {
                Some(frozen) => {
                    if let Err(drift) = frozen.verify(&system_text, current_tools) {
                        tracing::debug!(
                            target: "prefix_cache",
                            "three-zone drift: {drift}"
                        );
                        let pinned = PinnedPrefix::new(
                            self.session.system_prompt.as_ref(),
                            current_tools.to_vec(),
                        );
                        self.session.frozen_prefix = Some(pinned.freeze());
                    }
                }
                None => {
                    let pinned = PinnedPrefix::new(
                        self.session.system_prompt.as_ref(),
                        current_tools.to_vec(),
                    );
                    let frozen = pinned.freeze();
                    let _ = self
                        .tx_event
                        .send(Event::PrefixCacheChange {
                            description: format!("frozen: {}", frozen.short_id()),
                            system_prompt_changed: false,
                            tools_changed: false,
                            stability_pct: 100,
                            changed: false,
                            pinned_combined_hash: frozen.hash().to_string(),
                        })
                        .await;
                    self.session.frozen_prefix = Some(frozen);
                }
            }

            let mut request = MessageRequest {
                model: self.session.model.clone(),
                messages: self.messages_with_turn_metadata(),
                max_tokens: effective_max_output_tokens_for_route(
                    &self.session.model,
                    self.active_route_limits,
                ),
                system: self.session.system_prompt.clone(),
                tools: active_tools.clone(),
                tool_choice: if active_tools.is_some() {
                    if self.config.strict_tool_mode {
                        Some(json!("required"))
                    } else {
                        Some(json!({ "type": "auto" }))
                    }
                } else {
                    None
                },
                metadata: None,
                thinking: None,
                reasoning_effort: effective_reasoning_effort,
                stream: Some(true),
                temperature: None,
                top_p: None,
                response_format: self.session.response_format.clone(),
            };

            // W3 (#836): give interceptors a chance to mutate the provider
            // request before it is sent.
            for ic in &self.interceptors {
                ic.request(&mut request);
            }

            // Stream the response. Keep the request around (cloned into the
            // first call) so we can resend it on a transparent retry below
            // when the wire dies before any content was streamed (#103).
            let stream_request = request;
            let stream_result = tokio::select! {
                biased;
                () = self.cancel_token.cancelled() => {
                    let _ = self.tx_event.send(Event::status("Request cancelled")).await;
                    self.persist_loop_guard_state(&self.session.id, &loop_guard).await;
                    return (TurnOutcomeStatus::Interrupted, None);
                }
                result = client.create_message_stream(stream_request.clone()) => result,
            };
            let stream = match stream_result {
                Ok(s) => {
                    context_recovery_attempts = 0;
                    s
                }
                Err(e) => {
                    let message = self.decorate_auth_error_message(e.to_string());
                    if is_context_length_error_message(&message)
                        && context_recovery_attempts < MAX_CONTEXT_RECOVERY_ATTEMPTS
                        && self
                            .recover_context_overflow(&client, "provider context-length rejection")
                            .await
                    {
                        crate::core::engine::recovery_stats::record_attempt();
                        crate::core::engine::recovery_stats::record_success();
                        context_recovery_attempts = context_recovery_attempts.saturating_add(1);
                        continue;
                    } else if is_context_length_error_message(&message)
                        && context_recovery_attempts >= MAX_CONTEXT_RECOVERY_ATTEMPTS
                    {
                        // Exhausted recovery budget on a context-length error.
                        crate::core::engine::recovery_stats::record_attempt();
                    }
                    turn_error = Some(message.clone());
                    let _ = self
                        .tx_event
                        .send(Event::error(ErrorEnvelope::classify(message, true)))
                        .await;
                    self.persist_loop_guard_state(&self.session.id, &loop_guard)
                        .await;
                    return (TurnOutcomeStatus::Failed, turn_error);
                }
            };
            // The stream value is itself `Pin<Box<dyn Stream + Send>>`, which
            // is `Unpin`, so we can rebind it on a transparent retry without
            // breaking the existing pin invariants.
            let mut stream = stream;

            // Track content blocks
            let mut content_blocks: Vec<ContentBlock> = Vec::new();
            let mut current_text_raw = String::new();
            let mut current_text_visible = String::new();
            let mut current_thinking = String::new();
            // #3014: Anthropic signed-thinking signature for the current
            // thinking block; must be replayed verbatim in tool loops.
            let mut current_thinking_signature: Option<String> = None;
            let mut tool_uses: Vec<ToolUseState> = Vec::new();
            let mut usage = Usage {
                input_tokens: 0,
                output_tokens: 0,
                ..Usage::default()
            };
            let mut current_block_kind: Option<ContentBlockKind> = None;
            // Map block_index → tool_uses position. Required because the
            // OpenAI-compatible streaming parser emits multiple
            // ContentBlockStart::ToolUse events back-to-back (one per
            // tool_call in a batch) before any ContentBlockStop arrives —
            // all Stops are flushed together at `finish_reason`. A single
            // Option<usize> gets overwritten by each new Start; the first
            // Stop then takes the last index, and every subsequent Stop
            // takes `None`, dropping ToolCallStarted events for every
            // tool call except the last one in the batch.
            let mut current_tool_indices: std::collections::HashMap<u32, usize> =
                std::collections::HashMap::new();
            let mut in_tool_call_block = false;
            let mut fake_wrapper_notice_emitted = false;
            let mut pending_message_complete = false;
            let mut last_text_index: Option<usize> = None;
            let mut stream_errors = 0u32;
            // #103 transparent retry bookkeeping. `any_content_received` flips
            // on the first non-MessageStart event so we know whether DeepSeek
            // billed us / the user has seen any output for this turn yet.
            // This is distinct from the outer `stream_retry_attempts` (which
            // restarts the whole turn-step when a stream died with no
            // content-block delta delivered to the consumer).
            let mut any_content_received = false;
            let mut transparent_stream_retries = 0u32;
            let mut pending_steers: Vec<String> = Vec::new();
            // `stream_start` is reset on a transparent retry so the wall-clock
            // budget restarts with the fresh stream.
            let mut stream_start = Instant::now();
            // #2990 sleep-resume bookkeeping: monotonic and wall-clock stamps
            // of the last stream progress. `Instant` pauses across a host
            // suspend while `SystemTime` does not, so a large divergence on
            // the next error tells "machine slept" apart from "network died".
            let mut last_progress_mono = Instant::now();
            let mut last_progress_wall = std::time::SystemTime::now();
            let mut sleep_resume_pending = false;
            let mut stream_content_bytes: usize = 0;
            let (chunk_timeout_secs, chunk_timeout) = stream_chunk_timeout_budget(&self.config);
            let max_duration = Duration::from_secs(STREAM_MAX_DURATION_SECS);

            // Process stream events
            loop {
                let poll_outcome = tokio::select! {
                    biased;
                    _ = self.cancel_token.cancelled() => None,
                    result = tokio::time::timeout(chunk_timeout, stream.next()) => {
                        match result {
                            Ok(Some(event_result)) => Some(event_result),
                            Ok(None) => None, // stream ended normally
                            Err(_) => {
                                let envelope = StreamError::Stall {
                                    timeout_secs: chunk_timeout_secs,
                                }
                                .into_envelope();
                                crate::logging::warn(&envelope.message);
                                let _ = self.tx_event.send(Event::error(envelope)).await;
                                None
                            }
                        }
                    }
                };
                let Some(event_result) = poll_outcome else {
                    break;
                };
                // W3 (#836): notify interceptors before draining steer input
                // queued mid-stream.
                for ic in &self.interceptors {
                    ic.pre_step(&self.session.workspace.to_string_lossy());
                }
                while let Ok(steer) = self.rx_steer.try_recv() {
                    let steer = steer.trim().to_string();
                    if steer.is_empty() {
                        continue;
                    }
                    pending_steers.push(steer.clone());
                    let _ = self
                        .tx_event
                        .send(Event::status(format!(
                            "Steer input queued: {}",
                            summarize_text(&steer, 120)
                        )))
                        .await;
                }

                if self.cancel_token.is_cancelled() {
                    break;
                }

                // Guard: max wall-clock duration
                if stream_start.elapsed() > max_duration {
                    let envelope = StreamError::DurationLimit {
                        limit_secs: STREAM_MAX_DURATION_SECS,
                    }
                    .into_envelope();
                    crate::logging::warn(&envelope.message);
                    turn_error.get_or_insert(envelope.message.clone());
                    let _ = self.tx_event.send(Event::error(envelope)).await;
                    break;
                }

                // Guard: max accumulated content bytes
                if stream_content_bytes > STREAM_MAX_CONTENT_BYTES {
                    let envelope = StreamError::Overflow {
                        limit_bytes: STREAM_MAX_CONTENT_BYTES,
                    }
                    .into_envelope();
                    crate::logging::warn(&envelope.message);
                    turn_error.get_or_insert(envelope.message.clone());
                    let _ = self.tx_event.send(Event::error(envelope)).await;
                    break;
                }

                let event = match event_result {
                    Ok(e) => {
                        last_progress_mono = Instant::now();
                        last_progress_wall = std::time::SystemTime::now();
                        // Flip on the first non-MessageStart event — that's
                        // the moment we cross from "stream not yet productive"
                        // (eligible for transparent retry) into "DeepSeek has
                        // billed us / user has seen output" (must surface).
                        if !any_content_received && !matches!(e, StreamEvent::MessageStart { .. }) {
                            any_content_received = true;
                        }
                        e
                    }
                    Err(e) => {
                        stream_errors = stream_errors.saturating_add(1);
                        let message = self.decorate_auth_error_message(e.to_string());
                        // #2990: wall-clock far ahead of the monotonic clock
                        // since the last chunk means the host slept mid-stream.
                        // The partial output predates the sleep and the user
                        // was not watching — schedule a full request retry in
                        // the post-loop block instead of failing the turn.
                        let wall_elapsed = last_progress_wall
                            .elapsed()
                            .unwrap_or_else(|_| last_progress_mono.elapsed());
                        if should_resume_after_sleep(
                            sleep_gap_detected(last_progress_mono.elapsed(), wall_elapsed),
                            stream_retry_attempts,
                            self.cancel_token.is_cancelled(),
                        ) {
                            crate::logging::warn(format!(
                                "Stream error after suspected system sleep ({:?} monotonic vs {:?} wall since last chunk); scheduling request retry: {message}",
                                last_progress_mono.elapsed(),
                                wall_elapsed,
                            ));
                            sleep_resume_pending = true;
                            break;
                        }
                        // #103: when the stream errors before any content was
                        // streamed AND we still have retry budget, transparently
                        // resend the request. DeepSeek has not billed for any
                        // output and the user has seen nothing — re-trying is
                        // the right user-visible behavior.
                        if should_transparently_retry_stream(
                            any_content_received,
                            transparent_stream_retries,
                            self.cancel_token.is_cancelled(),
                        ) {
                            crate::core::engine::recovery_stats::record_attempt();
                            transparent_stream_retries =
                                transparent_stream_retries.saturating_add(1);
                            crate::logging::info(format!(
                                "Transparent stream retry {transparent_stream_retries}/{MAX_TRANSPARENT_STREAM_RETRIES} (no content received yet): {message}",
                            ));
                            // Drop the failed stream before issuing the new
                            // request to release the underlying connection.
                            drop(stream);
                            let retry_stream_result = tokio::select! {
                                biased;
                                () = self.cancel_token.cancelled() => break,
                                result = client.create_message_stream(stream_request.clone()) => result,
                            };
                            match retry_stream_result {
                                Ok(fresh) => {
                                    stream = fresh;
                                    stream_start = Instant::now();
                                    // Roll back the error counter — this one
                                    // didn't surface to the user.
                                    stream_errors = stream_errors.saturating_sub(1);
                                    continue;
                                }
                                Err(retry_err) => {
                                    let retry_msg = self.decorate_auth_error_message(format!(
                                        "Stream retry failed: {retry_err}"
                                    ));
                                    turn_error.get_or_insert(retry_msg.clone());
                                    let _ = self
                                        .tx_event
                                        .send(Event::error(ErrorEnvelope::classify(
                                            retry_msg, true,
                                        )))
                                        .await;
                                    break;
                                }
                            }
                        }
                        let user_message =
                            stream_read_error_user_message(&message, any_content_received);
                        turn_error.get_or_insert(user_message.clone());
                        let _ = self
                            .tx_event
                            .send(Event::error(ErrorEnvelope::classify(user_message, true)))
                            .await;
                        if stream_errors >= MAX_STREAM_ERRORS_BEFORE_FAIL {
                            break;
                        }
                        continue;
                    }
                };

                match event {
                    StreamEvent::MessageStart { message } => {
                        usage = message.usage;
                    }
                    StreamEvent::ContentBlockStart {
                        index,
                        content_block,
                    } => match content_block {
                        ContentBlockStart::Text { text } => {
                            current_text_raw = text;
                            current_text_visible.clear();
                            in_tool_call_block = false;
                            let filtered =
                                filter_tool_call_delta(&current_text_raw, &mut in_tool_call_block);
                            if !fake_wrapper_notice_emitted
                                && filtered.len() < current_text_raw.len()
                                && contains_fake_tool_wrapper(&current_text_raw)
                            {
                                let _ =
                                    self.tx_event.send(Event::status(FAKE_WRAPPER_NOTICE)).await;
                                fake_wrapper_notice_emitted = true;
                            }
                            current_text_visible.push_str(&filtered);
                            current_block_kind = Some(ContentBlockKind::Text);
                            last_text_index = Some(index as usize);
                            let _ = self
                                .tx_event
                                .send(Event::MessageStarted {
                                    index: index as usize,
                                })
                                .await;
                        }
                        ContentBlockStart::Thinking { thinking } => {
                            current_thinking = thinking;
                            current_block_kind = Some(ContentBlockKind::Thinking);
                            let _ = self
                                .tx_event
                                .send(Event::ThinkingStarted {
                                    index: index as usize,
                                })
                                .await;
                        }
                        ContentBlockStart::ToolUse {
                            id,
                            name,
                            input,
                            caller,
                        } => {
                            crate::logging::info(format!(
                                "Tool '{name}' block start. Initial input: {input:?}"
                            ));
                            current_block_kind = Some(ContentBlockKind::ToolUse);
                            current_tool_indices.insert(index, tool_uses.len());
                            // ToolCallStarted is deferred to ContentBlockStop —
                            // see `final_tool_input`. Emitting here would ship
                            // the placeholder `{}` and the cell would render
                            // `<command>` / `<file>` literals to the user.
                            tool_uses.push(ToolUseState {
                                id,
                                name,
                                input,
                                caller,
                                input_buffer: String::new(),
                            });
                        }
                        ContentBlockStart::ServerToolUse { id, name, input } => {
                            crate::logging::info(format!(
                                "Server tool '{name}' block start. Initial input: {input:?}"
                            ));
                            current_block_kind = Some(ContentBlockKind::ToolUse);
                            current_tool_indices.insert(index, tool_uses.len());
                            tool_uses.push(ToolUseState {
                                id,
                                name,
                                input,
                                caller: None,
                                input_buffer: String::new(),
                            });
                        }
                    },
                    StreamEvent::ContentBlockDelta { index, delta } => match delta {
                        Delta::TextDelta { text } => {
                            stream_content_bytes = stream_content_bytes.saturating_add(text.len());
                            current_text_raw.push_str(&text);
                            let filtered = filter_tool_call_delta(&text, &mut in_tool_call_block);
                            if !fake_wrapper_notice_emitted
                                && filtered.len() < text.len()
                                && contains_fake_tool_wrapper(&text)
                            {
                                let _ =
                                    self.tx_event.send(Event::status(FAKE_WRAPPER_NOTICE)).await;
                                fake_wrapper_notice_emitted = true;
                            }
                            if !filtered.is_empty() {
                                current_text_visible.push_str(&filtered);
                                let _ = self
                                    .tx_event
                                    .send(Event::MessageDelta {
                                        index: index as usize,
                                        content: filtered,
                                    })
                                    .await;
                            }
                        }
                        Delta::ThinkingDelta { thinking } => {
                            stream_content_bytes =
                                stream_content_bytes.saturating_add(thinking.len());
                            current_thinking.push_str(&thinking);
                            if !thinking.is_empty() {
                                let _ = self
                                    .tx_event
                                    .send(Event::ThinkingDelta {
                                        index: index as usize,
                                        content: thinking,
                                    })
                                    .await;
                            }
                        }
                        Delta::SignatureDelta { signature } => {
                            // #3014: capture (and concatenate, defensively)
                            // the signed-thinking signature for replay.
                            match current_thinking_signature.as_mut() {
                                Some(existing) => existing.push_str(&signature),
                                None => current_thinking_signature = Some(signature),
                            }
                        }
                        Delta::InputJsonDelta { partial_json } => {
                            if let Some(&tool_idx) = current_tool_indices.get(&index)
                                && let Some(tool_state) = tool_uses.get_mut(tool_idx)
                            {
                                tool_state.input_buffer.push_str(&partial_json);
                                crate::logging::info(format!(
                                    "Tool '{}' input delta: {} (buffer now: {})",
                                    tool_state.name, partial_json, tool_state.input_buffer
                                ));
                                if let Some(value) = parse_tool_input(&tool_state.input_buffer) {
                                    tool_state.input = value.clone();
                                    crate::logging::info(format!(
                                        "Tool '{}' input parsed: {:?}",
                                        tool_state.name, value
                                    ));
                                }
                            }
                        }
                    },
                    StreamEvent::ContentBlockStop { index } => {
                        let stopped_kind = current_block_kind.take();
                        match stopped_kind {
                            Some(ContentBlockKind::Text) => {
                                pending_message_complete = true;
                                last_text_index = Some(index as usize);
                            }
                            Some(ContentBlockKind::Thinking) => {
                                let _ = self
                                    .tx_event
                                    .send(Event::ThinkingComplete {
                                        index: index as usize,
                                    })
                                    .await;
                            }
                            Some(ContentBlockKind::ToolUse) | None => {}
                        }
                        // Route the Stop using event.index (via
                        // `current_tool_indices`) rather than the single
                        // `current_block_kind` slot. In an OpenAI batch
                        // tool-call stream every Stop after the first sees
                        // `stopped_kind = None` because `take()` cleared the
                        // slot, so the original `matches!(stopped_kind, …)`
                        // check would skip every tool except the last.
                        if let Some(tool_idx) = current_tool_indices.remove(&index)
                            && let Some(tool_state) = tool_uses.get_mut(tool_idx)
                        {
                            crate::logging::info(format!(
                                "Tool '{}' block stop. Buffer: '{}', Current input: {:?}",
                                tool_state.name, tool_state.input_buffer, tool_state.input
                            ));
                            if !tool_state.input_buffer.trim().is_empty() {
                                if let Some(value) = parse_tool_input(&tool_state.input_buffer) {
                                    tool_state.input = value;
                                    crate::logging::info(format!(
                                        "Tool '{}' final input: {:?}",
                                        tool_state.name, tool_state.input
                                    ));
                                } else {
                                    crate::logging::warn(format!(
                                        "Tool '{}' failed to parse final input buffer: '{}'",
                                        tool_state.name, tool_state.input_buffer
                                    ));
                                    let _ = self
                                        .tx_event
                                        .send(Event::status(format!(
                                            "⚠ Tool '{}' received malformed arguments from model",
                                            tool_state.name
                                        )))
                                        .await;
                                }
                            } else {
                                crate::logging::warn(format!(
                                    "Tool '{}' input buffer is empty, using initial input: {:?}",
                                    tool_state.name, tool_state.input
                                ));
                            }

                            // Now that the input is finalized, announce the
                            // tool call to the UI. Deferring to here is what
                            // keeps the cell from rendering `<command>` /
                            // `<file>` placeholders during the brief window
                            // between block start and the last InputJsonDelta.
                            let _ = self
                                .tx_event
                                .send(Event::ToolCallStarted {
                                    id: tool_state.id.clone(),
                                    name: tool_state.name.clone(),
                                    input: final_tool_input(tool_state),
                                })
                                .await;
                        }
                    }
                    StreamEvent::MessageDelta {
                        usage: delta_usage, ..
                    } => {
                        if let Some(u) = delta_usage {
                            usage = u;
                        }
                    }
                    StreamEvent::MessageStop | StreamEvent::Ping => {}
                    StreamEvent::Error { error } => {
                        // #3014: Anthropic SSE error event. The adapter
                        // surfaces fatal errors as stream Err items; this
                        // defensive arm keeps any passed-through error
                        // visible instead of silently dropped.
                        crate::logging::warn(format!("Provider stream error event: {error}"));
                        stream_errors += 1;
                    }
                }
            }

            if self.cancel_token.is_cancelled() {
                let _ = self.tx_event.send(Event::status("Request cancelled")).await;
                self.persist_loop_guard_state(&self.session.id, &loop_guard)
                    .await;
                return (TurnOutcomeStatus::Interrupted, None);
            }

            // #103 Phase 3 — transparent retry. The inner loop above bails
            // when reqwest yields chunk decode errors three times in a row;
            // most of the time those are recoverable proxy / HTTP/2 issues
            // and the request can simply be re-issued. Re-issue silently up
            // to MAX_STREAM_RETRIES, but only when the stream produced
            // nothing actionable — if any tool call landed or text was
            // streamed, ship the partial state to the rest of the turn
            // pipeline so we don't double-bill the user by re-running it.
            // Also retry when the stream ended cleanly but produced no
            // content (empty response) — this avoids requiring the user
            // to manually resend (#169).
            let stream_died_with_nothing = (stream_errors > 0
                || (stream_errors == 0 && !pending_message_complete))
                && tool_uses.is_empty()
                && current_text_visible.trim().is_empty()
                && current_thinking.trim().is_empty()
                && !pending_message_complete;
            if stream_died_with_nothing || sleep_resume_pending {
                if stream_retry_attempts < MAX_STREAM_RETRIES {
                    stream_retry_attempts = stream_retry_attempts.saturating_add(1);
                    if sleep_resume_pending {
                        crate::logging::warn(format!(
                            "Resuming after system sleep (attempt {stream_retry_attempts}/{MAX_STREAM_RETRIES}); discarding partial output and retrying request"
                        ));
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "System sleep detected; connection lost — retrying request ({stream_retry_attempts}/{MAX_STREAM_RETRIES})"
                            )))
                            .await;
                        // Finalize any partially-rendered assistant cell so
                        // the retried stream renders fresh instead of
                        // appending to the pre-sleep fragment.
                        if pending_message_complete {
                            let index = last_text_index.unwrap_or(0);
                            let _ = self.tx_event.send(Event::MessageComplete { index }).await;
                        }
                    } else {
                        crate::logging::warn(format!(
                            "Stream died with no content (attempt {stream_retry_attempts}/{MAX_STREAM_RETRIES}); retrying request"
                        ));
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "Connection interrupted; retrying ({stream_retry_attempts}/{MAX_STREAM_RETRIES})"
                            )))
                            .await;
                    }
                    // Don't preserve the per-stream `turn_error` — we're
                    // about to retry, and a successful retry should not
                    // surface the transient error as the turn outcome.
                    turn_error = None;
                    continue;
                }
                crate::logging::warn(format!(
                    "Stream retry budget exhausted ({stream_retry_attempts} attempts); failing turn"
                ));
            } else if stream_errors == 0 {
                // Healthy round → reset retry budget so we don't carry over
                // state from a previous bad round.
                stream_retry_attempts = 0;
            }

            // Update turn usage
            turn.add_usage(&usage);

            // Build content blocks. If this assistant turn produced tool
            // calls, ensure a Thinking block is present even when the model
            // didn't stream any reasoning text — DeepSeek's thinking-mode
            // API requires `reasoning_content` to accompany every tool-call
            // assistant message in the conversation history. Saving a
            // placeholder here keeps the on-disk session structurally
            // correct so subsequent requests won't 400.
            let needs_thinking_block =
                !tool_uses.is_empty() || tool_parser::has_tool_call_markers(&current_text_raw);
            let thinking_to_persist = if !current_thinking.is_empty() {
                Some(current_thinking.clone())
            } else if needs_thinking_block {
                Some(String::from("(reasoning omitted)"))
            } else {
                None
            };
            if let Some(thinking) = thinking_to_persist {
                content_blocks.push(ContentBlock::Thinking {
                    thinking,
                    signature: current_thinking_signature.clone(),
                });
            }
            let mut final_text = current_text_visible.clone();
            if tool_uses.is_empty() && tool_parser::has_tool_call_markers(&current_text_raw) {
                let parsed = tool_parser::parse_tool_calls(&current_text_raw);
                final_text = parsed.clean_text;
                for call in parsed.tool_calls {
                    let _ = self
                        .tx_event
                        .send(Event::ToolCallStarted {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            input: call.args.clone(),
                        })
                        .await;
                    tool_uses.push(ToolUseState {
                        id: call.id,
                        name: call.name,
                        input: call.args,
                        caller: None,
                        input_buffer: String::new(),
                    });
                }
            }

            // Deduplicate exact-match tool calls within a single LLM response.
            // When the model emits identical (name, input) pairs, keep only the
            // first occurrence to avoid redundant execution.
            {
                let mut seen = std::collections::HashSet::new();
                tool_uses.retain(|t| {
                    // Serialize input to string for hashing since serde_json::Value
                    // does not implement Hash/Eq.
                    let key = match serde_json::to_string(&t.input) {
                        Ok(s) => format!("{}:{}", t.name, s),
                        Err(_) => format!("{}:{:?}", t.name, t.input),
                    };
                    seen.insert(key)
                });
            }

            if !final_text.is_empty() {
                content_blocks.push(ContentBlock::Text {
                    text: final_text,
                    cache_control: None,
                });
            }
            for tool in &tool_uses {
                content_blocks.push(ContentBlock::ToolUse {
                    id: tool.id.clone(),
                    name: tool.name.clone(),
                    input: tool.input.clone(),
                    caller: tool.caller.clone(),
                });
            }

            if pending_message_complete {
                let index = last_text_index.unwrap_or(0);
                let _ = self.tx_event.send(Event::MessageComplete { index }).await;
            }

            // RLM is a structured tool call (`rlm_query`) handled by the
            // normal tool dispatch path; inline ```repl blocks (paper §2)
            // are executed below when tool_uses is empty.
            // DeepSeek chat API rejects assistant messages that contain only
            // Keep thinking for UI stream events, but persist only sendable
            // assistant turns in the conversation state.
            let has_sendable_assistant_content = content_blocks.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { .. } | ContentBlock::ToolUse { .. }
                )
            });

            // Issue #1727: did this turn produce ONLY a reasoning/thinking
            // block — empty content, no tool calls (e.g. gpt-oss via ollama's
            // harmony→OpenAI shim mapping to `reasoning_content`)? We do NOT
            // surface anything here: after this point the same turn can still
            // CONTINUE for pending steers (~below) or sub-agent completions,
            // and emitting now would show a spurious "turn ended" notice right
            // before the turn resumes. Capture the fact and decide later, at
            // the point the turn is certain to be finishing with no sendable
            // content (see the `tool_uses.is_empty()` tail).
            let thinking_only_no_sendable = !has_sendable_assistant_content;

            // Add assistant message to session
            if has_sendable_assistant_content {
                self.add_session_message(Message {
                    role: "assistant".to_string(),
                    content: content_blocks,
                })
                .await;
            }

            // If no tool uses, check for inline REPL blocks (paper §2) or
            // finish the turn.
            if tool_uses.is_empty() {
                if !pending_steers.is_empty() {
                    for steer in pending_steers.drain(..) {
                        self.session
                            .working_set
                            .observe_user_message(&steer, &self.session.workspace);
                        self.add_session_message(self.user_text_message_with_turn_metadata(steer))
                            .await;
                    }
                    turn.next_step();
                    continue;
                }

                let shell_completions = self.drain_shell_completion_events();
                if let Some(status) = shell_completion_status_text(&shell_completions, "") {
                    let _ = self.tx_event.send(Event::status(status)).await;
                }
                // #696: inject a `<task-notification>` runtime event so the
                // model sees the background job result, not just a UI status.
                if let Some(notification) = shell_completion_notification_text(&shell_completions) {
                    self.add_session_message(
                        self.user_text_message_with_turn_metadata(notification),
                    )
                    .await;
                }

                // Sub-agent completion handoff (issue #756). The model finished
                // streaming with no tool calls — but if it has direct children
                // still running (or completions queued from children that
                // finished while we were inferring), surface their
                // `<mimo:subagent.done>` sentinels into the transcript and
                // resume instead of ending the turn. This fulfils the contract
                // already documented in `prompts/constitution.md`: the parent is
                // promised it'll see the sentinel when a child finishes.
                let subagent_completions = self.drain_subagent_completion_events("").await;
                if subagent_completions == 0 {
                    // #3216: do NOT barrier the parent on running children.
                    // Launching a sub-agent is not the same as joining it — the
                    // parent ends its turn and stays responsive. Running children
                    // are background work; their results return via the
                    // completion sentinel on a later turn. Stale children are filtered out of
                    // `running_count` by the manager's heartbeat, so they neither
                    // block nor inflate the surfaced count. (Previously the parent
                    // waited in a select! loop here until a completion or the
                    // heartbeat timeout, which read as a hard TUI freeze.)
                    // Cancellation and steering are handled at the top of the step
                    // loop; stale-agent cleanup is the manager's responsibility.
                    let running = {
                        let mgr = self.subagent_manager.read().await;
                        mgr.running_count()
                    };
                    if running > 0 {
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "Turn ending with {running} sub-agent(s) still running in the background; they'll report when done."
                            )))
                            .await;
                    }
                }
                if subagent_completions > 0 {
                    turn.next_step();
                    continue;
                }

                // Inline ```repl execution — paper-spec RLM integration.
                if has_sendable_assistant_content
                    && crate::repl::sandbox::has_repl_block(&current_text_visible)
                {
                    let repl_blocks =
                        crate::repl::sandbox::extract_repl_blocks(&current_text_visible);
                    let mut runtime = match crate::repl::runtime::PythonRuntime::new().await {
                        Ok(rt) => rt,
                        Err(e) => {
                            let _ = self
                                .tx_event
                                .send(Event::status(format!("REPL init failed: {e}")))
                                .await;
                            break;
                        }
                    };

                    let mut final_result: Option<String> = None;
                    for (i, block) in repl_blocks.iter().enumerate() {
                        let round_num = i + 1;
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "REPL round {round_num}: executing..."
                            )))
                            .await;

                        match runtime.execute(&block.code).await {
                            Ok(round) => {
                                if let Some(val) = &round.final_value {
                                    let _ = self
                                        .tx_event
                                        .send(Event::status(format!(
                                            "REPL round {round_num}: FINAL result obtained"
                                        )))
                                        .await;
                                    final_result = Some(val.clone());
                                    break;
                                }

                                // No FINAL — feed truncated stdout back as user metadata.
                                let feedback = if round.has_error {
                                    format!(
                                        "[REPL round {round_num} error]\nstdout:\n{}\nstderr:\n{}",
                                        round.stdout, round.stderr
                                    )
                                } else {
                                    format!("[REPL round {round_num} output]\n{}", round.stdout)
                                };
                                self.add_session_message(
                                    self.runtime_text_message_with_turn_metadata(
                                        feedback,
                                        UserInputProvenance::Runtime,
                                    ),
                                )
                                .await;
                            }
                            Err(e) => {
                                let _ = self
                                    .tx_event
                                    .send(Event::status(format!(
                                        "REPL round {round_num} failed: {e}"
                                    )))
                                    .await;
                                self.add_session_message(
                                    self.runtime_text_message_with_turn_metadata(
                                        format!("[REPL round {round_num} execution failed]\n{e}"),
                                        UserInputProvenance::Runtime,
                                    ),
                                )
                                .await;
                            }
                        }
                    }

                    if let Some(final_val) = final_result {
                        // Replace the assistant's text with the FINAL answer.
                        if let Some(last_msg) = self.session.messages.last_mut()
                            && last_msg.role == "assistant"
                        {
                            for block in &mut last_msg.content {
                                if let ContentBlock::Text { text, .. } = block {
                                    *text = final_val;
                                    break;
                                }
                            }
                        }
                        self.emit_session_updated().await;
                        break;
                    }

                    // No FINAL — let the model iterate with the feedback.
                    turn.next_step();
                    continue;
                }

                // Issue #1727: the turn is now genuinely finishing with no
                // sendable content. Control only reaches here when there were
                // no pending steers (`continue`d above), no sub-agent
                // completions to resume with, and we were not holding for
                // running children (the `should_hold_turn_for_subagents`
                // branch above would have awaited / `continue`d / returned).
                // If the assistant produced ONLY a reasoning block, the prior
                // code fell straight through to this `break`, emitting nothing
                // and leaving the UI spinner hung. Surface a status now —
                // safe because the turn can no longer resume.
                // #1961: Before breaking, drain any sub-agent completions that
                // arrived between the last hold check and now. If a child finished
                // while we were running the thinking-only check, surface its
                // sentinel rather than delaying it to the next turn.
                let late_shell_completions = self.drain_shell_completion_events();
                if let Some(status) = shell_completion_status_text(&late_shell_completions, "late")
                {
                    let _ = self.tx_event.send(Event::status(status)).await;
                }
                // #696: same completion-notification injection as the main
                // drain site, for jobs that finished during the late check.
                if let Some(notification) =
                    shell_completion_notification_text(&late_shell_completions)
                {
                    self.add_session_message(
                        self.user_text_message_with_turn_metadata(notification),
                    )
                    .await;
                }

                if self.drain_subagent_completion_events("late").await > 0 {
                    turn.next_step();
                    continue;
                }

                if let Some(continuation) = self
                    .goal_continuation_message_if_needed(
                        tool_registry,
                        &mut goal_continuations_this_turn,
                    )
                    .await
                {
                    self.add_session_message(self.runtime_text_message_with_turn_metadata(
                        continuation,
                        UserInputProvenance::Runtime,
                    ))
                    .await;
                    turn.next_step();
                    continue;
                }

                if thinking_only_no_sendable {
                    let holding_for_subagents = {
                        let running = {
                            let mgr = self.subagent_manager.read().await;
                            mgr.running_count()
                        };
                        should_hold_turn_for_subagents(0, running)
                    };
                    if should_emit_thinking_only_status(
                        tool_uses.is_empty(),
                        turn_error.is_none(),
                        self.cancel_token.is_cancelled(),
                        !pending_steers.is_empty(),
                        holding_for_subagents,
                    ) {
                        let message = "Model returned reasoning but no answer or tool call; \
                                       turn ended without output. Send a follow-up to retry."
                            .to_string();
                        crate::logging::warn(&message);
                        let _ = self.tx_event.send(Event::status(message)).await;
                    }
                }

                break;
            }

            // Execute tools
            if self.shared_paused.lock().is_ok_and(|paused| *paused) {
                let _ = self
                    .tx_event
                    .send(Event::status("Request was Paused"))
                    .await;
                self.persist_loop_guard_state(&self.session.id, &loop_guard)
                    .await;
                return (TurnOutcomeStatus::Interrupted, None);
            }

            let tool_exec_lock = self.tool_exec_lock.clone();
            let mcp_pool = if tool_uses
                .iter()
                .any(|tool| McpPool::is_mcp_tool(&tool.name))
            {
                match self.ensure_mcp_pool().await {
                    Ok(pool) => Some(pool),
                    Err(err) => {
                        let _ = self.tx_event.send(Event::status(err.to_string())).await;
                        None
                    }
                }
            } else {
                None
            };

            let active_tools_at_batch_start = active_tool_names.clone();
            let mut deferred_tools_hydrated_this_batch: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            // #3026: `additionalContext` strings from tool_call_before hooks,
            // keyed by tool id; appended to the tool result sent to the model.
            let mut hook_contexts: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let mut plans: Vec<ToolExecutionPlan> = Vec::with_capacity(tool_uses.len());
            for (index, tool) in tool_uses.iter_mut().enumerate() {
                let tool_id = tool.id.clone();
                let mut tool_name = tool.name.clone();
                let mut tool_input = tool.input.clone();
                let tool_caller = tool.caller.clone();
                crate::logging::info(format!(
                    "Planning tool '{tool_name}' with input: {tool_input:?}"
                ));

                let requested_tool_name = tool_name.clone();
                let tool_def =
                    resolve_tool_definition(&mut tool_name, &tool_catalog, tool_registry);
                if requested_tool_name != tool_name {
                    tool.name = tool_name.clone();
                }

                let interactive = (tool_name == "exec_shell"
                    && tool_input
                        .get("interactive")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true))
                    || tool_name == REQUEST_USER_INPUT_NAME
                    // Blocks on the user's approval decision, so it must run
                    // serially like any other interactive prompt.
                    || tool_name == EXIT_PLAN_MODE_NAME;

                let mut approval_required = false;
                let mut approval_description = "Tool execution requires approval".to_string();
                let mut approval_force_prompt = false;
                let mut supports_parallel = false;
                let mut read_only = false;
                let mut detached_start = false;
                let mut blocked_error: Option<ToolError> = None;
                let mut guard_result: Option<ToolResult> = None;
                // #3026: set by a hook `ask` decision; applied AFTER the
                // registry-based approval computation below so it cannot be
                // clobbered by it.
                let mut hook_requires_approval = false;

                if mode == AppMode::Plan
                    && matches!(
                        tool_name.as_str(),
                        "exec_shell"
                            | "exec_shell_wait"
                            | "exec_shell_interact"
                            | CODE_EXECUTION_TOOL_NAME
                            | JS_EXECUTION_TOOL_NAME
                    )
                {
                    blocked_error = Some(ToolError::permission_denied(format!(
                        "'{tool_name}' is not available in Plan mode — switch to Agent or YOLO mode to run commands and code."
                    )));
                }

                // #3027: deny wins over allow — check the deny-list first so a
                // tool present in both lists is still blocked.
                if blocked_error.is_none()
                    && command_denies_tool(self.config.disallowed_tools.as_deref(), &tool_name)
                {
                    blocked_error = Some(ToolError::permission_denied(format!(
                        "Tool '{tool_name}' is in the disallowed-tools list"
                    )));
                }

                if blocked_error.is_none()
                    && !command_allows_tool(self.config.allowed_tools.as_deref(), &tool_name)
                {
                    blocked_error = Some(ToolError::permission_denied(format!(
                        "Tool '{tool_name}' is not in the allowed-tools list for the current command"
                    )));
                }

                if blocked_error.is_none()
                    && !caller_allowed_for_tool(tool_caller.as_ref(), tool_def)
                {
                    blocked_error = Some(ToolError::permission_denied(format!(
                        "Tool '{tool_name}' does not allow caller '{}'",
                        caller_type_for_tool_use(tool_caller.as_ref())
                    )));
                }

                if blocked_error.is_none()
                    && tool_def.is_none()
                    && !McpPool::is_mcp_tool(&tool_name)
                    && tool_name != CODE_EXECUTION_TOOL_NAME
                    && tool_name != JS_EXECUTION_TOOL_NAME
                    && !is_tool_search_tool(&tool_name)
                {
                    blocked_error = Some(ToolError::not_available(missing_tool_error_message(
                        &tool_name,
                        &tool_catalog,
                    )));
                }

                if blocked_error.is_none()
                    && let Some(hook_executor) = self.config.hook_executor.as_ref()
                    && hook_executor.has_hooks_for_event(crate::hooks::HookEvent::ToolCallBefore)
                {
                    // Warn if any ToolCallBefore hook is configured as background
                    // — background hooks return exit_code: None immediately, so
                    // the denial check (exit_code == Some(2)) can never match.
                    if hook_executor
                        .has_background_hooks_for_event(crate::hooks::HookEvent::ToolCallBefore)
                    {
                        tracing::warn!(
                            "ToolCallBefore hook(s) configured with background=true — \
                             background hooks cannot deny tool calls because they exit \
                             immediately with no result"
                        );
                    }

                    let hook_context = crate::hooks::HookContext::new()
                        .with_tool_name(&tool_name)
                        .with_tool_args(&tool_input)
                        .with_mode(&format!("{mode:?}"))
                        .with_workspace(self.session.workspace.clone())
                        .with_model(&self.config.model)
                        .with_session_id(&self.session.id);
                    // Run hooks off the Tokio worker thread: `execute()` calls
                    // `child.wait_timeout()` which is a blocking syscall that
                    // would stall all other async tasks on this thread.
                    let executor = hook_executor.clone();
                    let hook_results = tokio::task::spawn_blocking(move || {
                        executor.execute(crate::hooks::HookEvent::ToolCallBefore, &hook_context)
                    })
                    .await
                    .unwrap_or_else(|join_err| {
                        tracing::error!("Hook executor task panicked: {join_err}");
                        Vec::new()
                    });
                    // #3026: fold all foreground hook results into one
                    // decision: deny (exit code 2 or JSON) > ask > allow;
                    // last `updatedInput` writer wins; `additionalContext`
                    // strings are concatenated.
                    let fold = fold_tool_call_before_results(&hook_results);
                    if let Some(reason) = fold.deny_reason {
                        blocked_error = Some(ToolError::permission_denied(format!(
                            "ToolCallBefore hook denied tool '{tool_name}': {reason}"
                        )));
                    } else {
                        if fold.requires_approval {
                            hook_requires_approval = true;
                        }
                        if let Some(updated) = fold.updated_input {
                            tool_input = updated;
                        }
                        if let Some(context) = fold.additional_context {
                            hook_contexts.insert(tool_id.clone(), context);
                        }
                    }
                }

                if McpPool::is_mcp_tool(&tool_name) {
                    read_only = mcp_tool_is_read_only(&tool_name);
                    supports_parallel = mcp_tool_is_parallel_safe(&tool_name);
                    approval_required = !read_only;
                    approval_description = mcp_tool_approval_description(&tool_name);
                } else if let Some(registry) = tool_registry
                    && let Some(spec) = registry.get(&tool_name)
                {
                    approval_required = registered_tool_approval_required(
                        &tool_name,
                        spec.approval_requirement_for(&tool_input),
                        registry.context().auto_approve,
                    );
                    approval_description = spec.description().to_string();
                    supports_parallel = spec.supports_parallel_for(&tool_input);
                    read_only = spec.is_read_only_for(&tool_input);
                    detached_start = spec.starts_detached_for(&tool_input);
                } else if tool_name == CODE_EXECUTION_TOOL_NAME {
                    approval_required = true;
                    approval_description =
                        "Run model-provided Python code in local execution sandbox".to_string();
                    supports_parallel = false;
                    read_only = false;
                } else if tool_name == JS_EXECUTION_TOOL_NAME {
                    approval_required = true;
                    approval_description =
                        "Run model-provided JavaScript code in local Node.js execution sandbox"
                            .to_string();
                    supports_parallel = false;
                    read_only = false;
                } else if is_tool_search_tool(&tool_name) {
                    approval_required = false;
                    approval_description = "Search tool catalog".to_string();
                    supports_parallel = false;
                    read_only = true;
                }

                // #3026: a hook `ask` decision forces the approval prompt even
                // for tools the registry would auto-run. Must stay after the
                // registry-based computation above, which assigns rather than
                // ORs `approval_required`.
                if hook_requires_approval {
                    approval_required = true;
                }

                if blocked_error.is_none() {
                    let ask_rule_decision = exec_shell_ask_rule_decision(
                        &self.config,
                        &tool_name,
                        &tool_input,
                        &self.session.workspace,
                        self.session.approval_mode,
                    )
                    .or_else(|| {
                        file_tool_ask_rule_decision(
                            &self.config,
                            &tool_name,
                            &tool_input,
                            &self.session.workspace,
                            self.session.approval_mode,
                        )
                    });
                    if let Some(decision) = ask_rule_decision {
                        match decision {
                            ToolAskRuleDecision::Prompt(reason) => {
                                // YOLO mode (auto_approve) is the explicit
                                // "no approvals" contract: a typed ask-rule
                                // must not pop a modal in YOLO. The
                                // auto_review safety floor below still
                                // independently holds publish/destructive
                                // actions, and a typed deny rule still
                                // blocks hard.
                                if !self.session.auto_approve {
                                    approval_required = true;
                                    approval_description = reason;
                                    approval_force_prompt = true;
                                }
                            }
                            ToolAskRuleDecision::Block(reason) => {
                                approval_required = false;
                                approval_force_prompt = false;
                                blocked_error = Some(ToolError::permission_denied(reason));
                            }
                        }
                    }
                }

                if blocked_error.is_none() {
                    let (decision, audit_event) = auto_review_plan_decision(
                        &self.config.auto_review_policy,
                        &tool_name,
                        &tool_input,
                        auto_review_run_origin_for_plan(detached_start),
                        self.session.approval_mode,
                        None,
                        crate::config::is_workspace_trusted(&self.session.workspace),
                        false,
                    );
                    emit_tool_audit(json!({
                        "event": "tool.auto_review_decision",
                        "tool_id": tool_id.clone(),
                        "auto_review": audit_event,
                    }));
                    match decision {
                        AutoReviewPlanDecision::NoChange => {}
                        AutoReviewPlanDecision::ForcePrompt(reason) => {
                            approval_required = true;
                            approval_description = reason;
                            approval_force_prompt = true;
                        }
                        AutoReviewPlanDecision::Block(reason) => {
                            approval_required = false;
                            approval_force_prompt = false;
                            blocked_error = Some(ToolError::permission_denied(reason));
                        }
                    }
                }

                let should_emit_hydration_status =
                    !deferred_tools_hydrated_this_batch.contains(&tool_name);
                if blocked_error.is_none()
                    && let Some(result) = maybe_hydrate_requested_deferred_tool(
                        &tool_name,
                        &tool_input,
                        &tool_catalog,
                        &active_tools_at_batch_start,
                        &mut deferred_tools_hydrated_this_batch,
                    )
                {
                    if should_emit_hydration_status {
                        let status = if requested_tool_name == tool_name {
                            format!("Auto-loaded deferred tool '{tool_name}' after model request.")
                        } else {
                            format!(
                                "Auto-loaded deferred tool '{tool_name}' after resolving '{requested_tool_name}'."
                            )
                        };
                        let _ = self.tx_event.send(Event::status(status)).await;
                    }
                    guard_result = Some(result);
                }

                plans.push(ToolExecutionPlan {
                    index,
                    id: tool_id,
                    name: tool_name,
                    input: tool_input,
                    caller: tool_caller,
                    interactive,
                    approval_required,
                    approval_description,
                    approval_force_prompt,
                    supports_parallel,
                    read_only,
                    detached_start,
                    blocked_error,
                    guard_result,
                });
            }
            active_tool_names.extend(deferred_tools_hydrated_this_batch);

            // --- Intent summary for write tools (#2381) ---
            // When the model invokes write tools, extract its preceding text
            // as an "intent summary" so the approval view can show *why* the
            // change is being made, not just *what* will change.
            let has_write_tools = plans.iter().any(|p| {
                !p.read_only
                    && p.approval_required
                    && p.blocked_error.is_none()
                    && p.guard_result.is_none()
            });
            let intent_summary: Option<String> = if has_write_tools {
                approval_intent_summary(&current_text_visible)
            } else {
                None
            };

            let plan_count = plans.len();
            let batches = plan_tool_execution_batches(plans);
            let parallel_chunks = batches
                .iter()
                .filter_map(|batch| match batch {
                    ToolExecutionBatch::Parallel(plans) if plans.len() > 1 => Some(plans.len()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if !parallel_chunks.is_empty() {
                let parallel_tool_count: usize = parallel_chunks.iter().sum();
                let detached_start_count: usize = batches
                    .iter()
                    .filter_map(|batch| match batch {
                        ToolExecutionBatch::Parallel(plans) if plans.len() > 1 => {
                            Some(plans.iter().filter(|plan| plan.detached_start).count())
                        }
                        _ => None,
                    })
                    .sum();
                let tool_kind = if detached_start_count > 0 {
                    "read-only/background-start tools"
                } else {
                    "read-only tools"
                };
                let _ = self
                    .tx_event
                    .send(Event::status(format!(
                        "Executing {parallel_tool_count} {tool_kind} in {} parallel chunk(s)",
                        parallel_chunks.len(),
                    )))
                    .await;
            } else if plan_count > 1 {
                let _ = self
                    .tx_event
                    .send(Event::status(
                        "Executing tools sequentially (writes, approvals, or non-parallel tools detected)",
                    ))
                    .await;
            }

            let mut outcomes: Vec<Option<ToolExecOutcome>> = Vec::with_capacity(plan_count);
            outcomes.resize_with(plan_count, || None);

            for batch in batches {
                let (parallel_allowed, plans) = match batch {
                    ToolExecutionBatch::Parallel(plans) => (true, plans),
                    ToolExecutionBatch::Serial(plan) => (false, vec![*plan]),
                };

                // #3216 / #2211: once the turn is cancelled, do not start any
                // further tool batches. Cancellation arrives out-of-band (the
                // TUI cancels the shared token directly), so we can observe it
                // here even while a long serial fan-out — e.g. six `agent`
                // calls each resolving a model route under the global tool lock
                // — is mid-flight. Without this check the batch loop ran to
                // completion (~6×4s) with no way to interrupt, which read as a
                // hard TUI freeze. We record an interrupted result for every
                // remaining plan so each `tool_use` keeps a matching
                // `tool_result` (well-formed transcript), then fall through to
                // the post-loop cancellation check which ends the turn as
                // Interrupted. This branch is a no-op on the normal path.
                if self.cancel_token.is_cancelled() {
                    for plan in plans {
                        let result = Ok(interrupted_tool_result());
                        let _ = self
                            .tx_event
                            .send(Event::ToolCallComplete {
                                id: plan.id.clone(),
                                name: plan.name.clone(),
                                result: result.clone(),
                            })
                            .await;
                        outcomes[plan.index] = Some(ToolExecOutcome {
                            index: plan.index,
                            id: plan.id,
                            name: plan.name,
                            input: plan.input,
                            started_at: Instant::now(),
                            result,
                        });
                    }
                    continue;
                }

                if parallel_allowed {
                    let mut tool_tasks = FuturesUnordered::new();
                    let shell_permits = Arc::new(tokio::sync::Semaphore::new(
                        self.api_config
                            .limits
                            .as_ref()
                            .and_then(|cfg| cfg.max_parallel_shell_exec)
                            .unwrap_or(MAX_PARALLEL_SHELL_EXEC),
                    ));
                    for plan in plans {
                        if let Some(result) = plan.guard_result.clone() {
                            let result = Ok(result);
                            let _ = self
                                .tx_event
                                .send(Event::ToolCallComplete {
                                    id: plan.id.clone(),
                                    name: plan.name.clone(),
                                    result: result.clone(),
                                })
                                .await;
                            outcomes[plan.index] = Some(ToolExecOutcome {
                                index: plan.index,
                                id: plan.id,
                                name: plan.name,
                                input: plan.input,
                                started_at: Instant::now(),
                                result,
                            });
                            continue;
                        }
                        if let Some(err) = plan.blocked_error.clone() {
                            outcomes[plan.index] = Some(ToolExecOutcome {
                                index: plan.index,
                                id: plan.id,
                                name: plan.name,
                                input: plan.input,
                                started_at: Instant::now(),
                                result: Err(err),
                            });
                            continue;
                        }
                        let registry = tool_registry;
                        let lock = tool_exec_lock.clone();
                        let mcp_pool = mcp_pool.clone();
                        let tx_event = self.tx_event.clone();
                        let session_id = self.session.id.clone();
                        let started_at = Instant::now();
                        let shell_permits = shell_permits.clone();
                        let workspace = self.session.workspace.clone();

                        tool_tasks.push(async move {
                            let _shell_permit = if plan.name == "exec_shell" {
                                shell_permits.acquire_owned().await.ok()
                            } else {
                                None
                            };
                            let mut result = Engine::execute_tool_with_lock(
                                lock,
                                plan.supports_parallel || plan.detached_start,
                                plan.interactive,
                                tx_event.clone(),
                                plan.name.clone(),
                                plan.input.clone(),
                                workspace,
                                registry,
                                mcp_pool,
                                None,
                            )
                            .await;

                            // #500: spill outsized output before fanout (mirror
                            // of the sequential path below). Emit a
                            // `tool.spillover` audit event so operators can
                            // correlate large-output episodes with disk usage.
                            if let Ok(tool_result) = result.as_mut()
                                && let Some(path) =
                                    crate::tools::truncate::apply_spillover_with_artifact(
                                        tool_result,
                                        &plan.id,
                                        &plan.name,
                                        &session_id,
                                    )
                            {
                                emit_tool_audit(json!({
                                    "event": "tool.spillover",
                                    "tool_id": plan.id.clone(),
                                    "tool_name": plan.name.clone(),
                                    "path": path.display().to_string(),
                                }));
                            }

                            let _ = tx_event
                                .send(Event::ToolCallComplete {
                                    id: plan.id.clone(),
                                    name: plan.name.clone(),
                                    result: result.clone(),
                                })
                                .await;

                            ToolExecOutcome {
                                index: plan.index,
                                id: plan.id,
                                name: plan.name,
                                input: plan.input,
                                started_at,
                                result,
                            }
                        });
                    }

                    while let Some(outcome) = tool_tasks.next().await {
                        let index = outcome.index;
                        outcomes[index] = Some(outcome);
                    }
                } else {
                    for plan in plans {
                        let tool_id = plan.id.clone();
                        let tool_name = plan.name.clone();
                        let tool_input = plan.input.clone();
                        let tool_caller = plan.caller.clone();

                        if let Some(result) = plan.guard_result.clone() {
                            let result = Ok(result);
                            let _ = self
                                .tx_event
                                .send(Event::ToolCallComplete {
                                    id: tool_id.clone(),
                                    name: tool_name.clone(),
                                    result: result.clone(),
                                })
                                .await;
                            outcomes[plan.index] = Some(ToolExecOutcome {
                                index: plan.index,
                                id: tool_id,
                                name: tool_name,
                                input: tool_input,
                                started_at: Instant::now(),
                                result,
                            });
                            continue;
                        }

                        if let Some(err) = plan.blocked_error.clone() {
                            let result = Err(err);
                            let _ = self
                                .tx_event
                                .send(Event::ToolCallComplete {
                                    id: tool_id.clone(),
                                    name: tool_name.clone(),
                                    result: result.clone(),
                                })
                                .await;
                            outcomes[plan.index] = Some(ToolExecOutcome {
                                index: plan.index,
                                id: tool_id,
                                name: tool_name,
                                input: tool_input,
                                started_at: Instant::now(),
                                result,
                            });
                            continue;
                        }

                        if tool_name == MULTI_TOOL_PARALLEL_NAME {
                            let started_at = Instant::now();
                            let result = self
                                .execute_parallel_tool(
                                    tool_input.clone(),
                                    tool_registry,
                                    tool_exec_lock.clone(),
                                )
                                .await;

                            let _ = self
                                .tx_event
                                .send(Event::ToolCallComplete {
                                    id: tool_id.clone(),
                                    name: tool_name.clone(),
                                    result: result.clone(),
                                })
                                .await;

                            outcomes[plan.index] = Some(ToolExecOutcome {
                                index: plan.index,
                                id: tool_id,
                                name: tool_name,
                                input: tool_input,
                                started_at,
                                result,
                            });
                            continue;
                        }

                        if is_tool_search_tool(&tool_name) {
                            let started_at = Instant::now();
                            let result = execute_tool_search(
                                &tool_name,
                                &tool_input,
                                &tool_catalog,
                                &mut active_tool_names,
                            );

                            let _ = self
                                .tx_event
                                .send(Event::ToolCallComplete {
                                    id: tool_id.clone(),
                                    name: tool_name.clone(),
                                    result: result.clone(),
                                })
                                .await;

                            outcomes[plan.index] = Some(ToolExecOutcome {
                                index: plan.index,
                                id: tool_id,
                                name: tool_name,
                                input: tool_input,
                                started_at,
                                result,
                            });
                            continue;
                        }

                        if tool_name == EXIT_PLAN_MODE_NAME {
                            let started_at = Instant::now();
                            let result =
                                self.await_plan_approval(&tool_id, &tool_input, mode).await;

                            let _ = self
                                .tx_event
                                .send(Event::ToolCallComplete {
                                    id: tool_id.clone(),
                                    name: tool_name.clone(),
                                    result: result.clone(),
                                })
                                .await;

                            outcomes[plan.index] = Some(ToolExecOutcome {
                                index: plan.index,
                                id: tool_id,
                                name: tool_name,
                                input: tool_input,
                                started_at,
                                result,
                            });
                            continue;
                        }

                        if tool_name == REQUEST_USER_INPUT_NAME {
                            let started_at = Instant::now();
                            let result = match UserInputRequest::from_value(&tool_input) {
                                Ok(request) => self
                                    .await_user_input(&tool_id, request)
                                    .await
                                    .and_then(|response| {
                                        ToolResult::json(&response)
                                            .map_err(|e| ToolError::execution_failed(e.to_string()))
                                    }),
                                Err(err) => Err(err),
                            };

                            let _ = self
                                .tx_event
                                .send(Event::ToolCallComplete {
                                    id: tool_id.clone(),
                                    name: tool_name.clone(),
                                    result: result.clone(),
                                })
                                .await;

                            outcomes[plan.index] = Some(ToolExecOutcome {
                                index: plan.index,
                                id: tool_id,
                                name: tool_name,
                                input: tool_input,
                                started_at,
                                result,
                            });
                            continue;
                        }

                        // Handle approval flow: returns (result_override, context_override)
                        let (result_override, context_override): (
                            Option<Result<ToolResult, ToolError>>,
                            Option<crate::tools::ToolContext>,
                        ) = if plan.approval_required {
                            emit_tool_audit(json!({
                                "event": "tool.approval_required",
                                "tool_id": tool_id.clone(),
                                "tool_name": tool_name.clone(),
                            }));
                            let approval_key = crate::tools::approval_cache::build_approval_key(
                                &tool_name,
                                &tool_input,
                            )
                            .0;
                            let approval_grouping_key =
                                crate::tools::approval_cache::build_approval_grouping_key(
                                    &tool_name,
                                    &tool_input,
                                )
                                .0;
                            let _ = self
                                .tx_event
                                .send(Event::ApprovalRequired {
                                    id: tool_id.clone(),
                                    tool_name: tool_name.clone(),
                                    input: tool_input.clone(),
                                    description: plan.approval_description.clone(),
                                    approval_key,
                                    approval_grouping_key,
                                    intent_summary: if plan.read_only {
                                        None
                                    } else {
                                        intent_summary.clone()
                                    },
                                    approval_force_prompt: plan.approval_force_prompt,
                                })
                                .await;

                            match self.await_tool_approval(&tool_id).await {
                                Ok(ApprovalResult::Approved) => {
                                    emit_tool_audit(json!({
                                        "event": "tool.approval_decision",
                                        "tool_id": tool_id.clone(),
                                        "tool_name": tool_name.clone(),
                                        "decision": "approved",
                                        "caller": caller_type_for_tool_use(tool_caller.as_ref()),
                                    }));
                                    (None, None)
                                }
                                Ok(ApprovalResult::Denied) => {
                                    emit_tool_audit(json!({
                                        "event": "tool.approval_decision",
                                        "tool_id": tool_id.clone(),
                                        "tool_name": tool_name.clone(),
                                        "decision": "denied",
                                        "caller": caller_type_for_tool_use(tool_caller.as_ref()),
                                    }));
                                    (
                                        Some(Err(ToolError::permission_denied(format!(
                                            "Tool '{tool_name}' denied by user"
                                        )))),
                                        None,
                                    )
                                }
                                Ok(ApprovalResult::RetryWithPolicy(policy)) => {
                                    emit_tool_audit(json!({
                                        "event": "tool.approval_decision",
                                        "tool_id": tool_id.clone(),
                                        "tool_name": tool_name.clone(),
                                        "decision": "retry_with_policy",
                                        "policy": format!("{policy:?}"),
                                        "caller": caller_type_for_tool_use(tool_caller.as_ref()),
                                    }));
                                    let elevated_context = tool_registry.map(|r| {
                                        r.context().clone().with_elevated_sandbox_policy(policy)
                                    });
                                    (None, elevated_context)
                                }
                                Err(err) => (Some(Err(err)), None),
                            }
                        } else {
                            (None, None)
                        };

                        // Per-tool snapshot for surgical undo (#384): capture workspace
                        // state before file-modifying tools execute so `/undo` can
                        // revert the most recent write_file/edit_file/apply_patch.
                        // See `should_pre_tool_snapshot` for the gating rationale (#3292).
                        if should_pre_tool_snapshot(
                            self.config.snapshots_enabled,
                            result_override.is_some(),
                            tool_name.as_str(),
                        ) {
                            let ws = self.session.workspace.clone();
                            let tid = tool_id.clone();
                            let cap = self.config.snapshots_max_workspace_bytes;
                            let _ = tokio::task::spawn_blocking(move || {
                                crate::core::turn::pre_tool_snapshot(&ws, &tid, cap)
                            })
                            .await;
                        }

                        let started_at = Instant::now();
                        let mut result = if let Some(result_override) = result_override {
                            result_override
                        } else {
                            Self::execute_tool_with_lock(
                                tool_exec_lock.clone(),
                                plan.supports_parallel,
                                plan.interactive,
                                self.tx_event.clone(),
                                tool_name.clone(),
                                tool_input.clone(),
                                self.session.workspace.clone(),
                                tool_registry,
                                mcp_pool.clone(),
                                context_override,
                            )
                            .await
                        };

                        // Aggregate this tool call's latency into the turn metrics
                        // (#734): count + wall-clock duration, so diagnostics can
                        // surface a real latency total/average rather than a boolean.
                        turn.record_tool_call_timed(started_at.elapsed());

                        // #871 — capture the tool-selection decision so the
                        // decision trail survives compaction. `tool_name` renders
                        // to its canonical string form.
                        self.record_decision(
                            crate::compaction::decision_log::Kind::ToolChosen,
                            format!("chose tool `{tool_name}`"),
                        );

                        // #500: spill outsized tool outputs to disk before the
                        // result fans out to the model context and the UI cell.
                        // Both consumers see the same artifact reference block +
                        // metadata pointing at the session-owned full file.
                        // Emit a discrete `tool.spillover` audit event so
                        // operators can correlate large-output episodes with
                        // disk-usage growth in `~/.mimofan/tool_outputs/`.
                        if let Ok(tool_result) = result.as_mut()
                            && let Some(path) =
                                crate::tools::truncate::apply_spillover_with_artifact(
                                    tool_result,
                                    &tool_id,
                                    &tool_name,
                                    &self.session.id,
                                )
                        {
                            emit_tool_audit(json!({
                                "event": "tool.spillover",
                                "tool_id": tool_id.clone(),
                                "tool_name": tool_name.clone(),
                                "path": path.display().to_string(),
                            }));
                        }

                        let _ = self
                            .tx_event
                            .send(Event::ToolCallComplete {
                                id: tool_id.clone(),
                                name: tool_name.clone(),
                                result: result.clone(),
                            })
                            .await;

                        outcomes[plan.index] = Some(ToolExecOutcome {
                            index: plan.index,
                            id: tool_id,
                            name: tool_name,
                            input: tool_input,
                            started_at,
                            result,
                        });
                    }
                }
            }

            let mut step_error_count = 0usize;
            // Categorized tool errors collected this step. Feeds the capacity
            // controller's error-escalation checkpoint so it can distinguish
            // (e.g.) a Tool failure that should escalate from a permission
            // denial that should not.
            let mut step_error_categories: Vec<ErrorCategory> = Vec::new();
            let mut stop_after_plan_tool = false;
            // Tool-use self-heal: ensures we append the recovery hint at most
            // once per tool call per step, preventing an infinite append loop.
            let mut self_healed_ids: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            for outcome in outcomes.into_iter().flatten() {
                let tool_input = outcome.input.clone();
                let tool_name_for_ws = outcome.name.clone();
                let should_stop_this_turn =
                    should_stop_after_plan_tool(mode, &outcome.name, &outcome.result);
                // W3 (#836): OR the existing stop decision with any
                // interceptor that wants to force a stop. The base function's
                // own logic is left untouched; we only widen the decision here.
                let should_stop_this_turn = should_stop_this_turn
                    || self
                        .interceptors
                        .iter()
                        .any(|ic| ic.turn_stopping(self.turn_counter) == Some(true));

                match outcome.result {
                    Ok(output) => {
                        emit_tool_audit(json!({
                            "event": "tool.result",
                            "tool_id": outcome.id.clone(),
                            "tool_name": outcome.name.clone(),
                            "success": output.success,
                        }));
                        let output_for_context = compact_tool_result_for_context(
                            &self.session.model,
                            &outcome.name,
                            &output,
                        );
                        let tool_was_executed = output
                            .metadata
                            .as_ref()
                            .and_then(|metadata| metadata.get("executed"))
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true);
                        // Feed the goal anti-drift breaker: a successful
                        // file-write counts as forward progress.
                        if output.success
                            && tool_was_executed
                            && Self::is_file_write_tool(&outcome.name)
                        {
                            turn_had_write = true;
                        }
                        // Loop guard: a successful write is unambiguous forward
                        // progress, which resets the repeat/stall counters so a
                        // legitimate edit-heavy loop is never flagged. A tool
                        // that did not execute (blocked/denied) contributes no
                        // progress signal.
                        if let Some(loop_break) =
                            loop_guard
                                .lock()
                                .await
                                .observe(&crate::loop_guard::ToolObservation {
                                    name: &outcome.name,
                                    args: &tool_input,
                                    success: output.success,
                                    output: &output_for_context,
                                    progress: output.success
                                        && tool_was_executed
                                        && Self::is_file_write_tool(&outcome.name),
                                })
                        {
                            tracing::warn!(
                                target: "engine.loop_guard",
                                pattern = loop_break.pattern.as_str(),
                                occurrences = loop_break.occurrences,
                                tools = ?loop_break.tools,
                                "in-turn loop detected; injecting self-correction hint"
                            );
                            pending_loop_nudges.push(loop_break.nudge);
                        }
                        // Self-heal: a tool that executed but reported
                        // `success: false` (e.g. non-zero command exit, missing
                        // file) gets a recovery hint so the model retries within
                        // the same step. Capped at one hint per tool call.
                        let mut output_for_context = output_for_context;
                        if !output.success
                            && tool_was_executed
                            && self_healed_ids.insert(outcome.id.clone())
                        {
                            output_for_context.push_str("\n\n");
                            output_for_context.push_str(&self_heal_hint(&outcome.name, "tool"));
                        }
                        self.session.working_set.observe_tool_call(
                            &tool_name_for_ws,
                            &tool_input,
                            Some(&output_for_context),
                            &self.session.workspace,
                        );

                        // #136: post-edit LSP diagnostics hook. We only run
                        // this on success — failed edits leave the file
                        // untouched, so polling for diagnostics would just
                        // surface stale state.
                        if output.success && tool_was_executed {
                            self.run_post_edit_lsp_hook(&outcome.name, &tool_input)
                                .await;
                        }

                        // #3026: pipe `additionalContext` from tool_call_before
                        // hooks back to the model alongside the tool result.
                        let output_for_context = match hook_contexts.get(&outcome.id) {
                            Some(context) => {
                                format!("{output_for_context}\n\n[hook context] {context}")
                            }
                            None => output_for_context,
                        };

                        self.add_session_message(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::ToolResult {
                                tool_use_id: outcome.id,
                                content: output_for_context,
                                is_error: None,
                                content_blocks: None,
                            }],
                        })
                        .await;
                    }
                    Err(e) => {
                        let envelope: ErrorEnvelope = e.clone().into();
                        emit_tool_audit(json!({
                            "event": "tool.result",
                            "tool_id": outcome.id.clone(),
                            "tool_name": outcome.name.clone(),
                            "success": false,
                            "error": e.to_string(),
                            "category": envelope.category.to_string(),
                            "severity": envelope.severity.to_string(),
                        }));
                        step_error_count += 1;
                        step_error_categories.push(envelope.category);
                        let error = format_tool_error(&e, &outcome.name);
                        last_tool_error_text = error.clone();
                        // Loop guard: a hard error is never progress. The error
                        // text feeds the outcome digest, so a retry that fails
                        // *differently* still counts as converging and will not
                        // trip the stall detector.
                        if let Some(loop_break) =
                            loop_guard
                                .lock()
                                .await
                                .observe(&crate::loop_guard::ToolObservation {
                                    name: &outcome.name,
                                    args: &tool_input,
                                    success: false,
                                    output: &error,
                                    progress: false,
                                })
                        {
                            tracing::warn!(
                                target: "engine.loop_guard",
                                pattern = loop_break.pattern.as_str(),
                                occurrences = loop_break.occurrences,
                                tools = ?loop_break.tools,
                                "in-turn loop detected; injecting self-correction hint"
                            );
                            pending_loop_nudges.push(loop_break.nudge);
                        }
                        self.session.working_set.observe_tool_call(
                            &tool_name_for_ws,
                            &tool_input,
                            Some(&error),
                            &self.session.workspace,
                        );
                        // Self-heal: append a recovery hint for recoverable
                        // errors (e.g. execution failures, timeouts, invalid
                        // input) so the model can retry within the same step.
                        // Permission/availability errors are not recoverable by
                        // retrying, so they are left as-is. Capped at one hint
                        // per tool call via `self_healed_ids`.
                        let mut error_content = format!("Error: {error}");
                        if envelope.recoverable && self_healed_ids.insert(outcome.id.clone()) {
                            error_content.push_str("\n\n");
                            error_content.push_str(&self_heal_hint(
                                &outcome.name,
                                &envelope.category.to_string(),
                            ));
                        }
                        self.add_session_message(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::ToolResult {
                                tool_use_id: outcome.id,
                                content: error_content,
                                is_error: Some(true),
                                content_blocks: None,
                            }],
                        })
                        .await;
                    }
                }

                stop_after_plan_tool |= should_stop_this_turn;
            }

            // W3 (#836): notify interceptors a step has been processed.
            for ic in &self.interceptors {
                ic.post_step(self.turn_counter);
            }

            if stop_after_plan_tool {
                break;
            }

            if !pending_steers.is_empty() {
                for steer in pending_steers.drain(..) {
                    self.session
                        .working_set
                        .observe_user_message(&steer, &self.session.workspace);
                    self.add_session_message(self.user_text_message_with_turn_metadata(steer))
                        .await;
                }
            }

            // Loop guard intervention: bounded, advisory, non-fatal. Injected
            // after the tool results so the model reads the evidence first,
            // then the diagnosis. The turn continues either way — the model
            // gets a chance to self-correct, and `max_steps` remains the only
            // hard stop.
            for nudge in pending_loop_nudges.drain(..) {
                let _ = self
                    .tx_event
                    .send(Event::status(
                        "Loop detected — nudging the model to re-plan",
                    ))
                    .await;
                self.add_session_message(self.user_text_message_with_turn_metadata(nudge))
                    .await;
            }

            if step_error_count > 0 {
                consecutive_tool_error_steps = consecutive_tool_error_steps.saturating_add(1);
            } else {
                consecutive_tool_error_steps = 0;
            }
            // After 3+ consecutive tool errors, inject a system hint to help
            // the model break out of a potential error loop.
            if consecutive_tool_error_steps >= 3 {
                let hint = format!(
                    "[System hint] The last {consecutive_tool_error_steps} tool calls all failed. \
                     Consider taking a different approach, checking your assumptions, \
                     or asking the user for guidance rather than retrying the same strategy."
                );
                self.add_session_message(self.user_text_message_with_turn_metadata(hint))
                    .await;
                consecutive_tool_error_steps = 0;
            }

            // Anti-drift + automatic memory capture at the end of each step.
            // Runs inside the synchronous post-step region (no `.await` in the
            // goal-state lock guard) to respect the std::Mutex red line.
            self.record_goal_progress_signal(
                turn_had_write,
                step_error_count,
                &last_tool_error_text,
            );
            self.auto_capture_memory(&mut seen_auto_memory).await;

            turn.next_step();
        }

        if self.cancel_token.is_cancelled() {
            self.persist_loop_guard_state(&self.session.id, &loop_guard);
            return (TurnOutcomeStatus::Interrupted, None);
        }
        if let Some(err) = turn_error {
            self.persist_loop_guard_state(&self.session.id, &loop_guard);
            return (TurnOutcomeStatus::Failed, Some(err));
        }
        self.persist_loop_guard_state(&self.session.id, &loop_guard);
        (TurnOutcomeStatus::Completed, None)
    }

    /// Whether a tool name denotes a file-writing operation (counts as forward
    /// progress for the goal NoProgress circuit breaker).
    fn is_file_write_tool(name: &str) -> bool {
        matches!(
            name,
            "Write"
                | "Edit"
                | "MultiEdit"
                | "NotebookEdit"
                | "write"
                | "edit"
                | "multi_edit"
                | "notebook_edit"
        )
    }

    /// Load the cross-turn loop-guard state for `session_id` and return a
    /// `SharedLoopGuard` (Arc<Mutex<LoopGuard>>). When a persisted
    /// `loop_guard_state` exists it is restored into the guard so loop
    /// suspicion accumulated in previous turns (and process runs) continues;
    /// otherwise a fresh guard is returned.
    fn load_shared_loop_guard(&self, session_id: &str) -> crate::loop_guard::SharedLoopGuard {
        use crate::loop_guard::LoopGuard;
        let mut guard = LoopGuard::default();
        if let Ok(manager) = crate::session_manager::SessionManager::default_location()
            && let Ok(Some(state)) = manager.load_loop_guard_state(session_id)
        {
            guard.restore_state(&state);
            tracing::debug!(
                target: "engine.loop_guard",
                observed = state.observed,
                "restored persisted loop_guard_state across turns"
            );
        }
        std::sync::Arc::new(tokio::sync::Mutex::new(guard))
    }

    /// Persist the durable portion of the session's `SharedLoopGuard` to disk
    /// as `loop_guard_state`, so the loop suspicion survives process restarts
    /// and continues into the next turn.
    async fn persist_loop_guard_state(
        &self,
        session_id: &str,
        guard: &crate::loop_guard::SharedLoopGuard,
    ) {
        use crate::loop_guard::LoopGuardState;
        let state: LoopGuardState = guard.lock().await.snapshot_state();
        if let Ok(manager) = crate::session_manager::SessionManager::default_location()
            && let Err(err) = manager.save_loop_guard_state(session_id, &state)
        {
            tracing::warn!(
                target: "engine.loop_guard",
                ?err,
                "failed to persist loop_guard_state for session {session_id}"
            );
        }
    }

    /// Feed a per-step progress signal into the active goal's circuit breakers.
    ///
    /// Must be called synchronously — the `SharedGoalQueue` guard is never held
    /// across an `.await` (see `ARCHITECTURE_STABILITY.md` §8.3).
    fn record_goal_progress_signal(
        &self,
        turn_had_write: bool,
        step_error_count: usize,
        last_tool_error_text: &str,
    ) {
        use crate::tools::goal::{ProgressSignal, SharedGoalQueue};
        let state: &SharedGoalQueue = &self.config.goal_queue;
        let signal = if step_error_count > 0 {
            // Bound the fingerprint so the breaker keys on the error class,
            // not unbounded text growth.
            let fingerprint = last_tool_error_text
                .trim()
                .chars()
                .take(120)
                .collect::<String>();
            ProgressSignal::ToolError { fingerprint }
        } else if turn_had_write {
            ProgressSignal::FileChanged
        } else {
            ProgressSignal::NoChange
        };
        match state.lock() {
            Ok(mut goal) => goal.record_progress_signal(&signal),
            Err(err) => tracing::warn!("goal state lock poisoned while recording progress: {err}"),
        }
    }

    /// Lightweight automatic memory capture at the end of each step.
    ///
    /// Extracts durable signals from the turn transcript (no extra LLM call)
    /// and persists them when a memory backend is enabled:
    /// - file memory (`[memory] enabled`) → append to the category file;
    /// - vector memory (`MIMOFAN_MEMORY_API_KEY` set) → embed + store observation.
    ///
    /// Honors the std::Mutex / Send red lines: file writes are fully
    /// synchronous; the embedding future is awaited only after extracting the
    /// `Send` embedder, and the non-`Send` `VectorMemory` is never held across
    /// an `.await`.
    async fn auto_capture_memory(&mut self, seen: &mut std::collections::HashSet<String>) {
        let signals = crate::turn_memory::extract_signals(&self.session.messages);
        if signals.is_empty() {
            return;
        }

        // File memory (opt-in).
        if self.config.memory_enabled {
            for signal in &signals {
                let key = format!("{}/{}", signal.category.as_str(), signal.content);
                if !seen.insert(key) {
                    continue; // de-dupe within this turn
                }
                if let Err(err) = crate::memory::append_entry(
                    &self.config.memory_dir,
                    signal.category.as_str(),
                    &signal.content,
                ) {
                    tracing::warn!(?err, "auto memory: failed to append entry");
                }
            }
        }

        // Vector memory (configured).
        if crate::vector_memory::VectorMemory::is_configured() {
            let mem_dir = self.config.memory_dir.clone();
            let project = self.config.workspace.display().to_string();
            let to_store: Vec<(String, String)> = signals
                .iter()
                .filter(|s| seen.insert(format!("vec/{}/{}", s.category.as_str(), s.content)))
                .map(|s| (s.category.as_str().to_string(), s.content.clone()))
                .collect();
            if !to_store.is_empty() {
                // Open is cheap and synchronous; the store itself is non-Send,
                // so we take the Send embedder, await the embedding, then store
                // synchronously.
                match crate::vector_memory::VectorMemory::open(&mem_dir) {
                    Ok(mut vm) => {
                        if let Some(embedder) = vm.take_embedder() {
                            for (kind, content) in to_store {
                                match embedder.embed_text(&content).await {
                                    Ok(embedding) => {
                                        if let Err(err) = vm.store_observation(
                                            &project,
                                            &kind,
                                            &content,
                                            &self.session.id,
                                            &embedding,
                                        ) {
                                            tracing::warn!(
                                                ?err,
                                                "auto memory: vector store failed"
                                            );
                                        }
                                    }
                                    Err(err) => {
                                        tracing::warn!(?err, "auto memory: embedding failed");
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => tracing::warn!(?err, "auto memory: vector open failed"),
                }
            }
        }

        // #659 经验学习闭环: distill the session transcript into UserProfile
        // corrections and persist them so stable preferences/constraints survive
        // across sessions. We only act when a profile already exists (the model
        // or a prior session seeded one) to avoid writing on every no-signal
        // turn; new profiles are still created when a genuine correction appears.
        let transcript: Vec<String> = self
            .session
            .messages
            .iter()
            .flat_map(|m| {
                m.content
                    .iter()
                    .filter_map(|b| match b {
                        crate::models::ContentBlock::Text { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        if !transcript.is_empty() {
            let candidates = crate::memory::distill_session(&transcript);
            if !candidates.is_empty() {
                let mut profile = self
                    .user_profile
                    .clone()
                    .unwrap_or_else(crate::memory::UserProfile::empty);
                for (bucket, entry) in candidates {
                    profile.apply_correction(bucket, entry);
                }
                if let Some(path) = crate::memory::UserProfile::default_path() {
                    if let Err(err) = profile.save(&path) {
                        tracing::warn!(?err, "auto memory: failed to save user profile");
                    } else {
                        self.user_profile = profile.into_non_empty();
                    }
                }
            }
        }
    }

    async fn goal_continuation_message_if_needed(
        &self,
        tool_registry: Option<&crate::tools::ToolRegistry>,
        continuations_this_turn: &mut u32,
    ) -> Option<String> {
        let registry = tool_registry?;
        if !registry.contains("goal_update") {
            return None;
        }

        let mut snapshot = match self.config.goal_queue.lock() {
            Ok(state) => state.active_snapshot(),
            Err(err) => {
                tracing::warn!("goal queue lock poisoned during continuation check: {err}");
                return None;
            }
        };

        // 无活动 goal（全部完成/阻塞/取消）→ 尝试提升下一个就绪 goal。
        let mut snapshot = match snapshot {
            Some(snap) => snap,
            None => {
                self.config.goal_queue.lock().ok()?.promote_next_ready()?;
                match self.config.goal_queue.lock() {
                    Ok(state) => state.active_snapshot()?,
                    Err(_) => return None,
                }
            }
        };

        if !snapshot.is_active() {
            return None;
        }

        let per_turn_max = crate::tools::goal::MAX_GOAL_CONTINUATIONS_PER_TURN;
        if *continuations_this_turn >= per_turn_max {
            let _ = self
                .tx_event
                .send(Event::status(format!(
                    "Goal remains active after {per_turn_max} continuation pass(es) this turn; ending turn to avoid a runaway loop."
                )))
                .await;
            return None;
        }

        // Route the continuation decision through the goal-loop decision core.
        // There is no run-level cap — a goal runs until complete/blocked,
        // paused, or an optional token/time budget is exhausted. The per-turn
        // guard (`per_turn_max`) only bounds how many continuation passes
        // happen *within* a single turn before yielding back to the engine.
        let decision = crate::goal_loop::decide_continuation(
            crate::goal_loop::GoalRunStatus::Active,
            crate::goal_loop::GoalProgress {
                tokens_used: snapshot.tokens_used,
                time_used_seconds: snapshot.time_used_seconds,
                continuations: snapshot.continuation_count,
                no_progress_rounds: snapshot.no_progress_rounds,
                repeated_error_rounds: snapshot.repeated_error_rounds,
            },
            crate::goal_loop::GoalBudget {
                token_budget: snapshot.token_budget.map(u64::from),
                time_budget_seconds: snapshot.time_budget_seconds,
                // `/loop --max N` overrides the default safety cap.
                max_continuations: snapshot
                    .max_rounds
                    .or(Some(crate::goal_loop::DEFAULT_MAX_CONTINUATIONS)),
                no_progress_rounds: None,
                repeated_error_rounds: None,
            }
            .with_guardrails(
                Some(crate::tools::goal::DEFAULT_NO_PROGRESS_ROUNDS),
                Some(crate::tools::goal::DEFAULT_REPEATED_ERROR_ROUNDS),
            ),
        );
        if let crate::goal_loop::ContinuationDecision::Stop(reason) = decision {
            let message = match reason {
                crate::goal_loop::StopReason::TokenBudget => format!(
                    "Goal token budget reached ({} / {} tokens); ending continuation.",
                    snapshot.tokens_used,
                    snapshot.token_budget.unwrap_or_default()
                ),
                crate::goal_loop::StopReason::NoProgress => format!(
                    "Goal stopped by anti-drift circuit breaker: {} consecutive turns made no file changes. The objective may be stuck or complete; review and resume if needed.",
                    snapshot.no_progress_rounds
                ),
                crate::goal_loop::StopReason::RepeatedError => format!(
                    "Goal stopped by anti-drift circuit breaker: the same tool error repeated {} consecutive turns. Fix the root cause before resuming.",
                    snapshot.repeated_error_rounds
                ),
                crate::goal_loop::StopReason::ContinuationLimit => format!(
                    "Goal continuation limit reached ({} continuations); ending to prevent unbounded loop.",
                    snapshot.continuation_count,
                ),
                other => format!("Goal continuation stopped: {other:?}."),
            };
            let _ = self.tx_event.send(Event::status(message)).await;
            return None;
        }

        *continuations_this_turn = (*continuations_this_turn).saturating_add(1);
        match self.config.goal_queue.lock() {
            Ok(mut state) => {
                state.record_continuation();
                if let Some(snap) = state.active_snapshot() {
                    snapshot = snap;
                }
            }
            Err(err) => {
                tracing::warn!("goal queue lock poisoned while recording continuation: {err}")
            }
        }

        // `/loop --checkpoint`: snapshot the workspace before this continuation
        // round so the user can `/rewind` to a specific loop iteration.
        if snapshot.checkpoint_each_round {
            let ck_workspace = self.session.workspace.clone();
            let ck_cap = self.config.snapshots_max_workspace_bytes;
            let ck_round = snapshot.continuation_count;
            let ck_conv = self.session.messages.len();
            let _ = tokio::task::spawn_blocking(move || {
                crate::core::turn::loop_round_snapshot(&ck_workspace, ck_round, ck_cap, ck_conv)
            })
            .await;
        }

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "Continuing active goal ({}/{per_turn_max} this turn, {} total)",
                *continuations_this_turn, snapshot.continuation_count
            )))
            .await;

        Some(crate::tools::goal::render_continuation_prompt(
            &snapshot,
            snapshot.continuation_count,
            snapshot.stop_condition.as_deref(),
        ))
    }

    pub(super) fn messages_with_turn_metadata(&self) -> Vec<Message> {
        self.session.messages.clone().into()
    }
}

pub(super) fn subagent_completion_runtime_text(payload: &str) -> String {
    format!(
        "<mimo:runtime_event kind=\"subagent_completion\" visibility=\"internal\">\n\
This is an internal runtime event, not user input. Use the sub-agent completion \
data below to continue coordinating the current task. Do not tell the user they \
pasted sentinels, do not explain the sentinel protocol, and do not quote the raw \
XML unless the user explicitly asks to debug sub-agent internals.\n\n\
{payload}\n\
</mimo:runtime_event>"
    )
}

fn subagent_completion_runtime_message(payload: &str) -> Message {
    // Role is "user", not "system": some OpenAI-compatible backends apply a
    // strict chat template (e.g. vLLM serving Qwen3) that requires any system
    // message to be messages[0]. A system message appended mid-conversation
    // makes the template raise "System message must be at the beginning",
    // which surfaces as a 400 BadRequest and breaks the whole sub-agent
    // hand-off in the parent turn. The `visibility="internal"` tag already
    // tells the model this is a runtime event rather than user input, so the
    // role carries no semantic weight here — only template-compatibility cost.
    Message {
        role: "user".to_string(),
        content: vec![
            ContentBlock::Text {
                text: subagent_completion_runtime_text(payload),
                cache_control: None,
            },
            runtime_event_turn_metadata_block(UserInputProvenance::SubAgentHandoff),
        ],
    }
}

fn runtime_event_turn_metadata_block(provenance: UserInputProvenance) -> ContentBlock {
    ContentBlock::Text {
        text: format!(
            "<turn_meta>\nInput provenance: {}\nInput authority: non_authoritative\n</turn_meta>",
            provenance.as_str()
        ),
        cache_control: None,
    }
}

fn shell_completion_status_text(
    events: &[crate::tools::shell::ShellCompletionEvent],
    timing: &str,
) -> Option<String> {
    if events.is_empty() {
        return None;
    }

    let count = events.len();
    let failed = events
        .iter()
        .filter(|event| event.status != crate::tools::shell::ShellStatus::Completed)
        .count();
    let noun = if count == 1 { "job" } else { "jobs" };
    let prefix = if timing.trim().is_empty() {
        String::new()
    } else {
        format!("{} ", timing.trim())
    };
    let mut status = if failed == 0 {
        format!("{prefix}{count} background shell {noun} completed")
    } else {
        format!("{prefix}{count} background shell {noun} finished ({failed} failed)")
    };

    if count == 1
        && let Some(event) = events.first()
    {
        let command = truncate_runtime_status_field(&event.command, 80);
        status.push_str(&format!(": {command}"));
        if let Some(owner) = event
            .owner_agent_name
            .as_deref()
            .or(event.owner_agent_id.as_deref())
            .filter(|owner| !owner.trim().is_empty())
        {
            status.push_str(&format!(" (by {owner})"));
        }
    }

    Some(status)
}

/// Build a `<task-notification>` runtime event for finished background shell
/// jobs, mirroring the sub-agent completion hand-off convention. This fulfils
/// issue #696: a background command's completion must be injected into the
/// conversation (not only shown as a transient status line) so the model can
/// react to the result during idle time. Returns `None` when there is nothing
/// to report.
fn shell_completion_notification_text(
    events: &[crate::tools::shell::ShellCompletionEvent],
) -> Option<String> {
    if events.is_empty() {
        return None;
    }

    let mut body = String::new();
    for event in events {
        let status_word = match event.status {
            crate::tools::shell::ShellStatus::Completed => "completed",
            _ => "failed",
        };
        let exit = event
            .exit_code
            .map(|code| format!("exit={code}"))
            .unwrap_or_else(|| "exit=<unknown>".to_string());
        body.push_str(&format!(
            "- task {} ({}) {} [{}]: {}\n",
            event.task_id,
            status_word,
            exit,
            truncate_runtime_status_field(&event.command, 120),
            truncate_runtime_status_field(
                if event.stderr_tail.is_empty() {
                    &event.stdout_tail
                } else {
                    &event.stderr_tail
                },
                200
            ),
        ));
    }

    Some(format!(
        "<mimo:runtime_event kind=\"task_notification\" visibility=\"internal\">\n\
This is an internal runtime event, not user input. It reports that one or more \
background shell jobs you started have finished. Use the result below to decide \
next steps (e.g. read artifacts, run follow-ups, or report to the user). Do not \
tell the user they pasted sentinels, do not explain the sentinel protocol, and \
do not quote the raw XML unless the user explicitly asks to debug internals.\n\n\
{body}</mimo:runtime_event>"
    ))
}

fn truncate_runtime_status_field(text: &str, max_chars: usize) -> String {
    let normalized = text.replace(['\n', '\r'], " ");
    let mut chars = normalized.chars();
    let mut out = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

fn should_hold_turn_for_subagents(queued_completions: usize, running_children: usize) -> bool {
    // #3216: launching sub-agents must NOT barrier the parent turn. Only queued
    // completions (work already finished that must be surfaced into the
    // transcript) hold the turn open. Running children are background work — the
    // parent ends its turn and their results arrive via the completion sentinel
    // on a later turn. The
    // `running_children` argument is kept for call-site clarity and the
    // background-status message, but deliberately no longer gates the hold.
    let _ = running_children;
    queued_completions > 0
}

fn stream_chunk_timeout_budget(config: &EngineConfig) -> (u64, Duration) {
    let secs = config.stream_chunk_timeout.as_secs();
    (secs, Duration::from_secs(secs))
}

/// Whether a per-tool pre-execution snapshot should be taken before running
/// `tool_name` (#384).
///
/// Gated on `snapshots.enabled` (#3292) so that disabling snapshots suppresses
/// the per-tool `tool:<call_id>` commits, matching the pre/post-turn snapshot
/// call sites which already honor the same flag. A tool whose result is already
/// overridden (denied, hook-supplied, or otherwise short-circuited) never
/// executes a file write, so it is skipped too. Only the file-modifying tools
/// produce undoable workspace changes worth snapshotting.
fn should_pre_tool_snapshot(
    snapshots_enabled: bool,
    has_result_override: bool,
    tool_name: &str,
) -> bool {
    snapshots_enabled
        && !has_result_override
        && matches!(tool_name, "write_file" | "edit_file" | "apply_patch")
}

/// Synthesize the tool result recorded for a tool call that never executed
/// because the turn was cancelled mid-batch (#3216 / #2211).
///
/// Esc/Ctrl+C cancels the shared cancellation token out-of-band (see
/// `EngineHandle::cancel_with_reason`), so the `for batch in batches` loop can
/// observe the cancellation between batches and stop launching further tools —
/// turning a wedged "six sub-agents, ~24s, can't cancel" turn into a prompt
/// interrupt. We still record a result for every un-run `tool_use` so each
/// keeps a matching `tool_result` and the transcript stays well-formed on
/// resume. It is an `Ok(ToolResult { success: false })` rather than an `Err`
/// so it routes through the benign outcome branch and does not inflate the
/// step's error counters or trip error-escalation.
fn interrupted_tool_result() -> ToolResult {
    ToolResult::error("Tool not executed: the request was cancelled before this tool ran.")
}

pub(super) fn command_allows_tool(allowed_tools: Option<&[String]>, tool_name: &str) -> bool {
    let Some(allowed_tools) = allowed_tools else {
        return true;
    };
    allowed_tools.contains(&tool_name.to_ascii_lowercase())
}

/// Folded outcome of all `tool_call_before` hook results for one tool call
/// (#3026). Precedence: deny (exit code 2 or JSON) > ask > allow;
/// `updatedInput` is last-writer-wins; `additionalContext` is concatenated.
#[derive(Debug, Default, PartialEq)]
struct ToolCallHookFold {
    /// Denial reason from an exit-code-2 hook or a JSON `deny` decision.
    deny_reason: Option<String>,
    /// At least one hook returned a JSON `ask` decision.
    requires_approval: bool,
    /// Replacement tool input from the last hook that supplied one.
    updated_input: Option<serde_json::Value>,
    /// Concatenated `additionalContext` strings from all hooks.
    additional_context: Option<String>,
}

fn fold_tool_call_before_results(results: &[crate::hooks::HookResult]) -> ToolCallHookFold {
    let mut fold = ToolCallHookFold::default();

    // Legacy hard deny: exit code 2 wins regardless of stdout (backwards
    // compatible with pre-#3026 hooks).
    if let Some(denial) = results.iter().find(|result| result.exit_code == Some(2)) {
        let reason = denial
            .stdout
            .trim()
            .lines()
            .next()
            .filter(|line| !line.is_empty())
            .or_else(|| {
                denial
                    .stderr
                    .trim()
                    .lines()
                    .next()
                    .filter(|line| !line.is_empty())
            })
            .or(denial.error.as_deref())
            .unwrap_or("ToolCallBefore hook denied tool execution");
        fold.deny_reason = Some(reason.to_string());
        return fold;
    }

    for result in results {
        // Background hooks return immediately with no process result and
        // cannot steer (the caller warns about that configuration).
        if result.exit_code.is_none() {
            continue;
        }
        let parsed = crate::hooks::parse_tool_call_before_stdout(&result.stdout);
        match parsed.decision {
            Some(crate::hooks::ToolCallDecision::Deny) => {
                fold.deny_reason =
                    Some(parsed.reason.unwrap_or_else(|| {
                        "ToolCallBefore hook denied tool execution".to_string()
                    }));
                return fold;
            }
            Some(crate::hooks::ToolCallDecision::Ask) => fold.requires_approval = true,
            Some(crate::hooks::ToolCallDecision::Allow) | None => {}
        }
        if let Some(updated) = parsed.updated_input {
            fold.updated_input = Some(updated);
        }
        if let Some(context) = parsed.additional_context {
            match &mut fold.additional_context {
                Some(existing) => {
                    existing.push('\n');
                    existing.push_str(&context);
                }
                None => fold.additional_context = Some(context),
            }
        }
    }
    fold
}

/// Check whether `tool_name` is explicitly denied (#3027).
/// Deny always wins over allow.
pub(super) fn command_denies_tool(disallowed_tools: Option<&[String]>, tool_name: &str) -> bool {
    let Some(disallowed_tools) = disallowed_tools else {
        return false;
    };
    disallowed_tools.contains(&tool_name.to_ascii_lowercase())
}

fn resolve_tool_definition<'a>(
    tool_name: &mut String,
    tool_catalog: &'a [Tool],
    tool_registry: Option<&crate::tools::ToolRegistry>,
) -> Option<&'a Tool> {
    let mut tool_def = tool_catalog
        .iter()
        .find(|def| def.name.as_str() == tool_name.as_str());

    // Resolve hallucinated tool names before policy gates run, so aliases like
    // ReadFile are checked against the canonical registered tool name.
    if tool_def.is_none()
        && let Some(registry) = tool_registry
        && let Some(canonical) = registry.resolve(tool_name.as_str())
    {
        crate::logging::info(format!(
            "Resolved hallucinated tool name '{tool_name}' -> '{canonical}'"
        ));
        tool_def = tool_catalog.iter().find(|d| d.name == canonical);
        if tool_def.is_some() {
            *tool_name = canonical.to_string();
        }
    }

    tool_def
}

/// Issue #1727: decide whether to surface a "thinking-only, no output" status.
///
/// Reached when the assistant turn had no sendable content (no Text, no
/// ToolUse — only a reasoning/thinking block). We notify the user *only* when
/// the turn is genuinely finishing: no tool uses to dispatch, no `turn_error`
/// already surfaced for this turn, the request wasn't cancelled, AND the turn
/// is not about to CONTINUE — there are no pending steers and we are not
/// holding the turn open for running sub-agents. The status must fire at the
/// point the turn truly ends; emitting it earlier (at the persist site) would
/// show a spurious "turn ended" notice immediately before the turn resumed
/// for a steer or a sub-agent completion.
fn should_emit_thinking_only_status(
    tool_uses_empty: bool,
    turn_error_is_none: bool,
    cancelled: bool,
    steers_pending: bool,
    holding_for_subagents: bool,
) -> bool {
    tool_uses_empty && turn_error_is_none && !cancelled && !steers_pending && !holding_for_subagents
}

/// Resolve an `"auto"` reasoning-effort tier to a concrete value.
///
/// When the configured effort is `"auto"`, inspects the last user message
/// and calls [`crate::auto_reasoning::select`] to pick the actual tier.
/// Non-`"auto"` values pass through unchanged.
fn resolve_auto_effort(
    reasoning_effort: Option<&str>,
    messages: &[Message],
    provider: crate::config::ApiProvider,
) -> Option<String> {
    match reasoning_effort {
        Some("auto") => {
            // Find the last user message in the conversation.
            let last_msg = messages
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| {
                    m.content
                        .iter()
                        .filter_map(|block| {
                            if let ContentBlock::Text { text, .. } = block {
                                if is_turn_metadata_text(text) {
                                    None
                                } else {
                                    Some(text.as_str())
                                }
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<&str>>()
                        .join(" ")
                })
                .unwrap_or_default();

            // is_subagent is false here — handle_deepseek_turn runs in the
            // main engine (not a sub-agent's inner loop). Sub-agents have
            // their own turn pass and can pass is_subagent=true when they
            // call this function directly.
            let tier = crate::auto_reasoning::select(false, &last_msg);
            let resolved =
                crate::model_routing::normalize_auto_route_effort_for_provider(provider, tier)
                    .as_setting()
                    .to_string();
            tracing::debug!(
                reasoning_effort = %resolved,
                is_subagent = false,
                "auto_reasoning: resolved auto tier from user message"
            );
            Some(resolved)
        }
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

fn is_turn_metadata_text(text: &str) -> bool {
    text.trim_start().starts_with("<turn_meta>")
}

#[cfg(test)]
mod tests {
    use super::{self_heal_hint, shell_completion_notification_text};

    #[test]
    fn self_heal_hint_mentions_tool_name_and_category() {
        let hint = self_heal_hint("exec_shell", "tool");
        assert!(hint.contains("exec_shell"), "hint must name the tool");
        assert!(hint.contains("tool"), "hint must surface the category");
        assert!(hint.contains("[Self-heal]"), "hint must be tagged");
        assert!(
            hint.contains("do NOT blindly repeat"),
            "hint must warn against blind retry"
        );
    }

    #[test]
    fn self_heal_hint_distinct_per_category() {
        let a = self_heal_hint("read_file", "invalid_input");
        let b = self_heal_hint("read_file", "timeout");
        // Same tool, different category should still be valid hints; the
        // category string is embedded so they differ in content.
        assert_ne!(a, b);
        assert!(a.contains("invalid_input"));
        assert!(b.contains("timeout"));
    }

    #[test]
    fn shell_completion_notification_is_empty_when_no_events() {
        assert!(shell_completion_notification_text(&[]).is_none());
    }

    #[test]
    fn shell_completion_notification_reports_task_result() {
        use crate::tools::shell::{ShellCompletionEvent, ShellStatus};
        let event = ShellCompletionEvent {
            task_id: "shell_ab12cd34".to_string(),
            command: "cargo build".to_string(),
            status: ShellStatus::Completed,
            exit_code: Some(0),
            duration_ms: 1234,
            stdout_tail: "Compiling mimofan".to_string(),
            stderr_tail: String::new(),
            linked_task_id: None,
            owner_agent_id: None,
            owner_agent_name: None,
        };
        let text = shell_completion_notification_text(&[event]).expect("should produce text");
        assert!(
            text.contains("kind=\"task_notification\""),
            "must be a task-notification event"
        );
        assert!(text.contains("shell_ab12cd34"), "must name the task id");
        assert!(text.contains("exit=0"), "must report exit code");
        assert!(text.contains("cargo build"), "must name the command");
        assert!(
            text.contains("Compiling mimofan"),
            "must include output tail"
        );
    }

    #[test]
    fn shell_completion_notification_prefers_stderr_tail_on_failure() {
        use crate::tools::shell::{ShellCompletionEvent, ShellStatus};
        let event = ShellCompletionEvent {
            task_id: "shell_ff".to_string(),
            command: "false".to_string(),
            status: ShellStatus::Failed,
            exit_code: Some(1),
            duration_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: "boom".to_string(),
            linked_task_id: None,
            owner_agent_id: None,
            owner_agent_name: None,
        };
        let text = shell_completion_notification_text(&[event]).expect("should produce text");
        assert!(text.contains("failed"), "failed status must be labelled");
        assert!(
            text.contains("boom"),
            "stderr tail should be surfaced on failure"
        );
    }
}
