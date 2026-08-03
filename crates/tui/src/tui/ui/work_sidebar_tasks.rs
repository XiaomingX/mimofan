//! work sidebar tasks 子系统（从 ui 上帝文件切片）
use super::*;

/// Choose which durable-task summaries should appear in the Work
/// sidebar's Tasks panel.
///
/// Active tasks (`Queued`/`Running`) are always included. Terminal
/// tasks (`Completed`/`Failed`/`Canceled`) are kept only if their
/// `ended_at` falls within the "recent" window — defined as either:
///
/// - within the current TUI session (`ended_at >= session_started_at`), or
/// - within `recent_ttl` of `now` (so a task that finished a few
///   minutes before the session started still shows).
///
/// Anything older than that — including the multi-day-old completed
/// tasks reported in bug #1913 — is excluded so the sidebar does not
/// accumulate indefinitely across sessions.
///
/// A terminal task missing `ended_at` is treated as not-recent and
/// dropped: durable tasks always stamp `ended_at` when they reach a
/// terminal state, so absence of it indicates a record from a much
/// older schema and isn't worth surfacing.
pub(crate) fn select_work_sidebar_tasks(
    tasks: Vec<TaskSummary>,
    session_started_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
    recent_ttl: chrono::Duration,
) -> Vec<TaskSummary> {
    let recent_cutoff = now - recent_ttl;
    tasks
        .into_iter()
        .filter(|task| match task.status {
            TaskStatus::Queued | TaskStatus::Running => true,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Canceled => {
                match task.ended_at {
                    Some(ended_at) => ended_at >= session_started_at || ended_at >= recent_cutoff,
                    None => false,
                }
            }
        })
        .collect()
}

pub(crate) async fn refresh_active_task_panel(app: &mut App, task_manager: &SharedTaskManager) {
    let tasks = task_manager.list_tasks(None).await;
    let session_started_at = app.session_started_at;
    let now = chrono::Utc::now();
    let mut entries: Vec<TaskPanelEntry> = select_work_sidebar_tasks(
        tasks,
        session_started_at,
        now,
        WORK_SIDEBAR_RECENT_COMPLETED_TTL,
    )
    .into_iter()
    .map(task_summary_to_panel_entry)
    .collect();

    entries.extend(active_reasoning_task_entries(app));
    entries.extend(active_rlm_task_entries(app));

    if let Some(shell_mgr) = app.runtime_services.shell_manager.as_ref()
        && let Ok(mut mgr) = shell_mgr.lock()
    {
        for job in mgr.list_jobs() {
            if !matches!(job.status, crate::tools::shell::ShellStatus::Running) {
                continue;
            }
            entries.push(TaskPanelEntry {
                id: job.id,
                status: "running".to_string(),
                prompt_summary: format!("shell: {}", job.command),
                duration_ms: Some(job.elapsed_ms),
                kind: TaskPanelEntryKind::Background,
                stale: job.stale,
                elapsed_since_output_ms: job.elapsed_since_output_ms,
                owner_agent_id: job.owner_agent_id,
                owner_agent_name: job.owner_agent_name,
            });
        }
    }

    app.task_panel = entries;
}

pub(crate) fn refresh_shell_exec_live_output(app: &mut App) -> bool {
    let Some(shell_mgr) = app.runtime_services.shell_manager.as_ref().cloned() else {
        return false;
    };
    let jobs = {
        let Ok(mut mgr) = shell_mgr.lock() else {
            return false;
        };
        mgr.list_jobs()
            .into_iter()
            .map(|job| (job.id.clone(), job))
            .collect::<std::collections::HashMap<_, _>>()
    };
    if jobs.is_empty() {
        return false;
    }

    let mut changed = false;
    for index in 0..app.virtual_cell_count() {
        let Some((task_id, next_status, next_live, next_duration)) =
            shell_exec_live_update(app, index, &jobs)
        else {
            continue;
        };
        let Some(HistoryCell::Tool(ToolCell::Exec(exec))) = app.cell_at_virtual_index_mut(index)
        else {
            continue;
        };
        if exec.output.is_some() || exec.shell_task_id.as_deref() != Some(task_id.as_str()) {
            continue;
        }
        exec.status = next_status;
        exec.live_output = next_live;
        exec.duration_ms = Some(next_duration);
        changed = true;
    }
    changed
}

