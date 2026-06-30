//! Footer bar widget displaying mode, status, model, and auxiliary chips.
//!
//! `FooterWidget` is a pure render of a [`FooterProps`] struct: all content
//! (labels, colors, span clusters) is computed once per redraw at a higher
//! level, then `FooterWidget::new(props).render(area, buf)` paints the
//! result. The widget owns no `App` knowledge; this mirrors the layout used
//! by `HeaderWidget` (and Codex's `bottom_pane::footer::Footer`).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::localization::{Locale, MessageId, tr};
use crate::palette;
use crate::tui::app::{App, AppMode};

use super::Renderable;

/// Pre-computed data the footer needs to render.
///
/// All fields are owned `String` / `Vec<Span<'static>>` values so the props
/// can be built once per redraw and then handed to a borrow-free widget.
#[derive(Debug, Clone)]
pub struct FooterProps {
    /// The current model identifier shown after the mode chip.
    pub model: String,
    /// `"agent"` / `"yolo"` / `"plan"` — the canonical setting label.
    pub mode_label: &'static str,
    /// Color used for the mode chip.
    pub mode_color: Color,
    /// Color used for small separators between chips.
    pub text_dim_color: Color,
    /// Color used for the model label.
    pub text_hint_color: Color,
    /// Color used for steady secondary chips such as cost.
    pub text_muted_color: Color,
    /// Background color for the full footer/status bar row.
    pub footer_bg: Color,
    /// Status label like `"idle"`, `"busy"`, `"working"`. When the label
    /// equals `"ready"` the footer hides the status segment entirely.
    pub state_label: String,
    /// Color used for the status label.
    pub state_color: Color,
    /// Sub-agent count chip spans (empty when zero in-flight).
    pub agents: Vec<Span<'static>>,
    /// Reasoning-replay chip spans (empty when zero / not applicable).
    pub reasoning_replay: Vec<Span<'static>>,
    /// Cache-hit-rate chip spans (empty when no usage reported).
    pub cache: Vec<Span<'static>>,
    /// MCP server health chip spans (empty when no MCP servers configured).
    /// Populated lazily — see [`footer_mcp_chip`]. (#502)
    pub mcp: Vec<Span<'static>>,
    /// Cumulative model-work chip spans ("worked 3h 12m"). Sums the
    /// elapsed time of completed turns (from `App::cumulative_turn_duration`),
    /// **not** wall-clock since launch — an idle TUI shouldn't claim
    /// it's been "working." Empty until cumulative turn time crosses
    /// 60s. Populated by [`footer_worked_chip`]. (#448)
    pub worked: Vec<Span<'static>>,
    /// Snapshot of the global retry-status surface (#499). Sampled once
    /// at props-build time and rendered as a foreground banner on the
    /// left of the footer when active. Captured here (rather than read
    /// from `retry_status` at render time) so tests can pin a
    /// deterministic state without racing the parallel runner.
    pub retry: crate::retry_status::RetryState,
    /// Session-cost chip spans (empty when below the display threshold).
    /// Rendered in the left cluster (after the model name) — cost is steady
    /// info, not a transient signal, so it lives with mode and model.
    pub cost: Vec<Span<'static>>,
    /// Account balance chip spans (empty when un fetched or zero). Rendered
    /// in the left cluster right after cost.
    pub balance: Vec<Span<'static>>,
    /// Optional toast that, when present, replaces the left status line.
    pub toast: Option<FooterToast>,
    /// When `Some(frame_idx)`, the gap between the left status line and the
    /// right-hand chips is filled with an animated water-spout strip keyed
    /// off `frame_idx` (deterministic given the frame). `None` keeps the gap
    /// as plain whitespace, which is the idle/ready state.
    pub working_strip_frame: Option<u64>,
}

const WAVE_GLYPHS: [char; 8] = [
    '\u{2581}', // ▁
    '\u{2582}', // ▂
    '\u{2583}', // ▃
    '\u{2584}', // ▄
    '\u{2585}', // ▅
    '\u{2586}', // ▆
    '\u{2587}', // ▇
    '\u{2588}', // █
];

