//! Color palette and semantic roles for the TUI.
//!
//! This module defines the color system in three layers:
//!
//! 1. **RGB tuples** (`*_RGB` constants) — raw color values used by theme
//!    generation and runtime palette construction.
//! 2. **Semantic `Color` constants** — pre-computed `ratatui::style::Color`
//!    values mapped to UI roles (surface, text, accent, status, mode).
//! 3. **Theme definitions** — complete `UiTheme` structs for each theme.

use ratatui::style::Color;
#[cfg(target_os = "macos")]
use std::process::Command;

// =============================================================================
// Default dark palette (Mimofan) — used as fallback
// =============================================================================
pub const MIMOFAN_BG_RGB: (u8, u8, u8) = (10, 17, 32); // #0A1120 Deep Navy
pub const MIMOFAN_PANEL_RGB: (u8, u8, u8) = (22, 34, 56); // #162238
pub const MIMOFAN_ELEVATED_RGB: (u8, u8, u8) = (36, 52, 78); // #24344E
pub const MIMOFAN_SELECTION_RGB: (u8, u8, u8) = (40, 56, 84); // #283854
pub const MIMOFAN_TEXT_BODY_RGB: (u8, u8, u8) = (246, 242, 232); // #F6F2E8 Mimofan Ivory
pub const MIMOFAN_TEXT_SOFT_RGB: (u8, u8, u8) = (217, 224, 234); // #D9E0EA
pub const MIMOFAN_TEXT_MUTED_RGB: (u8, u8, u8) = (169, 180, 199); // #A9B4C7 Mist Gray
pub const MIMOFAN_TEXT_HINT_RGB: (u8, u8, u8) = (138, 150, 174); // #8A96AE
pub const MIMOFAN_ACCENT_PRIMARY_RGB: (u8, u8, u8) = (246, 196, 83); // #F6C453 Signal Gold
pub const MIMOFAN_ACCENT_SECONDARY_RGB: (u8, u8, u8) = (79, 209, 197); // #4FD1C5 Seafoam
pub const MIMOFAN_ACCENT_ACTION_RGB: (u8, u8, u8) = (255, 122, 89); // #FF7A59 Coral Spark
pub const MIMOFAN_ERROR_RGB: (u8, u8, u8) = (255, 92, 122); // #FF5C7A Rose Red
pub const MIMOFAN_ERROR_HOVER_RGB: (u8, u8, u8) = (255, 120, 144); // #FF7890 Rose Hover
pub const MIMOFAN_ERROR_SURFACE_RGB: (u8, u8, u8) = (42, 18, 26); // #2A121A Error Surface
pub const MIMOFAN_ERROR_BORDER_RGB: (u8, u8, u8) = (255, 138, 160); // #FF8AA0 Error Border
pub const MIMOFAN_ERROR_TEXT_RGB: (u8, u8, u8) = (255, 214, 222); // #FFD6DE Error Text
pub const MIMOFAN_WARNING_RGB: (u8, u8, u8) = (240, 160, 48); // #F0A030
pub const MIMOFAN_SUCCESS_RGB: (u8, u8, u8) = (79, 209, 197); // #4FD1C5 Seafoam
pub const MIMOFAN_INFO_RGB: (u8, u8, u8) = (106, 174, 242); // #6AAEF2 Sky
pub const MIMOFAN_BORDER_RGB: (u8, u8, u8) = (52, 88, 145); // #345891
pub const MIMOFAN_REASONING_TEXT_RGB: (u8, u8, u8) = (224, 153, 72); // #E09948
pub const MIMOFAN_REASONING_SURFACE_RGB: (u8, u8, u8) = (42, 34, 24); // #2A2218
pub const MIMOFAN_REASONING_TINT_RGB: (u8, u8, u8) = (24, 36, 52); // #182434

pub const MIMOFAN_DIFF_ADDED_RGB: (u8, u8, u8) = (87, 199, 133); // #57C785
pub const MIMOFAN_DIFF_ADDED_BG_RGB: (u8, u8, u8) = (18, 42, 34); // #122A22
pub const MIMOFAN_DIFF_DELETED_BG_RGB: (u8, u8, u8) = (42, 18, 26); // #2A121A
pub const MIMOFAN_MODE_AGENT_RGB: (u8, u8, u8) = (80, 150, 255); // #5096FF
pub const MIMOFAN_MODE_YOLO_RGB: (u8, u8, u8) = (255, 100, 100); // #FF6464
pub const MIMOFAN_MODE_PLAN_RGB: (u8, u8, u8) = (246, 196, 83); // #F6C453 Signal Gold
pub const MIMOFAN_MODE_GOAL_RGB: (u8, u8, u8) = (100, 220, 160); // #64DCA0
pub const MIMOFAN_TOOL_LIVE_RGB: (u8, u8, u8) = (140, 190, 238); // #8CBEEE
pub const MIMOFAN_TOOL_ISSUE_RGB: (u8, u8, u8) = (198, 150, 160); // #C696A0
pub const MIMOFAN_TOOL_OUTPUT_RGB: (u8, u8, u8) = (194, 208, 224); // #C2D0E0
pub const MIMOFAN_TOOL_SURFACE_RGB: (u8, u8, u8) = (28, 40, 62); // #1C283E
pub const MIMOFAN_TOOL_ACTIVE_RGB: (u8, u8, u8) = (38, 54, 80); // #263650

// Backward-compatible aliases
pub const DEEPSEEK_SKY_RGB: (u8, u8, u8) = MIMOFAN_INFO_RGB;
pub const DEEPSEEK_INK_RGB: (u8, u8, u8) = MIMOFAN_BG_RGB;
pub const DEEPSEEK_SLATE_RGB: (u8, u8, u8) = MIMOFAN_PANEL_RGB;
pub const DEEPSEEK_RED_RGB: (u8, u8, u8) = MIMOFAN_ERROR_RGB;

// =============================================================================
// Light palette
// =============================================================================
pub const LIGHT_SURFACE_RGB: (u8, u8, u8) = (246, 248, 251); // #F6F8FB
pub const LIGHT_PANEL_RGB: (u8, u8, u8) = (236, 242, 248); // #ECF2F8
pub const LIGHT_ELEVATED_RGB: (u8, u8, u8) = (219, 229, 240); // #DBE5F0
pub const LIGHT_TEXT_BODY_RGB: (u8, u8, u8) = (15, 23, 42); // #0F172A
pub const LIGHT_TEXT_MUTED_RGB: (u8, u8, u8) = (51, 65, 85); // #334155
pub const LIGHT_TEXT_HINT_RGB: (u8, u8, u8) = (100, 116, 139); // #64748B
pub const LIGHT_TEXT_SOFT_RGB: (u8, u8, u8) = (30, 41, 59); // #1E293B
pub const LIGHT_BORDER_RGB: (u8, u8, u8) = (139, 161, 184); // #8BA1B8
pub const LIGHT_SELECTION_RGB: (u8, u8, u8) = (207, 224, 247); // #CFE0F7

// =============================================================================
// Semantic Color constants (using Mimofan palette as default)
// =============================================================================
pub const MIMOFAN_ACCENT_PRIMARY: Color = Color::Rgb(
    MIMOFAN_ACCENT_PRIMARY_RGB.0,
    MIMOFAN_ACCENT_PRIMARY_RGB.1,
    MIMOFAN_ACCENT_PRIMARY_RGB.2,
);
pub const DEEPSEEK_SKY: Color =
    Color::Rgb(DEEPSEEK_SKY_RGB.0, DEEPSEEK_SKY_RGB.1, DEEPSEEK_SKY_RGB.2);
