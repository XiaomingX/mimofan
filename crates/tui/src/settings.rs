//! Settings system - Persistent user preferences
//!
//! Settings are stored at ~/.mimofan/settings.json.
//!
//! TUI-specific preferences (theme, keybinds, font_size) that survive project
//! switches are stored separately in tui.json. See [`TuiPrefs`].

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{ApiProvider, expand_path, normalize_model_name};
use crate::localization::normalize_configured_locale;
use crate::palette::{normalize_hex_rgb_color, normalize_theme_name};

const SETTINGS_FILE_NAME: &str = "settings.json";
const TUI_PREFS_FILE_NAME: &str = "tui.json";

// ============================================================================
// TuiPrefs — ~/.mimofan/tui.json
// ============================================================================

/// TUI-specific preferences that are decoupled from agent/project config so
/// they survive project switches (issue #437).
///
/// Stored at `~/.mimofan/tui.json`. When the file is
/// absent the values fall back to the struct's own defaults.
///
/// # Example `~/.mimofan/tui.json`
///
/// ```json
/// {
///   "theme": "dark",
///   "font_size": 14,
///   "keybinds": {
///     "submit": "ctrl+enter",
///     "new_line": "enter"
///   }
/// }
/// ```
//
// NOTE: the loader is defined but not yet called from startup — wiring is
// `-D warnings` failure until the call site lands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiPrefs {
    /// UI colour theme.
    /// Default `"dark"`.
    pub theme: String,
    /// Terminal font size hint forwarded to supporting front-ends (e.g. the
    /// Tauri shell). `0` means "use terminal default". Default `0`.
    pub font_size: u16,
    /// Key-binding overrides. Each field accepts an xterm-style chord string
    /// such as `"ctrl+enter"`, `"alt+n"`, or `"f1"`.
    pub keybinds: KeybindPrefs,
}

impl Default for TuiPrefs {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            font_size: 0,
            keybinds: KeybindPrefs::default(),
        }
    }
}

/// Per-action keybinding overrides stored inside [`TuiPrefs`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KeybindPrefs {
    /// Key to submit the current composer input to the model.
    /// Default: `"ctrl+enter"`.
    pub submit: Option<String>,
    /// Key to insert a literal newline inside the composer.
    /// Default: `"enter"`.
    pub new_line: Option<String>,
    /// Key to open the command palette.
    /// Default: `"ctrl+k"`.
    pub command_palette: Option<String>,
    /// Key to cancel / interrupt a running turn.
    /// Default: `"ctrl+c"`.
    pub cancel: Option<String>,
    /// Key to toggle the sidebar.
    /// Default: `"ctrl+b"`.
    pub toggle_sidebar: Option<String>,
}

impl TuiPrefs {
    /// Return the canonical path of the TUI preferences file:
    /// `~/.mimofan/tui.json`.
    ///
    /// Tests may override the home directory through the
    /// `MIMOFAN_CONFIG_PATH` environment variable (the parent directory of
    /// the pointed-to config is used instead of `~/.mimofan`).
    pub fn path() -> Result<PathBuf> {
        // Honour the same env-var escape hatch used by Settings::path so that
        // integration tests can redirect all config I/O to a temp directory.
        if let Ok(config_path) = std::env::var("MIMOFAN_CONFIG_PATH") {
            let config_path = config_path.trim();
            if !config_path.is_empty() {
                let p = expand_path(config_path);
                if let Some(parent) = p.parent() {
                    return Ok(parent.join(TUI_PREFS_FILE_NAME));
                }
            }
        }

        mimofan_config::mimofan_home()
            .ok()
            .map(|home| home.join(TUI_PREFS_FILE_NAME))
            .ok_or_else(|| {
                anyhow::anyhow!("Failed to resolve tui preferences path: no home directory found.")
            })
    }

    /// Load TUI preferences from `~/.mimofan/tui.json`.
    ///
    /// If the file does not exist the struct defaults are returned — no error
    /// is produced. Parse errors surface as `Err` so the caller can warn the
    /// user without crashing the session.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read tui.json from {}", path.display()))?;
        let prefs: TuiPrefs = match serde_json::from_str(&content) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to parse {} (using defaults): {e:#}", path.display());
                return Ok(Self::default());
            }
        };
        Ok(prefs)
    }

    /// Save TUI preferences to `~/.mimofan/tui.json`, creating the target
    /// directory if needed.
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }
        let body = serde_json::to_string_pretty(self).context("Failed to serialize TuiPrefs")?;
        std::fs::write(&path, body)
            .with_context(|| format!("Failed to write tui.json to {}", path.display()))?;
        Ok(())
    }

    /// Validate field values and normalise them in place.
    ///
    /// Returns `Err` if an unrecognised `theme` value is found so callers can
    /// surface a helpful message rather than silently ignoring a typo.
    pub fn validate(&mut self) -> Result<()> {
        let theme = self.theme.trim().to_ascii_lowercase();
        let Some(theme) = normalize_theme_name(&theme) else {
            anyhow::bail!(
                "Invalid tui.json theme '{}': expected system, dark, light, grayscale, catppuccin-mocha, tokyo-night, dracula, gruvbox-dark, or solarized-light.",
                self.theme
            );
        };
        self.theme = theme.to_string();
        Ok(())
    }
}