/// One frame of the footer's live-work wave animation. `col` is the cell
/// index inside the strip, `width` the strip's total width, `frame` the raw
/// millisecond counter. Returns the glyph that should appear in that cell on
/// that frame.
///
/// Visual: a full-width phase-shifted wave made from one-cell block-height
/// glyphs. The earlier crest-pair animation only changed when rounded crest
/// positions crossed a terminal cell boundary; at an 80 ms repaint cadence it
/// read as visible hops. Sampling a few moving sine components gives every
/// repaint a new surface while keeping the math deterministic for tests.
#[must_use]
pub fn footer_working_strip_glyph_at(col: usize, width: usize, frame: u64) -> char {
    if width == 0 {
        return ' ';
    }

    let t = frame as f64 / 1000.0;
    let x = col as f64;

    let primary = (x * 0.52 - t * 8.0).sin();
    let swell = (x * 0.18 + t * 3.1).sin() * 0.35;
    let shimmer = (x * 1.35 - t * 11.0).sin() * 0.12;
    let value = ((primary + swell + shimmer) / 1.47).clamp(-1.0, 1.0);
    let normalized = (value + 1.0) * 0.5;
    let idx = (normalized * (WAVE_GLYPHS.len() - 1) as f64).round() as usize;
    WAVE_GLYPHS[idx.min(WAVE_GLYPHS.len() - 1)]
}

/// Build the per-frame live-work wave string of `width` characters. Empty string
/// when width is 0. The result is the same visual width as requested (one
/// char per column for the selected block-height glyphs) and is safe to drop
/// into a `Span` between the footer's left and right segments.
#[must_use]
pub fn footer_working_strip_string(width: usize, frame: u64) -> String {
    let mut out = String::with_capacity(width * 4);
    for col in 0..width {
        out.push(footer_working_strip_glyph_at(col, width, frame));
    }
    out
}

/// Pulse the localized "working" label through 0–3 trailing ASCII dots
/// keyed off `frame`. The cycle period is 4 frames (matching the four
/// states), so adjacent ticks visibly differ. Dots stay ASCII regardless
/// of locale so the animation reads identically across scripts. Returns a
/// `String` so callers can drop it into a `Span::styled` without lifetime
/// gymnastics.
#[must_use]
pub fn footer_working_label(frame: u64, locale: Locale) -> String {
    let dots = (frame % 4) as usize;
    let base = tr(locale, MessageId::FooterWorking);
    let mut out = String::with_capacity(base.len() + dots);
    out.push_str(base);
    for _ in 0..dots {
        out.push('.');
    }
    out
}

#[must_use]
pub fn footer_shell_label_chip(label: String) -> Vec<Span<'static>> {
    if label.trim().is_empty() {
        return Vec::new();
    }
    vec![Span::styled(
        format!("\u{23F3} {label}"),
        Style::default().fg(palette::STATUS_WARNING),
    )]
}

/// Build a "N agents" chip span list when there are sub-agents in flight.
/// Empty list when N == 0 hides the chip entirely. Singular for N == 1
/// reads naturally; plural otherwise. The pluralization template lives in
/// the locale registry so CJK locales can render the count without the
/// English plural-`s` artefact.
#[must_use]
pub fn footer_agents_chip(running: usize, locale: Locale) -> Vec<Span<'static>> {
    if running == 0 {
        return Vec::new();
    }
    let text = if running == 1 {
        tr(locale, MessageId::FooterAgentSingular).to_string()
    } else {
        tr(locale, MessageId::FooterAgentsPlural).replace("{count}", &running.to_string())
    };
    vec![Span::styled(
        text,
        Style::default().fg(palette::DEEPSEEK_SKY),
    )]
}

