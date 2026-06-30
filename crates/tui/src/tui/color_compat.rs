//! Terminal color compatibility shim.
//!
//! Ratatui's crossterm backend emits truecolor SGR for every `Color::Rgb`
//! cell. That is correct for truecolor terminals, but macOS Terminal.app often
//! advertises only `xterm-256color`; sending `38;2` / `48;2` there can render
//! as stray green/cyan backgrounds. This backend adapts every cell to the
//! detected color depth before handing it to crossterm.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};

use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
};

use crate::palette::{self, ColorDepth, PaletteMode, ThemeId, UiTheme};

const RENDER_DEBUG_ENV: &str = "CODEWHALE_TUI_DEBUG";
const RENDER_DEBUG_SAMPLE_LIMIT: usize = 24;

#[derive(Debug)]
pub(crate) struct ColorCompatBackend<W: Write> {
    inner: CrosstermBackend<W>,
    depth: ColorDepth,
    palette_mode: PaletteMode,
    /// Currently active named theme. `System`/`Whale`/`WhaleLight` make the
    /// theme remap a no-op (those rely on the dark/light pipeline); the
    /// community presets (Catppuccin, Tokyo Night, Dracula, Gruvbox) trigger
    /// a per-cell rewrite of dark-palette constants → preset slots.
    theme_id: ThemeId,
    /// Resolved active `UiTheme`, *including* any user `background_color`
    /// override (`UiTheme::with_background_color`). The cell remap reads
    /// target slots from this struct, not from `theme_id.ui_theme()`, so
    /// `theme = "tokyo-night"` + `background_color = "#000000"` lands as a
    /// pure-black surface instead of being overwritten back to
    /// tokyo-night's `#16161e` by the remap.
    active_ui_theme: UiTheme,
    /// During a resize event the terminal emulator may report stale dimensions
    /// for a brief window (observed on macOS Terminal.app and Windows ConHost).
    /// Forcing the expected size prevents ratatui's internal `autoresize` from
    /// shrinking the viewport back to the stale dimension inside `draw()`.
    forced_size: Option<Size>,
    /// Cached terminal size from `crossterm::terminal::size()`, set after
    /// re-entering alt-screen to avoid stale buffer dimensions on Windows.
    /// Used as the primary fallback in `size()` before falling through to
    /// the live crossterm query.
    terminal_size: Option<Size>,
    render_debug: Option<RenderDebugLog>,
}

impl<W: Write> ColorCompatBackend<W> {
    pub(crate) fn new(writer: W, depth: ColorDepth, palette_mode: PaletteMode) -> Self {
        Self {
            inner: CrosstermBackend::new(writer),
            depth,
            palette_mode,
            theme_id: ThemeId::System,
            // Default to whatever System resolves to right now — it stays a
            // no-op for the remap since `theme_id` is also System, so this
            // initial value only matters once `set_theme` flips both fields
            // to a community preset.
            active_ui_theme: UiTheme::detect(),
            forced_size: None,
            terminal_size: None,
            render_debug: RenderDebugLog::from_env(),
        }
    }

    pub(crate) fn force_size(&mut self, size: Size) {
        self.forced_size = Some(size);
    }

    pub(crate) fn clear_forced_size(&mut self) {
        self.forced_size = None;
    }

    pub(crate) fn set_terminal_size(&mut self, size: Size) {
        self.terminal_size = Some(size);
    }

    pub(crate) fn set_palette_mode(&mut self, palette_mode: PaletteMode) {
        self.palette_mode = palette_mode;
    }

    pub(crate) fn set_theme(&mut self, theme_id: ThemeId, ui_theme: UiTheme) {
        self.theme_id = theme_id;
        self.active_ui_theme = ui_theme;
    }
}

impl<W: Write> Write for ColorCompatBackend<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(&mut self.inner)
    }
}

