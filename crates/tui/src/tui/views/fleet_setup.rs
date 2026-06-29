//! Fleet setup and loadout planner.
//!
//! NOTE (audit #7 / #3167): the modal title, footer hints, and the lane/row
//! taxonomy below are intentionally English for now. #3167 reworks this view
//! from a read-only summary into an interactive provider/model picker, which
//! will churn most of this text; localizing the ~90 volatile technical strings
//! into all shipped locales before that lands would be throwaway work. The
//! command entry (`CmdFleetDescription`) is already localized, and the
//! functional selection wiring (audit #8) is handled here regardless of locale.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
};

use crate::config::Config;
use crate::palette;
use crate::tui::app::App;
use crate::tui::views::{
    CommandPaletteAction, ModalKind, ModalView, ViewAction, ViewEvent, truncate_view_text,
};

const PROFILE_DIR: &str = ".mimofan/agents";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowTone {
    Current,
    Ready,
    Info,
    Warning,
}

impl RowTone {
    fn style(self, selected: bool) -> Style {
        if selected {
            return Style::default()
                .fg(palette::SELECTION_TEXT)
                .bg(palette::SELECTION_BG)
                .add_modifier(Modifier::BOLD);
        }

        Style::default().fg(match self {
            Self::Current => palette::WHALE_ACCENT_PRIMARY,
            Self::Ready => palette::STATUS_SUCCESS,
            Self::Info => palette::TEXT_PRIMARY,
            Self::Warning => palette::STATUS_WARNING,
        })
    }
}

#[derive(Debug, Clone)]
struct FleetSetupRow {
    label: String,
    value: String,
    detail: String,
    tone: RowTone,
}

impl FleetSetupRow {
    fn new(label: impl Into<String>, value: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            detail: detail.into(),
            tone: RowTone::Info,
        }
    }

    fn tone(mut self, tone: RowTone) -> Self {
        self.tone = tone;
        self
    }
}

#[derive(Debug, Clone)]
struct FleetSetupLane {
    title: &'static str,
    subtitle: &'static str,
    rows: Vec<FleetSetupRow>,
}

#[derive(Debug, Clone)]
pub struct FleetSetupSnapshot {
    workspace: PathBuf,
    provider: String,
    model: String,
    reasoning: String,
    subagents_enabled: bool,
    max_subagents: usize,
    launch_concurrency: usize,
    max_admitted: usize,
    subagent_spawn_depth: u32,
    fleet_spawn_depth: u32,
    token_budget: Option<u64>,
    api_timeout_secs: u64,
    heartbeat_timeout_secs: u64,
}

impl FleetSetupSnapshot {
    #[must_use]
    pub fn from_app(app: &App, config: &Config) -> Self {
        let provider = app.api_provider.display_name().to_string();
        let model = if app.auto_model {
            app.last_effective_model
                .as_deref()
                .map(|effective| format!("auto -> {effective}"))
                .unwrap_or_else(|| "auto".to_string())
        } else {
            app.model.clone()
        };
        let fleet_spawn_depth = config
            .fleet
            .as_ref()
            .map(|fleet| fleet.exec.max_spawn_depth)
            .unwrap_or_else(|| mimofan_config::FleetExecConfig::default().max_spawn_depth)
            .min(mimofan_config::MAX_SPAWN_DEPTH_CEILING);

        Self {
            workspace: app.workspace.clone(),
            provider,
            model,
            reasoning: app.reasoning_effort_display_label(),
            subagents_enabled: config.subagents_enabled_for_provider(app.api_provider),
            max_subagents: config.max_subagents_for_provider(app.api_provider),
            launch_concurrency: config.launch_concurrency_for_provider(app.api_provider),
            max_admitted: config.max_admitted_subagents_for_provider(app.api_provider),
            subagent_spawn_depth: config.subagent_max_spawn_depth_for_provider(app.api_provider),
            fleet_spawn_depth,
            token_budget: config.subagent_token_budget_for_provider(app.api_provider),
            api_timeout_secs: config.subagent_api_timeout_secs_for_provider(app.api_provider),
            heartbeat_timeout_secs: config
                .subagent_heartbeat_timeout_secs_for_provider(app.api_provider),
        }
    }
}

/// Lane index of the role picker (lane "1 Role").
const ROLE_LANE: usize = 0;
/// Lane index of the model-class picker (lane "2 Model").
const MODEL_LANE: usize = 1;