/// Build the cumulative-elapsed chip ("worked 3h 12m") for the
/// footer's right cluster (#448). Hidden during the first minute of
/// a session so a fresh launch doesn't render a noisy `worked 5s`
/// indicator that immediately starts ticking. Above the threshold,
/// reuses [`crate::tui::notifications::humanize_duration`] for
/// consistent w/d/h/m formatting.
#[must_use]
pub fn footer_worked_chip(elapsed: std::time::Duration) -> Vec<Span<'static>> {
    if elapsed < std::time::Duration::from_secs(60) {
        return Vec::new();
    }
    let label = format!(
        "worked {}",
        crate::tui::notifications::humanize_duration(elapsed)
    );
    vec![Span::styled(
        label,
        Style::default().fg(palette::TEXT_MUTED),
    )]
}

/// Build the "MCP M/N" health chip (#502) from the user's stored
/// snapshot. `connected` is the number of servers currently reachable;
/// `configured` is the number declared in the user's MCP config. When
/// `configured` is zero the chip is hidden entirely.
///
/// Colour-codes the count by health:
/// - all reachable → success
/// - some reachable → warning
/// - none reachable but at least one configured → error
/// - configured but no live snapshot yet → muted (count only)
#[must_use]
pub fn footer_mcp_chip(connected: Option<usize>, configured: usize) -> Vec<Span<'static>> {
    if configured == 0 {
        return Vec::new();
    }
    let (label, color) = match connected {
        None => (format!("MCP {configured}"), palette::TEXT_MUTED),
        Some(c) if c == configured => (format!("MCP {c}/{configured}"), palette::STATUS_SUCCESS),
        Some(0) => (format!("MCP 0/{configured}"), palette::STATUS_ERROR),
        Some(c) => (format!("MCP {c}/{configured}"), palette::STATUS_WARNING),
    };
    vec![Span::styled(label, Style::default().fg(color))]
}

/// A status toast routed to the footer's left segment for a short time.
#[derive(Debug, Clone)]
pub struct FooterToast {
    pub text: String,
    pub color: Color,
}

impl FooterProps {
    /// Build footer props from common app state. Helpers in `tui/ui.rs`
    /// supply the pre-styled spans and labels — this constructor just bundles
    /// them.
    ///
    /// Argument fan-out is intentional: each input maps 1:1 to a piece of
    /// pre-computed footer content the caller resolved from `App`. Forcing
    /// these into a builder would obscure the call site without making the
    /// data flow any clearer.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn from_app(
        app: &App,
        toast: Option<FooterToast>,
        state_label: &'static str,
        state_color: Color,
        agents: Vec<Span<'static>>,
        reasoning_replay: Vec<Span<'static>>,
        cache: Vec<Span<'static>>,
        cost: Vec<Span<'static>>,
        balance: Vec<Span<'static>>,
    ) -> Self {
        let (mode_label, mode_color) = mode_style(app);
        // MCP chip (#502) — passive, derived from the user's existing
        // snapshot. `connected` is `None` until the user runs `/mcp`,
        // which is the same trigger the issue spec accepts for now.
        let mcp_configured = app.mcp_configured_count;
        let mcp_connected = app
            .mcp_snapshot
            .as_ref()
            .map(|s| s.servers.iter().filter(|server| server.connected).count());
        let mcp = footer_mcp_chip(mcp_connected, mcp_configured);
        // #448: cumulative work-time chip. Sums actual turn durations
        // (set on `TurnComplete`) rather than wall-clock uptime — a TUI
        // that's been open and idle for 4 minutes shouldn't claim
        // "worked 4m". The chip stays empty until enough turns add up
        // to cross the 60s threshold inside `footer_worked_chip`.
        let worked = footer_worked_chip(app.cumulative_turn_duration);
        Self {
            model: app.model_display_label(),
            mode_label,
            mode_color,
            text_dim_color: app.ui_theme.text_dim,
            text_hint_color: app.ui_theme.text_hint,
            text_muted_color: app.ui_theme.text_muted,
            footer_bg: app.ui_theme.footer_bg,
            state_label: state_label.to_string(),
            state_color,
            agents,
            reasoning_replay,
            cache,
            mcp,
            worked,
            cost,
            balance,
            toast,
            working_strip_frame: None,
            retry: crate::retry_status::snapshot(),
        }
    }
}