/// User settings with defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Auto-compact conversations when they approach the model limit.
    pub auto_compact: bool,
    /// Context-window percentage that triggers pre-send auto-compaction when
    /// `auto_compact` is enabled. The hard token floor still applies.
    pub compact_threshold: f64,
    /// Reduce status noise and collapse details more aggressively.
    pub calm_mode: bool,
    /// Dense tool-run collapse mode: compact, expanded, or calm.
    pub tool_collapse: String,
    /// Streaming pacing mode. `true` pins the chunker to one-character-per-
    /// commit-tick (typewriter); `false` drains the upstream cadence.
    pub low_motion: bool,
    /// Enable the footer water-spout animation strip during live turns.
    pub fancy_animations: bool,
    /// Enable terminal bracketed-paste mode. Default true.
    pub bracketed_paste: bool,
    /// Enable rapid-key paste-burst detection for terminals that do not emit
    /// bracketed-paste events.
    pub paste_burst_detection: bool,
    /// Maximum number of file-mention popup candidates.
    pub mention_limit: usize,
    /// Maximum workspace depth for `@`-mention completion walks. `0` means
    /// unlimited depth.
    pub mention_depth: usize,
    /// `@`-mention completion behavior: fuzzy or directory.
    pub mention_behavior: String,
    /// Show thinking blocks from the model.
    pub show_thinking: bool,
    /// Show detailed tool output.
    pub show_tool_details: bool,
    /// UI locale: auto, en, ja, zh-Hans, pt-BR, es-419.
    pub locale: String,
    /// Named UI theme: system, dark, light, grayscale, catppuccin-mocha,
    /// tokyo-night, dracula, gruvbox-dark.
    pub theme: String,
    /// Optional main TUI background color as a 6-digit hex RGB value.
    pub background_color: Option<String>,
    /// Composer layout density: compact, comfortable, spacious.
    pub composer_density: String,
    /// Show a border around the composer input area.
    pub composer_border: bool,
    /// Composer editing mode: normal or vim.
    pub vim_mode: String,
    /// Transcript spacing rhythm: compact, comfortable, spacious.
    pub transcript_spacing: String,
    /// Default mode: agent, plan, yolo.
    pub default_mode: String,
    /// Sidebar width as percentage of terminal width (10-50).
    pub sidebar_width: u16,
    /// Sidebar focus mode: pinned, auto, tasks, agents, context, hidden.
    pub sidebar_focus: String,
    /// Allow idle auto-collapse of sidebar.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sidebar_collapse: bool,
    /// Enable the session-context panel.
    pub context_panel: bool,
    /// Cost display currency: usd or cny.
    pub cost_currency: String,
    /// Maximum number of input history entries to save.
    pub max_input_history: usize,
    /// Default provider override (e.g. "deepseek", "openai").
    pub default_provider: Option<String>,
    /// Default model to use.
    pub default_model: Option<String>,
    /// Default reasoning effort selected from the TUI model picker.
    pub reasoning_effort: Option<String>,
    /// Per-provider model overrides. Key is provider name, value is model id.
    pub provider_models: Option<std::collections::HashMap<String, String>>,
    /// Header status indicator: whale, dots, or off.
    pub status_indicator: String,
    /// Synchronized output mode: auto, on, or off.
    pub synchronized_output: String,
    /// Prefer external pdftotext over pure-Rust extractor for PDF reads.
    pub prefer_external_pdftotext: bool,
    /// Follow symbolic links during workspace file discovery walks.
    pub workspace_follow_symlinks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_compact: false,
            compact_threshold: 80.0,
            calm_mode: false,
            tool_collapse: "compact".to_string(),
            low_motion: false,
            fancy_animations: true,
            bracketed_paste: true,
            paste_burst_detection: true,
            mention_limit: 128,
            mention_depth: 10,
            mention_behavior: "fuzzy".to_string(),
            show_thinking: true,
            show_tool_details: true,
            locale: "auto".to_string(),
            theme: "system".to_string(),
            background_color: None,
            composer_density: "comfortable".to_string(),
            composer_border: true,
            vim_mode: "normal".to_string(),
            transcript_spacing: "comfortable".to_string(),
            default_mode: "agent".to_string(),
            sidebar_width: 28,
            sidebar_focus: "pinned".to_string(),
            sidebar_collapse: false,
            context_panel: false,
            cost_currency: "usd".to_string(),
            max_input_history: 100,
            default_provider: None,
            default_model: None,
            reasoning_effort: None,
            provider_models: None,
            status_indicator: "whale".to_string(),
            synchronized_output: "auto".to_string(),
            prefer_external_pdftotext: false,
            workspace_follow_symlinks: false,
        }
    }
}

/// The `calm` transcript preset (#3478): a coherent "beautiful/calm" bundle that
/// favors a quiet, readable transcript over debug-dense output. Presentation
/// only, and evidence-preserving — `show_thinking` is deliberately left untouched
/// (thinking stays visible) and tool runs only have their inline detail
/// collapsed, never hidden. Keyed by [`Settings::set`] names so the preset and a
/// single-key `/config` set share one validation path.
pub const CALM_PRESET_FIELDS: &[(&str, &str)] = &[
    ("calm_mode", "true"),
    ("tool_collapse", "calm"),
    ("transcript_spacing", "comfortable"),
    ("low_motion", "true"),
    ("fancy_animations", "false"),
    ("show_tool_details", "false"),
];

/// The `(key, value)` fields a named preset applies, or `None` for an unknown
/// name. Single source of truth shared by [`Settings::apply_preset`] and the
/// `/config preset` command so the bundle is never defined twice.
#[must_use]
pub fn preset_fields(name: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match name.trim().to_ascii_lowercase().as_str() {
        "calm" => Some(CALM_PRESET_FIELDS),
        _ => None,
    }
}

impl Settings {
    /// Get the canonical settings file path.
    ///
    /// Settings are stored at `~/.mimofan/settings.json`.
    pub fn path() -> Result<PathBuf> {
        settings_path().ok_or_else(|| {
            anyhow::anyhow!("Failed to resolve settings path: no config directory found.")
        })
    }

