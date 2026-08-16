//! Sidebar sub-panel rendering (extracted from sidebar.rs for #647).

use std::fmt::Write;

use crate::tui::sidebar::*;
use crate::tui::subagent_routing::active_fanout_counts;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};

pub(crate) fn render_sidebar_subagents(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let usable_rows = area.height.saturating_sub(3) as usize;
    let cached_ids: std::collections::HashSet<&str> = app
        .subagent_cache
        .iter()
        .map(|agent| agent.agent_id.as_str())
        .collect();
    let progress_only_count = app
        .agent_progress
        .keys()
        .filter(|id| !cached_ids.contains(id.as_str()))
        .count();
    let cached_running = app
        .subagent_cache
        .iter()
        .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
        .count();
    let role_counts: std::collections::BTreeMap<String, usize> =
        app.subagent_cache
            .iter()
            .fold(std::collections::BTreeMap::new(), |mut acc, agent| {
                *acc.entry(agent.agent_type.as_str().to_string())
                    .or_insert(0) += 1;
                acc
            });
    let (fanout_running, fanout_total) = active_fanout_counts(app)
        .map(|(running, total)| (running, Some(total)))
        .unwrap_or((0, None));
    let foreground_rlm_running = foreground_rlm_running(app);

    let summary = SidebarSubagentSummary {
        cached_total: app.subagent_cache.len(),
        cached_running,
        progress_only_count,
        fanout_total,
        fanout_running,
        foreground_rlm_running,
        role_counts,
    };
    let rows = sidebar_agent_rows(app);
    let (lines, row_actions) = subagent_panel_rows(
        &summary,
        &rows,
        content_width,
        usable_rows.max(1),
        &app.ui_theme,
    );
    let full_texts = subagent_panel_hover_texts(&summary, &rows, usable_rows.max(1));

    render_sidebar_section(f, area, "Agents", lines, full_texts, row_actions, app);
}

