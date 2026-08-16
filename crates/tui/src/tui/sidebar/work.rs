//! Sidebar sub-panel rendering (extracted from sidebar.rs for #647).

use std::fmt::Write;

use crate::tui::sidebar::*;
use ratatui::{
    style::Style,
    text::{Line, Span},
};

impl SidebarWorkSummary {
    fn checklist_is_primary(&self) -> bool {
        !self.checklist_items.is_empty()
    }

    fn checklist_is_complete(&self) -> bool {
        self.checklist_is_primary()
            && self
                .checklist_items
                .iter()
                .all(|item| item.status == TodoStatus::Completed)
    }

    fn has_strategy(&self) -> bool {
        self.strategy_explanation
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || !self.strategy_steps.is_empty()
    }

    pub(crate) fn has_useful_content(&self) -> bool {
        self.goal_objective
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || !self.checklist_items.is_empty()
            || self.has_strategy()
            || self.state_updating
    }

    fn strategy_counts(&self) -> (usize, usize, usize) {
        let mut pending = 0;
        let mut in_progress = 0;
        let mut completed = 0;
        for step in &self.strategy_steps {
            match step.status {
                StepStatus::Pending => pending += 1,
                StepStatus::InProgress => in_progress += 1,
                StepStatus::Completed => completed += 1,
            }
        }
        (pending, in_progress, completed)
    }

    fn strategy_progress_percent(&self) -> u8 {
        if self.strategy_steps.is_empty() {
            return 0;
        }
        let completed = self
            .strategy_steps
            .iter()
            .filter(|step| step.status == StepStatus::Completed)
            .count();
        let percent = completed.saturating_mul(100) / self.strategy_steps.len();
        u8::try_from(percent).unwrap_or(u8::MAX)
    }
}

pub(crate) fn should_render_strategy_step(
    summary: &SidebarWorkSummary,
    step: &SidebarWorkStrategyStep,
) -> bool {
    !summary.checklist_is_complete() || step.status == StepStatus::Completed
}

pub(crate) fn renderable_strategy_steps(
    summary: &SidebarWorkSummary,
) -> Vec<&SidebarWorkStrategyStep> {
    summary
        .strategy_steps
        .iter()
        .filter(|step| should_render_strategy_step(summary, step))
        .collect()
}

pub(crate) fn has_renderable_strategy(summary: &SidebarWorkSummary) -> bool {
    summary
        .strategy_explanation
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
        || summary
            .strategy_steps
            .iter()
            .any(|step| should_render_strategy_step(summary, step))
}

