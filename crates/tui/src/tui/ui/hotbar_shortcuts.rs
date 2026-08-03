//! hotbar shortcuts 子系统（从 ui 上帝文件切片）
use super::*;

pub(crate) fn hotbar_slot_from_key(app: &App, key: &event::KeyEvent) -> Option<u8> {
    if app.onboarding != OnboardingState::None || !app.view_stack.is_empty() {
        return None;
    }

    let KeyCode::Char(c) = key.code else {
        return None;
    };
    if !('1'..='8').contains(&c) {
        return None;
    }
    let slot = c.to_digit(10).and_then(|digit| u8::try_from(digit).ok())?;

    if key.modifiers.contains(KeyModifiers::ALT)
        && !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::SUPER)
    {
        return Some(slot);
    }

    None
}

pub(crate) fn dispatch_hotbar_slot(
    app: &mut App,
    config: &Config,
    slot: u8,
) -> Result<Option<HotbarDispatch>> {
    let known_action_ids = app
        .hotbar_actions
        .iter()
        .map(|action| action.id())
        .collect::<Vec<_>>();
    let bindings = config.resolve_hotbar_bindings(&known_action_ids).bindings;
    let Some(action_id) = bindings
        .iter()
        .find(|binding| binding.slot == slot)
        .map(|binding| binding.action.clone())
    else {
        return Ok(None);
    };

    let Some(action) = app.hotbar_actions.get(&action_id) else {
        app.status_message = Some(format!(
            "Hotbar slot {slot} action is not available: {action_id}"
        ));
        app.needs_redraw = true;
        return Ok(Some(HotbarDispatch::Handled));
    };

    action.dispatch(app).map(Some)
}

pub(crate) fn apply_alt_4_shortcut(app: &mut App, _modifiers: KeyModifiers) {
    app.set_sidebar_focus(SidebarFocus::Agents);
    app.status_message = Some("Sidebar focus: agents".to_string());
}

pub(crate) fn persist_sidebar_settings_if_dirty(app: &mut App) {
    if !app.sidebar_width_dirty && !app.sidebar_focus_dirty {
        return;
    }

    let width_dirty = app.sidebar_width_dirty;
    let focus_dirty = app.sidebar_focus_dirty;
    app.sidebar_width_dirty = false;
    app.sidebar_focus_dirty = false;

    if let Ok(mut settings) = Settings::load() {
        if width_dirty {
            settings.update_sidebar_width(app.sidebar_width);
        }
        if focus_dirty {
            let _ = settings.set("sidebar_focus", app.sidebar_focus.as_setting());
        }
        let _ = settings.save();
    }
}

pub(crate) fn apply_alt_0_shortcut(app: &mut App, modifiers: KeyModifiers) {
    if modifiers.contains(KeyModifiers::CONTROL) {
        if app.sidebar_focus == SidebarFocus::Hidden {
            app.set_sidebar_focus(SidebarFocus::Pinned);
            app.status_message = Some("Sidebar focus: pinned".to_string());
        } else {
            app.set_sidebar_focus(SidebarFocus::Hidden);
            app.status_message = Some("Sidebar hidden".to_string());
        }
    } else {
        app.set_sidebar_focus(SidebarFocus::Auto);
        app.status_message = Some("Sidebar focus: auto".to_string());
    }
}