/// Minimal projection of the data the sub-agent sidebar needs. Lifted out
/// of `render_sidebar_subagents` so the rendering can be snapshot-tested
/// without a full `App`.
#[derive(Debug, Clone, Default)]
pub struct SidebarSubagentSummary {
    pub cached_total: usize,
    pub cached_running: usize,
    pub progress_only_count: usize,
    pub fanout_total: Option<usize>,
    pub fanout_running: usize,
    pub foreground_rlm_running: bool,
    pub role_counts: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct SidebarAgentRow {
    pub id: String,
    pub parent_run_id: Option<String>,
    pub spawn_depth: u32,
    pub name: String,
    pub role: String,
    pub status: String,
    pub objective: Option<String>,
    pub git_branch: Option<String>,
    pub progress: Option<String>,
    pub steps_taken: u32,
    pub duration_ms: Option<u64>,
}

pub(crate) fn foreground_rlm_running(app: &App) -> bool {
    app.active_cell.as_ref().is_some_and(|active| {
        active.entries().iter().any(|entry| {
            matches!(
                entry,
                HistoryCell::Tool(ToolCell::Generic(generic))
                    if matches!(
                        generic.name.as_str(),
                        "rlm_open" | "rlm_eval" | "rlm_configure" | "rlm_close" | "rlm"
                    ) && generic.status == ToolStatus::Running
            )
        })
    })
}

pub(crate) fn sidebar_agent_rows(app: &App) -> Vec<SidebarAgentRow> {
    let mut rows: Vec<SidebarAgentRow> = app
        .subagent_cache
        .iter()
        .map(|agent| {
            let progress = app
                .agent_progress
                .get(&agent.agent_id)
                .cloned()
                .or_else(|| {
                    agent
                        .result
                        .as_deref()
                        .map(summarize_tool_output)
                        .filter(|summary| !summary.trim().is_empty())
                });
            // #3030: Prefer the user-assigned nickname > stable label
            // ("Agent 1") > raw name. Every spawned agent gets a label-map
            // entry, so the generated label must not shadow nicknames.
            let display_name = agent
                .nickname
                .clone()
                .or_else(|| app.agent_label_map.get(&agent.agent_id).cloned())
                .unwrap_or_else(|| agent.name.clone());
            SidebarAgentRow {
                id: agent.agent_id.clone(),
                parent_run_id: agent.parent_run_id.clone(),
                spawn_depth: agent.spawn_depth,
                name: display_name,
                role: agent.agent_type.as_str().to_string(),
                status: agent
                    .worker_status
                    .clone()
                    .map(sidebar_worker_status_text)
                    .unwrap_or_else(|| subagent_status_text(&agent.status))
                    .to_string(),
                objective: Some(agent.assignment.objective.clone())
                    .filter(|objective| !objective.trim().is_empty()),
                git_branch: agent.git_branch.clone(),
                progress,
                steps_taken: agent.steps_taken,
                duration_ms: Some(agent.duration_ms),
            }
        })
        .collect();

    let cached_ids: std::collections::HashSet<&str> = app
        .subagent_cache
        .iter()
        .map(|agent| agent.agent_id.as_str())
        .collect();
    rows.extend(
        app.agent_progress
            .iter()
            .filter(|(id, _)| !cached_ids.contains(id.as_str()))
            .map(|(id, progress)| {
                // #3030: Prefer stable label for progress-only agents too.
                let display_name = app
                    .agent_label_map
                    .get(id.as_str())
                    .cloned()
                    .unwrap_or_else(|| id.clone());
                let meta = app.agent_progress_meta.get(id.as_str());
                let spawn_depth = meta.map(|meta| meta.spawn_depth).unwrap_or_default();
                SidebarAgentRow {
                    id: id.clone(),
                    parent_run_id: meta.and_then(|meta| meta.parent_run_id.clone()),
                    spawn_depth,
                    name: display_name,
                    role: if spawn_depth > 1 {
                        "child".to_string()
                    } else {
                        "agent".to_string()
                    },
                    status: sidebar_progress_status_text(progress).to_string(),
                    objective: None,
                    git_branch: None,
                    progress: Some(progress.clone()),
                    steps_taken: 0,
                    duration_ms: None,
                }
            }),
    );

    sort_sidebar_agent_rows_as_tree(rows)
}

pub(crate) fn sort_sidebar_agent_rows_as_tree(rows: Vec<SidebarAgentRow>) -> Vec<SidebarAgentRow> {
    let known_ids: std::collections::HashSet<String> =
        rows.iter().map(|row| row.id.clone()).collect();
    let mut children: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    let mut roots = Vec::new();

    for (idx, row) in rows.iter().enumerate() {
        if let Some(parent) = row.parent_run_id.as_deref()
            && known_ids.contains(parent)
        {
            children.entry(parent.to_string()).or_default().push(idx);
            continue;
        }
        roots.push(idx);
    }

    fn push_tree(
        idx: usize,
        rows: &[SidebarAgentRow],
        children: &std::collections::HashMap<String, Vec<usize>>,
        seen: &mut std::collections::HashSet<usize>,
        out: &mut Vec<SidebarAgentRow>,
    ) {
        if !seen.insert(idx) {
            return;
        }
        out.push(rows[idx].clone());
        if let Some(child_indices) = children.get(&rows[idx].id) {
            for child_idx in child_indices {
                push_tree(*child_idx, rows, children, seen, out);
            }
        }
    }

    let mut out = Vec::with_capacity(rows.len());
    let mut seen = std::collections::HashSet::new();
    for idx in roots {
        push_tree(idx, &rows, &children, &mut seen, &mut out);
    }
    for idx in 0..rows.len() {
        push_tree(idx, &rows, &children, &mut seen, &mut out);
    }
    out
}

pub(crate) fn subagent_status_text(status: &SubAgentStatus) -> &'static str {
    match status {
        SubAgentStatus::Running => "running",
        SubAgentStatus::Completed => "done",
        SubAgentStatus::Interrupted(_) => "interrupted",
        SubAgentStatus::Failed(_) => "failed",
        SubAgentStatus::Cancelled => "canceled",
        SubAgentStatus::BudgetExhausted => "budget",
    }
}

pub(crate) fn sidebar_worker_status_text(status: AgentWorkerStatus) -> &'static str {
    match status {
        AgentWorkerStatus::Queued => "queued",
        AgentWorkerStatus::Starting => "starting",
        AgentWorkerStatus::Running => "running",
        AgentWorkerStatus::WaitingForUser => "waiting",
        AgentWorkerStatus::ModelWait => "model wait",
        AgentWorkerStatus::RunningTool => "tool",
        AgentWorkerStatus::Completed => "done",
        AgentWorkerStatus::Failed => "failed",
        AgentWorkerStatus::Cancelled => "canceled",
        AgentWorkerStatus::Interrupted => "interrupted",
    }
}

pub(crate) fn sidebar_progress_status_text(progress: &str) -> &'static str {
    let lower = progress.to_ascii_lowercase();
    if lower.contains("queued") {
        "queued"
    } else if lower.contains("waiting for user") || lower.contains("waiting for follow-up") {
        "waiting"
    } else if lower.contains("waiting for model") || lower.contains("requesting model") {
        "model wait"
    } else if lower.contains("running tool")
        || lower.contains("executing tool")
        || lower.contains("tool:")
    {
        "tool"
    } else if lower.contains("starting") {
        "starting"
    } else {
        agent_worker_status_name(AgentWorkerStatus::Running)
    }
}

