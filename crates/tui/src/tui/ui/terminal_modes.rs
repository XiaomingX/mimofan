//! terminal modes 子系统（从 ui 上帝文件切片）
use super::*;

pub(crate) fn pause_terminal(
    terminal: &mut AppTerminal,
    use_alt_screen: bool,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
) -> Result<()> {
    // #443: pop keyboard enhancement flags before handing the terminal
    // to a child process so it doesn't inherit a half-configured input
    // mode. Best-effort — terminals that didn't accept the flags
    // silently ignore the pop. Matches the shutdown and panic paths.
    pop_keyboard_enhancement_flags(terminal.backend_mut());
    disable_alternate_scroll_mode(terminal.backend_mut());
    execute!(terminal.backend_mut(), DisableFocusChange)?;
    disable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    if use_mouse_capture {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    if use_bracketed_paste {
        disable_bracketed_paste_mode(terminal.backend_mut());
    }
    Ok(())
}

pub(crate) fn resume_terminal(
    terminal: &mut AppTerminal,
    use_alt_screen: bool,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
    sync_output_enabled: bool,
) -> Result<()> {
    enable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), EnterAlternateScreen)?;
        // Re-entering alt-screen after mode recovery — suppress verbose
        // CLI logging again so eprintln! doesn't leak into the TUI.
    }
    recover_terminal_modes(
        terminal.backend_mut(),
        use_mouse_capture,
        use_bracketed_paste,
    );
    // Cache the real terminal size *before* resetting the viewport, so that
    // reset_terminal_viewport → terminal.clear() → autoresize() → backend.size()
    // picks up the cached size instead of falling through to
    // crossterm::terminal::size() which may return stale buffer metadata
    // (especially on Windows after a secondary EnterAlternateScreen).
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        terminal
            .backend_mut()
            .set_terminal_size(Size::new(cols, rows));
    }
    reset_terminal_viewport(terminal, sync_output_enabled)?;
    Ok(())
}

pub(crate) fn reset_terminal_viewport(
    terminal: &mut AppTerminal,
    sync_output_enabled: bool,
) -> Result<()> {
    // Reset scroll margins and origin mode before clearing. Some interactive
    // child processes leave DECSTBM/DECOM behind; if ratatui's diff renderer
    // then writes "row 0", terminals can place it relative to the leaked
    // scroll region and the whole viewport appears shifted down. We
    // deliberately do *not* emit CSI 2J/3J here — see TERMINAL_ORIGIN_RESET
    // for why; the immediately-following ratatui `terminal.clear()` flushes a
    // single clear via the diff renderer, which the alt-screen buffer absorbs
    // without visible flicker on the affected terminals.
    //
    // Wrap the reset+clear sequence in DEC 2026 synchronized-output mode
    // (`\x1b[?2026h` … `\x1b[?2026l`) so GPU-accelerated terminals
    // (Ghostty, VSCode, Kitty, WezTerm) defer rendering until the whole
    // frame is staged. Terminals that don't support it silently ignore.
    // The wrap is opt-out via `synchronized_output = "off"` for terminals
    // that mishandle the sequence (Ptyxis 50.x on VTE 0.84.x flashes the
    // whole viewport on each wrapped frame).
    if sync_output_enabled {
        let _ = terminal.backend_mut().write_all(BEGIN_SYNC_UPDATE);
    }

    let result = (|| -> Result<()> {
        terminal.backend_mut().write_all(TERMINAL_ORIGIN_RESET)?;
        terminal.clear()?;
        Ok(())
    })();

    // Always end the synchronized update, regardless of success or failure.
    if sync_output_enabled {
        let _ = terminal.backend_mut().write_all(END_SYNC_UPDATE);
    }
    let _ = terminal.backend_mut().flush();
    result
}

fn push_keyboard_enhancement_flags<W: Write>(writer: &mut W) {
    // crossterm's PushKeyboardEnhancementFlags command unconditionally
    // returns Unsupported on Windows (is_ansi_code_supported() == false), so
    // the ANSI escape is written directly on that platform. Modern Windows
    // terminals (VSCode integrated terminal, Windows Terminal ≥1.17) honour
    // the kitty keyboard protocol but crossterm's event reader does not
    // decode CSI u sequences on Windows (issue #1599). Write \033[>0u to
    // probe the protocol without enabling any flags — Enter stays as \n.
    #[cfg(not(windows))]
    if let Err(err) = execute!(
        writer,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    ) {
        tracing::debug!(
            target: "kitty_keyboard",
            ?err,
            "PushKeyboardEnhancementFlags ignored (terminal lacks support)"
        );
    }
}