fn mode_style(app: &App) -> (&'static str, Color) {
    let label = match app.mode {
        AppMode::Agent => "agent",
        AppMode::Yolo => "yolo",
        AppMode::Plan => "plan",
    };
    let color = match app.mode {
        AppMode::Agent => app.ui_theme.mode_agent,
        AppMode::Yolo => app.ui_theme.mode_yolo,
        AppMode::Plan => app.ui_theme.mode_plan,
    };
    (label, color)
}

/// Pure-render footer. Build once per frame, then `render(area, buf)`.
pub struct FooterWidget {
    props: FooterProps,
}

impl FooterWidget {
    #[must_use]
    pub fn new(props: FooterProps) -> Self {
        Self { props }
    }

    fn auxiliary_spans(&self, max_width: usize) -> Vec<Span<'static>> {
        // `cost` is rendered in the left cluster now — keep it out of the
        // right-hand chip parade. Agents / replay / cache are transient
        // signals; they belong on the right where they appear and
        // disappear without disturbing the steady mode·model·cost line.
        let parts: Vec<&Vec<Span<'static>>> = [
            &self.props.agents,
            &self.props.reasoning_replay,
            &self.props.cache,
            &self.props.mcp,
            // `worked` is the lowest-priority chip — drops first under
            // narrow widths (the priority loop below removes from the
            // tail). `cost` is steady info and stays in the left
            // cluster where the eye finds it without scanning.
            &self.props.worked,
        ]
        .into_iter()
        .filter(|spans| !spans.is_empty())
        .collect();

        // Try to fit as many parts as possible, dropping from the end.
        for end in (0..=parts.len()).rev() {
            let mut combined: Vec<Span<'static>> = Vec::new();
            for (i, part) in parts[..end].iter().enumerate() {
                if i > 0 {
                    combined.push(Span::raw("  "));
                }
                combined.extend(part.iter().cloned());
            }
            if span_width(&combined) <= max_width {
                return combined;
            }
        }
        Vec::new()
    }

    fn toast_spans(toast: &FooterToast, max_width: usize) -> Vec<Span<'static>> {
        let truncated = truncate_to_width(&toast.text, max_width.max(1));
        vec![Span::styled(truncated, Style::default().fg(toast.color))]
    }

    /// Build the left status line with priority-ordered hint dropping.
    ///
    /// Priority order (highest to lowest — last to drop):
    /// 1. Mode label (always visible at any width; truncated only as a last resort)
    /// 2. Model name (always visible; then truncated mid-word once all hints are gone)
    /// 3. Balance chip — drops third (account balance is more actionable than session cost)
    /// 4. Cost chip — drops fourth
    /// 5. Status label (e.g. "working", "draft") — drops first when space is tight
    ///
    /// At every width ≥40 cols the line never wraps mid-hint.
    fn status_line_spans(&self, max_width: usize) -> Vec<Span<'static>> {
        if max_width == 0 {
            return Vec::new();
        }

        let mode_label = self.props.mode_label;
        let sep = " \u{00B7} ";
        let model = self.props.model.as_str();
        let show_status = self.props.state_label != "ready";
        let status_label = self.props.state_label.as_str();
        let cost_text = spans_text(&self.props.cost);
        let show_cost = !cost_text.is_empty();
        let balance_text = spans_text(&self.props.balance);
        let show_balance = !balance_text.is_empty();

        let mode_w = mode_label.width();
        let sep_w = sep.width();
        let model_w = UnicodeWidthStr::width(model);
        let status_w = if show_status { status_label.width() } else { 0 };
        let cost_w = if show_cost { cost_text.width() } else { 0 };
        let balance_w = if show_balance {
            balance_text.width()
        } else {
            0
        };

        let extra_sep = |w: usize| if w > 0 { sep_w } else { 0 };

        // Tier 1: mode · model · balance · cost · status
        let full_w = mode_w
            + sep_w
            + model_w
            + extra_sep(balance_w)
            + balance_w
            + extra_sep(cost_w)
            + cost_w
            + extra_sep(status_w)
            + status_w;
        if (show_balance || show_cost || show_status) && full_w <= max_width {
            return self.build_status_line_spans(
                mode_label,
                model.to_string(),
                show_balance.then(|| balance_text.clone()),
                show_cost.then(|| cost_text.clone()),
                show_status.then_some(status_label),
            );
        }

        // Tier 2: mode · model · balance · cost — drop status.
        let with_cost_w = mode_w
            + sep_w
            + model_w
            + extra_sep(balance_w)
            + balance_w
            + extra_sep(cost_w)
            + cost_w;
        if (show_balance || show_cost) && with_cost_w <= max_width {
            return self.build_status_line_spans(
                mode_label,
                model.to_string(),
                show_balance.then(|| balance_text.clone()),
                show_cost.then(|| cost_text.clone()),
                None,
            );
        }

        // Tier 3: mode · model · balance — drop cost.
        if show_balance {
            let with_balance_w = mode_w + sep_w + model_w + sep_w + balance_w;
            if with_balance_w <= max_width {
                return self.build_status_line_spans(
                    mode_label,
                    model.to_string(),
                    Some(balance_text.clone()),
                    None,
                    None,
                );
            }
        }

        // Tier 4: mode · model — drop balance too.
        let mode_model_w = mode_w + sep_w + model_w;
        if mode_model_w <= max_width {
            return self.build_status_line_spans(mode_label, model.to_string(), None, None, None);
        }

        // Tier 5: mode · <truncated model> — keep both labels visible by
        // ellipsizing the model name. Only do this when there is enough room
        // for at least the ellipsis ("..."). Below that we drop to mode-only.
        let prefix_w = mode_w + sep_w;
        if prefix_w < max_width {
            let model_budget = max_width - prefix_w;
            if model_budget >= 4 {
                let truncated = truncate_to_width(model, model_budget);
                if !truncated.is_empty() {
                    return self.build_status_line_spans(mode_label, truncated, None, None, None);
                }
            }
        }

        // Tier 6: mode-only.
        if mode_w <= max_width {
            return vec![Span::styled(
                mode_label.to_string(),
                Style::default().fg(self.props.mode_color),
            )];
        }
        vec![Span::styled(
            truncate_to_width(mode_label, max_width),
            Style::default().fg(self.props.mode_color),
        )]
    }

    fn build_status_line_spans(
        &self,
        mode_label: &'static str,
        model_label: String,
        balance: Option<String>,
        cost: Option<String>,
        status: Option<&str>,
    ) -> Vec<Span<'static>> {
        let sep = " \u{00B7} ";
        let mut spans: Vec<Span<'static>> = Vec::new();
        if !mode_label.is_empty() {
            spans.push(Span::styled(
                mode_label.to_string(),
                Style::default().fg(self.props.mode_color),
            ));
        }
        if !model_label.is_empty() {
            if !spans.is_empty() {
                spans.push(Span::styled(
                    sep.to_string(),
                    Style::default().fg(self.props.text_dim_color),
                ));
            }
            spans.push(Span::styled(
                model_label,
                Style::default().fg(self.props.text_hint_color),
            ));
        }
        if let Some(balance_text) = balance {
            if !spans.is_empty() {
                spans.push(Span::styled(
                    sep.to_string(),
                    Style::default().fg(self.props.text_dim_color),
                ));
            }
            spans.push(Span::styled(
                balance_text,
                Style::default().fg(self.props.text_muted_color),
            ));
        }
        if let Some(cost_text) = cost {
            if !spans.is_empty() {
                spans.push(Span::styled(
                    sep.to_string(),
                    Style::default().fg(self.props.text_dim_color),
                ));
            }
            spans.push(Span::styled(
                cost_text,
                Style::default().fg(self.props.text_muted_color),
            ));
        }
        if let Some(status_label) = status {
            if !spans.is_empty() {
                spans.push(Span::styled(
                    sep.to_string(),
                    Style::default().fg(self.props.text_dim_color),
                ));
            }
            spans.push(Span::styled(
                status_label.to_string(),
                Style::default().fg(self.props.state_color),
            ));
        }
        spans
    }

    fn left_spans(&self, max_width: usize) -> Vec<Span<'static>> {
        if let Some(banner) = retry_banner_spans(max_width, &self.props) {
            // Retry banner takes precedence over toast and the regular
            // status line so the user sees it loud and clear (#499).
            // The banner clears automatically on success or on the next
            // `TurnStarted` (engine emits the clear).
            banner
        } else if let Some(toast) = self.props.toast.as_ref() {
            Self::toast_spans(toast, max_width)
        } else {
            self.status_line_spans(max_width)
        }
    }
}