pub const DEEPSEEK_INK: Color =
    Color::Rgb(DEEPSEEK_INK_RGB.0, DEEPSEEK_INK_RGB.1, DEEPSEEK_INK_RGB.2);
pub const DEEPSEEK_SLATE: Color = Color::Rgb(
    DEEPSEEK_SLATE_RGB.0,
    DEEPSEEK_SLATE_RGB.1,
    DEEPSEEK_SLATE_RGB.2,
);
pub const DEEPSEEK_RED: Color =
    Color::Rgb(DEEPSEEK_RED_RGB.0, DEEPSEEK_RED_RGB.1, DEEPSEEK_RED_RGB.2);

pub const LIGHT_SURFACE: Color = Color::Rgb(
    LIGHT_SURFACE_RGB.0,
    LIGHT_SURFACE_RGB.1,
    LIGHT_SURFACE_RGB.2,
);
pub const LIGHT_PANEL: Color = Color::Rgb(LIGHT_PANEL_RGB.0, LIGHT_PANEL_RGB.1, LIGHT_PANEL_RGB.2);
pub const LIGHT_ELEVATED: Color = Color::Rgb(
    LIGHT_ELEVATED_RGB.0,
    LIGHT_ELEVATED_RGB.1,
    LIGHT_ELEVATED_RGB.2,
);
pub const LIGHT_TEXT_BODY: Color = Color::Rgb(
    LIGHT_TEXT_BODY_RGB.0,
    LIGHT_TEXT_BODY_RGB.1,
    LIGHT_TEXT_BODY_RGB.2,
);
pub const LIGHT_TEXT_MUTED: Color = Color::Rgb(
    LIGHT_TEXT_MUTED_RGB.0,
    LIGHT_TEXT_MUTED_RGB.1,
    LIGHT_TEXT_MUTED_RGB.2,
);
pub const LIGHT_TEXT_HINT: Color = Color::Rgb(
    LIGHT_TEXT_HINT_RGB.0,
    LIGHT_TEXT_HINT_RGB.1,
    LIGHT_TEXT_HINT_RGB.2,
);
pub const LIGHT_TEXT_SOFT: Color = Color::Rgb(
    LIGHT_TEXT_SOFT_RGB.0,
    LIGHT_TEXT_SOFT_RGB.1,
    LIGHT_TEXT_SOFT_RGB.2,
);
pub const LIGHT_BORDER: Color =
    Color::Rgb(LIGHT_BORDER_RGB.0, LIGHT_BORDER_RGB.1, LIGHT_BORDER_RGB.2);
pub const LIGHT_SELECTION_BG: Color = Color::Rgb(
    LIGHT_SELECTION_RGB.0,
    LIGHT_SELECTION_RGB.1,
    LIGHT_SELECTION_RGB.2,
);

pub const TEXT_BODY: Color = Color::Rgb(
    MIMOFAN_TEXT_BODY_RGB.0,
    MIMOFAN_TEXT_BODY_RGB.1,
    MIMOFAN_TEXT_BODY_RGB.2,
);
pub const TEXT_SECONDARY: Color = Color::Rgb(
    MIMOFAN_TEXT_MUTED_RGB.0,
    MIMOFAN_TEXT_MUTED_RGB.1,
    MIMOFAN_TEXT_MUTED_RGB.2,
);
pub const TEXT_HINT: Color = Color::Rgb(
    MIMOFAN_TEXT_HINT_RGB.0,
    MIMOFAN_TEXT_HINT_RGB.1,
    MIMOFAN_TEXT_HINT_RGB.2,
);
pub const TEXT_ACCENT: Color = Color::Rgb(
    MIMOFAN_ACCENT_SECONDARY_RGB.0,
    MIMOFAN_ACCENT_SECONDARY_RGB.1,
    MIMOFAN_ACCENT_SECONDARY_RGB.2,
);
pub const SELECTION_TEXT: Color = Color::Rgb(
    MIMOFAN_TEXT_BODY_RGB.0,
    MIMOFAN_TEXT_BODY_RGB.1,
    MIMOFAN_TEXT_BODY_RGB.2,
);
pub const TEXT_SOFT: Color = Color::Rgb(
    MIMOFAN_TEXT_SOFT_RGB.0,
    MIMOFAN_TEXT_SOFT_RGB.1,
    MIMOFAN_TEXT_SOFT_RGB.2,
);
pub const TEXT_REASONING: Color = Color::Rgb(
    MIMOFAN_REASONING_TEXT_RGB.0,
    MIMOFAN_REASONING_TEXT_RGB.1,
    MIMOFAN_REASONING_TEXT_RGB.2,
);

// Compatibility aliases
pub const TEXT_PRIMARY: Color = TEXT_BODY;
pub const TEXT_MUTED: Color = TEXT_SECONDARY;
pub const TEXT_DIM: Color = TEXT_HINT;
pub const USER_BODY: Color = Color::Rgb(74, 222, 128); // #4ADE80 green
pub const LIGHT_USER_BODY: Color = Color::Rgb(21, 128, 61); // #15803D green

// New semantic colors for UI theming
pub const BORDER_COLOR_RGB: (u8, u8, u8) = MIMOFAN_BORDER_RGB;
pub const BORDER_COLOR: Color =
    Color::Rgb(BORDER_COLOR_RGB.0, BORDER_COLOR_RGB.1, BORDER_COLOR_RGB.2);
pub const BACKGROUND_DARK: Color = Color::Rgb(MIMOFAN_BG_RGB.0, MIMOFAN_BG_RGB.1, MIMOFAN_BG_RGB.2);
pub const SURFACE_PANEL: Color = Color::Rgb(
    MIMOFAN_PANEL_RGB.0,
    MIMOFAN_PANEL_RGB.1,
    MIMOFAN_PANEL_RGB.2,
);
pub const SURFACE_ELEVATED: Color = Color::Rgb(
    MIMOFAN_ELEVATED_RGB.0,
    MIMOFAN_ELEVATED_RGB.1,
    MIMOFAN_ELEVATED_RGB.2,
);
pub const SURFACE_REASONING: Color = Color::Rgb(
    MIMOFAN_REASONING_SURFACE_RGB.0,
    MIMOFAN_REASONING_SURFACE_RGB.1,
    MIMOFAN_REASONING_SURFACE_RGB.2,
);
pub const SURFACE_REASONING_TINT: Color = Color::Rgb(
    MIMOFAN_REASONING_TINT_RGB.0,
    MIMOFAN_REASONING_TINT_RGB.1,
    MIMOFAN_REASONING_TINT_RGB.2,
);
pub const SURFACE_REASONING_ACTIVE: Color = Color::Rgb(58, 46, 32);
pub const SURFACE_TOOL: Color = Color::Rgb(
    MIMOFAN_TOOL_SURFACE_RGB.0,
    MIMOFAN_TOOL_SURFACE_RGB.1,
    MIMOFAN_TOOL_SURFACE_RGB.2,
);
pub const SURFACE_TOOL_ACTIVE: Color = Color::Rgb(
    MIMOFAN_TOOL_ACTIVE_RGB.0,
    MIMOFAN_TOOL_ACTIVE_RGB.1,
    MIMOFAN_TOOL_ACTIVE_RGB.2,
);
pub const SURFACE_SUCCESS: Color = Color::Rgb(18, 42, 37);
pub const SURFACE_ERROR: Color = Color::Rgb(
    MIMOFAN_ERROR_SURFACE_RGB.0,
    MIMOFAN_ERROR_SURFACE_RGB.1,
    MIMOFAN_ERROR_SURFACE_RGB.2,
);
pub const DIFF_ADDED_BG: Color = Color::Rgb(
    MIMOFAN_DIFF_ADDED_BG_RGB.0,
    MIMOFAN_DIFF_ADDED_BG_RGB.1,
    MIMOFAN_DIFF_ADDED_BG_RGB.2,
);
pub const DIFF_DELETED_BG: Color = Color::Rgb(
    MIMOFAN_DIFF_DELETED_BG_RGB.0,
    MIMOFAN_DIFF_DELETED_BG_RGB.1,
    MIMOFAN_DIFF_DELETED_BG_RGB.2,
);
pub const DIFF_ADDED: Color = Color::Rgb(
    MIMOFAN_DIFF_ADDED_RGB.0,
    MIMOFAN_DIFF_ADDED_RGB.1,
    MIMOFAN_DIFF_ADDED_RGB.2,
);
pub const ACCENT_REASONING_LIVE: Color = Color::Rgb(
    MIMOFAN_REASONING_TEXT_RGB.0,
    MIMOFAN_REASONING_TEXT_RGB.1,
    MIMOFAN_REASONING_TEXT_RGB.2,
);
pub const ACCENT_TOOL_LIVE: Color = Color::Rgb(
    MIMOFAN_TOOL_LIVE_RGB.0,
    MIMOFAN_TOOL_LIVE_RGB.1,
    MIMOFAN_TOOL_LIVE_RGB.2,
);
pub const ACCENT_TOOL_ISSUE: Color = Color::Rgb(
    MIMOFAN_TOOL_ISSUE_RGB.0,
    MIMOFAN_TOOL_ISSUE_RGB.1,
    MIMOFAN_TOOL_ISSUE_RGB.2,
);
pub const TEXT_TOOL_OUTPUT: Color = Color::Rgb(
    MIMOFAN_TOOL_OUTPUT_RGB.0,
    MIMOFAN_TOOL_OUTPUT_RGB.1,
    MIMOFAN_TOOL_OUTPUT_RGB.2,
);

