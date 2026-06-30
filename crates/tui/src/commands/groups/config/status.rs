//! Runtime status command.

use std::fmt::Write as _;
use std::path::Path;

use super::CommandResult;
use crate::compaction::estimate_input_tokens_conservative;
use crate::tui::app::App;
use crate::utils::{display_path, estimate_message_chars};

/// Show a compact runtime status report for the current TUI session.
pub fn status(app: &mut App) -> CommandResult {
    CommandResult::message(format_status(app))
}

fn format_status(app: &App) -> String {
    let mut out = String::new();
    let (context_used, context_max, context_percent) = context_usage(app);

    let _ = writeln!(out, "mimofan Status");
    let _ = writeln!(out, "===================");
    let _ = writeln!(out);
    push_row(&mut out, "Version:", env!("CARGO_PKG_VERSION"));
    push_row(&mut out, "Provider:", app.api_provider.as_str());
    push_row(
        &mut out,
        "Model:",
        &format!(
            "{} (reasoning {})",
            app.model_display_label(),
            app.reasoning_effort_display_label()
        ),
    );
    push_row(&mut out, "Directory:", &display_path(&app.workspace));
    push_row(&mut out, "Mode:", app.mode.label());
    push_row(&mut out, "Permissions:", &permission_summary(app));
    push_row(&mut out, "Project docs:", &project_docs(&app.workspace));
    push_row(
        &mut out,
        "Session:",
        app.current_session_id.as_deref().unwrap_or("not saved yet"),
    );
    push_row(
        &mut out,
        "MCP:",
        &format!("{} configured", app.mcp_configured_count),
    );
    push_row(&mut out, "Footer items:", &footer_items(app));
    let _ = writeln!(out);
    push_row(
        &mut out,
        "Context window:",
        &format!("{context_percent:.1}% used ({context_used} / {context_max} tokens)"),
    );
    push_row(
        &mut out,
        "Last API input:",
        &token_count(app.session.last_prompt_tokens),
    );
    push_row(
        &mut out,
        "Last API output:",
        &token_count(app.session.last_completion_tokens),
    );
    push_row(&mut out, "Cache hit/miss:", &cache_summary(app));
    push_row(
        &mut out,
        "Session input:",
        &app.session.total_input_tokens.to_string(),
    );
    let session_cache =
        if app.session.total_cache_hit_tokens == 0 && app.session.total_cache_miss_tokens == 0 {
            "not reported".to_string()
        } else {
            format!(
                "{} hit / {} miss",
                app.session.total_cache_hit_tokens, app.session.total_cache_miss_tokens
            )
        };
    push_row(&mut out, "Session cache:", &session_cache);
    push_row(
        &mut out,
        "Session output:",
        &app.session.total_output_tokens.to_string(),
    );
    push_row(
        &mut out,
        "Total tokens:",
        &app.session.total_tokens.to_string(),
    );
    push_row(
        &mut out,
        "Session cost:",
        &app.format_cost_amount_precise(app.session_cost_for_currency(app.cost_currency)),
    );
    push_row(
        &mut out,
        "Transcript:",
        &format!(
            "{} cells, {} API messages",
            app.history.len(),
            app.api_messages.len()
        ),
    );
    let tool_output_status =
        crate::tool_output_receipts::tool_output_status(&app.api_messages, &app.session_artifacts);
    push_row(
        &mut out,
        "Tool outputs:",
        &crate::tool_output_receipts::format_tool_output_status(&tool_output_status),
    );
    push_row(
        &mut out,
        "Rate limits:",
        "not available from provider telemetry",
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Use /statusline to configure footer items.");

    out
}

fn push_row(out: &mut String, label: &str, value: &str) {
    let _ = writeln!(out, "  {label:<16} {value}");
}

fn permission_summary(app: &App) -> String {
    let trust = if app.trust_mode {
        "trusted workspace"
    } else {
        "workspace"
    };
    let shell = if app.allow_shell {
        "shell on"
    } else {
        "shell off"
    };
    format!(
        "{trust}, approvals {}, {shell}",
        app.approval_mode.label().to_ascii_lowercase()
    )
}

fn project_docs(workspace: &Path) -> String {
    let docs: Vec<&str> = ["AGENTS.md", "CLAUDE.md"]
        .into_iter()
        .filter(|name| workspace.join(name).is_file())
        .collect();
    if docs.is_empty() {
        "not found".to_string()
    } else {
        docs.join(", ")
    }
}

fn footer_items(app: &App) -> String {
    if app.status_items.is_empty() {
        return "none".to_string();
    }
    app.status_items
        .iter()
        .map(|item| item.key())
        .collect::<Vec<_>>()
        .join(", ")
}

fn context_usage(app: &App) -> (usize, u32, f64) {
    let max = crate::route_budget::route_context_window_tokens(
        app.api_provider,
        app.effective_model_for_budget(),
        app.active_route_limits,
    );
    let estimated =
        estimate_input_tokens_conservative(&app.api_messages, app.system_prompt.as_ref());
    let total_chars = estimate_message_chars(&app.api_messages);
    let used = estimated.max(total_chars / 4);
    let percent = ((used as f64 / f64::from(max)) * 100.0).clamp(0.0, 100.0);
    (used, max, percent)
}

fn token_count(value: Option<u32>) -> String {
    value.map_or_else(|| "not reported".to_string(), |tokens| tokens.to_string())
}

fn cache_summary(app: &App) -> String {
    match (
        app.session.last_prompt_cache_hit_tokens,
        app.session.last_prompt_cache_miss_tokens,
    ) {
        (Some(hit), Some(miss)) => format!("{hit} hit / {miss} miss"),
        (Some(hit), None) => format!("{hit} hit / miss not reported"),
        (None, Some(miss)) => format!("hit not reported / {miss} miss"),
        (None, None) => "not reported".to_string(),
    }
}

#[cfg(test)]
mod tests {}
