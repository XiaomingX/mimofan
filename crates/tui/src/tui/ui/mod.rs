//! TUI event loop and rendering logic for `DeepSeek` CLI.

use std::collections::{HashSet, VecDeque};
use std::fmt::Write as _;
use std::io::{self, Stdout, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::resource_telemetry::{TokenThroughput, estimate_output_tokens_from_text};
use anyhow::{Context, Result};
// On Windows the push/pop helpers write the escapes directly; crossterm's
// PushKeyboardEnhancementFlags / PopKeyboardEnhancementFlags commands are
// never referenced, so the imports are gated to avoid -D warnings failures.
use crate::audit::log_sensitive_event;
use crate::automation_manager::{AutomationManager, AutomationSchedulerConfig, spawn_scheduler};
use crate::client::{
    ApiClient, CacheWarmupKey, PromptInspection, build_cache_warmup_request,
    inspect_prompt_for_request,
};
use crate::commands;
use crate::compaction::estimate_input_tokens_conservative;
use crate::config::{
    ApiProvider, Config, ProviderConfig, ProvidersConfig, StatusItem, UpdateConfig,
    provider_capability, save_provider_auth_mode_for,
};
use crate::config_ui::{self, ConfigUiMode, WebConfigSession, WebConfigSessionEvent};
use crate::core::engine::{EngineConfig, EngineHandle, spawn_engine};
use crate::core::events::Event as EngineEvent;
use crate::core::ops::{Op, USER_SHELL_TOOL_ID_PREFIX};
use crate::hooks::{HookEvent, HookExecutor, TurnEndPayloadInput, TurnEndTotals};
use crate::llm_client::LlmClient;
use crate::localization::{MessageId, tr};
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt, Usage};
use crate::palette;
use crate::prompts;
use crate::session_manager::{
    OfflineQueueState, QueuedSessionMessage, SavedSession, SessionManager,
    create_saved_session_with_id_and_mode, create_saved_session_with_mode, update_session,
};
use crate::settings::Settings;
use crate::task_manager::{
    NewTaskRequest, SharedTaskManager, TaskManager, TaskManagerConfig, TaskStatus, TaskSummary,
};
use crate::tools::goal::{GoalSnapshot, GoalStatus};
use crate::tools::shell::{ShellJobSnapshot, ShellStatus};
use crate::tools::spec::{RuntimeToolServices, ToolResult};
use crate::tools::subagent::SubAgentStatus;
use crate::tui::color_compat::ColorCompatBackend;
use crate::tui::command_palette::{
    CommandPaletteView, build_entries as build_command_palette_entries,
};
use crate::tui::composer_ui::*;
use crate::tui::context_inspector::build_context_inspector_text;
use crate::tui::event_broker::EventBroker;
use crate::tui::file_picker_relevance;
use crate::tui::footer_ui::{
    friendly_subagent_progress, is_noisy_subagent_progress, one_line_summary,
};
use crate::tui::format_helpers;
use crate::tui::hotbar::actions::HotbarDispatch;
use crate::tui::key_shortcuts;
use crate::tui::mouse_ui::*;
use crate::tui::notifications;
use crate::tui::onboarding;
use crate::tui::pager::PagerView;
use crate::tui::persistence_actor::{self, PersistRequest};
use crate::tui::plan_prompt::PlanPromptView;
use crate::tui::scrolling::TranscriptScroll;
#[cfg(not(windows))]
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    layout::{Rect, Size},
};
use tracing;
// SelectionAutoscroll unused
use crate::tui::session_picker::SessionPickerView;
use crate::tui::streaming_thinking;

use crate::tui::subagent_routing::{
    format_task_list, handle_subagent_mailbox, open_task_pager, reconcile_subagent_activity_state,
    running_agent_count, sort_subagents_in_place, subagent_message_refreshes_workspace_context,
    task_mode_label, task_summary_to_panel_entry,
};

use crate::tui::tool_routing::{
    handle_tool_call_complete, handle_tool_call_started, maybe_add_patch_preview,
};
use crate::tui::ui_text::{history_cell_to_text, line_to_plain, truncate_line_to_width};
use crate::tui::user_input::UserInputView;
use crate::tui::views::subagent_view_agents;
use crate::tui::vim_mode;
use crate::tui::workspace_context;

use super::key_actions;

use super::app::{
    App, AppAction, AppMode, HuntVerdict, OnboardingState, PendingProviderSwitch, QueuedMessage,
    ReasoningEffort, SidebarFocus, StatusToastLevel, TaskPanelEntry, TaskPanelEntryKind,
    TuiOptions, looks_like_slash_command_input, shell_command_from_bang_input,
};
use super::approval::{
    ApprovalMode, ApprovalRequest, ApprovalView, ElevationRequest, ElevationView, ReviewDecision,
};
use super::history::{
    HistoryCell, ToolCell, ToolStatus, TranscriptRenderOptions, history_cells_from_message,
    summarize_tool_output,
};
use super::slash_menu::{
    apply_slash_menu_selection, partial_inline_skill_mention_at_cursor,
    try_autocomplete_slash_command, visible_slash_menu_entries,
};
use super::views::{ConfigView, HelpView, ModalKind};

mod terminal_input;
use terminal_input::TerminalInputPump;

mod translation_event;
use translation_event::TranslationEvent;

mod sidebar_geometry;
pub(crate) use sidebar_geometry::{SidebarRenderState, sidebar_render_state};

mod plan_choice;
pub(crate) use plan_choice::plan_next_step_prompt;

mod ports;
use mimofan_core::BalanceProvider;
use ports::ReqwestBalanceProvider;
mod engine_config_prompt;
pub(crate) use engine_config_prompt::build_engine_config;
mod search_direction;
pub(crate) use search_direction::{SearchDirection, jump_to_adjacent_tool_cell};
mod foreground_shell;
pub(crate) use foreground_shell::{
    active_foreground_shell_running, prefill_jobs_cancel_all_if_tasks_sidebar,
    request_foreground_shell_background, terminal_pause_has_live_owner,
};
mod toast;
pub(crate) use toast::status_color;
mod provider_picker_api_key;
mod queued_message;
pub(crate) use queued_message::{
    build_queued_message, queue_current_draft_for_next_turn, send_ctrl_s_queued_message_now,
    send_queued_message_at_index_now, submit_initial_input_if_ready,
};
mod engine_error_handling;
pub(crate) use engine_error_handling::{
    apply_engine_error_to_app, persist_offline_queue_state, rollback_provider_after_auth_failure,
};
mod session_warmup;
pub(crate) use session_warmup::{
    build_session_snapshot, fetch_available_models, queued_session_to_ui, queued_ui_to_session,
    run_cache_warmup,
};
mod hotbar_shortcuts;
pub(crate) use hotbar_shortcuts::{
    apply_alt_0_shortcut, apply_alt_4_shortcut, dispatch_hotbar_slot, hotbar_slot_from_key,
    persist_sidebar_settings_if_dirty,
};
mod work_sidebar_tasks;
pub(crate) use work_sidebar_tasks::{refresh_active_task_panel, refresh_shell_exec_live_output};
mod subagent_hooks;
pub(crate) use subagent_hooks::{execute_subagent_observer_hook, execute_turn_end_observer_hook};
mod activity_detail;
pub(crate) use activity_detail::{
    copy_cell_to_clipboard, copy_focused_cell, detail_target_cell_index, detail_target_label,
    extract_reasoning_header, open_activity_detail_pager, open_details_pager_for_cell,
    open_pager_for_last_message, open_pager_for_selection, open_tool_details_pager,
    selected_detail_footer_label,
};
mod session_loading;
pub(crate) use session_loading::{apply_loaded_session, derive_session_title};
mod approval;
pub(crate) use approval::{
    ignore_stale_stream_event_while_idle, mark_active_turn_cancelled_locally,
    push_approval_request_view, suppress_engine_event_after_local_cancel,
};
mod paused_command;
pub(crate) use paused_command::{clear_paused_command_state, pause_pausable_command};
mod streaming;
pub(crate) use streaming::{
    append_streaming_text, ensure_streaming_assistant_history_cell, push_assistant_message,
    replace_matching_assistant_text, sanitize_stream_chunk, tool_result_content_for_api_message,
};
mod context_usage;
pub(crate) use context_usage::{
    active_poll_ms, clamp_event_poll_timeout, context_usage_snapshot, history_has_live_motion,
    idle_poll_ms, status_animation_interval_ms,
};
mod terminal_modes;
pub(crate) use terminal_modes::{
    disable_alternate_scroll_mode, disable_bracketed_paste_mode, emergency_restore_terminal,
    pause_terminal, pop_keyboard_enhancement_flags, recover_terminal_modes,
    reset_terminal_viewport, resume_terminal, terminal_event_needs_viewport_recapture,
};
mod version_check;
pub(crate) use version_check::spawn_startup_version_check;

mod turn_liveness;
pub(crate) use turn_liveness::{
    agent_progress_redraw_permitted_for_drain, persist_recovery_snapshot, reconcile_turn_liveness,
    record_turn_activity, recover_engine_event_disconnect, terminal_input_recovery_relevant,
};

mod render;
pub(crate) use render::draw_app_frame_inner;

mod provider_and_model;
pub(crate) use provider_and_model::{
    apply_mode_update, apply_model_and_compaction_update, apply_provider_fallback_switch,
    switch_provider, sync_config_provider_from_app, sync_mode_update,
};

mod message_dispatch;
pub(crate) use message_dispatch::{
    dispatch_user_message, merge_pending_steers, steer_user_message, submit_or_steer_message,
};

mod workspace;
pub(crate) use workspace::{apply_workspace_runtime_state, switch_workspace};

mod mcp_shell;
pub(crate) use mcp_shell::{handle_mcp_ui_action, handle_shell_job_action};

mod view_dispatch;
pub(crate) use view_dispatch::{
    build_pending_input_preview, count_user_history_cells, handle_plan_choice, handle_view_events,
    open_backtrack_overlay, refresh_live_transcript_overlay, toggle_live_transcript_overlay,
};

// === Constants ===

/// Upper bound on slash-menu entries returned to the renderer. The composer's
/// render path already paginates with center-tracking (see
/// `widgets::ComposerWidget::render`), so this only needs to be high enough to
/// encompass the full filtered command list — never the visible-row budget.
/// Bumped from 6 to 128 to fix #64 (selection couldn't reach commands beyond
/// the visible window because the source list itself was capped).
const SLASH_MENU_LIMIT: usize = 128;
const MIN_CHAT_HEIGHT: u16 = 3;
const MIN_COMPOSER_HEIGHT: u16 = 2;
const CONTEXT_WARNING_THRESHOLD_PERCENT: f64 = 85.0;
const CONTEXT_CRITICAL_THRESHOLD_PERCENT: f64 = 95.0;
const CONTEXT_SUGGEST_COMPACT_THRESHOLD_PERCENT: f64 = 60.0;
const UI_IDLE_POLL_MS: u64 = 48;
const UI_ACTIVE_POLL_MS: u64 = 24;
const SUBAGENT_HOOK_PREVIEW_LIMIT: usize = 2_048;
const WEB_CONFIG_POLL_MS: u64 = 16;
// Forced repaint cadence while a turn is live (model loading, compacting,
// sub-agents running). Drives the footer water-spout animation as well as
// the per-tool spinner pulse — keep this fast enough that the spout reads as
// motion (~12 fps) instead of teleport-frames.
const UI_STATUS_ANIMATION_MS: u64 = 80;
pub(crate) const SIDEBAR_VISIBLE_MIN_WIDTH: u16 = 64;
const DEFAULT_TERMINAL_PROBE_TIMEOUT_MS: u64 = 500;
const PERIODIC_FULL_REPAINT_EVERY_N: u64 = 50;
const TURN_META_PREFIX: &str = "<turn_meta>";
const SESSION_TITLE_MAX_CHARS: usize = 32;
const VERSION_HINT_TOAST_TTL_MS: u64 = 12_000;

const REQUIRED_RELEASE_ASSETS: &[&str] = &[
    "mimofan-artifacts-sha256.txt",
    "mimofan-linux-arm64",
    "mimofan-linux-arm64.tar.gz",
    "mimofan-linux-x64",
    "mimofan-linux-x64.tar.gz",
    "mimofan-macos-arm64",
    "mimofan-macos-arm64.tar.gz",
    "mimofan-macos-x64",
    "mimofan-macos-x64.tar.gz",
    "mimofan-windows-x64.exe",
    "mimofan-windows-x64-portable.zip",
    "mimofan-windows-x64.zip",
];

fn is_session_approved_for_tool(app: &App, tool_name: &str, grouping_key: &str) -> bool {
    app.approval_session_approved.contains(grouping_key)
        || app.approval_session_approved.contains(tool_name)
}

fn is_session_denied_for_key(app: &App, approval_key: &str) -> bool {
    app.approval_session_denied.contains(approval_key)
}

fn should_auto_approve_approval_request(
    app: &App,
    tool_name: &str,
    grouping_key: &str,
    approval_force_prompt: bool,
) -> bool {
    !approval_force_prompt
        && (is_session_approved_for_tool(app, tool_name, grouping_key)
            || app.approval_mode == ApprovalMode::Auto)
}

type AppTerminal = Terminal<ColorCompatBackend<Stdout>>;

type PendingToolUses = Vec<(String, String, serde_json::Value)>;

// Reset scroll region (`\x1b[r`), origin mode (`\x1b[?6l`), and home the cursor
// (`\x1b[H`) before letting ratatui's diff renderer repaint. The destructive
// `\x1b[2J\x1b[3J` pair was previously appended here to also wipe the visible
// screen and saved scrollback, but combined with the immediately-following
// `terminal.clear()` it produced a double-clear that several terminals
// (Ghostty, VSCode terminal, Win10 conhost) render as visible flicker on every
// TurnComplete / focus-gain / resize. The alt-screen buffer's double-buffering
// plus ratatui's `terminal.clear()` are sufficient to repaint cleanly.
const TERMINAL_ORIGIN_RESET: &[u8] = b"\x1b[r\x1b[?6l\x1b[H";
// Xterm alternate-scroll mode keeps wheel events inside the alternate-screen
// viewport. Crossterm's mouse-capture command does not enable this DEC private
// mode, so terminals can still scroll the host scrollback if mouse capture is
// disabled, dropped during focus changes, or unavailable in the host.
const ENABLE_ALT_SCROLL_MODE: &[u8] = b"\x1b[?1007h";
const DISABLE_ALT_SCROLL_MODE: &[u8] = b"\x1b[?1007l";
/// Begin synchronized update (DEC 2026): tell the terminal to defer
/// rendering until END_SYNC_UPDATE is received. Best-effort —
/// terminals that don't support this silently ignore the sequence.
/// Reduces flicker on GPU-accelerated terminals (Ghostty, VSCode
/// Terminal, Kitty, WezTerm) by batching ratatui's incremental
/// diff writes into a single frame.
const BEGIN_SYNC_UPDATE: &[u8] = b"\x1b[?2026h";
/// End synchronized update (DEC 2026): tell the terminal to render
/// the complete frame now.
const END_SYNC_UPDATE: &[u8] = b"\x1b[?2026l";
const TERMINAL_INPUT_STALL_TIMEOUT: Duration = Duration::from_secs(5);
const TERMINAL_INPUT_RECOVERY_COOLDOWN: Duration = Duration::from_secs(10);
const MAX_ENGINE_EVENTS_PER_DRAIN: usize = 128;

fn next_terminal_event(
    input: &TerminalInputPump,
    pending: &mut VecDeque<Event>,
    timeout: Duration,
) -> io::Result<Option<Event>> {
    if let Some(event) = pending.pop_front() {
        return Ok(Some(event));
    }
    input.recv_timeout(timeout)
}

fn try_next_terminal_event(
    input: &TerminalInputPump,
    pending: &mut VecDeque<Event>,
) -> io::Result<Option<Event>> {
    if let Some(event) = pending.pop_front() {
        return Ok(Some(event));
    }
    input.try_recv()
}

