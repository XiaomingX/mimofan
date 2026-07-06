//! Diagnostic prompt source map for context pressure reports.
//!
//! The report is intentionally approximate for v0.8.59. It uses the same
//! conservative token heuristic as compaction and describes the runtime sources
//! mimofan already tracks, without claiming provider-tokenizer parity.

use std::fmt::Write as _;
use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;

use mimofan_config::route::RouteLimits;

use crate::compaction::{estimate_input_tokens_conservative, estimate_text_tokens_conservative};
use crate::config::{ApiProvider, Config, provider_capability};
use crate::context_budget::PressureLevel;
use crate::models::{ContentBlock, Message};
use crate::prompts::{COMPACT_TEMPLATE, Personality};
use crate::route_budget::route_context_window_tokens;
use crate::tui::app::App;

#[derive(Debug, Clone, Serialize)]
pub struct PromptSourceMap {
    pub entries: Vec<SourceEntry>,
    pub total_estimated_tokens: usize,
    pub active_context_estimated_tokens: usize,
    pub context_window_tokens: Option<u32>,
    pub budget_used_percent: Option<f64>,
    pub generated_at: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceEntry {
    pub source_kind: SourceKind,
    pub label: String,
    pub source_path: Option<String>,
    pub activation_reason: ActivationReason,
    pub estimated_tokens: usize,
    pub counting_confidence: CountingConfidence,
    pub authority_tier: Option<u8>,
    pub truncation_reason: Option<String>,
}

impl SourceEntry {
    fn text(
        source_kind: SourceKind,
        label: impl Into<String>,
        source_path: Option<String>,
        activation_reason: ActivationReason,
        text: &str,
        counting_confidence: CountingConfidence,
        authority_tier: Option<u8>,
    ) -> Self {
        Self::estimate(
            source_kind,
            label,
            source_path,
            activation_reason,
            estimate_text_tokens_conservative(text),
            counting_confidence,
            authority_tier,
        )
    }

    fn estimate(
        source_kind: SourceKind,
        label: impl Into<String>,
        source_path: Option<String>,
        activation_reason: ActivationReason,
        estimated_tokens: usize,
        counting_confidence: CountingConfidence,
        authority_tier: Option<u8>,
    ) -> Self {
        Self {
            source_kind,
            label: label.into(),
            source_path,
            activation_reason,
            estimated_tokens,
            counting_confidence,
            authority_tier,
            truncation_reason: None,
        }
    }

