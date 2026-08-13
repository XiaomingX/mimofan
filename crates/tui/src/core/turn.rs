//! Turn context and tracking.
//!
//! A "turn" is one user message and the resulting AI response,
//! including any tool calls that occur.
//!
//! ## Snapshot lifecycle hooks
//!
//! [`pre_turn_snapshot`] and [`post_turn_snapshot`] book-end a turn by
//! taking a workspace-level snapshot into a side git repo (see
//! `crate::snapshot`). They are intentionally non-blocking and
//! non-fatal: any IO error is logged at WARN and swallowed so a busted
//! filesystem or missing `git` binary never derails the agent loop.
//! `/restore N` and the `revert_turn` tool both consume these
//! snapshots.

use crate::models::Usage;
use crate::snapshot::SnapshotRepo;
use std::path::Path;
use std::time::{Duration, Instant};

/// Context for a single turn (user message + AI response).
#[derive(Debug)]
pub struct TurnContext {
    /// Turn ID
    pub id: String,

    /// When the turn started
    pub started_at: Instant,

    /// Current step in the turn (tool call iteration)
    pub step: u32,

    /// Maximum steps allowed
    pub max_steps: u32,

    /// Number of tool calls made in this turn.
    tool_call_count: usize,

    /// Total wall-clock time spent in tool calls this turn, accumulated across
    /// every `record_tool_call_timed` call. Lets diagnostics/reporting surface
    /// a real latency aggregate instead of only a boolean "did tools run".
    tool_call_duration: Duration,

    /// Whether the turn has been cancelled
    pub cancelled: bool,

    /// Usage for this turn
    pub usage: Usage,
}

impl TurnContext {
    /// Create a new turn context
    pub fn new(max_steps: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            started_at: Instant::now(),
            step: 0,
            max_steps,
            tool_call_count: 0,
            tool_call_duration: Duration::ZERO,
            cancelled: false,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                ..Usage::default()
            },
        }
    }

    /// Increment the step counter
    pub fn next_step(&mut self) -> bool {
        self.step += 1;
        self.step <= self.max_steps
    }

    /// Check if the turn has reached max steps
    pub fn at_max_steps(&self) -> bool {
        self.step >= self.max_steps
    }

    /// Record that a tool call occurred (count only). Kept for call sites that
    /// do not measure latency; prefer [`TurnContext::record_tool_call_timed`].
    pub fn record_tool_call(&mut self) {
        self.tool_call_count += 1;
    }

    /// Record a tool call along with how long it took. Accumulates both the
    /// count and the total duration so reporting can derive averages and a
    /// latency total. Issue #734.
    pub fn record_tool_call_timed(&mut self, duration: Duration) {
        self.tool_call_count += 1;
        self.tool_call_duration = self.tool_call_duration.saturating_add(duration);
    }

    /// Number of tool calls made so far this turn.
    pub fn tool_call_count(&self) -> usize {
        self.tool_call_count
    }

    /// Total wall-clock time spent in tool calls this turn.
    pub fn tool_call_total_duration(&self) -> Duration {
        self.tool_call_duration
    }

    /// Average duration per tool call, or `None` when no calls occurred.
    pub fn tool_call_avg_duration(&self) -> Option<Duration> {
        if self.tool_call_count == 0 {
            None
        } else {
            Some(self.tool_call_duration / self.tool_call_count as u32)
        }
    }

    /// Whether this turn has executed at least one tool call.
    pub fn has_tool_calls(&self) -> bool {
        self.tool_call_count > 0
    }

    /// Get the elapsed time
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Add usage from an API response
    pub fn add_usage(&mut self, usage: &Usage) {
        self.usage.input_tokens += usage.input_tokens;
        self.usage.output_tokens += usage.output_tokens;
        self.usage.prompt_cache_hit_tokens = add_optional_usage(
            self.usage.prompt_cache_hit_tokens,
            usage.prompt_cache_hit_tokens,
        );
        self.usage.prompt_cache_miss_tokens = add_optional_usage(
            self.usage.prompt_cache_miss_tokens,
            usage.prompt_cache_miss_tokens,
        );
        self.usage.reasoning_tokens =
            add_optional_usage(self.usage.reasoning_tokens, usage.reasoning_tokens);
    }
}

fn add_optional_usage(total: Option<u32>, delta: Option<u32>) -> Option<u32> {
    match (total, delta) {
        (Some(total), Some(delta)) => Some(total.saturating_add(delta)),
        (None, Some(delta)) => Some(delta),
        (Some(total), None) => Some(total),
        (None, None) => None,
    }
}

/// Maximum characters of the user prompt snippet to embed in a snapshot
/// label. Longer prompts are truncated with an ellipsis.
const USER_PROMPT_LABEL_MAX: usize = 100;

/// Format a snapshot label that includes the user prompt for readability
/// in `/restore` listings.
///
/// Takes the first line of the prompt (up to `USER_PROMPT_LABEL_MAX`
/// characters) and appends it to the traditional `type:seq` label so
/// users can identify which turn each snapshot belongs to.
fn format_snapshot_label(prefix: &str, turn_seq: u64, user_prompt: Option<&str>) -> String {
    let base = format!("{prefix}:{turn_seq}");
    match user_prompt {
        None | Some("") => base,
        Some(prompt) => {
            let first_line = prompt.lines().next().unwrap_or("");
            let truncated: String = first_line.chars().take(USER_PROMPT_LABEL_MAX).collect();
            if truncated.chars().count() < first_line.chars().count() {
                format!("{base}: {truncated}…")
            } else {
                format!("{base}: {truncated}")
            }
        }
    }
}

