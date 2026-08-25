use super::lifecycle::{AgentLifecycleState, LifecycleTracker, StalledDetector, WaitCond};
use super::notify::Notifier;
use super::*;

impl SubAgentManager {
    /// Create a new manager for sub-agents.
    #[must_use]
    pub fn new(workspace: PathBuf, max_agents: usize) -> Self {
        // The stalled detector (#866) must watch the SAME tracker instance
        // the manager records into; `LifecycleTracker` derives `Clone` from
        // its `Arc`, so cloning shares the inner map rather than copying it.
        let lifecycle_tracker = LifecycleTracker::new();
        let stalled_detector = StalledDetector::new(lifecycle_tracker.clone());
        Self {
            agents: HashMap::new(),
            worker_records: HashMap::new(),
            worker_event_seq: 0,
            workspace,
            state_path: None,
            max_steps: DEFAULT_MAX_STEPS,
            max_agents,
            max_admitted_agents: max_agents,
            default_token_budget: None,
            running_heartbeat_timeout: Duration::from_secs(
                crate::config::DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
            ),
            // Fresh boot id per manager. Used by #405 to classify
            // re-loaded persisted agents as "prior session".
            current_session_boot_id: format!("boot_{}", &Uuid::new_v4().to_string()[..12]),
            // Default launch concurrency = the full agent cap; the gate only
            // throttles when a lower `launch_concurrency` is configured.
            launch_gate: Arc::new(Semaphore::new(max_agents.max(1))),
            last_persist_at: None,
            persist_pending: false,
            bus: Arc::new(AgentBus::new()),
            task_claims: new_shared_task_claim_manager(),
            file_claims: new_shared_file_claim_manager(),
            lifecycle_tracker,
            stalled_detector,
            notifier: Notifier::new(),
        }
    }

    /// Set the number of direct children that may execute concurrently
    /// before further launches queue (#3095). Clamped to `1..=max_agents`.
    #[must_use]
    pub fn with_launch_concurrency(mut self, limit: usize) -> Self {
        self.launch_gate = Arc::new(Semaphore::new(limit.clamp(1, self.max_agents)));
        self
    }

    /// Set the total queued + running admission ceiling for this manager.
    /// The value is always at least the instantaneous concurrency cap.
    #[must_use]
    pub fn with_admission_limit(mut self, max_admitted: usize) -> Self {
        self.max_admitted_agents =
            max_admitted.clamp(self.max_agents, crate::config::MAX_SUBAGENT_ADMISSION);
        self
    }

    /// Return a reference to the shared agent bus.
    pub fn bus(&self) -> &Arc<AgentBus> {
        &self.bus
    }

    /// Return a reference to the shared task-claim manager (#699).
    pub fn task_claims(&self) -> &SharedTaskClaimManager {
        &self.task_claims
    }

    /// Return a reference to the shared file-claim manager (#842).
    pub fn file_claims(&self) -> &SharedFileClaimManager {
        &self.file_claims
    }

    /// Plan non-overlapping file scopes for a set of agents that will run
    /// concurrently (#842).
    ///
    /// Feed it each parallel agent's declared file intent
    /// (`FileScopeAssignment { agent_id, files }`); it returns an
    /// `agent_id -> files` map with no file owned by two agents. Pair this
    /// with `file_claims()` so each agent only leases its assigned domain,
    /// preventing concurrent edits to the same file. This is the conflict-safety
    /// layer that makes mimofan's baseline multi-sub-agent concurrency safe by
    /// default.
    #[must_use]
    pub fn plan_concurrent_file_scopes(
        &self,
        assignments: &[FileScopeAssignment],
    ) -> std::collections::HashMap<String, Vec<String>> {
        plan_disjoint_file_sets(assignments)
    }

    /// Set the default aggregate token budget for root sub-agent runs.
    /// `None` and `Some(0)` both preserve unlimited legacy behavior.
    #[must_use]
    pub fn with_default_token_budget(mut self, budget: Option<u64>) -> Self {
        self.default_token_budget = positive_token_budget(budget);
        self
    }

    /// Classify an agent by its `session_boot_id`: `true` when the
    /// agent was either (a) loaded from disk with no id, or (b) carries
    /// a different id than the manager's current boot. Filters
    /// listing output by default (#405).
    fn is_from_prior_session(&self, agent: &SubAgent) -> bool {
        agent.session_boot_id.is_empty() || agent.session_boot_id != self.current_session_boot_id
    }

    #[must_use]
    pub(crate) fn with_state_path(mut self, path: PathBuf) -> Self {
        self.state_path = Some(path);
        self
    }

    #[must_use]
    pub fn with_running_heartbeat_timeout(mut self, timeout: Duration) -> Self {
        self.running_heartbeat_timeout = if timeout.is_zero() {
            Duration::from_secs(crate::config::DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS)
        } else {
            timeout
        };
        self
    }

    /// Apply live runtime limits. The launch semaphore is replaced only when
    /// no sub-agent is currently running, because active tasks may still hold
    /// permits from the previous semaphore.
    pub fn update_runtime_limits(
        &mut self,
        max_agents: usize,
        max_admitted_agents: usize,
        running_heartbeat_timeout: Duration,
        launch_concurrency: usize,
        default_token_budget: Option<u64>,
    ) -> bool {
        self.max_agents = max_agents.clamp(1, crate::config::MAX_SUBAGENTS);
        self.max_admitted_agents =
            max_admitted_agents.clamp(self.max_agents, crate::config::MAX_SUBAGENT_ADMISSION);
        self.default_token_budget = positive_token_budget(default_token_budget);
        self.running_heartbeat_timeout = if running_heartbeat_timeout.is_zero() {
            Duration::from_secs(crate::config::DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS)
        } else {
            running_heartbeat_timeout
        };
        if self.running_count() == 0 {
            self.launch_gate =
                Arc::new(Semaphore::new(launch_concurrency.clamp(1, self.max_agents)));
            true
        } else {
            false
        }
    }