/// Run the interactive TUI event loop.
///
/// # Examples
///
/// ```ignore
/// # use crate::config::Config;
/// # use crate::tui::TuiOptions;
/// # async fn example(config: &Config, options: TuiOptions) -> anyhow::Result<()> {
/// crate::tui::run_tui(config, options).await
/// # }
/// ```
pub async fn run_tui(config: &Config, options: TuiOptions) -> Result<()> {
    let use_alt_screen = options.use_alt_screen;
    let use_mouse_capture = options.use_mouse_capture;
    let use_bracketed_paste = options.use_bracketed_paste;

    // Apply OSC 8 hyperlink toggle from config.
    //
    // #3029: OSC 8 hyperlinks are now emitted out-of-band. The transcript
    // carries the link payloads in-band inside `Span` content, but each render
    // seam calls `osc8::extract_buffer_link_regions`, which blanks the payload
    // cells (so no buffer cell ever holds `\x1b` or `]8;;` — the old column-
    // drift corruption is gone by construction) and publishes `LinkRegion`s.
    // `ColorCompatBackend::draw` then re-emits the OSC 8 escapes through the
    // backend's `Write` impl, interleaved with the cell stream — never inside a
    // buffer cell. So the corruption that previously forced this default off is
    // fixed, and hyperlinks are on by default for terminals that handle the OSC
    // terminator (`ESC \`) cleanly. Windows legacy consoles (conhost) still
    // mishandle the terminator, so the default stays off there; opt in via
    // `[tui] osc8_links = true` on any platform.
    let osc8_default_on = !cfg!(target_os = "windows");
    crate::tui::osc8::set_enabled(
        config
            .tui
            .as_ref()
            .and_then(|tui| tui.osc8_links)
            .unwrap_or(osc8_default_on),
    );

    // Terminal probe with timeout to prevent hanging on unresponsive terminals
    let probe_timeout = terminal_probe_timeout(config);
    let enable_raw = tokio::task::spawn_blocking(move || {
        enable_raw_mode().map_err(|e| anyhow::anyhow!("Failed to enable raw mode: {e}"))
    });

    match tokio::time::timeout(probe_timeout, enable_raw).await {
        Ok(inner_result) => {
            inner_result??; // propagate both join and raw-mode errors
        }
        Err(_) => {
            tracing::warn!(
                "Terminal probe timed out after {}ms - terminal may be unresponsive",
                probe_timeout.as_millis()
            );
            return Err(anyhow::anyhow!(
                "Terminal probe timed out after {}ms",
                probe_timeout.as_millis()
            ));
        }
    }

    let mut stdout = io::stdout();
    // Initialize the file-backed TUI log and redirect raw stderr away from
    // the alt-screen for the lifetime of this guard. MUST run BEFORE
    // EnterAlternateScreen; otherwise logging between alt-screen entry and
    // redirect init leaks raw bytes into the TUI buffer, causing the "scroll
    // demon" on Windows (#1909) and garbled output on all platforms (#1085).
    // The guard is held until the function returns; dropping it after
    // LeaveAlternateScreen restores the original stderr handle/fd so shutdown
    // messages reach the user's terminal. We accept the init failing (e.g.,
    // read-only $HOME) and continue without the redirect rather than refusing
    // to start the TUI.
    let _tui_log_guard = match crate::runtime_log::init() {
        Ok(guard) => Some(guard),
        Err(err) => {
            tracing::warn!(target: "runtime_log", ?err, "TUI log init failed; stderr leaks may render as scroll-demon");
            None
        }
    };
    if use_alt_screen {
        execute!(stdout, EnterAlternateScreen)?;
        // Windows also suppresses mimofan's own verbose CLI logger while
        // the alt-screen is active. The stderr redirect above catches raw
        // writes; this prevents the known verbose source at the origin.
    }
    // Mouse capture, bracketed paste, focus events, and the Kitty
    // keyboard-protocol escape-disambiguation flag (#442). Single source
    // of truth shared with the FocusGained recovery path and
    // resume_terminal — see recover_terminal_modes.
    //
    // Focus events are necessary for IME compositor re-activation on
    // macOS when the user switches away (Cmd+Tab) and returns. The Kitty
    // keyboard protocol opt-in is best-effort: terminals that don't
    // support it (iTerm2, Terminal.app, Windows 10 conhost) silently
    // discard the escape, while supporting terminals (Kitty, Ghostty,
    // Alacritty 0.13+, WezTerm, recent Konsole, recent xterm) report
    // unambiguous events for Option/Alt-modified keys and plain Esc.
    //
    // Only `DISAMBIGUATE_ESCAPE_CODES` is pushed — the higher tiers
    // (`REPORT_EVENT_TYPES`, `REPORT_ALL_KEYS_AS_ESCAPE_CODES`) emit
    // release events that the existing key handlers would mis-route
    // as duplicate presses.
    //
    // On Windows, crossterm's `PushKeyboardEnhancementFlags` command always
    // reports the terminal as unsupported (`is_ansi_code_supported` returns
    // false), so the escape is written directly instead. VSCode's integrated
    // terminal and Windows Terminal ≥1.17 honour the kitty keyboard protocol
    // and will correctly disambiguate Shift+Enter from plain Enter once this
    // sequence is received. Terminals that do not understand it silently
    // ignore it.
    recover_terminal_modes(&mut stdout, use_mouse_capture, use_bracketed_paste);
    let mut cleanup_guard = TerminalCleanupGuard {
        use_alt_screen,
        use_mouse_capture,
        use_bracketed_paste,
        defused: false,
    };
    let color_depth = palette::ColorDepth::detect();
    let palette_mode = palette::PaletteMode::detect();
    tracing::debug!(
        ?color_depth,
        ?palette_mode,
        "terminal color profile detected"
    );
    let backend = ColorCompatBackend::new(stdout, color_depth, palette_mode);
    let mut terminal = Terminal::new(backend)?;
    // At this point Settings hasn't loaded yet, so we can't read the
    // user's `synchronized_output` knob. Use the same env-based terminal
    // quirk detection that `Settings::apply_env_overrides` uses, so the
    // startup viewport reset matches what every later draw will do on
    // flicker-sensitive hosts. A user who has explicitly set
    // `synchronized_output = "on"` to override detection will get sync wrap
    // from the main draw loop onward; the one-time startup viewport reset
    // stays opt-out for them, which is the safe default because the cost is
    // at most brief tearing on the first frame.
    let sync_output_at_init = !crate::settings::detected_ptyxis_terminal()
        && !crate::settings::detected_legacy_windows_console_host();
    reset_terminal_viewport(&mut terminal, sync_output_at_init)?;
    let event_broker = EventBroker::new();

    // Local mutable copy so runtime config flips (e.g. `/provider` switch)
    // can rebuild the API client without restarting the process.
    let mut config = config.clone();
    let config = &mut config;
    let mut app = App::new(options.clone(), config);
    sync_config_provider_from_app(config, &app);

    // Load existing session if resuming.
    if let Some(ref session_id) = options.resume_session_id
        && let Ok(manager) = SessionManager::default_location()
    {
        // Try to load by prefix or full ID
        let load_result: std::io::Result<Option<crate::session_manager::SavedSession>> =
            if session_id == "latest" {
                // Special case: resume the most recent session in this workspace.
                match manager.get_latest_session_for_workspace(&options.workspace) {
                    Ok(Some(meta)) => manager.load_session(&meta.id).map(Some),
                    Ok(None) => Ok(None),
                    Err(e) => Err(e),
                }
            } else {
                manager.load_session_by_prefix(session_id).map(Some)
            };

        match load_result {
            Ok(Some(saved)) => {
                let recovered = apply_loaded_session(&mut app, config, &saved);
                if !recovered {
                    app.status_message = Some(format!(
                        "Resumed session: {}",
                        crate::session_manager::truncate_id(&saved.metadata.id)
                    ));
                }
            }
            Ok(None) => {
                app.status_message = Some("No sessions found to resume".to_string());
            }
            Err(e) => {
                app.status_message = Some(format!("Failed to load session: {e}"));
            }
        }
    }

    if let Ok(manager) = SessionManager::default_location() {
        match manager.load_offline_queue_state() {
            Ok(Some(state)) => {
                // Only restore queue if session_id matches (or if we're resuming the same session)
                let should_restore = match (&state.session_id, &app.current_session_id) {
                    (Some(saved_id), Some(current_id)) => saved_id == current_id,
                    (None, _) => false, // Legacy unscoped queues are stale-risky; fail closed.
                    (_, None) => false, // No current session - don't restore
                };

                if should_restore {
                    app.queued_messages = state
                        .messages
                        .into_iter()
                        .map(queued_session_to_ui)
                        .collect();
                    let restored_draft = state.draft.map(queued_session_to_ui);
                    if restored_draft.is_some() || app.queued_draft.is_none() {
                        app.queued_draft = restored_draft;
                    }
                    if app.status_message.is_none() && app.queued_message_count() > 0 {
                        app.status_message = Some(format!(
                            "Restored {} queued message(s) from previous session — ↑ to edit, Ctrl+X to discard",
                            app.queued_message_count()
                        ));
                    }
                } else {
                    // Session mismatch - clear the stale queue
                    let _ = manager.clear_offline_queue_state();
                }
            }
            Ok(None) => {}
            Err(err) => {
                if app.status_message.is_none() {
                    app.status_message = Some(format!("Failed to restore offline queue: {err}"));
                }
            }
        }
    }

    let task_manager = TaskManager::start(
        TaskManagerConfig::from_runtime(
            config,
            app.workspace.clone(),
            Some(app.model.clone()),
            Some(app.max_subagents.clamp(1, 4)),
        ),
        config.clone(),
    )
    .await?;
    let automations = std::sync::Arc::new(tokio::sync::Mutex::new(
        AutomationManager::default_location()?,
    ));
    let automation_cancel = tokio_util::sync::CancellationToken::new();
    let automation_scheduler = spawn_scheduler(
        automations.clone(),
        task_manager.clone(),
        automation_cancel.clone(),
        AutomationSchedulerConfig::default(),
    );
    let shell_manager = app
        .runtime_services
        .shell_manager
        .clone()
        .unwrap_or_else(|| crate::tools::shell::new_shared_shell_manager(app.workspace.clone()));
    // #2511: ensure hook_executor is initialized for fresh sessions — it is
    // only set by apply_workspace_runtime_state (session resume / workspace
    // switch), so a brand-new session would otherwise leave it None and both
    // exec_shell shell_env hooks and ToolCallBefore gate would silently no-op.
    if app.runtime_services.hook_executor.is_none() {
        app.runtime_services.hook_executor = Some(std::sync::Arc::new(app.hooks.clone()));
    }
    app.runtime_services = RuntimeToolServices {
        shell_manager: Some(shell_manager),
        task_manager: Some(task_manager.clone()),
        automations: Some(automations),
        task_data_dir: Some(task_manager.data_dir()),
        active_task_id: None,
        active_thread_id: None,
        dynamic_tool_executor: None,
        // #456: plumb the App's HookExecutor so `exec_shell` can surface
        // the configured `shell_env` hooks. Clone the shared Arc.
        hook_executor: app.runtime_services.hook_executor.clone(),
        handle_store: app.runtime_services.handle_store.clone(),
        rlm_sessions: app.runtime_services.rlm_sessions.clone(),
    };
    refresh_active_task_panel(&mut app, &task_manager).await;

    let engine_config = build_engine_config(&app, config);

    // Spawn the Engine - it will handle all API communication
    let engine_handle = spawn_engine(engine_config, config);
    // The translation client is optional: it never crashes the TUI on
    // startup, even when the API key is missing, the base URL is malformed,
    // or the network is unavailable.
    // Translations are skipped with a logged warning until a key is saved.
    let translation_client = match ApiClient::new(config) {
        Ok(client) => Some(Arc::new(client)),
        Err(err) => {
            if app.onboarding == OnboardingState::None {
                tracing::warn!("Translation client initialization failed: {err}");
            }
            None
        }
    };

    if !app.api_messages.is_empty() {
        let _ = engine_handle
            .send(Op::SyncSession {
                session_id: app.current_session_id.clone(),
                messages: app.api_messages.clone(),
                system_prompt: app.system_prompt.clone(),
                system_prompt_override: false,
                model: app.model.clone(),
                workspace: app.workspace.clone(),
            })
            .await;
    }

    // Fire session start hook
    {
        let context = app.base_hook_context();
        let _ = app.execute_hooks(HookEvent::SessionStart, &context);
    }

    // Spawn the persistence actor so checkpoint/session-save I/O stays off
    // the UI thread.  The actor serialises + writes to disk in a dedicated
    // task; the UI just `try_send`s a request and returns immediately.
    if let Ok(persist_manager) = SessionManager::default_location() {
        let handle = persistence_actor::spawn_persistence_actor(persist_manager);
        persistence_actor::init_actor(handle);
    }

    submit_initial_input_if_ready(&mut app, config, &engine_handle).await?;

    let result = run_event_loop(
        &mut terminal,
        &mut app,
        config,
        engine_handle,
        task_manager,
        &event_broker,
        translation_client,
        Arc::new(ReqwestBalanceProvider::new()),
    )
    .await;
    automation_cancel.cancel();
    automation_scheduler.abort();

    // Fire session end hook
    {
        let context = app.base_hook_context();
        let _ = app.execute_hooks(HookEvent::SessionEnd, &context);
    }

    // Flush the persistence actor: clear checkpoint + graceful shutdown.
    persistence_actor::persist(PersistRequest::ClearCheckpoint);
    persistence_actor::persist(PersistRequest::Shutdown);

    cleanup_guard.defused = true;
    pop_keyboard_enhancement_flags(terminal.backend_mut());
    disable_alternate_scroll_mode(terminal.backend_mut());
    execute!(terminal.backend_mut(), DisableFocusChange)?;
    disable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    if use_mouse_capture {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    if use_bracketed_paste {
        disable_bracketed_paste_mode(terminal.backend_mut());
    }
    terminal.show_cursor()?;
    drop(terminal);

    if result.is_ok() && should_show_resume_hint(app.current_session_id.as_deref()) {
        // Printed AFTER `LeaveAlternateScreen` / `drop(terminal)` above,
        // so we're back on the primary screen — this is the one
        // legitimate stdout write in the TUI module tree. The
        // module-level `#![deny(clippy::print_stdout)]` would otherwise
        // refuse it.
        #[allow(clippy::print_stdout)]
        {
            println!("{}", resume_hint_text());
        }
    }

    result
}

fn should_show_resume_hint(session_id: Option<&str>) -> bool {
    session_id.is_some_and(|id| !id.trim().is_empty())
}

fn resume_hint_text() -> &'static str {
    "To continue this session, execute mimofan run --continue"
}

fn terminal_probe_timeout(config: &Config) -> Duration {
    let timeout_ms = config
        .tui
        .as_ref()
        .and_then(|tui| tui.terminal_probe_timeout_ms)
        .unwrap_or(DEFAULT_TERMINAL_PROBE_TIMEOUT_MS)
        .clamp(100, 5_000);
    Duration::from_millis(timeout_ms)
}

struct TerminalCleanupGuard {
    use_alt_screen: bool,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
    defused: bool,
}

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        if self.defused {
            return;
        }

        let mut stdout = io::stdout();
        pop_keyboard_enhancement_flags(&mut stdout);
        disable_alternate_scroll_mode(&mut stdout);
        let _ = execute!(stdout, DisableFocusChange);
        let _ = disable_raw_mode();
        if self.use_alt_screen {
            let _ = execute!(stdout, LeaveAlternateScreen);
        }
        if self.use_mouse_capture {
            let _ = execute!(stdout, DisableMouseCapture);
        }
        if self.use_bracketed_paste {
            disable_bracketed_paste_mode(&mut stdout);
        }
        let _ = execute!(stdout, crossterm::cursor::Show);
    }
}

/// Recognise composer input that is a `# foo` memory quick-add (#492).
///
/// Returns `true` for inputs that:
/// - start with `#`,
/// - have at least one non-whitespace character after the leading `#`,
/// - are a single line (no embedded `\n`), and
/// - are not a shebang (`#!`) or Markdown heading (`## …`, `### …`).
///
/// Multi-`#` prefixes are deliberately rejected so users can paste
/// Markdown headings into the composer without triggering the quick-add.
#[must_use]
fn is_memory_quick_add(input: &str) -> bool {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('#') {
        return false;
    }
    if trimmed.starts_with("##") || trimmed.starts_with("#!") {
        return false;
    }
    if input.contains('\n') {
        return false;
    }
    // Require something after the `#`.
    !trimmed.trim_start_matches('#').trim().is_empty()
}

/// Persist a `# foo` quick-add to the memory file and surface a status
/// note to the user. Errors land in the same status channel so a missing
/// memory directory becomes visible without crashing the composer.
fn handle_memory_quick_add(app: &mut App, input: &str, config: &Config) {
    let path = config.memory_path();
    match crate::memory::append_entry(&path, input) {
        Ok(()) => {
            app.status_message = Some(format!("memory: appended to {}", path.display()));
        }
        Err(err) => {
            app.status_message = Some(format!(
                "memory: failed to write {}: {}",
                path.display(),
                err
            ));
        }
    }
}

/// How long after a task finishes it should still appear in the Work
/// sidebar even if its `ended_at` predates the current TUI session.
///
/// Tasks completing during the current session always show (until the
/// next session boundary). Tasks that completed shortly before the
/// session also show, so users coming back to a terminal see "you just
/// finished X". Anything older than this window is hidden — preventing
/// the sidebar from accumulating indefinitely (bug #1913).
const WORK_SIDEBAR_RECENT_COMPLETED_TTL: chrono::Duration = chrono::Duration::hours(2);

/// Minimum interval between balance API fetches to avoid flooding.
const BALANCE_FETCH_COOLDOWN: Duration = Duration::from_secs(60);

/// Fetch the DeepSeek account balance from the balance API.
///
/// Returns `None` on any error (network, auth, parse) — callers should treat
/// a `None` return as "balance unknown" and keep the previous value.
/// Fetch the DeepSeek account balance via the injected [`BalanceProvider`].
///
/// Returns `None` on any error (network, auth, parse) — callers should treat
/// a `None` return as "balance unknown" and keep the previous value.
async fn fetch_deepseek_balance(
    provider: &impl BalanceProvider<Balance = crate::pricing::BalanceInfo>,
    api_key: &str,
    base_url: &str,
) -> Option<crate::pricing::BalanceInfo> {
    provider.fetch_balance(api_key, base_url).await
}

fn should_fetch_deepseek_balance(app: &App) -> bool {
    app.status_items.contains(&StatusItem::Balance)
        && matches!(app.api_provider, ApiProvider::XiaomiMimo)
}