    fn omitted(
        source_kind: SourceKind,
        label: impl Into<String>,
        source_path: Option<String>,
        authority_tier: Option<u8>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            source_kind,
            label: label.into(),
            source_path,
            activation_reason: ActivationReason::Omitted,
            estimated_tokens: 0,
            counting_confidence: CountingConfidence::High,
            authority_tier,
            truncation_reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Constitution,
    ProjectContext,
    ProjectContextPack,
    SkillsBlock,
    ContextManagement,
    CompactionRelayTemplate,
    RuntimePolicy,
    EnvironmentBlock,
    UserMemory,
    SessionGoal,
    HandoffRelay,
    ToolSchemas,
    UserRequest,
    ConversationHistory,
    ToolResult,
    ModelProviderFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationReason {
    AlwaysOn,
    FilePresent,
    ConfigEnabled,
    RuntimeState,
    PerRequest,
    Omitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CountingConfidence {
    High,
    Approximate,
}

struct ReportBuilder {
    entries: Vec<SourceEntry>,
}

impl ReportBuilder {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn push(&mut self, entry: SourceEntry) {
        self.entries.push(entry);
    }

    fn finish(
        self,
        provider: ApiProvider,
        model: &str,
        route_limits: Option<RouteLimits>,
        active_context_estimated_tokens: usize,
        note: impl Into<String>,
    ) -> PromptSourceMap {
        let total_estimated_tokens = self
            .entries
            .iter()
            .map(|entry| entry.estimated_tokens)
            .sum();
        // Overlay the resolved route's context window when known, falling back
        // to the provider+model capability matrix (route_context_window_tokens
        // always yields a concrete value, so this is never None at runtime).
        let context_window_tokens =
            Some(route_context_window_tokens(provider, model, route_limits));
        let budget_used_percent = context_window_tokens.map(|window| {
            ((active_context_estimated_tokens as f64 / f64::from(window)) * 100.0).clamp(0.0, 100.0)
        });
        PromptSourceMap {
            entries: self.entries,
            total_estimated_tokens,
            active_context_estimated_tokens,
            context_window_tokens,
            budget_used_percent,
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            note: note.into(),
        }
    }
}

pub fn build_context_report(app: &App) -> PromptSourceMap {
    let mut builder = base_source_entries(&app.model, &app.workspace, Some(&app.skills_dir));
    add_app_runtime_entries(&mut builder, app);
    let active_context_estimated_tokens =
        estimate_input_tokens_conservative(&app.api_messages, app.system_prompt.as_ref());
    builder.finish(
        app.api_provider,
        &app.model,
        app.active_route_limits,
        active_context_estimated_tokens,
        "Diagnostic source map. Token counts are conservative estimates and may differ from provider billing.",
    )
}

pub fn build_headless_context_report(config: &Config, workspace: &Path) -> PromptSourceMap {
    let model = config.default_model();
    let global_skills_dir = config.skills_dir();
    let selected_skills_dir =
        crate::tui::app::resolve_skills_dir(workspace, &global_skills_dir, config);
    let mut builder = base_source_entries(&model, workspace, Some(&selected_skills_dir));
    let memory_path = config.memory_path();

    if let Some(memory_block) = crate::memory::compose_block(config.memory_enabled(), &memory_path)
    {
        builder.push(SourceEntry::text(
            SourceKind::UserMemory,
            "User memory",
            Some(memory_path.display().to_string()),
            ActivationReason::ConfigEnabled,
            &memory_block,
            CountingConfidence::High,
            Some(6),
        ));
    } else {
        builder.push(SourceEntry::omitted(
            SourceKind::UserMemory,
            "User memory",
            Some(memory_path.display().to_string()),
            Some(6),
            "disabled, missing, or empty",
        ));
    }

    builder.push(SourceEntry::text(
        SourceKind::ModelProviderFact,
        format!("Provider facts ({})", config.api_provider().as_str()),
        None,
        ActivationReason::RuntimeState,
        &format!(
            "provider: {}\nmodel: {}\ncontext_window: {}",
            config.api_provider().as_str(),
            model,
            // Route limits aren't resolved in the headless doctor path, so report
            // the provider+model capability window (route overlay is unavailable).
            provider_capability(config.api_provider(), &model).context_window
        ),
        CountingConfidence::Approximate,
        None,
    ));

    let active_context_estimated_tokens = builder
        .entries
        .iter()
        .map(|entry| entry.estimated_tokens)
        .sum();
    builder.finish(
        config.api_provider(),
        &model,
        // Route limits aren't resolved in the headless doctor path.
        None,
        active_context_estimated_tokens,
        "Headless diagnostic source map. Conversation, tool results, and live TUI state are unavailable in doctor mode.",
    )
}

fn base_source_entries(model: &str, workspace: &Path, skills_dir: Option<&Path>) -> ReportBuilder {
    let mut builder = ReportBuilder::new();

    let constitution =
        crate::prompts::compose_prompt_with_approval_model_and_shell(Personality::Calm, model);
    builder.push(SourceEntry::text(
        SourceKind::Constitution,
        "Constitution and static prompt",
        Some("crates/tui/src/prompts/constitution.md".to_string()),
        ActivationReason::AlwaysOn,
        &constitution,
        CountingConfidence::High,
        Some(1),
    ));

    let project_context = crate::project_context::load_project_context_with_parents(workspace);
    if let Some(block) = project_context.as_system_block() {
        builder.push(SourceEntry::text(
            SourceKind::ProjectContext,
            "Project context and repository instructions",
            Some(workspace.display().to_string()),
            ActivationReason::FilePresent,
            &block,
            CountingConfidence::High,
            Some(5),
        ));
    } else {
        builder.push(SourceEntry::omitted(
            SourceKind::ProjectContext,
            "Project context and repository instructions",
            Some(workspace.display().to_string()),
            Some(5),
            "no project context block available",
        ));
    }

    if let Some(pack) = crate::project_context::generate_project_context_pack(workspace) {
        builder.push(SourceEntry::text(
            SourceKind::ProjectContextPack,
            "Project context pack",
            Some(workspace.display().to_string()),
            ActivationReason::RuntimeState,
            &pack,
            CountingConfidence::Approximate,
            Some(5),
        ));
    }

    let skills_block = match skills_dir {
        Some(dir) => {
            crate::skills::render_available_skills_context_for_workspace_and_dir(workspace, dir)
        }
        None => crate::skills::render_available_skills_context_for_workspace(workspace),
    };
    if let Some(block) = skills_block {
        builder.push(SourceEntry::text(
            SourceKind::SkillsBlock,
            "Available skills",
            skills_dir.map(|path| path.display().to_string()),
            ActivationReason::FilePresent,
            &block,
            CountingConfidence::High,
            Some(5),
        ));
    } else {
        builder.push(SourceEntry::omitted(
            SourceKind::SkillsBlock,
            "Available skills",
            skills_dir.map(|path| path.display().to_string()),
            Some(5),
            "no skills discovered",
        ));
    }

    builder.push(SourceEntry::estimate(
        SourceKind::ContextManagement,
        "Context management guidance",
        None,
        ActivationReason::AlwaysOn,
        430,
        CountingConfidence::Approximate,
        Some(3),
    ));
    builder.push(SourceEntry::text(
        SourceKind::CompactionRelayTemplate,
        "Compaction relay template",
        Some("crates/tui/src/prompts/compact.md".to_string()),
        ActivationReason::AlwaysOn,
        COMPACT_TEMPLATE,
        CountingConfidence::High,
        Some(3),
    ));
    builder.push(SourceEntry::estimate(
        SourceKind::RuntimePolicy,
        "Runtime policy reference",
        None,
        ActivationReason::AlwaysOn,
        650,
        CountingConfidence::Approximate,
        Some(3),
    ));

    add_handoff_entry(&mut builder, workspace);
    builder
}

fn add_app_runtime_entries(builder: &mut ReportBuilder, app: &App) {
    builder.push(SourceEntry::text(
        SourceKind::EnvironmentBlock,
        "Runtime environment",
        Some(app.workspace.display().to_string()),
        ActivationReason::PerRequest,
        &format!(
            "workspace: {}\nmodel: {}\nprovider: {}\nmode: {}\napproval: {}",
            app.workspace.display(),
            app.model,
            app.api_provider.as_str(),
            app.mode.label(),
            app.approval_mode.label()
        ),
        CountingConfidence::Approximate,
        Some(4),
    ));

    if let Some(memory_block) = crate::memory::compose_block(app.use_memory, &app.memory_path) {
        builder.push(SourceEntry::text(
            SourceKind::UserMemory,
            "User memory",
            Some(app.memory_path.display().to_string()),
            ActivationReason::ConfigEnabled,
            &memory_block,
            CountingConfidence::High,
            Some(6),
        ));
    } else {
        builder.push(SourceEntry::omitted(
            SourceKind::UserMemory,
            "User memory",
            Some(app.memory_path.display().to_string()),
            Some(6),
            "disabled, missing, or empty",
        ));
    }

    if let Some(goal) = app
        .hunt
        .quarry
        .as_deref()
        .filter(|goal| !goal.trim().is_empty())
    {
        builder.push(SourceEntry::text(
            SourceKind::SessionGoal,
            "Session goal",
            None,
            ActivationReason::RuntimeState,
            goal,
            CountingConfidence::High,
            Some(6),
        ));
    } else {
        builder.push(SourceEntry::omitted(
            SourceKind::SessionGoal,
            "Session goal",
            None,
            Some(6),
            "no active /goal objective",
        ));
    }

    if let Some(tools) = app.session.last_tool_catalog.as_ref() {
        let rendered = serde_json::to_string(tools).unwrap_or_default();
        builder.push(SourceEntry::text(
            SourceKind::ToolSchemas,
            format!("Tool schemas ({} tools)", tools.len()),
            None,
            ActivationReason::PerRequest,
            &rendered,
            CountingConfidence::Approximate,
            Some(3),
        ));
    } else {
        builder.push(SourceEntry::omitted(
            SourceKind::ToolSchemas,
            "Tool schemas",
            None,
            Some(3),
            "no tool catalog has been sent yet",
        ));
    }

    add_message_entries(builder, &app.api_messages);
}

fn add_handoff_entry(builder: &mut ReportBuilder, workspace: &Path) {
    let primary = workspace.join(crate::prompts::HANDOFF_RELATIVE_PATH);
    let legacy = workspace.join(".mimofan/handoff.md");
    let path = if primary.exists() { primary } else { legacy };
    let Some(raw) = std::fs::read_to_string(&path)
        .ok()
        .filter(|raw| !raw.trim().is_empty())
    else {
        builder.push(SourceEntry::omitted(
            SourceKind::HandoffRelay,
            "Previous session relay",
            Some(
                workspace
                    .join(crate::prompts::HANDOFF_RELATIVE_PATH)
                    .display()
                    .to_string(),
            ),
            Some(6),
            "no relay artifact found",
        ));
        return;
    };

    builder.push(SourceEntry::text(
        SourceKind::HandoffRelay,
        "Previous session relay",
        Some(path.display().to_string()),
        ActivationReason::FilePresent,
        &raw,
        CountingConfidence::High,
        Some(6),
    ));
}

fn add_message_entries(builder: &mut ReportBuilder, messages: &[Message]) {
    if messages.is_empty() {
        builder.push(SourceEntry::omitted(
            SourceKind::ConversationHistory,
            "Conversation history",
            None,
            None,
            "no API messages yet",
        ));
        return;
    }

    let latest_user = messages.iter().rposition(|message| message.role == "user");
    let mut latest_user_tokens = 0usize;
    let mut conversation_tokens = 0usize;
    let mut tool_result_tokens = 0usize;
    let mut tool_result_count = 0usize;

    for (index, message) in messages.iter().enumerate() {
        for block in &message.content {
            let tokens = estimate_text_tokens_conservative(&content_block_text(block));
            match block {
                ContentBlock::ToolResult { .. }
                | ContentBlock::ToolSearchToolResult { .. }
                | ContentBlock::CodeExecutionToolResult { .. } => {
                    tool_result_tokens += tokens;
                    tool_result_count += 1;
                }
                ContentBlock::Text { .. } if Some(index) == latest_user => {
                    latest_user_tokens += tokens;
                }
                _ => {
                    conversation_tokens += tokens;
                }
            }
        }
    }

    if latest_user_tokens > 0 {
        builder.push(SourceEntry::estimate(
            SourceKind::UserRequest,
            "Latest user request",
            None,
            ActivationReason::PerRequest,
            latest_user_tokens,
            CountingConfidence::High,
            Some(7),
        ));
    }
    if conversation_tokens > 0 {
        builder.push(SourceEntry::estimate(
            SourceKind::ConversationHistory,
            "Conversation history",
            None,
            ActivationReason::RuntimeState,
            conversation_tokens,
            CountingConfidence::High,
            None,
        ));
    }
    if tool_result_count > 0 {
        builder.push(SourceEntry::estimate(
            SourceKind::ToolResult,
            format!("Tool results ({tool_result_count})"),
            None,
            ActivationReason::RuntimeState,
            tool_result_tokens,
            CountingConfidence::High,
            None,
        ));
    }
}

fn content_block_text(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text, .. } => text.clone(),
        ContentBlock::Thinking { thinking, .. } => thinking.clone(),
        ContentBlock::ToolResult { content, .. } => content.clone(),
        ContentBlock::ToolSearchToolResult { content, .. }
        | ContentBlock::CodeExecutionToolResult { content, .. } => content.to_string(),
        ContentBlock::ToolUse { input, .. } | ContentBlock::ServerToolUse { input, .. } => {
            input.to_string()
        }
        ContentBlock::ImageUrl { image_url } => image_url.url.clone(),
    }
}

fn pressure_label(percent: Option<f64>) -> &'static str {
    // Delegate to the unified pressure thresholds so this diagnostic label can't
    // drift from `context_budget::PressureLevel`. `None` (unknown window) keeps
    // its own sentinel since a level requires a usage percentage.
    match percent {
        Some(value) => PressureLevel::from_usage_percent(value).label(),
        None => "unknown",
    }
}

pub fn format_context_report(report: &PromptSourceMap) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Context Source Map");
    let _ = writeln!(
        out,
        "Estimated active context: {} tokens",
        report.active_context_estimated_tokens
    );
    match (report.context_window_tokens, report.budget_used_percent) {
        (Some(window), Some(percent)) => {
            let _ = writeln!(
                out,
                "Window: {window} tokens ({percent:.1}% used, {})",
                pressure_label(Some(percent))
            );
        }
        _ => {
            let _ = writeln!(out, "Window: unknown");
        }
    }
    let _ = writeln!(
        out,
        "Source-entry total: {} tokens",
        report.total_estimated_tokens
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Sources:");
    for entry in &report.entries {
        let path = entry
            .source_path
            .as_deref()
            .map(|path| format!(" [{path}]"))
            .unwrap_or_default();
        let tier = entry
            .authority_tier
            .map(|tier| format!(", tier {tier}"))
            .unwrap_or_default();
        let omitted = entry
            .truncation_reason
            .as_deref()
            .map(|reason| format!(" - {reason}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "- {:?}: {}{} - {} tokens ({:?}{}){}",
            entry.source_kind,
            entry.label,
            path,
            entry.estimated_tokens,
            entry.counting_confidence,
            tier,
            omitted
        );
    }
    let _ = writeln!(out);
    let _ = write!(out, "{}", report.note);
    out
}

pub fn format_context_summary(report: &PromptSourceMap) -> String {
    let mut entries = report.entries.clone();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.estimated_tokens));
    let top = entries
        .iter()
        .take(5)
        .map(|entry| format!("{} ({})", entry.label, entry.estimated_tokens))
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = String::new();
    let _ = writeln!(out, "Context Summary");
    let _ = writeln!(
        out,
        "Pressure: {}",
        pressure_label(report.budget_used_percent)
    );
    let _ = writeln!(
        out,
        "Estimated active context: {} tokens",
        report.active_context_estimated_tokens
    );
    if let Some(percent) = report.budget_used_percent {
        let _ = writeln!(out, "Budget used: {percent:.1}%");
    }
    let _ = write!(out, "Top sources: {top}");
    out
}

pub fn context_report_json(report: &PromptSourceMap) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|err| {
        format!("{{\"error\":\"failed to serialize context report: {err}\"}}")
    })
}

#[cfg(test)]
mod tests {}
