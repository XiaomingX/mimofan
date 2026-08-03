//! toast 子系统（从 ui 上帝文件切片）
use super::*;

pub(crate) fn status_color(level: StatusToastLevel) -> ratatui::style::Color {
    match level {
        StatusToastLevel::Info => palette::MIMOFAN_SKY,
        StatusToastLevel::Success => palette::STATUS_SUCCESS,
        StatusToastLevel::Warning => palette::STATUS_WARNING,
        StatusToastLevel::Error => palette::STATUS_ERROR,
    }
}

/// Maximum stacked toasts rendered above the footer (#439). The footer line
/// itself stays the most-recent; this overlay surfaces up to two older
/// queued toasts so a burst of status events isn't dropped silently.
const TOAST_STACK_MAX_VISIBLE: usize = 3;

/// Render up to `TOAST_STACK_MAX_VISIBLE - 1` *additional* toasts as an
/// overlay just above the footer when multiple are active. The most recent
/// toast continues to render in the footer line itself; this strip is for
/// the older entries the user would otherwise miss when statuses arrive in
/// bursts.
pub(crate) fn render_toast_stack_overlay(
    f: &mut Frame,
    full_area: Rect,
    composer_area: Rect,
    footer_area: Rect,
    app: &mut App,
) {
    let toasts = app.active_status_toasts(TOAST_STACK_MAX_VISIBLE);
    if toasts.len() < 2 || footer_area.y == 0 {
        return;
    }
    // Drop the most recent (rendered inline by the footer), keep the rest.
    let extra = toasts.len() - 1;
    let stack_height = extra.min(TOAST_STACK_MAX_VISIBLE - 1) as u16;
    // Toast stack can only use space between composer and footer.
    // Composer occupies rows [composer_area.y, composer_area.y + composer_area.height).
    // Toast must start at or after row (composer_area.y + composer_area.height).
    let composer_end = composer_area.y + composer_area.height;
    let max_above = footer_area.y.saturating_sub(composer_end);
    if stack_height == 0 || max_above == 0 {
        return;
    }
    let height = stack_height.min(max_above);
    let stack_area = Rect {
        x: full_area.x,
        y: footer_area.y.saturating_sub(height),
        width: full_area.width,
        height,
    };
    // Iterate oldest-first so the freshest *non-inline* toast is closest to
    // the footer (visually nearest the most-recent message in the line below).
    let visible = &toasts[..extra];
    for (i, toast) in visible.iter().take(height as usize).enumerate() {
        let row_y = stack_area.y + i as u16;
        let row = Rect {
            x: stack_area.x,
            y: row_y,
            width: stack_area.width,
            height: 1,
        };
        let style = ratatui::style::Style::default()
            .fg(status_color(toast.level))
            .add_modifier(ratatui::style::Modifier::DIM);
        let line = ratatui::text::Line::styled(format!(" {} ", toast.text), style);
        f.render_widget(ratatui::widgets::Paragraph::new(line), row);
    }
}
