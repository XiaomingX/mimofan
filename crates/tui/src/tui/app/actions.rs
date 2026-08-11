//! Action definitions for the TUI application.

use std::path::PathBuf;

use crate::compaction::CompactionConfig;
use crate::config::ApiProvider;
use crate::config_ui::ConfigUiMode;
use crate::models::{Message, SystemPrompt};

use super::state::AppMode;

/// Actions emitted by the UI event loop.
#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    Quit,
    SaveSession(PathBuf),
    LoadSession(PathBuf),
    SyncSession {
        session_id: Option<String>,
        messages: Vec<Message>,
        system_prompt: Option<SystemPrompt>,
        model: String,
        workspace: PathBuf,
    },
    OpenConfigEditor(ConfigUiMode),
    OpenConfigView,
    /// Open the `/model` two-pane picker (Pro/Flash + Off/High/Max).
    OpenModelPicker,
    /// Open the `/provider` picker modal — DeepSeek / NVIDIA NIM / OpenRouter
    /// / Novita with inline API-key prompt for un-configured providers (#52).
    OpenProviderPicker,
    /// Open the `/mode` picker modal for Agent / Plan / YOLO.
    OpenModePicker,
    /// Open the backtrack overlay (equivalent to Esc Esc keyboard combination).
    OpenBacktrackOverlay,
    /// Refresh the engine prompt after the UI operating mode changes.
    ModeChanged(AppMode),
    /// Open the `/statusline` multi-select picker for footer items.
    OpenStatusPicker,
    /// Open the `/fleet` setup and loadout planner.
    OpenFleetSetup,
    /// Open an external URL in the system browser.
    OpenExternalUrl {
        url: String,
        label: String,
    },
    /// Send a message to the AI (normal chat mode).
    SendMessage(String),
    /// Update the runtime goal status (`/goal pause|resume|clear|…`) without
    /// dispatching a model turn. The UI layer translates this into
    /// `Op::SetGoalStatus`. `loop_config`, when `Some`, carries `/loop`-specific
    /// fields (stop condition, round cap, per-round checkpoint) written into the
    /// engine's `SharedGoalState` when the goal is (re)created.
    SetGoalStatus {
        status: crate::tools::goal::GoalStatus,
        clear: bool,
        loop_config: Option<crate::tools::goal::LoopConfig>,
    },
    ListSubAgents,
    FetchModels,
    CacheWarmup,
    /// Switch the active LLM backend (DeepSeek vs NVIDIA NIM) without
    /// restarting the process. The runtime rebuilds its API client from
    /// the updated config. `model` overrides the post-switch model
    /// (already normalized but not yet provider-prefixed).
    SwitchProvider {
        provider: ApiProvider,
        model: Option<String>,
    },
    UpdateCompaction(CompactionConfig),
    UpdateStreamChunkTimeout(u64),
    UpdateSubagentRuntimeConfig {
        enabled: bool,
        max_subagents: usize,
        launch_concurrency: usize,
        max_spawn_depth: u32,
        api_timeout_secs: u64,
        heartbeat_timeout_secs: u64,
    },
    OpenContextInspector,
    CompactContext {
        /// Optional `/compact <instructions>` guidance for this run.
        instructions: Option<String>,
    },
    PurgeContext,
    TaskAdd {
        prompt: String,
    },
    TaskList,
    TaskShow {
        id: String,
    },
    TaskCancel {
        id: String,
    },
    ShellJob(ShellJobAction),
    Mcp(McpUiAction),
    /// Switch to a different config profile without restarting.
    SwitchProfile {
        /// Profile name to load.
        profile: String,
    },
    /// Switch the workspace used by tools, hooks, tasks, and session metadata.
    SwitchWorkspace {
        workspace: PathBuf,
    },
    /// Record from the microphone and route the transcription into the
    /// composer (or auto-send it). Emitted by `/voice` and the voice hotbar
    /// action; handled in the UI event loop where the live `Config` supplies
    /// provider credentials.
    VoiceCapture,
    /// Export and share the current session as a web URL.
    ShareSession {
        history_len: usize,
        model: String,
        mode: String,
        /// When true, export the session to a local file instead of uploading to a Gist.
        local: bool,
    },
    /// Spec has been frozen (#557).
    SpecFrozen,
    /// Spec has been unfrozen (#557).
    SpecUnfrozen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellJobAction {
    List,
    Show {
        id: String,
    },
    Poll {
        id: String,
        wait: bool,
    },
    SendStdin {
        id: String,
        input: String,
        close: bool,
    },
    Cancel {
        id: String,
    },
    CancelAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpUiAction {
    Show,
    Init {
        force: bool,
    },
    AddStdio {
        name: String,
        command: String,
        args: Vec<String>,
    },
    AddHttp {
        name: String,
        url: String,
        transport: Option<String>,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    Remove {
        name: String,
    },
    Login {
        name: String,
        scopes: Vec<String>,
    },
    Logout {
        name: String,
    },
    Validate,
    Reload,
}