    /// Load settings from disk, or return defaults if not found
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            let mut settings = Self::default();
            settings.apply_env_overrides();
            return Ok(settings);
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read settings from {}", path.display()))?;
        let mut s: Settings = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to parse {} (using defaults): {e:#}", path.display());
                return Ok(Self::default());
            }
        };
        s.default_mode = normalize_mode(&s.default_mode).to_string();
        s.composer_density = normalize_composer_density(&s.composer_density).to_string();
        s.transcript_spacing = normalize_transcript_spacing(&s.transcript_spacing).to_string();
        s.tool_collapse = normalize_tool_collapse_mode(&s.tool_collapse).to_string();
        s.sidebar_focus = normalize_sidebar_focus(&s.sidebar_focus).to_string();
        if s.sidebar_focus == "auto" && !s.sidebar_collapse {
            s.sidebar_focus = "pinned".to_string();
        }
        s.status_indicator = normalize_status_indicator(&s.status_indicator).to_string();
        s.synchronized_output = normalize_synchronized_output(&s.synchronized_output).to_string();
        s.locale = normalize_configured_locale(&s.locale)
            .unwrap_or("en")
            .to_string();
        s.background_color = normalize_optional_background_color(s.background_color.as_deref());
        s.theme = normalize_settings_theme(&s.theme).to_string();
        s.default_model = s.default_model.as_deref().and_then(normalize_default_model);
        s.reasoning_effort = s
            .reasoning_effort
            .as_deref()
            .and_then(|value| normalize_reasoning_effort_setting(value).ok().flatten());
        s.apply_env_overrides();
        Ok(s)
    }

    /// Whether the user explicitly persisted an `auto_compact` preference.
    /// When absent, callers may choose a model-aware default.
    pub fn auto_compact_explicitly_configured() -> bool {
        let path = Self::path().ok();
        let Some(path) = path else {
            return false;
        };
        let Ok(content) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return false;
        };
        value
            .as_object()
            .is_some_and(|obj| obj.contains_key("auto_compact"))
    }

    /// Apply environment-driven overlays after disk load. Used for
    /// platform a11y signals that should ignore the user's saved
    /// preference (#450). The env values are consulted at startup;
    /// changing them mid-session has no effect because settings are
    /// only re-read on `Settings::load()`.
    pub fn apply_env_overrides(&mut self) {
        if env_truthy("NO_ANIMATIONS") {
            self.low_motion = true;
            self.fancy_animations = false;
        }
        // VS Code (TERM_PROGRAM=vscode, #1356), Ghostty (#1445), and a few
        // VTE terminals (#1470) produce visible flicker at 120 FPS. Drop to
        // the 30 FPS low-motion cap for them automatically. Ghostty may report
        // either TERM_PROGRAM=Ghostty/ghostty or TERM=xterm-ghostty.
        // Like NO_ANIMATIONS above, this unconditionally overrides any
        // disk-loaded value — consistent precedence: env signals always win.
        let term_program = std::env::var("TERM_PROGRAM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let term = std::env::var("TERM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let term_forces_low_motion =
            matches!(term_program.as_str(), "vscode" | "ghostty") || term.contains("ghostty");
        let vte_env_forces_low_motion = std::env::var_os("TILIX_ID").is_some_and(|v| !v.is_empty())
            || std::env::var_os("TERMINATOR_UUID").is_some_and(|v| !v.is_empty());
        if term_forces_low_motion || vte_env_forces_low_motion {
            self.low_motion = true;
            self.fancy_animations = false;
        }

        // Termius (TERM_PROGRAM=Termius) and SSH sessions exhibit the
        // same 120-FPS flicker class as VS Code — the SSH round-trip
        // races ahead of what the remote renderer can flush, so rapid
        // cursor-positioning sequences cycle through input boxes.
        // Drop both to the 30 FPS low-motion cap. Harvested from
        // PR #1479 by @CrepuscularIRIS / autoghclaw (closes #1433).
        //
        // SSH_CLIENT is exported by sshd for every TCP SSH session;
        // SSH_TTY is exported only for interactive PTY logins, so we
        // check both so non-PTY-allocating tools (rsync wrappers, etc.)
        // still pick this up if they end up running the TUI.
        let term_is_termius = std::env::var("TERM_PROGRAM").as_deref() == Ok("Termius");
        let in_ssh_session = std::env::var_os("SSH_CLIENT").is_some_and(|v| !v.is_empty())
            || std::env::var_os("SSH_TTY").is_some_and(|v| !v.is_empty());
        if term_is_termius || in_ssh_session {
            self.low_motion = true;
            self.fancy_animations = false;
        }

        // tmux/screen activity monitors treat purely animated redraws as
        // activity. Keep multiplexer sessions calm by pinning animations.
        let in_terminal_multiplexer = std::env::var_os("TMUX").is_some_and(|v| !v.is_empty())
            || std::env::var_os("STY").is_some_and(|v| !v.is_empty());
        if in_terminal_multiplexer {
            self.low_motion = true;
            self.fancy_animations = false;
        }

        // Plain Windows PowerShell / cmd.exe under legacy ConHost exposes none
        // of the modern terminal markers below. Keep rendering calmer there:
        // lower the motion rate, disable animated chrome, and avoid DEC 2026
        // synchronized-output wrapping unless the user explicitly forced it on.
        if detected_legacy_windows_console_host() {
            self.low_motion = true;
            self.fancy_animations = false;
            if self.synchronized_output.eq_ignore_ascii_case("auto") {
                self.synchronized_output = "off".to_string();
            }
        }

        // Ptyxis 50.x (the new default terminal on Ubuntu 26.04) ships with
        // VTE 0.84.x which mishandles DEC mode 2026 synchronized output: the
        // begin/end pair is parsed but each wrapped frame still triggers a
        // full-viewport flash on the GPU compositor side, so any TUI that
        // uses DEC 2026 to avoid tearing instead gets visible flicker on
        // every redraw. gnome-terminal 3.58 on the same VTE renders cleanly,
        // so we can't broaden the opt-out to all VTE-based terminals —
        // only the Ptyxis-specific signals trigger it. Confirmed
        // user-visible regression starting with Ubuntu 26.04's default
        // terminal swap; cargo-installed binaries are not exempt because
        // the bug is in the terminal, not the binary.
        //
        // Only flip `auto` to `off`; respect an explicit `"on"` so users
        // who upgrade Ptyxis or want to confirm the fix landed upstream
        // can override the heuristic from the persisted settings.json or
        // `/set synchronized_output on`.
        if self.synchronized_output.eq_ignore_ascii_case("auto") && detected_ptyxis_terminal() {
            self.synchronized_output = "off".to_string();
        }
    }

    /// Save settings to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }

        let body = serde_json::to_string_pretty(self).context("Failed to serialize settings")?;
        std::fs::write(&path, body)
            .with_context(|| format!("Failed to write settings to {}", path.display()))?;
        Ok(())
    }

    /// Update and persist sidebar width percentage (10-50) — used by the
    /// drag-to-resize handle in the TUI.
    pub fn update_sidebar_width(&mut self, percent: u16) {
        self.sidebar_width = percent.clamp(10, 50);
    }

    /// Set a single setting by key
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "auto_compact" | "compact" => {
                self.auto_compact = parse_bool(value)?;
            }
            "compact_threshold" => {
                self.compact_threshold = parse_percent_setting("compact_threshold", value)?;
            }
            "calm_mode" | "calm" => {
                self.calm_mode = parse_bool(value)?;
            }
            "tool_collapse" | "collapse" => {
                let normalized = normalize_tool_collapse_mode(value);
                if !matches!(normalized, "compact" | "expanded" | "calm") {
                    return Err(anyhow::anyhow!(
                        "Failed to update setting: invalid tool collapse '{value}'. Expected: compact, expanded, or calm."
                    ));
                }
                self.tool_collapse = normalized.to_string();
            }
            "low_motion" | "motion" => {
                self.low_motion = parse_bool(value)?;
            }
            "fancy_animations" | "fancy" | "animations" => {
                self.fancy_animations = parse_bool(value)?;
            }
            "bracketed_paste" | "paste" => {
                self.bracketed_paste = parse_bool(value)?;
            }
            "paste_burst_detection" | "paste_burst" => {
                self.paste_burst_detection = parse_bool(value)?;
            }
            "mention_limit" => {
                self.mention_limit = parse_usize_setting("mention_limit", value)?;
            }
            "mention_depth" => {
                self.mention_depth = parse_usize_setting("mention_depth", value)?;
            }
            "mention_behavior" => {
                self.mention_behavior = normalize_mention_behavior(value)?;
            }
            "show_thinking" | "thinking" => {
                self.show_thinking = parse_bool(value)?;
            }
            "show_tool_details" | "tool_details" => {
                self.show_tool_details = parse_bool(value)?;
            }
            "locale" | "language" => {
                let Some(locale) = normalize_configured_locale(value) else {
                    anyhow::bail!(
                        "Failed to update setting: invalid locale '{value}'. Expected: auto, en, ja, zh-Hans, pt-BR, es-419."
                    );
                };
                self.locale = locale.to_string();
            }
            "theme" => {
                let Some(id) = crate::palette::ThemeId::from_name(value) else {
                    anyhow::bail!(
                        "Failed to update setting: invalid theme '{value}'. Expected: system, dark, light, grayscale, catppuccin-mocha, tokyo-night, dracula, gruvbox-dark, solarized-light."
                    );
                };
                self.theme = id.name().to_string();
            }
            "background_color" | "background" | "bg" => {
                self.background_color = normalize_background_color_setting(value)?;
            }
            "composer_density" | "composer" => {
                let normalized = normalize_composer_density(value);
                if !["compact", "comfortable", "spacious"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid composer density '{value}'. Expected: compact, comfortable, spacious."
                    );
                }
                self.composer_density = normalized.to_string();
            }
            "composer_border" | "border" => {
                self.composer_border = parse_bool(value)?;
            }
            "vim_mode" | "vim" => {
                let normalized = value.trim().to_ascii_lowercase();
                if !["vim", "normal"].contains(&normalized.as_str()) {
                    anyhow::bail!(
                        "Failed to update setting: invalid vim mode '{value}'. Expected: normal, vim."
                    );
                }
                self.vim_mode = normalized;
            }
            "transcript_spacing" | "spacing" => {
                let normalized = normalize_transcript_spacing(value);
                if !["compact", "comfortable", "spacious"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid transcript spacing '{value}'. Expected: compact, comfortable, spacious."
                    );
                }
                self.transcript_spacing = normalized.to_string();
            }
            "status_indicator" | "indicator" => {
                let normalized = normalize_status_indicator(value);
                if !["whale", "dots", "off"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid status indicator '{value}'. Expected: whale, dots, off."
                    );
                }
                self.status_indicator = normalized.to_string();
            }
            "synchronized_output" | "sync_output" | "sync" => {
                let normalized = normalize_synchronized_output(value);
                if !["auto", "on", "off"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid synchronized_output '{value}'. Expected: auto, on, off."
                    );
                }
                self.synchronized_output = normalized.to_string();
            }
            "prefer_external_pdftotext" | "external_pdftotext" | "pdftotext" => {
                self.prefer_external_pdftotext = parse_bool(value)?;
            }
            "workspace_follow_symlinks" | "follow_symlinks" => {
                self.workspace_follow_symlinks = parse_bool(value)?;
            }
            "default_mode" | "mode" => {
                let normalized = normalize_mode(value);
                if !["agent", "plan", "yolo"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid mode '{value}'. Expected: agent, plan, yolo."
                    );
                }
                self.default_mode = normalized.to_string();
            }
            "sidebar_width" | "sidebar" => {
                let width: u16 = value
                    .parse()
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "Failed to update setting: invalid width '{value}'. Expected a number between 10-50."
                        )
                    })?;
                if !(10..=50).contains(&width) {
                    anyhow::bail!(
                        "Failed to update setting: width must be between 10 and 50 percent."
                    );
                }
                self.sidebar_width = width;
            }
            "sidebar_focus" | "focus" => {
                let normalized = match value.trim().to_ascii_lowercase().as_str() {
                    "auto" => "auto",
                    "pinned" | "visible" | "show" | "on" | "work" | "plan" | "todos" => "pinned",
                    "tasks" => "tasks",
                    "agents" | "subagents" | "sub-agents" => "agents",
                    "context" | "session" => "context",
                    "hidden" | "hide" | "closed" | "off" | "none" => "hidden",
                    _ => {
                        anyhow::bail!(
                            "Failed to update setting: invalid sidebar focus '{value}'. Expected: pinned, auto, tasks, agents, context, hidden."
                        )
                    }
                };
                self.sidebar_focus = normalized.to_string();
                self.sidebar_collapse = normalized == "auto";
            }
            "context_panel" | "context" | "session_panel" => {
                self.context_panel = parse_bool(value)?;
            }
            "cost_currency" | "currency" => {
                let Some(currency) = crate::pricing::CostCurrency::from_setting(value) else {
                    anyhow::bail!(
                        "Failed to update setting: invalid cost currency '{value}'. Expected: usd, cny, rmb, yuan."
                    );
                };
                self.cost_currency = match currency {
                    crate::pricing::CostCurrency::Usd => "usd",
                    crate::pricing::CostCurrency::Cny => "cny",
                }
                .to_string();
            }
            "max_history" | "history" => {
                let max: usize = value.parse().map_err(|_| {
                    anyhow::anyhow!(
                        "Failed to update setting: invalid max history '{value}'. Expected a positive number."
                    )
                })?;
                self.max_input_history = max;
            }
            "default_model" | "model" => {
                let trimmed = value.trim();
                if trimmed.is_empty()
                    || matches!(
                        trimmed.to_ascii_lowercase().as_str(),
                        "none" | "default" | "(default)"
                    )
                {
                    self.default_model = None;
                    return Ok(());
                }

                let Some(model) = normalize_default_model(trimmed) else {
                    anyhow::bail!(
                        "Failed to update setting: invalid model '{value}'. Expected: auto, a DeepSeek model ID (for example deepseek-v4-pro, deepseek-v4-flash), or none/default."
                    );
                };
                self.default_model = Some(model);
            }
            "reasoning_effort" | "effort" => {
                self.reasoning_effort = normalize_reasoning_effort_setting(value)?;
            }
            _ => {
                anyhow::bail!("Failed to update setting: unknown setting '{key}'.");
            }
        }
        Ok(())
    }

    /// Apply a named settings preset (#3478).
    ///
    /// Presets are the first bundled-settings mechanism: a single name applies a
    /// coherent group of presentation knobs. `calm` is the "beautiful/calm
    /// transcript" preset — it quiets motion and verbose tool output while
    /// **keeping evidence reachable**: thinking stays visible and tool runs stay
    /// expandable (only their inline detail is collapsed), so maintainer/release
    /// work is never blind to failures. Presentation only — no model, provider,
    /// routing, or safety setting is touched. Reuses [`Settings::set`] so each
    /// field goes through the same validation as a single-key set.
    ///
    /// Returns the keys changed, or an error for an unknown preset.
    pub fn apply_preset(&mut self, name: &str) -> Result<Vec<&'static str>> {
        let Some(bundle) = preset_fields(name) else {
            anyhow::bail!("Unknown preset '{}'. Available presets: calm", name.trim());
        };
        let mut changed = Vec::with_capacity(bundle.len());
        for (key, value) in bundle {
            self.set(key, value)?;
            changed.push(*key);
        }
        Ok(changed)
    }

    /// Get all settings as a displayable string
    pub fn display(&self, locale: crate::localization::Locale) -> String {
        use crate::localization::{MessageId, tr};
        let mut lines = Vec::new();
        lines.push(tr(locale, MessageId::SettingsTitle).to_string());
        lines.push("─────────────────────────────".to_string());
        lines.push(format!("  auto_compact:       {}", self.auto_compact));
        lines.push(format!(
            "  compact_threshold:  {:.0}",
            self.compact_threshold
        ));
        lines.push(format!("  calm_mode:          {}", self.calm_mode));
        lines.push(format!("  tool_collapse:      {}", self.tool_collapse));
        lines.push(format!("  low_motion:         {}", self.low_motion));
        lines.push(format!("  fancy_animations:   {}", self.fancy_animations));
        lines.push(format!("  bracketed_paste:    {}", self.bracketed_paste));
        lines.push(format!(
            "  paste_burst_detect: {}",
            self.paste_burst_detection
        ));
        lines.push(format!("  mention_limit:      {}", self.mention_limit));
        lines.push(format!("  mention_depth:      {}", self.mention_depth));
        lines.push(format!("  mention_behavior:   {}", self.mention_behavior));
        lines.push(format!("  show_thinking:      {}", self.show_thinking));
        lines.push(format!("  show_tool_details:  {}", self.show_tool_details));
        lines.push(format!("  locale:             {}", self.locale));
        lines.push(format!("  theme:              {}", self.theme));
        lines.push(format!(
            "  background_color:   {}",
            self.background_color.as_deref().unwrap_or("(default)")
        ));
        lines.push(format!("  composer_density:   {}", self.composer_density));
        lines.push(format!("  composer_border:    {}", self.composer_border));
        lines.push(format!("  vim_mode:           {}", self.vim_mode));
        lines.push(format!("  transcript_spacing: {}", self.transcript_spacing));
        lines.push(format!("  status_indicator:   {}", self.status_indicator));
        lines.push(format!(
            "  synchronized_output: {}",
            self.synchronized_output
        ));
        lines.push(format!(
            "  prefer_external_pdftotext: {}",
            self.prefer_external_pdftotext
        ));
        lines.push(format!(
            "  workspace_follow_symlinks: {}",
            self.workspace_follow_symlinks
        ));
        lines.push(format!("  default_mode:       {}", self.default_mode));
        lines.push(format!("  sidebar_width:      {}%", self.sidebar_width));
        lines.push(format!("  sidebar_focus:      {}", self.sidebar_focus));
        lines.push(format!("  context_panel:      {}", self.context_panel));
        lines.push(format!("  cost_currency:      {}", self.cost_currency));
        lines.push(format!("  max_history:        {}", self.max_input_history));
        lines.push(format!(
            "  default_model:      {}",
            self.default_model.as_deref().unwrap_or("(default)")
        ));
        lines.push(format!(
            "  reasoning_effort:   {}",
            self.reasoning_effort
                .as_deref()
                .unwrap_or("(config/default)")
        ));
        lines.push(String::new());
        lines.push(format!(
            "{} {}",
            tr(locale, MessageId::SettingsConfigFile),
            Self::path().map_or_else(|_| "(unknown)".to_string(), |p| p.display().to_string())
        ));
        lines.join("\n")
    }

    /// Get available setting keys and their descriptions
    pub fn available_settings() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "auto_compact",
                "Auto-compact near the hard context limit: on/off (model-aware default)",
            ),
            (
                "compact_threshold",
                "Auto-compact trigger threshold percent when auto_compact is on: 10-100 (default 80)",
            ),
            ("calm_mode", "Calmer UI defaults: on/off"),
            (
                "tool_collapse",
                "Dense tool-run collapse mode: compact, expanded, calm",
            ),
            (
                "low_motion",
                "Streaming pacing: on = typewriter (one char/tick), off = upstream cadence",
            ),
            (
                "fancy_animations",
                "Footer water-spout strip (wave synced to typing speed): on/off",
            ),
            (
                "bracketed_paste",
                "Terminal bracketed-paste mode: on/off (rare to disable)",
            ),
            (
                "paste_burst_detection",
                "Fallback rapid-key paste detection: on/off",
            ),
            (
                "mention_limit",
                "Maximum @-mention popup candidates retained before rendering (default 128)",
            ),
            (
                "mention_depth",
                "Maximum @-mention workspace walk depth; 0 means unlimited (default 6)",
            ),
            (
                "mention_behavior",
                "@-mention completion behavior: fuzzy/browser (default fuzzy)",
            ),
            ("show_thinking", "Show model thinking: on/off"),
            ("show_tool_details", "Show detailed tool output: on/off"),
            (
                "base_url",
                "HTTP base URL for DeepSeek-compatible endpoints.",
            ),
            (
                "locale",
                "UI locale and default model language: auto, en, ja, zh-Hans, pt-BR, es-419",
            ),
            (
                "theme",
                "UI theme: system, dark, light, grayscale, catppuccin-mocha, tokyo-night, dracula, gruvbox-dark, solarized-light",
            ),
            (
                "background_color",
                "Main TUI background color: #RRGGBB or default",
            ),
            (
                "composer_density",
                "Composer density: compact, comfortable, spacious",
            ),
            (
                "composer_border",
                "Show a border around the composer input area: on/off",
            ),
            ("vim_mode", "Composer editing mode: normal, vim"),
            (
                "transcript_spacing",
                "Transcript spacing: compact, comfortable, spacious",
            ),
            (
                "status_indicator",
                "Header status indicator next to effort chip: whale, dots, off",
            ),
            (
                "synchronized_output",
                "DEC 2026 synchronized output: auto, on, off (set off if your terminal flickers)",
            ),
            (
                "prefer_external_pdftotext",
                "Route PDF reads through Poppler's pdftotext instead of the bundled pure-Rust extractor: on/off (default off)",
            ),
            (
                "workspace_follow_symlinks",
                "Follow symbolic links during workspace file discovery walks: on/off (default off). Enable for symlink-based multi-project workspaces. Has built-in cycle detection but may increase latency on large symlinked trees.",
            ),
            ("default_mode", "Default mode: agent, plan, yolo"),
            ("sidebar_width", "Sidebar width percentage: 10-50"),
            (
                "sidebar_focus",
                "Sidebar focus: auto, work, tasks, agents, context, hidden",
            ),
            (
                "context_panel",
                "Show the session context sidebar panel: on/off",
            ),
            ("cost_currency", "Cost display currency: usd, cny"),
            ("max_history", "Max input history entries"),
            (
                "default_model",
                "Default model: auto or any DeepSeek model ID (e.g. deepseek-v4-pro)",
            ),
            (
                "reasoning_effort",
                "Default thinking effort: auto, off, low, medium, high, max, or default",
            ),
        ]
    }

    /// Persist the model for a specific provider.
    pub fn set_model_for_provider(&mut self, provider: &str, model: &str) {
        self.provider_models
            .get_or_insert_with(std::collections::HashMap::new)
            .insert(provider.to_string(), model.to_string());
    }

    /// Persist a provider's model selection.
    ///
    /// `persist_as_default` controls the blast radius (#3227):
    ///
    /// - `false` (session-local, the default for `/model` and the model
    ///   picker): record the model only under that provider's scoped entry in
    ///   [`Self::provider_models`]. The shared `default_provider` and global
    ///   `default_model` are left untouched, so a model change in one terminal
    ///   no longer rewrites the global default that a second terminal reads on
    ///   startup. This is what stopped a GLM/Z.ai session from being dragged
    ///   onto a DeepSeek model (and vice-versa).
    /// - `true` (explicit "save as default"): also pin `default_provider`, and
    ///   for DeepSeek providers the global `default_model`, to this tuple.
    pub fn set_provider_model_selection(
        &mut self,
        provider: ApiProvider,
        model: &str,
        persist_as_default: bool,
    ) -> Result<()> {
        let model = model.trim();
        if model.is_empty() {
            anyhow::bail!("model cannot be empty");
        }
        self.set_model_for_provider(provider.as_str(), model);
        if persist_as_default {
            self.default_provider = Some(provider.as_str().to_string());
            if matches!(provider, ApiProvider::XiaomiMimo) {
                self.set("default_model", model)?;
            }
        }
        Ok(())
    }

    /// Load, update, and save a provider's model selection *without* touching
    /// the shared global default (the session-local path; see
    /// [`Self::set_provider_model_selection`]).
    pub fn persist_provider_model_selection(provider: ApiProvider, model: &str) -> Result<()> {
        let mut settings = Self::load()?;
        settings.set_provider_model_selection(provider, model, false)?;
        settings.save()
    }

    /// Resolved boolean for whether the renderer should wrap each frame in
    /// DEC mode 2026 synchronized output. `auto` and `on` enable; `off`
    /// disables. The `auto` → `off` flip for known-bad terminals happens
    /// earlier in [`Self::apply_env_overrides`]; this method only inspects
    /// the final state.
    #[must_use]
    pub fn synchronized_output_enabled(&self) -> bool {
        !self.synchronized_output.eq_ignore_ascii_case("off")
    }

    /// Runtime bracketed-paste mode after terminal-host quirks are applied.
    ///
    /// This deliberately does not mutate [`Settings::bracketed_paste`]:
    /// `apply_env_overrides()` can run before saving settings, and a legacy
    /// conhost runtime fallback must not permanently disable bracketed paste
    /// when the same config is later used in Windows Terminal or another
    /// modern terminal.
    #[must_use]
    pub fn effective_bracketed_paste(&self) -> bool {
        self.bracketed_paste && !detected_legacy_windows_console_host()
    }
}