pub(crate) fn sidebar_work_summary(app: &mut App) -> SidebarWorkSummary {
    fn live_goal_objective(app: &App) -> Option<String> {
        if app.paused || app.paused_quarry.is_some() {
            app.hunt
                .quarry
                .clone()
                .or_else(|| app.paused_quarry.clone())
        } else {
            app.hunt.quarry.clone()
        }
    }

    fn live_pause_indicator(app: &App) -> Option<String> {
        if app.paused && app.is_loading {
            Some("(Pausing)".to_string())
        } else if app.paused || app.paused_quarry.is_some() {
            Some("(Paused)".to_string())
        } else {
            None
        }
    }

    fn apply_live_goal_state(summary: &mut SidebarWorkSummary, app: &App) {
        summary.goal_objective = live_goal_objective(app);
        summary.goal_token_budget = app.hunt.token_budget;
        summary.goal_completed = app.hunt.verdict == HuntVerdict::Hunted;
        summary.goal_started_at = app.hunt.started_at;
        summary.tokens_used = app.session.total_conversation_tokens;
        summary.pause_indicator = live_pause_indicator(app);
        summary.workflow_paused = app.paused || app.paused_quarry.is_some();
    }

    let fresh = (|| {
        let todos = app.todos.try_lock().ok()?;
        let plan = app.plan_state.try_lock().ok()?;

        let snapshot = todos.snapshot();
        let checklist_completion_pct = snapshot.completion_pct;
        let checklist_items = snapshot
            .items
            .into_iter()
            .map(|item| SidebarWorkChecklistItem {
                id: item.id,
                content: item.content,
                status: item.status,
            })
            .collect();

        let (strategy_explanation, strategy_steps) = if plan.is_empty() {
            (None, Vec::new())
        } else {
            (
                plan.explanation().map(str::to_string),
                plan.steps()
                    .iter()
                    .map(|step| SidebarWorkStrategyStep {
                        text: step.text.clone(),
                        status: step.status.clone(),
                        elapsed: step.elapsed_str(),
                    })
                    .collect(),
            )
        };

        let mut summary = SidebarWorkSummary {
            goal_objective: live_goal_objective(app),
            goal_token_budget: app.hunt.token_budget,
            goal_completed: app.hunt.verdict == HuntVerdict::Hunted,
            goal_started_at: app.hunt.started_at,
            tokens_used: app.session.total_conversation_tokens,
            checklist_completion_pct,
            checklist_items,
            strategy_explanation,
            strategy_steps,
            state_updating: false,
            pause_indicator: live_pause_indicator(app),
            workflow_paused: app.paused || app.paused_quarry.is_some(),
        };
        apply_live_goal_state(&mut summary, app);
        Some(summary)
    })();

    if let Some(summary) = fresh {
        app.cached_work_summary = Some(summary.clone());
        return summary;
    }

    if let Some(cached) = app.cached_work_summary.as_ref() {
        let mut summary = cached.clone();
        apply_live_goal_state(&mut summary, app);
        return summary;
    }

    let mut summary = SidebarWorkSummary {
        state_updating: true,
        ..SidebarWorkSummary::default()
    };
    apply_live_goal_state(&mut summary, app);
    summary
}

pub(crate) fn work_panel_lines(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
    palette_mode: palette::PaletteMode,
    ui_theme: &palette::UiTheme,
) -> Vec<Line<'static>> {
    let theme = Theme::for_palette_mode(palette_mode);
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));

    push_work_goal_lines(summary, content_width, max_rows, &mut lines, ui_theme);

    if summary.state_updating && lines.len() < max_rows {
        lines.push(Line::from(Span::styled(
            "Work state updating...",
            Style::default().fg(ui_theme.text_muted),
        )));
    }

    push_work_checklist_lines(summary, content_width, max_rows, &mut lines, ui_theme);
    push_work_strategy_lines(summary, content_width, max_rows, &mut lines, &theme);

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            work_panel_empty_hint(content_width),
            Style::default().fg(ui_theme.text_muted).italic(),
        )));
    }

    lines
}