pub struct FleetSetupView {
    lanes: Vec<FleetSetupLane>,
    selected_lane: usize,
    selected_rows: Vec<usize>,
    scrolls: Vec<usize>,
    // The route context the profile prompt is generated against. The prompt
    // itself is built on demand from the *current* role/model selection (see
    // `insert_profile_prompt_action`) so the planner selection has a functional
    // outcome.
    snapshot: FleetSetupSnapshot,
}

impl FleetSetupView {
    #[must_use]
    pub fn new(app: &App, config: &Config) -> Self {
        Self::from_snapshot(FleetSetupSnapshot::from_app(app, config))
    }

    fn from_snapshot(snapshot: FleetSetupSnapshot) -> Self {
        let lanes = build_lanes(&snapshot);
        let len = lanes.len();
        Self {
            lanes,
            selected_lane: 0,
            selected_rows: vec![0; len],
            scrolls: vec![0; len],
            snapshot,
        }
    }

    /// Label of the row currently selected in `lane`, if any.
    fn selected_label(&self, lane: usize) -> Option<&str> {
        let row = self.selected_rows.get(lane).copied().unwrap_or_default();
        self.lanes
            .get(lane)
            .and_then(|lane| lane.rows.get(row))
            .map(|row| row.label.as_str())
    }

    /// The planner role chosen in the Role lane (drives the profile file name
    /// and `role_hint`). Falls back to `custom` when unresolved.
    fn selected_role(&self) -> &str {
        self.selected_label(ROLE_LANE).unwrap_or("custom")
    }

    /// The model class chosen in the Model lane, mapped to a profile schema
    /// `model_class_hint` value.
    fn selected_model_class(&self) -> &'static str {
        model_class_hint(self.selected_label(MODEL_LANE).unwrap_or("inherit"))
    }

    fn selected_row(&self) -> usize {
        self.selected_rows
            .get(self.selected_lane)
            .copied()
            .unwrap_or_default()
    }

    fn selected_row_mut(&mut self) -> &mut usize {
        &mut self.selected_rows[self.selected_lane]
    }

    fn selected_scroll_mut(&mut self) -> &mut usize {
        &mut self.scrolls[self.selected_lane]
    }

    fn move_lane_left(&mut self) {
        self.selected_lane = self.selected_lane.saturating_sub(1);
    }

    fn move_lane_right(&mut self) {
        if self.selected_lane + 1 < self.lanes.len() {
            self.selected_lane += 1;
        }
    }

    fn move_row_up(&mut self) {
        *self.selected_row_mut() = self.selected_row().saturating_sub(1);
        *self.selected_scroll_mut() = self.selected_row();
    }

    fn move_row_down(&mut self) {
        let max = self
            .lanes
            .get(self.selected_lane)
            .map(|lane| lane.rows.len().saturating_sub(1))
            .unwrap_or_default();
        let next = (self.selected_row() + 1).min(max);
        *self.selected_row_mut() = next;
        *self.selected_scroll_mut() = next.saturating_sub(4);
    }

    fn insert_profile_prompt_action(&self) -> ViewAction {
        ViewAction::EmitAndClose(ViewEvent::CommandPaletteSelected {
            action: CommandPaletteAction::InsertText {
                text: self.profile_prompt(),
            },
        })
    }

    /// Build the profile authoring prompt for the *current* role/model
    /// selection. Re-evaluated each time so navigating the planner changes what
    /// `g`/Enter inserts.
    fn profile_prompt(&self) -> String {
        profile_authoring_prompt(
            &self.snapshot,
            self.selected_role(),
            self.selected_model_class(),
        )
    }
}

