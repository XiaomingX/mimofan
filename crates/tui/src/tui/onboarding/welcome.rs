//! Welcome screen content for onboarding.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;

pub fn lines() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(
                ">_ mimofan",
                Style::default()
                    .fg(palette::MIMOFAN_ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]),
        Line::from(Span::styled(
            "你的终端 AI 编程伙伴",
            Style::default()
                .fg(palette::MIMOFAN_SKY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "接下来会引导你完成几步简单设置：",
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(Span::styled(
            "添加 API Key · 信任当前目录 · 进入对话",
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "按 Enter 继续",
                Style::default()
                    .fg(palette::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "    Ctrl+C 随时退出",
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]),
    ]
}