pub(crate) fn work_panel_hover_texts(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
) -> Vec<String> {
    let mut texts = Vec::with_capacity(max_rows.max(4));

    if let Some(objective) = summary.goal_objective.as_deref()
        && !objective.trim().is_empty()
        && texts.len() < max_rows
    {
        let icon = if summary.goal_completed {
            "✓"
        } else if summary.workflow_paused {
            "⏸"
        } else {
            "◆"
        };
        texts.push(format!("{icon} {objective}"));

        if let Some(started) = summary.goal_started_at
            && texts.len() < max_rows
        {
            let elapsed = crate::tui::notifications::humanize_duration(started.elapsed());
            let elapsed_str = if summary.goal_completed {
                format!("completed in {elapsed}")
            } else {
                format!("elapsed: {elapsed}")
            };
            texts.push(elapsed_str);
        }

        if let Some(budget) = summary.goal_token_budget
            && texts.len() < max_rows
        {
            let pct = if budget > 0 {
                ((summary.tokens_used as f64 / budget as f64) * 100.0).min(100.0)
            } else {
                0.0
            };
            let bar_width = content_width.min(20);
            let filled = ((pct / 100.0) * bar_width as f64) as usize;
            let bar = format!(
                "[{}{}] {:.0}%",
                "█".repeat(filled),
                "░".repeat(bar_width.saturating_sub(filled)),
                pct
            );
            texts.push(format!(
                "tokens: {}/{} {}",
                summary.tokens_used, budget, bar
            ));
        }
    }

    if summary.state_updating && texts.len() < max_rows {
        texts.push("Work state updating...".to_string());
    }

    if !summary.checklist_items.is_empty() && texts.len() < max_rows {
        let total = summary.checklist_items.len();
        let completed = summary
            .checklist_items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        texts.push(format!(
            "{}% complete ({completed}/{total})",
            summary.checklist_completion_pct
        ));

        let reserve_for_strategy = if has_renderable_strategy(summary) {
            2
        } else {
            0
        };
        let available_item_rows = max_rows
            .saturating_sub(texts.len())
            .saturating_sub(reserve_for_strategy)
            .min(summary.checklist_items.len());
        let max_items =
            if summary.checklist_items.len() > available_item_rows && available_item_rows > 1 {
                available_item_rows - 1
            } else {
                available_item_rows
            };
        let start = checklist_window_start(&summary.checklist_items, max_items);
        let end = start
            .saturating_add(max_items)
            .min(summary.checklist_items.len());
        for item in summary.checklist_items[start..end].iter() {
            let prefix = match item.status {
                TodoStatus::Pending => "[ ]",
                TodoStatus::InProgress => "[~]",
                TodoStatus::Completed => "[✓]",
            };
            texts.push(format!("{prefix} #{} {}", item.id, item.content));
        }

        let earlier = start;
        let later = summary.checklist_items.len().saturating_sub(end);
        let remaining = earlier.saturating_add(later);
        if remaining > 0 && texts.len() < max_rows {
            let mut label = match (earlier, later) {
                (0, later) => format!("+{later} more checklist items"),
                (earlier, 0) => format!("+{earlier} earlier checklist items"),
                (earlier, later) => format!("+{earlier} earlier, +{later} later"),
            };
            // Hovering the overflow row reveals the omitted items, since
            // the compact panel gives no other way to inspect them (#3063).
            let omitted = summary.checklist_items[..start]
                .iter()
                .chain(summary.checklist_items[end..].iter());
            for item in omitted {
                let prefix = match item.status {
                    TodoStatus::Pending => "[ ]",
                    TodoStatus::InProgress => "[~]",
                    TodoStatus::Completed => "[✓]",
                };
                let _ = write!(label, "\n{prefix} #{} {}", item.id, item.content);
            }
            texts.push(label);
        }
    }

    if has_renderable_strategy(summary) && texts.len() < max_rows {
        let strategy_steps = renderable_strategy_steps(summary);

        if !summary.checklist_is_primary() && !summary.strategy_steps.is_empty() {
            let (pending, in_progress, completed) = summary.strategy_counts();
            let total = pending + in_progress + completed;
            texts.push(format!(
                "Strategy metadata {}% complete ({completed}/{total})",
                summary.strategy_progress_percent()
            ));
        } else {
            texts.push(work_strategy_context_label(summary).to_string());
        }

        if let Some(explanation) = summary.strategy_explanation.as_deref()
            && texts.len() < max_rows
        {
            texts.push(explanation.to_string());
        }

        let max_steps = max_rows
            .saturating_sub(texts.len())
            .min(strategy_steps.len());
        let remaining = strategy_steps.len().saturating_sub(max_steps);
        for step in strategy_steps.into_iter().take(max_steps) {
            let prefix = match step.status {
                StepStatus::Pending => "[ ]",
                StepStatus::InProgress => "[~]",
                StepStatus::Completed => "[✓]",
            };
            let mut text = if summary.checklist_is_primary() {
                format!(
                    "{} {}",
                    strategy_context_step_prefix(&step.status),
                    step.text
                )
            } else {
                format!("{prefix} {}", step.text)
            };
            if !step.elapsed.is_empty() {
                let _ = write!(text, " ({})", step.elapsed);
            }
            texts.push(text);
        }

        if remaining > 0 && texts.len() < max_rows {
            texts.push(format!("+{remaining} more strategy steps"));
        }
    }

    if texts.is_empty() {
        texts.push("No active work".to_string());
    }

    texts
}

