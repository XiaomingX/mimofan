//! History search state for the composer.

use std::time::Instant;

use serde::{Deserialize, Serialize};

/// History search state for the composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerHistorySearch {
    pub(crate) pre_search_input: String,
    pub(crate) pre_search_cursor: usize,
    pub(crate) query: String,
    pub(crate) selected: usize,
}

impl ComposerHistorySearch {
    pub(crate) fn new(pre_search_input: String, pre_search_cursor: usize) -> Self {
        Self {
            pre_search_input,
            pre_search_cursor,
            query: String::new(),
            selected: 0,
        }
    }
}

/// Draft state for input history navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputHistoryDraft {
    pub(crate) input: String,
    pub(crate) cursor: usize,
}

/// Verdict for a hunt (#2092).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HuntVerdict {
    #[default]
    Hunting,
    Hunted,
    Wounded,
    Escaped,
}

impl HuntVerdict {
    #[must_use]
    pub fn goal_status(self) -> crate::tools::goal::GoalStatus {
        match self {
            Self::Hunting => crate::tools::goal::GoalStatus::Active,
            Self::Hunted => crate::tools::goal::GoalStatus::Complete,
            Self::Wounded => crate::tools::goal::GoalStatus::Paused,
            Self::Escaped => crate::tools::goal::GoalStatus::Blocked,
        }
    }

    #[must_use]
    pub fn from_goal_status(status: crate::tools::goal::GoalStatus) -> Self {
        match status {
            crate::tools::goal::GoalStatus::Active => Self::Hunting,
            crate::tools::goal::GoalStatus::Paused => Self::Wounded,
            crate::tools::goal::GoalStatus::Complete => Self::Hunted,
            crate::tools::goal::GoalStatus::Blocked => Self::Escaped,
        }
    }
}

/// Hunt tracking state (#2092 — was GoalState).
#[derive(Debug, Clone, Default)]
pub struct HuntState {
    pub quarry: Option<String>,
    pub token_budget: Option<u32>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    pub continuation_count: u32,
    pub started_at: Option<Instant>,
    pub verdict: HuntVerdict,
}