fn settings_path() -> Option<PathBuf> {
    // Allow tests to override the settings directory via the same env var
    // used for config (MIMOFAN_CONFIG_PATH points at config.toml; the
    // settings file lives as a sibling in the same directory).
    if let Ok(config_path) = std::env::var("MIMOFAN_CONFIG_PATH") {
        let config_path = config_path.trim();
        if !config_path.is_empty() {
            let p = expand_path(config_path);
            if let Some(parent) = p.parent() {
                return Some(parent.join(SETTINGS_FILE_NAME));
            }
        }
    }

    mimofan_config::mimofan_home()
        .ok()
        .map(|home| home.join(SETTINGS_FILE_NAME))
}

fn normalize_default_model(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("auto") {
        Some("auto".to_string())
    } else {
        normalize_model_name(trimmed)
    }
}

fn normalize_reasoning_effort_setting(value: &str) -> Result<Option<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "default" | "(default)" | "config" | "configured" | "unset"
        )
    {
        return Ok(None);
    }

    let normalized = match trimmed.to_ascii_lowercase().as_str() {
        "off" | "disabled" | "none" | "false" => "off",
        "low" | "minimal" => "low",
        "medium" | "mid" => "medium",
        "high" => "high",
        "auto" | "automatic" => "auto",
        "max" | "maximum" | "xhigh" | "ultracode" => "max",
        _ => {
            anyhow::bail!(
                "Failed to update setting: invalid reasoning_effort '{value}'. Expected: auto, off, low, medium, high, max, xhigh, ultracode, or default."
            );
        }
    };
    Ok(Some(normalized.to_string()))
}