impl<W: Write> Backend for ColorCompatBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let adapted = content
            .map(|(x, y, cell)| {
                let mut cell = cell.clone();
                adapt_cell_colors(
                    &mut cell,
                    self.depth,
                    self.palette_mode,
                    self.theme_id,
                    &self.active_ui_theme,
                );
                (x, y, cell)
            })
            .collect::<Vec<_>>();
        let viewport = if self.render_debug.is_some() {
            self.size().ok()
        } else {
            None
        };
        if let Some(render_debug) = &mut self.render_debug {
            render_debug.record(viewport, &adapted);
        }
        // #3029: Emit OSC 8 hyperlinks out-of-band through the backend's
        // Write impl.  ratatui's buffer pipeline strips ESC bytes, so the
        // open/close sequences must be interleaved with the cell stream
        // here.  OSC 8 is stateful and last-writer-wins: every cell painted
        // between an open and the next close links to that open's target,
        // so each region's cells must be bracketed by their OWN open/close
        // pair — never batched.
        let mut frame_links = crate::tui::osc8::take_frame_links();
        if frame_links.is_empty() || !crate::tui::osc8::enabled() {
            self.inner
                .draw(adapted.iter().map(|(x, y, cell)| (*x, *y, cell)))?;
            return Ok(());
        }
        // Deterministic region lookup when regions are adjacent/overlapping:
        // the first (top-left-most) region wins.
        frame_links.sort_unstable_by_key(|link| (link.row, link.col_start));
        let region_for = |x: u16, y: u16| -> Option<usize> {
            frame_links
                .iter()
                .position(|link| y == link.row && x >= link.col_start && x <= link.col_end)
        };

        // Walk the diff in its original order and split it into runs at
        // region boundaries, so the visible byte stream stays identical to
        // a no-link render apart from the inserted OSC 8 sequences.
        let mut idx = 0;
        while idx < adapted.len() {
            let current_region = region_for(adapted[idx].0, adapted[idx].1);
            let run_start = idx;
            while idx < adapted.len()
                && region_for(adapted[idx].0, adapted[idx].1) == current_region
            {
                idx += 1;
            }
            let run = &adapted[run_start..idx];
            if let Some(region_idx) = current_region {
                crate::tui::osc8::write_osc8_open(self, &frame_links[region_idx].target)?;
                self.inner
                    .draw(run.iter().map(|(x, y, cell)| (*x, *y, cell)))?;
                crate::tui::osc8::write_osc8_close(self)?;
            } else {
                self.inner
                    .draw(run.iter().map(|(x, y, cell)| (*x, *y, cell)))?;
            }
        }
        Ok(())
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        // forced_size takes priority: it is set during resize events to prevent
        // ratatui's autoresize from shrinking the viewport back to a stale
        // dimension. terminal_size is the cached real terminal size used as a
        // fallback after alt-screen re-entry (Windows buffer width workaround).
        if let Some(size) = self.forced_size.or(self.terminal_size) {
            return Ok(size);
        }
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}

#[derive(Debug)]
struct RenderDebugLog {
    file: File,
    frame: u64,
}

impl RenderDebugLog {
    fn from_env() -> Option<Self> {
        if !render_debug_enabled_from_value(std::env::var(RENDER_DEBUG_ENV).ok().as_deref()) {
            return None;
        }

        let log_dir = crate::runtime_log::log_directory()?;
        if let Err(err) = fs::create_dir_all(&log_dir) {
            tracing::debug!(?err, "failed to create TUI render debug log directory");
            return None;
        }
        let path = log_dir.join("tui-render.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|err| {
                tracing::debug!(?err, path = %path.display(), "failed to open TUI render debug log");
                err
            })
            .ok()?;

        Some(Self { file, frame: 0 })
    }

    fn record(&mut self, viewport: Option<Size>, diff: &[(u16, u16, Cell)]) {
        self.frame = self.frame.saturating_add(1);
        let sample = diff
            .iter()
            .take(RENDER_DEBUG_SAMPLE_LIMIT)
            .map(|(x, y, _)| (*x, *y))
            .collect::<Vec<_>>();
        let line = render_debug_line(self.frame, viewport, diff.len(), &sample);
        let _ = self.file.write_all(line.as_bytes());
    }
}

fn render_debug_enabled_from_value(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

fn render_debug_line(
    frame: u64,
    viewport: Option<Size>,
    diff_cells: usize,
    sample: &[(u16, u16)],
) -> String {
    let mut line = String::new();
    match viewport {
        Some(size) => {
            let _ = write!(
                &mut line,
                "frame={frame} size={}x{} diff_cells={diff_cells} sample=",
                size.width, size.height
            );
        }
        None => {
            let _ = write!(
                &mut line,
                "frame={frame} size=unknown diff_cells={diff_cells} sample="
            );
        }
    }
    for (index, (x, y)) in sample.iter().enumerate() {
        if index > 0 {
            line.push(',');
        }
        let _ = write!(&mut line, "{x}:{y}");
    }
    line.push('\n');
    line
}

fn adapt_cell_colors(
    cell: &mut Cell,
    depth: ColorDepth,
    palette_mode: PaletteMode,
    theme_id: ThemeId,
    ui_theme: &UiTheme,
) {
    // Stage 1: community-theme remap (dark palette → preset slots). No-op
    // for System / Whale / WhaleLight so legacy dark/light flows are
    // untouched. Runs *before* the palette-mode remap so a light terminal
    // running e.g. Catppuccin still routes the preset colors through the
    // light adaptation below (rare combo, but the sequencing is the same).
    cell.fg = palette::adapt_fg_for_theme(cell.fg, theme_id, ui_theme);
    cell.bg = palette::adapt_bg_for_theme(cell.bg, theme_id, ui_theme);
    // Stage 2: legacy dark↔light remap.
    let original_bg = cell.bg;
    cell.fg = palette::adapt_fg_for_palette_mode(cell.fg, original_bg, palette_mode);
    cell.bg = palette::adapt_bg_for_palette_mode(cell.bg, palette_mode);
    // Stage 3: depth (truecolor / 256 / 16) downsampling.
    cell.fg = palette::adapt_color(cell.fg, depth);
    cell.bg = palette::adapt_bg(cell.bg, depth);
}
