use super::*;

/// Maximum number of *whole-run* retries for a sub-agent when it dies with a
/// transient (non-deterministic) error. This is a best-effort safety net on top
/// of the per-step API retry inside `request_subagent_model_response_with_retries`:
/// if a run still fails fatally after those internal retries, we re-dispatch the
/// whole sub-agent a couple of times before giving up. Deterministic logic
/// errors (bad args, permission denied, truncation) are *never* retried.
const SUBAGENT_MAX_RETRIES: u32 = 2;

/// Best-effort backoff between whole-run retries. Deliberately tiny: transient
/// failures (rate limits, network blips) usually clear within milliseconds, and
/// we must not stall a long-horizon task behind a long sleep.
const SUBAGENT_RETRY_BACKOFF: Duration = Duration::from_millis(200);

/// Classify an error message as a transient / non-deterministic failure that is
/// worth retrying. Conservative keyword match only — we intentionally do NOT
/// retry truncation, permission denied, bad arguments, or other deterministic
/// logic errors, since retrying those would loop forever on the same failure.
#[must_use]
fn is_transient_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    [
        "timeout",
        "timed out",
        "rate limit",
        "429",
        "503",
        "502",
        "504",
        "network",
        "connection",
        "temporarily",
        "service unavailable",
        "bad gateway",
        "gateway timeout",
        "deadline has elapsed",
        "connection reset",
        "connection closed",
        "connection aborted",
        "did not receive response headers",
        "request timed out",
        "operation timed out",
    ]
    .iter()
    .any(|needle| m.contains(needle))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_subagent(
    runtime: &SubAgentRuntime,
    agent_id: String,
    agent_type: SubAgentType,
    prompt: String,
    assignment: SubAgentAssignment,
    allowed_tools: Option<Vec<String>>,
    fork_context: bool,
    fork_turns: Option<usize>,
    started_at: Instant,
    max_steps: u32,
    token_budget: Option<u64>,
    initial_input_rx: mpsc::UnboundedReceiver<SubAgentInput>,
    custom_agent_def: Option<custom_agents::CustomAgentDef>,
) -> Result<SubAgentResult> {
    let system_prompt =
        build_subagent_system_prompt(&agent_type, &assignment, custom_agent_def.as_ref());
    let fork_context_enabled = fork_context;
    let fork_context = fork_context_enabled
        .then_some(runtime.fork_context.as_ref())
        .flatten();
    let request_system = subagent_request_system_prompt(&system_prompt, fork_context);
    let mut messages = build_initial_subagent_messages(
        &prompt,
        &assignment,
        &agent_type,
        fork_context,
        custom_agent_def.as_ref(),
    );
    let (runtime_for_tools, mut child_completion_rx) = runtime_for_nested_agent_tools(
        runtime,
        &agent_id,
        SubAgentForkContext {
            system: Some(request_system.clone()),
            messages: messages.clone(),
            structured_state_block: None,
            fork_turns,
        },
    );
    let tool_registry = Arc::new(SubAgentToolRegistry::new_with_owner(
        runtime_for_tools,
        agent_type.clone(),
        agent_id.clone(),
        assignment
            .role
            .as_deref()
            .filter(|role| !role.trim().is_empty())
            .unwrap_or(agent_type.as_str())
            .to_string(),
        allowed_tools.clone(),
        // Share the parent's todo list so child checklist updates are visible
        // in the Work sidebar live. Previously each child got a fresh isolated
        // TodoList — parent never saw child progress until completion.
        runtime.todos.clone(),
        Arc::new(Mutex::new(PlanState::default())),
    ));
    let unavailable_tools = tool_registry.unavailable_allowed_tools();
    if !unavailable_tools.is_empty() {
        return Err(anyhow!(
            "Sub-agent requested unavailable tools: {}",
            unavailable_tools.join(", ")
        ));
    }
    let tools = tool_registry.tools_for_model(&agent_type);
    if let Some(mb) = runtime.mailbox.as_ref() {
        let _ = mb.send(MailboxMessage::started(&agent_id, agent_type.clone()));
    }
    record_agent_progress(
        runtime,
        &agent_id,
        format!("started ({})", agent_type.as_str()),
    );

    // Whole-run retry loop. Each iteration re-dispatches the *entire* sub-agent
    // run. A retry is only attempted when the run failed with a transient error
    // (see `is_transient_error`) and we have not exhausted `SUBAGENT_MAX_RETRIES`.
    // The per-step `max_steps` budget is untouched — a retry is a separate,
    // bounded compensation, not an extra step. Deterministic failures fall
    // straight through to `return Err`. The loop is always finite.
    let mut attempt: u32 = 0;
    // The original input receiver can only be consumed once (on the first
    // attempt). Wrap it so retries get a fresh empty receiver instead.
    let mut initial_input_rx = Some(initial_input_rx);
    'run_attempt: loop {
        attempt = attempt.saturating_add(1);
        // Best-effort state reset: rebuild a fresh input receiver so a retried
        // run does not get stuck on a half-drained queue. Deliberately empty —
        // transient fatalities are expected before meaningful progress, and the
        // parent can re-dispatch any pending input.
        let mut input_rx = match initial_input_rx.take() {
            Some(rx) => rx,
            None => mpsc::unbounded_channel().1,
        };

    let mut steps = 0;
    let mut final_result: Option<String> = None;
    let mut pending_inputs: VecDeque<SubAgentInput> = VecDeque::new();
    let mut consecutive_truncated_responses = 0;
    let mut latest_checkpoint: Option<SubAgentCheckpoint> = None;
    let mut tokens_used: u64 = 0;

    for _step in 0..max_steps {
        // Cooperative cancellation: bail if this session's token was cancelled
        // while we were between steps. Top-level model-visible sub-agents use
        // a detached token so parent turn cancellation does not stop them.
        if runtime.cancel_token.is_cancelled() {
            record_agent_progress(
                runtime,
                &agent_id,
                format!("{}: cancelled", format_step_counter(steps, max_steps)),
            );
            if let Some(mb) = runtime.mailbox.as_ref() {
                let _ = mb.send(MailboxMessage::Cancelled {
                    agent_id: agent_id.clone(),
                });
            }
            let status = SubAgentStatus::Cancelled;
            let duration_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
            insert_subagent_full_transcript_handle(
                runtime,
                &agent_id,
                &agent_type,
                &assignment,
                &status,
                None,
                latest_checkpoint.as_ref(),
                &messages,
                steps,
                duration_ms,
                fork_context_enabled,
            )
            .await;
            return Ok(SubAgentResult {
                name: agent_id.clone(),
                agent_id: agent_id.clone(),
                context_mode: if fork_context_enabled {
                    "forked"
                } else {
                    "fresh"
                }
                .to_string(),
                fork_context: fork_context_enabled,
                workspace: Some(runtime.context.workspace.clone()),
                git_branch: current_git_branch(&runtime.context.workspace),
                agent_type: agent_type.clone(),
                assignment: assignment.clone(),
                model: runtime.model.clone(),
                nickname: None,
                status,
                worker_status: None,
                parent_run_id: runtime.parent_agent_id.clone(),
                spawn_depth: runtime.spawn_depth,
                result: None,
                steps_taken: steps,
                checkpoint: latest_checkpoint.clone(),
                needs_input: None,
                duration_ms,
                from_prior_session: false,
            });
        }

        steps += 1;
        record_agent_progress(
            runtime,
            &agent_id,
            format!(
                "{}: requesting model response",
                format_step_counter(steps, max_steps)
            ),
        );

        while let Ok(input) = input_rx.try_recv() {
            if input.interrupt {
                pending_inputs.clear();
            }
            pending_inputs.push_back(input);
        }

        while let Some(input) = pending_inputs.pop_front() {
            if !input.text.trim().is_empty() {
                messages.push(Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: input.text,
                        cache_control: None,
                    }],
                });
            }
        }

        let child_completions = drain_child_completion_events(&mut child_completion_rx);
        if !child_completions.is_empty() {
            let count = child_completions.len();
            record_agent_progress(
                runtime,
                &agent_id,
                format!(
                    "{}: received {count} child sub-agent completion(s)",
                    format_step_counter(steps, max_steps)
                ),
            );
            messages.push(child_completion_runtime_message(&child_completions));
        }

        let request = MessageRequest {
            model: runtime.model.clone(),
            messages: messages.clone(),
            max_tokens: SUBAGENT_RESPONSE_MAX_TOKENS,
            system: Some(request_system.clone()),
            tools: Some(tools.clone()),
            tool_choice: Some(json!({ "type": "auto" })),
            metadata: None,
            thinking: None,
            reasoning_effort: runtime.reasoning_effort.clone(),
            stream: Some(false),
            temperature: None,
            top_p: None,
            response_format: None,
        };
        latest_checkpoint = Some(
            checkpoint_subagent_progress(
                runtime,
                &agent_id,
                "before_api_request",
                &messages,
                steps,
                true,
            )
            .await,
        );

        // Race the API call against the cancellation token so a parent
        // cancel during a long thinking turn doesn't have to wait for the
        // step timeout.
        let response = tokio::select! {
            biased;
            () = runtime.cancel_token.cancelled() => {
                record_agent_progress(
                    runtime,
                    &agent_id,
                    format!("{}: cancelled mid-request", format_step_counter(steps, max_steps)),
                );
                if let Some(mb) = runtime.mailbox.as_ref() {
                    let _ = mb.send(MailboxMessage::Cancelled {
                        agent_id: agent_id.clone(),
                    });
                }
                let status = SubAgentStatus::Cancelled;
                let duration_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                insert_subagent_full_transcript_handle(
                    runtime,
                    &agent_id,
                    &agent_type,
                    &assignment,
                    &status,
                    None,
                    latest_checkpoint.as_ref(),
                    &messages,
                    steps,
                    duration_ms,
                    fork_context_enabled,
                )
                .await;
                return Ok(SubAgentResult {
                    name: agent_id.clone(),
                    agent_id: agent_id.clone(),
                    context_mode: if fork_context_enabled { "forked" } else { "fresh" }.to_string(),
                    fork_context: fork_context_enabled,
                    workspace: Some(runtime.context.workspace.clone()),
                    git_branch: current_git_branch(&runtime.context.workspace),
                    agent_type: agent_type.clone(),
                    assignment: assignment.clone(),
                    model: runtime.model.clone(),
                    nickname: None,
                    status,
                    worker_status: None,
                    parent_run_id: runtime.parent_agent_id.clone(),
                    spawn_depth: runtime.spawn_depth,
                    result: None,
                    steps_taken: steps,
                    checkpoint: latest_checkpoint.clone(),
                    needs_input: None,
                    duration_ms,
                    from_prior_session: false,
                });
            }
            api = request_subagent_model_response_with_retries(
                runtime,
                &agent_id,
                steps,
                max_steps,
                request,
            ) => {
                match api {
                    Ok(response) => response,
                    Err(SubAgentApiRequestFailure::Fatal(err)) => {
                        // Whole-run retry for transient failures only. If the
                        // error is transient and we have retries left, re-dispatch
                        // the entire run via the outer loop; otherwise surface the
                        // fatal error unchanged (deterministic failures, e.g. bad
                        // args / permission denied / truncation, are never retried).
                        if is_transient_error(&err.to_string()) && attempt <= SUBAGENT_MAX_RETRIES {
                            record_agent_progress(
                                runtime,
                                &agent_id,
                                format!(
                                    "transient run failure; retrying whole sub-agent {}/{} ({err})",
                                    attempt, SUBAGENT_MAX_RETRIES
                                ),
                            );
                            tokio::time::sleep(SUBAGENT_RETRY_BACKOFF).await;
                            continue 'run_attempt;
                        }
                        return Err(err);
                    }
                    Err(SubAgentApiRequestFailure::Interrupted { reason, checkpoint_reason }) => {
                        let checkpoint = checkpoint_subagent_progress(
                            runtime,
                            &agent_id,
                            checkpoint_reason,
                            &messages,
                            steps,
                            true,
                        )
                        .await;
                        record_agent_progress(
                            runtime,
                            &agent_id,
                            format!("{}: interrupted; {reason}", format_step_counter(steps, max_steps)),
                        );
                        let status = SubAgentStatus::Interrupted(reason.clone());
                        let duration_ms =
                            u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                        insert_subagent_full_transcript_handle(
                            runtime,
                            &agent_id,
                            &agent_type,
                            &assignment,
                            &status,
                            Some(&reason),
                            Some(&checkpoint),
                            &messages,
                            steps,
                            duration_ms,
                            fork_context_enabled,
                        )
                        .await;
                        let needs_input =
                            needs_input_for_interrupted_checkpoint(&reason, &checkpoint);
                        let interrupted_snapshot = {
                            let mut manager = runtime.manager.write().await;
                            manager.interrupt_with_checkpoint(
                                &agent_id,
                                reason.clone(),
                                checkpoint.clone(),
                                Some(needs_input.clone()),
                            )?
                        };
                        record_agent_progress(
                            runtime,
                            &agent_id,
                            format!(
                                "{}: waiting for user; {}",
                                format_step_counter(steps, max_steps),
                                needs_input.question
                            ),
                        );
                        if let Some(mb) = runtime.mailbox.as_ref() {
                            let _ = mb.send(MailboxMessage::Interrupted {
                                agent_id: agent_id.clone(),
                                reason: reason.clone(),
                            });
                        }
                        return Ok(interrupted_snapshot);
                    }
                }
            }
        };

        let mut tool_uses = Vec::new();

        // Report token usage so the parent's cost counter updates live.
        if let Some(mb) = runtime.mailbox.as_ref() {
            let _ = mb.send(MailboxMessage::token_usage(
                &agent_id,
                response.model.clone(),
                response.usage.clone(),
            ));
        }
        {
            let mut manager = runtime.manager.write().await;
            manager.record_worker_usage(&agent_id, &response.usage);
        }

        // Per-worker token-budget enforcement (#3321): stop a single runaway
        // worker once its accumulated model tokens exceed its own cap. This
        // complements — and does not double-count — the scope-level admission
        // gate (#3319), which bounds aggregate fan-out across siblings. The
        // local accumulator mirrors the manager's `record.usage.total_tokens`
        // (both derive from `response.usage`), so the scope accounting stays
        // consistent and is never inflated by this check.
        tokens_used = tokens_used.saturating_add(usage_total_tokens(&response.usage));
        if let Some(budget) = token_budget
            && tokens_used > budget
        {
            record_agent_progress(
                runtime,
                &agent_id,
                format!(
                    "{}: token budget exhausted ({tokens_used}/{budget})",
                    format_step_counter(steps, max_steps)
                ),
            );
            if let Some(mb) = runtime.mailbox.as_ref() {
                let _ = mb.send(MailboxMessage::Cancelled {
                    agent_id: agent_id.clone(),
                });
            }
            let status = SubAgentStatus::BudgetExhausted;
            let duration_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
            latest_checkpoint = Some(
                checkpoint_subagent_progress(
                    runtime,
                    &agent_id,
                    "token_budget_exhausted",
                    &messages,
                    steps,
                    true,
                )
                .await,
            );
            insert_subagent_full_transcript_handle(
                runtime,
                &agent_id,
                &agent_type,
                &assignment,
                &status,
                final_result.as_ref(),
                latest_checkpoint.as_ref(),
                &messages,
                steps,
                duration_ms,
                fork_context_enabled,
            )
            .await;
            return Ok(SubAgentResult {
                name: agent_id.clone(),
                agent_id: agent_id.clone(),
                context_mode: if fork_context_enabled {
                    "forked"
                } else {
                    "fresh"
                }
                .to_string(),
                fork_context: fork_context_enabled,
                workspace: Some(runtime.context.workspace.clone()),
                git_branch: current_git_branch(&runtime.context.workspace),
                agent_type: agent_type.clone(),
                assignment: assignment.clone(),
                model: runtime.model.clone(),
                nickname: None,
                status,
                worker_status: None,
                parent_run_id: runtime.parent_agent_id.clone(),
                spawn_depth: runtime.spawn_depth,
                result: final_result.clone(),
                steps_taken: steps,
                checkpoint: latest_checkpoint.clone(),
                needs_input: None,
                duration_ms,
                from_prior_session: false,
            });
        }

        for block in &response.content {
            match block {
                ContentBlock::Text { text, .. } if !text.trim().is_empty() => {
                    final_result = Some(text.clone());
                }
                ContentBlock::ToolUse {
                    id, name, input, ..
                } => {
                    tool_uses.push((id.clone(), name.clone(), input.clone()));
                }
                _ => {}
            }
        }

        messages.push(Message {
            role: "assistant".to_string(),
            content: response.content.clone(),
        });
        latest_checkpoint = Some(
            checkpoint_subagent_progress(
                runtime,
                &agent_id,
                "after_model_response",
                &messages,
                steps,
                true,
            )
            .await,
        );

        if response_was_truncated(&response) {
            final_result = None;
            record_truncated_subagent_response(&mut consecutive_truncated_responses)?;
            let progress = if tool_uses.is_empty() {
                "response truncated, returning retry instruction".to_string()
            } else {
                format!(
                    "response truncated, returning {} tool error(s)",
                    tool_uses.len()
                )
            };
            record_agent_progress(
                runtime,
                &agent_id,
                format!("{}: {progress}", format_step_counter(steps, max_steps)),
            );
            messages.push(Message {
                role: "user".to_string(),
                content: if tool_uses.is_empty() {
                    truncated_response_text_retry_message()
                } else {
                    truncated_response_tool_results(&tool_uses)
                },
            });
            latest_checkpoint = Some(
                checkpoint_subagent_progress(
                    runtime,
                    &agent_id,
                    "after_truncated_response_retry_message",
                    &messages,
                    steps,
                    true,
                )
                .await,
            );
            continue;
        }
        reset_truncated_subagent_responses(&mut consecutive_truncated_responses);

        if tool_uses.is_empty() {
            let child_completions = drain_child_completion_events(&mut child_completion_rx);
            if !child_completions.is_empty() {
                let count = child_completions.len();
                record_agent_progress(
                    runtime,
                    &agent_id,
                    format!(
                        "{}: resuming with {count} child sub-agent completion(s)",
                        format_step_counter(steps, max_steps)
                    ),
                );
                messages.push(child_completion_runtime_message(&child_completions));
                latest_checkpoint = Some(
                    checkpoint_subagent_progress(
                        runtime,
                        &agent_id,
                        "after_tail_child_subagent_completion",
                        &messages,
                        steps,
                        true,
                    )
                    .await,
                );
                continue;
            }
            while let Ok(input) = input_rx.try_recv() {
                if input.interrupt {
                    pending_inputs.clear();
                }
                pending_inputs.push_back(input);
            }
            if pending_inputs.is_empty() {
                record_agent_progress(
                    runtime,
                    &agent_id,
                    format!("{}: complete", format_step_counter(steps, max_steps)),
                );
                break;
            }
            continue;
        }

        record_agent_progress(
            runtime,
            &agent_id,
            format!(
                "{}: executing {} tool call(s)",
                format_step_counter(steps, max_steps),
                tool_uses.len()
            ),
        );

        // Determine which tools can run in parallel (read-only, parallel-safe,
        // auto-approved) vs which must run sequentially. This mirrors the main
        // engine's `tool_plan_is_parallel_safe` logic.
        let parallel_safe: Vec<bool> = tool_uses
            .iter()
            .map(|(_, name, input)| {
                // Only parallelize when there are multiple tools
                if tool_uses.len() < 2 {
                    return false;
                }
                // Check tool spec flags
                let Some(spec) = tool_registry.registry.get(name) else {
                    return false;
                };
                spec.is_read_only_for(input)
                    && spec.supports_parallel_for(input)
                    && spec.approval_requirement_for(input) == ApprovalRequirement::Auto
            })
            .collect();

        let has_any_parallel = parallel_safe.iter().any(|&p| p);

        let mut tool_results: Vec<ContentBlock> = Vec::new();

        if has_any_parallel {
            // Execute parallel-eligible tools concurrently using FuturesUnordered.
            // Serial-only tools are executed inline between parallel batches.
            let mut parallel_tasks = FuturesUnordered::new();
            let mut serial_batch: Vec<(String, String, Value)> = Vec::new();

            for (idx, (tool_id, tool_name, tool_input)) in tool_uses.into_iter().enumerate() {
                if parallel_safe[idx] {
                    // Flush any pending serial batch before starting a parallel batch
                    for (sid, sname, sinput) in serial_batch.drain(..) {
                        let tool_display_name = subagent_progress_tool_display_name(&sname);
                        record_agent_progress(
                            runtime,
                            &agent_id,
                            format!(
                                "{}: running tool '{tool_display_name}' (serial)",
                                format_step_counter(steps, max_steps)
                            ),
                        );
                        if let Some(mb) = runtime.mailbox.as_ref() {
                            let _ = mb.send(MailboxMessage::ToolCallStarted {
                                agent_id: agent_id.clone(),
                                tool_name: sname.clone(),
                                step: steps,
                            });
                        }
                        let result = match tokio::time::timeout(runtime.tool_timeout, async {
                            tool_registry.execute(&agent_id, &sname, sinput).await
                        })
                        .await
                        {
                            Ok(Ok(output)) => output,
                            Ok(Err(e)) => format!("Error: {e}"),
                            Err(_) => format!("Error: Tool {sname} timed out"),
                        };
                        let tool_ok = !result.starts_with("Error:");
                        record_agent_progress(
                            runtime,
                            &agent_id,
                            format!(
                                "{}: finished tool '{tool_display_name}' (serial)",
                                format_step_counter(steps, max_steps)
                            ),
                        );
                        if let Some(mb) = runtime.mailbox.as_ref() {
                            let _ = mb.send(MailboxMessage::ToolCallCompleted {
                                agent_id: agent_id.clone(),
                                tool_name: sname.clone(),
                                step: steps,
                                ok: tool_ok,
                            });
                        }
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: sid,
                            content: result,
                            is_error: None,
                            content_blocks: None,
                        });
                    }

                    // Launch parallel task
                    let tool_display_name = subagent_progress_tool_display_name(&tool_name);
                    record_agent_progress(
                        runtime,
                        &agent_id,
                        format!(
                            "{}: running tool '{tool_display_name}' (parallel)",
                            format_step_counter(steps, max_steps)
                        ),
                    );
                    if let Some(mb) = runtime.mailbox.as_ref() {
                        let _ = mb.send(MailboxMessage::ToolCallStarted {
                            agent_id: agent_id.clone(),
                            tool_name: tool_name.clone(),
                            step: steps,
                        });
                    }
                    let agent_id_clone = agent_id.clone();
                    let mb_clone = runtime.mailbox.clone();
                    let tool_timeout = runtime.tool_timeout;
                    let registry_clone = tool_registry.clone();
                    let steps_clone = steps;
                    parallel_tasks.push(async move {
                        let result = match tokio::time::timeout(tool_timeout, async {
                            registry_clone
                                .execute(&agent_id_clone, &tool_name, tool_input)
                                .await
                        })
                        .await
                        {
                            Ok(Ok(output)) => output,
                            Ok(Err(e)) => format!("Error: {e}"),
                            Err(_) => format!("Error: Tool {tool_name} timed out"),
                        };
                        let tool_ok = !result.starts_with("Error:");
                        record_agent_progress(
                            runtime,
                            &agent_id_clone,
                            format!(
                                "{}: finished tool '{}' (parallel)",
                                format_step_counter(steps_clone, max_steps),
                                subagent_progress_tool_display_name(&tool_name)
                            ),
                        );
                        if let Some(mb) = mb_clone.as_ref() {
                            let _ = mb.send(MailboxMessage::ToolCallCompleted {
                                agent_id: agent_id_clone,
                                tool_name: tool_name.clone(),
                                step: steps_clone,
                                ok: tool_ok,
                            });
                        }
                        (tool_id, tool_name, result)
                    });
                } else {
                    // Queue for serial execution
                    serial_batch.push((tool_id, tool_name, tool_input));
                }
            }

            // Drain remaining serial batch
            for (sid, sname, sinput) in serial_batch.drain(..) {
                let tool_display_name = subagent_progress_tool_display_name(&sname);
                record_agent_progress(
                    runtime,
                    &agent_id,
                    format!(
                        "{}: running tool '{tool_display_name}' (serial)",
                        format_step_counter(steps, max_steps)
                    ),
                );
                if let Some(mb) = runtime.mailbox.as_ref() {
                    let _ = mb.send(MailboxMessage::ToolCallStarted {
                        agent_id: agent_id.clone(),
                        tool_name: sname.clone(),
                        step: steps,
                    });
                }
                let result = match tokio::time::timeout(runtime.tool_timeout, async {
                    tool_registry.execute(&agent_id, &sname, sinput).await
                })
                .await
                {
                    Ok(Ok(output)) => output,
                    Ok(Err(e)) => format!("Error: {e}"),
                    Err(_) => format!("Error: Tool {sname} timed out"),
                };
                let tool_ok = !result.starts_with("Error:");
                record_agent_progress(
                    runtime,
                    &agent_id,
                    format!(
                        "{}: finished tool '{tool_display_name}' (serial)",
                        format_step_counter(steps, max_steps)
                    ),
                );
                if let Some(mb) = runtime.mailbox.as_ref() {
                    let _ = mb.send(MailboxMessage::ToolCallCompleted {
                        agent_id: agent_id.clone(),
                        tool_name: sname.clone(),
                        step: steps,
                        ok: tool_ok,
                    });
                }
                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: sid,
                    content: result,
                    is_error: None,
                    content_blocks: None,
                });
            }

            // Collect parallel results (order may differ from insertion order)
            while let Some((tool_id, _tool_name, result)) = parallel_tasks.next().await {
                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: tool_id,
                    content: result,
                    is_error: None,
                    content_blocks: None,
                });
            }
        } else {
            // All tools must run sequentially
            for (tool_id, tool_name, tool_input) in tool_uses {
                let tool_display_name = subagent_progress_tool_display_name(&tool_name);
                record_agent_progress(
                    runtime,
                    &agent_id,
                    format!(
                        "{}: running tool '{tool_display_name}'",
                        format_step_counter(steps, max_steps)
                    ),
                );
                if let Some(mb) = runtime.mailbox.as_ref() {
                    let _ = mb.send(MailboxMessage::ToolCallStarted {
                        agent_id: agent_id.clone(),
                        tool_name: tool_name.clone(),
                        step: steps,
                    });
                }
                let result = match tokio::time::timeout(runtime.tool_timeout, async {
                    tool_registry
                        .execute(&agent_id, &tool_name, tool_input)
                        .await
                })
                .await
                {
                    Ok(Ok(output)) => output,
                    Ok(Err(e)) => format!("Error: {e}"),
                    Err(_) => format!("Error: Tool {tool_name} timed out"),
                };
                let tool_ok = !result.starts_with("Error:");
                record_agent_progress(
                    runtime,
                    &agent_id,
                    format!(
                        "{}: finished tool '{tool_display_name}'",
                        format_step_counter(steps, max_steps)
                    ),
                );
                if let Some(mb) = runtime.mailbox.as_ref() {
                    let _ = mb.send(MailboxMessage::ToolCallCompleted {
                        agent_id: agent_id.clone(),
                        tool_name: tool_name.clone(),
                        step: steps,
                        ok: tool_ok,
                    });
                }

                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: tool_id,
                    content: result,
                    is_error: None,
                    content_blocks: None,
                });
            }
        }

        if !tool_results.is_empty() {
            messages.push(Message {
                role: "user".to_string(),
                content: tool_results,
            });
            latest_checkpoint = Some(
                checkpoint_subagent_progress(
                    runtime,
                    &agent_id,
                    "after_tool_results",
                    &messages,
                    steps,
                    true,
                )
                .await,
            );
        }
    }

    release_resident_leases_for(&agent_id);
    let status = SubAgentStatus::Completed;
    let duration_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    latest_checkpoint = Some(build_subagent_checkpoint(
        &agent_id,
        "completed",
        &messages,
        steps,
        false,
    ));
    insert_subagent_full_transcript_handle(
        runtime,
        &agent_id,
        &agent_type,
        &assignment,
        &status,
        final_result.as_ref(),
        latest_checkpoint.as_ref(),
        &messages,
        steps,
        duration_ms,
        fork_context_enabled,
    )
    .await;

    return Ok(SubAgentResult {
        name: agent_id.clone(),
        agent_id,
        context_mode: if fork_context_enabled {
            "forked"
        } else {
            "fresh"
        }
        .to_string(),
        fork_context: fork_context_enabled,
        workspace: Some(runtime.context.workspace.clone()),
        git_branch: current_git_branch(&runtime.context.workspace),
        agent_type,
        assignment,
        model: runtime.model.clone(),
        nickname: None,
        status,
        worker_status: None,
        parent_run_id: runtime.parent_agent_id.clone(),
        spawn_depth: runtime.spawn_depth,
        result: final_result,
        steps_taken: steps,
        checkpoint: latest_checkpoint,
        needs_input: None,
        duration_ms,
        from_prior_session: false,
    });
    } // end 'run_attempt retry loop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_error_keywords_are_detected() {
        // Network / timeout / rate-limit / 5xx signals must be retried.
        assert!(is_transient_error("request timed out after 30000ms"));
        assert!(is_transient_error("Error: Tool read_file timed out"));
        assert!(is_transient_error("rate limit exceeded, retry later"));
        assert!(is_transient_error("HTTP 503 Service Unavailable"));
        assert!(is_transient_error("HTTP 429 Too Many Requests"));
        assert!(is_transient_error("connection reset by peer"));
        assert!(is_transient_error("network unreachable"));
        assert!(is_transient_error("service temporarily unavailable"));
        // Mixed-case must still match.
        assert!(is_transient_error("Deadline Has Elapsed"));
    }

    #[test]
    fn deterministic_errors_are_not_transient() {
        // Deterministic logic errors must NEVER be retried.
        assert!(!is_transient_error("permission denied"));
        assert!(!is_transient_error("invalid tool arguments"));
        assert!(!is_transient_error("Sub-agent requested unavailable tools: edit"));
        assert!(!is_transient_error("response truncated; context length exceeded"));
        assert!(!is_transient_error("user cancelled the operation"));
        assert!(!is_transient_error(""));
    }
}