/// Parse a boolean value from various formats
fn parse_bool(value: &str) -> Result<bool> {
    match value.to_lowercase().as_str() {
        "on" | "true" | "yes" | "1" | "enabled" => Ok(true),
        "off" | "false" | "no" | "0" | "disabled" => Ok(false),
        _ => {
            anyhow::bail!("Failed to parse boolean '{value}': expected on/off, true/false, yes/no.")
        }
    }
}

fn parse_usize_setting(key: &str, value: &str) -> Result<usize> {
    value.trim().parse::<usize>().map_err(|_| {
        anyhow::anyhow!(
            "Failed to update setting: invalid {key} '{value}'. Expected 0 or a positive integer."
        )
    })
}

fn parse_percent_setting(key: &str, value: &str) -> Result<f64> {
    let trimmed = value.trim().trim_end_matches('%').trim();
    let percent = trimmed.parse::<f64>().map_err(|_| {
        anyhow::anyhow!(
            "Failed to update setting: invalid {key} '{value}'. Expected a number from 10 to 100."
        )
    })?;
    if !(10.0..=100.0).contains(&percent) {
        anyhow::bail!(
            "Failed to update setting: invalid {key} '{value}'. Expected a number from 10 to 100."
        );
    }
    Ok(percent)
}

fn normalize_mention_behavior(value: &str) -> Result<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "fuzzy" | "default" => Ok("fuzzy".to_string()),
        "browser" | "browse" | "file-browser" | "file_browser" => Ok("browser".to_string()),
        _ => {
            anyhow::bail!(
                "Failed to update setting: invalid mention_behavior '{value}'. Expected: fuzzy, browser."
            )
        }
    }
}