#[allow(clippy::too_many_lines)]
async fn run_event_loop(
    terminal: &mut AppTerminal,
    app: &mut App,
    config: &mut Config,
    mut engine_handle: EngineHandle,
    task_manager: SharedTaskManager,
    event_broker: &EventBroker,
    translation_client: Option<Arc<ApiClient>>,
    balance_provider: Arc<impl BalanceProvider<Balance = crate::pricing::BalanceInfo> + 'static>,
) -> Result<()> {
    // Subscribe to task completion events for proactive notification.
    let mut task_completion_rx = task_manager.subscribe_completions();
    // Track streaming state
    let mut current_streaming_text = String::new();
    let (translation_tx, mut translation_rx) =
        tokio::sync::mpsc::unbounded_channel::<TranslationEvent>();
    let mut pending_translations = 0usize;
    let mut pending_thinking_translations = 0usize;
    let mut last_queue_state = (app.queued_messages.clone(), app.queued_draft.clone());
    let mut last_task_refresh = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .unwrap_or_else(Instant::now);
    let mut last_status_frame = Instant::now()
        .checked_sub(Duration::from_millis(UI_STATUS_ANIMATION_MS))
        .unwrap_or_else(Instant::now);
    // 120 FPS draw cap. Without this we redraw on every SSE chunk during a
    // long stream — wasted work the user can't perceive. See
    // `tui::frame_rate_limiter` for the rationale; ports the small piece of
    // codex's frame coalescing that maps cleanly onto our poll-based loop.
    let mut frame_rate_limiter = crate::tui::frame_rate_limiter::FrameRateLimiter::default();
    let mut web_config_session: Option<WebConfigSession> = None;
    let mut prev_input_snapshot = String::new();
    let mut terminal_paused_at: Option<Instant> = None;
    let mut force_terminal_repaint = false;
    let mut draws_since_last_full_repaint: u64 = 0;
    // FocusGained debounce: some terminal emulators (e.g. Tabby) re-trigger
    // FocusGained when we re-arm focus-change reporting inside
    // recover_terminal_modes, creating a tight repaint loop. Skip
    // mode recovery (but still mark a repaint) within the debounce window.
    const FOCUS_RECOVERY_DEBOUNCE: Duration = Duration::from_millis(200);
    let mut last_focus_recovery = Instant::now()
        .checked_sub(Duration::from_secs(60))
        .unwrap_or_else(Instant::now);
    #[cfg(not(target_os = "windows"))]
    let terminal_input = TerminalInputPump::spawn()?;
    let mut pending_terminal_events: VecDeque<Event> = VecDeque::new();
    let mut last_terminal_input_recovery = Instant::now()
        .checked_sub(TERMINAL_INPUT_RECOVERY_COOLDOWN)
        .unwrap_or_else(Instant::now);

    // Fire-and-forget version check — runs once per session in the
    // background. On success, a short status toast advertises the update
    // without replacing the user's configured footer/status-line chips.
    let mut version_check: Option<tokio::task::JoinHandle<Option<String>>> =
        spawn_startup_version_check(config.update_config());

    // Fire a one-shot initial balance fetch for DeepSeek providers
    // so the footer chip shows balance on the first frame without
    // waiting for a turn to complete.
    if !app.balance_initiated && should_fetch_deepseek_balance(app) {
        let cell = app.balance_cell.clone();
        let api_key = config.api_key().unwrap_or_default();
        let base_url = config.api_base_url();
        if !api_key.is_empty() {
            app.last_balance_fetch = Some(Instant::now());
            let provider = balance_provider.clone();
            tokio::spawn(async move {
                if let Some(info) = fetch_deepseek_balance(&*provider, &api_key, &base_url).await
                    && let Ok(mut guard) = cell.lock()
                {
                    *guard = Some(info);
                }
            });
        }
        app.balance_initiated = true;
    }

    loop {
        // Drain the version-check handle once; re-assign None so we
        // don't poll it again.
        let should_check = version_check.as_ref().is_some_and(|h| h.is_finished());
        if should_check && let Ok(Some(hint)) = version_check.take().expect("checked above").await {
            app.push_status_toast(
                hint,
                StatusToastLevel::Warning,
                Some(VERSION_HINT_TOAST_TTL_MS),
            );
        }

        if !drain_web_config_events(&mut web_config_session, app, config, &engine_handle).await {
            web_config_session = None;
        }

        while let Ok(event) = translation_rx.try_recv() {
            match event {
                TranslationEvent::AssistantMessage {
                    history_index,
                    original_text,
                    translated,
                    thinking,
                    tool_uses,
                } => {
                    pending_translations = pending_translations.saturating_sub(1);
                    pending_thinking_translations = pending_thinking_translations.saturating_sub(1);
                    let text = match translated {
                        Ok(text) => {
                            app.status_message = Some(
                                crate::localization::tr(
                                    app.ui_locale,
                                    crate::localization::MessageId::TranslationComplete,
                                )
                                .to_string(),
                            );
                            text
                        }
                        Err(err) => {
                            tracing::warn!("assistant translation failed: {err}");
                            app.status_message = Some(format!(
                                "{}: {err}",
                                crate::localization::tr(
                                    app.ui_locale,
                                    crate::localization::MessageId::TranslationFailed,
                                )
                            ));
                            crate::localization::hidden_translation_failed(app.ui_locale)
                                .to_string()
                        }
                    };

                    if let Some(index) = history_index
                        && let Some(HistoryCell::Assistant { content, .. }) =
                            app.history.get_mut(index)
                    {
                        *content = text.clone();
                        app.bump_history_cell(index);
                    }
                    if !replace_matching_assistant_text(app, &original_text, text.clone()) {
                        push_assistant_message(app, text, thinking, tool_uses);
                    }
                    if pending_translations == 0
                        && !matches!(app.runtime_turn_status.as_deref(), Some("in_progress"))
                    {
                        app.is_loading = pending_translations > 0;
                    }
                    app.needs_redraw = true;
                }
                TranslationEvent::Thinking {
                    placeholder,
                    translated,
                } => {
                    pending_translations = pending_translations.saturating_sub(1);
                    let text = match translated {
                        Ok(text) => {
                            app.status_message = Some(
                                crate::localization::thinking_translation_complete(app.ui_locale)
                                    .to_string(),
                            );
                            text
                        }
                        Err(err) => {
                            tracing::warn!("thinking translation failed: {err}");
                            app.status_message = Some(format!(
                                "{}: {err}",
                                crate::localization::thinking_translation_failed(app.ui_locale)
                            ));
                            crate::localization::hidden_translation_failed(app.ui_locale)
                                .to_string()
                        }
                    };
                    streaming_thinking::replace_pending_translation(app, &placeholder, text);
                    if pending_translations == 0
                        && !matches!(app.runtime_turn_status.as_deref(), Some("in_progress"))
                    {
                        app.is_loading = false;
                    }
                    app.needs_redraw = true;
                }
            }
        }

        if last_task_refresh.elapsed() >= Duration::from_millis(2500) {
            refresh_active_task_panel(app, &task_manager).await;
            if refresh_shell_exec_live_output(app) {
                app.needs_redraw = true;
            }
            last_task_refresh = Instant::now();
            app.needs_redraw = true;
        }

        // Poll for task completion events and inject steer messages when idle.
        // This enables proactive notification: the model is informed when a
        // background task finishes, rather than having to poll manually.
        while let Ok(completion) = task_completion_rx.try_recv() {
            // Only inject if the engine is idle (not currently processing a turn).
            if !app.is_loading {
                let notification = format!(
                    "[Task Completed] Background task `{task_id}` finished with status: {status}. \
                     Duration: {duration}. Summary: {summary}",
                    task_id = completion.task_id,
                    status = completion.status,
                    duration = completion
                        .duration_ms
                        .map(|ms| format!("{}ms", ms))
                        .unwrap_or_else(|| "unknown".to_string()),
                    summary = completion.summary,
                );
                if let Err(err) = engine_handle.steer(notification).await {
                    tracing::warn!("Failed to steer task completion notification: {err}");
                }
                app.needs_redraw = true;
            }
        }

        // Clear suggestion when the user modifies the input.
        if app.input != prev_input_snapshot {
            app.prompt_suggestion = None;
            prev_input_snapshot = app.input.clone();
        }

        // Poll prompt suggestion cell from background generation task.
        // Discard stale results whose generation token no longer matches.
        if let Ok(mut guard) = app.prompt_suggestion_cell.try_lock()
            && let Some((gen_token, suggestion)) = guard.take()
            && gen_token
                == app
                    .prompt_suggestion_gen
                    .load(std::sync::atomic::Ordering::Relaxed)
        {
            app.prompt_suggestion = Some(suggestion);
        }

        // First, poll for engine events (non-blocking)
        let mut received_engine_event = false;
        let mut transcript_batch_updated = false;
        // #freeze: coalesce per-event `Op::ListSubAgents` sends into a single
        // trailing-edge refresh per drain. At high fanout, many spawn/complete/
        // mailbox events in one drain otherwise each take the manager write
        // lock and trigger a full O(N) list reconcile.
        let mut subagent_list_refresh_requested = false;
        let mut queued_to_send: Option<QueuedMessage> = None;
        let mut respawn_after_provider_rollback: Option<String> = None;
        let mut fallback_after_engine_error: Option<ApiProvider> = None;
        {
            let mut rx = engine_handle.rx_event.write().await;
            let mut progress_redraw_agents: HashSet<String> = HashSet::new();
            for _ in 0..MAX_ENGINE_EVENTS_PER_DRAIN {
                let event = match rx.try_recv() {
                    Ok(event) => event,
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        if recover_engine_event_disconnect(app) {
                            received_engine_event = true;
                            transcript_batch_updated = true;
                        }
                        break;
                    }
                };
                // #3033: remember whether an EARLIER event in this drain batch
                // already requested a redraw. The AgentProgress throttle below
                // may opt the current event out of repainting, but it must not
                // cancel redraws owed to other events in the same batch.
                let redraw_requested_before_event = received_engine_event;
                received_engine_event = true;
                if app.suppress_stream_events_until_turn_complete {
                    if matches!(event, EngineEvent::TurnStarted { .. }) {
                        // Ctrl+C can race with the engine's per-turn token
                        // reset: the first cancel may hit the previous token
                        // if SendMessage is queued but TurnStarted has not
                        // arrived yet. Reassert cancellation once the real
                        // turn starts, then keep hiding its queued deltas.
                        engine_handle.cancel();
                        continue;
                    }
                    if suppress_engine_event_after_local_cancel(&event) {
                        continue;
                    }
                } else if !app.is_loading && ignore_stale_stream_event_while_idle(&event) {
                    continue;
                }
                record_turn_activity(app, &event, Instant::now());
                match event {
                    EngineEvent::MessageStarted { .. } => {
                        // Assistant text starting after parallel tool work
                        // means the tool group is done. Flush the active
                        // cell first so the message lands BELOW the
                        // committed tool group (Codex pattern: streamed
                        // assistant content always flows after work).
                        app.flush_active_cell();
                        current_streaming_text.clear();
                        app.streaming_output_token_estimate = 0;
                        app.streaming_state.reset();
                        app.streaming_state.start_text(0, None);
                        app.streaming_message_index = None;
                    }
                    EngineEvent::MessageDelta { content, .. } => {
                        let sanitized = sanitize_stream_chunk(&content);
                        if sanitized.is_empty() {
                            continue;
                        }
                        // Record TTFT on the first non-empty content delta.
                        if app.turn_first_token_at.is_none() {
                            app.turn_first_token_at = Some(Instant::now());
                        }
                        // First delta of a fresh stream has no streaming
                        // cell yet; flush active so the tool group settles
                        // before the assistant prose appears below it.
                        if app.streaming_message_index.is_none() {
                            app.flush_active_cell();
                        }
                        current_streaming_text.push_str(&sanitized);
                        app.streaming_output_token_estimate =
                            estimate_output_tokens_from_text(&current_streaming_text);
                        let index = ensure_streaming_assistant_history_cell(app);
                        app.streaming_state.push_content(0, &sanitized);
                        let committed = app.streaming_state.commit_text(0);
                        if !committed.is_empty() {
                            append_streaming_text(app, index, &committed);
                            transcript_batch_updated = true;
                        }
                    }
                    EngineEvent::MessageComplete { .. } => {
                        // #861 RC3: defensive drain of a still-active thinking
                        // entry. Normally `ThinkingComplete` arrives first and
                        // populates `last_reasoning` before we get here, but
                        // when the engine bursts events the channel can
                        // deliver `MessageComplete` first, in which case
                        // `last_reasoning.take()` below would be `None` and
                        // the thinking block would be dropped from
                        // `api_messages` — causing a DeepSeek HTTP 400 on the
                        // next turn (V4 thinking-mode requires
                        // `reasoning_content` replay). Inline-finalize the
                        // thinking entry here so this branch is order-
                        // independent.
                        if app.streaming_thinking_active_entry.is_some() {
                            if streaming_thinking::finalize_current(app) {
                                transcript_batch_updated = true;
                            }
                            streaming_thinking::stash_reasoning_buffer_into_last_reasoning(app);
                        }
                        let mut completed_message_index = None;
                        if let Some(index) = app.streaming_message_index.take() {
                            completed_message_index = Some(index);
                            let remaining = app.streaming_state.finalize_block_text(0);
                            if !remaining.is_empty() {
                                append_streaming_text(app, index, &remaining);
                            }
                            if let Some(HistoryCell::Assistant { streaming, .. }) =
                                app.history.get_mut(index)
                            {
                                *streaming = false;
                            }
                            // Streaming flag flipped — the cell's compact /
                            // transcript variants render slightly
                            // differently, so bump its revision so the cache
                            // refreshes this row only.
                            app.bump_history_cell(index);
                            transcript_batch_updated = true;
                        }

                        let thinking = app.last_reasoning.take();
                        let tool_uses = app.pending_tool_uses.drain(..).collect::<Vec<_>>();
                        let history_index = completed_message_index;

                        if app.translation_enabled
                            && !current_streaming_text.is_empty()
                            && crate::tui::translation::needs_translation(&current_streaming_text)
                            && let Some(translation_client) = translation_client.as_ref()
                        {
                            app.status_message = Some(
                                crate::localization::tr(
                                    app.ui_locale,
                                    crate::localization::MessageId::TranslationInProgress,
                                )
                                .to_string(),
                            );
                            app.is_loading = true;
                            pending_translations = pending_translations.saturating_add(1);
                            let tx = translation_tx.clone();
                            let client = translation_client.clone();
                            let original_text = current_streaming_text.clone();
                            let translation_model = app
                                .last_effective_model
                                .clone()
                                .unwrap_or_else(|| app.model.clone());
                            let target_language =
                                app.ui_locale.translation_target_name().to_string();
                            tokio::spawn(async move {
                                let translated = crate::tui::translation::translate_text(
                                    &original_text,
                                    &client,
                                    &translation_model,
                                    &target_language,
                                )
                                .await;
                                let _ = tx.send(TranslationEvent::AssistantMessage {
                                    history_index,
                                    original_text,
                                    translated,
                                    thinking,
                                    tool_uses,
                                });
                            });
                        } else {
                            push_assistant_message(
                                app,
                                current_streaming_text.clone(),
                                thinking,
                                tool_uses,
                            );
                        }
                    }
                    EngineEvent::ThinkingStarted { .. } => {
                        // P2.3: thinking lives in the active cell so it groups
                        // visually with the tool calls that follow until the
                        // next assistant prose chunk flushes the group.
                        if streaming_thinking::start_block(app) {
                            transcript_batch_updated = true;
                        }
                        if app.translation_enabled {
                            let entry_idx = streaming_thinking::ensure_active_entry(app);
                            streaming_thinking::set_placeholder(app, entry_idx);
                            transcript_batch_updated = true;
                        }
                    }
                    EngineEvent::ThinkingDelta { content, .. } => {
                        let sanitized = sanitize_stream_chunk(&content);
                        if sanitized.is_empty() {
                            continue;
                        }
                        app.reasoning_buffer.push_str(&sanitized);
                        if app.reasoning_header.is_none() {
                            app.reasoning_header = extract_reasoning_header(&app.reasoning_buffer);
                        }

                        let entry_idx = streaming_thinking::ensure_active_entry(app);
                        app.streaming_state.push_content(0, &sanitized);
                        let committed = app.streaming_state.commit_text(0);
                        if !committed.is_empty() {
                            if app.translation_enabled {
                                streaming_thinking::set_placeholder(app, entry_idx);
                            } else {
                                streaming_thinking::append(app, entry_idx, &committed);
                            }
                            transcript_batch_updated = true;
                        }
                    }
                    EngineEvent::ThinkingComplete { .. } => {
                        if app.translation_enabled {
                            let original_thinking = app.reasoning_buffer.clone();
                            let _ = app.streaming_state.finalize_block_text(0);
                            let duration = app
                                .thinking_started_at
                                .take()
                                .map(|t| t.elapsed().as_secs_f32());
                            if streaming_thinking::finalize_active_entry(app, duration, "") {
                                transcript_batch_updated = true;
                            }
                            if !original_thinking.is_empty()
                                && crate::tui::translation::needs_translation(&original_thinking)
                                && let Some(translation_client) = translation_client.as_ref()
                            {
                                app.status_message = Some(
                                    crate::localization::thinking_translation_in_progress(
                                        app.ui_locale,
                                    )
                                    .to_string(),
                                );
                                app.is_loading = true;
                                pending_translations = pending_translations.saturating_add(1);
                                pending_thinking_translations =
                                    pending_thinking_translations.saturating_add(1);
                                let tx = translation_tx.clone();
                                let client = translation_client.clone();
                                let translation_model = app
                                    .last_effective_model
                                    .clone()
                                    .unwrap_or_else(|| app.model.clone());
                                let placeholder =
                                    crate::localization::thinking_translation_placeholder(
                                        app.ui_locale,
                                    )
                                    .to_string();
                                let target_language =
                                    app.ui_locale.translation_target_name().to_string();
                                tokio::spawn(async move {
                                    let translated = crate::tui::translation::translate_text(
                                        &original_thinking,
                                        &client,
                                        &translation_model,
                                        &target_language,
                                    )
                                    .await;
                                    let _ = tx.send(TranslationEvent::Thinking {
                                        placeholder,
                                        translated,
                                    });
                                });
                            } else {
                                let placeholder =
                                    crate::localization::thinking_translation_placeholder(
                                        app.ui_locale,
                                    );
                                streaming_thinking::replace_pending_translation(
                                    app,
                                    placeholder,
                                    original_thinking,
                                );
                            }
                        } else if streaming_thinking::finalize_current(app) {
                            transcript_batch_updated = true;
                        }
                        streaming_thinking::stash_reasoning_buffer_into_last_reasoning(app);
                    }
                    EngineEvent::ToolCallStarted { id, name, input } => {
                        app.pending_tool_uses
                            .push((id.clone(), name.clone(), input.clone()));
                        // Note this dispatch so the next sub-agent `Started`
                        // mailbox envelope routes into the right card kind
                        // (delegate vs fanout).
                        if matches!(
                            name.as_str(),
                            "agent" | "rlm_open" | "rlm_eval" | "rlm" | "delegate"
                        ) {
                            app.pending_subagent_dispatch = Some(name.clone());
                            if matches!(name.as_str(), "rlm_open" | "rlm_eval" | "rlm") {
                                // New fanout invocation — children should
                                // group under a fresh card, not the
                                // previous fanout's leftover.
                                app.last_fanout_card_index = None;
                            }
                        }
                        handle_tool_call_started(app, &id, &name, &input);
                    }
                    EngineEvent::ToolCallComplete { id, name, result } => {
                        if name == "update_plan" {
                            app.plan_tool_used_in_turn = true;
                        }
                        if is_model_visible_tool_call(&id) {
                            let tool_content = match &result {
                                Ok(output) => sanitize_stream_chunk(
                                    &tool_result_content_for_api_message(app, &id, &name, output)
                                        .await,
                                ),
                                Err(err) => sanitize_stream_chunk(&format!("Error: {err}")),
                            };
                            app.api_messages.push(Message {
                                role: "user".to_string(),
                                content: vec![ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: tool_content,
                                    is_error: None,
                                    content_blocks: None,
                                }],
                            });
                        } else {
                            app.pending_tool_uses
                                .retain(|(tool_id, _, _)| tool_id != &id);
                        }
                        handle_tool_call_complete(app, &id, &name, &result);

                        // Immediately refresh the task panel sidebar when a
                        // tool that changes task state completes, so the
                        // Tasks panel stays in sync with tool execution
                        // rather than waiting up to 2.5 s for the periodic
                        // poll. Also merge shell jobs (#373).
                        // Only tools that actually change durable tasks or
                        // background shell jobs force a jobs-panel refresh.
                        // Checklist/todo/plan tools drive the To-do panel,
                        // which reads `app.todos` directly and repaints on the
                        // normal redraw — no forced refresh needed (avoids the
                        // old per-checklist Tasks-panel churn).
                        if matches!(
                            name.as_str(),
                            "agent"
                                | "task_shell_start"
                                | "exec_shell"
                                | "exec_shell_cancel"
                                | "exec_shell_wait"
                                | "task_cancel"
                        ) {
                            refresh_active_task_panel(app, &task_manager).await;
                            last_task_refresh = Instant::now();
                        }
                        if matches!(name.as_str(), "agent") {
                            subagent_list_refresh_requested = true;
                        }
                    }
                    EngineEvent::TurnStarted { turn_id } => {
                        app.suppress_stream_events_until_turn_complete = false;
                        app.is_loading = true;
                        app.offline_mode = false;
                        app.turn_error_posted = false;
                        app.prompt_suggestion = None;
                        app.prompt_suggestion_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        app.dispatch_started_at = None;
                        current_streaming_text.clear();
                        app.streaming_output_token_estimate = 0;
                        app.streaming_state.reset();
                        app.streaming_message_index = None;
                        app.streaming_thinking_active_entry = None;
                        let now = Instant::now();
                        app.turn_started_at = Some(now);
                        app.turn_last_activity_at = Some(now);
                        app.session.last_output_throughput = None;
                        app.session.last_ttft = None;
                        app.turn_first_token_at = None;
                        app.streaming_output_token_estimate = 0;
                        app.provider_wait_incident_logged = false;
                        // Discoverability hint for users who don't know how
                        // to interrupt a long-running turn (#1367). Only
                        // surface when the status_message slot is empty so
                        // we don't trample over a real transient message
                        // (e.g. "/queue saved", "Selection copied"); the
                        // hint then auto-clears as soon as anything else
                        // updates the slot.
                        if app.status_message.is_none() {
                            app.status_message = Some("Press Esc or Ctrl+C to cancel".to_string());
                        }
                        app.runtime_turn_id = Some(turn_id);
                        app.runtime_turn_status = Some("in_progress".to_string());
                        app.turn_counter = app.turn_counter.saturating_add(1);
                        app.reasoning_buffer.clear();
                        app.reasoning_header = None;
                        app.last_reasoning = None;
                        app.pending_tool_uses.clear();
                        app.plan_tool_used_in_turn = false;
                        last_status_frame = Instant::now();
                    }
                    EngineEvent::TurnComplete {
                        usage,
                        status,
                        error,
                        tool_catalog,
                        base_url,
                    } => {
                        app.session.last_tool_catalog = tool_catalog;
                        app.session.last_base_url = base_url;
                        let was_locally_cancelled = app.suppress_stream_events_until_turn_complete;
                        app.suppress_stream_events_until_turn_complete = false;
                        app.active_allowed_tools = None;
                        if app.paused_quarry.is_none() {
                            app.pausable = false;
                            app.paused = false;
                        }
                        if !matches!(status, crate::core::events::TurnOutcomeStatus::Completed)
                            || draws_since_last_full_repaint >= PERIODIC_FULL_REPAINT_EVERY_N
                        {
                            force_terminal_repaint = true;
                        }
                        // Finalize any in-flight tool group. Cancellation
                        // marks still-running entries as Failed so the user
                        // sees they were interrupted rather than the spinner
                        // hanging forever.
                        if matches!(
                            status,
                            crate::core::events::TurnOutcomeStatus::Interrupted
                                | crate::core::events::TurnOutcomeStatus::Failed
                        ) {
                            app.finalize_active_cell_as_interrupted();
                            // Also mark the streaming Assistant cell (if any)
                            // so partial reasoning/text isn't left with a
                            // permanent spinner. Idempotent with the
                            // optimistic call in the Esc handler.
                            app.finalize_streaming_assistant_as_interrupted();
                        } else {
                            app.flush_active_cell();
                        }
                        app.is_loading = false;
                        app.dispatch_started_at = None;
                        app.pending_provider_switch = None;
                        app.offline_mode = false;
                        app.streaming_state.reset();
                        if was_locally_cancelled {
                            current_streaming_text.clear();
                        }
                        // Capture elapsed before clearing turn_started_at so
                        // notifications can use the real wall-clock duration.
                        let turn_elapsed =
                            app.turn_started_at.map(|t| t.elapsed()).unwrap_or_default();
                        // Compute TTFT from the instant the turn started to the
                        // first content delta. Only store when we have both anchors
                        // and the result is positive.
                        if let Some(first_token_at) = app.turn_first_token_at.take()
                            && let Some(started) = app.turn_started_at
                        {
                            let ttft = first_token_at.duration_since(started);
                            if ttft.as_secs_f64() > 0.0 && ttft.as_secs_f64().is_finite() {
                                app.session.last_ttft = Some(ttft);
                            }
                        }
                        app.turn_started_at = None;
                        app.turn_last_activity_at = None;
                        app.streaming_output_token_estimate = 0;
                        // Roll the just-finished turn's elapsed time into the
                        // cumulative session work-time (#448 follow-up). The
                        // footer's `worked Nh Mm` chip reads this so the
                        // label reflects actual model work, not idle
                        // uptime since launch.
                        app.cumulative_turn_duration =
                            app.cumulative_turn_duration.saturating_add(turn_elapsed);
                        // Stream lock applies per-turn; clear it so the next
                        // turn's chunks pull the view down again until the
                        // user opts out by scrolling up.
                        app.user_scrolled_during_stream = false;
                        app.runtime_turn_status = Some(match status {
                            crate::core::events::TurnOutcomeStatus::Completed => {
                                "completed".to_string()
                            }
                            crate::core::events::TurnOutcomeStatus::Interrupted => {
                                "interrupted".to_string()
                            }
                            crate::core::events::TurnOutcomeStatus::Failed => "failed".to_string(),
                        });
                        if matches!(
                            status,
                            crate::core::events::TurnOutcomeStatus::Interrupted
                                | crate::core::events::TurnOutcomeStatus::Failed
                        ) {
                            subagent_list_refresh_requested = true;
                        }
                        crate::tui::notifications::clear_taskbar_progress();
                        if status != crate::core::events::TurnOutcomeStatus::Completed {
                            crate::retry_status::clear();
                            crate::tui::notifications::stop_title_animation_quietly();
                        }
                        let turn_tokens = usage.input_tokens + usage.output_tokens;
                        app.session.total_tokens =
                            app.session.total_tokens.saturating_add(turn_tokens);
                        app.session.total_conversation_tokens = app
                            .session
                            .total_conversation_tokens
                            .saturating_add(turn_tokens);
                        app.session.total_input_tokens = app
                            .session
                            .total_input_tokens
                            .saturating_add(usage.input_tokens);
                        app.session.total_output_tokens = app
                            .session
                            .total_output_tokens
                            .saturating_add(usage.output_tokens);
                        // Only accumulate cache telemetry when reported.
                        if let Some(hit_tokens) = usage.prompt_cache_hit_tokens {
                            app.session.total_cache_hit_tokens = app
                                .session
                                .total_cache_hit_tokens
                                .saturating_add(hit_tokens);
                            let cache_miss = usage
                                .prompt_cache_miss_tokens
                                .unwrap_or_else(|| usage.input_tokens.saturating_sub(hit_tokens));
                            app.session.total_cache_miss_tokens = app
                                .session
                                .total_cache_miss_tokens
                                .saturating_add(cache_miss);
                        }
                        app.session.last_prompt_tokens = Some(usage.input_tokens);
                        app.session.last_completion_tokens = Some(usage.output_tokens);
                        app.session.last_output_throughput =
                            TokenThroughput::new(u64::from(usage.output_tokens), turn_elapsed);
                        app.session.last_prompt_cache_hit_tokens = usage.prompt_cache_hit_tokens;
                        app.session.last_prompt_cache_miss_tokens = usage.prompt_cache_miss_tokens;
                        app.session.last_reasoning_replay_tokens = usage.reasoning_replay_tokens;
                        app.push_turn_cache_record(crate::tui::app::TurnCacheRecord {
                            input_tokens: usage.input_tokens,
                            output_tokens: usage.output_tokens,
                            cache_hit_tokens: usage.prompt_cache_hit_tokens,
                            cache_miss_tokens: usage.prompt_cache_miss_tokens,
                            reasoning_replay_tokens: usage.reasoning_replay_tokens,
                            recorded_at: Instant::now(),
                        });
                        if let Some(error) = error.as_deref() {
                            // Only show "Turn failed:" in the composer status
                            // area when an EngineEvent::Error has NOT already
                            // posted the same message into the transcript.
                            // Otherwise the error appears twice: once in a
                            // HistoryCell and again as a redundant status line.
                            if !app.turn_error_posted {
                                app.status_message = Some(format!("Turn failed: {error}"));
                            }
                        }

                        // Update session cost
                        let pricing_model = if app.auto_model {
                            app.last_effective_model.as_deref().unwrap_or(&app.model)
                        } else {
                            &app.model
                        };
                        let turn_cost = crate::pricing::calculate_turn_cost_estimate_from_usage(
                            pricing_model,
                            &usage,
                        );
                        if let Some(cost) = turn_cost {
                            app.accrue_session_cost_estimate(cost);
                        }

                        // Emit OSC 9 / BEL desktop notification for long turns, and
                        // always stop the title animation that began on TurnStarted.
                        if status == crate::core::events::TurnOutcomeStatus::Completed {
                            if let Some((method, threshold, include_summary)) =
                                notifications::settings(config)
                            {
                                let in_tmux = std::env::var("TMUX").is_ok_and(|v| !v.is_empty());
                                let msg = notifications::completed_turn_message(
                                    app,
                                    &current_streaming_text,
                                    include_summary,
                                    turn_elapsed,
                                    turn_cost,
                                );
                                crate::tui::notifications::notify_done(
                                    method,
                                    in_tmux,
                                    &msg,
                                    threshold,
                                    turn_elapsed,
                                );
                                crate::tui::notifications::stop_title_animation();
                            } else {
                                crate::tui::notifications::stop_title_animation_quietly();
                            }
                        }

                        // Generate ghost-text follow-up suggestion asynchronously.
                        if status == crate::core::events::TurnOutcomeStatus::Completed
                            && config.prompt_suggestion_enabled()
                            && app.api_messages.len() >= 2
                        {
                            let suggestion_cell = app.prompt_suggestion_cell.clone();
                            let api_key = config.api_key().unwrap_or_default();
                            let base_url = config.api_base_url();
                            let model = config.default_model();
                            let messages: Vec<crate::models::Message> = app.api_messages.clone();
                            let gen_token = app
                                .prompt_suggestion_gen
                                .load(std::sync::atomic::Ordering::Relaxed);
                            if !api_key.is_empty() {
                                tokio::spawn(async move {
                                    let summary =
                                        crate::tui::prompt_suggestion::summarize_recent_messages(
                                            &messages, 8,
                                        );
                                    if let Some(suggestion) =
                                        crate::tui::prompt_suggestion::generate_suggestion(
                                            &api_key, &base_url, &model, &summary,
                                        )
                                        .await
                                        && let Ok(mut guard) = suggestion_cell.lock()
                                    {
                                        *guard = Some((gen_token, suggestion));
                                    }
                                });
                            }
                        }

                        // Generate post-turn receipt for completed turns.
                        // Also push a persistent status toast so users always
                        // see the outcome in the footer (not just the 8-second
                        // composer receipt), regardless of notification method
                        // or platform.
                        if status == crate::core::events::TurnOutcomeStatus::Completed {
                            // Debt ledger completion-gate: after every completed
                            // turn, check whether there are unresolved entries
                            // the agent should address before claiming the task is
                            // done (#2127). This runs autonomously — no tool call
                            // required — so the agent can't forget to check.
                            if let Ok(ledger) = crate::slop_ledger::SlopLedger::load()
                                && ledger.has_open_entries()
                                && let Some(gate_msg) = ledger.completion_gate_summary()
                            {
                                let short = gate_msg.lines().nth(4).unwrap_or("review before done");
                                app.push_status_toast(
                                    format!("⚠️ Debt ledger: {short}"),
                                    crate::tui::app::StatusToastLevel::Warning,
                                    Some(12_000),
                                );
                            }

                            let tool_count = app.tool_evidence.len();
                            let mut receipt = "✓ turn completed".to_string();
                            if tool_count > 0 {
                                let _ = write!(receipt, " · {tool_count} tool(s) used");
                                for evidence in &app.tool_evidence {
                                    let summary = crate::utils::truncate_with_ellipsis(
                                        &evidence.summary,
                                        60,
                                        "…",
                                    );
                                    let _ = write!(receipt, " · {}: {summary}", evidence.tool_name);
                                }
                            }
                            app.set_receipt_text(receipt.clone());
                            // Mirror as a persistent status toast (10s TTL).
                            // The footer bar visibly shows status toasts,
                            // which is more glanceable than the composer
                            // border receipt alone.
                            app.push_status_toast(
                                receipt,
                                crate::tui::app::StatusToastLevel::Info,
                                Some(10_000),
                            );
                        }

                        // Auto-save completed turn and clear crash checkpoint.
                        // Offloaded to the persistence actor so the UI
                        // stays responsive.
                        if let Ok(manager) = SessionManager::default_location() {
                            let session = build_session_snapshot(app, &manager);
                            app.current_session_id = Some(session.metadata.id.clone());
                            persistence_actor::persist(PersistRequest::SessionSnapshot(session));
                        }
                        persistence_actor::persist(PersistRequest::ClearCheckpoint);

                        // Refresh DeepSeek account balance after each completed
                        // turn so the footer balance chip stays current without
                        // adding latency to any request path.
                        let balance_cooldown_expired = app
                            .last_balance_fetch
                            .is_none_or(|t| t.elapsed() >= BALANCE_FETCH_COOLDOWN);
                        if balance_cooldown_expired && should_fetch_deepseek_balance(app) {
                            let cell = app.balance_cell.clone();
                            let api_key = config.api_key().unwrap_or_default();
                            let base_url = config.api_base_url();
                            if !api_key.is_empty() {
                                app.last_balance_fetch = Some(Instant::now());
                                let provider = balance_provider.clone();
                                tokio::spawn(async move {
                                    if let Some(info) =
                                        fetch_deepseek_balance(&*provider, &api_key, &base_url)
                                            .await
                                        && let Ok(mut guard) = cell.lock()
                                    {
                                        *guard = Some(info);
                                    }
                                });
                            }
                        }

                        if app.mode == AppMode::Plan
                            && app.plan_tool_used_in_turn
                            && !app.plan_prompt_pending
                            && app.queued_message_count() == 0
                            && app.queued_draft.is_none()
                        {
                            app.plan_prompt_pending = true;
                            app.add_message(HistoryCell::System {
                                content: plan_next_step_prompt(),
                            });
                            if app.view_stack.top_kind() != Some(ModalKind::PlanPrompt) {
                                let plan = Some(app.plan_state.lock().await.snapshot());
                                let todos = Some(app.todos.lock().await.snapshot());
                                app.view_stack
                                    .push(PlanPromptView::new(plan).with_todos(todos));
                            }
                        }
                        app.plan_tool_used_in_turn = false;

                        // Legacy pending-steer recovery. Current keyboard
                        // handling keeps Esc as cancel-only, but older saved
                        // state may still carry pending steers.
                        if status == crate::core::events::TurnOutcomeStatus::Interrupted
                            && app.submit_pending_steers_after_interrupt
                        {
                            if let Some(merged) = merge_pending_steers(&mut *app) {
                                queued_to_send = Some(merged);
                            }
                        } else if status == crate::core::events::TurnOutcomeStatus::Failed
                            && !app.pending_steers.is_empty()
                        {
                            // Hard-fail recovery: if the engine failed before
                            // a clean Interrupted landed, demote pending
                            // steers to the visible queue so they're not
                            // silently lost. User can /queue to inspect.
                            for msg in app.drain_pending_steers() {
                                app.queue_message(msg);
                            }
                        }

                        execute_turn_end_observer_hook(app, &usage, turn_elapsed, error.as_deref());

                        if queued_to_send.is_none() {
                            queued_to_send = app.pop_queued_message();
                        }
                    }
                    EngineEvent::Error {
                        envelope,
                        recoverable: _,
                    } => {
                        let provider_before_error = app.api_provider;
                        let rollback_after_auth_failure =
                            matches!(
                                envelope.category,
                                crate::error_taxonomy::ErrorCategory::Authentication
                            ) && app.pending_provider_switch.is_some();
                        apply_engine_error_to_app(app, envelope);
                        if app.api_provider != provider_before_error && app.is_fallback_active() {
                            fallback_after_engine_error = Some(provider_before_error);
                        }
                        if rollback_after_auth_failure
                            && let Some(rollback_warning) =
                                rollback_provider_after_auth_failure(app, config)
                        {
                            respawn_after_provider_rollback = Some(rollback_warning);
                        }
                    }
                    EngineEvent::Status { message } => {
                        app.status_message = Some(message);
                    }
                    EngineEvent::GoalUpdated { snapshot } => {
                        if apply_goal_snapshot_to_app(app, &snapshot) {
                            transcript_batch_updated = true;
                        }
                    }
                    EngineEvent::SessionUpdated {
                        session_id,
                        messages,
                        system_prompt,
                        model,
                        workspace,
                    } => {
                        app.current_session_id = Some(session_id);
                        app.api_messages = messages;
                        app.system_prompt = system_prompt;
                        if app.auto_model {
                            app.last_effective_model = Some(model);
                        } else {
                            app.set_model_selection(model);
                        }
                        app.update_model_compaction_budget();
                        app.workspace = workspace;
                        if (app.is_loading || app.is_compacting || app.is_purging)
                            && let Ok(manager) = SessionManager::default_location()
                        {
                            let session = build_session_snapshot(app, &manager);
                            app.session_title = Some(session.metadata.title.clone());
                            persistence_actor::persist(PersistRequest::Checkpoint(session));
                        } else if app.session_title.is_none() {
                            // First turn on a brand-new session: persist hasn't fired yet so
                            // read the title from the session file if it already exists,
                            // otherwise fall back to deriving from messages.
                            let persisted = app
                                .current_session_id
                                .as_deref()
                                .and_then(|id| {
                                    SessionManager::default_location()
                                        .ok()?
                                        .load_session(id)
                                        .ok()
                                })
                                .map(|s| s.metadata.title);
                            app.session_title =
                                persisted.or_else(|| derive_session_title(&app.api_messages));
                        }
                    }
                    EngineEvent::CompactionStarted { message, .. } => {
                        app.is_compacting = true;
                        app.status_message = Some(message);
                    }
                    EngineEvent::CompactionCompleted { message, .. } => {
                        app.is_compacting = false;
                        app.status_message = Some(message);
                    }
                    EngineEvent::CompactionFailed { message, .. } => {
                        app.is_compacting = false;
                        app.status_message = Some(message);
                    }
                    EngineEvent::PurgeStarted { message } => {
                        app.is_purging = true;
                        app.status_message = Some(message);
                    }
                    EngineEvent::PurgeCompleted { message, .. } => {
                        app.is_purging = false;
                        app.status_message = Some(message);
                    }
                    EngineEvent::PurgeFailed { message } => {
                        app.is_purging = false;
                        app.status_message = Some(message);
                    }
                    EngineEvent::PrefixCacheChange {
                        description,
                        stability_pct,
                        changed,
                        pinned_combined_hash,
                        ..
                    } => {
                        app.prefix_checks_total = app.prefix_checks_total.saturating_add(1);
                        app.prefix_stability_pct = Some(stability_pct);
                        app.last_pinned_prefix_hash =
                            (!pinned_combined_hash.is_empty()).then_some(pinned_combined_hash);
                        if changed {
                            app.prefix_change_count = app.prefix_change_count.saturating_add(1);
                            if !description.is_empty() {
                                app.last_prefix_change_desc = Some(description);
                            }
                        }
                    }
                    EngineEvent::PauseEvents { ack } => {
                        if !event_broker.is_paused() {
                            pause_terminal(
                                terminal,
                                app.use_alt_screen,
                                app.use_mouse_capture,
                                app.use_bracketed_paste,
                            )?;
                            event_broker.pause_events();
                            terminal_paused_at = Some(Instant::now());
                        }
                        if let Some(ack) = ack {
                            ack.notify_one();
                        }
                    }
                    EngineEvent::ResumeEvents => {
                        if event_broker.is_paused() {
                            resume_terminal(
                                terminal,
                                app.use_alt_screen,
                                app.use_mouse_capture,
                                app.use_bracketed_paste,
                                app.synchronized_output_enabled,
                            )?;
                            event_broker.resume_events();
                            terminal_paused_at = None;
                        }
                    }
                    EngineEvent::AgentSpawned {
                        id,
                        prompt,
                        parent_run_id,
                        spawn_depth,
                    } => {
                        let prompt_summary = summarize_tool_output(&prompt);
                        execute_subagent_observer_hook(
                            app,
                            HookEvent::SubagentSpawn,
                            &id,
                            "prompt",
                            &prompt,
                        );
                        app.agent_progress
                            .insert(id.clone(), format!("starting: {prompt_summary}"));
                        app.agent_progress_meta.insert(
                            id.clone(),
                            crate::tui::app::AgentProgressMeta {
                                parent_run_id,
                                spawn_depth,
                            },
                        );
                        if app.agent_activity_started_at.is_none() {
                            app.agent_activity_started_at = Some(Instant::now());
                        }
                        // #3030: Assign a stable user-facing label for this
                        // agent and keep the raw id out of the status bar.
                        let label = app.ensure_agent_label(&id);
                        app.status_message = Some(format!("{label} starting: {prompt_summary}"));
                        subagent_list_refresh_requested = true;
                    }
                    EngineEvent::AgentProgress {
                        id,
                        status,
                        parent_run_id,
                        spawn_depth,
                    } => {
                        let display = friendly_subagent_progress(app, &id, &status);
                        if is_noisy_subagent_progress(&status) {
                            app.agent_progress
                                .entry(id.clone())
                                .or_insert_with(|| display.clone());
                        } else {
                            app.agent_progress.insert(id.clone(), display.clone());
                        }
                        app.agent_progress_meta.insert(
                            id.clone(),
                            crate::tui::app::AgentProgressMeta {
                                parent_run_id,
                                spawn_depth,
                            },
                        );
                        if app.agent_activity_started_at.is_none() {
                            app.agent_activity_started_at = Some(Instant::now());
                        }
                        // #3030: progress can arrive before AgentSpawned is
                        // observed — assign the stable label on first sight.
                        let label = app.ensure_agent_label(&id);
                        app.status_message = Some(format!("{label}: {display}"));
                        // #3033: Throttle redraws from rapid AgentProgress events.
                        // When 4+ sub-agents are running concurrently, each firing
                        // progress events, the per-event `needs_redraw = true` saturates
                        // the render loop and starves terminal input.  Limit
                        // progress-driven repaints to at most one per 100ms; the
                        // status-animation timer (80ms cadence) provides a guaranteed
                        // floor for sidebar updates.  Data is still recorded immediately;
                        // the sidebar picks it up on the next permitted redraw.
                        if !agent_progress_redraw_permitted_for_drain(
                            &mut app.last_agent_progress_redraw,
                            &mut progress_redraw_agents,
                            &id,
                            Instant::now(),
                        ) {
                            // Restore the pre-event accumulator value: a
                            // throttled progress event contributes no redraw of
                            // its own, but earlier events' redraws survive.
                            received_engine_event = redraw_requested_before_event;
                        }
                    }
                    EngineEvent::AgentComplete { id, result } => {
                        execute_subagent_observer_hook(
                            app,
                            HookEvent::SubagentComplete,
                            &id,
                            "result",
                            &result,
                        );
                        let subagent_elapsed = app
                            .agent_activity_started_at
                            .or(app.turn_started_at)
                            .map(|started| started.elapsed())
                            .unwrap_or_default();
                        let has_other_running_subagents =
                            app.agent_progress.keys().any(|agent_id| agent_id != &id)
                                || app.subagent_cache.iter().any(|agent| {
                                    agent.agent_id != id
                                        && matches!(agent.status, SubAgentStatus::Running)
                                });
                        app.agent_progress.remove(&id);
                        app.agent_progress_meta.remove(&id);
                        // #3030: stable label with raw-id fallback.
                        let label = app.agent_display_label(&id);
                        app.status_message = Some(format!(
                            "{label} completed: {}",
                            summarize_tool_output(&result)
                        ));
                        let should_recapture_terminal =
                            !has_other_running_subagents && app.use_alt_screen;
                        if !has_other_running_subagents
                            && let Some((method, threshold, include_summary)) =
                                notifications::settings(config)
                        {
                            let in_tmux = std::env::var("TMUX").is_ok_and(|v| !v.is_empty());
                            let msg = notifications::subagent_completion_message(
                                app.ui_locale,
                                &id,
                                &result,
                                include_summary,
                                subagent_elapsed,
                            );
                            crate::tui::notifications::notify_done(
                                method,
                                in_tmux,
                                &msg,
                                threshold,
                                subagent_elapsed,
                            );
                        }
                        if should_recapture_terminal && event_broker.is_paused() {
                            resume_terminal(
                                terminal,
                                app.use_alt_screen,
                                app.use_mouse_capture,
                                app.use_bracketed_paste,
                                app.synchronized_output_enabled,
                            )?;
                            event_broker.resume_events();
                            terminal_paused_at = None;
                            app.needs_redraw = true;
                        }
                        subagent_list_refresh_requested = true;
                    }
                    EngineEvent::AgentList { agents } => {
                        let mut sorted = agents.clone();
                        sort_subagents_in_place(&mut sorted);
                        sorted.retain(|a| !a.from_prior_session);
                        app.subagent_cache = sorted.clone();
                        reconcile_subagent_activity_state(app);
                        let view_agents = subagent_view_agents(app, &app.subagent_cache);
                        if app.view_stack.update_subagents(&view_agents) {
                            app.status_message =
                                Some(format!("Fleet workers: {} total", view_agents.len()));
                        }
                        // Individual spawn/complete events already log to history;
                        // full list available via /agents command.
                    }
                    EngineEvent::SubAgentMailbox { seq, message } => {
                        let should_refresh_subagents =
                            subagent_message_refreshes_workspace_context(&message);
                        let updated_transcript = handle_subagent_mailbox(app, seq, &message);
                        if should_refresh_subagents {
                            subagent_list_refresh_requested = true;
                        }
                        if updated_transcript {
                            transcript_batch_updated = true;
                        } else if !should_refresh_subagents
                            && matches!(
                                message,
                                crate::tools::subagent::MailboxMessage::Progress { .. }
                            )
                        {
                            // Progress mailbox envelopes mirror AgentProgress.
                            // When the card state did not visibly change, do
                            // not let the duplicate envelope bypass the
                            // AgentProgress redraw throttle.
                            received_engine_event = redraw_requested_before_event;
                        }
                    }
                    EngineEvent::ApprovalRequired {
                        id,
                        tool_name,
                        description,
                        input,
                        approval_key,
                        approval_grouping_key,
                        intent_summary,
                        approval_force_prompt,
                    } => {
                        let session_denied = is_session_denied_for_key(app, &approval_key);
                        if session_denied {
                            // The user already said no to this exact tool /
                            // approval key in this session; auto-deny so the
                            // model's retry loop doesn't keep re-prompting
                            // (#360).
                            log_sensitive_event(
                                "tool.approval.auto_deny_session",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "approval_key": approval_key,
                                    "session_id": app.current_session_id,
                                }),
                            );
                            let _ = engine_handle.deny_tool_call(id.clone()).await;
                        } else if should_auto_approve_approval_request(
                            app,
                            &tool_name,
                            &approval_grouping_key,
                            approval_force_prompt,
                        ) {
                            log_sensitive_event(
                                "tool.approval.auto_approve",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "approval_key": approval_key,
                                    "session_id": app.current_session_id,
                                    "mode": app.mode.label(),
                                }),
                            );
                            let _ = engine_handle.approve_tool_call(id.clone()).await;
                        } else if app.approval_mode == ApprovalMode::Never {
                            log_sensitive_event(
                                "tool.approval.auto_deny",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "session_id": app.current_session_id,
                                    "mode": app.mode.label(),
                                }),
                            );
                            let _ = engine_handle.deny_tool_call(id.clone()).await;
                            app.status_message =
                                Some(format!("Blocked tool '{tool_name}' (approval_mode=never)"));
                        } else {
                            let tool_input = input;

                            push_approval_request_view(
                                app,
                                &id,
                                &tool_name,
                                &description,
                                &tool_input,
                                &approval_key,
                                intent_summary.as_deref(),
                            );
                            log_sensitive_event(
                                "tool.approval.prompted",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "description": description,
                                    "session_id": app.current_session_id,
                                    "mode": app.mode.label(),
                                }),
                            );
                            if let Some((method, _, _)) =
                                crate::tui::notifications::settings(config)
                            {
                                let in_tmux = std::env::var("TMUX").is_ok_and(|v| !v.is_empty());
                                crate::tui::notifications::notify_done(
                                    method,
                                    in_tmux,
                                    &format!("Approval needed: {tool_name} - {description}"),
                                    Duration::ZERO,
                                    Duration::ZERO,
                                );
                            }
                            app.status_message = Some(format!(
                                "Approval required for '{tool_name}': {description}"
                            ));
                        }
                    }
                    EngineEvent::UserInputRequired { id, request } => {
                        app.view_stack.push(UserInputView::new(id.clone(), request));
                        if let Some((method, _, _)) = crate::tui::notifications::settings(config) {
                            let in_tmux = std::env::var("TMUX").is_ok_and(|v| !v.is_empty());
                            crate::tui::notifications::notify_done(
                                method,
                                in_tmux,
                                "Action required: please respond in the terminal",
                                Duration::ZERO,
                                Duration::ZERO,
                            );
                        }
                        app.status_message = Some(
                            "Action required: answer the popup with 1-4, arrows, or Enter"
                                .to_string(),
                        );
                    }
                    EngineEvent::ElevationRequired {
                        tool_id,
                        tool_name,
                        command,
                        denial_reason,
                        blocked_network,
                        blocked_write,
                    } => {
                        // In YOLO mode, auto-elevate to full access
                        if app.approval_mode == ApprovalMode::Auto {
                            log_sensitive_event(
                                "tool.sandbox.auto_elevate",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "tool_id": tool_id,
                                    "reason": denial_reason,
                                    "session_id": app.current_session_id,
                                }),
                            );
                            app.add_message(HistoryCell::System {
                                content: format!(
                                    "Sandbox denied {tool_name}: {denial_reason} - auto-elevating to full access"
                                ),
                            });
                            // Auto-elevate to full access (no sandbox)
                            let policy = crate::sandbox::SandboxPolicy::DangerFullAccess;
                            let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                        } else {
                            log_sensitive_event(
                                "tool.sandbox.prompt_elevation",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "tool_id": tool_id,
                                    "reason": denial_reason,
                                    "session_id": app.current_session_id,
                                }),
                            );
                            // Show elevation dialog
                            let request = ElevationRequest::for_shell(
                                &tool_id,
                                command.as_deref().unwrap_or(&tool_name),
                                &denial_reason,
                                blocked_network,
                                blocked_write,
                            );
                            app.view_stack
                                .push(ElevationView::new(request, app.ui_locale));
                            if let Some((method, _, _)) =
                                crate::tui::notifications::settings(config)
                            {
                                let in_tmux = std::env::var("TMUX").is_ok_and(|v| !v.is_empty());
                                crate::tui::notifications::notify_done(
                                    method,
                                    in_tmux,
                                    &format!("Sandbox: {denial_reason} for '{tool_name}'"),
                                    Duration::ZERO,
                                    Duration::ZERO,
                                );
                            }
                            app.status_message =
                                Some(format!("Sandbox blocked {tool_name}: {denial_reason}"));
                        }
                    }
                }
            }
        }
        if let Some(previous_provider) = fallback_after_engine_error {
            apply_provider_fallback_switch(app, &mut engine_handle, config, previous_provider)
                .await;
        }
        if let Some(rollback_warning) = respawn_after_provider_rollback {
            let _ = engine_handle.send(Op::Shutdown).await;
            let engine_config = build_engine_config(app, config);
            engine_handle = spawn_engine(engine_config, config);
            if !app.api_messages.is_empty() {
                let _ = engine_handle
                    .send(Op::SyncSession {
                        session_id: app.current_session_id.clone(),
                        messages: app.api_messages.clone(),
                        system_prompt: app.system_prompt.clone(),
                        system_prompt_override: false,
                        model: app.model.clone(),
                        workspace: app.workspace.clone(),
                    })
                    .await;
            }
            let _ = engine_handle
                .send(Op::SetCompaction {
                    config: app.compaction_config(),
                })
                .await;
            app.status_message = Some(rollback_warning);
        }
        if let Some(index) = app.streaming_message_index {
            let committed = app.streaming_state.commit_text(0);
            if !committed.is_empty() {
                append_streaming_text(app, index, &committed);
                transcript_batch_updated = true;
            }
        } else if let Some(entry_idx) = app.streaming_thinking_active_entry {
            let committed = app.streaming_state.commit_text(0);
            if !committed.is_empty() {
                if app.translation_enabled {
                    streaming_thinking::set_placeholder(app, entry_idx);
                } else {
                    streaming_thinking::append(app, entry_idx, &committed);
                }
                transcript_batch_updated = true;
            }
        }
        if transcript_batch_updated {
            app.mark_history_updated();
        }
        if received_engine_event {
            app.needs_redraw = true;
        }
        // #freeze: one trailing-edge sub-agent list refresh per drain, no
        // matter how many spawn/complete/mailbox events arrived this batch.
        if subagent_list_refresh_requested {
            let _ = engine_handle.send(Op::ListSubAgents).await;
        }

        if let Some(next) = queued_to_send {
            if let Err(err) = dispatch_user_message(app, config, &engine_handle, next.clone()).await
            {
                app.queue_message(next);
                app.status_message = Some(format!(
                    "Dispatch failed ({err}); kept {} queued message(s)",
                    app.queued_message_count()
                ));
            }

            app.needs_redraw = true;
        }

        let queue_state = (app.queued_messages.clone(), app.queued_draft.clone());
        if queue_state != last_queue_state {
            persist_offline_queue_state(app);
            last_queue_state = queue_state;
            app.needs_redraw = true;
        }

        if !app.view_stack.is_empty() {
            let events = app.view_stack.tick();
            if !events.is_empty() {
                app.needs_redraw = true;
            }
            if handle_view_events(
                terminal,
                app,
                config,
                &task_manager,
                &mut engine_handle,
                &mut web_config_session,
                events,
            )
            .await?
            {
                return Ok(());
            }
        }

        let has_running_agents = running_agent_count(app) > 0;
        if reconcile_turn_liveness(app, Instant::now(), has_running_agents) {
            app.needs_redraw = true;
        }
        if (app.is_loading || has_running_agents || app.is_compacting || app.is_purging)
            && last_status_frame.elapsed()
                >= Duration::from_millis(status_animation_interval_ms(app))
        {
            if streaming_thinking::animate_pending_translation(
                app,
                pending_thinking_translations > 0,
            ) {
                app.mark_history_updated();
            }
            if !app.low_motion && history_has_live_motion(&app.history) {
                app.mark_history_updated();
            }
            app.needs_redraw = true;
            last_status_frame = Instant::now();
        }

        if event_broker.is_paused() {
            let grace_active = terminal_paused_at
                .map(|paused_at| paused_at.elapsed() < Duration::from_millis(500))
                .unwrap_or(false);
            if terminal_pause_has_live_owner(app) || grace_active {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
            resume_terminal(
                terminal,
                app.use_alt_screen,
                app.use_mouse_capture,
                app.use_bracketed_paste,
                app.synchronized_output_enabled,
            )?;
            event_broker.resume_events();
            terminal_paused_at = None;
            app.status_message = Some("Terminal controls restored".to_string());
            app.needs_redraw = true;
            force_terminal_repaint = true;
        }

        let now = Instant::now();
        app.flush_paste_burst_if_enabled(now);
        app.sync_status_message_to_toasts();
        // Drain background-LLM cost (compaction summaries, seam
        // recompaction, cycle briefings) accumulated since the last
        // tick and fold it into the session-cost counter (#526).
        // Background callers populate `cost_status::report`; we sweep
        // the pool once per loop iteration so the footer chip matches
        // the DeepSeek website's billing.
        let pending_bg_cost = crate::cost_status::drain();
        if pending_bg_cost.is_positive() {
            app.accrue_subagent_cost_estimate(pending_bg_cost);
            app.needs_redraw = true;
        }
        // Expire the "Press Ctrl+C again to quit" prompt silently after its
        // window. Triggers a redraw if the prompt was visible.
        app.tick_quit_armed();
        app.tick_receipt();
        crate::tui::footer_ui::maybe_log_provider_wait_incident(app);
        // While the user is drag-selecting past the transcript edge, advance
        // the viewport on a fixed cadence and extend the selection head so a
        // long passage can be selected in one drag (#1163).
        tick_selection_autoscroll(app);
        let allow_workspace_context_refresh =
            !app.is_loading && !has_running_agents && !app.is_compacting && !app.is_purging;
        workspace_context::refresh_if_needed(app, now, allow_workspace_context_refresh);

        // Draw is gated by the frame-rate limiter (120 FPS cap). When a
        // redraw is needed but the limiter says we're inside the cooldown
        // window, leave `needs_redraw = true` and shorten the poll timeout
        // so the loop wakes up exactly when drawing is allowed.

        // Sync low-motion flag into the frame-rate limiter and streaming
        // chunking policy. Low-motion mode drops the frame cap to 30 FPS
        // and forces Smooth-only chunking so the display stays calm.
        frame_rate_limiter.set_low_motion(app.low_motion);
        app.streaming_state.set_low_motion(app.low_motion);

        let draw_wait = if app.needs_redraw {
            frame_rate_limiter.time_until_next_draw(now)
        } else {
            None
        };
        // Merge the per-app full-repaint hint (set by theme switches)
        // into the loop-level flag before the draw decision.
        if app.force_next_full_repaint {
            force_terminal_repaint = true;
            app.force_next_full_repaint = false;
        }
        if app.needs_redraw && draw_wait.is_none() {
            let was_full_repaint = force_terminal_repaint;
            draw_app_frame_inner(terminal, app, force_terminal_repaint)?;
            force_terminal_repaint = false;
            if was_full_repaint {
                draws_since_last_full_repaint = 0;
            } else {
                draws_since_last_full_repaint = draws_since_last_full_repaint.saturating_add(1);
            }
            frame_rate_limiter.mark_emitted(Instant::now());
            app.needs_redraw = false;
        }

        let mut poll_timeout =
            if app.is_loading || has_running_agents || app.is_compacting || app.is_purging {
                Duration::from_millis(active_poll_ms(app))
            } else {
                Duration::from_millis(idle_poll_ms(app))
            };
        if let Some(until_flush) = app.paste_burst_next_flush_delay_if_enabled(now) {
            poll_timeout = poll_timeout.min(until_flush);
        }
        if let Some(until_draw) = draw_wait {
            poll_timeout = poll_timeout.min(until_draw);
        }
        if web_config_session.is_some() {
            poll_timeout = poll_timeout.min(Duration::from_millis(WEB_CONFIG_POLL_MS));
        }
        // While the quit-confirmation prompt is armed, ensure we wake up to
        // expire it on time even if no input event arrives.
        if let Some(deadline) = app.quit_armed_until {
            let remaining = deadline.saturating_duration_since(now);
            poll_timeout = poll_timeout.min(remaining.max(Duration::from_millis(50)));
        }
        // Drag-edge auto-scroll wakes the loop on its own cadence so the
        // viewport keeps advancing while the user holds the mouse outside
        // the transcript rect (#1163).
        if let Some(state) = app.viewport.selection_autoscroll {
            let remaining = state.next_tick.saturating_duration_since(now);
            poll_timeout = poll_timeout.min(remaining);
        }
        poll_timeout = clamp_event_poll_timeout(poll_timeout);

        // #549/#3216: give the engine task a scheduler turn before waiting on
        // the terminal-input channel. Crossterm's blocking poll/read runs on
        // `TerminalInputPump`, so engine floods cannot pin the OS input read.
        tokio::task::yield_now().await;

        let maybe_terminal_event =
            next_terminal_event(&terminal_input, &mut pending_terminal_events, poll_timeout)?;
        if maybe_terminal_event.is_none() {
            let now = Instant::now();
            let input_stalled_for = terminal_input.stalled_for(now);
            if terminal_input_recovery_relevant(app, has_running_agents)
                && input_stalled_for >= TERMINAL_INPUT_STALL_TIMEOUT
                && now.duration_since(last_terminal_input_recovery)
                    >= TERMINAL_INPUT_RECOVERY_COOLDOWN
            {
                tracing::warn!(
                    stalled_ms = input_stalled_for.as_millis(),
                    "terminal input pump heartbeat stalled; attempting terminal input recovery"
                );
                recover_terminal_modes(
                    terminal.backend_mut(),
                    app.use_mouse_capture,
                    app.use_bracketed_paste,
                );
                #[cfg(not(target_os = "windows"))]
                {
                    app.push_status_toast(
                        "Terminal input heartbeat stalled; terminal modes were refreshed.",
                        StatusToastLevel::Warning,
                        None,
                    );
                }
                terminal_input.mark_alive();
                last_terminal_input_recovery = now;
                force_terminal_repaint = true;
                app.needs_redraw = true;
            }
        }

        if let Some(evt) = maybe_terminal_event {
            app.needs_redraw = true;

            // Handle bracketed paste events
            if let Event::Paste(text) = &evt {
                tracing::debug!(
                    paste_len = text.len(),
                    preview = %text.chars().take(80).collect::<String>(),
                    "Received bracketed paste event"
                );
                // Once a real bracketed-paste event has been observed in
                // this session, the rapid-keystroke heuristic in
                // paste_burst is redundant — disable it so fast typing /
                // IME commits / autocomplete bursts don't get
                // mis-classified as a paste.
                app.bracketed_paste_seen = true;
                if app.onboarding == OnboardingState::ApiKey {
                    // Paste into API key input
                    app.insert_api_key_str(text);
                    onboarding::sync_api_key_validation_status(app, false);
                } else if app.is_history_search_active() {
                    app.history_search_insert_str(text);
                } else if app.view_stack.handle_paste(text) {
                    // Modal consumed the paste (e.g. provider picker key entry)
                } else if !app.view_stack.is_empty() {
                    // A non-consumed modal is open — don't leak paste into composer
                } else {
                    // Paste into main input
                    app.insert_paste_text(text);
                }
                continue;
            }

            // Re-establish terminal mode flags on focus-gain and force a full
            // viewport reset before repainting. App-switching and interactive
            // handoffs can leave the host terminal scrolled away from row 0
            // and (on macOS) can drop the keyboard, mouse-tracking, or
            // bracketed-paste modes — recover_terminal_modes() is the
            // canonical place those flags live.
            if terminal_event_needs_viewport_recapture(&evt) {
                let now = Instant::now();
                if now.duration_since(last_focus_recovery) >= FOCUS_RECOVERY_DEBOUNCE {
                    recover_terminal_modes(
                        terminal.backend_mut(),
                        app.use_mouse_capture,
                        app.use_bracketed_paste,
                    );
                    last_focus_recovery = now;
                }
                force_terminal_repaint = true;
                app.needs_redraw = true;
            }
            if let Event::Resize(width, height) = evt {
                tracing::debug!(
                    width,
                    height,
                    use_alt_screen = app.use_alt_screen,
                    "Event::Resize received; clearing terminal"
                );
                // Drain any further Resize events queued in this poll cycle so we
                // act on the final size only, then issue a single clear + redraw.
                // crossterm coalesces some resize events but rapid drag-resizes
                // can still queue several; processing them all here avoids the
                // common "stale art on the right edge" symptom (#65) caused by
                // the diff renderer skipping cells that match a stale back
                // buffer between intermediate sizes.
                let mut final_w = width;
                let mut final_h = height;
                while let Some(next_evt) =
                    try_next_terminal_event(&terminal_input, &mut pending_terminal_events)?
                {
                    match next_evt {
                        Event::Resize(w, h) => {
                            final_w = w;
                            final_h = h;
                        }
                        other => {
                            pending_terminal_events.push_back(other);
                            break;
                        }
                    }
                }

                if final_w == 0 || final_h == 0 {
                    tracing::debug!(
                        final_w,
                        final_h,
                        "zero-size Resize event ignored while terminal is hidden/minimized"
                    );
                    force_terminal_repaint = true;
                    app.needs_redraw = true;
                    continue;
                }

                // #582: commit the event-reported size to ratatui's
                // viewport explicitly before the redraw, instead of
                // relying on `crossterm::terminal::size()` which gets
                // queried internally during `terminal.draw`. On
                // Windows ConHost specifically, `terminal::size()` has
                // been observed to return stale dimensions briefly
                // during a maximize→windowed transition; the next
                // `draw` then paints into a buffer that does not
                // match the post-restore viewport, producing the
                // unrecoverable black screen reported by @imakid.
                // The `Event::Resize` payload itself carries the
                // authoritative new size, so we forward it.
                if let Err(err) = terminal.resize(Rect::new(0, 0, final_w, final_h)) {
                    tracing::warn!(
                        ?err,
                        final_w,
                        final_h,
                        "terminal.resize during Resize event failed; falling back to clear+draw"
                    );
                }

                app.handle_resize(final_w, final_h);
                // #macos-resize: some terminals (macOS Terminal.app, Windows
                // ConHost) briefly report stale dimensions via
                // `terminal::size()` after a resize. ratatui's `draw()` calls
                // `autoresize()` internally, which queries the backend size;
                // if it sees the old dimension it shrinks the viewport back,
                // leaving the newly-expanded area filled with stale content
                // from the previous frame (duplicate UI panels).
                //
                // We force the backend to report the resize-event size for
                // this single draw so the buffer matches the real viewport.
                {
                    let backend = terminal.backend_mut();
                    let new_size = Size::new(final_w, final_h);
                    backend.force_size(new_size);
                    backend.set_terminal_size(new_size);
                }
                draw_app_frame_inner(terminal, app, true)?;
                draws_since_last_full_repaint = 0;
                {
                    let backend = terminal.backend_mut();
                    backend.clear_forced_size();
                }
                app.needs_redraw = false;
                continue;
            }

            if app.use_mouse_capture
                && let Event::Mouse(mouse) = evt
            {
                // Mouse interaction clears the ✅ completion marker.
                crate::tui::notifications::reset_title_on_interaction();
                if should_drop_loading_mouse_motion(app, mouse) {
                    continue;
                }
                let events = handle_mouse_event(app, mouse);
                if handle_view_events(
                    terminal,
                    app,
                    config,
                    &task_manager,
                    &mut engine_handle,
                    &mut web_config_session,
                    events,
                )
                .await?
                {
                    return Ok(());
                }
                persist_sidebar_settings_if_dirty(app);
                continue;
            }

            // User interaction — clear the ✅ completion marker from the title.
            crate::tui::notifications::reset_title_on_interaction();

            let Event::Key(mut key) = evt else {
                continue;
            };

            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Normalize macOS modifiers: map SUPER (Cmd) to CONTROL so that
            // keyboard shortcuts work consistently across terminal emulators
            // (Terminal.app, iTerm2, Kitty, etc.) that may report different
            // modifier flags (#2938).
            let mapped = crate::tui::composer_ui::normalize_macos_modifiers(key.modifiers);
            key.modifiers = mapped;

            // Decision card keyboard routing (v0.8.43 truth-surface).
            // When a card is active, number keys 1-9 select options,
            // j/k or Up/Down navigate, and Enter confirms.
            // Only route keys to the decision card when no other modal
            // (Help, Config, Pager, etc.) is on top of the view stack (#2005).
            if app.view_stack.is_empty()
                && let Some(card) = app.decision_card.as_mut()
            {
                match key.code {
                    KeyCode::Char(c @ '1'..='9') => {
                        let n = (c as u8 - b'1' + 1) as usize;
                        card.select_number(n);
                        card.confirm();
                        app.status_message = card
                            .confirmed_label()
                            .map(|label| format!("Selected: {label}"));
                        app.decision_card = None;
                        app.needs_redraw = true;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        card.select_next();
                        app.needs_redraw = true;
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        card.select_prev();
                        app.needs_redraw = true;
                    }
                    KeyCode::Enter => {
                        card.confirm();
                        app.status_message = card
                            .confirmed_label()
                            .map(|label| format!("Selected: {label}"));
                        app.decision_card = None;
                        app.needs_redraw = true;
                    }
                    KeyCode::Esc => {
                        app.decision_card = None;
                        app.status_message = Some("Decision cancelled".to_string());
                        app.needs_redraw = true;
                    }
                    _ => {}
                }
                submit_initial_input_if_ready(app, config, &engine_handle).await?;
                continue;
            }

            // Handle onboarding flow
            if app.onboarding != OnboardingState::None {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let _ = engine_handle.send(Op::Shutdown).await;
                        return Ok(());
                    }
                    KeyCode::Esc if app.onboarding == OnboardingState::ApiKey => {
                        app.onboarding = OnboardingState::Welcome;
                        app.api_key_input.clear();
                        app.api_key_cursor = 0;
                        app.status_message = None;
                    }
                    KeyCode::Esc if app.onboarding == OnboardingState::Language => {
                        app.onboarding = OnboardingState::Welcome;
                        app.status_message = None;
                    }
                    // Language picker hotkeys select + persist (#566).
                    //
                    // Note: this used to be a single match-guard with `&& let`,
                    // but `if_let_guard` is a nightly-only feature on Rust
                    // before 1.94. Rewriting as a plain guard + nested `if let`
                    // keeps `cargo install` working on stable.
                    KeyCode::Char(c)
                        if app.onboarding == OnboardingState::Language && c.is_ascii_digit() =>
                    {
                        if let Some((_, tag, _, _)) = onboarding::language::LANGUAGE_OPTIONS
                            .iter()
                            .find(|(hotkey, _, _, _)| *hotkey == c)
                        {
                            match app.set_locale_from_onboarding(tag) {
                                Ok(()) => {
                                    app.push_status_toast(
                                        format!("Language set to {tag}"),
                                        StatusToastLevel::Info,
                                        Some(2_500),
                                    );
                                    onboarding::advance_onboarding_after_language(app);
                                }
                                Err(err) => {
                                    app.status_message =
                                        Some(format!("Failed to save locale: {err}"));
                                }
                            }
                        }
                    }
                    KeyCode::Enter => match app.onboarding {
                        OnboardingState::Welcome => {
                            onboarding::advance_onboarding_from_welcome(app);
                        }
                        OnboardingState::Language => {
                            // Enter without a digit pick keeps the existing
                            // setting (which defaults to "auto").
                            onboarding::advance_onboarding_after_language(app);
                        }
                        OnboardingState::ApiKey => {
                            let key = app.api_key_input.trim().to_string();
                            if let onboarding::ApiKeyValidation::Reject(message) =
                                onboarding::validate_api_key_for_onboarding(&key)
                            {
                                app.status_message = Some(message);
                                continue;
                            }
                            match app.submit_api_key() {
                                Ok(saved) => {
                                    // Surface where the key landed so the
                                    // user can verify the shared config
                                    // file path before the welcome
                                    // screen advances. The toast queue
                                    // outlives the onboarding state
                                    // transition, so it stays visible on
                                    // the next screen too.
                                    app.push_status_toast(
                                        format!("API key saved to {}", saved.describe()),
                                        StatusToastLevel::Info,
                                        Some(4_000),
                                    );
                                    app.status_message = None;
                                    // Recreate the engine so it picks up the newly saved key
                                    // without requiring a full process restart.
                                    let _ = engine_handle.send(Op::Shutdown).await;
                                    // Stamp the new key on the long-lived
                                    // `Config` reference so any future clone
                                    // (e.g. a subsequent /provider switch)
                                    // sees it; the explicit-override path
                                    // in `api_key` (#343) makes
                                    // this win immediately.
                                    config.api_key = Some(key.clone());
                                    let mut refreshed_config = config.clone();
                                    refreshed_config.api_key = Some(key);
                                    let engine_config = build_engine_config(app, &refreshed_config);
                                    engine_handle = spawn_engine(engine_config, &refreshed_config);
                                    app.offline_mode = false;
                                    app.api_key_env_only = false;

                                    if !app.api_messages.is_empty() {
                                        let _ = engine_handle
                                            .send(Op::SyncSession {
                                                session_id: app.current_session_id.clone(),
                                                messages: app.api_messages.clone(),
                                                system_prompt: app.system_prompt.clone(),
                                                system_prompt_override: false,
                                                model: app.model.clone(),
                                                workspace: app.workspace.clone(),
                                            })
                                            .await;
                                    }

                                    onboarding::advance_onboarding_after_language(app);
                                }
                                Err(e) => {
                                    app.status_message = Some(e.to_string());
                                }
                            }
                        }
                        OnboardingState::TrustDirectory => {}
                        OnboardingState::Tips => {
                            app.finish_onboarding();
                        }
                        OnboardingState::None => {}
                    },
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('1')
                        if app.onboarding == OnboardingState::TrustDirectory =>
                    {
                        match onboarding::mark_trusted(&app.workspace) {
                            Ok(_) => {
                                app.trust_mode = true;
                                app.hooks = HookExecutor::new(
                                    crate::hooks::HooksConfig::load_with_project(
                                        config.hooks_config(),
                                        &app.workspace,
                                    ),
                                    app.workspace.clone(),
                                );
                                app.runtime_services.hook_executor =
                                    Some(std::sync::Arc::new(app.hooks.clone()));
                                app.status_message = None;
                                if app.onboarding_workspace_trust_gate {
                                    app.onboarding_workspace_trust_gate = false;
                                    app.onboarding = OnboardingState::None;
                                } else {
                                    app.onboarding = OnboardingState::Tips;
                                }
                            }
                            Err(err) => {
                                app.status_message =
                                    Some(format!("Failed to trust workspace: {err}"));
                            }
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('2')
                        if app.onboarding == OnboardingState::TrustDirectory =>
                    {
                        let _ = engine_handle.send(Op::Shutdown).await;
                        return Ok(());
                    }
                    KeyCode::Backspace if app.onboarding == OnboardingState::ApiKey => {
                        app.delete_api_key_char();
                        onboarding::sync_api_key_validation_status(app, false);
                    }
                    KeyCode::Char('h')
                        if key_shortcuts::is_ctrl_h_backspace(&key)
                            && app.onboarding == OnboardingState::ApiKey =>
                    {
                        app.delete_api_key_char();
                        onboarding::sync_api_key_validation_status(app, false);
                    }
                    _ if key_shortcuts::is_paste_shortcut(&key)
                        && app.onboarding == OnboardingState::ApiKey =>
                    {
                        // Cmd+V / Ctrl+V paste (bracketed paste handled above)
                        app.paste_api_key_from_clipboard();
                        onboarding::sync_api_key_validation_status(app, false);
                    }
                    KeyCode::Char(c)
                        if app.onboarding == OnboardingState::ApiKey
                            && key_shortcuts::is_text_input_key(&key) =>
                    {
                        app.insert_api_key_char(c);
                        onboarding::sync_api_key_validation_status(app, false);
                    }
                    _ => {}
                }
                continue;
            }

            if key.code == KeyCode::F(1) {
                if app.view_stack.top_kind() == Some(ModalKind::Help) {
                    app.view_stack.pop();
                } else {
                    app.view_stack.push(HelpView::new_for_locale(app.ui_locale));
                }
                continue;
            }

            if key.code == KeyCode::Char('/') && key.modifiers.contains(KeyModifiers::CONTROL) {
                if app.view_stack.top_kind() == Some(ModalKind::Help) {
                    app.view_stack.pop();
                } else {
                    app.view_stack.push(HelpView::new_for_locale(app.ui_locale));
                }
                continue;
            }

            if key.code == KeyCode::Char('x')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && prefill_jobs_cancel_all_if_tasks_sidebar(app)
            {
                continue;
            }

            if key.code == KeyCode::Char('k') && key.modifiers.contains(KeyModifiers::CONTROL) {
                // When the composer is the active input target (no modal/pager
                // intercepting keys), Ctrl+K performs an emacs-style kill to
                // end-of-line. If the kill is a no-op (cursor at end of empty
                // input), fall through to the existing command palette.
                if app.view_stack.is_empty() && app.kill_to_end_of_line() {
                    continue;
                }
                app.view_stack
                    .push(CommandPaletteView::new(build_command_palette_entries(
                        app.ui_locale,
                        &app.skills_dir,
                        app.skills_scan_mimofan_only,
                        &app.workspace,
                        &app.mcp_config_path,
                        app.mcp_snapshot.as_ref(),
                    )));
                continue;
            }

            // y / Y in the Tasks sidebar: yank the current turn id (y)
            // or copy full task detail (Y) to the system clipboard.
            // Only active when the composer is empty to avoid stealing
            // keystrokes from typed input (#2000).
            if app.view_stack.is_empty()
                && app.sidebar_focus == SidebarFocus::Tasks
                && app.input.is_empty()
                && !app.runtime_turn_id.as_deref().unwrap_or("").is_empty()
            {
                if key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::NONE {
                    if let Some(turn_id) = app.runtime_turn_id.as_ref()
                        && app.clipboard.write_text(turn_id).is_ok()
                    {
                        app.status_message = Some(format!("Copied turn id {turn_id}"));
                    }
                    continue;
                }
                if key.code == KeyCode::Char('Y') && key.modifiers == KeyModifiers::NONE {
                    let mut detail = String::new();
                    if let Some(turn_id) = app.runtime_turn_id.as_ref() {
                        let _ = write!(detail, "turn {turn_id}");
                    }
                    if let Some(status) = app.runtime_turn_status.as_deref() {
                        let _ = write!(detail, "  status={status}");
                    }
                    if !detail.is_empty() && app.clipboard.write_text(&detail).is_ok() {
                        app.status_message = Some(format!("Copied {detail}"));
                    }
                    continue;
                }
            }

            // Shifted shortcuts toggle the file-tree pane. Keep plain Ctrl+E
            // reserved for the composer end-of-line binding used by shells.
            if key_shortcuts::is_file_tree_toggle_shortcut(&key) {
                if let Some(_state) = app.file_tree.as_mut() {
                    // File tree visible → hide it.
                    app.file_tree = None;
                    app.status_message = Some("File tree closed".to_string());
                } else {
                    // Build the file tree from the current workspace.
                    let state = crate::tui::file_tree::FileTreeState::new(&app.workspace);
                    app.file_tree = Some(state);
                    app.status_message = Some(
                        "File tree: \u{2191}/\u{2193} navigate  Enter select  Esc close"
                            .to_string(),
                    );
                }
                app.needs_redraw = true;
                continue;
            }

            // Ctrl+P opens the fuzzy file-picker overlay. Bound only when the
            // composer is focused (no other modal or inline popup on top) and the
            // engine is not actively streaming a turn.
            if key.code == KeyCode::Char('p')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && visible_slash_menu_entries(app, SLASH_MENU_LIMIT).is_empty()
                && app.view_stack.is_empty()
                && !app.is_loading
            {
                file_picker_relevance::open_file_picker(app);
                continue;
            }

            if matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && app.view_stack.is_empty()
            {
                app.status_message = Some(if app.is_compacting {
                    "Context compaction already in progress...".to_string()
                } else {
                    "Compacting context (Ctrl+L)...".to_string()
                });
                if !app.is_compacting {
                    let _ = engine_handle.send(Op::CompactContext).await;
                }
                app.needs_redraw = true;
                continue;
            }

            if matches!(key.code, KeyCode::Char('b') | KeyCode::Char('B'))
                && key_shortcuts::has_control_like_modifier(key.modifiers)
                && app.view_stack.is_empty()
            {
                // #3032: Ctrl+B directly backgrounds the active foreground
                // shell command instead of opening a two-step shell-control
                // menu.  When nothing is backgroundable, the status message
                // tells the user what's going on.
                request_foreground_shell_background(app);
                app.needs_redraw = true;
                continue;
            }

            if matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
                && key.modifiers.contains(KeyModifiers::ALT)
                && !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::SUPER)
                && app.view_stack.is_empty()
            {
                open_context_inspector(app);
                continue;
            }

            if !app.view_stack.is_empty() {
                let events = app.view_stack.handle_key(key);
                app.needs_redraw = true;
                if handle_view_events(
                    terminal,
                    app,
                    config,
                    &task_manager,
                    &mut engine_handle,
                    &mut web_config_session,
                    events,
                )
                .await?
                {
                    return Ok(());
                }
                persist_sidebar_settings_if_dirty(app);
                continue;
            }

            if let Some(slot) = hotbar_slot_from_key(app, &key) {
                if let Some(dispatch) = dispatch_hotbar_slot(app, config, slot)? {
                    match dispatch {
                        HotbarDispatch::Handled => {
                            app.needs_redraw = true;
                        }
                        HotbarDispatch::AppAction(action) => {
                            if apply_command_result(
                                terminal,
                                app,
                                &mut engine_handle,
                                &task_manager,
                                config,
                                &mut web_config_session,
                                commands::CommandResult::action(action),
                            )
                            .await?
                            {
                                return Ok(());
                            }
                            app.needs_redraw = true;
                        }
                    }
                }
                continue;
            }

            // File-tree navigation: delegated to key_actions module.
            if key_actions::handle_file_tree_key(app, &key) {
                continue;
            }

            if app.is_history_search_active() {
                handle_history_search_key(app, key);
                continue;
            }

            if matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
                && key.modifiers.contains(KeyModifiers::ALT)
                && !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::SUPER)
            {
                app.start_history_search();
                continue;
            }

            let now = Instant::now();
            app.flush_paste_burst_if_enabled(now);

            // On Windows, AltGr is delivered as `Ctrl+Alt`; treat
            // AltGr-typed chars (e.g. European layouts producing `@`, `\`,
            // `|`) as plain text rather than swallowing them as a modified
            // shortcut. `key_hint::has_ctrl_or_alt` filters AltGr out.
            let has_ctrl_alt_or_super = super::widgets::key_hint::has_ctrl_or_alt(key.modifiers)
                || key.modifiers.contains(KeyModifiers::SUPER);
            let is_plain_char = matches!(key.code, KeyCode::Char(_)) && !has_ctrl_alt_or_super;
            let is_enter = matches!(key.code, KeyCode::Enter);

            if key_shortcuts::is_macos_option_v_legacy_key(&key) {
                open_tool_details_pager(app);
                continue;
            }

            if !is_plain_char
                && !is_enter
                && let Some(pending) = app.flush_paste_burst_before_modified_input_if_enabled()
            {
                app.insert_str(&pending);
            }

            if (is_plain_char || is_enter) && super::paste::handle_paste_burst_key(app, &key, now) {
                continue;
            }

            let slash_menu_entries = visible_slash_menu_entries(app, SLASH_MENU_LIMIT);
            let slash_menu_open = !slash_menu_entries.is_empty();
            if slash_menu_open && app.slash_menu_selected >= slash_menu_entries.len() {
                app.slash_menu_selected = slash_menu_entries.len().saturating_sub(1);
            }
            let mention_menu_limit = app.mention_limit;
            let mention_menu_entries =
                crate::tui::file_mention::visible_mention_menu_entries(app, mention_menu_limit);
            let mention_menu_open = !mention_menu_entries.is_empty();
            if mention_menu_open && app.mention_menu_selected >= mention_menu_entries.len() {
                app.mention_menu_selected = mention_menu_entries.len().saturating_sub(1);
            }

            // Cancel a pending Esc-Esc prime as soon as any non-Esc key
            // arrives. Without this the prime would hang around for the
            // rest of the session and the user's next genuine Esc would
            // suddenly skip straight into the backtrack overlay.
            if !matches!(key.code, KeyCode::Esc)
                && matches!(
                    app.backtrack.phase,
                    crate::tui::backtrack::BacktrackPhase::Primed
                )
            {
                app.backtrack.reset();
            }

            // Global keybindings
            match key.code {
                KeyCode::Enter
                    if app.input.is_empty()
                        && app.viewport.transcript_selection.is_active()
                        && open_pager_for_selection(app) =>
                {
                    continue;
                }
                KeyCode::Enter
                    if key.modifiers == KeyModifiers::NONE
                        && app.input.is_empty()
                        && detail_target_cell_index(app)
                            .is_some_and(|idx| app.toggle_tool_run_expansion_at(idx)) =>
                {
                    continue;
                }
                KeyCode::Char('l')
                    if key_shortcuts::alt_nav_modifiers(key.modifiers)
                        && app.input.is_empty()
                        && open_pager_for_last_message(app) =>
                {
                    continue;
                }
                // This detail shortcut intentionally precedes vim-normal-mode
                // handling: visual selection has no useful empty-composer
                // target, while selected tool cards do.
                KeyCode::Char('v')
                    if key.modifiers == KeyModifiers::NONE
                        && app.input.is_empty()
                        && detail_target_cell_index(app).is_some() =>
                {
                    open_tool_details_pager(app);
                    continue;
                }
                KeyCode::Char('o')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && app.input.is_empty()
                        && open_activity_detail_pager(app) =>
                {
                    continue;
                }
                // Space toggles fold/unfold of the focused thinking block
                // when the composer is empty. For thinking cells, toggles
                // between summary and full content; for other cells, toggles
                // visibility (#1972, #2348).
                KeyCode::Char(' ')
                    if key.modifiers == KeyModifiers::NONE && app.input.is_empty() =>
                {
                    if let Some(idx) = detail_target_cell_index(app) {
                        if app.toggle_tool_run_expansion_at(idx) {
                            continue;
                        }
                        let is_thinking = app
                            .history
                            .get(idx)
                            .is_some_and(|c| matches!(c, HistoryCell::Thinking { .. }));
                        if is_thinking {
                            if app.folded_thinking.contains(&idx) {
                                app.folded_thinking.remove(&idx);
                                app.status_message = Some("Thinking block expanded".to_string());
                            } else {
                                app.folded_thinking.insert(idx);
                                app.status_message = Some("Thinking block folded".to_string());
                            }
                        } else if app.collapsed_cells.contains(&idx) {
                            app.collapsed_cells.remove(&idx);
                            app.status_message = Some("Cell expanded".to_string());
                        } else {
                            app.collapsed_cells.insert(idx);
                            app.status_message = Some("Cell collapsed".to_string());
                        }
                        app.mark_history_updated();
                        app.needs_redraw = true;
                    }
                    continue;
                }
                KeyCode::Char('t') | KeyCode::Char('T')
                    if key.modifiers == KeyModifiers::CONTROL =>
                {
                    toggle_live_transcript_overlay(app);
                    continue;
                }
                KeyCode::Char('1')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && key_shortcuts::has_control_like_modifier(key.modifiers) =>
                {
                    app.set_sidebar_focus(SidebarFocus::Pinned);
                    app.status_message = Some("Sidebar focus: pinned".to_string());
                    continue;
                }
                KeyCode::Char('2')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && key_shortcuts::has_control_like_modifier(key.modifiers) =>
                {
                    app.set_sidebar_focus(SidebarFocus::Tasks);
                    app.status_message = Some("Sidebar focus: tasks".to_string());
                    continue;
                }
                KeyCode::Char('3')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && key_shortcuts::has_control_like_modifier(key.modifiers) =>
                {
                    app.set_sidebar_focus(SidebarFocus::Agents);
                    app.status_message = Some("Sidebar focus: agents".to_string());
                    continue;
                }
                KeyCode::Char('4')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && key_shortcuts::has_control_like_modifier(key.modifiers) =>
                {
                    apply_alt_4_shortcut(app, key.modifiers);
                    continue;
                }
                // Sidebar focus via Alt+! / Alt+@ / Alt+# / Alt+$ / Alt+%)
                // AltGr on European keyboards emits Ctrl+Alt on Windows, so
                // exclude Ctrl to avoid swallowing AltGr-typed characters
                // like @ (AltGr+0 on French AZERTY) and # (AltGr+3). This
                // matches the has_ctrl_or_alt / is_altgr philosophy in
                // key_hint.rs: treat Ctrl+Alt as AltGr, not a shortcut.
                KeyCode::Char('!')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.set_sidebar_focus(SidebarFocus::Pinned);
                    app.status_message = Some("Sidebar focus: pinned".to_string());
                    continue;
                }
                KeyCode::Char('@')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.set_sidebar_focus(SidebarFocus::Tasks);
                    app.status_message = Some("Sidebar focus: tasks".to_string());
                    continue;
                }
                KeyCode::Char('#')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.set_sidebar_focus(SidebarFocus::Agents);
                    app.status_message = Some("Sidebar focus: agents".to_string());
                    continue;
                }
                KeyCode::Char('$') | KeyCode::Char('%')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.set_sidebar_focus(SidebarFocus::Context);
                    app.status_message = Some("Sidebar focus: context".to_string());
                    continue;
                }
                KeyCode::Char(')')
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.set_sidebar_focus(SidebarFocus::Auto);
                    app.status_message = Some("Sidebar focus: auto".to_string());
                    continue;
                }
                KeyCode::Char('0') if key.modifiers.contains(KeyModifiers::ALT) => {
                    apply_alt_0_shortcut(app, key.modifiers);
                    continue;
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Scope the picker to the current workspace so Ctrl+R
                    // never restores a different project's history by
                    // surprise (#1395). Press `a` inside the picker to
                    // broaden to every saved session.
                    app.view_stack.push(SessionPickerView::new(&app.workspace));
                    continue;
                }
                KeyCode::Char('c') | KeyCode::Char('C')
                    if key_shortcuts::is_copy_shortcut(&key) =>
                {
                    let sel = app.selected_text();
                    if !sel.is_empty() {
                        if app.clipboard.write_text(&sel).is_ok() {
                            app.push_status_toast(
                                "Copied to clipboard",
                                StatusToastLevel::Info,
                                None,
                            );
                            app.clear_selection();
                        } else {
                            app.push_status_toast("Copy failed", StatusToastLevel::Error, None);
                        }
                    } else {
                        copy_active_selection(app);
                    }
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Four behaviors layered on Ctrl+C in priority order — see
                    // `CtrlCDisposition` for the unit-tested decision table.
                    // 1. selection active → copy + clear (Windows convention,
                    //    #1337); 2. turn in flight → cancel; 3. quit-armed →
                    //    exit; 4. otherwise → arm the 2-second exit prompt.
                    match ctrl_c_disposition(app) {
                        CtrlCDisposition::CopySelection => {
                            copy_active_selection(app);
                            app.viewport.transcript_selection.clear();
                        }
                        CtrlCDisposition::CancelTurn => {
                            engine_handle.cancel();
                            mark_active_turn_cancelled_locally(app);
                            current_streaming_text.clear();
                            let prompt_restored = app.restore_last_submitted_prompt_if_empty();
                            app.status_message = Some(
                                if prompt_restored {
                                    "Request cancelled; prompt restored to composer"
                                } else {
                                    "Request cancelled"
                                }
                                .to_string(),
                            );
                            app.disarm_quit();
                        }
                        CtrlCDisposition::ConfirmExit => {
                            let _ = engine_handle.send(Op::Shutdown).await;
                            return Ok(());
                        }
                        CtrlCDisposition::ArmExit => {
                            app.arm_quit();
                        }
                    }
                }
                KeyCode::Char('d')
                    if key.modifiers.contains(KeyModifiers::CONTROL) && app.input.is_empty() =>
                {
                    let _ = engine_handle.send(Op::Shutdown).await;
                    return Ok(());
                }
                // Vim composer mode: Esc from Insert/Visual → Normal.
                // This arm runs before the generic Esc handler so Insert mode
                // Esc doesn't accidentally cancel an in-flight request.
                KeyCode::Esc
                    if app.composer.vim_enabled
                        && app.composer.vim_mode != crate::tui::app::VimMode::Normal =>
                {
                    app.vim_enter_normal();
                    continue;
                }
                KeyCode::Esc if app.clear_composer_attachment_selection() => {
                    continue;
                }
                KeyCode::Esc if mention_menu_open => {
                    app.mention_menu_hidden = true;
                    app.mention_menu_selected = 0;
                }
                KeyCode::Esc if app.sidebar_hover_tooltip.is_some() => {
                    app.sidebar_hover_tooltip = None;
                    app.needs_redraw = true;
                }
                KeyCode::Esc => {
                    match next_escape_action(app, slash_menu_open) {
                        EscapeAction::CloseSlashMenu => {
                            // A popup-style action wins over backtrack — clear
                            // any prime so a stale Primed state can't jump us
                            // straight into Selecting on the next Esc.
                            app.backtrack.reset();
                            app.close_slash_menu();
                        }
                        EscapeAction::CancelRequest => {
                            app.backtrack.reset();
                            if app.paused || app.paused_quarry.is_some() {
                                clear_paused_command_state(app, &engine_handle);
                                if app.is_loading
                                    || matches!(
                                        app.runtime_turn_status.as_deref(),
                                        Some("in_progress")
                                    )
                                {
                                    engine_handle.cancel();
                                    mark_active_turn_cancelled_locally(app);
                                    current_streaming_text.clear();
                                }
                                app.active_allowed_tools = None;
                                app.hunt.quarry = None;
                                app.hunt.tokens_used = 0;
                                app.hunt.time_used_seconds = 0;
                                app.hunt.continuation_count = 0;
                                app.status_message = Some("Paused command cancelled".to_string());
                            } else {
                                engine_handle.cancel();
                                mark_active_turn_cancelled_locally(app);
                                current_streaming_text.clear();
                                app.status_message = Some("Request cancelled".to_string());
                            }
                        }
                        EscapeAction::PauseCommand => {
                            app.backtrack.reset();
                            pause_pausable_command(app, &engine_handle);
                        }
                        EscapeAction::DiscardQueuedDraft => {
                            app.backtrack.reset();
                            if app.cancel_queued_draft_edit() {
                                app.status_message =
                                    Some("Queued edit canceled; follow-up restored".to_string());
                            }
                        }
                        EscapeAction::ClearInput => {
                            app.backtrack.reset();
                            app.edit_in_progress = false;
                            app.clear_input_recoverable();
                        }
                        EscapeAction::Noop => {
                            // Nothing else cares about this Esc — route it
                            // through the backtrack state machine. While
                            // streaming or with the live transcript already
                            // open, fall through silently (#133 acceptance:
                            // "during streaming Esc-Esc is a silent no-op").
                            if app.is_loading
                                || app.view_stack.top_kind() == Some(ModalKind::LiveTranscript)
                            {
                                continue;
                            }
                            let total = count_user_history_cells(app);
                            match app.backtrack.handle_esc(total) {
                                crate::tui::backtrack::EscEffect::None => {}
                                crate::tui::backtrack::EscEffect::Prime => {
                                    app.status_message =
                                        Some("Press Esc again to backtrack".to_string());
                                    app.needs_redraw = true;
                                }
                                crate::tui::backtrack::EscEffect::Cancel => {
                                    app.status_message = Some("Backtrack canceled".to_string());
                                    app.needs_redraw = true;
                                }
                                crate::tui::backtrack::EscEffect::OpenOverlay => {
                                    open_backtrack_overlay(app);
                                }
                            }
                        }
                    }
                }
                KeyCode::Up if key.modifiers.contains(KeyModifiers::SUPER) => {
                    app.scroll_up(app.viewport.last_transcript_visible.max(3));
                }
                KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.scroll_up(3);
                }
                KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    app.scroll_up(3);
                }
                KeyCode::Up
                    if key.modifiers.is_empty()
                        && mention_menu_open
                        && app.mention_menu_selected > 0 =>
                {
                    app.mention_menu_selected = app.mention_menu_selected.saturating_sub(1);
                }
                KeyCode::Up if key.modifiers.is_empty() && slash_menu_open => {
                    select_previous_slash_menu_entry(app, slash_menu_entries.len());
                }
                KeyCode::Char('p')
                    if key.modifiers.contains(KeyModifiers::CONTROL) && slash_menu_open =>
                {
                    select_previous_slash_menu_entry(app, slash_menu_entries.len());
                }
                KeyCode::Up
                    if key.modifiers.is_empty()
                        && app.selected_composer_attachment_index().is_some() =>
                {
                    let _ = app.select_previous_composer_attachment();
                }
                KeyCode::Up
                    if key.modifiers.is_empty()
                        && app.cursor_position == 0
                        && !mention_menu_open
                        && !slash_menu_open
                        && app.composer_attachment_count() > 0 =>
                {
                    let _ = app.select_previous_composer_attachment();
                    continue;
                }
                // #85: ↑ edits the most-recent queued message when the composer
                // is idle and the pending-input preview is showing queued work.
                KeyCode::Up
                    if key.modifiers.is_empty()
                        && app.input.is_empty()
                        && app.cursor_position == 0
                        && app.queued_draft.is_none()
                        && !app.queued_messages.is_empty()
                        && !mention_menu_open
                        && !slash_menu_open
                        && app.selected_composer_attachment_index().is_none() =>
                {
                    let _ = app.pop_last_queued_into_draft();
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::SUPER) => {
                    app.scroll_down(app.viewport.last_transcript_visible.max(3));
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.scroll_down(3);
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    app.scroll_down(3);
                }
                KeyCode::Down if key.modifiers.is_empty() && mention_menu_open => {
                    app.mention_menu_selected = (app.mention_menu_selected + 1)
                        .min(mention_menu_entries.len().saturating_sub(1));
                }
                KeyCode::Down if key.modifiers.is_empty() && slash_menu_open => {
                    select_next_slash_menu_entry(app, slash_menu_entries.len());
                }
                KeyCode::Char('n')
                    if key.modifiers.contains(KeyModifiers::CONTROL) && slash_menu_open =>
                {
                    select_next_slash_menu_entry(app, slash_menu_entries.len());
                }
                KeyCode::Down
                    if key.modifiers.is_empty()
                        && app.selected_composer_attachment_index().is_some() =>
                {
                    let _ = app.select_next_composer_attachment();
                }
                KeyCode::PageUp => {
                    let page = app.viewport.last_transcript_visible.max(1);
                    app.scroll_up(page);
                }
                KeyCode::PageDown => {
                    let page = app.viewport.last_transcript_visible.max(1);
                    app.scroll_down(page);
                }
                KeyCode::Tab => {
                    if mention_menu_open
                        && crate::tui::file_mention::apply_mention_menu_selection(
                            app,
                            &mention_menu_entries,
                        )
                    {
                        continue;
                    }
                    if slash_menu_open && apply_slash_menu_selection(app, &slash_menu_entries, true)
                    {
                        continue;
                    }
                    if try_autocomplete_slash_command(app) {
                        continue;
                    }
                    if crate::tui::file_mention::try_autocomplete_file_mention(app) {
                        continue;
                    }
                    if app.is_loading && queue_current_draft_for_next_turn(app) {
                        continue;
                    }
                    if app.input.is_empty()
                        && let Some(suggestion) = app.prompt_suggestion.take()
                    {
                        app.input = suggestion;
                        app.cursor_position = app.input.chars().count();
                        app.needs_redraw = true;
                        continue;
                    }
                    let prior_model = app.model.clone();
                    let prior_mode = app.mode;
                    app.cycle_mode();
                    if app.mode != prior_mode {
                        sync_mode_update(&engine_handle, app.mode).await;
                    }
                    if app.model != prior_model {
                        let _ = engine_handle
                            .send(Op::SetModel {
                                model: app.model.clone(),
                                mode: app.mode,
                                route_limits: app.active_route_limits,
                            })
                            .await;
                    }
                }
                KeyCode::BackTab => {
                    app.cycle_effort();
                }
                // Transcript-nav shortcuts now require Alt, leaving most bare
                // letters free to insert as text. Before v0.8.30, bare `g`,
                // `G`, `[`, `]`, `?`, and `l` on an empty composer were
                // hijacked for navigation — typing "good" yielded "ood" with
                // no whale and no warning. The Alt-prefixed shortcuts mirror
                // the Alt+R / Alt+C pattern already in use. Shift is
                // permitted for most capital-letter forms.
                KeyCode::Char('g')
                    if key_shortcuts::alt_nav_modifiers(key.modifiers)
                        && app.input.is_empty()
                        && !slash_menu_open =>
                {
                    if let Some(anchor) =
                        TranscriptScroll::anchor_for(app.viewport.transcript_cache.line_meta(), 0)
                    {
                        app.viewport.transcript_scroll = anchor;
                    }
                }
                KeyCode::Char('G')
                    if key_shortcuts::alt_nav_modifiers(key.modifiers)
                        && app.input.is_empty()
                        && !slash_menu_open =>
                {
                    app.scroll_to_bottom();
                }
                KeyCode::Char('[')
                    if key_shortcuts::alt_nav_modifiers(key.modifiers)
                        && app.input.is_empty()
                        && !slash_menu_open
                        && !jump_to_adjacent_tool_cell(app, SearchDirection::Backward) =>
                {
                    app.status_message = Some("No previous tool output".to_string());
                }
                KeyCode::Char(']')
                    if key_shortcuts::alt_nav_modifiers(key.modifiers)
                        && app.input.is_empty()
                        && !slash_menu_open
                        && !jump_to_adjacent_tool_cell(app, SearchDirection::Forward) =>
                {
                    app.status_message = Some("No next tool output".to_string());
                }
                // `Alt+?` opens the searchable help overlay (#93). F1 and
                // Ctrl+/ are also bound; bare `?` is reserved as text input
                // so users can start a message with "?" without losing the
                // first character.
                KeyCode::Char('?')
                    if key_shortcuts::alt_nav_modifiers(key.modifiers)
                        && app.input.is_empty()
                        && !slash_menu_open =>
                {
                    if app.view_stack.top_kind() != Some(ModalKind::Help) {
                        app.view_stack.push(HelpView::new_for_locale(app.ui_locale));
                    }
                    continue;
                }
                // Shift+Enter steers a running turn. When idle, the
                // normal composer-newline branch below still handles it
                // as a multiline input gesture.
                KeyCode::Enter
                    if app.is_loading
                        && key.modifiers.contains(KeyModifiers::SHIFT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    if let Some(input) = app.submit_input() {
                        if handle_bang_shell_input(app, &engine_handle, &input).await? {
                            continue;
                        }
                        if looks_like_slash_command_input(&input) {
                            if execute_command_input(
                                terminal,
                                app,
                                &mut engine_handle,
                                &task_manager,
                                config,
                                &mut web_config_session,
                                &input,
                            )
                            .await?
                            {
                                return Ok(());
                            }
                        } else {
                            let queued = if let Some(mut draft) = app.queued_draft.take() {
                                draft.display = input;
                                draft
                            } else {
                                build_queued_message(app, input)
                            };
                            if let Err(err) =
                                steer_user_message(app, &engine_handle, queued.clone()).await
                            {
                                app.queue_message(queued);
                                app.status_message = Some(format!(
                                    "Steer failed ({err}); {} queued follow-up(s) — /queue send <n>",
                                    app.queued_message_count()
                                ));
                            }
                        }
                    }
                }
                // Input handling
                _ if is_composer_newline_key(key)
                    && !(app.is_loading && is_forced_submit_key(key)) =>
                {
                    app.insert_char('\n');
                }
                KeyCode::Enter
                    if mention_menu_open
                        && crate::tui::file_mention::apply_mention_menu_selection(
                            app,
                            &mention_menu_entries,
                        ) =>
                {
                    continue;
                }
                // #382: Ctrl+Enter forces a steer into the current turn.
                // Some terminals report Ctrl/Cmd+Enter as Ctrl+J; while a
                // turn is running, accept that encoding here instead of
                // inserting a newline.
                _ if is_forced_submit_key(key)
                    && (matches!(key.code, KeyCode::Enter) || app.is_loading) =>
                {
                    if let Some(input) = app.submit_input() {
                        if handle_bang_shell_input(app, &engine_handle, &input).await? {
                            continue;
                        }
                        if looks_like_slash_command_input(&input) {
                            if execute_command_input(
                                terminal,
                                app,
                                &mut engine_handle,
                                &task_manager,
                                config,
                                &mut web_config_session,
                                &input,
                            )
                            .await?
                            {
                                return Ok(());
                            }
                        } else {
                            let queued = if let Some(mut draft) = app.queued_draft.take() {
                                draft.display = input;
                                draft
                            } else {
                                build_queued_message(app, input)
                            };
                            if app.is_loading {
                                // Engine is busy — steer into the current turn.
                                if let Err(err) =
                                    steer_user_message(app, &engine_handle, queued.clone()).await
                                {
                                    app.queue_message(queued);
                                    app.status_message = Some(format!(
                                        "Steer failed ({err}); {} queued follow-up(s) — /queue send <n>",
                                        app.queued_message_count()
                                    ));
                                }
                            } else {
                                // Engine is idle — send as a regular message
                                // so the content is not lost to rx_steer's
                                // stale-drain in handle_send_message (#1331).
                                submit_or_steer_message(app, config, &engine_handle, queued)
                                    .await?;
                            }
                        }
                    }
                }
                KeyCode::Enter => {
                    // #573: when the user typed a slash-command prefix that
                    // the popup is matching (e.g. `/mo` → `/model`), Enter
                    // should run the *highlighted match* rather than
                    // sending the literal `/mo` text. Only kick in when the
                    // popup has at least one entry; otherwise fall through
                    // to the legacy submit path.
                    let selecting_inline_skill = slash_menu_open
                        && partial_inline_skill_mention_at_cursor(&app.input, app.cursor_position)
                            .is_some();
                    if slash_menu_open
                        && !slash_menu_entries.is_empty()
                        && apply_slash_menu_selection(app, &slash_menu_entries, false)
                    {
                        app.close_slash_menu();
                        if selecting_inline_skill {
                            continue;
                        }
                    }
                    if let Some(input) = app.handle_composer_enter() {
                        if handle_plan_choice(app, config, &engine_handle, &input).await? {
                            continue;
                        }
                        // `# foo` quick-add (#492) — when memory is enabled,
                        // a single line starting with `#` (but not `##` /
                        // `#!` shebangs / Markdown headings the user might
                        // be pasting in) is intercepted: the text is
                        // appended to the user memory file and the input
                        // is consumed without firing a turn. Disabled
                        // behaviour falls through to normal turn submit.
                        if config.memory_enabled() && is_memory_quick_add(&input) {
                            handle_memory_quick_add(app, &input, config);
                            continue;
                        }
                        if handle_bang_shell_input(app, &engine_handle, &input).await? {
                            continue;
                        }
                        if looks_like_slash_command_input(&input) {
                            if execute_command_input(
                                terminal,
                                app,
                                &mut engine_handle,
                                &task_manager,
                                config,
                                &mut web_config_session,
                                &input,
                            )
                            .await?
                            {
                                return Ok(());
                            }
                        } else {
                            let queued = if let Some(mut draft) = app.queued_draft.take() {
                                draft.display = input;
                                draft
                            } else {
                                build_queued_message(app, input)
                            };
                            // #383: /edit — if the user invoked /edit to revise
                            // the last message, undo the last exchange before
                            // dispatching the replacement. Sync the engine
                            // session so it also drops the old exchange.
                            if app.edit_in_progress {
                                crate::commands::execute("/undo", app);
                                app.edit_in_progress = false;
                                let _ = engine_handle
                                    .send(Op::SyncSession {
                                        session_id: app.current_session_id.clone(),
                                        messages: app.api_messages.clone(),
                                        system_prompt: app.system_prompt.clone(),
                                        system_prompt_override: false,
                                        model: app.model.clone(),
                                        workspace: app.workspace.clone(),
                                    })
                                    .await;
                            }
                            submit_or_steer_message(app, config, &engine_handle, queued).await?;
                        }
                    }
                }
                KeyCode::Backspace
                    if key.modifiers.contains(KeyModifiers::SUPER)
                        && !app.remove_selected_composer_attachment() =>
                {
                    app.delete_to_start_of_line();
                }
                KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SUPER) => {}
                KeyCode::Backspace
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && !app.remove_selected_composer_attachment() =>
                {
                    app.delete_word_backward();
                }
                KeyCode::Backspace if key.modifiers.contains(KeyModifiers::ALT) => {}
                KeyCode::Backspace
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && !app.remove_selected_composer_attachment() =>
                {
                    app.delete_word_backward();
                }
                KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {}
                KeyCode::Delete
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && !app.remove_selected_composer_attachment() =>
                {
                    app.delete_word_forward();
                }
                KeyCode::Delete if key.modifiers.contains(KeyModifiers::ALT) => {}
                KeyCode::Delete
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && !app.remove_selected_composer_attachment() =>
                {
                    app.delete_word_forward();
                }
                KeyCode::Delete if key.modifiers.contains(KeyModifiers::CONTROL) => {}
                KeyCode::Backspace if !app.remove_selected_composer_attachment() => {
                    app.delete_char();
                }
                KeyCode::Backspace => {}
                KeyCode::Char('h')
                    if key_shortcuts::is_ctrl_h_backspace(&key)
                        && !app.remove_selected_composer_attachment() =>
                {
                    app.delete_char();
                }
                KeyCode::Char('h') if key_shortcuts::is_ctrl_h_backspace(&key) => {}
                KeyCode::Delete if !app.remove_selected_composer_attachment() => {
                    app.delete_char_forward();
                }
                KeyCode::Delete => {}
                KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    if app.selection_anchor.is_none() {
                        app.selection_anchor = Some(app.cursor_position);
                    }
                    app.move_cursor_left();
                }
                KeyCode::Left if is_word_cursor_modifier(key.modifiers) => {
                    app.clear_selection();
                    app.move_cursor_word_backward();
                }
                KeyCode::Left => {
                    app.clear_selection();
                    app.move_cursor_left();
                }
                KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    if app.selection_anchor.is_none() {
                        app.selection_anchor = Some(app.cursor_position);
                    }
                    app.move_cursor_right();
                }
                KeyCode::Right if is_word_cursor_modifier(key.modifiers) => {
                    app.clear_selection();
                    app.move_cursor_word_forward();
                }
                KeyCode::Right => {
                    app.clear_selection();
                    app.move_cursor_right();
                }
                KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(anchor) =
                        TranscriptScroll::anchor_for(app.viewport.transcript_cache.line_meta(), 0)
                    {
                        app.viewport.transcript_scroll = anchor;
                    }
                }
                KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.scroll_to_bottom();
                }
                KeyCode::Home | KeyCode::Char('a')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.clear_selection();
                    app.move_cursor_start();
                }
                KeyCode::Home => {
                    app.clear_selection();
                    app.move_cursor_line_start();
                }
                KeyCode::End => {
                    app.clear_selection();
                    app.move_cursor_line_end();
                }
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.clear_selection();
                    app.move_cursor_end();
                }
                _ if handle_composer_alt_word_motion_key(app, key) => {}
                KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl+O: spawn $EDITOR on the composer contents (#91).
                    // Only fires when no modal is active (the !view_stack
                    // branch above already returns early in that case) and
                    // the composer is the focused input target. We accept the
                    // shortcut whether or not a model turn is streaming —
                    // editing the buffer never disturbs in-flight work.
                    let seed = app.input.clone();
                    match super::external_editor::spawn_editor_for_input(
                        terminal,
                        app.use_alt_screen,
                        app.use_mouse_capture,
                        app.use_bracketed_paste,
                        &seed,
                    ) {
                        Ok(super::external_editor::EditorOutcome::Edited(new)) => {
                            app.input = new;
                            app.move_cursor_end();
                            let editor = std::env::var("VISUAL")
                                .ok()
                                .filter(|s| !s.trim().is_empty())
                                .or_else(|| {
                                    std::env::var("EDITOR")
                                        .ok()
                                        .filter(|s| !s.trim().is_empty())
                                })
                                .unwrap_or_else(|| "vi".to_string());
                            app.status_message = Some(format!("Edited in {editor}"));
                        }
                        Ok(super::external_editor::EditorOutcome::Unchanged) => {
                            app.status_message = Some("Editor closed (no changes)".to_string());
                        }
                        Ok(super::external_editor::EditorOutcome::Cancelled) => {
                            app.status_message = Some("Editor cancelled".to_string());
                        }
                        Err(err) => {
                            app.status_message = Some(format!("Editor error: {err}"));
                        }
                    }
                    app.needs_redraw = true;
                }
                KeyCode::Up => {
                    let _ =
                        handle_composer_history_arrow(app, key, slash_menu_open, mention_menu_open);
                }
                KeyCode::Down => {
                    let _ =
                        handle_composer_history_arrow(app, key, slash_menu_open, mention_menu_open);
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.clear_input_recoverable();
                }
                KeyCode::Char('z')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && app.restore_last_cleared_input_if_empty() =>
                {
                    app.status_message = Some("Restored cleared draft".to_string());
                }
                KeyCode::Char('w') | KeyCode::Char('W')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.delete_word_backward();
                }
                KeyCode::Char('s') | KeyCode::Char('S')
                    if key.modifiers == KeyModifiers::CONTROL =>
                {
                    if send_ctrl_s_queued_message_now(app, config, &engine_handle).await? {
                        continue;
                    }
                    // #440: park the current draft to the persistent
                    // stash and clear the composer. Empty composers
                    // are a no-op so a stray Ctrl+S can't pollute the
                    // file. Surface a toast so the user sees the
                    // confirmation (no-op feels broken otherwise).
                    if !app.input.is_empty() {
                        crate::composer_stash::push_stash(&app.input);
                        app.clear_input_recoverable();
                        app.push_status_toast(
                            "Draft stashed — `/stash pop` to restore",
                            StatusToastLevel::Info,
                            Some(3_000),
                        );
                    }
                }
                KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // #379: context-sensitive Ctrl+Y.
                    // When the composer has content → emacs-style yank
                    // from the kill buffer at the cursor.
                    // When the composer is empty (transcript focus) →
                    // copy the focused cell text to the system clipboard.
                    if app.input.is_empty() && app.view_stack.is_empty() {
                        if copy_focused_cell(app) {
                            app.push_status_toast(
                                "Copied to clipboard",
                                StatusToastLevel::Info,
                                Some(2_000),
                            );
                        } else {
                            app.status_message = Some("No transcript cell to copy".to_string());
                        }
                    } else {
                        app.yank();
                    }
                }
                KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let sel = app.selected_text();
                    if !sel.is_empty() {
                        if app.clipboard.write_text(&sel).is_ok() {
                            app.push_status_toast("Cut to clipboard", StatusToastLevel::Info, None);
                            app.delete_selection();
                        } else {
                            app.push_status_toast("Cut failed", StatusToastLevel::Error, None);
                        }
                    }
                }
                _ if key_shortcuts::is_paste_shortcut(&key) => {
                    app.paste_from_clipboard();
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::ALT) => {
                    apply_mode_update(app, &engine_handle, AppMode::Agent).await;
                    continue;
                }
                KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::ALT) => {
                    apply_mode_update(app, &engine_handle, AppMode::Yolo).await;
                    continue;
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::ALT) => {
                    apply_mode_update(app, &engine_handle, AppMode::Plan).await;
                    continue;
                }
                KeyCode::Char('A') if key.modifiers.contains(KeyModifiers::ALT) => {
                    apply_mode_update(app, &engine_handle, AppMode::Agent).await;
                    continue;
                }
                KeyCode::Char('Y') if key.modifiers.contains(KeyModifiers::ALT) => {
                    apply_mode_update(app, &engine_handle, AppMode::Yolo).await;
                    continue;
                }
                KeyCode::Char('P') if key.modifiers.contains(KeyModifiers::ALT) => {
                    apply_mode_update(app, &engine_handle, AppMode::Plan).await;
                    continue;
                }
                KeyCode::Char('v') | KeyCode::Char('V')
                    if key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    open_tool_details_pager(app);
                    continue;
                }
                // Vim composer: Normal-mode motion / operator keys.
                // Only fires when vim is enabled, the input is focused (no modal
                // open on top), and the key has no modifier (pure char).
                KeyCode::Char(c)
                    if app.vim_is_normal_mode()
                        && key.modifiers.is_empty()
                        && !slash_menu_open
                        && !mention_menu_open
                        && app.view_stack.is_empty() =>
                {
                    vim_mode::handle_vim_normal_key(app, c);
                    continue;
                }
                // Vim composer: in Visual mode plain chars are ignored
                // (no text insertion until `i` / `a` enters Insert).
                KeyCode::Char(_)
                    if app.vim_is_visual_mode()
                        && key.modifiers.is_empty()
                        && app.view_stack.is_empty() =>
                {
                    // absorb — Visual mode not yet fully implemented
                }
                KeyCode::Char(c) if is_plain_char => {
                    app.insert_char(c);
                }
                KeyCode::Char(_) => {}
                _ => {}
            }

            if !is_plain_char && !is_enter {
                app.paste_burst.deactivate_keep_window();
            }
        }
    }
}