impl ModalView for FleetSetupView {
    fn kind(&self) -> ModalKind {
        ModalKind::FleetSetup
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ViewAction::Close,
            KeyCode::Left | KeyCode::Char('h') => {
                self.move_lane_left();
                ViewAction::None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.move_lane_right();
                ViewAction::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_row_up();
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_row_down();
                ViewAction::None
            }
            KeyCode::Enter | KeyCode::Char('g') | KeyCode::Char('G') => {
                self.insert_profile_prompt_action()
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = area.width.saturating_sub(4).clamp(72, 116).min(area.width);
        let popup_height = area.height.saturating_sub(4).clamp(18, 38).min(area.height);
        let popup_area = Rect {
            x: area.x + area.width.saturating_sub(popup_width) / 2,
            y: area.y + area.height.saturating_sub(popup_height) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let block = Block::default()
            .title(Line::from(Span::styled(
                " Fleet Setup ",
                Style::default()
                    .fg(palette::WHALE_ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(vec![
                Span::styled(" Left/Right ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("lane "),
                Span::styled(" Up/Down ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("option "),
                Span::styled(" Enter/G ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("profile prompt "),
                Span::styled(" Esc ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("close "),
            ]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::DEEPSEEK_INK))
            .padding(Padding::uniform(1));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        let direction = if inner.width >= 94 {
            Direction::Horizontal
        } else {
            Direction::Vertical
        };
        let constraints = vec![Constraint::Ratio(1, self.lanes.len() as u32); self.lanes.len()];
        let areas = Layout::default()
            .direction(direction)
            .constraints(constraints)
            .split(inner);

        for (idx, lane) in self.lanes.iter().enumerate() {
            let focused = idx == self.selected_lane;
            let scroll = self.scrolls.get(idx).copied().unwrap_or_default();
            let selected = self.selected_rows.get(idx).copied().unwrap_or_default();
            render_lane(lane, areas[idx], buf, focused, selected, scroll);
        }
    }
}

fn build_lanes(snapshot: &FleetSetupSnapshot) -> Vec<FleetSetupLane> {
    let (profile_value, profile_detail) = profile_file_status(&snapshot.workspace);
    let token_budget = snapshot
        .token_budget
        .map(|budget| format!("{budget} tokens"))
        .unwrap_or_else(|| "unbounded".to_string());

    vec![
        FleetSetupLane {
            title: "1 Role",
            subtitle: "role and persona",
            rows: vec![
                FleetSetupRow::new("manager", "plan/split", "coordinates queued Fleet work"),
                FleetSetupRow::new("main", "orchestrator", "default parent for the whole Fleet")
                    .tone(RowTone::Ready),
                FleetSetupRow::new("scout", "read-first", "research and repo reconnaissance"),
                FleetSetupRow::new("builder", "write", "implements bounded changes"),
                FleetSetupRow::new("reviewer", "read-only", "checks regressions and tests"),
                FleetSetupRow::new("verifier", "test-runner", "runs focused validation"),
                FleetSetupRow::new("synthesizer", "reduce", "turns receipts into handoff state"),
                FleetSetupRow::new(
                    "custom",
                    "agent profile",
                    "workspace TOML can define posture",
                )
                .tone(RowTone::Current),
            ],
        },
        FleetSetupLane {
            title: "2 Model",
            subtitle: "class and route intent",
            rows: vec![
                FleetSetupRow::new(
                    "current route",
                    format!("{} / {}", snapshot.provider, snapshot.model),
                    format!("reasoning {}", snapshot.reasoning),
                )
                .tone(RowTone::Current),
                FleetSetupRow::new("inherit", "session route", "reuse active provider/model"),
                FleetSetupRow::new("fast", "scout", "low-latency fanout and summaries"),
                FleetSetupRow::new("balanced", "default", "normal build/review work")
                    .tone(RowTone::Ready),
                FleetSetupRow::new("strong", "hard", "security, release, architecture"),
                FleetSetupRow::new(
                    "fixed model",
                    "profile model",
                    "visible model id on active route",
                )
                .tone(RowTone::Current),
                FleetSetupRow::new("deep-reasoning", "debug", "higher reasoning when supported"),
                FleetSetupRow::new("tool-heavy", "operator", "shell and artifact workflows"),
            ],
        },
        FleetSetupLane {
            title: "3 Permission",
            subtitle: "authority posture",
            rows: vec![
                FleetSetupRow::new(
                    "parent envelope",
                    "inherit+narrow",
                    "children cannot widen approval, trust, or secrets",
                )
                .tone(RowTone::Ready),
                FleetSetupRow::new("reviewer", "read-only", "default for review/plan/scout")
                    .tone(RowTone::Ready),
                FleetSetupRow::new("builder", "scoped write", "write only inside task bounds"),
                FleetSetupRow::new(
                    "approval",
                    "required",
                    "profiles cannot disable required approvals",
                )
                .tone(RowTone::Ready),
                FleetSetupRow::new(
                    "custom profile",
                    "no grants",
                    "TOML may narrow posture but not expand it",
                ),
            ],
        },
        FleetSetupLane {
            title: "4 Tools",
            subtitle: "capability loadout",
            rows: vec![
                FleetSetupRow::new("workspace files", profile_value, profile_detail)
                    .tone(RowTone::Ready),
                FleetSetupRow::new(
                    "custom profile",
                    "generate TOML",
                    "Enter inserts a safe authoring prompt",
                )
                .tone(RowTone::Current),
                FleetSetupRow::new("read tools", "default", "search, inspect, summarize")
                    .tone(RowTone::Ready),
                FleetSetupRow::new(
                    "write tools",
                    "builder only",
                    "edit/apply patch after scope",
                ),
                FleetSetupRow::new("shell", "policy gated", "allowed by parent/runtime posture"),
                FleetSetupRow::new(
                    "artifacts",
                    "receipts",
                    "logs and handoff data stay inspectable",
                ),
            ],
        },
        FleetSetupLane {
            title: "5 Org",
            subtitle: "team and recursion",
            rows: vec![
                FleetSetupRow::new(
                    "role workers",
                    if snapshot.subagents_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    format!(
                        "{} concurrent, {} launch slots, {} admitted",
                        snapshot.max_subagents, snapshot.launch_concurrency, snapshot.max_admitted
                    ),
                )
                .tone(if snapshot.subagents_enabled {
                    RowTone::Ready
                } else {
                    RowTone::Warning
                }),
                FleetSetupRow::new(
                    "recursion",
                    format!(
                        "agent {} / fleet {}",
                        snapshot.subagent_spawn_depth, snapshot.fleet_spawn_depth
                    ),
                    format!("ceiling {}", mimofan_config::MAX_SPAWN_DEPTH_CEILING),
                )
                .tone(RowTone::Ready),
                FleetSetupRow::new(
                    "starter team",
                    "3 scout + 1 each",
                    "builder, reviewer, verifier, synthesizer, operator",
                )
                .tone(RowTone::Ready),
                FleetSetupRow::new(
                    "scout tree",
                    "3 scouts",
                    "recursive exploration splits breadth-first",
                ),
                FleetSetupRow::new(
                    "builder tree",
                    "builder+reviewer",
                    "implementation gets paired review by default",
                ),
                FleetSetupRow::new(
                    "verifier tree",
                    "verifier+reviewer",
                    "test evidence gets interpreted before handoff",
                ),
                FleetSetupRow::new(
                    "budget",
                    token_budget,
                    format!(
                        "{}s api, {}s heartbeat",
                        snapshot.api_timeout_secs, snapshot.heartbeat_timeout_secs
                    ),
                ),
                FleetSetupRow::new("retry/ledger", "Fleet", "durable receipts and inspection"),
            ],
        },
        FleetSetupLane {
            title: "6 Review",
            subtitle: "check and run",
            rows: vec![
                FleetSetupRow::new(
                    "runtime",
                    "Fleet -> exec",
                    "durable workers launch the headless runtime",
                )
                .tone(RowTone::Current),
                FleetSetupRow::new(
                    "status",
                    "/fleet status",
                    "compat /subagents opens the same worker view",
                )
                .tone(RowTone::Ready),
                FleetSetupRow::new(
                    "inspect",
                    "ledger",
                    "route, receipt, artifact, terminal state",
                ),
                FleetSetupRow::new(
                    "run spec",
                    "review first",
                    "confirm role/profile/loadout before launch",
                ),
                FleetSetupRow::new("handoff", "bounded", "summaries over raw transcript replay"),
            ],
        },
    ]
}

fn render_lane(
    lane: &FleetSetupLane,
    area: Rect,
    buf: &mut Buffer,
    focused: bool,
    selected: usize,
    scroll: usize,
) {
    let border = if focused {
        palette::WHALE_ACCENT_PRIMARY
    } else {
        palette::BORDER_COLOR
    };
    let block = Block::default()
        .title(Line::from(Span::styled(
            format!(" {} ", lane.title),
            Style::default()
                .fg(if focused {
                    palette::WHALE_ACCENT_PRIMARY
                } else {
                    palette::TEXT_MUTED
                })
                .add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(palette::DEEPSEEK_INK))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    block.render(area, buf);

    let width = inner.width.saturating_sub(1) as usize;
    let visible_rows = inner.height.saturating_sub(2) as usize;
    let max_scroll = lane.rows.len().saturating_sub(visible_rows);
    let scroll = scroll.min(max_scroll);

    let mut lines = Vec::with_capacity(visible_rows + 2);
    lines.push(Line::from(Span::styled(
        truncate_view_text(lane.subtitle, width),
        Style::default().fg(palette::TEXT_MUTED),
    )));

    for (row_idx, row) in lane.rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let is_selected = focused && row_idx == selected;
        let row_style = row.tone.style(is_selected);
        let muted_style = if is_selected {
            row_style
        } else {
            Style::default().fg(palette::TEXT_MUTED)
        };
        let pointer = if is_selected { ">" } else { " " };
        let summary = format!("{pointer} {}: {}", row.label, row.value);
        lines.push(Line::from(Span::styled(
            truncate_view_text(&summary, width),
            row_style,
        )));
        if inner.height >= 9 {
            lines.push(Line::from(Span::styled(
                truncate_view_text(&format!("  {}", row.detail), width),
                muted_style,
            )));
        }
    }

    Paragraph::new(lines).render(inner, buf);
}

fn profile_file_status(workspace: &Path) -> (String, String) {
    let dir = workspace.join(PROFILE_DIR);
    if !dir.exists() {
        return (
            "0 files".to_string(),
            format!("create {PROFILE_DIR}/*.toml"),
        );
    }
    if !dir.is_dir() {
        return (
            "blocked".to_string(),
            format!("{} is not a dir", dir.display()),
        );
    }

    let count = std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("toml"))
        .count();

    if count == 1 {
        ("1 file".to_string(), PROFILE_DIR.to_string())
    } else {
        (format!("{count} files"), PROFILE_DIR.to_string())
    }
}

/// Map a Model-lane row label to a profile-schema `model_class_hint` value.
/// Route-context rows (`current route`, `fixed model`) and anything unknown
/// resolve to `inherit` so the generated profile reuses the active route.
fn model_class_hint(label: &str) -> &'static str {
    match label {
        "fast" => "fast",
        "balanced" => "balanced",
        // "strong" = security/release/architecture work → the strongest schema class.
        "strong" => "deep-reasoning",
        "deep-reasoning" => "deep-reasoning",
        "tool-heavy" => "tool-heavy",
        _ => "inherit",
    }
}

/// Sanitize a planner role label into a safe TOML file stem.
fn profile_file_stem(role: &str) -> String {
    let stem: String = role
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let stem = stem.trim_matches('-').to_ascii_lowercase();
    if stem.is_empty() {
        "custom".to_string()
    } else {
        stem
    }
}

fn profile_authoring_prompt(
    snapshot: &FleetSetupSnapshot,
    role: &str,
    model_class: &str,
) -> String {
    let file_stem = profile_file_stem(role);
    format!(
        "Create a safe mimofan Fleet agent profile file for this workspace.\n\n\
         Selected planner role: {role}. Selected model class: {model_class}.\n\
         Target path: {PROFILE_DIR}/{file_stem}.toml\n\
         Current route context only: provider = {provider}, model = {model}, reasoning = {reasoning}\n\n\
         Write TOML using only this schema:\n\
         - name\n\
         - display_name\n\
         - description\n\
         - role_hint (set to \"{role}\")\n\
         - model_class_hint (set to \"{model_class}\"; one of inherit, fast, balanced, deep-reasoning, code, review, or tool-heavy)\n\
         - model (optional explicit model id on the active/resolved route; omit for loadout auto)\n\
         - [instructions].text\n\
         - [tools].posture = \"read-only\"\n\n\
         Do not include provider, base_url, api_key, auth, secrets, trust, allow_shell, or approval_required=false.\n\
         If model is present, keep it to a visible model id such as deepseek-v4-pro or glm-5.2.\n\
         Default operational shape:\n\
         - one main orchestrator profile manages the Fleet run\n\
         - starter team is 3 scout/explore workers plus 1 builder, 1 reviewer, 1 verifier, 1 synthesizer, and 1 operator\n\
         - scout recursion can split into 3 scout children\n\
         - builder recursion can split into builder + reviewer children\n\
         - verifier recursion can split into verifier + reviewer children\n\n\
         Keep the profile permission-narrowing and compatible with recursive Fleet role workers.",
        provider = snapshot.provider,
        model = snapshot.model,
        reasoning = snapshot.reasoning
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn snapshot() -> FleetSetupSnapshot {
        FleetSetupSnapshot {
            workspace: PathBuf::from("/tmp/mimofan-test-workspace"),
            provider: "DeepSeek".to_string(),
            model: "deepseek-v4-pro".to_string(),
            reasoning: "Auto".to_string(),
            subagents_enabled: true,
            max_subagents: 8,
            launch_concurrency: 3,
            max_admitted: 20,
            subagent_spawn_depth: 3,
            fleet_spawn_depth: 3,
            token_budget: Some(100_000),
            api_timeout_secs: 120,
            heartbeat_timeout_secs: 300,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn arrow_keys_move_across_lanes_and_rows() {
        let mut view = FleetSetupView::from_snapshot(snapshot());

        view.handle_key(key(KeyCode::Right));
        view.handle_key(key(KeyCode::Down));

        assert_eq!(view.selected_lane, 1);
        assert_eq!(view.selected_row(), 1);

        view.handle_key(key(KeyCode::Left));

        assert_eq!(view.selected_lane, 0);
        assert_eq!(view.selected_row(), 0);
    }

    #[test]
    fn enter_inserts_profile_authoring_prompt() {
        let mut view = FleetSetupView::from_snapshot(snapshot());

        let action = view.handle_key(key(KeyCode::Enter));

        match action {
            ViewAction::EmitAndClose(ViewEvent::CommandPaletteSelected {
                action: CommandPaletteAction::InsertText { text },
            }) => {
                // Default selection is the first Role row ("manager").
                assert!(text.contains("Target path: .mimofan/agents/manager.toml"));
                assert!(text.contains("role_hint (set to \"manager\")"));
                assert!(text.contains("provider = DeepSeek"));
                assert!(text.contains("model (optional explicit model id"));
                assert!(text.contains("Do not include provider, base_url"));
                assert!(text.contains("starter team is 3 scout/explore workers"));
            }
            other => panic!("expected profile prompt insertion, got {other:?}"),
        }
    }

    #[test]
    fn selected_role_and_model_class_drive_generated_profile() {
        let mut view = FleetSetupView::from_snapshot(snapshot());

        // Role lane: manager(0) main(1) scout(2) builder(3) ... → move to builder.
        view.handle_key(key(KeyCode::Down));
        view.handle_key(key(KeyCode::Down));
        view.handle_key(key(KeyCode::Down));
        // Model lane: current route(0) inherit(1) fast(2) ... → move right then to fast.
        view.handle_key(key(KeyCode::Right));
        view.handle_key(key(KeyCode::Down));
        view.handle_key(key(KeyCode::Down));

        let prompt = view.profile_prompt();
        assert!(
            prompt.contains("Target path: .mimofan/agents/builder.toml"),
            "selection should drive the profile file name; got: {prompt}"
        );
        assert!(
            prompt.contains("role_hint (set to \"builder\")"),
            "selection should drive role_hint; got: {prompt}"
        );
        assert!(
            prompt.contains("model_class_hint (set to \"fast\""),
            "selection should drive model_class_hint; got: {prompt}"
        );
        assert!(prompt.contains("Selected planner role: builder. Selected model class: fast."));
    }

    #[test]
    fn profile_prompt_uses_current_route_only_as_context() {
        let view = FleetSetupView::from_snapshot(snapshot());

        assert!(view.profile_prompt().contains("Current route context only"));
        assert!(view.profile_prompt().contains("permission-narrowing"));
        assert!(view.profile_prompt().contains("builder + reviewer"));
    }

    #[test]
    fn render_mentions_fleet_to_agent_bridge_and_recursion() {
        let view = FleetSetupView::from_snapshot(snapshot());
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 32));

        assert!(
            view.lanes
                .iter()
                .flat_map(|lane| lane.rows.iter())
                .any(|row| row.value == "Fleet -> exec")
        );
        assert!(
            view.lanes
                .iter()
                .flat_map(|lane| lane.rows.iter())
                .any(|row| row.label == "starter team" && row.value == "3 scout + 1 each")
        );
        view.render(Rect::new(0, 0, 120, 32), &mut buf);
        let rendered = buf
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("Fleet"));
        assert!(rendered.contains("recursion"));
    }
}
