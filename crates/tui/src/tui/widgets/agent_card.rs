//! In-transcript cards for sub-agent activity (issue #128).
//!
//! Two cards consume the #130 mailbox stream and render live in the chat
//! transcript:
//!
//! - [`DelegateCard`] — single `agent` invocation. Live tree of the
//!   last 3 actions plus a header with status / glyph / role.
//! - [`FanoutCard`] — `rlm` fanout (or any future multi-child dispatch).
//!   Dot-grid of worker slots (`●` filled, `○` pending) plus an aggregate
//!   counts line.
//!
//! Both cards are state machines updated by [`apply_to_delegate`] /
//! [`apply_to_fanout`]. The sidebar (see `tui/sidebar.rs`) defers detail
//! to whichever card is active in the transcript, so these are the
//! primary status surface.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::localization::{Locale, MessageId, tr};
use crate::palette;
use crate::tools::subagent::MailboxMessage;
use crate::tui::widgets::tool_card::{ToolFamily, family_glyph, family_label};

/// Maximum number of recent actions kept on a `DelegateCard`. Older entries
/// are dropped from the head; an ellipsis row signals truncation.
pub const DELEGATE_MAX_ACTIONS: usize = 3;

/// Lifecycle of a delegated / fanned-out agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLifecycle {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    /// Interrupted with a continuable checkpoint (e.g. API timeout); not
    /// running, but recoverable from its checkpoint.
    Interrupted,
}

impl AgentLifecycle {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Pending => palette::TEXT_MUTED,
            Self::Running => palette::STATUS_WARNING,
            Self::Completed => palette::STATUS_SUCCESS,
            Self::Failed => palette::STATUS_ERROR,
            Self::Cancelled => palette::TEXT_MUTED,
            Self::Interrupted => palette::STATUS_WARNING,
        }
    }
}

/// Card for a single delegated `agent` invocation.
///
/// Stores the last [`DELEGATE_MAX_ACTIONS`] action lines; older entries are
/// truncated and a single ellipsis row is rendered above the visible tail.
#[derive(Debug, Clone)]
pub struct DelegateCard {
    pub agent_id: String,
    pub agent_type: String,
    pub status: AgentLifecycle,
    pub summary: Option<String>,
    actions: Vec<String>,
    truncated: bool,
}