    fn persist_state(&self) -> Result<()> {
        let Some(path) = self.state_path.as_ref() else {
            return Ok(());
        };
        let path = checked_subagent_state_path(&self.workspace, path)?;
        let now_ms = epoch_millis_now();
        let mut agents = Vec::with_capacity(self.agents.len());
        for agent in self.agents.values() {
            agents.push(PersistedSubAgent {
                id: agent.id.clone(),
                session_name: Some(agent.session_name.clone()),
                fork_context: agent.fork_context,
                workspace: Some(agent.workspace.clone()),
                agent_type: agent.agent_type.clone(),
                prompt: agent.prompt.clone(),
                assignment: agent.assignment.clone(),
                model: agent.model.clone(),
                nickname: agent.nickname.clone(),
                status: agent.status.clone(),
                result: agent.result.clone(),
                steps_taken: agent.steps_taken,
                checkpoint: agent.checkpoint.clone(),
                needs_input: agent.needs_input.clone(),
                duration_ms: u64::try_from(agent.started_at.elapsed().as_millis())
                    .unwrap_or(u64::MAX),
                // Backward-compat: Vec on disk. None → empty vec; Some(list) → list.
                // Reload converts empty vec back to None (full inheritance).
                allowed_tools: agent.allowed_tools.clone().unwrap_or_default(),
                updated_at_ms: now_ms,
                session_boot_id: agent.session_boot_id.clone(),
            });
        }
        agents.sort_by(|a, b| a.id.cmp(&b.id));

        let payload = PersistedSubAgentState {
            schema_version: SUBAGENT_STATE_SCHEMA_VERSION,
            agents,
            workers: self.sorted_worker_records(),
        };
        write_json_atomic(&self.workspace, &path, &payload)
    }

    fn persist_state_best_effort(&self) {
        if let Err(err) = self.persist_state() {
            // Must not be `eprintln!` — raw stderr inside the alt-screen
            // leaks into the buffer and produces the scroll-demon
            // regression (#1085). Routed through tracing so the
            // file-backed subscriber in `runtime_log` captures it.
            tracing::warn!(target: "subagent", ?err, "failed to persist sub-agent state");
        }
    }

