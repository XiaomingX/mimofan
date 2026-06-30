use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Result, bail};

use crate::commands::{self, CommandInfo, CommandResult};
use crate::tui::app::{App, AppAction, AppMode, SidebarFocus};
use crate::tui::command_palette::{
    CommandPaletteView, build_entries as build_command_palette_entries,
};

/// Result of firing a hotbar action.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum HotbarDispatch {
    /// The action was fully handled by mutating [`App`].
    Handled,
    /// The event loop must handle an existing application action.
    AppAction(AppAction),
}

/// Uniform interface for actions that can be bound to a hotbar slot.
#[allow(dead_code)]
pub trait HotbarAction: Send + Sync {
    /// Stable action id used in config and dispatch.
    fn id(&self) -> &str;

    /// Compact cell label. Built-ins keep this at seven characters or less.
    fn short_label(&self) -> &str;

    /// Source category, such as `app`, `slash`, `mcp`, `skill`, or `plugin`.
    fn category(&self) -> &str;

    /// Whether the action is currently active in the supplied app state.
    fn is_active(&self, app: &App) -> bool;

    /// Fire the action.
    fn dispatch(&self, app: &mut App) -> Result<HotbarDispatch>;
}

#[derive(Default, Clone)]
pub struct HotbarActionRegistry {
    actions: BTreeMap<String, Arc<dyn HotbarAction>>,
}