impl DelegateCard {
    #[must_use]
    pub fn new(agent_id: impl Into<String>, agent_type: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            agent_type: agent_type.into(),
            status: AgentLifecycle::Pending,
            summary: None,
            actions: Vec::new(),
            truncated: false,
        }
    }

    pub fn push_action(&mut self, action: impl Into<String>) {
        self.actions.push(action.into());
        if self.actions.len() > DELEGATE_MAX_ACTIONS {
            // Drop one head entry per overflow so steady-state is exactly
            // DELEGATE_MAX_ACTIONS lines; the ellipsis row signals the rest.
            self.actions.remove(0);
            self.truncated = true;
        }
    }

    #[must_use]
    pub fn render_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::with_capacity(self.actions.len() + 3);
        let role = readable_agent_role(&self.agent_type);
        let short_id = crate::session_manager::truncate_id(&self.agent_id).to_string();
        let detail = if let Some(ref summary) = self.summary {
            truncate_action(summary, 72)
        } else {
            short_id
        };
        lines.push(card_header(
            ToolFamily::Delegate,
            self.status,
            &role,
            &detail,
        ));
        if self.truncated {
            lines.push(Line::from(Span::styled(
                "  \u{2026}".to_string(), // …
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }
        for action in &self.actions {
            lines.push(Line::from(vec![
                Span::styled("  \u{2502} ", Style::default().fg(palette::TEXT_DIM)),
                Span::styled(
                    truncate_action(action, 200),
                    Style::default().fg(palette::TEXT_TOOL_OUTPUT),
                ),
            ]));
        }
        if self.status.is_terminal()
            && let Some(summary) = self.summary.as_ref()
        {
            lines.push(Line::from(vec![
                Span::styled("  \u{2570} ", Style::default().fg(palette::TEXT_DIM)),
                Span::styled(
                    truncate_action(summary, 200),
                    Style::default().fg(self.status.color()),
                ),
            ]));
        }
        lines
    }
}

/// One worker slot in a fanout group.
#[derive(Debug, Clone)]
pub struct WorkerSlot {
    /// Stable logical worker key. Stays tied to the worker slot even after a
    /// concrete sub-agent id exists.
    pub worker_id: String,
    /// Concrete agent id once spawned; placeholders use the worker id.
    pub agent_id: String,
    pub status: AgentLifecycle,
}

impl WorkerSlot {
    #[must_use]
    pub fn new(worker_id: impl Into<String>, status: AgentLifecycle) -> Self {
        let worker_id = worker_id.into();
        Self {
            agent_id: worker_id.clone(),
            worker_id,
            status,
        }
    }
}

/// Card for `rlm` (or any multi-child dispatch) fanout: dot-grid +
/// aggregate counts.
///
/// Slots are added as `ChildSpawned` envelopes arrive (or pre-allocated by
/// the engine when the worker count is known up front); each slot
/// transitions independently as its `Completed` / `Failed` / `Cancelled`
/// envelope is observed.
#[derive(Debug, Clone)]
pub struct FanoutCard {
    pub kind: String,
    pub workers: Vec<WorkerSlot>,
    pub locale: Locale,
}

impl FanoutCard {
    #[must_use]
    pub fn new(kind: impl Into<String>, locale: Locale) -> Self {
        Self {
            kind: kind.into(),
            workers: Vec::new(),
            locale,
        }
    }

    /// Pre-seed worker slots when the fanout size is known up front.
    #[allow(dead_code)]
    pub fn with_workers<I, S>(mut self, ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for id in ids {
            self.workers
                .push(WorkerSlot::new(id.into(), AgentLifecycle::Pending));
        }
        self
    }

    /// Update or insert a worker by id. Returns whether the visible state
    /// changed and the card should be redrawn.
    pub fn upsert_worker(&mut self, agent_id: &str, status: AgentLifecycle) -> bool {
        if let Some(slot) = self
            .workers
            .iter_mut()
            .find(|s| s.agent_id == agent_id || s.worker_id == agent_id)
        {
            if slot.agent_id == agent_id && slot.status == status {
                return false;
            }
            slot.agent_id = agent_id.to_string();
            slot.status = status;
            true
        } else {
            self.workers.push(WorkerSlot::new(agent_id, status));
            true
        }
    }

    /// Attach a real agent id to the first pending placeholder slot. Fanout
    /// cards are seeded from task ids before child agents exist; when a child
    /// starts, this keeps the dot count stable instead of appending a second
    /// circle for the same unit of work.
    pub fn claim_pending_worker(&mut self, agent_id: &str, status: AgentLifecycle) -> bool {
        if let Some(slot) = self.workers.iter_mut().find(|s| s.agent_id == agent_id) {
            if slot.status == status {
                return false;
            }
            slot.status = status;
            return true;
        }
        if let Some(slot) = self
            .workers
            .iter_mut()
            .find(|s| matches!(s.status, AgentLifecycle::Pending))
        {
            slot.agent_id = agent_id.to_string();
            slot.status = status;
            return true;
        }
        self.upsert_worker(agent_id, status)
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        let mut done = 0usize;
        let mut running = 0usize;
        let mut failed = 0usize;
        let mut pending = 0usize;
        for slot in &self.workers {
            match slot.status {
                AgentLifecycle::Completed => done += 1,
                AgentLifecycle::Running => running += 1,
                AgentLifecycle::Failed
                | AgentLifecycle::Cancelled
                | AgentLifecycle::Interrupted => failed += 1,
                AgentLifecycle::Pending => pending += 1,
            }
        }
        (done, running, failed, pending)
    }

    #[must_use]
    pub fn dot_grid(&self) -> String {
        let mut s = String::with_capacity(self.workers.len());
        for slot in &self.workers {
            let glyph = match slot.status {
                AgentLifecycle::Completed => '\u{25CF}',   // ●
                AgentLifecycle::Running => '\u{25D0}',     // ◐
                AgentLifecycle::Failed => '\u{00D7}',      // ×
                AgentLifecycle::Cancelled => '\u{2298}',   // ⊘
                AgentLifecycle::Pending => '\u{25CB}',     // ○
                AgentLifecycle::Interrupted => '\u{25CC}', // ◌
            };
            s.push(glyph);
        }
        s
    }

    #[must_use]
    pub fn render_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::with_capacity(3);
        let header_status = self.aggregate_status();
        let title = format!("{} ({} workers)", self.kind, self.workers.len());
        let family = if matches!(self.kind.as_str(), "rlm_open" | "rlm_eval" | "rlm") {
            ToolFamily::Rlm
        } else {
            ToolFamily::Fanout
        };
        lines.push(card_header(family, header_status, &self.kind, &title));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                self.dot_grid(),
                Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        let (done, running, failed, pending) = self.counts();
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                tr(self.locale, MessageId::FanoutCounts)
                    .replace("{done}", &done.to_string())
                    .replace("{running}", &running.to_string())
                    .replace("{failed}", &failed.to_string())
                    .replace("{pending}", &pending.to_string()),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]));
        lines
    }

    fn aggregate_status(&self) -> AgentLifecycle {
        let (done, running, failed, pending) = self.counts();
        if running > 0 || pending > 0 {
            AgentLifecycle::Running
        } else if self
            .workers
            .iter()
            .any(|slot| matches!(slot.status, AgentLifecycle::Interrupted))
        {
            AgentLifecycle::Interrupted
        } else if failed > 0 && done == 0 {
            AgentLifecycle::Failed
        } else if done > 0 {
            AgentLifecycle::Completed
        } else {
            AgentLifecycle::Pending
        }
    }

    /// Worker count (slots seeded or observed via mailbox).
    #[must_use]
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
}