    /// #freeze: persist on the hot per-step checkpoint path, coalesced to at
    /// most one disk write per `SUBAGENT_PERSIST_DEBOUNCE`. A skipped write
    /// sets `persist_pending` so the next terminal persist (which always
    /// rewrites the full fleet) or `flush_pending_persist` captures it.
    fn persist_state_debounced(&mut self) {
        let now = Instant::now();
        let due = match self.last_persist_at {
            Some(last) => now.duration_since(last) >= SUBAGENT_PERSIST_DEBOUNCE,
            None => true,
        };
        if due {
            self.last_persist_at = Some(now);
            self.persist_pending = false;
            self.persist_state_best_effort();
            let writes =
                SUBAGENT_PERSIST_WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if subagent_perf_enabled() {
                let skipped = SUBAGENT_PERSIST_SKIPPED.load(std::sync::atomic::Ordering::Relaxed);
                tracing::info!(
                    target: "subagent_perf",
                    writes,
                    skipped,
                    agents = self.agents.len(),
                    "checkpoint persist (debounced write)"
                );
            }
        } else {
            self.persist_pending = true;
            SUBAGENT_PERSIST_SKIPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// #freeze: force a persist if a hot-path write was previously coalesced
    /// away. Call on graceful shutdown / session teardown so the most recent
    /// intermediate checkpoint is not lost.
    pub fn flush_pending_persist(&mut self) {
        if self.persist_pending {
            self.last_persist_at = Some(Instant::now());
            self.persist_pending = false;
            self.persist_state_best_effort();
        }
    }

    pub(crate) fn load_state(&mut self) -> Result<()> {
        let Some(path) = self.state_path.as_ref() else {
            return Ok(());
        };
        let path = checked_subagent_state_path(&self.workspace, path)?;
        if !path.exists() {
            return Ok(());
        }

        let raw = read_subagent_state_file(&self.workspace, &path)?;
        let state = serde_json::from_str::<PersistedSubAgentState>(&raw)?;
        if state.schema_version != SUBAGENT_STATE_SCHEMA_VERSION {
            return Err(anyhow!(
                "Unsupported sub-agent state schema {}",
                state.schema_version
            ));
        }

        self.agents.clear();
        self.worker_records.clear();
        for persisted in state.agents {
            let mut status = persisted.status;
            if matches!(status, SubAgentStatus::Running) {
                status = SubAgentStatus::Interrupted(SUBAGENT_RESTART_REASON.to_string());
            }

            let started_at = instant_from_duration(Duration::from_millis(persisted.duration_ms));
            // Empty vec on disk → None (full inheritance, v0.6.6 default).
            // Non-empty vec → Some(list) (preserves narrow scope from older sessions).
            let allowed_tools = if persisted.allowed_tools.is_empty() {
                None
            } else {
                Some(persisted.allowed_tools)
            };
            let agent = SubAgent {
                id: persisted.id.clone(),
                session_name: persisted
                    .session_name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| persisted.id.clone()),
                fork_context: persisted.fork_context,
                workspace: persisted
                    .workspace
                    .unwrap_or_else(|| self.workspace.clone()),
                // Restored agents have no tracked worktree; legacy records
                // predate worktree tracking (#691).
                worktree_path: None,
                agent_type: persisted.agent_type,
                prompt: persisted.prompt,
                assignment: persisted.assignment,
                model: if persisted.model.is_empty() {
                    "unknown".to_string()
                } else {
                    persisted.model
                },
                nickname: persisted.nickname,
                status,
                result: persisted.result,
                steps_taken: persisted.steps_taken,
                checkpoint: persisted.checkpoint,
                needs_input: persisted.needs_input,
                started_at,
                last_activity_at: started_at,
                allowed_tools,
                // Empty string when loading pre-#405 records; the
                // manager treats that the same as a non-matching id —
                // i.e. agent classified as prior-session.
                session_boot_id: persisted.session_boot_id,
                input_tx: None,
                task_handle: None,
            };
            self.agents.insert(persisted.id, agent);
        }
        for worker in state.workers {
            let worker = normalize_worker_record(worker);
            self.worker_event_seq = self.worker_event_seq.max(
                worker
                    .events
                    .iter()
                    .map(|event| event.seq)
                    .max()
                    .unwrap_or(0),
            );
            self.worker_records
                .insert(worker.spec.worker_id.clone(), worker);
        }
        self.refresh_all_budget_scopes();
        self.prune_worker_records();

        Ok(())
    }

    fn sorted_worker_records(&self) -> Vec<AgentWorkerRecord> {
        let mut workers: Vec<_> = self.worker_records.values().cloned().collect();
        workers.sort_by(|a, b| {
            b.updated_at_ms
                .cmp(&a.updated_at_ms)
                .then_with(|| a.spec.worker_id.cmp(&b.spec.worker_id))
        });
        workers
    }

    fn prune_worker_records(&mut self) {
        if self.worker_records.len() <= MAX_AGENT_WORKER_RECORDS {
            return;
        }
        let keep_ids: std::collections::HashSet<String> = self
            .sorted_worker_records()
            .into_iter()
            .take(MAX_AGENT_WORKER_RECORDS)
            .map(|record| record.spec.worker_id)
            .collect();
        self.worker_records
            .retain(|worker_id, _| keep_ids.contains(worker_id));
    }

    pub fn register_worker(&mut self, spec: AgentWorkerSpec) {
        let worker_id = spec.worker_id.clone();
        let now_ms = epoch_millis_now();
        let mut record = AgentWorkerRecord::new(normalize_worker_spec(spec), now_ms);
        self.push_worker_event(
            &mut record,
            AgentWorkerStatus::Starting,
            Some("starting".to_string()),
            None,
            None,
            now_ms,
        );
        self.worker_records.insert(worker_id, record);
        self.prune_worker_records();
    }

    pub fn list_worker_records(&self) -> Vec<AgentWorkerRecord> {
        self.sorted_worker_records()
    }

    pub fn get_worker_record(&self, worker_id: &str) -> Option<AgentWorkerRecord> {
        self.worker_records.get(worker_id).cloned()
    }

    fn aggregate_budget_spent(&self, scope_id: &str) -> u64 {
        self.worker_records
            .values()
            .filter(|record| record.usage.budget_scope.as_deref() == Some(scope_id))
            .fold(0_u64, |total, record| {
                total.saturating_add(record.usage.total_tokens.unwrap_or(0))
            })
    }

    fn inherited_budget_scope(&self, parent_run_id: Option<&str>) -> Option<(String, u64)> {
        let parent = self.worker_records.get(parent_run_id?)?;
        let limit = parent.usage.token_budget?;
        let scope_id = parent
            .usage
            .budget_scope
            .clone()
            .unwrap_or_else(|| parent.spec.worker_id.clone());
        Some((scope_id, limit))
    }

    fn resolve_spawn_budget_scope(
        &self,
        worker_id: &str,
        parent_run_id: Option<&str>,
        requested_budget: Option<u64>,
    ) -> Result<Option<AgentUsageBudgetScope>> {
        let scope = if let Some(limit) = positive_token_budget(requested_budget) {
            Some((worker_id.to_string(), limit))
        } else if let Some(parent_scope) = self.inherited_budget_scope(parent_run_id) {
            Some(parent_scope)
        } else {
            self.default_token_budget
                .map(|limit| (worker_id.to_string(), limit))
        };

        let Some((scope_id, limit)) = scope else {
            return Ok(None);
        };
        let spent = self.aggregate_budget_spent(&scope_id);
        let remaining = limit.saturating_sub(spent);
        if remaining < MIN_SUBAGENT_SPAWN_TOKEN_RESERVE {
            return Err(anyhow!(
                "Sub-agent token budget exhausted for scope {scope_id}: {spent}/{limit} tokens spent, {remaining} remaining. Wait for the parent/Workflow to summarize results or start a new agent run with an explicit token_budget override."
            ));
        }
        Ok(Some(AgentUsageBudgetScope {
            scope_id,
            limit,
            spent,
            remaining,
        }))
    }

    fn attach_budget_scope(&mut self, worker_id: &str, scope: AgentUsageBudgetScope) {
        let Some(record) = self.worker_records.get_mut(worker_id) else {
            return;
        };
        record.usage.token_budget = Some(scope.limit);
        record.usage.budget_scope = Some(scope.scope_id.clone());
        record.usage.budget_spent_tokens = Some(scope.spent);
        record.usage.budget_remaining_tokens = Some(scope.remaining);
        refresh_usage_note(&mut record.usage);
        self.refresh_budget_scope(&scope.scope_id);
    }

    fn refresh_budget_scope(&mut self, scope_id: &str) {
        let Some(limit) = self
            .worker_records
            .values()
            .find(|record| record.usage.budget_scope.as_deref() == Some(scope_id))
            .and_then(|record| record.usage.token_budget)
        else {
            return;
        };
        let spent = self.aggregate_budget_spent(scope_id);
        let remaining = limit.saturating_sub(spent);
        for record in self.worker_records.values_mut() {
            if record.usage.budget_scope.as_deref() == Some(scope_id) {
                record.usage.token_budget = Some(limit);
                record.usage.budget_spent_tokens = Some(spent);
                record.usage.budget_remaining_tokens = Some(remaining);
                refresh_usage_note(&mut record.usage);
            }
        }
    }

    fn refresh_all_budget_scopes(&mut self) {
        let scope_ids = self
            .worker_records
            .values()
            .filter_map(|record| record.usage.budget_scope.clone())
            .collect::<std::collections::HashSet<_>>();
        for scope_id in scope_ids {
            self.refresh_budget_scope(&scope_id);
        }
    }

    pub(crate) fn record_worker_usage(&mut self, worker_id: &str, usage: &Usage) {
        let now_ms = epoch_millis_now();
        let total_delta = usage_total_tokens(usage);
        let Some(record) = self.worker_records.get_mut(worker_id) else {
            return;
        };
        record.updated_at_ms = now_ms;
        record.usage.input_tokens = Some(
            record
                .usage
                .input_tokens
                .unwrap_or(0)
                .saturating_add(u64::from(usage.input_tokens)),
        );
        record.usage.output_tokens = Some(
            record
                .usage
                .output_tokens
                .unwrap_or(0)
                .saturating_add(u64::from(usage.output_tokens)),
        );
        record.usage.total_tokens = Some(
            record
                .usage
                .total_tokens
                .unwrap_or(0)
                .saturating_add(total_delta),
        );
        let scope_id = record.usage.budget_scope.clone();
        refresh_usage_note(&mut record.usage);
        if let Some(scope_id) = scope_id {
            self.refresh_budget_scope(&scope_id);
        }
        self.persist_state_debounced();
    }

    fn push_worker_event(
        &mut self,
        record: &mut AgentWorkerRecord,
        status: AgentWorkerStatus,
        message: Option<String>,
        step: Option<u32>,
        tool_name: Option<String>,
        now_ms: u64,
    ) {
        self.worker_event_seq = self.worker_event_seq.saturating_add(1);
        record.events.push_back(AgentWorkerEvent {
            seq: self.worker_event_seq,
            worker_id: record.spec.worker_id.clone(),
            status,
            timestamp_ms: now_ms,
            message,
            step,
            tool_name,
        });
        while record.events.len() > MAX_AGENT_WORKER_EVENTS_PER_RECORD {
            record.events.pop_front();
        }
    }

    pub(crate) fn record_worker_event(
        &mut self,
        worker_id: &str,
        status: AgentWorkerStatus,
        message: Option<String>,
        step: Option<u32>,
        tool_name: Option<String>,
    ) {
        let now_ms = epoch_millis_now();
        let Some(mut record) = self.worker_records.remove(worker_id) else {
            return;
        };
        record.status = status.clone();
        record.recommended_action =
            recommended_action_for_worker_status(status.clone(), &record.spec);
        record.updated_at_ms = now_ms;
        record.latest_message = message.clone();
        if matches!(
            status,
            AgentWorkerStatus::Starting | AgentWorkerStatus::Running
        ) && record.started_at_ms.is_none()
        {
            record.started_at_ms = Some(now_ms);
        }
        if matches!(
            status,
            AgentWorkerStatus::Completed
                | AgentWorkerStatus::Failed
                | AgentWorkerStatus::Cancelled
                | AgentWorkerStatus::Interrupted
        ) {
            record.completed_at_ms = Some(now_ms);
        }
        if let Some(step) = step {
            record.steps_taken = step;
        }
        self.push_worker_event(&mut record, status, message, step, tool_name, now_ms);
        self.worker_records.insert(worker_id.to_string(), record);
    }

    pub(crate) fn record_worker_progress(&mut self, worker_id: &str, message: String) {
        let (status, step, tool_name) = worker_progress_event_parts(&message);
        self.record_worker_event(worker_id, status, Some(message), step, tool_name);
    }

    fn complete_worker_from_result(&mut self, worker_id: &str, result: &SubAgentResult) {
        let status = worker_status_from_subagent_result(result);
        let message = match &result.status {
            SubAgentStatus::Completed => Some("completed".to_string()),
            SubAgentStatus::Failed(err) => Some(err.clone()),
            SubAgentStatus::Interrupted(reason) => Some(reason.clone()),
            SubAgentStatus::Cancelled => Some("cancelled".to_string()),
            SubAgentStatus::BudgetExhausted => Some("token budget exhausted".to_string()),
            SubAgentStatus::Running => Some("running".to_string()),
        };
        self.record_worker_event(worker_id, status, message, Some(result.steps_taken), None);
        if let Some(record) = self.worker_records.get_mut(worker_id) {
            record.result_summary = result.result.clone();
            record.steps_taken = result.steps_taken;
            if let SubAgentStatus::Failed(err) = &result.status {
                record.error = Some(err.clone());
            }
        }
    }

    fn fail_worker(&mut self, worker_id: &str, error: String) {
        self.record_worker_event(
            worker_id,
            AgentWorkerStatus::Failed,
            Some(error.clone()),
            None,
            None,
        );
        if let Some(record) = self.worker_records.get_mut(worker_id) {
            record.error = Some(error);
        }
    }

    pub fn cancel_agent(&mut self, agent_ref: &str) -> Result<SubAgentResult> {
        let agent_id = self.resolve_agent_ref(agent_ref)?;
        let snapshot = {
            let agent = self
                .agents
                .get_mut(&agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;
            if agent.status != SubAgentStatus::Running {
                return Ok(agent.snapshot());
            }
            agent.status = SubAgentStatus::Cancelled;
            agent.result = Some("Cancelled by parent request.".to_string());
            release_resident_leases_for(&agent.id);
            if let Some(handle) = agent.task_handle.take() {
                handle.abort();
            }
            agent.input_tx = None;
            agent.snapshot()
        };
        self.record_worker_event(
            &agent_id,
            AgentWorkerStatus::Cancelled,
            snapshot.result.clone(),
            Some(snapshot.steps_taken),
            None,
        );
        // #864: a cancelled agent is terminal → Done.
        self.lifecycle_tracker
            .set(&agent_id, AgentLifecycleState::Done);
        self.persist_state_best_effort();
        Ok(snapshot)
    }

    /// Count running agents.
    pub fn running_count(&self) -> usize {
        self.admitted_count()
    }

    /// Count live sub-agents that have been admitted, including queued
    /// workers waiting on the launch gate.
    pub fn admitted_count(&self) -> usize {
        self.agents
            .values()
            .filter(|agent| {
                // Exclude non-running statuses
                if agent.status != SubAgentStatus::Running {
                    return false;
                }
                // Exclude persisted agents with no task_handle (they're not actually running)
                if agent.task_handle.is_none() {
                    return false;
                }
                // Keep recently finished handles counted until the terminal
                // status update has reconciled. Otherwise a fanout burst can
                // refill the cap before the UI/state catches up (#2211).
                !self.running_heartbeat_timed_out(agent)
            })
            .count()
    }

    /// Count admitted workers that are currently waiting for the launch gate.
    pub fn queued_count(&self) -> usize {
        self.agents
            .values()
            .filter(|agent| {
                agent.status == SubAgentStatus::Running
                    && agent.task_handle.is_some()
                    && !self.running_heartbeat_timed_out(agent)
                    && self
                        .worker_records
                        .get(&agent.id)
                        .is_some_and(|record| record.status == AgentWorkerStatus::Queued)
            })
            .count()
    }

    /// Count admitted workers not currently in the queued launch state.
    pub fn active_count(&self) -> usize {
        self.admitted_count().saturating_sub(self.queued_count())
    }

    pub(crate) fn check_admission_capacity(&self) -> Result<()> {
        let admitted = self.admitted_count();
        if admitted >= self.max_admitted_agents {
            return Err(anyhow!(
                "Sub-agent admission limit reached (max_admitted {}, admitted {}, running {}, queued {}). Wait for queued/running agents to finish, cancel unneeded agents, or raise [subagents] max_admitted for this Workflow.",
                self.max_admitted_agents,
                admitted,
                self.active_count(),
                self.queued_count()
            ));
        }
        Ok(())
    }

    fn running_heartbeat_timed_out(&self, agent: &SubAgent) -> bool {
        agent.status == SubAgentStatus::Running
            && agent.task_handle.is_some()
            && agent.last_activity_at.elapsed() >= self.running_heartbeat_timeout
    }

    pub fn touch(&mut self, agent_id: &str) -> bool {
        let Some(agent) = self.agents.get_mut(agent_id) else {
            return false;
        };
        if agent.status != SubAgentStatus::Running {
            return false;
        }
        agent.last_activity_at = Instant::now();
        true
    }

    /// Spawn a new background sub-agent.
    pub fn spawn_background(
        &mut self,
        manager_handle: SharedSubAgentManager,
        runtime: SubAgentRuntime,
        agent_type: SubAgentType,
        prompt: String,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<SubAgentResult> {
        self.spawn_background_with_assignment(
            manager_handle,
            runtime,
            agent_type,
            prompt.clone(),
            SubAgentAssignment::new(prompt, None),
            allowed_tools,
        )
    }

    /// Spawn a new background sub-agent with explicit assignment metadata.
    pub fn spawn_background_with_assignment(
        &mut self,
        manager_handle: SharedSubAgentManager,
        runtime: SubAgentRuntime,
        agent_type: SubAgentType,
        prompt: String,
        assignment: SubAgentAssignment,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<SubAgentResult> {
        self.spawn_background_with_assignment_options(
            manager_handle,
            runtime,
            agent_type,
            prompt,
            assignment,
            allowed_tools,
            SubAgentSpawnOptions::default(),
            None, // No custom agent definition
        )
    }

    /// Spawn a new background sub-agent with explicit assignment and display
    /// metadata.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_background_with_assignment_options(
        &mut self,
        manager_handle: SharedSubAgentManager,
        mut runtime: SubAgentRuntime,
        agent_type: SubAgentType,
        prompt: String,
        assignment: SubAgentAssignment,
        allowed_tools: Option<Vec<String>>,
        options: SubAgentSpawnOptions,
        custom_agent_def: Option<custom_agents::CustomAgentDef>,
    ) -> Result<SubAgentResult> {
        self.cleanup(COMPLETED_AGENT_RETENTION);

        // Reclaim orphaned worktree metadata from crashed / force-killed
        // sub-agents before we add a new one (#691).
        prune_orphan_worktrees(&self.workspace);

        self.check_admission_capacity()?;

        if let Some(model) = options.model.as_deref() {
            runtime.model = model.to_string();
        }
        let effective_model = runtime.model.clone();
        let agent_id = format!("agent_{}", &Uuid::new_v4().to_string()[..8]);
        let budget_scope = self.resolve_spawn_budget_scope(
            &agent_id,
            runtime.parent_agent_id.as_deref(),
            options.token_budget,
        )?;
        let active_names: std::collections::HashSet<String> = self
            .agents
            .values()
            .filter_map(|a| a.nickname.clone())
            .collect();
        let nickname = options
            .nickname
            .or_else(|| Some(assign_unique_whale_name(&agent_id, &active_names)));
        let tools = build_allowed_tools(&agent_type, allowed_tools, runtime.allow_shell)?;
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let mut agent = SubAgent::new(
            agent_id.clone(),
            agent_type.clone(),
            prompt.clone(),
            assignment.clone(),
            effective_model,
            nickname,
            tools.clone(),
            input_tx,
            runtime.context.workspace.clone(),
            self.current_session_boot_id.clone(),
            runtime.worktree_path.clone(),
        );
        if let Some(name) = options
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if let Some(existing) = self
                .agents
                .values()
                .find(|existing| existing.session_name == name)
            {
                // #3020: Include elapsed time so the parent can distinguish a
                // live worker from a stale/failed earlier spawn (#2656).
                let elapsed = existing.started_at.elapsed();
                let since = if elapsed.as_secs() < 120 {
                    format!("{}s ago", elapsed.as_secs())
                } else {
                    let mins = elapsed.as_secs() / 60;
                    let secs = elapsed.as_secs() % 60;
                    format!("{mins}m{secs}s ago")
                };
                return Err(anyhow!(
                    "Sub-agent session name '{name}' is already in use by agent_id '{}' \
                     (status: {}, started {since}). \
                     Wait for its completion event, or open a new agent with a different name.",
                    existing.id,
                    subagent_status_name(&existing.status)
                ));
            }
            agent.session_name = name.to_string();
        }
        agent.fork_context = options.fork_context;
        let agent_id = agent.id.clone();
        let started_at = agent.started_at;
        let max_steps = self.max_steps;
        let tool_profile = match tools.clone() {
            Some(tools) => AgentWorkerToolProfile::Explicit(tools),
            None => AgentWorkerToolProfile::Inherited,
        };
        let runtime_profile = worker_profile_for_spawn(
            &runtime,
            &agent_type,
            &tool_profile,
            &agent.model,
            options.model_route.clone(),
        );
        runtime.worker_profile = runtime_profile.clone();
        let worker_spec = AgentWorkerSpec {
            worker_id: agent_id.clone(),
            run_id: agent_id.clone(),
            parent_run_id: runtime.parent_agent_id.clone(),
            session_name: Some(agent.session_name.clone()),
            objective: assignment.objective.clone(),
            role: assignment.role.clone(),
            agent_type: agent_type.clone(),
            model: agent.model.clone(),
            workspace: agent.workspace.clone(),
            git_branch: current_git_branch(&agent.workspace),
            context_mode: if options.fork_context {
                "forked"
            } else {
                "fresh"
            }
            .to_string(),
            fork_context: options.fork_context,
            tool_profile,
            runtime_profile,
            max_steps,
            spawn_depth: runtime.spawn_depth,
            max_spawn_depth: runtime.max_spawn_depth,
        };
        self.register_worker(worker_spec);
        if let Some(scope) = budget_scope {
            self.attach_budget_scope(&agent_id, scope);
        }

        if let Some(event_tx) = runtime.event_tx.clone() {
            let _ = event_tx.try_send(Event::AgentSpawned {
                id: agent_id.clone(),
                prompt: prompt.clone(),
                parent_run_id: runtime.parent_agent_id.clone(),
                spawn_depth: runtime.spawn_depth,
            });
        }

        let launch_gate = (runtime.spawn_depth == 1).then(|| self.launch_gate.clone());
        let task = SubAgentTask {
            manager_handle,
            runtime,
            agent_id: agent_id.clone(),
            agent_type,
            prompt,
            assignment,
            allowed_tools: tools,
            fork_context: options.fork_context,
            fork_turns: options.fork_turns,
            started_at,
            max_steps,
            token_budget: options.token_budget,
            input_rx,
            custom_agent_def,
            launch_gate,
        };
        let handle = spawn_supervised(
            "subagent-task",
            std::panic::Location::caller(),
            run_subagent_task(task),
        );
        agent.task_handle = Some(handle);
        self.agents.insert(agent_id.clone(), agent);
        // #864: record the spawn in the lifecycle tracker. The child starts
        // `Working` (it has been dispatched with a task handle). It will
        // transition to `Blocked`/`Done` as the run_report hook reports.
        self.lifecycle_tracker
            .set(&agent_id, AgentLifecycleState::Working);
        self.persist_state_best_effort();

        Ok(self
            .agents
            .get(&agent_id)
            .expect("agent should exist after spawn")
            .snapshot())
    }

    /// Get the current snapshot for an agent.
    pub fn get_result(&self, agent_id: &str) -> Result<SubAgentResult> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;
        Ok(agent.snapshot())
    }

    pub fn get_result_by_ref(&self, agent_ref: &str) -> Result<SubAgentResult> {
        let agent_id = self.resolve_agent_ref(agent_ref)?;
        self.get_result(&agent_id)
    }

    pub fn terminal_results_excluding(
        &self,
        delivered_ids: &std::collections::HashSet<String>,
    ) -> Vec<SubAgentResult> {
        let mut results = self
            .agents
            .values()
            .filter(|agent| agent.status != SubAgentStatus::Running)
            .filter(|agent| agent.session_boot_id == self.current_session_boot_id)
            .filter(|agent| {
                self.worker_records
                    .get(&agent.id)
                    .is_none_or(|record| record.spec.parent_run_id.is_none())
            })
            .filter(|agent| !delivered_ids.contains(&agent.id))
            .map(SubAgent::snapshot)
            .collect::<Vec<_>>();
        results.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
        results
    }

    /// Resolve either a durable agent id or a model-facing session name.
    fn resolve_agent_ref(&self, agent_ref: &str) -> Result<String> {
        let agent_ref = agent_ref.trim();
        if self.agents.contains_key(agent_ref) {
            return Ok(agent_ref.to_string());
        }

        let matches = self
            .agents
            .values()
            .filter(|agent| agent.session_name == agent_ref)
            .map(|agent| agent.id.clone())
            .collect::<Vec<_>>();

        match matches.as_slice() {
            [id] => Ok(id.clone()),
            [] => Err(anyhow!("Agent session {agent_ref} not found")),
            _ => Err(anyhow!(
                "Agent session name '{agent_ref}' is ambiguous; use an agent_id"
            )),
        }
    }

    /// List all agents and their status.
    #[must_use]
    /// Snapshot a single agent and tag it with the manager's
    /// classification. The bare `SubAgent::snapshot` defaults
    /// `from_prior_session` to `false`; only the manager knows the
    /// matching boot id, so listing goes through here.
    fn snapshot_for_listing(&self, agent: &SubAgent) -> SubAgentResult {
        let mut snap = agent.snapshot();
        snap.from_prior_session = self.is_from_prior_session(agent);
        if let Some(record) = self.worker_records.get(&agent.id) {
            snap.worker_status = Some(record.status.clone());
            snap.parent_run_id = record
                .parent_run_id
                .clone()
                .or_else(|| record.spec.parent_run_id.clone());
            snap.spawn_depth = record.spec.spawn_depth;
        }
        snap
    }

    /// List all agents currently held by the manager, regardless of
    /// session origin. Use [`Self::list_filtered`] in user-facing tool
    /// paths so prior-session agents stay hidden by default (#405).
    pub fn list(&self) -> Vec<SubAgentResult> {
        self.agents
            .values()
            .map(|agent| self.snapshot_for_listing(agent))
            .collect()
    }

    /// List agents respecting the session-boundary filter (#405).
    ///
    /// `include_archived = false` drops
    /// any prior-session agent that is no longer running. Prior-session
    /// agents that are still `Running` (e.g. interrupted by a process
    /// restart) stay visible — they may matter for ongoing recovery.
    ///
    /// `include_archived = true` returns everything, with the
    /// `from_prior_session` flag on each `SubAgentResult` so the model
    /// can tell active and archived apart at a glance.
    pub fn list_filtered(&self, include_archived: bool) -> Vec<SubAgentResult> {
        self.agents
            .values()
            .filter(|agent| {
                if include_archived {
                    return true;
                }
                if agent.status == SubAgentStatus::Running {
                    return true;
                }
                !self.is_from_prior_session(agent)
            })
            .map(|agent| self.snapshot_for_listing(agent))
            .collect()
    }

    /// Clean up stale running agents and completed agents older than the
    /// given duration. Returns the number of running agents auto-cancelled
    /// during this pass.
    pub fn cleanup(&mut self, max_age: Duration) -> usize {
        let before = self.agents.len();
        let mut auto_cancelled = 0;
        let timeout = self.running_heartbeat_timeout;
        let mut worker_cancellations = Vec::new();
        for agent in self.agents.values_mut() {
            if agent.status == SubAgentStatus::Running
                && agent.task_handle.is_some()
                && agent.last_activity_at.elapsed() >= timeout
            {
                tracing::warn!(
                    target: "subagent",
                    agent_id = %agent.id,
                    timeout_secs = timeout.as_secs(),
                    "auto-cancelling stale sub-agent with no manager-visible progress"
                );
                agent.status = SubAgentStatus::Cancelled;
                agent.result = Some(format!(
                    "Auto-cancelled after {}s without sub-agent progress.",
                    timeout.as_secs()
                ));
                release_resident_leases_for(&agent.id);
                // Auto-cancelled agents have been silent past the heartbeat
                // timeout; reclaim their isolated worktree now rather than
                // waiting out the retention window (#691).
                if let Some(path) = agent.worktree_path.clone() {
                    remove_worktree(&path);
                }
                if let Some(handle) = agent.task_handle.take() {
                    handle.abort();
                }
                agent.input_tx = None;
                worker_cancellations.push((
                    agent.id.clone(),
                    agent.result.clone(),
                    agent.steps_taken,
                ));
                auto_cancelled += 1;
            }
        }
        for (agent_id, message, steps_taken) in worker_cancellations {
            self.record_worker_event(
                &agent_id,
                AgentWorkerStatus::Cancelled,
                message,
                Some(steps_taken),
                None,
            );
        }
        // Collect worktree paths of terminal agents about to be evicted so we
        // can `git worktree remove` them after the retain (avoiding a borrow of
        // `self` inside the closure). Only terminal agents past `max_age` are
        // removed here; in-retention worktrees stay so the parent can still read
        // the child's output / files (#691).
        let mut removed_worktrees: Vec<PathBuf> = self
            .agents
            .iter()
            .filter(|(_, agent)| {
                agent.status != SubAgentStatus::Running && agent.started_at.elapsed() >= max_age
            })
            .filter_map(|(_, agent)| agent.worktree_path.clone())
            .collect();
        self.agents.retain(|_, agent| {
            if agent.status == SubAgentStatus::Running {
                true
            } else {
                agent.started_at.elapsed() < max_age
            }
        });
        for path in removed_worktrees.drain(..) {
            remove_worktree(&path);
        }
        if self.agents.len() != before || auto_cancelled > 0 {
            self.persist_state_best_effort();
        }
        auto_cancelled
    }

    pub(crate) fn update_from_result(&mut self, agent_id: &str, result: SubAgentResult) {
        let mut changed = false;
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = result.status.clone();
            agent.assignment = result.assignment.clone();
            agent.result = result.result.clone();
            agent.steps_taken = result.steps_taken;
            agent.checkpoint = result.checkpoint.clone();
            agent.needs_input = result.needs_input.clone();
            if result.status != SubAgentStatus::Running {
                agent.input_tx = None;
            }
            agent.task_handle = None;
            changed = true;
        }
        self.complete_worker_from_result(agent_id, &result);
        // #864: any non-running terminal status advances the lifecycle to Done.
        self.lifecycle_tracker
            .set(agent_id, AgentLifecycleState::Done);
        if changed {
            self.persist_state_best_effort();
        }
    }