// dispatch_user_message moved to message_dispatch.rs

fn goal_status_from_snapshot(snapshot: &GoalSnapshot) -> Option<GoalStatus> {
    match snapshot.status.trim() {
        "active" => Some(GoalStatus::Active),
        "paused" => Some(GoalStatus::Paused),
        "complete" => Some(GoalStatus::Complete),
        "blocked" => Some(GoalStatus::Blocked),
        _ => None,
    }
}

pub(crate) fn apply_goal_snapshot_to_app(app: &mut App, snapshot: &GoalSnapshot) -> bool {
    let Some(objective) = snapshot
        .objective
        .as_deref()
        .map(str::trim)
        .filter(|objective| !objective.is_empty())
    else {
        return false;
    };
    let Some(status) = goal_status_from_snapshot(snapshot) else {
        tracing::warn!("ignoring unknown runtime goal status: {}", snapshot.status);
        return false;
    };
    let verdict = HuntVerdict::from_goal_status(status);
    let objective_changed = app.hunt.quarry.as_deref() != Some(objective);
    let changed = objective_changed
        || app.hunt.token_budget != snapshot.token_budget
        || app.hunt.tokens_used != snapshot.tokens_used
        || app.hunt.time_used_seconds != snapshot.time_used_seconds
        || app.hunt.continuation_count != snapshot.continuation_count
        || app.hunt.verdict != verdict;
    if !changed {
        return false;
    }

    app.hunt.quarry = Some(objective.to_string());
    app.hunt.token_budget = snapshot.token_budget;
    app.hunt.tokens_used = snapshot.tokens_used;
    app.hunt.time_used_seconds = snapshot.time_used_seconds;
    app.hunt.continuation_count = snapshot.continuation_count;
    app.hunt.verdict = verdict;
    if objective_changed || app.hunt.started_at.is_none() {
        app.hunt.started_at = Some(Instant::now());
    }
    true
}