fn normalize_mode(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "edit" => "agent",
        "normal" => "agent",
        "agent" => "agent",
        "plan" => "plan",
        "yolo" => "yolo",
        _ => value,
    }
}

fn normalize_composer_density(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" | "tight" => "compact",
        "comfortable" | "default" | "normal" => "comfortable",
        "spacious" | "loose" => "spacious",
        _ => value,
    }
}

fn normalize_transcript_spacing(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" | "tight" => "compact",
        "comfortable" | "default" | "normal" => "comfortable",
        "spacious" | "loose" => "spacious",
        _ => value,
    }
}

fn normalize_tool_collapse_mode(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" | "default" | "on" | "true" => "compact",
        "expanded" | "expand" | "off" | "none" | "false" => "expanded",
        "calm" | "calm_mode" | "calm-mode" | "calm_only" | "calm-only" => "calm",
        _ => value,
    }
}

/// Normalize the `status_indicator` header chip setting. Accepts the
/// canonical names plus common aliases ("none"/"hidden" → "off",
/// "dot" → "dots"). Unknown values fall through unchanged so the parser
/// in `update_setting` can surface a clear error.
fn normalize_status_indicator(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "whale" | "🐳" | "🐋" => "whale",
        "dots" | "dot" => "dots",
        "off" | "none" | "hidden" | "false" => "off",
        _ => value,
    }
}