    pub(crate) fn update_failed(&mut self, agent_id: &str, error: String) {
        let mut changed = false;
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = SubAgentStatus::Failed(error.clone());
            release_resident_leases_for(agent_id);
            agent.input_tx = None;
            agent.task_handle = None;
            changed = true;
        }
        self.fail_worker(agent_id, error);
        // #864: a failed agent can no longer progress → Done.
        self.lifecycle_tracker
            .set(agent_id, AgentLifecycleState::Done);
        if changed {
            self.persist_state_best_effort();
        }
    }

    pub(crate) fn update_checkpoint(
        &mut self,
        agent_id: &str,
        checkpoint: SubAgentCheckpoint,
    ) -> bool {
        let Some(agent) = self.agents.get_mut(agent_id) else {
            return false;
        };
        agent.steps_taken = checkpoint.steps_taken;
        agent.checkpoint = Some(checkpoint);
        agent.last_activity_at = Instant::now();
        // #866: a per-step checkpoint is real progress. A paused/blocked
        // agent that is producing checkpoints is NOT stalled, so refresh the
        // lifecycle tracker timestamp without changing the coarse state.
        if let Some(state) = self.lifecycle_tracker.state(&agent.id) {
            self.lifecycle_tracker.set(&agent.id, state);
        }
        // #freeze: hot per-step path — coalesce the full-fleet persist so 20
        // agents stepping concurrently do not serialize the whole fleet (with
        // full transcripts) to disk under the write lock on every step.
        self.persist_state_debounced();
        true
    }

    pub(crate) fn interrupt_with_checkpoint(
        &mut self,
        agent_id: &str,
        reason: String,
        checkpoint: SubAgentCheckpoint,
        needs_input: Option<SubAgentNeedsInput>,
    ) -> Result<SubAgentResult> {
        let snapshot = {
            let agent = self
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;
            agent.status = SubAgentStatus::Interrupted(reason.clone());
            agent.result = Some(reason);
            agent.steps_taken = checkpoint.steps_taken;
            agent.checkpoint = Some(checkpoint);
            agent.needs_input = needs_input;
            agent.last_activity_at = Instant::now();
            release_resident_leases_for(agent_id);
            agent.snapshot()
        };
        // #864: an interrupt that is waiting on human input/approval is
        // `Blocked`; a plain interrupt (no input requested) is terminal → Done.
        if snapshot.needs_input.is_some() {
            self.lifecycle_tracker.set(
                agent_id,
                AgentLifecycleState::Blocked(
                    snapshot
                        .result
                        .clone()
                        .unwrap_or_else(|| "interrupted".to_string()),
                ),
            );
        } else {
            self.lifecycle_tracker
                .set(agent_id, AgentLifecycleState::Done);
        }
        self.record_worker_event(
            agent_id,
            AgentWorkerStatus::Interrupted,
            snapshot.result.clone(),
            Some(snapshot.steps_taken),
            None,
        );
        self.persist_state_best_effort();
        Ok(snapshot)
    }
}