pub(crate) fn push_work_goal_lines(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
    lines: &mut Vec<Line<'static>>,
    theme: &palette::UiTheme,
) {
    let Some(objective) = summary.goal_objective.as_deref() else {
        return;
    };
    if objective.trim().is_empty() || lines.len() >= max_rows {
        return;
    }

    let icon = if summary.goal_completed {
        "✓"
    } else if summary.workflow_paused {
        "⏸"
    } else {
        "◆"
    };
    let status_style = if summary.goal_completed {
        Style::default()
            .fg(theme.success)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.warning)
            .add_modifier(ratatui::style::Modifier::BOLD)
    };
    let label = if let Some(indicator) = summary.pause_indicator.as_deref() {
        format!("{objective} {indicator}")
    } else {
        objective.to_string()
    };

    lines.push(Line::from(Span::styled(
        format!(
            "{} {}",
            icon,
            truncate_line_to_width(&label, content_width.saturating_sub(2).max(1))
        ),
        status_style,
    )));

    // Elapsed time
    if let Some(started) = summary.goal_started_at
        && lines.len() < max_rows
    {
        let elapsed = crate::tui::notifications::humanize_duration(started.elapsed());
        let elapsed_str = if summary.goal_completed {
            format!("completed in {elapsed}")
        } else {
            format!("elapsed: {elapsed}")
        };
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&elapsed_str, content_width),
            Style::default().fg(theme.text_muted),
        )));
    }

    if let Some(budget) = summary.goal_token_budget
        && lines.len() < max_rows
    {
        let pct = if budget > 0 {
            ((summary.tokens_used as f64 / budget as f64) * 100.0).min(100.0)
        } else {
            0.0
        };
        let bar_width = content_width.min(20);
        let filled = ((pct / 100.0) * bar_width as f64) as usize;
        let bar = format!(
            "[{}{}] {:.0}%",
            "█".repeat(filled),
            "░".repeat(bar_width.saturating_sub(filled)),
            pct
        );
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(
                &format!("tokens: {}/{} {}", summary.tokens_used, budget, bar),
                content_width,
            ),
            Style::default().fg(theme.text_muted),
        )));
    }
}

pub(crate) fn push_work_checklist_lines(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
    lines: &mut Vec<Line<'static>>,
    theme: &palette::UiTheme,
) {
    if summary.checklist_items.is_empty() || lines.len() >= max_rows {
        return;
    }

    let total = summary.checklist_items.len();
    let completed = summary
        .checklist_items
        .iter()
        .filter(|item| item.status == TodoStatus::Completed)
        .count();
    lines.push(Line::from(vec![
        Span::styled(
            format!("{}%", summary.checklist_completion_pct),
            Style::default().fg(theme.success).bold(),
        ),
        Span::styled(
            format!(" complete ({completed}/{total})"),
            Style::default().fg(theme.text_muted),
        ),
    ]));

    let reserve_for_strategy = if has_renderable_strategy(summary) {
        2
    } else {
        0
    };
    let available_item_rows = max_rows
        .saturating_sub(lines.len())
        .saturating_sub(reserve_for_strategy)
        .min(summary.checklist_items.len());
    let max_items =
        if summary.checklist_items.len() > available_item_rows && available_item_rows > 1 {
            available_item_rows - 1
        } else {
            available_item_rows
        };
    let start = checklist_window_start(&summary.checklist_items, max_items);
    let end = start
        .saturating_add(max_items)
        .min(summary.checklist_items.len());
    for item in summary.checklist_items[start..end].iter() {
        let (prefix, color) = match item.status {
            TodoStatus::Pending => ("[ ]", theme.text_muted),
            TodoStatus::InProgress => ("[~]", theme.warning),
            TodoStatus::Completed => ("[✓]", theme.success),
        };
        let text = format!("{prefix} #{} {}", item.id, item.content);
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&text, content_width),
            Style::default().fg(color),
        )));
    }

    let earlier = start;
    let later = summary.checklist_items.len().saturating_sub(end);
    let remaining = earlier.saturating_add(later);
    if remaining > 0 && lines.len() < max_rows {
        let label = match (earlier, later) {
            (0, later) => format!("+{later} more checklist items"),
            (earlier, 0) => format!("+{earlier} earlier checklist items"),
            (earlier, later) => format!("+{earlier} earlier, +{later} later"),
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(theme.text_muted),
        )));
    }
}

