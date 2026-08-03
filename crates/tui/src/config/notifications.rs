//! Notifications and snapshot configuration.

use std::path::PathBuf;

use serde::Deserialize;

/// Notification condition for turn completion.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationCondition {
    /// Notify on every successful turn (no duration threshold).
    Always,
    /// Suppress notifications entirely.
    Never,
}

/// Notification delivery method.
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationMethod {
    /// Auto-detect: picks the best protocol for the current terminal.
    #[default]
    Auto,
    /// OSC 9 escape.
    Osc9,
    /// Plain BEL character.
    Bel,
    /// Kitty notification protocol (OSC 99).
    Kitty,
    /// Ghostty notification protocol (OSC 777).
    Ghostty,
    /// Disable notifications.
    Off,
}

fn default_threshold_secs() -> u64 {
    30
}

/// Completion sound options.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CompletionSound {
    /// No sound on turn completion.
    Off,
    /// System notification beep (default).
    #[default]
    Beep,
    /// Terminal BEL character (`\x07`).
    Bell,
    /// Play a configured WAV sound file.
    File,
}

/// Desktop-notification configuration (OSC 9 / BEL on turn completion).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NotificationsConfig {
    /// Delivery method: `auto` | `osc9` | `bel` | `off`. Default: `auto`.
    #[serde(default)]
    pub method: NotificationMethod,
    /// Only notify when the turn took at least this many seconds. Default: 30.
    #[serde(default = "default_threshold_secs")]
    pub threshold_secs: u64,
    /// Include a short summary (elapsed time + cost) in the notification body.
    #[serde(default)]
    pub include_summary: bool,
    /// Completion sound: `"off"` | `"beep"` | `"bell"` | `"file"`. Default: `"beep"`.
    #[serde(default)]
    pub completion_sound: CompletionSound,
    /// Path to the WAV sound file used when `completion_sound = "file"`.
    #[serde(default)]
    pub sound_file: Option<PathBuf>,
}

fn default_snapshots_enabled() -> bool {
    true
}

fn default_snapshot_max_age_days() -> u64 {
    crate::snapshot::DEFAULT_MAX_AGE.as_secs() / (24 * 60 * 60)
}

fn default_snapshot_max_workspace_gb() -> u64 {
    crate::snapshot::DEFAULT_MAX_WORKSPACE_BYTES_FOR_SNAPSHOT / (1024 * 1024 * 1024)
}

/// Workspace side-git snapshot configuration (#137).
#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotsConfig {
    /// Snapshot the workspace before and after each interactive agent turn.
    #[serde(default = "default_snapshots_enabled")]
    pub enabled: bool,
    /// Prune side-git snapshots older than this many days at session boot.
    #[serde(default = "default_snapshot_max_age_days")]
    pub max_age_days: u64,
    /// Maximum non-excluded workspace size (in GB) before the snapshot
    /// feature self-disables on first use.
    #[serde(default = "default_snapshot_max_workspace_gb")]
    pub max_workspace_gb: u64,
}

impl Default for SnapshotsConfig {
    fn default() -> Self {
        Self {
            enabled: default_snapshots_enabled(),
            max_age_days: default_snapshot_max_age_days(),
            max_workspace_gb: default_snapshot_max_workspace_gb(),
        }
    }
}

impl SnapshotsConfig {
    /// Maximum workspace bytes for snapshot.
    #[must_use]
    pub fn max_workspace_bytes(&self) -> u64 {
        self.max_workspace_gb.saturating_mul(1024 * 1024 * 1024)
    }

    /// Maximum snapshot age as a `Duration`.
    #[must_use]
    pub fn max_age(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.max_age_days.saturating_mul(24 * 60 * 60))
    }
}

/// User-level memory configuration (#489).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemoryConfig {
    /// When `true`, load the user memory file into the system prompt.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Optional external memory service URL for long-term memory.
    #[serde(default)]
    pub service_url: Option<String>,
    /// API key for the external memory service.
    #[serde(default)]
    pub service_api_key: Option<String>,
    /// Maximum number of memories to load from the service per session.
    #[serde(default)]
    pub max_memories: Option<usize>,
}

/// Xiaomi MiMo speech/TTS output configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpeechConfig {
    /// Default directory for generated speech/TTS files when no explicit
    /// output path is provided.
    #[serde(default)]
    pub output_dir: Option<String>,
}