/// Build the Agents panel lines together with a parallel per-line
/// click-action vector (#3028). Agent label rows open the Fleet worker status
/// view via `/fleet status`; header, role-mix, detail, and RLM lines are not
/// clickable.
pub(crate) fn subagent_panel_rows(
    summary: &SidebarSubagentSummary,
    rows: &[SidebarAgentRow],
    content_width: usize,
    max_rows: usize,
    theme: &palette::UiTheme,
) -> (Vec<Line<'static>>, Vec<Option<String>>) {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));
    let mut actions: Vec<Option<String>> = Vec::with_capacity(max_rows.max(4));

    let fanout_total = summary.fanout_total.unwrap_or(0);
    if summary.cached_total == 0
        && summary.progress_only_count == 0
        && fanout_total == 0
        && !summary.foreground_rlm_running
    {
        lines.push(Line::from(Span::styled(
            "No agents",
            Style::default().fg(theme.text_muted),
        )));
        actions.push(None);
        return (lines, actions);
    }

    let (live_running, total) = if let Some(total) = summary.fanout_total {
        (summary.fanout_running, total)
    } else {
        (
            summary.cached_running + summary.progress_only_count,
            summary.cached_total + summary.progress_only_count,
        )
    };
    let done = total.saturating_sub(live_running);
    let header = if live_running > 0 {
        vec![
            Span::styled(
                format!("{live_running} running"),
                Style::default().fg(theme.accent_primary).bold(),
            ),
            Span::styled(format!(" / {total}"), Style::default().fg(theme.text_muted)),
        ]
    } else {
        vec![Span::styled(
            format!("{done} done"),
            Style::default().fg(theme.success),
        )]
    };
    lines.push(Line::from(header));
    actions.push(None);

    if !summary.role_counts.is_empty() {
        let mix: Vec<String> = summary
            .role_counts
            .iter()
            .map(|(role, count)| format!("{count} {role}"))
            .collect();
        let role_line = mix.join(" \u{00B7} ");
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&role_line, content_width.max(1)),
            Style::default().fg(theme.text_dim),
        )));
        actions.push(None);
    }

    for row in rows {
        if lines.len() >= max_rows {
            break;
        }
        let (marker, color) = agent_status_marker(row.status.as_str(), theme);
        let tree_prefix = agent_tree_prefix(row);
        let label = format!("{tree_prefix}{marker} {} {}", row.role, row.name);
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&label, content_width.max(1)),
            Style::default().fg(color),
        )));
        actions.push(Some("/fleet status".to_string()));

        // Auto-collapse finished sub-agents: hide detail lines for completed
        // agents so the sidebar stays compact when work is done.
        if row.status == "done" {
            continue;
        }

        if lines.len() >= max_rows {
            break;
        }
        // #3030: keep raw agent ids out of the compact detail line — the
        // full id remains available in the hover text.
        let mut detail_parts = Vec::new();
        if row.steps_taken > 0 {
            detail_parts.push(format!("{} step(s)", row.steps_taken));
        }
        if let Some(progress) = row.progress.as_deref()
            && !progress.trim().is_empty()
        {
            detail_parts.push(summarize_tool_output(progress));
        }
        if let Some(branch) = row.git_branch.as_deref() {
            detail_parts.push(format!("branch {branch}"));
        }
        if detail_parts.is_empty() {
            detail_parts.push(row.status.clone());
        }
        lines.push(Line::from(Span::styled(
            format!(
                "  {}",
                truncate_line_to_width(
                    &detail_parts.join(" · "),
                    content_width.saturating_sub(2).max(1)
                )
            ),
            Style::default().fg(theme.text_dim),
        )));
        actions.push(None);
    }

    if summary.foreground_rlm_running {
        lines.push(Line::from(vec![
            Span::styled("RLM", Style::default().fg(theme.accent_primary).bold()),
            Span::styled(
                " foreground work active",
                Style::default().fg(theme.text_dim),
            ),
        ]));
        actions.push(None);
    }

    debug_assert_eq!(lines.len(), actions.len());
    (lines, actions)
}

pub(crate) fn agent_tree_prefix(row: &SidebarAgentRow) -> String {
    if row.parent_run_id.is_none() && row.spawn_depth <= 1 {
        return String::new();
    }
    let depth = row.spawn_depth.max(2).saturating_sub(2).min(6);
    format!("{}└─ ", "  ".repeat(depth as usize))
}