/// Normalize the `synchronized_output` setting. Accepts the canonical
/// `"auto"` / `"on"` / `"off"` plus the usual truthy/falsey spellings.
/// Unknown values fall through unchanged so the parser in `set` can
/// surface a clear error.
fn normalize_synchronized_output(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" | "default" => "auto",
        "on" | "true" | "yes" | "1" | "enabled" => "on",
        "off" | "false" | "no" | "0" | "disabled" => "off",
        _ => value,
    }
}

fn normalize_settings_theme(value: &str) -> &'static str {
    normalize_theme_name(value).unwrap_or("system")
}

/// Returns `true` when the active terminal is Ptyxis (the new default
/// terminal on Ubuntu 26.04). Used by [`Settings::apply_env_overrides`]
/// to flip `synchronized_output` from `auto` to `off` so DEC mode 2026
/// flicker on Ptyxis 50.x + VTE 0.84.x stops at the source.
///
/// We deliberately keep this narrow:
///
/// - `TERM_PROGRAM` matches `ptyxis` case-insensitively (the value
///   Ptyxis sets when it forwards a process-launch context).
/// - `PTYXIS_VERSION` is set to any non-empty value (the binary's
///   own version probe, present whether or not `TERM_PROGRAM` made it
///   into the child environment).
///
/// Either signal is sufficient. We do *not* trigger on `VTE_VERSION`
/// alone because gnome-terminal 3.58 ships with the same VTE 0.84.x
/// and renders cleanly — broadening the heuristic would regress every
/// gnome-terminal user.
pub fn detected_ptyxis_terminal() -> bool {
    if let Ok(program) = std::env::var("TERM_PROGRAM")
        && program.trim().to_ascii_lowercase().contains("ptyxis")
    {
        return true;
    }
    matches!(std::env::var("PTYXIS_VERSION"), Ok(v) if !v.trim().is_empty())
}