/// Take a `pre-turn:<seq>` workspace snapshot.
///
/// `cap_bytes` is the workspace-size ceiling that gates first-init
/// (passed through to [`SnapshotRepo::open_or_init_with_cap`]); pass
/// `0` to disable the cap.
/// `user_prompt` is an optional snippet of the user's message for this
/// turn, embedded in the snapshot label so `/restore` listings are
/// human-readable.
///
/// Returns the snapshot SHA on success, `None` on any error. Errors are
/// logged at WARN; the turn loop must not block on this.
pub fn pre_turn_snapshot(
    workspace: &Path,
    turn_seq: u64,
    cap_bytes: u64,
    user_prompt: Option<&str>,
    conversation_len: usize,
) -> Option<String> {
    snapshot_with_label(
        workspace,
        &format_snapshot_label("pre-turn", turn_seq, user_prompt),
        cap_bytes,
        conversation_len,
    )
}

/// Take a `tool:<call_id>` workspace snapshot, taken before executing a
/// file-modifying tool call (write_file, edit_file, apply_patch).
///
/// This enables surgical undo: `/undo` can restore to the most recent
/// `tool:<call_id>` snapshot to revert just the last file write.
///
/// Returns the snapshot SHA on success, `None` on any error. Errors are
/// logged at WARN and are non-fatal.
pub fn pre_tool_snapshot(workspace: &Path, call_id: &str, cap_bytes: u64) -> Option<String> {
    // Tool-level snapshots revert files only; they are not tied to a
    // conversation turn, so conversation_len is left at 0.
    snapshot_with_label(workspace, &format!("tool:{call_id}"), cap_bytes, 0)
}

/// Take a `loop-round:<n>` workspace snapshot before a `/loop` continuation
/// round, so the user can `/rewind` to a specific loop iteration. Enabled by
/// the `/loop --checkpoint` flag (surfaced via `GoalSnapshot::checkpoint_each_round`).
///
/// Returns the snapshot SHA on success, `None` on any error (non-fatal).
pub fn loop_round_snapshot(
    workspace: &Path,
    round: u32,
    cap_bytes: u64,
    conversation_len: usize,
) -> Option<String> {
    snapshot_with_label(
        workspace,
        &format!("loop-round-{round}"),
        cap_bytes,
        conversation_len,
    )
}

/// Take a `post-turn:<seq>` workspace snapshot. Same failure model as
/// [`pre_turn_snapshot`].
pub fn post_turn_snapshot(
    workspace: &Path,
    turn_seq: u64,
    cap_bytes: u64,
    user_prompt: Option<&str>,
    conversation_len: usize,
) -> Option<String> {
    snapshot_with_label(
        workspace,
        &format_snapshot_label("post-turn", turn_seq, user_prompt),
        cap_bytes,
        conversation_len,
    )
}

fn snapshot_with_label(
    workspace: &Path,
    label: &str,
    cap_bytes: u64,
    conversation_len: usize,
) -> Option<String> {
    match SnapshotRepo::open_or_init_with_cap(workspace, cap_bytes) {
        Ok(repo) => {
            let id = match repo.snapshot(label, conversation_len) {
                Ok(id) => Some(id.0),
                Err(e) => {
                    tracing::warn!(target: "snapshot", "snapshot '{label}' failed: {e}");
                    return None;
                }
            };
            // Prune oldest snapshots to cap disk usage (#1112).
            if let Err(e) = repo.prune_keep_last_n(crate::snapshot::DEFAULT_MAX_SNAPSHOTS) {
                tracing::warn!(target: "snapshot", "snapshot prune failed: {e}");
            }
            id
        }
        Err(e) => {
            tracing::warn!(target: "snapshot", "snapshot repo init failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_metrics_accumulate_count_and_duration() {
        let mut turn = TurnContext::new(100);
        assert_eq!(turn.tool_call_count(), 0);
        assert!(turn.tool_call_avg_duration().is_none());
        assert_eq!(turn.tool_call_total_duration(), Duration::ZERO);

        turn.record_tool_call_timed(Duration::from_millis(100));
        turn.record_tool_call_timed(Duration::from_millis(300));

        assert_eq!(turn.tool_call_count(), 2);
        assert_eq!(turn.tool_call_total_duration(), Duration::from_millis(400));
        assert_eq!(
            turn.tool_call_avg_duration(),
            Some(Duration::from_millis(200))
        );
        assert!(turn.has_tool_calls());
    }

    #[test]
    fn untimed_record_tool_call_still_counts_without_duration() {
        let mut turn = TurnContext::new(100);
        turn.record_tool_call();
        assert_eq!(turn.tool_call_count(), 1);
        // No timed call recorded yet, so total duration stays zero; average is
        // zero (count > 0), not None.
        assert_eq!(turn.tool_call_total_duration(), Duration::ZERO);
        assert_eq!(turn.tool_call_avg_duration(), Some(Duration::ZERO));
    }
}