// === Unified lifecycle state machine (#864) + blocking wait (#865) ===
// === + stalled detection (#866) + notifications (#867) ===

impl SubAgentManager {
    /// Query the current coarse lifecycle state of a spawned agent (#864).
    ///
    /// Returns `None` if the agent id is unknown (never spawned, or already
    /// evicted). This is a cheap query that does not lock the manager's full
    /// `agents` map.
    #[must_use]
    pub fn agent_state(&self, id: &str) -> Option<AgentLifecycleState> {
        self.lifecycle_tracker.state(id)
    }

    /// Snapshot every tracked agent's id → lifecycle state (#864).
    ///
    /// Cheap and lock-free with respect to the manager's `agents` map; safe
    /// to call from the engine turn loop or a UI renderer.
    #[must_use]
    pub fn all_agent_states(&self) -> HashMap<String, AgentLifecycleState> {
        self.lifecycle_tracker.all_states()
    }

    /// Blocking-aware wait (#865).
    ///
    /// Polls the [`LifecycleTracker`] until `id` actually reaches `cond`
    /// (`Blocked` or `Done`), or `timeout` elapses. This is NOT a fixed
    /// `join`: it returns the moment the agent's coarse lifecycle satisfies
    /// the condition, so a parent can wait for "child is truly blocked and
    /// needs my input" rather than spinning on wall-clock.
    ///
    /// Returns `Err` (carrying the last observed state) when the agent is
    /// unknown or the timeout elapses first.
    pub fn wait_until(
        &self,
        id: &str,
        cond: WaitCond,
        timeout: Duration,
    ) -> Result<AgentLifecycleState> {
        let start = Instant::now();
        // Poll cadence is a small fraction of the timeout, clamped so a very
        // short timeout still polls a few times.
        let poll = (timeout / 20).clamp(Duration::from_millis(5), Duration::from_millis(250));
        loop {
            match self.lifecycle_tracker.state(id) {
                None => {
                    if start.elapsed() >= timeout {
                        return Err(anyhow!("agent {id} unknown while waiting"));
                    }
                }
                Some(state @ AgentLifecycleState::Blocked(_)) if cond == WaitCond::Blocked => {
                    return Ok(state);
                }
                Some(state @ AgentLifecycleState::Done) if cond == WaitCond::Done => {
                    return Ok(state);
                }
                Some(other) => {
                    if start.elapsed() >= timeout {
                        return Err(anyhow!(
                            "timed out waiting for {cond:?} on agent {id} (last state: {})",
                            other.as_str()
                        ));
                    }
                }
            }
            if start.elapsed() >= timeout {
                // Final re-check after the loop body in case the state moved
                // during the sleep below.
                if let Some(state) = self.lifecycle_tracker.state(id)
                    && matches!(
                        (&state, cond),
                        (AgentLifecycleState::Blocked(_), WaitCond::Blocked)
                            | (AgentLifecycleState::Done, WaitCond::Done)
                    )
                {
                    return Ok(state);
                }
                break;
            }
            std::thread::sleep(poll);
        }
        Err(anyhow!("timed out waiting for {cond:?} on agent {id}"))
    }

