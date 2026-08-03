//! Tool output collapse mode for the transcript.

/// Controls how dense tool-call runs are collapsed in the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCollapseMode {
    /// Collapse qualifying tool runs by default.
    Compact,
    /// Never collapse tool runs automatically.
    Expanded,
    /// Collapse only when calm mode is active.
    Calm,
}

impl ToolCollapseMode {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "expanded" | "off" | "none" => Self::Expanded,
            "calm" | "calm-mode" | "calm_only" | "calm-only" => Self::Calm,
            _ => Self::Compact,
        }
    }

    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Expanded => "expanded",
            Self::Calm => "calm",
        }
    }

    #[must_use]
    pub fn is_active(self, calm_mode: bool) -> bool {
        match self {
            Self::Compact => true,
            Self::Expanded => false,
            Self::Calm => calm_mode,
        }
    }
}
