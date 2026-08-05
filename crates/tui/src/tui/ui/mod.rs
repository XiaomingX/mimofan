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

mod ui_event_loop;
pub(crate) use ui_event_loop::*;

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