pub(crate) fn subagent_panel_hover_texts(
    summary: &SidebarSubagentSummary,
    rows: &[SidebarAgentRow],
    max_rows: usize,
) -> Vec<String> {
    let mut texts = Vec::with_capacity(max_rows.max(4));

    let fanout_total = summary.fanout_total.unwrap_or(0);
    if summary.cached_total == 0
        && summary.progress_only_count == 0
        && fanout_total == 0
        && !summary.foreground_rlm_running
    {
        texts.push("No agents".to_string());
        return texts;
    }

    let (live_running, total) = if let Some(total) = summary.fanout_total {
        (summary.fanout_running, total)
    } else {
        (
            summary.cached_running + summary.progress_only_count,
            summary.cached_total + summary.progress_only_count,
        )
    };
    let done = total.saturating_sub(live_running);
    if live_running > 0 {
        texts.push(format!("{live_running} running / {total}"));
    } else {
        texts.push(format!("{done} done"));
    }

    if !summary.role_counts.is_empty() && texts.len() < max_rows {
        let mix: Vec<String> = summary
            .role_counts
            .iter()
            .map(|(role, count)| format!("{count} {role}"))
            .collect();
        texts.push(mix.join(" · "));
    }

    for row in rows {
        if texts.len() >= max_rows {
            break;
        }
        // The compact label row truncates aggressively, so its hover text
        // carries the full agent dossier: id, role, status, elapsed,
        // objective, branch, and untruncated progress (#3063).
        texts.push(agent_row_hover_text(row));

        if row.status == "done" {
            continue;
        }

        if texts.len() >= max_rows {
            break;
        }
        let mut detail_parts = Vec::new();
        detail_parts.push(row.id.clone());
        if row.steps_taken > 0 {
            detail_parts.push(format!("{} step(s)", row.steps_taken));
        }
        if let Some(progress) = row.progress.as_deref()
            && !progress.trim().is_empty()
        {
            detail_parts.push(progress.trim().to_string());
        }
        if let Some(branch) = row.git_branch.as_deref() {
            detail_parts.push(format!("branch {branch}"));
        }
        if let Some(duration) = row.duration_ms {
            detail_parts.push(format_duration_ms(duration));
        }
        texts.push(format!("  {}", detail_parts.join(" · ")));
    }

    if summary.foreground_rlm_running && texts.len() < max_rows {
        texts.push("RLM foreground work active".to_string());
    }

    texts
}

/// Full hover dossier for one Agents-panel label row (#3063). The compact
/// row only shows `marker role name`, so hovering reveals everything else
/// without spamming raw ids into the normal view.
pub(crate) fn agent_row_hover_text(row: &SidebarAgentRow) -> String {
    let (marker, _) = agent_status_marker(row.status.as_str(), &palette::UI_THEME);
    let mut text = format!(
        "{}{} {} {}",
        agent_tree_prefix(row),
        marker,
        row.role,
        row.name
    );
    let _ = write!(text, "\nid: {}", row.id);
    if let Some(parent) = row.parent_run_id.as_deref() {
        let _ = write!(text, "\nparent: {parent}");
    }
    if row.spawn_depth > 0 {
        let _ = write!(text, "\ndepth: {}", row.spawn_depth);
    }
    let mut status_line = format!("status: {}", row.status);
    if let Some(duration) = row.duration_ms {
        let _ = write!(status_line, " · elapsed {}", format_duration_ms(duration));
    }
    if row.steps_taken > 0 {
        let _ = write!(status_line, " · {} step(s)", row.steps_taken);
    }
    let _ = write!(text, "\n{status_line}");
    if let Some(objective) = row.objective.as_deref() {
        let _ = write!(text, "\nobjective: {}", objective.trim());
    }
    if let Some(branch) = row.git_branch.as_deref() {
        let _ = write!(text, "\nbranch: {branch}");
    }
    if let Some(progress) = row.progress.as_deref()
        && !progress.trim().is_empty()
    {
        let _ = write!(text, "\nprogress: {}", progress.trim());
    }
    text
}

pub(crate) fn agent_status_marker(
    status: &str,
    theme: &palette::UiTheme,
) -> (&'static str, ratatui::style::Color) {
    match status {
        "running" => ("[~]", theme.warning),
        "done" => ("[✓]", theme.success),
        "failed" => ("[!]", theme.error_fg),
        "canceled" | "interrupted" => ("[-]", theme.text_muted),
        _ => ("[ ]", theme.text_muted),
    }
}