fn card_header(
    family: ToolFamily,
    status: AgentLifecycle,
    role: &str,
    detail: &str,
) -> Line<'static> {
    let glyph = family_glyph(family);
    let verb = family_label(family);
    let header_color = status.color();
    Line::from(vec![
        Span::styled(
            format!("{glyph} "),
            Style::default()
                .fg(header_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            verb.to_string(),
            Style::default()
                .fg(header_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(role.to_string(), Style::default().fg(palette::TEXT_PRIMARY)),
        Span::raw(" "),
        Span::styled(
            format!("[{}]", status.label()),
            Style::default().fg(header_color),
        ),
        Span::raw(" "),
        Span::styled(detail.to_string(), Style::default().fg(palette::TEXT_MUTED)),
    ])
}

/// Map agent types to human-readable role labels (#1981).
fn readable_agent_role(agent_type: &str) -> String {
    match agent_type.to_ascii_lowercase().as_str() {
        "general" => "worker".to_string(),
        "explore" => "scout".to_string(),
        "plan" => "planner".to_string(),
        "review" => "reviewer".to_string(),
        "implementer" => "builder".to_string(),
        "verifier" => "verifier".to_string(),
        "custom" => "specialist".to_string(),
        other => other.to_string(),
    }
}

fn truncate_action(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

/// Apply a mailbox envelope to a `DelegateCard`. Returns `true` if the
/// state changed (UI may want to redraw); `false` if the envelope was for
/// a different `agent_id`.
pub fn apply_to_delegate(card: &mut DelegateCard, msg: &MailboxMessage) -> bool {
    if msg.agent_id() != card.agent_id {
        return false;
    }
    match msg {
        MailboxMessage::Started { .. } => {
            if card.status == AgentLifecycle::Running {
                return false;
            }
            card.status = AgentLifecycle::Running;
        }
        MailboxMessage::Progress { status, .. } => {
            let low_signal = is_low_signal_progress(status);
            if low_signal && card.status == AgentLifecycle::Running {
                return false;
            }
            card.status = AgentLifecycle::Running;
            if !low_signal {
                card.push_action(status);
            }
        }
        MailboxMessage::ToolCallStarted { tool_name, .. } => {
            card.push_action(format!("{tool_name} running"));
        }
        MailboxMessage::ToolCallCompleted { tool_name, ok, .. } => {
            card.push_action(format!("{tool_name} {}", if *ok { "ok" } else { "failed" }));
        }
        MailboxMessage::Completed { summary, .. } => {
            card.status = AgentLifecycle::Completed;
            card.summary = Some(summary.clone());
        }
        MailboxMessage::Failed { error, .. } => {
            card.status = AgentLifecycle::Failed;
            card.summary = Some(error.clone());
        }
        MailboxMessage::Interrupted { reason, .. } => {
            card.status = AgentLifecycle::Interrupted;
            card.summary = Some(reason.clone());
        }
        MailboxMessage::Cancelled { .. } => {
            card.status = AgentLifecycle::Cancelled;
        }
        MailboxMessage::ChildSpawned { .. } => {
            // Delegate cards represent a single agent; child spawns belong
            // to a sibling fanout card, not this one.
            return false;
        }
        MailboxMessage::TokenUsage { .. } => {
            // Cost accumulation happens in handle_subagent_mailbox (ui.rs)
            // before this apply function is called; TokenUsage never reaches
            // this arm in practice.
            return false;
        }
    }
    true
}

fn is_low_signal_progress(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    status.contains("requesting model response")
        || status.starts_with("started (")
        || (status.starts_with("step ") && status.contains(": complete"))
}

/// Apply a mailbox envelope to a `FanoutCard`. Updates per-worker state
/// based on which child the envelope is about. Returns `true` on change.
pub fn apply_to_fanout(card: &mut FanoutCard, msg: &MailboxMessage) -> bool {
    let id = msg.agent_id();
    match msg {
        MailboxMessage::Started { .. } => card.claim_pending_worker(id, AgentLifecycle::Running),
        MailboxMessage::Progress { .. } => card.claim_pending_worker(id, AgentLifecycle::Running),
        MailboxMessage::ToolCallStarted { .. } => {
            card.claim_pending_worker(id, AgentLifecycle::Running)
        }
        MailboxMessage::ToolCallCompleted { .. } => true,
        MailboxMessage::Completed { .. } => card.upsert_worker(id, AgentLifecycle::Completed),
        MailboxMessage::Failed { .. } => card.upsert_worker(id, AgentLifecycle::Failed),
        MailboxMessage::Interrupted { .. } => card.upsert_worker(id, AgentLifecycle::Interrupted),
        MailboxMessage::Cancelled { .. } => card.upsert_worker(id, AgentLifecycle::Cancelled),
        MailboxMessage::ChildSpawned { child_id, .. } => {
            card.upsert_worker(child_id, AgentLifecycle::Pending)
        }
        MailboxMessage::TokenUsage { .. } => {
            // Cost accumulation happens in handle_subagent_mailbox (ui.rs)
            // before this apply function is called; TokenUsage never reaches
            // this arm in practice.
            true
        }
    }
}

#[cfg(test)]
mod tests {}