fn spans_text(spans: &[Span<'_>]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect::<String>()
}

/// Render the retry banner (#499) when the props' captured snapshot
/// reports an active retry or a final failure. Returns `None` when idle
/// so callers fall back to the regular status line / toast.
fn retry_banner_spans(max_width: usize, props: &FooterProps) -> Option<Vec<Span<'static>>> {
    let (label, color) = match &props.retry {
        crate::retry_status::RetryState::Active(banner) => {
            let secs = props.retry.seconds_remaining().unwrap_or(0);
            // Round to 1s — we redraw each frame anyway so the
            // countdown ticks visually without us having to schedule
            // anything extra.
            (
                format!("⟳ retry {} in {secs}s — {}", banner.attempt, banner.reason),
                crate::palette::STATUS_WARNING,
            )
        }
        crate::retry_status::RetryState::Failed { reason, .. } => {
            (format!("× failed: {reason}"), crate::palette::STATUS_ERROR)
        }
        crate::retry_status::RetryState::Idle => return None,
    };
    let truncated = truncate_to_width(&label, max_width);
    Some(vec![Span::styled(truncated, Style::default().fg(color))])
}

impl Renderable for FooterWidget {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let available_width = area.width as usize;
        if available_width == 0 {
            return;
        }

        // Clear the whole footer row first so stale transcript glyphs from
        // the previous frame cannot survive in cells this frame's spans do not
        // touch (#2244).
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)]
                    .set_symbol(" ")
                    .set_style(Style::default().bg(self.props.footer_bg));
            }
        }

        let preview_left_spans = self.left_spans(available_width);
        let preview_left_width = span_width(&preview_left_spans);
        let right_budget = available_width
            .saturating_sub(preview_left_width)
            .saturating_sub(2);
        let right_spans = self.auxiliary_spans(right_budget);
        let right_width = span_width(&right_spans);
        let min_gap = if right_width > 0 { 2 } else { 0 };
        let max_left_width = available_width
            .saturating_sub(right_width)
            .saturating_sub(min_gap)
            .max(1);
        let left_spans = self.left_spans(max_left_width);

        let left_width = span_width(&left_spans);
        let spacer_width = available_width.saturating_sub(left_width + right_width);

        // When a turn is in flight, fill the gap with a thin animated water-
        // spout strip; otherwise the gap stays as plain whitespace.
        let spacer_span = match self.props.working_strip_frame {
            Some(frame) if spacer_width > 0 => Span::styled(
                footer_working_strip_string(spacer_width, frame),
                Style::default().fg(palette::DEEPSEEK_SKY),
            ),
            _ => Span::raw(" ".repeat(spacer_width)),
        };

        let mut all_spans = left_spans;
        all_spans.push(spacer_span);
        all_spans.extend(right_spans);

        let paragraph =
            Paragraph::new(Line::from(all_spans)).style(Style::default().bg(self.props.footer_bg));
        paragraph.render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

fn span_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 3 {
        return text.chars().take(max_width).collect();
    }

    let mut out = String::new();
    let mut width = 0usize;
    let limit = max_width.saturating_sub(3);
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > limit {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {}