    /// Stalled detection (#866).
    ///
    /// Fails fast if `id` has shown NO lifecycle-state change within `window`
    /// after a prompt/instruction was issued. Distinct from a long wall-clock
    /// timeout: a long-running but *progressing* agent (which bumps its
    /// state periodically) is never flagged; only a silent one is.
    ///
    /// Returns `Ok(())` when the agent is still making lifecycle progress,
    /// `Err` enumerating the stall reason + elapsed time otherwise.
    pub fn assert_not_stalled(&self, id: &str, window: Duration) -> Result<()> {
        match self.stalled_detector.assert_not_stalled(id, window) {
            Ok(()) => Ok(()),
            Err(elapsed) => Err(anyhow!(
                "agent {id} appears stalled: no lifecycle-state change in {}ms (window {}ms)",
                elapsed.as_millis(),
                window.as_millis()
            )),
        }
    }

    /// Push an external notification from an agent (#867).
    ///
    /// Agents call this when they are blocked and need human input, or on
    /// completion, so the orchestrator/shell can surface it. The sink is
    /// pluggable; the default writes to stderr (no network deps).
    pub fn notify(&self, id: &str, msg: &str) {
        self.notifier.notify(id, msg);
    }

    /// Replace the notification sink (#867). Primarily for tests and
    /// alternative delivery backends; the default sink writes to stderr.
    pub fn set_notifier(&mut self, notifier: Notifier) {
        self.notifier = notifier;
    }
}