impl HotbarActionRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register_builtins();
        registry.register_slash_commands();
        registry
    }

    pub fn register(&mut self, action: impl HotbarAction + 'static) {
        self.actions
            .insert(action.id().to_string(), Arc::new(action));
    }

    pub(crate) fn register_builtins(&mut self) {
        self.register(AppHotbarAction::new(
            "voice.toggle",
            "voice",
            AppHotbarKind::VoiceToggle,
        ));
        self.register(AppHotbarAction::new(
            "session.compact",
            "compact",
            AppHotbarKind::SessionCompact,
        ));
        self.register(AppHotbarAction::new(
            "mode.plan",
            "plan",
            AppHotbarKind::Mode(AppMode::Plan),
        ));
        self.register(AppHotbarAction::new(
            "mode.agent",
            "agent",
            AppHotbarKind::Mode(AppMode::Agent),
        ));
        self.register(AppHotbarAction::new(
            "mode.yolo",
            "yolo",
            AppHotbarKind::Mode(AppMode::Yolo),
        ));
        self.register(AppHotbarAction::new(
            "reasoning.cycle",
            "reason",
            AppHotbarKind::ReasoningCycle,
        ));
        self.register(AppHotbarAction::new(
            "sidebar.toggle",
            "side",
            AppHotbarKind::SidebarToggle,
        ));
        self.register(AppHotbarAction::new(
            "filetree.toggle",
            "files",
            AppHotbarKind::FileTreeToggle,
        ));
        self.register(AppHotbarAction::new(
            "palette.open",
            "palette",
            AppHotbarKind::PaletteOpen,
        ));
        self.register(AppHotbarAction::new(
            "trust.toggle",
            "trust",
            AppHotbarKind::TrustToggle,
        ));
    }

    pub(crate) fn register_slash_commands(&mut self) {
        for info in commands::command_infos() {
            self.register(SlashHotbarAction::new(info));
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn HotbarAction>> {
        self.actions.get(id).cloned()
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = &dyn HotbarAction> {
        self.actions.values().map(Arc::as_ref)
    }
}

fn dispatch_command_result(app: &mut App, result: CommandResult) -> HotbarDispatch {
    app.status_message = result.message;
    result
        .action
        .map_or(HotbarDispatch::Handled, HotbarDispatch::AppAction)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppHotbarKind {
    VoiceToggle,
    SessionCompact,
    Mode(AppMode),
    ReasoningCycle,
    SidebarToggle,
    FileTreeToggle,
    PaletteOpen,
    TrustToggle,
}

#[allow(dead_code)]
struct AppHotbarAction {
    id: &'static str,
    short_label: &'static str,
    kind: AppHotbarKind,
}

impl AppHotbarAction {
    const fn new(id: &'static str, short_label: &'static str, kind: AppHotbarKind) -> Self {
        Self {
            id,
            short_label,
            kind,
        }
    }
}

impl HotbarAction for AppHotbarAction {
    fn id(&self) -> &str {
        self.id
    }

    fn short_label(&self) -> &str {
        self.short_label
    }

    fn category(&self) -> &str {
        "app"
    }

    fn is_active(&self, app: &App) -> bool {
        match self.kind {
            AppHotbarKind::VoiceToggle => app.voice_enabled,
            AppHotbarKind::SessionCompact => app.is_compacting,
            AppHotbarKind::Mode(mode) => app.mode == mode,
            AppHotbarKind::ReasoningCycle => {
                !app.auto_model && app.reasoning_effort != crate::tui::app::ReasoningEffort::Off
            }
            AppHotbarKind::SidebarToggle => app.sidebar_focus != SidebarFocus::Hidden,
            AppHotbarKind::FileTreeToggle => app.file_tree.is_some(),
            AppHotbarKind::PaletteOpen => false,
            AppHotbarKind::TrustToggle => app.trust_mode,
        }
    }

    fn dispatch(&self, app: &mut App) -> Result<HotbarDispatch> {
        match self.kind {
            AppHotbarKind::VoiceToggle => {
                let result = crate::commands::voice::voice(app);
                Ok(dispatch_command_result(app, result))
            }
            AppHotbarKind::SessionCompact => {
                if app.is_compacting {
                    app.status_message = Some("Compaction is already running.".to_string());
                    return Ok(HotbarDispatch::Handled);
                }
                Ok(HotbarDispatch::AppAction(AppAction::CompactContext))
            }
            AppHotbarKind::Mode(mode) => {
                let changed = app.set_mode(mode);
                if changed {
                    Ok(HotbarDispatch::AppAction(AppAction::ModeChanged(mode)))
                } else {
                    Ok(HotbarDispatch::Handled)
                }
            }
            AppHotbarKind::ReasoningCycle => {
                if app.auto_model {
                    bail!("Reasoning effort is controlled by auto model routing.");
                }
                app.reasoning_effort = app
                    .reasoning_effort
                    .cycle_next_for_provider(app.api_provider);
                app.last_effective_reasoning_effort = None;
                app.update_model_compaction_budget();
                app.status_message = Some(format!(
                    "Reasoning effort: {}",
                    app.reasoning_effort
                        .display_label_for_provider(app.api_provider)
                ));
                Ok(HotbarDispatch::AppAction(AppAction::UpdateCompaction(
                    app.compaction_config(),
                )))
            }
            AppHotbarKind::SidebarToggle => {
                if app.sidebar_focus == SidebarFocus::Hidden {
                    app.set_sidebar_focus(SidebarFocus::Pinned);
                    app.status_message = Some("Sidebar focus: pinned".to_string());
                } else {
                    app.set_sidebar_focus(SidebarFocus::Hidden);
                    app.status_message = Some("Sidebar hidden".to_string());
                }
                Ok(HotbarDispatch::Handled)
            }
            AppHotbarKind::FileTreeToggle => {
                if app.file_tree.is_some() {
                    app.file_tree = None;
                    app.status_message = Some("File tree closed".to_string());
                } else {
                    app.file_tree = Some(crate::tui::file_tree::FileTreeState::new(&app.workspace));
                    app.status_message =
                        Some("File tree: ↑/↓ navigate  Enter select  Esc close".to_string());
                }
                app.needs_redraw = true;
                Ok(HotbarDispatch::Handled)
            }
            AppHotbarKind::PaletteOpen => {
                app.view_stack
                    .push(CommandPaletteView::new(build_command_palette_entries(
                        app.ui_locale,
                        &app.skills_dir,
                        app.skills_scan_mimofan_only,
                        &app.workspace,
                        &app.mcp_config_path,
                        app.mcp_snapshot.as_ref(),
                    )));
                Ok(HotbarDispatch::Handled)
            }
            AppHotbarKind::TrustToggle => {
                app.trust_mode = !app.trust_mode;
                app.status_message = Some(if app.trust_mode {
                    "Workspace trust mode enabled.".to_string()
                } else {
                    "Workspace trust mode disabled.".to_string()
                });
                Ok(HotbarDispatch::Handled)
            }
        }
    }
}

#[allow(dead_code)]
struct SlashHotbarAction {
    info: &'static CommandInfo,
    id: String,
    short_label: String,
}

impl SlashHotbarAction {
    fn new(info: &'static CommandInfo) -> Self {
        Self {
            info,
            id: format!("slash.{}", info.name),
            short_label: info.name.chars().take(7).collect(),
        }
    }

    fn prefill_composer(&self, app: &mut App) {
        app.clear_input_recoverable();
        app.input = format!("/{} ", self.info.name);
        app.cursor_position = app.input.chars().count();
        app.slash_menu_hidden = false;
        app.needs_redraw = true;
        app.status_message = Some(format!(
            "Command needs arguments; complete {}",
            app.input.trim_end()
        ));
    }
}

impl HotbarAction for SlashHotbarAction {
    fn id(&self) -> &str {
        &self.id
    }

    fn short_label(&self) -> &str {
        &self.short_label
    }

    fn category(&self) -> &str {
        "slash"
    }

    fn is_active(&self, _app: &App) -> bool {
        false
    }

    fn dispatch(&self, app: &mut App) -> Result<HotbarDispatch> {
        if self.info.requires_required_argument() {
            self.prefill_composer(app);
            return Ok(HotbarDispatch::Handled);
        }

        let input = format!("/{}", self.info.name);
        let result = commands::execute(&input, app);
        Ok(dispatch_command_result(app, result))
    }
}

#[cfg(test)]
mod tests {}