pub(crate) fn pop_keyboard_enhancement_flags<W: Write>(writer: &mut W) {
    // Mirror of push_keyboard_enhancement_flags: crossterm's
    // PopKeyboardEnhancementFlags also has is_ansi_code_supported() == false
    // on Windows, so write the pop escape directly to restore the terminal to
    // its pre-launch keyboard mode.
    // pub(crate) so the panic hook in main.rs and external_editor.rs can
    // also call the Windows-aware path instead of using the raw crossterm
    // execute!() macro which silently no-ops on Windows.
    #[cfg(not(windows))]
    let _ = execute!(writer, PopKeyboardEnhancementFlags);
}

fn set_alternate_scroll_mode<W: Write>(writer: &mut W, enabled: bool) {
    let sequence = if enabled {
        ENABLE_ALT_SCROLL_MODE
    } else {
        DISABLE_ALT_SCROLL_MODE
    };
    if let Err(err) = writer.write_all(sequence).and_then(|()| writer.flush()) {
        tracing::debug!(
            ?err,
            enabled,
            "alternate-scroll terminal mode change ignored"
        );
    }
}

fn enable_alternate_scroll_mode<W: Write>(writer: &mut W) {
    set_alternate_scroll_mode(writer, true);
}

pub(crate) fn disable_alternate_scroll_mode<W: Write>(writer: &mut W) {
    set_alternate_scroll_mode(writer, false);
}

/// Best-effort terminal restoration for emergency exit paths
/// (panic hook, signal handlers). Mirrors the normal teardown in
/// `run_event_loop` but tolerates any subset of modes not actually being
/// active — every step is discarded on failure so a half-initialized TUI
/// (e.g. SIGINT during startup before `EnterAlternateScreen`) still gets
/// raw mode + kitty keyboard flags cleared, which is what causes the
/// `^[[>5u` shell pollution reported in #1583.
pub(crate) fn emergency_restore_terminal() {
    let mut stdout = std::io::stdout();
    pop_keyboard_enhancement_flags(&mut stdout);
    disable_alternate_scroll_mode(&mut stdout);
    let _ = execute!(stdout, DisableFocusChange);
    disable_bracketed_paste_mode(&mut stdout);
    let _ = execute!(stdout, DisableMouseCapture);
    let _ = disable_raw_mode();
    let _ = execute!(stdout, LeaveAlternateScreen);
}

/// Re-establish terminal mode flags. Idempotent and best-effort: each
/// underlying flag is silently discarded by terminals that don't support
/// it, and a single flag's failure doesn't prevent later flags from being
/// attempted.
///
/// **Canonical location for terminal-mode setup.** If you add a new mode
/// flag at startup or in `resume_terminal`, add it here too — `FocusGained`
/// recovery calls this and will silently fall behind otherwise.
///
/// Excluded by design: raw mode and the alternate screen — those persist
/// across focus events and are only re-established by `resume_terminal`
/// after a suspension, which always runs a separate path.
///
pub(crate) fn recover_terminal_modes<W: Write>(
    writer: &mut W,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
) {
    pop_keyboard_enhancement_flags(writer);
    push_keyboard_enhancement_flags(writer);
    enable_alternate_scroll_mode(writer);
    if use_mouse_capture && let Err(err) = execute!(writer, EnableMouseCapture) {
        tracing::debug!(?err, "EnableMouseCapture ignored");
    }
    if use_bracketed_paste {
        try_enable_bracketed_paste_mode(writer);
    }
    if let Err(err) = execute!(writer, EnableFocusChange) {
        tracing::debug!(?err, "EnableFocusChange ignored");
    }
}

fn try_enable_bracketed_paste_mode<W: Write>(writer: &mut W) -> bool {
    match execute!(writer, EnableBracketedPaste) {
        Ok(()) => true,
        Err(err) => {
            tracing::debug!(?err, "EnableBracketedPaste ignored");
            false
        }
    }
}

pub(crate) fn disable_bracketed_paste_mode<W: Write>(writer: &mut W) {
    if let Err(err) = execute!(writer, DisableBracketedPaste) {
        tracing::debug!(?err, "DisableBracketedPaste ignored");
    }
}

pub(crate) fn terminal_event_needs_viewport_recapture(evt: &Event) -> bool {
    matches!(evt, Event::FocusGained)
}
