//! Sidebar visibility geometry.
//!
//! Pure functions that decide how (and whether) the sidebar should render given
//! the current `App` state and the last measured host width. Kept free of any
//! terminal/rendering I/O so it can be unit-reasoned about in isolation.

use crate::tui::app::{App, SidebarFocus};
use super::SIDEBAR_VISIBLE_MIN_WIDTH;

/// How the sidebar should be presented on the next paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarRenderState {
    Hidden,
    SuppressedByWidth {
        available_width: u16,
        min_width: u16,
    },
    AutoCollapsed,
    Visible,
}

pub(crate) fn sidebar_render_state(app: &mut App) -> SidebarRenderState {
    if app.sidebar_focus == SidebarFocus::Hidden {
        return SidebarRenderState::Hidden;
    }

    if let Some(available_width) = sidebar_host_width_hint(app)
        && available_width < SIDEBAR_VISIBLE_MIN_WIDTH
    {
        return SidebarRenderState::SuppressedByWidth {
            available_width,
            min_width: SIDEBAR_VISIBLE_MIN_WIDTH,
        };
    }

    if crate::tui::sidebar::sidebar_auto_idle(app) {
        return SidebarRenderState::AutoCollapsed;
    }

    SidebarRenderState::Visible
}

fn sidebar_host_width_hint(app: &App) -> Option<u16> {
    app.last_sidebar_host_width.or_else(|| {
        let transcript_width = app.viewport.last_transcript_area.map(|area| area.width)?;
        let sidebar_width = app
            .viewport
            .last_sidebar_area
            .or(app.last_sidebar_area)
            .map(|area| area.width)
            .unwrap_or(0);
        Some(transcript_width.saturating_add(sidebar_width))
    })
}

pub(crate) fn sidebar_width_for_chat_area(app: &App, chat_width: u16) -> Option<u16> {
    if app.sidebar_focus == SidebarFocus::Hidden || chat_width < SIDEBAR_VISIBLE_MIN_WIDTH {
        return None;
    }

    let preferred_sidebar =
        (u32::from(chat_width) * u32::from(app.sidebar_width.clamp(10, 50)) / 100) as u16;
    let sidebar_width = preferred_sidebar.max(24).min(chat_width.saturating_sub(40));

    (sidebar_width >= 20).then_some(sidebar_width)
}