/// Returns `true` for the unmarked Windows console-host path used by plain
/// PowerShell / cmd.exe. Modern Windows terminals set at least one marker that
/// lets us keep the richer rendering path.
pub fn detected_legacy_windows_console_host() -> bool {
    cfg!(windows)
        && legacy_windows_console_host_env([
            std::env::var_os("WT_SESSION").as_deref(),
            std::env::var_os("ConEmuPID").as_deref(),
            std::env::var_os("TERM_PROGRAM").as_deref(),
            std::env::var_os("WEZTERM_EXECUTABLE").as_deref(),
            std::env::var_os("WEZTERM_PANE").as_deref(),
            std::env::var_os("ALACRITTY_WINDOW_ID").as_deref(),
            std::env::var_os("ANSICON").as_deref(),
            std::env::var_os("TERM").as_deref(),
        ])
}

fn legacy_windows_console_host_env(markers: [Option<&std::ffi::OsStr>; 8]) -> bool {
    fn has_value(value: Option<&std::ffi::OsStr>) -> bool {
        value.is_some_and(|v| !v.is_empty())
    }

    markers.into_iter().all(|value| !has_value(value))
}

fn normalize_optional_background_color(value: Option<&str>) -> Option<String> {
    value.and_then(|raw| normalize_background_color_setting(raw).ok().flatten())
}

fn normalize_background_color_setting(value: &str) -> Result<Option<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "default" | "none" | "reset" | "off"
        )
    {
        return Ok(None);
    }

    normalize_hex_rgb_color(trimmed).map(Some).ok_or_else(|| {
        anyhow::anyhow!(
            "Failed to update setting: invalid background_color '{value}'. Expected #RRGGBB, RRGGBB, or default."
        )
    })
}

fn normalize_sidebar_focus(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "pinned" | "visible" | "show" | "on" | "work" | "plan" | "todos" => "pinned",
        "tasks" => "tasks",
        "agents" | "subagents" | "sub-agents" => "agents",
        "context" | "session" => "context",
        "hidden" | "hide" | "closed" | "off" | "none" => "hidden",
        _ => "auto",
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Resolve an environment variable as a boolean. Recognises the
/// common truthy spellings (`1`, `true`, `yes`, `on`) case-
/// insensitively. Used by [`Settings::apply_env_overrides`] for
/// platform a11y signals like `NO_ANIMATIONS`.
fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {}