// Legacy status colors
pub const STATUS_SUCCESS: Color = Color::Rgb(
    MIMOFAN_SUCCESS_RGB.0,
    MIMOFAN_SUCCESS_RGB.1,
    MIMOFAN_SUCCESS_RGB.2,
);
pub const STATUS_WARNING: Color = Color::Rgb(
    MIMOFAN_WARNING_RGB.0,
    MIMOFAN_WARNING_RGB.1,
    MIMOFAN_WARNING_RGB.2,
);
pub const STATUS_ERROR: Color = Color::Rgb(
    MIMOFAN_ERROR_RGB.0,
    MIMOFAN_ERROR_RGB.1,
    MIMOFAN_ERROR_RGB.2,
);
pub const STATUS_INFO: Color =
    Color::Rgb(MIMOFAN_INFO_RGB.0, MIMOFAN_INFO_RGB.1, MIMOFAN_INFO_RGB.2);

// Mode-specific accent colors
pub const MODE_AGENT: Color = Color::Rgb(
    MIMOFAN_MODE_AGENT_RGB.0,
    MIMOFAN_MODE_AGENT_RGB.1,
    MIMOFAN_MODE_AGENT_RGB.2,
);
pub const MODE_YOLO: Color = Color::Rgb(
    MIMOFAN_MODE_YOLO_RGB.0,
    MIMOFAN_MODE_YOLO_RGB.1,
    MIMOFAN_MODE_YOLO_RGB.2,
);
pub const MODE_PLAN: Color = Color::Rgb(
    MIMOFAN_MODE_PLAN_RGB.0,
    MIMOFAN_MODE_PLAN_RGB.1,
    MIMOFAN_MODE_PLAN_RGB.2,
);
pub const MODE_GOAL: Color = Color::Rgb(
    MIMOFAN_MODE_GOAL_RGB.0,
    MIMOFAN_MODE_GOAL_RGB.1,
    MIMOFAN_MODE_GOAL_RGB.2,
);

pub const SELECTION_BG: Color = Color::Rgb(
    MIMOFAN_SELECTION_RGB.0,
    MIMOFAN_SELECTION_RGB.1,
    MIMOFAN_SELECTION_RGB.2,
);
pub const COMPOSER_BG: Color = DEEPSEEK_SLATE;

// =============================================================================
// PaletteMode enum
// =============================================================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    Dark,
    Light,
}

impl PaletteMode {
    #[must_use]
    pub fn from_colorfgbg(value: &str) -> Option<Self> {
        let bg = value
            .split(';')
            .rev()
            .find_map(|part| part.parse::<u16>().ok())?;
        Some(if bg >= 8 { Self::Light } else { Self::Dark })
    }

    #[must_use]
    pub fn detect() -> Self {
        Self::detect_from_sources(
            std::env::var("COLORFGBG").ok().as_deref(),
            detect_macos_palette_mode(),
        )
    }

