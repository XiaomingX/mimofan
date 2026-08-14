use std::collections::{HashMap, HashSet, VecDeque};

use mimofan_config::ProviderChain;

use crate::config::{
    ApiProvider, Config, DEFAULT_TEXT_MODEL, SavedCredential, has_api_key, has_api_key_for,
    save_api_key,
};
use crate::cost_budget::{CostBudgetKind, CostBudgetLevel};
use crate::hooks::{HookContext, HookEvent, HookExecutor, HookResult};
use crate::localization::{MessageId, resolve_locale, tr};
use crate::models::{auto_compact_default_for_model, compaction_threshold_for_model_at_percent};
use crate::palette;
use crate::pricing::{CostCurrency, CostEstimate};
use crate::session_manager::PlanAndTodoState;
use crate::settings::Settings;
use crate::tools::plan::new_shared_plan_state;
use crate::tools::shell::new_shared_shell_manager;
use crate::tools::spec::RuntimeToolServices;
use crate::tools::todo::new_shared_todo_list;
use crate::tui::approval::ApprovalMode;
use crate::tui::clipboard::ClipboardHandler;
use crate::tui::composer::ComposerState;
use crate::tui::history::HistoryCell;
use crate::tui::history_search::HuntState;
use crate::tui::hotbar::HotbarActionRegistry;
use crate::tui::paste_burst::PasteBurst;
use crate::tui::state::{SessionState, SidebarHoverState};
use crate::tui::streaming::StreamingState;
use crate::tui::tool_collapse::ToolCollapseMode;
use crate::tui::viewport::ViewportState;
use crate::tui::views::ViewStack;
use crate::tui::vim::VimMode;

use super::helpers::{
    base_policy_for_mode, default_composer_arrows_scroll, initial_onboarding_state,
    onboarding_is_workspace_trust_gate, resolve_skills_dir,
};
use super::state::{
    ApiKeyError, App, AppMode, ComposerDensity, InitialInput, ModeSessionPrefs, OnboardingState,
    ReasoningEffort, SidebarFocus, StatusToastLevel, TranscriptSpacing, TuiOptions,
    TurnCacheRecord,
};

// === Constants ===

const MAX_SUBMITTED_INPUT_CHARS: usize = 16_000;
const MAX_COMPOSER_DISPLAY_CHARS: usize = 4_000;
const MAX_DRAFT_HISTORY: usize = 50;

impl App {
    /// Cap on the session turn-cache history. Holds enough turns to debug a long
    /// session without being so large the on-screen `/cache` table wraps.
    pub const TURN_CACHE_HISTORY_CAP: usize = 50;

    /// Append a per-turn cache-telemetry record, trimming the oldest entry once
    /// the ring exceeds [`Self::TURN_CACHE_HISTORY_CAP`].
    pub fn push_turn_cache_record(&mut self, record: TurnCacheRecord) {
        self.session.turn_cache_history.push_back(record);
        while self.session.turn_cache_history.len() > Self::TURN_CACHE_HISTORY_CAP {
            self.session.turn_cache_history.pop_front();
        }
    }

    /// Capture the current plan and todo (checklist) state for persistence.
    ///
    /// `plan_state`/`todos` are `tokio::sync::Mutex`, so this uses
    /// [`tokio::sync::Mutex::try_lock`] (non-blocking) to stay safe inside both
    /// synchronous and `.await`-ing callers — callers must never hold either
    /// guard across an `.await` (ARCHITECTURE_STABILITY.md §8.3). If either lock
    /// is momentarily contended, that field is omitted (best-effort capture;
    /// the next save will include it).
    pub fn current_plan_and_todo(&self) -> PlanAndTodoState {
        let plan = self
            .plan_state
            .try_lock()
            .ok()
            .map(|guard| guard.snapshot());
        let todos = self.todos.try_lock().ok().map(|guard| guard.snapshot());
        PlanAndTodoState {
            plan: plan.filter(|p| !p.is_empty()),
            todos: todos.filter(|t| !t.items.is_empty()),
        }
    }

    pub(crate) fn clear_model_scoped_telemetry(&mut self) {
        self.session.last_prompt_tokens = None;
        self.session.last_completion_tokens = None;
        self.session.last_output_throughput = None;
        self.session.last_ttft = None;
        self.session.last_prompt_cache_hit_tokens = None;
        self.session.last_prompt_cache_miss_tokens = None;
        self.session.last_reasoning_replay_tokens = None;
        self.session.turn_cache_history.clear();
        self.last_pinned_prefix_hash = None;
    }