impl Drop for SubAgentManager {
    fn drop(&mut self) {
        // Best-effort reclamation of any isolated worktrees still tracked at
        // manager teardown (e.g. a parent exiting within the retention window).
        // `remove_worktree` swallows failures, so this never panics. Mirrors the
        // cleanup done in `cleanup` / auto-cancel (#691).
        for agent in self.agents.values() {
            if let Some(path) = agent.worktree_path.as_ref() {
                remove_worktree(path);
            }
        }
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::tools::subagent::notify::{CapturingSink, NotificationSink};

    fn test_manager() -> SubAgentManager {
        let dir = std::env::temp_dir().join(format!("mimofan-lifecycle-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).ok();
        SubAgentManager::new(dir, 4)
    }

    #[test]
    fn state_transitions_idle_to_done() {
        let mut mgr = test_manager();
        mgr.lifecycle_tracker.register("a1");
        assert_eq!(mgr.agent_state("a1"), Some(AgentLifecycleState::Idle));

        mgr.lifecycle_tracker
            .set("a1", AgentLifecycleState::Working);
        assert_eq!(mgr.agent_state("a1"), Some(AgentLifecycleState::Working));

        mgr.lifecycle_tracker.set(
            "a1",
            AgentLifecycleState::Blocked("needs_approval".to_string()),
        );
        match mgr.agent_state("a1") {
            Some(AgentLifecycleState::Blocked(reason)) => assert_eq!(reason, "needs_approval"),
            other => panic!("expected Blocked, got {other:?}"),
        }

        mgr.lifecycle_tracker.set("a1", AgentLifecycleState::Done);
        assert_eq!(mgr.agent_state("a1"), Some(AgentLifecycleState::Done));

        // Snapshot reflects both registered + transitioned entries.
        mgr.lifecycle_tracker.register("a2");
        let all = mgr.all_agent_states();
        assert_eq!(all["a1"], AgentLifecycleState::Done);
        assert_eq!(all["a2"], AgentLifecycleState::Idle);
    }

    #[test]
    fn wait_until_returns_on_blocked() {
        let mgr = test_manager();
        mgr.lifecycle_tracker
            .set("b1", AgentLifecycleState::Working);
        // Transition to Blocked shortly after, in a background thread.
        let tracker = mgr.lifecycle_tracker.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            tracker.set("b1", AgentLifecycleState::Blocked("input".to_string()));
        });
        let result = mgr.wait_until("b1", WaitCond::Blocked, Duration::from_secs(2));
        match result {
            Ok(AgentLifecycleState::Blocked(reason)) => assert_eq!(reason, "input"),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn wait_until_times_out_when_not_blocked() {
        let mgr = test_manager();
        mgr.lifecycle_tracker
            .set("c1", AgentLifecycleState::Working);
        let result = mgr.wait_until("c1", WaitCond::Blocked, Duration::from_millis(80));
        assert!(result.is_err(), "should time out while only Working");
    }

    #[test]
    fn stalled_detector_fires_on_no_change() {
        let mgr = test_manager();
        mgr.lifecycle_tracker
            .set("s1", AgentLifecycleState::Working);
        // Let the last-change timestamp age past the window.
        std::thread::sleep(Duration::from_millis(20));
        let result = mgr.assert_not_stalled("s1", Duration::from_millis(5));
        assert!(result.is_err(), "expected stalled detection to fire");
    }

    #[test]
    fn stalled_detector_ok_on_recent_change() {
        let mgr = test_manager();
        mgr.lifecycle_tracker
            .set("s2", AgentLifecycleState::Working);
        // Re-set immediately to refresh the timestamp, then check wide window.
        mgr.lifecycle_tracker
            .set("s2", AgentLifecycleState::Working);
        let result = mgr.assert_not_stalled("s2", Duration::from_millis(50));
        assert!(result.is_ok(), "recent change must not be flagged stalled");
    }

    #[test]
    fn notifier_invokes_sink() {
        let mut mgr = test_manager();
        let sink = Arc::new(CapturingSink::default());
        mgr.set_notifier(Notifier::with_sink(
            Arc::clone(&sink) as Arc<dyn NotificationSink>
        ));
        mgr.notify("n1", "blocked: needs approval");
        assert!(sink.was_notified("n1"));
        let captured = sink.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "n1");
        assert_eq!(captured[0].1, "blocked: needs approval");
    }
}
