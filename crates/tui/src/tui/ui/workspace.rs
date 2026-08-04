//! Workspace switching and runtime state management.

use std::path::PathBuf;

use crate::config::Config;
use crate::core::engine::{EngineHandle, spawn_engine};
use crate::core::ops::Op;
use crate::hooks::HookExecutor;
use crate::task_manager::SharedTaskManager;

use super::super::app::App;
use super::super::history::HistoryCell;
use super::engine_config_prompt::build_engine_config;

pub(crate) fn apply_workspace_runtime_state(app: &mut App, config: &Config, workspace: PathBuf) {
    app.workspace = workspace.clone();
    app.hooks = HookExecutor::new(
        crate::hooks::HooksConfig::load_with_project(config.hooks_config(), &workspace),
        workspace.clone(),
    );
    app.skills_dir = crate::tui::app::resolve_skills_dir(&workspace, &config.skills_dir(), config);
    app.skills_scan_mimofan_only = config.skills_config().scan_mimofan_only();
    app.refresh_skill_cache();
    app.workspace_context = None;
    if let Ok(mut cell) = app.workspace_context_cell.lock() {
        *cell = None;
    }
    app.workspace_context_refreshed_at = None;
    app.file_tree = None;

    let shell_manager = crate::tools::shell::new_shared_shell_manager(workspace);
    app.runtime_services.shell_manager = Some(shell_manager);
    app.runtime_services.hook_executor = Some(std::sync::Arc::new(app.hooks.clone()));
}

pub(crate) async fn sync_runtime_workspace_state(
    task_manager: &SharedTaskManager,
    workspace: PathBuf,
) {
    task_manager.set_default_workspace(workspace).await;
}

pub(crate) async fn switch_workspace(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    task_manager: &SharedTaskManager,
    config: &Config,
    workspace: PathBuf,
) {
    if app.is_loading {
        app.status_message =
            Some("Cannot switch workspace while a request is running.".to_string());
        app.add_message(HistoryCell::System {
            content: "Cannot switch workspace while a request is running.".to_string(),
        });
        return;
    }

    if app.workspace == workspace {
        app.status_message = Some(format!("Workspace unchanged: {}", workspace.display()));
        return;
    }

    apply_workspace_runtime_state(app, config, workspace.clone());
    sync_runtime_workspace_state(task_manager, workspace.clone()).await;

    let _ = engine_handle.send(Op::Shutdown).await;
    let engine_config = build_engine_config(app, config);
    *engine_handle = spawn_engine(engine_config, config);
    if !app.api_messages.is_empty() {
        let _ = engine_handle
            .send(Op::SyncSession {
                session_id: app.current_session_id.clone(),
                messages: app.api_messages.clone(),
                system_prompt: app.system_prompt.clone(),
                system_prompt_override: false,
                model: app.model.clone(),
                workspace: workspace.clone(),
            })
            .await;
    }

    app.add_message(HistoryCell::System {
        content: format!("Switched workspace to {}", workspace.display()),
    });
    app.status_message = Some(format!("Workspace: {}", workspace.display()));
}