    #[must_use]
    fn detect_from_sources(colorfgbg: Option<&str>, macos_fallback: Option<Self>) -> Self {
        colorfgbg
            .and_then(Self::from_colorfgbg)
            .or(macos_fallback)
            .unwrap_or(Self::Dark)
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_palette_mode() -> Option<PaletteMode> {
    let output = Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()?;

    if output.status.success() {
        Some(palette_mode_from_apple_interface_style(
            &String::from_utf8_lossy(&output.stdout),
        ))
    } else {
        // Command failed — likely means Light mode (no AppleInterfaceStyle key)
        Some(PaletteMode::Light)
    }
}

#[cfg(not(target_os = "macos"))]
fn detect_macos_palette_mode() -> Option<PaletteMode> {
    None
}

#[cfg(target_os = "macos")]
fn palette_mode_from_apple_interface_style(style: &str) -> PaletteMode {
    if style.trim().eq_ignore_ascii_case("Dark") {
        PaletteMode::Dark
    } else {
        PaletteMode::Light
    }
}

// =============================================================================
// UiTheme struct
// =============================================================================
#[derive(Debug, Clone, Copy)]
pub struct UiTheme {
    pub name: &'static str,
    pub mode: PaletteMode,
    // Surface hierarchy
    pub surface_bg: Color,
    pub panel_bg: Color,
    pub elevated_bg: Color,
    pub composer_bg: Color,
    pub selection_bg: Color,
    pub header_bg: Color,
    pub footer_bg: Color,
    // Text hierarchy
    pub text_dim: Color,
    pub text_hint: Color,
    pub text_muted: Color,
    pub text_body: Color,
    pub text_soft: Color,
    // Border
    pub border: Color,
    // Accents
    pub accent_primary: Color,
    pub accent_secondary: Color,
    pub accent_action: Color,
    // Error / destructive
    pub error_fg: Color,
    pub error_hover: Color,
    pub error_surface: Color,
    pub error_border: Color,
    pub error_text: Color,
    // Status
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    // Mode badges
    pub mode_agent: Color,
    pub mode_yolo: Color,
    pub mode_plan: Color,
    pub mode_goal: Color,
    // Footer statusline colors
    pub status_ready: Color,
    pub status_working: Color,
    pub status_warning: Color,
    // Diff colors
    pub diff_added_fg: Color,
    pub diff_deleted_fg: Color,
    pub diff_added_bg: Color,
    pub diff_deleted_bg: Color,
    // Tool cell colors
    pub tool_running: Color,
    pub tool_success: Color,
    pub tool_failed: Color,
}

// =============================================================================
// Theme: Mimofan (Default Dark)
// =============================================================================
pub const UI_THEME: UiTheme = UiTheme {
    name: "whale",
    mode: PaletteMode::Dark,
    surface_bg: DEEPSEEK_INK,
    panel_bg: DEEPSEEK_SLATE,
    elevated_bg: SURFACE_ELEVATED,
    composer_bg: DEEPSEEK_SLATE,
    selection_bg: SELECTION_BG,
    header_bg: DEEPSEEK_INK,
    footer_bg: DEEPSEEK_INK,
    text_dim: TEXT_DIM,
    text_hint: TEXT_HINT,
    text_muted: TEXT_MUTED,
    text_body: TEXT_BODY,
    text_soft: TEXT_SOFT,
    border: BORDER_COLOR,
    accent_primary: Color::Rgb(
        MIMOFAN_ACCENT_PRIMARY_RGB.0,
        MIMOFAN_ACCENT_PRIMARY_RGB.1,
        MIMOFAN_ACCENT_PRIMARY_RGB.2,
    ),
    accent_secondary: Color::Rgb(
        MIMOFAN_ACCENT_SECONDARY_RGB.0,
        MIMOFAN_ACCENT_SECONDARY_RGB.1,
        MIMOFAN_ACCENT_SECONDARY_RGB.2,
    ),
    accent_action: Color::Rgb(
        MIMOFAN_ACCENT_ACTION_RGB.0,
        MIMOFAN_ACCENT_ACTION_RGB.1,
        MIMOFAN_ACCENT_ACTION_RGB.2,
    ),
    error_fg: Color::Rgb(
        MIMOFAN_ERROR_RGB.0,
        MIMOFAN_ERROR_RGB.1,
        MIMOFAN_ERROR_RGB.2,
    ),
    error_hover: Color::Rgb(
        MIMOFAN_ERROR_HOVER_RGB.0,
        MIMOFAN_ERROR_HOVER_RGB.1,
        MIMOFAN_ERROR_HOVER_RGB.2,
    ),
    error_surface: Color::Rgb(
        MIMOFAN_ERROR_SURFACE_RGB.0,
        MIMOFAN_ERROR_SURFACE_RGB.1,
        MIMOFAN_ERROR_SURFACE_RGB.2,
    ),
    error_border: Color::Rgb(
        MIMOFAN_ERROR_BORDER_RGB.0,
        MIMOFAN_ERROR_BORDER_RGB.1,
        MIMOFAN_ERROR_BORDER_RGB.2,
    ),
    error_text: Color::Rgb(
        MIMOFAN_ERROR_TEXT_RGB.0,
        MIMOFAN_ERROR_TEXT_RGB.1,
        MIMOFAN_ERROR_TEXT_RGB.2,
    ),
    warning: Color::Rgb(
        MIMOFAN_WARNING_RGB.0,
        MIMOFAN_WARNING_RGB.1,
        MIMOFAN_WARNING_RGB.2,
    ),
    success: Color::Rgb(
        MIMOFAN_SUCCESS_RGB.0,
        MIMOFAN_SUCCESS_RGB.1,
        MIMOFAN_SUCCESS_RGB.2,
    ),
    info: Color::Rgb(MIMOFAN_INFO_RGB.0, MIMOFAN_INFO_RGB.1, MIMOFAN_INFO_RGB.2),
    mode_agent: MODE_AGENT,
    mode_yolo: MODE_YOLO,
    mode_plan: MODE_PLAN,
    mode_goal: MODE_GOAL,
    status_ready: TEXT_MUTED,
    status_working: DEEPSEEK_SKY,
    status_warning: STATUS_WARNING,
    diff_added_fg: DIFF_ADDED,
    diff_deleted_fg: Color::Rgb(
        MIMOFAN_ERROR_RGB.0,
        MIMOFAN_ERROR_RGB.1,
        MIMOFAN_ERROR_RGB.2,
    ),
    diff_added_bg: DIFF_ADDED_BG,
    diff_deleted_bg: DIFF_DELETED_BG,
    tool_running: ACCENT_TOOL_LIVE,
    tool_success: TEXT_DIM,
    tool_failed: ACCENT_TOOL_ISSUE,
};

// =============================================================================
// Theme: Mimofan Light
// =============================================================================
pub const LIGHT_UI_THEME: UiTheme = UiTheme {
    name: "whale-light",
    mode: PaletteMode::Light,
    surface_bg: LIGHT_SURFACE,
    panel_bg: LIGHT_PANEL,
    elevated_bg: LIGHT_ELEVATED,
    composer_bg: LIGHT_PANEL,
    selection_bg: LIGHT_SELECTION_BG,
    header_bg: LIGHT_SURFACE,
    footer_bg: LIGHT_SURFACE,
    text_dim: LIGHT_TEXT_HINT,
    text_hint: LIGHT_TEXT_HINT,
    text_muted: LIGHT_TEXT_MUTED,
    text_body: LIGHT_TEXT_BODY,
    text_soft: LIGHT_TEXT_SOFT,
    border: LIGHT_BORDER,
    accent_primary: Color::Rgb(53, 120, 229),   // blue
    accent_secondary: Color::Rgb(79, 180, 160), // teal
    accent_action: Color::Rgb(220, 90, 60),     // warm coral
    error_fg: Color::Rgb(200, 40, 60),          // red
    error_hover: Color::Rgb(220, 70, 85),
    error_surface: Color::Rgb(254, 229, 229),
    error_border: Color::Rgb(240, 120, 130),
    error_text: Color::Rgb(120, 20, 30),
    warning: Color::Rgb(180, 83, 9),      // amber
    success: Color::Rgb(21, 128, 61),     // green
    info: Color::Rgb(53, 120, 229),       // blue
    mode_agent: Color::Rgb(53, 120, 229), // blue
    mode_yolo: Color::Rgb(200, 40, 60),   // red
    mode_plan: Color::Rgb(180, 83, 9),    // amber
    mode_goal: Color::Rgb(80, 180, 130),  // mint green
    status_ready: LIGHT_TEXT_MUTED,
    status_working: Color::Rgb(53, 120, 229),   // blue
    status_warning: Color::Rgb(180, 83, 9),     // amber
    diff_added_fg: Color::Rgb(22, 101, 52),     // green
    diff_deleted_fg: Color::Rgb(200, 40, 60),   // red
    diff_added_bg: Color::Rgb(223, 247, 231),   // light green
    diff_deleted_bg: Color::Rgb(254, 229, 229), // light red
    tool_running: Color::Rgb(53, 120, 229),     // blue
    tool_success: LIGHT_TEXT_HINT,
    tool_failed: Color::Rgb(200, 40, 60), // red
};

// =============================================================================
// Theme: Ember
// =============================================================================
pub const EMBER_UI_THEME: UiTheme = UiTheme {
    name: "ember",
    mode: PaletteMode::Dark,
    surface_bg: Color::Rgb(0x18, 0x17, 0x15),
    panel_bg: Color::Rgb(0x25, 0x23, 0x20),
    elevated_bg: Color::Rgb(0x1f, 0x1e, 0x1b),
    composer_bg: Color::Rgb(0x25, 0x23, 0x20),
    selection_bg: Color::Rgb(0x30, 0x2d, 0x28),
    header_bg: Color::Rgb(0x18, 0x17, 0x15),
    footer_bg: Color::Rgb(0x18, 0x17, 0x15),
    text_dim: Color::Rgb(0x72, 0x70, 0x6a),
    text_hint: Color::Rgb(0x7d, 0x7a, 0x73),
    text_muted: Color::Rgb(0xa0, 0x9d, 0x96),
    text_body: Color::Rgb(0xfa, 0xf9, 0xf5),
    text_soft: Color::Rgb(0xd0, 0xcd, 0xc5),
    border: Color::Rgb(0x30, 0x2d, 0x28),
    accent_primary: Color::Rgb(0xcc, 0x78, 0x5c), // coral
    accent_secondary: Color::Rgb(0x5d, 0xb8, 0xa6), // teal
    accent_action: Color::Rgb(0xe8, 0xa5, 0x5a),  // amber
    error_fg: Color::Rgb(0xe0, 0x60, 0x60),
    error_hover: Color::Rgb(0xd9, 0x66, 0x66),
    error_surface: Color::Rgb(0x2a, 0x1c, 0x1c),
    error_border: Color::Rgb(0xe0, 0x60, 0x60),
    error_text: Color::Rgb(0xe8, 0xb8, 0xb8),
    warning: Color::Rgb(0xd4, 0xa0, 0x17),
    success: Color::Rgb(0x5d, 0xb8, 0x72),
    info: Color::Rgb(0x5d, 0xb8, 0xa6),
    mode_agent: Color::Rgb(0xcc, 0x78, 0x5c),
    mode_yolo: Color::Rgb(0xc6, 0x45, 0x45),
    mode_plan: Color::Rgb(0xe8, 0xa5, 0x5a),
    mode_goal: Color::Rgb(0x5d, 0xb8, 0x72),
    status_ready: Color::Rgb(0xa0, 0x9d, 0x96),
    status_working: Color::Rgb(0x5d, 0xb8, 0xa6),
    status_warning: Color::Rgb(0xd4, 0xa0, 0x17),
    diff_added_fg: Color::Rgb(0x5d, 0xb8, 0x72),
    diff_deleted_fg: Color::Rgb(0xc6, 0x45, 0x45),
    diff_added_bg: Color::Rgb(0x1a, 0x24, 0x1d),
    diff_deleted_bg: Color::Rgb(0x24, 0x1a, 0x1a),
    tool_running: Color::Rgb(0x5d, 0xb8, 0xa6),
    tool_success: Color::Rgb(0xa0, 0x9d, 0x96),
    tool_failed: Color::Rgb(0xc6, 0x45, 0x45),
};

// =============================================================================
// Theme: Cosmic (Futuristic)
// =============================================================================
pub const COSMIC_UI_THEME: UiTheme = UiTheme {
    name: "cosmic",
    mode: PaletteMode::Dark,
    // Deep space backgrounds
    surface_bg: Color::Rgb(0x0a, 0x0a, 0x14), // #0A0A14 Void Black
    panel_bg: Color::Rgb(0x12, 0x12, 0x1f),   // #12121F Nebula Dark
    elevated_bg: Color::Rgb(0x1a, 0x1a, 0x2e), // #1A1A2E Cosmic Navy
    composer_bg: Color::Rgb(0x12, 0x12, 0x1f),
    selection_bg: Color::Rgb(0x25, 0x25, 0x3d), // #25253D Stardust
    header_bg: Color::Rgb(0x0a, 0x0a, 0x14),
    footer_bg: Color::Rgb(0x0a, 0x0a, 0x14),
    // Ethereal text
    text_dim: Color::Rgb(0x4a, 0x4a, 0x6a),   // #4A4A6A Ghost
    text_hint: Color::Rgb(0x6a, 0x6a, 0x8a),  // #6A6A8A Mist
    text_muted: Color::Rgb(0x9a, 0x9a, 0xba), // #9A9ABA Soft Light
    text_body: Color::Rgb(0xe0, 0xe0, 0xf0),  // #E0E0F0 Moonlight
    text_soft: Color::Rgb(0xc0, 0xc0, 0xe0),  // #C0C0E0 Starlight
    border: Color::Rgb(0x2a, 0x2a, 0x4a),     // #2A2A4A Horizon
    // Neon accents
    accent_primary: Color::Rgb(0x00, 0xd4, 0xff), // #00D4FF Cyan Neon
    accent_secondary: Color::Rgb(0xb4, 0x00, 0xff), // #B400FF Purple Neon
    accent_action: Color::Rgb(0x00, 0xff, 0x88),  // #00FF88 Green Neon
    // Error states
    error_fg: Color::Rgb(0xff, 0x33, 0x66), // #FF3366 Neon Red
    error_hover: Color::Rgb(0xff, 0x55, 0x88),
    error_surface: Color::Rgb(0x2a, 0x0a, 0x14),
    error_border: Color::Rgb(0xff, 0x33, 0x66),
    error_text: Color::Rgb(0xff, 0xaa, 0xcc),
    // Status colors
    warning: Color::Rgb(0xff, 0xaa, 0x00), // #FFAA00 Amber Neon
    success: Color::Rgb(0x00, 0xff, 0x88), // #00FF88 Green Neon
    info: Color::Rgb(0x00, 0xd4, 0xff),    // #00D4FF Cyan Neon
    // Mode badges
    mode_agent: Color::Rgb(0x00, 0xd4, 0xff), // Cyan
    mode_yolo: Color::Rgb(0xff, 0x33, 0x66),  // Red
    mode_plan: Color::Rgb(0xff, 0xaa, 0x00),  // Amber
    mode_goal: Color::Rgb(0x00, 0xff, 0x88),  // Green
    // Footer statusline
    status_ready: Color::Rgb(0x6a, 0x6a, 0x8a),
    status_working: Color::Rgb(0x00, 0xd4, 0xff),
    status_warning: Color::Rgb(0xff, 0xaa, 0x00),
    // Diff colors
    diff_added_fg: Color::Rgb(0x00, 0xff, 0x88),
    diff_deleted_fg: Color::Rgb(0xff, 0x33, 0x66),
    diff_added_bg: Color::Rgb(0x0a, 0x1a, 0x14),
    diff_deleted_bg: Color::Rgb(0x2a, 0x0a, 0x14),
    // Tool cells
    tool_running: Color::Rgb(0x00, 0xd4, 0xff),
    tool_success: Color::Rgb(0x6a, 0x6a, 0x8a),
    tool_failed: Color::Rgb(0xff, 0x33, 0x66),
};

// =============================================================================
// Theme: Handwritten (Warm/Paper-like)
// =============================================================================
pub const HANDWRITTEN_UI_THEME: UiTheme = UiTheme {
    name: "handwritten",
    mode: PaletteMode::Light,
    // Warm paper backgrounds
    surface_bg: Color::Rgb(0xf5, 0xf0, 0xe6), // #F5F0E6 Parchment
    panel_bg: Color::Rgb(0xeb, 0xe4, 0xd6),   // #EBE4D6 Aged Paper
    elevated_bg: Color::Rgb(0xe0, 0xd8, 0xc8), // #E0D8C8 Cream
    composer_bg: Color::Rgb(0xeb, 0xe4, 0xd6),
    selection_bg: Color::Rgb(0xd4, 0xcc, 0xb8), // #D4CCB8 Tan
    header_bg: Color::Rgb(0xf5, 0xf0, 0xe6),
    footer_bg: Color::Rgb(0xf5, 0xf0, 0xe6),
    // Ink-like text
    text_dim: Color::Rgb(0x9a, 0x90, 0x80), // #9A9080 Faded Ink
    text_hint: Color::Rgb(0x7a, 0x70, 0x60), // #7A7060 Light Ink
    text_muted: Color::Rgb(0x5a, 0x50, 0x40), // #5A5040 Medium Ink
    text_body: Color::Rgb(0x2a, 0x25, 0x20), // #2A2520 Dark Ink
    text_soft: Color::Rgb(0x3a, 0x35, 0x30), // #3A3530 Soft Ink
    border: Color::Rgb(0xc0, 0xb8, 0xa4),   // #C0B8A4 Ruled Line
    // Warm accents
    accent_primary: Color::Rgb(0xb0, 0x5a, 0x2a), // #B05A2A Burnt Sienna
    accent_secondary: Color::Rgb(0x4a, 0x7a, 0x5a), // #4A7A5A Sage Green
    accent_action: Color::Rgb(0xc0, 0x7a, 0x2a),  // #C07A2A Ochre
    // Error states
    error_fg: Color::Rgb(0xa0, 0x30, 0x30), // #A03030 Rust Red
    error_hover: Color::Rgb(0xb0, 0x40, 0x40),
    error_surface: Color::Rgb(0xe8, 0xd8, 0xd0),
    error_border: Color::Rgb(0xa0, 0x30, 0x30),
    error_text: Color::Rgb(0x60, 0x20, 0x20),
    // Status colors
    warning: Color::Rgb(0xc0, 0x8a, 0x2a), // #C08A2A Amber
    success: Color::Rgb(0x4a, 0x7a, 0x5a), // #4A7A5A Sage
    info: Color::Rgb(0x4a, 0x6a, 0x8a),    // #4A6A8A Slate Blue
    // Mode badges
    mode_agent: Color::Rgb(0xb0, 0x5a, 0x2a), // Burnt Sienna
    mode_yolo: Color::Rgb(0xa0, 0x30, 0x30),  // Rust Red
    mode_plan: Color::Rgb(0xc0, 0x8a, 0x2a),  // Amber
    mode_goal: Color::Rgb(0x4a, 0x7a, 0x5a),  // Sage
    // Footer statusline
    status_ready: Color::Rgb(0x7a, 0x70, 0x60),
    status_working: Color::Rgb(0x4a, 0x6a, 0x8a),
    status_warning: Color::Rgb(0xc0, 0x8a, 0x2a),
    // Diff colors
    diff_added_fg: Color::Rgb(0x4a, 0x7a, 0x5a),
    diff_deleted_fg: Color::Rgb(0xa0, 0x30, 0x30),
    diff_added_bg: Color::Rgb(0xd8, 0xe8, 0xd0),
    diff_deleted_bg: Color::Rgb(0xe8, 0xd8, 0xd0),
    // Tool cells
    tool_running: Color::Rgb(0x4a, 0x6a, 0x8a),
    tool_success: Color::Rgb(0x7a, 0x70, 0x60),
    tool_failed: Color::Rgb(0xa0, 0x30, 0x30),
};

// =============================================================================
// Theme: Crush (Berry/Pink)
// =============================================================================
pub const CRUSH_UI_THEME: UiTheme = UiTheme {
    name: "crush",
    mode: PaletteMode::Dark,
    // Deep berry backgrounds
    surface_bg: Color::Rgb(0x1a, 0x0a, 0x14), // #1A0A14 Deep Berry
    panel_bg: Color::Rgb(0x24, 0x12, 0x1c),   // #24121C Blackberry
    elevated_bg: Color::Rgb(0x2e, 0x1a, 0x26), // #2E1A26 Plum
    composer_bg: Color::Rgb(0x24, 0x12, 0x1c),
    selection_bg: Color::Rgb(0x3d, 0x24, 0x34), // #3D2434 Mauve
    header_bg: Color::Rgb(0x1a, 0x0a, 0x14),
    footer_bg: Color::Rgb(0x1a, 0x0a, 0x14),
    // Soft text
    text_dim: Color::Rgb(0x6a, 0x4a, 0x5a), // #6A4A5A Muted Berry
    text_hint: Color::Rgb(0x8a, 0x6a, 0x7a), // #8A6A7A Light Berry
    text_muted: Color::Rgb(0xba, 0x9a, 0xaa), // #BA9AAA Soft Pink
    text_body: Color::Rgb(0xf0, 0xe0, 0xea), // #F0E0EA Rose White
    text_soft: Color::Rgb(0xd0, 0xc0, 0xca), // #D0C0CA Blush
    border: Color::Rgb(0x3d, 0x24, 0x34),   // #3D2434 Mauve
    // Berry accents
    accent_primary: Color::Rgb(0xff, 0x40, 0x81), // #FF4081 Crush Pink
    accent_secondary: Color::Rgb(0xe0, 0x40, 0xa0), // #E040A0 Magenta
    accent_action: Color::Rgb(0xff, 0x70, 0xb0),  // #FF70B0 Light Pink
    // Error states
    error_fg: Color::Rgb(0xff, 0x30, 0x50), // #FF3050 Bright Red
    error_hover: Color::Rgb(0xff, 0x50, 0x70),
    error_surface: Color::Rgb(0x3a, 0x1a, 0x24),
    error_border: Color::Rgb(0xff, 0x30, 0x50),
    error_text: Color::Rgb(0xff, 0xaa, 0xba),
    // Status colors
    warning: Color::Rgb(0xff, 0xb0, 0x40), // #FFB040 Warm Amber
    success: Color::Rgb(0x80, 0xc0, 0x80), // #80C080 Soft Green
    info: Color::Rgb(0x80, 0xa0, 0xe0),    // #80A0E0 Soft Blue
    // Mode badges
    mode_agent: Color::Rgb(0xff, 0x40, 0x81), // Crush Pink
    mode_yolo: Color::Rgb(0xff, 0x30, 0x50),  // Bright Red
    mode_plan: Color::Rgb(0xff, 0xb0, 0x40),  // Amber
    mode_goal: Color::Rgb(0x80, 0xc0, 0x80),  // Soft Green
    // Footer statusline
    status_ready: Color::Rgb(0x8a, 0x6a, 0x7a),
    status_working: Color::Rgb(0xff, 0x40, 0x81),
    status_warning: Color::Rgb(0xff, 0xb0, 0x40),
    // Diff colors
    diff_added_fg: Color::Rgb(0x80, 0xc0, 0x80),
    diff_deleted_fg: Color::Rgb(0xff, 0x30, 0x50),
    diff_added_bg: Color::Rgb(0x1a, 0x2a, 0x1a),
    diff_deleted_bg: Color::Rgb(0x3a, 0x1a, 0x24),
    // Tool cells
    tool_running: Color::Rgb(0xff, 0x40, 0x81),
    tool_success: Color::Rgb(0x8a, 0x6a, 0x7a),
    tool_failed: Color::Rgb(0xff, 0x30, 0x50),
};

// =============================================================================
// ThemeId enum
// =============================================================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    System,
    Terminal,
    Mimofan,
    MimofanLight,
    Ember,
    Cosmic,
    Handwritten,
    Crush,
}

impl ThemeId {
    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        match normalize_theme_name(value)? {
            "system" => Some(Self::System),
            "terminal" => Some(Self::Terminal),
            "dark" | "mimofan" => Some(Self::Mimofan),
            "light" | "mimofan-light" => Some(Self::MimofanLight),
            "ember" => Some(Self::Ember),
            "cosmic" => Some(Self::Cosmic),
            "handwritten" => Some(Self::Handwritten),
            "crush" => Some(Self::Crush),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Terminal => "terminal",
            Self::Mimofan => "dark",
            Self::MimofanLight => "light",
            Self::Ember => "ember",
            Self::Cosmic => "cosmic",
            Self::Handwritten => "handwritten",
            Self::Crush => "crush",
        }
    }

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Terminal => "Terminal",
            Self::Mimofan => "Mimofan (Dark)",
            Self::MimofanLight => "Mimofan Light",
            Self::Ember => "Ember",
            Self::Cosmic => "Cosmic",
            Self::Handwritten => "Handwritten",
            Self::Crush => "Crush",
        }
    }

    #[must_use]
    pub const fn tagline(self) -> &'static str {
        match self {
            Self::System => "Follow terminal background",
            Self::Terminal => "Inherit terminal colors",
            Self::Mimofan => "Deep navy & gold",
            Self::MimofanLight => "Paper-ish light theme",
            Self::Ember => "Warm navy & coral",
            Self::Cosmic => "Futuristic neon on cosmic dark",
            Self::Handwritten => "Warm paper with ink-like text",
            Self::Crush => "Berry pink & romantic",
        }
    }

    #[must_use]
    pub fn ui_theme(self) -> UiTheme {
        match self {
            Self::System => UiTheme::detect(),
            Self::Terminal => UI_THEME, // Terminal uses default with transparent surfaces
            Self::Mimofan => UI_THEME,
            Self::MimofanLight => LIGHT_UI_THEME,
            Self::Ember => EMBER_UI_THEME,
            Self::Cosmic => COSMIC_UI_THEME,
            Self::Handwritten => HANDWRITTEN_UI_THEME,
            Self::Crush => CRUSH_UI_THEME,
        }
    }
}