fn shell_exec_live_update(
    app: &App,
    index: usize,
    jobs: &std::collections::HashMap<String, ShellJobSnapshot>,
) -> Option<(String, ToolStatus, Option<String>, u64)> {
    let HistoryCell::Tool(ToolCell::Exec(exec)) = app.cell_at_virtual_index(index)? else {
        return None;
    };
    if exec.output.is_some() {
        return None;
    }
    let task_id = exec.shell_task_id.as_deref()?;
    let job = jobs.get(task_id)?;
    let next_status = shell_job_tool_status(&job.status);
    let next_live = shell_job_live_output(job).or_else(|| exec.live_output.clone());
    if exec.status == next_status
        && exec.live_output == next_live
        && exec.duration_ms == Some(job.elapsed_ms)
    {
        return None;
    }
    Some((task_id.to_string(), next_status, next_live, job.elapsed_ms))
}

fn shell_job_tool_status(status: &ShellStatus) -> ToolStatus {
    match status {
        ShellStatus::Running => ToolStatus::Running,
        ShellStatus::Completed => ToolStatus::Success,
        ShellStatus::Failed | ShellStatus::Killed | ShellStatus::TimedOut => ToolStatus::Failed,
    }
}

fn shell_job_live_output(job: &ShellJobSnapshot) -> Option<String> {
    match (job.stdout_tail.is_empty(), job.stderr_tail.is_empty()) {
        (true, true) => None,
        (false, true) => Some(job.stdout_tail.clone()),
        (true, false) => Some(format!("STDERR:\n{}", job.stderr_tail)),
        (false, false) => Some(format!(
            "{}\n\nSTDERR:\n{}",
            job.stdout_tail, job.stderr_tail
        )),
    }
}

fn active_reasoning_task_entries(app: &App) -> Vec<TaskPanelEntry> {
    let Some(active) = app.active_cell.as_ref() else {
        return Vec::new();
    };
    let duration_ms = app
        .turn_started_at
        .map(|started| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));

    active
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| match entry {
            HistoryCell::Thinking {
                streaming: true, ..
            } => Some(TaskPanelEntry {
                id: format!("reasoning-{}", idx + 1),
                status: "running".to_string(),
                prompt_summary: "model reasoning".to_string(),
                duration_ms,
                kind: TaskPanelEntryKind::ModelReasoning,
                stale: false,
                elapsed_since_output_ms: None,
                owner_agent_id: None,
                owner_agent_name: None,
            }),
            _ => None,
        })
        .collect()
}

fn active_rlm_task_entries(app: &App) -> Vec<TaskPanelEntry> {
    let Some(active) = app.active_cell.as_ref() else {
        return Vec::new();
    };
    let duration_ms = app
        .turn_started_at
        .map(|started| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
    active
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            let HistoryCell::Tool(ToolCell::Generic(generic)) = entry else {
                return None;
            };
            if !matches!(
                generic.name.as_str(),
                "rlm_open" | "rlm_eval" | "rlm_configure" | "rlm_close" | "rlm"
            ) || generic.status != ToolStatus::Running
            {
                return None;
            }
            let summary = generic
                .input_summary
                .as_deref()
                .filter(|summary| !summary.trim().is_empty())
                .unwrap_or("running chunked analysis");
            Some(TaskPanelEntry {
                id: format!("rlm-{}", idx + 1),
                status: "running".to_string(),
                prompt_summary: format!("RLM: {summary}"),
                duration_ms,
                kind: TaskPanelEntryKind::Background,
                stale: false,
                elapsed_since_output_ms: None,
                owner_agent_id: None,
                owner_agent_name: None,
            })
        })
        .collect()
}