pub(crate) fn checklist_window_start(
    items: &[SidebarWorkChecklistItem],
    max_items: usize,
) -> usize {
    if max_items >= items.len() {
        return 0;
    }
    let Some(active_idx) = items
        .iter()
        .position(|item| item.status == TodoStatus::InProgress)
    else {
        return 0;
    };
    active_idx
        .saturating_sub(max_items / 2)
        .min(items.len().saturating_sub(max_items))
}

pub(crate) fn push_work_strategy_lines(
    summary: &SidebarWorkSummary,
    content_width: usize,
    max_rows: usize,
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
) {
    if !has_renderable_strategy(summary) || lines.len() >= max_rows {
        return;
    }

    let checklist_is_primary = summary.checklist_is_primary();
    let strategy_steps = renderable_strategy_steps(summary);
    if !checklist_is_primary && !summary.strategy_steps.is_empty() {
        let (pending, in_progress, completed) = summary.strategy_counts();
        let total = pending + in_progress + completed;
        lines.push(Line::from(vec![
            Span::styled(
                "Strategy metadata ",
                Style::default().fg(theme.plan_summary_color).bold(),
            ),
            Span::styled(
                format!("{}%", summary.strategy_progress_percent()),
                Style::default().fg(theme.plan_progress_color).bold(),
            ),
            Span::styled(
                format!(" complete ({completed}/{total})"),
                Style::default().fg(theme.plan_summary_color),
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            work_strategy_context_label(summary),
            Style::default().fg(theme.plan_summary_color).bold(),
        )));
    }

    if let Some(explanation) = summary.strategy_explanation.as_deref()
        && lines.len() < max_rows
    {
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(explanation, content_width),
            Style::default().fg(theme.plan_explanation_color),
        )));
    }

    let max_steps = max_rows
        .saturating_sub(lines.len())
        .min(strategy_steps.len());
    let remaining = strategy_steps.len().saturating_sub(max_steps);
    for step in strategy_steps.into_iter().take(max_steps) {
        let (prefix, color) = match step.status {
            StepStatus::Pending => ("[ ]", theme.plan_pending_color),
            StepStatus::InProgress => ("[~]", theme.plan_in_progress_color),
            StepStatus::Completed => ("[✓]", theme.plan_completed_color),
        };
        let (text_prefix, color) = if checklist_is_primary {
            (
                strategy_context_step_prefix(&step.status),
                strategy_context_step_color(&step.status, theme),
            )
        } else {
            (prefix, color)
        };
        let mut text = format!("{text_prefix} {}", step.text);
        if !step.elapsed.is_empty() {
            let _ = write!(text, " ({})", step.elapsed);
        }
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&text, content_width),
            Style::default().fg(color),
        )));
    }

    if remaining > 0 && lines.len() < max_rows {
        lines.push(Line::from(Span::styled(
            format!("+{remaining} more strategy steps"),
            Style::default().fg(theme.plan_summary_color),
        )));
    }
}

pub(crate) fn work_strategy_context_label(summary: &SidebarWorkSummary) -> &'static str {
    if summary.checklist_is_primary() {
        "Strategy context"
    } else {
        "Strategy metadata"
    }
}

pub(crate) fn strategy_context_step_prefix(status: &StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "phase next:",
        StepStatus::InProgress => "phase now:",
        StepStatus::Completed => "phase done:",
    }
}

pub(crate) fn strategy_context_step_color(
    status: &StepStatus,
    theme: &Theme,
) -> ratatui::style::Color {
    match status {
        StepStatus::Pending => theme.plan_pending_color,
        StepStatus::InProgress => theme.plan_in_progress_color,
        StepStatus::Completed => theme.plan_summary_color,
    }
}

#[must_use]
pub(crate) fn work_panel_empty_hint(content_width: usize) -> String {
    truncate_line_to_width("No active work", content_width)
}