// sync_mode_update, apply_mode_update moved to provider_and_model.rs

async fn handle_bang_shell_input(
    app: &mut App,
    engine_handle: &EngineHandle,
    input: &str,
) -> Result<bool> {
    let command = match shell_command_from_bang_input(input) {
        Ok(Some(command)) => command,
        Ok(None) => return Ok(false),
        Err(message) => {
            app.status_message = Some(format!("Error: {message}"));
            return Ok(true);
        }
    };

    engine_handle
        .send(Op::RunShellCommand {
            command: command.to_string(),
            mode: app.mode,
            trust_mode: app.trust_mode,
            auto_approve: app.mode == AppMode::Yolo,
            approval_mode: app.approval_mode,
        })
        .await?;
    app.status_message = Some(format!("Shell command submitted: {command}"));
    Ok(true)
}

fn is_model_visible_tool_call(id: &str) -> bool {
    !id.starts_with(USER_SHELL_TOOL_ID_PREFIX)
}

// apply_model_and_compaction_update moved to provider_and_model.rs

async fn drain_web_config_events(
    web_config_session: &mut Option<WebConfigSession>,
    app: &mut App,
    config: &mut Config,
    engine_handle: &EngineHandle,
) -> bool {
    let Some(session) = web_config_session.as_mut() else {
        return true;
    };

    let mut keep_session = true;
    while let Ok(event) = session.receiver.try_recv() {
        match event {
            WebConfigSessionEvent::Draft(doc) => {
                match config_ui::apply_document(doc, app, config, false) {
                    Ok(outcome) if outcome.changed => {
                        if outcome.requires_engine_sync {
                            apply_model_and_compaction_update(
                                engine_handle,
                                app.compaction_config(),
                                app.mode,
                                app.active_route_limits,
                            )
                            .await;
                        }
                        app.status_message = Some(format!(
                            "Web config draft applied: {}",
                            outcome.final_message
                        ));
                    }
                    Ok(_) => {}
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Web config draft apply failed: {err}"),
                        });
                    }
                }
            }
            WebConfigSessionEvent::Committed(doc) => {
                keep_session = false;
                match config_ui::apply_document(doc, app, config, true) {
                    Ok(outcome) => {
                        if outcome.requires_engine_sync {
                            apply_model_and_compaction_update(
                                engine_handle,
                                app.compaction_config(),
                                app.mode,
                                app.active_route_limits,
                            )
                            .await;
                        }
                        app.add_message(HistoryCell::System {
                            content: outcome.final_message.clone(),
                        });
                        app.status_message = Some(outcome.final_message);
                    }
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Web config commit failed: {err}"),
                        });
                    }
                }
            }
            WebConfigSessionEvent::Failed(err) => {
                keep_session = false;
                app.add_message(HistoryCell::System {
                    content: format!("Web config session failed: {err}"),
                });
            }
        }
    }

    keep_session
}