    pub fn tr(&self, id: MessageId) -> &'static str {
        tr(self.ui_locale, id)
    }

    #[allow(clippy::too_many_lines)]
    pub fn new(options: TuiOptions, config: &Config) -> Self {
        let TuiOptions {
            model,
            workspace,
            config_path,
            config_profile,
            allow_shell,
            use_alt_screen,
            use_mouse_capture,
            use_bracketed_paste,
            max_subagents,
            skills_dir: global_skills_dir,
            memory_dir,
            notes_path: _,
            mcp_config_path,
            use_memory,
            start_in_agent_mode,
            skip_onboarding,
            yolo,
            resume_session_id: _,
            initial_input,
        } = options;

        let settings = Settings::load().unwrap_or_else(|_| Settings::default());

        // If settings.json exists on disk but couldn't be parsed (we fell back
        // to defaults), surface a warning in the TUI so the user knows their
        // file is broken instead of silently losing all settings.
        let settings_parse_warning = crate::settings::Settings::path().ok().and_then(|p| {
            if p.exists() {
                std::fs::read_to_string(&p).ok().and_then(|raw| {
                    ::serde_json::from_str::<::serde_json::Value>(&raw)
                        .err()
                        .map(|e| format!("⚠ settings.json is malformed — using defaults ({e})"))
                })
            } else {
                None
            }
        });
        let tui_prefs_warning = crate::settings::TuiPrefs::path().ok().and_then(|p| {
            if p.exists() {
                std::fs::read_to_string(&p).ok().and_then(|raw| {
                    ::serde_json::from_str::<::serde_json::Value>(&raw)
                        .err()
                        .map(|e| format!("⚠ tui.json is malformed — using defaults ({e})"))
                })
            } else {
                None
            }
        });

        let mut provider = config.api_provider();

        // Let settings preserve runtime switches only when config/CLI did not
        // explicitly select a provider. A configured provider must not be
        // pushed back to a stale saved setting on restart.
        if config
            .provider
            .as_deref()
            .and_then(ApiProvider::parse)
            .is_none()
            && let Some(ref provider_str) = settings.default_provider
            && let Some(parsed) = ApiProvider::parse(provider_str)
        {
            provider = parsed;
        }
        let mut effective_auth_config = config.clone();
        effective_auth_config.provider = Some(provider.as_str().to_string());
        let model_ids_passthrough = effective_auth_config.model_ids_pass_through();
        let provider_chain = provider
            .kind()
            .map(|kind| ProviderChain::new(kind, &config.fallback_providers))
            .filter(|chain| chain.providers().len() > 1);

        // Snapshot per-provider readiness for the fallback chain (#2574). Uses
        // the same `has_api_key_for` helper the provider picker uses, so hosted
        // providers require a key and self-hosted ones (Ollama/vLLM/SGLang) are
        // reported ready without one. Empty when there is no fallback chain.
        let provider_readiness = provider_chain
            .as_ref()
            .map(|chain| {
                chain
                    .providers()
                    .iter()
                    .map(|kind| {
                        let provider = ApiProvider::from_kind(*kind);
                        (provider, has_api_key_for(config, provider))
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Check if the effective provider has an API key. This must happen
        // after settings.default_provider is applied; otherwise a saved
        // third-party provider can be pushed back into DeepSeek onboarding.
        let needs_api_key = !has_api_key(&effective_auth_config);
        let api_key_env_only =
            crate::config::active_provider_uses_env_only_api_key(&effective_auth_config);
        let was_onboarded = crate::tui::onboarding::is_onboarded();
        let settings_auto_compact = settings.auto_compact;
        let auto_compact_user_configured = Settings::auto_compact_explicitly_configured();
        let compact_threshold_percent = settings.compact_threshold;
        let calm_mode = settings.calm_mode;
        let low_motion = settings.low_motion;
        let fancy_animations = settings.fancy_animations;
        let synchronized_output_enabled = settings.synchronized_output_enabled();
        let status_indicator = settings.status_indicator.clone();
        let show_thinking = settings.show_thinking;
        let show_tool_details = settings.show_tool_details;
        let ui_locale = resolve_locale(&settings.locale);
        let cost_currency = match (settings.cost_currency.as_str(), ui_locale.tag()) {
            ("usd", "zh-Hans") => CostCurrency::Cny,
            _ => CostCurrency::from_setting(&settings.cost_currency).unwrap_or(CostCurrency::Usd),
        };
        let cost_budget = crate::cost_budget::CostBudget::from_toml(
            config
                .cost_budget
                .as_ref()
                .unwrap_or(&mimofan_config::CostBudgetToml::default()),
        );
        let composer_density = ComposerDensity::from_setting(&settings.composer_density);
        let composer_border = settings.composer_border;
        let composer_vim_enabled = settings.vim_mode.trim().eq_ignore_ascii_case("vim");
        let transcript_spacing = TranscriptSpacing::from_setting(&settings.transcript_spacing);
        let sidebar_width = settings.sidebar_width;
        let sidebar_focus = SidebarFocus::from_setting(&settings.sidebar_focus);
        let max_input_history = settings.max_input_history;
        let use_paste_burst_detection = settings.paste_burst_detection;
        // Resolve the named theme from settings; unknown values were already
        // normalised to "system" in Settings::load. The background_color
        // setting still overlays on top.
        let theme_id =
            palette::ThemeId::from_name(&settings.theme).unwrap_or(palette::ThemeId::System);
        let mut ui_theme = theme_id.ui_theme();
        if let Some(background) = settings
            .background_color
            .as_deref()
            .and_then(palette::parse_hex_rgb_color)
        {
            ui_theme = ui_theme.with_background_color(background);
        }
        let provider_models = settings.provider_models.clone().unwrap_or_default();
        let model = provider_models
            .get(provider.as_str())
            .cloned()
            .or_else(|| {
                // default_model is a DeepSeek-centric setting; other providers
                // get their model from config.toml / env (e.g. OPENAI_MODEL).
                if matches!(provider, ApiProvider::OpenAiCompatible) {
                    settings.default_model.clone()
                } else {
                    None
                }
            })
            .unwrap_or(model);
        let auto_model = model.trim().eq_ignore_ascii_case("auto");
        let configured_reasoning_effort = settings
            .reasoning_effort
            .as_deref()
            .or_else(|| config.reasoning_effort());
        let threshold_model = if auto_model {
            DEFAULT_TEXT_MODEL
        } else {
            model.as_str()
        };
        let compact_threshold =
            compaction_threshold_for_model_at_percent(threshold_model, compact_threshold_percent);
        let compact_instructions =
            crate::project_context::load_project_context(&workspace).compact_instructions();
        let auto_compact = if auto_compact_user_configured {
            settings_auto_compact
        } else {
            auto_compact_default_for_model(threshold_model)
        };
        let reasoning_effort = if auto_model {
            ReasoningEffort::Auto
        } else {
            configured_reasoning_effort.map_or_else(ReasoningEffort::default, |s| {
                ReasoningEffort::from_setting_for_provider(s, provider)
            })
        };

        // `settings.fast_mode` makes `/fast` the startup default. Resolve the
        // same cheap-tier model and low effort `/fast` would pick, so the
        // persisted preference and the runtime command cannot drift apart. The
        // pre-fast model/effort are recorded as the `/normal` restore point.
        let start_fast = settings.fast_mode;
        let (model, auto_model, reasoning_effort, fast_saved_model, fast_saved_effort) =
            if start_fast {
                let candidates = crate::model_routing::provider_router_candidates(provider, &model);
                let cheap = candidates.cheap_or_big().to_string();
                (
                    cheap,
                    false,
                    ReasoningEffort::Low,
                    Some(model),
                    Some(reasoning_effort),
                )
            } else {
                (model, auto_model, reasoning_effort, None, None)
            };

        // Start in YOLO mode if --yolo flag was passed
        let preferred_mode = AppMode::from_setting(&settings.default_mode);
        let initial_mode = if yolo {
            AppMode::Yolo
        } else if start_in_agent_mode {
            AppMode::Agent
        } else {
            preferred_mode
        };
        let needs_workspace_trust =
            initial_mode != AppMode::Yolo && crate::tui::onboarding::needs_trust(&workspace);
        let onboarding = initial_onboarding_state(
            skip_onboarding,
            was_onboarded,
            needs_api_key,
            needs_workspace_trust,
        );
        let onboarding_workspace_trust_gate = onboarding_is_workspace_trust_gate(
            skip_onboarding,
            was_onboarded,
            needs_api_key,
            needs_workspace_trust,
        );

        // Durable Agent-era permission baseline (#3386). Plan/YOLO derive from
        // and restore to this. When the user starts in YOLO the live shell flag
        // is force-enabled below, so the baseline shell value is taken from
        // config (the pre-YOLO surface) rather than the live mirror; otherwise
        // it mirrors the resolved `allow_shell` option. Trust is never part of
        // the Agent baseline (it is YOLO-only authority). Approval mirrors the
        // configured policy. This preserves the exact values the previous
        // `YoloRestoreState`/`PlanRestoreState` snapshots restored.
        let configured_approval_mode = config
            .approval_policy
            .as_deref()
            .and_then(ApprovalMode::from_config_value)
            .unwrap_or_default();
        let mode_prefs = ModeSessionPrefs {
            agent_allow_shell: if initial_mode == AppMode::Yolo {
                config.allow_shell()
            } else {
                allow_shell
            },
            agent_trust_mode: false,
            agent_approval_mode: configured_approval_mode,
        };
        let allow_shell = allow_shell || initial_mode == AppMode::Yolo;
        let shell_manager = new_shared_shell_manager(workspace.clone());

        // Initialize hooks executor from config, merged with project-local
        // `.mimofan/hooks.toml` (#3026).
        let hooks_config =
            crate::hooks::HooksConfig::load_with_project(config.hooks_config(), &workspace);
        let hooks = HookExecutor::new(hooks_config, workspace.clone());

        // Initialize plan state
        let plan_state = new_shared_plan_state();

        let skills_scan_mimofan_only = config.skills_config().scan_mimofan_only();
        let skills_dir = resolve_skills_dir(&workspace, &global_skills_dir, config);
        let cached_skills =
            Self::discover_cached_skills(&workspace, &skills_dir, skills_scan_mimofan_only);

        let input_history = crate::composer_history::load_history();
        let (initial_input_text, initial_input_cursor, auto_submit_initial_input) =
            match initial_input {
                // #451: pre-populate the composer when invoked via
                // `deepseek pr <N>` (or any future caller that wants to
                // drop the model into a session with context already
                // typed). Cursor lands at the end so Enter sends as-is.
                Some(InitialInput::Prefill(text)) if !text.is_empty() => {
                    let cursor = text.chars().count();
                    (text, cursor, false)
                }
                Some(InitialInput::Submit(text)) if !text.is_empty() => {
                    let cursor = text.chars().count();
                    (text, cursor, true)
                }
                _ => (String::new(), 0, false),
            };
        let mcp_configured_count =
            crate::mcp::load_config_with_workspace(&mcp_config_path, &workspace)
                .map(|cfg| cfg.servers.len())
                .unwrap_or(0);
        Self {
            mode: initial_mode,
            hotbar_actions: HotbarActionRegistry::with_builtins(),
            composer: ComposerState {
                input: initial_input_text,
                cursor_position: initial_input_cursor,
                kill_buffer: String::new(),
                paste_burst: PasteBurst::default(),
                pending_paste_reference: None,
                oversized_paste_full_text: None,
                input_history,
                draft_history: VecDeque::new(),
                clear_undo_buffer: None,
                history_index: None,
                history_navigation_draft: None,
                composer_history_search: None,
                selected_attachment_index: None,
                slash_menu_selected: 0,
                slash_menu_hidden: false,
                mention_menu_selected: 0,
                mention_menu_hidden: false,
                mention_completion_cache: None,
                vim_enabled: composer_vim_enabled,
                vim_mode: VimMode::Normal,
                vim_pending_d: false,
                selection_anchor: None,
            },
            viewport: ViewportState::default(),
            hunt: HuntState::default(),
            session: SessionState::default(),
            active_allowed_tools: None,
            pausable: false,
            paused: false,
            paused_quarry: None,
            history: Vec::new(),
            history_version: 0,
            history_revisions: Vec::new(),
            next_history_revision: 1,
            api_messages: Vec::new(),
            is_loading: false,
            provider_wait_incident_logged: false,
            prompt_suggestion: None,
            prompt_suggestion_gen: std::sync::atomic::AtomicU64::new(0),
            offline_mode: false,
            turn_error_posted: false,
            // Surface parse warnings so the user knows their config file is
            // broken instead of silently losing all settings.
            status_message: settings_parse_warning.or(tui_prefs_warning),
            status_toasts: VecDeque::new(),
            sticky_status: None,
            last_status_message_seen: None,
            model,
            provider_models,
            auto_model,
            last_effective_model: None,
            api_provider: provider,
            provider_chain,
            provider_readiness,
            last_fallback_reason: None,
            model_ids_passthrough,
            active_route_limits: None,
            pending_provider_switch: None,
            catalog_cache: std::sync::Arc::new(std::sync::Mutex::new(
                crate::config_persistence::load_catalog_cache(),
            )),
            reasoning_effort,
            last_effective_reasoning_effort: None,
            fast_mode_active: start_fast,
            fast_saved_model,
            fast_saved_effort,
            workspace,
            config_path,
            config_profile,
            mcp_config_path: mcp_config_path.clone(),
            skills_dir,
            skills_scan_mimofan_only,
            memory_dir,
            use_memory,
            use_alt_screen,
            use_mouse_capture,
            use_bracketed_paste,
            use_paste_burst_detection,
            bracketed_paste_seen: false,
            system_prompt: None,
            auto_compact,
            auto_compact_user_configured,
            compact_threshold_percent,
            calm_mode,
            low_motion,
            fancy_animations,
            synchronized_output_enabled,
            status_indicator,
            show_thinking,
            verbose_transcript: false,
            show_tool_details,
            ui_locale,
            cost_currency,
            cost_budget,
            daily_cost_usd: 0.0,
            daily_cost_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
            cost_budget_alerts: std::collections::HashMap::new(),
            composer_density,
            composer_border,
            voice_enabled: false,
            voice_send_enabled: false,
            voice_control_enabled: false,
            transcript_spacing,
            sidebar_width,
            sidebar_focus,
            sidebar_hover: SidebarHoverState::default(),
            sidebar_hover_tooltip: None,
            cached_work_summary: None,
            last_mouse_pos: None,
            sidebar_resizing: false,
            sidebar_resize_anchor_x: 0,
            sidebar_resize_anchor_width: 0,
            last_sidebar_area: None,
            last_sidebar_host_width: None,
            last_sidebar_handle_area: None,
            sidebar_resize_total_width: 0,
            sidebar_width_dirty: false,
            sidebar_focus_dirty: false,
            context_panel: settings.context_panel,
            tool_collapse_threshold: 3,
            expanded_tool_runs: HashSet::new(),
            tool_collapse_mode: ToolCollapseMode::from_setting(&settings.tool_collapse),
            file_tree: None,
            file_tree_visible: false,
            compact_threshold,
            compact_instructions,
            max_input_history,
            allow_shell,
            verbosity: config.verbosity.clone(),
            max_subagents,
            stream_chunk_timeout_secs: config.stream_chunk_timeout_secs(),
            subagent_cache: Vec::new(),
            subagent_terminal_seen_at: HashMap::new(),
            agent_progress: HashMap::new(),
            agent_progress_meta: HashMap::new(),
            subagent_card_index: HashMap::new(),
            last_fanout_card_index: None,
            pending_subagent_dispatch: None,
            agent_activity_started_at: None,
            agent_counter: 0,
            agent_label_map: HashMap::new(),
            last_agent_progress_redraw: None,
            ui_theme,
            theme_id,
            onboarding,
            onboarding_needs_api_key: needs_api_key,
            onboarding_workspace_trust_gate,
            api_key_env_only,
            api_key_input: String::new(),
            api_key_cursor: 0,
            hooks,
            yolo: initial_mode == AppMode::Yolo,
            mode_prefs,
            clipboard: ClipboardHandler::new(),
            approval_session_approved: HashSet::new(),
            approval_session_denied: HashSet::new(),
            approval_mode: if matches!(initial_mode, AppMode::Yolo) {
                ApprovalMode::Auto
            } else {
                config
                    .approval_policy
                    .as_deref()
                    .and_then(ApprovalMode::from_config_value)
                    .unwrap_or_default()
            },
            view_stack: ViewStack::new(),
            backtrack: crate::tui::backtrack::BacktrackState::new(),
            current_session_id: None,
            session_artifacts: Vec::new(),
            trust_mode: initial_mode == AppMode::Yolo,
            translation_enabled: false,
            status_items: config
                .tui
                .as_ref()
                .and_then(|tui| tui.status_items.clone())
                .unwrap_or_else(crate::config::StatusItem::default_footer),
            project_doc: None,
            plan_state,
            plan_prompt_pending: false,
            plan_tool_used_in_turn: false,
            todos: new_shared_todo_list(),
            runtime_services: RuntimeToolServices {
                shell_manager: Some(shell_manager),
                ..RuntimeToolServices::default()
            },
            mcp_snapshot: None,
            // Read the MCP config once at boot to know how many servers
            // the user has declared. The footer chip uses this even when
            // no live snapshot is available (#502). Cheap (just reads
            // the JSON files); errors fall through to zero so a missing
            // or malformed config simply hides the chip.
            mcp_configured_count,
            mcp_restart_required: false,
            tool_log: Vec::new(),
            active_skill: None,
            cached_skills,
            tool_cells: HashMap::new(),
            tool_details_by_cell: HashMap::new(),
            context_references_by_cell: HashMap::new(),
            session_context_references: Vec::new(),
            active_cell: None,
            active_cell_revision: 0,
            active_tool_details: HashMap::new(),
            active_tool_entry_completed_at: HashMap::new(),
            exploring_cell: None,
            exploring_entries: HashMap::new(),
            ignored_tool_calls: HashSet::new(),
            last_exec_wait_command: None,
            streaming_message_index: None,
            suppress_stream_events_until_turn_complete: false,
            streaming_thinking_active_entry: None,
            thinking_revision_last_bump_at: None,
            streaming_state: StreamingState::new(),
            streaming_output_token_estimate: 0,
            reasoning_buffer: String::new(),
            reasoning_header: None,
            last_reasoning: None,
            pending_tool_uses: Vec::new(),
            queued_messages: VecDeque::new(),
            queued_draft: None,
            pending_steers: VecDeque::new(),
            rejected_steers: VecDeque::new(),
            submit_pending_steers_after_interrupt: false,
            turn_started_at: None,
            turn_first_token_at: None,
            turn_last_activity_at: None,
            cumulative_turn_duration: std::time::Duration::ZERO,
            balance_cell: std::sync::Arc::new(std::sync::Mutex::new(None)),
            prompt_suggestion_cell: std::sync::Arc::new(std::sync::Mutex::new(None)),
            balance_initiated: false,
            last_balance_fetch: None,
            runtime_turn_id: None,
            runtime_turn_status: None,
            turn_counter: 0,
            dispatch_started_at: None,
            workspace_context: None,
            workspace_context_cell: std::sync::Arc::new(std::sync::Mutex::new(None)),
            workspace_context_refreshed_at: None,
            task_panel: Vec::new(),
            decision_card: None,
            session_started_at: chrono::Utc::now(),
            needs_redraw: true,
            force_next_full_repaint: false,
            thinking_started_at: None,
            is_compacting: false,
            is_purging: false,
            user_scrolled_during_stream: false,
            last_send_at: None,
            last_submitted_prompt: None,
            auto_submit_initial_input,
            quit_armed_until: None,
            prefix_change_count: 0,
            prefix_checks_total: 0,
            prefix_stability_pct: None,
            last_prefix_change_desc: None,
            last_pinned_prefix_hash: None,
            collapsed_cells: HashSet::new(),
            folded_thinking: HashSet::new(),
            collapsed_cell_map: Vec::new(),
            edit_in_progress: false,
            lsp_enabled: config.lsp.as_ref().and_then(|l| l.enabled).unwrap_or(true),
            composer_arrows_scroll: config
                .tui
                .as_ref()
                .and_then(|tui| tui.composer_arrows_scroll)
                .unwrap_or_else(|| default_composer_arrows_scroll(use_mouse_capture)),
            mention_limit: settings.mention_limit,
            mention_depth: settings.mention_depth,
            mention_behavior: settings.mention_behavior.clone(),
            workspace_follow_symlinks: settings.workspace_follow_symlinks,
            session_title: None,
            receipt_text: None,
            receipt_started_at: None,
            tool_evidence: Vec::new(),
            spec_frozen: false,
            frozen_spec: None,
        }
    }

    fn discover_cached_skills(
        workspace: &std::path::Path,
        skills_dir: &std::path::Path,
        scan_mimofan_only: bool,
    ) -> Vec<(String, String)> {
        crate::skills::discover_for_workspace_and_dir_with_mode(
            workspace,
            skills_dir,
            crate::skills::SkillDiscoveryMode::from_mimofan_only(scan_mimofan_only),
        )
        .list()
        .iter()
        .map(|s| (s.name.clone(), s.description.clone()))
        .collect()
    }

    pub fn refresh_skill_cache(&mut self) {
        let skills_dir = self.skills_dir.clone();
        self.cached_skills = Self::discover_cached_skills(
            &self.workspace,
            &skills_dir,
            self.skills_scan_mimofan_only,
        );
    }

    pub fn submit_api_key(&mut self) -> Result<SavedCredential, ApiKeyError> {
        let key = self.api_key_input.trim().to_string();
        if key.is_empty() {
            return Err(ApiKeyError::Empty);
        }

        match save_api_key(&key) {
            Ok(saved) => {
                self.api_key_input.clear();
                self.api_key_cursor = 0;
                self.onboarding_needs_api_key = false;
                self.api_key_env_only = false;
                Ok(saved)
            }
            Err(source) => Err(ApiKeyError::SaveFailed { source }),
        }
    }

    pub fn finish_onboarding(&mut self) {
        self.onboarding = OnboardingState::None;
        if let Err(err) = crate::tui::onboarding::mark_onboarded() {
            self.status_message = Some(format!("Failed to mark onboarding: {err}"));
        }
        self.needs_redraw = true;
    }

    /// Apply a locale tag selected from the onboarding language picker (#566).
    /// Persists the value to settings.json and immediately
    /// re-resolves `ui_locale` so the rest of onboarding renders in the new
    /// language. `App` doesn't keep `Settings` resident — it loads on entry
    /// and rewrites on exit, mirroring the pattern used by the `/config`
    /// surface.
    pub fn set_locale_from_onboarding(&mut self, tag: &str) -> anyhow::Result<()> {
        let mut settings = Settings::load().unwrap_or_else(|_| Settings::default());
        settings.set("locale", tag)?;
        settings.save()?;
        self.ui_locale = crate::localization::resolve_locale(&settings.locale);
        self.needs_redraw = true;
        Ok(())
    }

    /// Locale tag currently persisted in settings.json (or
    /// `"auto"` when no settings file exists). Used by the onboarding
    /// language picker to highlight the current selection without `App`
    /// having to keep `Settings` resident.
    pub fn current_locale_tag(&self) -> String {
        Settings::load()
            .map(|s| s.locale)
            .unwrap_or_else(|_| "auto".to_string())
    }

    pub fn set_mode(&mut self, mode: AppMode) -> bool {
        let previous_mode = self.mode;
        if previous_mode == mode {
            return false;
        }

        self.mode = mode;
        self.status_message = Some(format!("Switched to {} mode", mode.label()));

        // Mode cycling is untangled from permission policy (#3386). The user
        // only edits the durable permission surface while in Agent mode, so
        // refresh the baseline from the live mirrors whenever we leave Agent —
        // before any transient Plan/YOLO policy overwrites them. This subsumes
        // the old per-mode `YoloRestoreState`/`PlanRestoreState` snapshots:
        // cross-mode hops (Plan -> YOLO, YOLO -> Plan) do not touch the baseline,
        // so YOLO's elevated authority never bleeds into the restored Agent
        // surface (#3279).
        if previous_mode == AppMode::Agent {
            self.mode_prefs = ModeSessionPrefs {
                agent_allow_shell: self.allow_shell,
                agent_trust_mode: self.trust_mode,
                agent_approval_mode: self.approval_mode,
            };
        }

        // Derive the effective permission policy for the incoming mode from the
        // single source of truth and apply it to the live mirrors in one block.
        // Plan's write-blocking still comes from `self.mode` in turn_loop; this
        // also keeps the TUI approval surface (which reads `self.approval_mode`
        // without consulting `self.mode`) consistent with the active mode.
        let policy = base_policy_for_mode(mode, &self.mode_prefs);
        self.allow_shell = policy.allow_shell;
        self.trust_mode = policy.trust_mode;
        self.approval_mode = policy.approval_mode;
        self.yolo = policy.auto_approve;

        if mode != AppMode::Plan {
            self.plan_prompt_pending = false;
            self.plan_tool_used_in_turn = false;
        }

        // Execute mode change hooks
        let context = HookContext::new()
            .with_mode(mode.label())
            .with_previous_mode(previous_mode.label())
            .with_workspace(self.workspace.clone())
            .with_model(&self.model);
        let _ = self.hooks.execute(HookEvent::ModeChange, &context);
        self.needs_redraw = true;
        true
    }

    /// Whether mode/thinking selection is locked because a turn is in flight.
    ///
    /// While `is_loading`, the model/permission surface the engine is acting on
    /// must not shift underneath it, so user-initiated mode and thinking changes
    /// are refused (#2982). Returns true (and posts a concise status message) if
    /// the change should be rejected — the caller leaves the selection unchanged
    /// so the chip "twitches" back instead of moving.
    fn reject_setting_change_while_busy(&mut self, what: &str) -> bool {
        if self.is_loading {
            self.status_message = Some(format!(
                "{what} is locked while a turn is running — press Esc to interrupt first"
            ));
            self.needs_redraw = true;
            true
        } else {
            false
        }
    }

    /// Cycle through modes: Plan → Agent → YOLO → Plan.
    pub fn cycle_mode(&mut self) {
        if self.reject_setting_change_while_busy("Mode") {
            return;
        }
        let next = self.mode.next();
        let _ = self.set_mode(next);
    }

    /// Cycle through modes in reverse.
    pub fn cycle_mode_reverse(&mut self) {
        if self.reject_setting_change_while_busy("Mode") {
            return;
        }
        let next = self.mode.previous();
        let _ = self.set_mode(next);
    }

    /// Cycle reasoning-effort through the active provider's distinct tiers.
    pub fn cycle_effort(&mut self) {
        if self.reject_setting_change_while_busy("Thinking") {
            return;
        }
        self.reasoning_effort = self
            .reasoning_effort
            .cycle_next_for_provider(self.api_provider);
        self.last_effective_reasoning_effort = None;
        self.needs_redraw = true;
        self.push_status_toast(
            format!(
                "Thinking: {}",
                self.reasoning_effort
                    .display_label_for_provider(self.api_provider)
            ),
            StatusToastLevel::Info,
            Some(1_500),
        );
    }

    /// Execute hooks for a specific event with the given context
    pub fn execute_hooks(&self, event: HookEvent, context: &HookContext) -> Vec<HookResult> {
        self.hooks.execute(event, context)
    }

    /// Create a hook context with common fields pre-populated
    pub fn base_hook_context(&self) -> HookContext {
        HookContext::new()
            .with_mode(self.mode.label())
            .with_workspace(self.workspace.clone())
            .with_model(&self.model)
            .with_session_id(self.hooks.session_id())
            .with_tokens(self.session.total_tokens)
    }

    /// Soft cap on [`Self::history`] length. When history exceeds this count,
    /// the oldest cells are folded into a single placeholder to bound memory
    /// and render cost (#399 S2). The cap is generous — 5000 cells is more
    /// than enough to keep the visible transcript intact across sessions.
    pub const HISTORY_SOFT_CAP: usize = 5_000;

    /// Number of oldest cells to fold when the soft cap fires. Folding in
    /// batches amortizes the cost instead of triggering on every push.
    pub(crate) const HISTORY_FOLD_BATCH: usize = 1_000;

    pub fn add_message(&mut self, msg: HistoryCell) {
        let rev = self.fresh_history_revision();
        self.history.push(msg);
        self.history_revisions.push(rev);
        self.history_version = self.history_version.wrapping_add(1);

        // Bound history length: when the soft cap fires, fold the oldest
        // batch into a single ArchivedContext placeholder.
        self.maybe_fold_history();
        let selection_has_range = self
            .viewport
            .transcript_selection
            .ordered_endpoints()
            .is_some_and(|(start, end)| start != end);
        if self.viewport.transcript_scroll.is_at_tail()
            && !self.viewport.transcript_selection.dragging
            && !selection_has_range
            && !self.user_scrolled_during_stream
        {
            self.scroll_to_bottom();
        }
    }

    /// Add `delta` to the parent-turn session cost and bump the displayed
    /// high-water mark so the footer total never reverses (#244).
    pub fn accrue_session_cost(&mut self, delta: f64) {
        self.accrue_session_cost_estimate(CostEstimate::usd_only(delta));
    }

    /// Add a dual-currency parent-turn cost estimate.
    pub fn accrue_session_cost_estimate(&mut self, estimate: CostEstimate) {
        self.session.session_cost += estimate.usd;
        self.session.session_cost_cny += estimate.cny;
        self.refresh_displayed_cost_high_water();
        self.check_cost_budget(
            CostBudgetKind::Session,
            self.session.displayed_cost_high_water,
        );
    }

    /// Add `delta` to the running sub-agent cost and bump the displayed
    /// high-water mark so the footer total never reverses (#244).
    pub fn accrue_subagent_cost(&mut self, delta: f64) {
        self.accrue_subagent_cost_estimate(CostEstimate::usd_only(delta));
    }

    /// Add a dual-currency sub-agent/background cost estimate.
    pub fn accrue_subagent_cost_estimate(&mut self, estimate: CostEstimate) {
        self.session.subagent_cost += estimate.usd;
        self.session.subagent_cost_cny += estimate.cny;
        self.refresh_displayed_cost_high_water();
        // Sub-agent spend counts toward both the session high-water ceiling and
        // the rolling daily ceiling.
        self.check_cost_budget(
            CostBudgetKind::Session,
            self.session.displayed_cost_high_water,
        );
        let daily_total = self.accrue_daily_cost(estimate.usd);
        self.check_cost_budget(CostBudgetKind::Daily, daily_total);
    }

    /// Add `delta` USD to the rolling daily cost accumulator, rolling the day
    /// over (and resetting to `delta`) when the local date changes. Returns the
    /// current daily total in USD.
    fn accrue_daily_cost(&mut self, delta: f64) -> f64 {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if today != self.daily_cost_date {
            self.daily_cost_date = today;
            self.daily_cost_usd = 0.0;
        }
        self.daily_cost_usd += delta;
        self.daily_cost_usd
    }

    /// Evaluate the running cost against the configured budget and surface a
    /// status-toast alert when a threshold is crossed for the first time.
    /// No-op when the budget is inactive (#620). Never blocks the turn.
    fn check_cost_budget(&mut self, kind: CostBudgetKind, current_usd: f64) {
        let Some(alert) = self.cost_budget.evaluate(kind, current_usd) else {
            return;
        };
        let already = self.cost_budget_alerts.get(&kind).copied();
        if already.is_some_and(|level| level >= alert.level) {
            return;
        }
        self.cost_budget_alerts.insert(kind, alert.level);
        let level = match alert.level {
            CostBudgetLevel::Warn => StatusToastLevel::Warning,
            CostBudgetLevel::Hard => StatusToastLevel::Error,
        };
        self.push_status_toast(alert.message(), level, Some(15_000));
    }

    /// Copy current session/subagent cost accumulators into session metadata
    /// for persistence.
    pub fn sync_cost_to_metadata(&self, metadata: &mut crate::session_manager::SessionMetadata) {
        metadata.cost.session_cost_usd = self.session.session_cost;
        metadata.cost.session_cost_cny = self.session.session_cost_cny;
        metadata.cost.subagent_cost_usd = self.session.subagent_cost;
        metadata.cost.subagent_cost_cny = self.session.subagent_cost_cny;
        metadata.cost.displayed_cost_high_water_usd = self.session.displayed_cost_high_water;
        metadata.cost.displayed_cost_high_water_cny = self.session.displayed_cost_high_water_cny;
        // Persist cumulative turn duration so the footer "worked" chip
        // survives session save/restore (#2038).
        metadata.cumulative_turn_secs = self.cumulative_turn_duration.as_secs();
    }

    /// Recompute the displayed cost high-water mark. Called any time a cost
    /// counter is mutated; never decreases.
    pub fn refresh_displayed_cost_high_water(&mut self) {
        let current = self.session.session_cost + self.session.subagent_cost;
        if current > self.session.displayed_cost_high_water {
            self.session.displayed_cost_high_water = current;
        }
        let current_cny = self.session.session_cost_cny + self.session.subagent_cost_cny;
        if current_cny > self.session.displayed_cost_high_water_cny {
            self.session.displayed_cost_high_water_cny = current_cny;
        }
    }
}