/// Themes shown in the `/theme` picker, in display order.
pub const SELECTABLE_THEMES: &[ThemeId] = &[
    ThemeId::System,
    ThemeId::Terminal,
    ThemeId::Mimofan,
    ThemeId::MimofanLight,
    ThemeId::Ember,
    ThemeId::Cosmic,
    ThemeId::Handwritten,
    ThemeId::Crush,
];

impl UiTheme {
    #[must_use]
    pub fn for_mode(mode: PaletteMode) -> Self {
        match mode {
            PaletteMode::Dark => UI_THEME,
            PaletteMode::Light => LIGHT_UI_THEME,
        }
    }

    #[must_use]
    pub fn detect() -> Self {
        Self::for_mode(PaletteMode::detect())
    }

    #[must_use]
    pub fn from_setting(value: &str) -> Option<Self> {
        ThemeId::from_name(value).map(ThemeId::ui_theme)
    }

    #[must_use]
    pub fn with_background_color(mut self, color: Color) -> Self {
        self.surface_bg = color;
        self.header_bg = color;
        self.footer_bg = color;
        self
    }
}

#[must_use]
pub fn normalize_theme_name(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "auto" | "system" | "default" => Some("system"),
        "terminal" | "term" | "transparent" | "follow-terminal" | "inherit" => Some("terminal"),
        "dark" | "mimofan" | "mimofan-dark" => Some("dark"),
        "light" | "mimofan-light" => Some("light"),
        "ember" => Some("ember"),
        "cosmic" | "neon" | "futuristic" => Some("cosmic"),
        "handwritten" | "hand-written" | "paper" | "ink" => Some("handwritten"),
        "crush" | "berry" | "pink" => Some("crush"),
        _ => None,
    }
}