// apply_model_picker_choice, apply_picker_effort_choice, switch_provider,
// apply_provider_fallback_switch, display_base_url_host,
// sync_config_provider_from_app, provider_picker_model_override
// moved to provider_and_model.rs

fn open_text_pager(app: &mut App, title: String, content: String) {
    let width = app
        .viewport
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    app.view_stack.push(PagerView::from_text(
        title,
        &content,
        width.saturating_sub(2),
    ));
}

pub(crate) fn open_context_inspector(app: &mut App) {
    let width = app
        .viewport
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    let content = build_context_inspector_text(app, app.ui_locale);
    app.view_stack.push(PagerView::from_text(
        tr(app.ui_locale, MessageId::CtxInspTitle),
        &content,
        width.saturating_sub(2),
    ));
}

// File-picker relevance scoring moved to `tui/file_picker_relevance.rs`.

async fn apply_command_result(
    terminal: &mut AppTerminal,
    app: &mut App,
    engine_handle: &mut EngineHandle,
    task_manager: &SharedTaskManager,
    config: &mut Config,
    #[cfg_attr(not(feature = "web"), allow(unused_variables))] web_config_session: &mut Option<
        WebConfigSession,
    >,
    result: commands::CommandResult,
) -> Result<bool> {
    if let Some(msg) = result.message {
        app.add_message(HistoryCell::System { content: msg });
    }

    if let Some(action) = result.action {
        match action {
            AppAction::Quit => {
                let _ = engine_handle.send(Op::Shutdown).await;
                return Ok(true);
            }
            AppAction::SaveSession(path) => {
                app.status_message = Some(format!("Session saved to {}", path.display()));
            }
            AppAction::LoadSession(path) => {
                app.status_message = Some(format!("Session loaded from {}", path.display()));
            }
            AppAction::SyncSession {
                session_id,
                messages,
                system_prompt,
                model,
                workspace,
            } => {
                let mut session_id = session_id;
                let is_full_reset = messages.is_empty() && system_prompt.is_none();
                if is_full_reset && session_id.is_none() {
                    let new_session_id = uuid::Uuid::new_v4().to_string();
                    app.current_session_id = Some(new_session_id.clone());
                    session_id = Some(new_session_id);
                }
                let _ = engine_handle
                    .send(Op::SyncSession {
                        session_id,
                        messages,
                        system_prompt,
                        system_prompt_override: false,
                        model,
                        workspace,
                    })
                    .await;
                let _ = engine_handle
                    .send(Op::SetCompaction {
                        config: app.compaction_config(),
                    })
                    .await;
                if is_full_reset {
                    if let Ok(manager) = SessionManager::default_location() {
                        let session = build_session_snapshot(app, &manager);
                        app.current_session_id = Some(session.metadata.id.clone());
                        persistence_actor::persist(PersistRequest::SessionSnapshot(session));
                    }
                    persistence_actor::persist(PersistRequest::ClearCheckpoint);
                }
            }
            AppAction::ModeChanged(mode) => {
                sync_mode_update(engine_handle, mode).await;
            }
            AppAction::SpecFrozen => {
                // Spec freeze state is already set in the command handler.
                // The frozen spec will be injected into the system prompt
                // when the next message is sent.
                app.status_message =
                    Some("Spec frozen. Agent will respect the frozen spec.".to_string());
            }
            AppAction::SpecUnfrozen => {
                // Spec unfreeze state is already set in the command handler.
                app.status_message =
                    Some("Spec unfrozen. Agent is no longer constrained.".to_string());
            }
            AppAction::SendMessage(content) => {
                let queued = build_queued_message(app, content);
                submit_or_steer_message(app, config, engine_handle, queued).await?;
            }
            AppAction::SetGoalStatus { status, clear } => {
                let _ = engine_handle
                    .send(Op::SetGoalStatus { status, clear })
                    .await;
            }
            AppAction::VoiceCapture => {
                use commands::voice::VoiceCaptureOutcome;
                match commands::voice::capture_and_transcribe(app, config).await {
                    Ok(VoiceCaptureOutcome::Insert(text)) => {
                        app.insert_str(&text);
                        app.status_message = Some(format!(
                            "{}: {text}",
                            tr(app.ui_locale, MessageId::VoiceTranscribed)
                        ));
                    }
                    Ok(VoiceCaptureOutcome::Send(content)) => {
                        app.status_message =
                            Some(tr(app.ui_locale, MessageId::VoiceTranscribed).to_string());
                        let queued = build_queued_message(app, content);
                        submit_or_steer_message(app, config, engine_handle, queued).await?;
                    }
                    Err(err) => {
                        app.voice_enabled = false;
                        app.status_message = Some(err);
                    }
                }
            }
            AppAction::ListSubAgents => {
                let _ = engine_handle.send(Op::ListSubAgents).await;
            }
            AppAction::FetchModels => {
                app.status_message = Some("Fetching models...".to_string());
                match fetch_available_models(config).await {
                    Ok(models) => {
                        app.add_message(HistoryCell::System {
                            content: format_helpers::available_models_message(&app.model, &models),
                        });
                        app.status_message = Some(format!("Found {} model(s)", models.len()));
                    }
                    Err(error) => {
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Failed to fetch models from {}: {error}",
                                config.api_provider().display_name()
                            ),
                        });
                    }
                }
            }
            AppAction::CacheWarmup => {
                app.status_message = Some("Warming DeepSeek cache...".to_string());
                match run_cache_warmup(app, config).await {
                    Ok((usage, base_url, inspection)) => {
                        app.session.last_base_url = Some(base_url.clone());
                        app.session.last_warmup_key = Some(CacheWarmupKey::from_inspection(
                            &format!("{:?}", app.api_provider),
                            &app.model,
                            &base_url,
                            &inspection,
                        ));
                        let mut message = format_helpers::cache_warmup_result(&usage);
                        if let Some(key) = app.session.last_warmup_key.as_ref() {
                            message.push_str(&format!("\nWarmup key: {}", key.hash_short()));
                        }
                        // Append prefix-cache stability info.
                        if app.prefix_checks_total > 0 {
                            let changes = app.prefix_change_count;
                            let total = app.prefix_checks_total;
                            let stable = total.saturating_sub(changes);
                            let pct = app
                                .prefix_stability_pct
                                .map(|p| format!("{p}%"))
                                .unwrap_or_else(|| "--".to_string());
                            message.push_str(&format!(
                                "\n\nPrefix stability: {pct} ({stable}/{total} checks stable, {changes} change{})",
                                if changes == 1 { "" } else { "s" }
                            ));
                            if let Some(ref desc) = app.last_prefix_change_desc {
                                message.push_str(&format!("\nLast prefix change: {desc}"));
                            }
                        }
                        app.add_message(HistoryCell::System { content: message });
                        app.status_message = Some("Cache warmup complete".to_string());
                    }
                    Err(error) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Cache warmup failed: {error}"),
                        });
                        app.status_message = Some("Cache warmup failed".to_string());
                    }
                }
            }
            AppAction::SwitchProvider { provider, model } => {
                switch_provider(app, engine_handle, config, provider, model).await;
                // Refresh balance after provider switch.
                let balance_cooldown_expired = app
                    .last_balance_fetch
                    .is_none_or(|t| t.elapsed() >= BALANCE_FETCH_COOLDOWN);
                if balance_cooldown_expired && should_fetch_deepseek_balance(app) {
                    let cell = app.balance_cell.clone();
                    let api_key = config.api_key().unwrap_or_default();
                    let base_url = config.api_base_url();
                    if !api_key.is_empty() {
                        app.last_balance_fetch = Some(Instant::now());
                        tokio::spawn(async move {
                            let provider = ReqwestBalanceProvider::new();
                            if let Some(info) =
                                fetch_deepseek_balance(&provider, &api_key, &base_url).await
                                && let Ok(mut guard) = cell.lock()
                            {
                                *guard = Some(info);
                            }
                        });
                    }
                } else {
                    // Clear balance when switching to a non-DeepSeek provider.
                    if let Ok(mut guard) = app.balance_cell.lock() {
                        *guard = None;
                    }
                }
            }
            AppAction::UpdateCompaction(compaction) => {
                apply_model_and_compaction_update(
                    engine_handle,
                    compaction,
                    app.mode,
                    app.active_route_limits,
                )
                .await;
            }
            AppAction::UpdateStreamChunkTimeout(timeout_secs) => {
                let _ = engine_handle
                    .send(Op::SetStreamChunkTimeout { timeout_secs })
                    .await;
            }
            AppAction::UpdateSubagentRuntimeConfig {
                enabled,
                max_subagents,
                launch_concurrency,
                max_spawn_depth,
                api_timeout_secs,
                heartbeat_timeout_secs,
            } => {
                let _ = engine_handle
                    .send(Op::SetSubagentRuntimeConfig {
                        enabled,
                        max_subagents,
                        launch_concurrency,
                        max_spawn_depth,
                        api_timeout_secs,
                        heartbeat_timeout_secs,
                    })
                    .await;
            }
            AppAction::OpenConfigEditor(mode) => match mode {
                ConfigUiMode::Native => {
                    if app.view_stack.top_kind() != Some(ModalKind::Config) {
                        app.view_stack.push(ConfigView::new_for_app(app));
                    }
                }
                ConfigUiMode::Tui => {
                    pause_terminal(
                        terminal,
                        app.use_alt_screen,
                        app.use_mouse_capture,
                        app.use_bracketed_paste,
                    )?;
                    let editor_result = config_ui::run_tui_editor(app, config)
                        .and_then(|doc| config_ui::apply_document(doc, app, config, true));
                    resume_terminal(
                        terminal,
                        app.use_alt_screen,
                        app.use_mouse_capture,
                        app.use_bracketed_paste,
                        app.synchronized_output_enabled,
                    )?;
                    match editor_result {
                        Ok(outcome) => {
                            if outcome.requires_engine_sync {
                                apply_model_and_compaction_update(
                                    engine_handle,
                                    app.compaction_config(),
                                    app.mode,
                                    app.active_route_limits,
                                )
                                .await;
                            }
                            app.add_message(HistoryCell::System {
                                content: outcome.final_message.clone(),
                            });
                            app.status_message = Some(outcome.final_message);
                        }
                        Err(err) => {
                            app.add_message(HistoryCell::System {
                                content: format!("Config UI failed: {err}"),
                            });
                        }
                    }
                }
                ConfigUiMode::Web => {
                    #[cfg(feature = "web")]
                    {
                        let session = config_ui::start_web_editor(app, config).await?;
                        let url = format!("http://{}", session.addr);
                        let open_err = config_ui::open_browser(&url).err();
                        if let Some(err) = open_err {
                            app.add_message(HistoryCell::System {
                                content: format!("Failed to open browser automatically: {err}"),
                            });
                        }
                        app.status_message = Some(format!("web ui listen on: {url}"));
                        *web_config_session = Some(session);
                    }
                    #[cfg(not(feature = "web"))]
                    {
                        app.add_message(HistoryCell::System {
                            content: "This build does not include the web config UI.".to_string(),
                        });
                    }
                }
            },
            AppAction::OpenConfigView => {
                if app.view_stack.top_kind() != Some(ModalKind::Config) {
                    app.view_stack.push(ConfigView::new_for_app(app));
                }
            }
            AppAction::OpenModelPicker => {
                if app.view_stack.top_kind() != Some(ModalKind::ModelPicker) {
                    app.view_stack
                        .push(crate::tui::model_picker::ModelPickerView::new(app));
                }
            }
            AppAction::OpenProviderPicker => {
                if app.view_stack.top_kind() != Some(ModalKind::ProviderPicker) {
                    app.view_stack
                        .push(crate::tui::provider_picker::ProviderPickerView::new(
                            app.api_provider,
                            config,
                        ));
                }
            }
            AppAction::OpenModePicker => {
                if app.view_stack.top_kind() != Some(ModalKind::ModePicker) {
                    app.view_stack
                        .push(crate::tui::views::mode_picker::ModePickerView::new(
                            app.mode,
                            app.ui_locale,
                        ));
                }
            }
            AppAction::OpenBacktrackOverlay => {
                open_backtrack_overlay(app);
            }
            AppAction::OpenStatusPicker => {
                if app.view_stack.top_kind() != Some(ModalKind::StatusPicker) {
                    app.view_stack
                        .push(crate::tui::views::status_picker::StatusPickerView::new(
                            &app.status_items,
                            app.api_provider,
                            app.ui_locale,
                        ));
                }
            }
            AppAction::OpenFleetSetup => {
                if app.view_stack.top_kind() != Some(ModalKind::FleetSetup) {
                    app.view_stack
                        .push(crate::tui::views::fleet_setup::FleetSetupView::new(
                            app, config,
                        ));
                }
            }
            AppAction::OpenExternalUrl { url, label } => match open_external_url(&url) {
                Ok(()) => {
                    app.status_message = Some(format!("Opened {label} in your browser"));
                }
                Err(err) => {
                    app.add_message(HistoryCell::System {
                        content: format!(
                            "Could not open {label} automatically: {err}\n\nThe URL is printed above."
                        ),
                    });
                }
            },
            AppAction::OpenContextInspector => {
                open_context_inspector(app);
            }
            AppAction::CompactContext => {
                app.status_message = Some("Compacting context...".to_string());
                let _ = engine_handle.send(Op::CompactContext).await;
            }
            AppAction::PurgeContext => {
                app.status_message = Some("Agent purging context...".to_string());
                let _ = engine_handle.send(Op::PurgeContext).await;
            }
            AppAction::TaskAdd { prompt } => {
                let request = NewTaskRequest {
                    prompt: prompt.clone(),
                    model: Some(app.model.clone()),
                    workspace: Some(app.workspace.clone()),
                    mode: Some(task_mode_label(app.mode).to_string()),
                    allow_shell: Some(app.allow_shell),
                    trust_mode: Some(app.trust_mode),
                    auto_approve: Some(app.approval_mode == ApprovalMode::Auto),
                };
                match task_manager.add_task(request).await {
                    Ok(task) => {
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Task queued: {} ({})",
                                task.id,
                                summarize_tool_output(&task.prompt)
                            ),
                        });
                        app.status_message = Some(format!("Queued {}", task.id));
                    }
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Failed to queue task: {err}"),
                        });
                    }
                }
                refresh_active_task_panel(app, task_manager).await;
            }
            AppAction::TaskList => {
                let tasks = task_manager.list_tasks(Some(30)).await;
                refresh_active_task_panel(app, task_manager).await;
                app.add_message(HistoryCell::System {
                    content: format_task_list(&tasks),
                });
            }
            AppAction::TaskShow { id } => match task_manager.get_task(&id).await {
                Ok(task) => open_task_pager(app, &task),
                Err(err) => {
                    app.add_message(HistoryCell::System {
                        content: format!("Task lookup failed: {err}"),
                    });
                }
            },
            AppAction::TaskCancel { id } => {
                match task_manager.cancel_task(&id).await {
                    Ok(task) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Task {} status: {:?}", task.id, task.status),
                        });
                    }
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Task cancel failed: {err}"),
                        });
                    }
                }
                refresh_active_task_panel(app, task_manager).await;
            }
            AppAction::ShellJob(action) => {
                handle_shell_job_action(app, action);
                // Immediately sync the task panel after cancel/poll so the
                // Tasks sidebar stays accurate without waiting for the
                // next 2.5 s periodic refresh (#2937).
                refresh_active_task_panel(app, task_manager).await;
            }
            AppAction::Mcp(action) => {
                handle_mcp_ui_action(app, config, action).await;
            }
            AppAction::SwitchWorkspace { workspace } => {
                switch_workspace(app, engine_handle, task_manager, config, workspace).await;
            }
            AppAction::SwitchProfile { profile } => {
                app.config_profile = Some(profile.clone());
                match Config::load(app.config_path.clone(), Some(&profile)) {
                    Ok(new_config) => {
                        *config = new_config.clone();
                        app.api_provider = config.api_provider();
                        let new_model = config.default_model();
                        app.set_model_selection(new_model.clone());
                        app.active_route_limits = None;
                        app.update_model_compaction_budget();
                        app.session.last_prompt_tokens = None;
                        app.session.last_completion_tokens = None;
                        app.session.last_output_throughput = None;
                        // Rebuild the engine with the new config so API key/model/base URL take effect.
                        let _ = engine_handle.send(Op::Shutdown).await;
                        let engine_config = build_engine_config(app, config);
                        *engine_handle = spawn_engine(engine_config, config);
                        if !app.api_messages.is_empty() {
                            let _ = engine_handle
                                .send(Op::SyncSession {
                                    session_id: app.current_session_id.clone(),
                                    messages: app.api_messages.clone(),
                                    system_prompt: app.system_prompt.clone(),
                                    system_prompt_override: false,
                                    model: app.model.clone(),
                                    workspace: app.workspace.clone(),
                                })
                                .await;
                        }
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Switched to profile '{profile}'. Model: {new_model}, Provider: {}",
                                config.api_provider().as_str()
                            ),
                        });
                        app.status_message = Some(format!("Profile: {profile}"));
                    }
                    Err(err) => {
                        app.config_profile = None;
                        app.status_message =
                            Some(format!("Failed to switch to profile '{profile}': {err}"));
                    }
                }
            }
            AppAction::ShareSession {
                history_len: _,
                model,
                mode,
            } => {
                let status = if app.api_messages.is_empty() {
                    "No session content to share.".to_string()
                } else {
                    let history_json = serde_json::to_string_pretty(&app.api_messages)
                        .unwrap_or_else(|_| "[]".to_string());
                    match crate::commands::share::perform_share(&history_json, &model, &mode).await
                    {
                        Ok(url) => format!("Session shared! URL: {url}"),
                        Err(err) => format!("Share failed: {err}"),
                    }
                };
                app.add_message(HistoryCell::System {
                    content: status.clone(),
                });
                app.status_message = Some(status);
            }
        }
    }

    Ok(false)
}

