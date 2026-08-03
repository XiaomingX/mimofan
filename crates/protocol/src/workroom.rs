//! Workroom types — durable chat-native containers for threaded agent work.
//!
//! A [`Workroom`] groups threads, events, and external references into a
//! stable, addressable surface that can be accessed from the TUI, mobile page,
//! chat bridges, and programmatic Runtime API consumers.
//!
//! See `docs/rfcs/3209-workrooms.md` for the full design.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for a workroom.
///
/// Stable across restarts. Opaque to callers; generated via UUID v4 with a
/// `wr_` prefix for link recognition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WorkroomId(pub String);

impl WorkroomId {
    /// Create a new workroom id from a UUID v4 string.
    pub fn new() -> Self {
        Self(format!("wr_{}", uuid::Uuid::new_v4().simple()))
    }
}

impl Default for WorkroomId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkroomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A durable container for threaded agent conversations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workroom {
    pub id: WorkroomId,
    pub title: String,
    pub workspace: Option<String>,
    pub repo_identity: Option<RepoRef>,
    pub owner: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub visibility: WorkroomVisibility,
}

/// GitHub repository identity attached to a workroom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

/// Visibility controls for a workroom.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkroomVisibility {
    /// Only the local user can access.
    Private,
    /// Accessible to callers bearing one of the listed bearer tokens.
    Shared { allowed_tokens: Vec<String> },
}

/// A thread within a workroom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkroomThread {
    pub id: String,
    pub workroom_id: WorkroomId,
    pub title: String,
    pub kind: WorkroomThreadKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ref: Option<ExternalThreadRef>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkroomThreadKind {
    Channel,
    DirectMessage,
    AgentTask,
    ApprovalQueue,
    ReceiptLog,
}

/// An external reference that can be attached to a workroom thread.
///
/// Stores only metadata — no API keys, tokens, or secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExternalThreadRef {
    GitHubIssue {
        owner: String,
        repo: String,
        number: u64,
    },
    GitHubPullRequest {
        owner: String,
        repo: String,
        number: u64,
    },
    GitHubCommit {
        owner: String,
        repo: String,
        sha: String,
    },
    GitHubCheck {
        owner: String,
        repo: String,
        check_run_id: u64,
    },
}

/// An event within a workroom thread, attributed to a specific agent/model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkroomEvent {
    pub id: String,
    pub thread_id: String,
    pub workroom_id: WorkroomId,
    pub timestamp: DateTime<Utc>,
    pub kind: WorkroomEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentAttribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkroomEventKind {
    Message { content: String },
    Mention { mentioned_user: String },
    ToolCall { tool_name: String, summary: String },
    ToolResult { tool_name: String, success: bool },
    ApprovalRequest { tool_name: String },
    ArtifactLinked { path: String, kind: String },
    Receipt { summary: String },
    Failure { error: String },
    NeedsHuman { reason: String },
    Resumed,
}

/// Attribution metadata recording which agent and model produced an event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAttribution {
    pub provider: String,
    pub model: String,
    pub agent_id: String,
}

/// A shareable link that resolves to a workroom, thread, or event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkroomLink {
    pub workroom_id: WorkroomId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
}

impl WorkroomLink {
    /// Parse a `mimofan://workroom/...` URL.
    ///
    /// Accepted forms:
    /// - `mimofan://workroom/wr_<id>`
    /// - `mimofan://workroom/wr_<id>/thread/<thread_id>`
    /// - `mimofan://workroom/wr_<id>/event/<event_id>`
    pub fn parse(url: &str) -> Option<Self> {
        let rest = url.strip_prefix("mimofan://workroom/")?;
        let mut segments = rest.split('/');
        let workroom_id = parse_segment_with_prefix(segments.next()?, "wr_")?;
        let next = segments.next();
        let (thread_id, event_id) = match next {
            None => (None, None),
            Some("thread") => {
                let thread_id = non_empty_segment(segments.next()?)?;
                match segments.next() {
                    None => (Some(thread_id), None),
                    Some("event") => {
                        let event_id = non_empty_segment(segments.next()?)?;
                        if segments.next().is_some() {
                            return None;
                        }
                        (Some(thread_id), Some(event_id))
                    }
                    _ => return None,
                }
            }
            Some("event") => {
                let event_id = non_empty_segment(segments.next()?)?;
                if segments.next().is_some() {
                    return None;
                }
                (None, Some(event_id))
            }
            _ => return None,
        };

        Some(Self {
            workroom_id: WorkroomId(workroom_id),
            thread_id,
            event_id,
        })
    }

    /// Serialise back to the `mimofan://workroom/...` URL form.
    pub fn to_url(&self) -> String {
        let mut url = format!("mimofan://workroom/{}", self.workroom_id);
        if let Some(ref thread_id) = self.thread_id {
            url.push_str(&format!("/thread/{thread_id}"));
            if let Some(ref event_id) = self.event_id {
                url.push_str(&format!("/event/{event_id}"));
            }
        } else if let Some(ref event_id) = self.event_id {
            url.push_str(&format!("/event/{event_id}"));
        }
        url
    }
}

fn parse_segment_with_prefix(segment: &str, prefix: &str) -> Option<String> {
    let segment = non_empty_segment(segment)?;
    if segment.len() == prefix.len() || !segment.starts_with(prefix) {
        return None;
    }
    Some(segment)
}

fn non_empty_segment(segment: &str) -> Option<String> {
    if segment.is_empty() {
        None
    } else {
        Some(segment.to_string())
    }
}

/// Summary projection of a workroom for list/inbox views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkroomSummary {
    pub id: WorkroomId,
    pub title: String,
    pub updated_at: DateTime<Utc>,
    pub active_threads: usize,
}

/// Paginated list of workrooms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkroomListResponse {
    pub workrooms: Vec<WorkroomSummary>,
}

/// Response from the `/workroom/resolve` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkroomResolveResponse {
    pub link: WorkroomLink,
    pub thread_title: Option<String>,
    pub external_ref: Option<ExternalThreadRef>,
    pub recent_events: Vec<WorkroomEvent>,
}