#[must_use]
pub fn theme_label_for_mode(mode: PaletteMode) -> &'static str {
    match mode {
        PaletteMode::Dark => "dark",
        PaletteMode::Light => "light",
    }
}

#[must_use]
pub fn ui_theme_from_settings(theme: &str, background_color: Option<&str>) -> UiTheme {
    let mut ui_theme = UiTheme::from_setting(theme).unwrap_or_else(UiTheme::detect);
    if let Some(background) = background_color.and_then(parse_hex_rgb_color) {
        ui_theme = ui_theme.with_background_color(background);
    }
    ui_theme
}

#[must_use]
pub fn parse_hex_rgb_color(value: &str) -> Option<Color> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if hex.len() != 6 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[must_use]
pub fn normalize_hex_rgb_color(value: &str) -> Option<String> {
    hex_rgb_string(parse_hex_rgb_color(value)?)
}

#[must_use]
pub fn hex_rgb_string(color: Color) -> Option<String> {
    let Color::Rgb(r, g, b) = color else {
        return None;
    };
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

#[must_use]
pub fn adapt_fg_for_palette_mode(color: Color, _bg: Color, mode: PaletteMode) -> Color {
    match mode {
        PaletteMode::Dark => color,
        PaletteMode::Light => adapt_fg_for_light_palette(color),
    }
}

#[must_use]
pub fn adapt_bg_for_palette_mode(color: Color, mode: PaletteMode) -> Color {
    match mode {
        PaletteMode::Dark => color,
        PaletteMode::Light => adapt_bg_for_light_palette(color),
    }
}

/// Adapt a foreground color for the active theme.
/// For community themes, remap dark-palette constants to the theme's slots.
#[must_use]
pub fn adapt_fg_for_theme(color: Color, theme_id: ThemeId, ui_theme: &UiTheme) -> Color {
    // Only remap for community themes (not System/Terminal/Mimofan/MimofanLight)
    match theme_id {
        ThemeId::System | ThemeId::Terminal | ThemeId::Mimofan | ThemeId::MimofanLight => color,
        ThemeId::Ember | ThemeId::Cosmic | ThemeId::Handwritten | ThemeId::Crush => {
            // Map common dark palette colors to theme equivalents
            if color == TEXT_BODY || color == SELECTION_TEXT {
                ui_theme.text_body
            } else if color == TEXT_SOFT {
                ui_theme.text_soft
            } else if color == TEXT_MUTED || color == TEXT_SECONDARY {
                ui_theme.text_muted
            } else if color == TEXT_HINT || color == TEXT_DIM {
                ui_theme.text_hint
            } else if color == BORDER_COLOR {
                ui_theme.border
            } else if color == MIMOFAN_ACCENT_PRIMARY || color == TEXT_ACCENT {
                ui_theme.accent_primary
            } else if color == DEEPSEEK_SKY || color == ACCENT_TOOL_LIVE {
                ui_theme.accent_secondary
            } else if color == STATUS_ERROR || color == MIMOFAN_ERROR_RGB.into() {
                ui_theme.error_fg
            } else if color == STATUS_WARNING {
                ui_theme.warning
            } else if color == STATUS_SUCCESS {
                ui_theme.success
            } else if color == STATUS_INFO {
                ui_theme.info
            } else if color == MODE_AGENT {
                ui_theme.mode_agent
            } else if color == MODE_YOLO {
                ui_theme.mode_yolo
            } else if color == MODE_PLAN {
                ui_theme.mode_plan
            } else if color == MODE_GOAL {
                ui_theme.mode_goal
            } else {
                color
            }
        }
    }
}

/// Adapt a background color for the active theme.
/// For community themes, remap dark-palette constants to the theme's slots.
#[must_use]
pub fn adapt_bg_for_theme(color: Color, theme_id: ThemeId, ui_theme: &UiTheme) -> Color {
    // Only remap for community themes (not System/Terminal/Mimofan/MimofanLight)
    match theme_id {
        ThemeId::System | ThemeId::Terminal | ThemeId::Mimofan | ThemeId::MimofanLight => color,
        ThemeId::Ember | ThemeId::Cosmic | ThemeId::Handwritten | ThemeId::Crush => {
            // Map common dark palette colors to theme equivalents
            if color == DEEPSEEK_INK || color == BACKGROUND_DARK || color == ui_theme.surface_bg {
                ui_theme.surface_bg
            } else if color == DEEPSEEK_SLATE
                || color == COMPOSER_BG
                || color == SURFACE_PANEL
                || color == SURFACE_TOOL
            {
                ui_theme.panel_bg
            } else if color == SURFACE_ELEVATED {
                ui_theme.elevated_bg
            } else if color == SELECTION_BG {
                ui_theme.selection_bg
            } else if color == SURFACE_ERROR || color == MIMOFAN_ERROR_SURFACE_RGB.into() {
                ui_theme.error_surface
            } else if color == SURFACE_REASONING {
                // Keep reasoning surface as-is for now
                color
            } else {
                color
            }
        }
    }
}

fn adapt_fg_for_light_palette(color: Color) -> Color {
    if color == TEXT_BODY || color == SELECTION_TEXT || color == Color::White {
        LIGHT_TEXT_BODY
    } else if color == TEXT_SECONDARY || color == TEXT_MUTED {
        LIGHT_TEXT_MUTED
    } else if color == TEXT_HINT || color == TEXT_DIM {
        LIGHT_TEXT_HINT
    } else if color == TEXT_SOFT || color == TEXT_TOOL_OUTPUT {
        LIGHT_TEXT_SOFT
    } else if color == BORDER_COLOR {
        LIGHT_BORDER
    } else if color == TEXT_ACCENT || color == DEEPSEEK_SKY || color == ACCENT_TOOL_LIVE {
        MIMOFAN_ACCENT_PRIMARY
    } else if color == TEXT_REASONING || color == ACCENT_REASONING_LIVE {
        Color::Rgb(146, 64, 14)
    } else if color == ACCENT_TOOL_ISSUE {
        Color::Rgb(159, 18, 57)
    } else if color == DIFF_ADDED {
        Color::Rgb(22, 101, 52)
    } else if color == USER_BODY {
        LIGHT_USER_BODY
    } else {
        color
    }
}

fn adapt_bg_for_light_palette(color: Color) -> Color {
    if color == DEEPSEEK_INK || color == BACKGROUND_DARK {
        LIGHT_SURFACE
    } else if color == DEEPSEEK_SLATE
        || color == COMPOSER_BG
        || color == SURFACE_PANEL
        || color == SURFACE_TOOL
    {
        LIGHT_PANEL
    } else if color == SURFACE_ELEVATED {
        LIGHT_ELEVATED
    } else if color == SELECTION_BG {
        LIGHT_SELECTION_BG
    } else {
        color
    }
}

// =============================================================================
// ColorDepth and color adaptation
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    Ansi256,
    Ansi16,
}