fn open_external_url(url: &str) -> Result<()> {
    crate::utils::open_url(url)
}

// apply_workspace_runtime_state, sync_runtime_workspace_state,
// switch_workspace moved to workspace.rs

// handle_mcp_ui_action, mcp_ui_action_refreshes_discovery,
// handle_shell_job_action moved to mcp_shell.rs

async fn execute_command_input(
    terminal: &mut AppTerminal,
    app: &mut App,
    engine_handle: &mut EngineHandle,
    task_manager: &SharedTaskManager,
    config: &mut Config,
    web_config_session: &mut Option<WebConfigSession>,
    input: &str,
) -> Result<bool> {
    if let Some(parsed_index) = parse_queue_send_command(input) {
        match parsed_index {
            Ok(index) => {
                send_queued_message_at_index_now(app, config, engine_handle, index).await?;
            }
            Err(message) => {
                app.status_message = Some(message);
            }
        }
        return Ok(false);
    }

    let result = commands::execute(input, app);
    // After /logout: clear the in-memory api_key fields so the next
    // onboarding round entering a new key doesn't see the stale value
    // (#343). The on-disk side is handled by clear_api_key() inside
    // commands::config::logout.
    if input.trim().eq_ignore_ascii_case("/logout") {
        // Only clear the active provider's in-memory API key, not every
        // provider.  The on-disk clear_api_key() inside commands::config::logout
        // already removes all saved keys; clearing only the active slot here
        // prevents surprising side-effects when the user has multiple providers
        // configured.
        config.api_key = None;
        config.provider_config_for_mut(app.api_provider).api_key = None;
        app.api_key_env_only = crate::config::active_provider_uses_env_only_api_key(config);
    }
    apply_command_result(
        terminal,
        app,
        engine_handle,
        task_manager,
        config,
        web_config_session,
        result,
    )
    .await
}