impl ColorDepth {
    #[must_use]
    pub fn detect() -> Self {
        Self::detect_from_term(std::env::var("TERM").ok().as_deref())
    }

    #[must_use]
    fn detect_from_term(term: Option<&str>) -> Self {
        let term = term.unwrap_or("");
        if term.contains("truecolor") || term.contains("24bit") {
            Self::TrueColor
        } else if term.contains("256") {
            Self::Ansi256
        } else if term.is_empty() || term == "dumb" {
            Self::Ansi16
        } else {
            Self::Ansi256
        }
    }
}

#[must_use]
pub fn adapt_color(color: Color, depth: ColorDepth) -> Color {
    match (color, depth) {
        (_, ColorDepth::TrueColor) => color,
        (Color::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(rgb_to_ansi256(r, g, b)),
        (Color::Rgb(r, g, b), ColorDepth::Ansi16) => nearest_ansi16(r, g, b),
        _ => color,
    }
}

#[must_use]
pub fn adapt_bg(color: Color, depth: ColorDepth) -> Color {
    match (color, depth) {
        (_, ColorDepth::TrueColor) => color,
        (Color::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(rgb_to_ansi256(r, g, b)),
        (_, ColorDepth::Ansi256) => color,
        (_, ColorDepth::Ansi16) => Color::Reset,
    }
}

#[must_use]
pub fn reasoning_surface_tint(depth: ColorDepth) -> Option<Color> {
    match depth {
        ColorDepth::Ansi16 => None,
        _ => Some(adapt_bg(SURFACE_REASONING_TINT, depth)),
    }
}

#[must_use]
pub fn pulse_brightness(color: Color, now_ms: u64) -> Color {
    let phase = (now_ms % 2000) as f32 / 2000.0;
    let t = (phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let alpha = 0.30 + t * 0.70;
    match color {
        Color::Rgb(r, g, b) => {
            let s = |c: u8| -> u8 { ((f32::from(c)) * alpha).round().clamp(0.0, 255.0) as u8 };
            Color::Rgb(s(r), s(g), s(b))
        }
        other => other,
    }
}

fn nearest_ansi16(r: u8, g: u8, b: u8) -> Color {
    let lum = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    if lum < 24 {
        return Color::Black;
    }
    if r > 220 && g > 220 && b > 220 {
        return Color::White;
    }
    let bright = lum > 144;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max.saturating_sub(min) < 16 {
        return if bright { Color::Gray } else { Color::DarkGray };
    }
    if r >= g && r >= b {
        if g > b + 24 {
            if bright {
                Color::LightYellow
            } else {
                Color::Yellow
            }
        } else if b > r.saturating_sub(24) {
            if bright {
                Color::LightMagenta
            } else {
                Color::Magenta
            }
        } else if bright {
            Color::LightRed
        } else {
            Color::Red
        }
    } else if g >= r && g >= b {
        if b > r + 24 {
            if bright {
                Color::LightCyan
            } else {
                Color::Cyan
            }
        } else if bright {
            Color::LightGreen
        } else {
            Color::Green
        }
    } else if r.saturating_add(48) >= b && r > g + 24 {
        if bright {
            Color::LightMagenta
        } else {
            Color::Magenta
        }
    } else if g.saturating_add(48) >= b && g > r + 24 {
        if bright {
            Color::LightCyan
        } else {
            Color::Cyan
        }
    } else if bright {
        Color::LightBlue
    } else {
        Color::Blue
    }
}

fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    fn nearest_cube_level(channel: u8) -> usize {
        CUBE_LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, level)| channel.abs_diff(**level))
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    fn dist_sq(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
        let dr = i32::from(a.0) - i32::from(b.0);
        let dg = i32::from(a.1) - i32::from(b.1);
        let db = i32::from(a.2) - i32::from(b.2);
        (dr * dr + dg * dg + db * db) as u32
    }

    let ri = nearest_cube_level(r);
    let gi = nearest_cube_level(g);
    let bi = nearest_cube_level(b);
    let cube_rgb = (CUBE_LEVELS[ri], CUBE_LEVELS[gi], CUBE_LEVELS[bi]);
    let cube_index = 16 + (36 * ri) as u8 + (6 * gi) as u8 + bi as u8;

    let avg = ((u16::from(r) + u16::from(g) + u16::from(b)) / 3) as u8;
    let gray_i = if avg <= 8 {
        0
    } else if avg >= 238 {
        23
    } else {
        ((u16::from(avg) - 8 + 5) / 10).min(23) as u8
    };
    let gray = 8 + 10 * gray_i;
    let gray_index = 232 + gray_i;

    if dist_sq((r, g, b), (gray, gray, gray)) < dist_sq((r, g, b), cube_rgb) {
        gray_index
    } else {
        cube_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_names_roundtrip() {
        for theme in SELECTABLE_THEMES {
            let name = theme.name();
            let parsed = ThemeId::from_name(name);
            assert_eq!(parsed, Some(*theme), "Failed roundtrip for {}", name);
        }
    }

    #[test]
    fn test_normalize_theme_aliases() {
        assert_eq!(normalize_theme_name("dark"), Some("dark"));
        assert_eq!(normalize_theme_name("mimofan"), Some("dark"));
        assert_eq!(normalize_theme_name("light"), Some("light"));
        assert_eq!(normalize_theme_name("mimofan-light"), Some("light"));
        assert_eq!(normalize_theme_name("cosmic"), Some("cosmic"));
        assert_eq!(normalize_theme_name("neon"), Some("cosmic"));
        assert_eq!(normalize_theme_name("handwritten"), Some("handwritten"));
        assert_eq!(normalize_theme_name("paper"), Some("handwritten"));
        assert_eq!(normalize_theme_name("crush"), Some("crush"));
        assert_eq!(normalize_theme_name("berry"), Some("crush"));
    }

    #[test]
    fn test_parse_hex_rgb_color() {
        assert!(parse_hex_rgb_color("#ff0000").is_some());
        assert!(parse_hex_rgb_color("00ff00").is_some());
        assert!(parse_hex_rgb_color("#fff").is_none());
        assert!(parse_hex_rgb_color("invalid").is_none());
    }
}