fn parse_queue_send_command(input: &str) -> Option<Result<usize, String>> {
    let rest = strip_queue_command_prefix(input.trim())?;
    let mut parts = rest.split_whitespace();
    let action = parts.next()?;
    if !matches!(action.to_ascii_lowercase().as_str(), "send" | "now") {
        return None;
    }
    let Some(raw_index) = parts.next() else {
        return Some(Err("Usage: /queue send <n>".to_string()));
    };
    if parts.next().is_some() {
        return Some(Err("Usage: /queue send <n>".to_string()));
    }
    let Ok(index) = raw_index.parse::<usize>() else {
        return Some(Err("Index must be a positive number".to_string()));
    };
    if index == 0 {
        return Some(Err("Index must be >= 1".to_string()));
    }
    Some(Ok(index - 1))
}

fn strip_queue_command_prefix(input: &str) -> Option<&str> {
    for prefix in ["/queue", "/queued"] {
        if let Some(rest) = input.strip_prefix(prefix)
            && (rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
        {
            return Some(rest);
        }
    }
    None
}

// steer_user_message, queue_follow_up, submit_or_steer_message,
// merge_pending_steers moved to message_dispatch.rs

// apply_plan_choice, handle_plan_choice, build_pending_input_preview
// moved to view_dispatch.rs

// refresh_live_transcript_overlay, open_backtrack_overlay, toggle_live_transcript_overlay,
// open_model_picker_for_provider, handle_view_events, update_backtrack_overlay_selection,
// count_user_history_cells, find_user_cell_index_from_tail, apply_backtrack
// have been moved to view_dispatch.rs

// refresh_live_transcript_overlay, open_backtrack_overlay,
// toggle_live_transcript_overlay, open_model_picker_for_provider
// moved to view_dispatch.rs

// handle_view_events moved to view_dispatch.rs

// Backtrack, view_dispatch, and overlay functions moved to view_dispatch.rs
