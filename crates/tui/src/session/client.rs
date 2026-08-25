//! Attach client: proxy the local terminal to a running session daemon.
//!
//! `mimofan session attach <id>` connects to `~/.mimofan/run/<id>.sock`, puts
//! the local terminal into raw mode, and bridges stdin→socket and socket→stdout.
//! Pressing `Ctrl-]` (0x1d) or hitting EOF detaches: the local terminal is
//! restored and the daemon (with its PTY) keeps running.

use anyhow::{Context, Result};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::session::protocol::{Frame, OP_DATA, OP_DETACH, OP_RESIZE, encode_frame, read_frame};
use crate::session::registry;

/// Spawn the daemon as a detached, independent process so the session outlives
/// the client. Uses the current executable (`mimofan daemon <id> ...`).
pub fn spawn_detached_daemon(id: &str, program: &str, args: &[String]) -> Result<()> {
    let exe = std::env::current_exe().context("resolve current exe")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon").arg(id).arg(program);
    for a in args {
        cmd.arg(a);
    }
    // Detach: don't wait, inherit nothing that ties it to this client.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        // Double-fork-ish detach: start a new session/process group.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    cmd.spawn().context("spawn detached daemon")?;
    Ok(())
}

/// Attach to session `id`. If `autostart` is set and the socket is absent, the
/// daemon is spawned first (running `default_program`). On detach, returns Ok
/// leaving the daemon alive.
pub async fn run_attach(
    id: &str,
    autostart: bool,
    default_program: &str,
    default_args: &[String],
) -> Result<()> {
    let socket = registry::socket_path(id)?;
    if !socket.exists() {
        if !autostart {
            anyhow::bail!(
                "session '{id}' is not running (no socket at {}). Use `mimofan session attach {id} --new` to start it.",
                socket.display()
            );
        }
        spawn_detached_daemon(id, default_program, default_args)?;
        // Give the daemon a moment to bind its socket.
        for _ in 0..50 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        if !socket.exists() {
            anyhow::bail!("daemon for session '{id}' failed to start (socket never appeared)");
        }
    }

    let mut stream = UnixStream::connect(&socket)
        .await
        .with_context(|| format!("connect session socket {}", socket.display()))?;

    enable_raw_mode().context("enable raw mode")?;
    // Resize PTY to current terminal size.
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        let mut frame = Vec::new();
        encode_frame(
            OP_RESIZE,
            &[(rows >> 8) as u8, rows as u8, (cols >> 8) as u8, cols as u8],
            &mut frame,
        );
        let _ = stream.write_all(&frame).await;
    }

    let (mut r, mut w) = stream.into_split();

    // Task A: local stdin → socket. Detect Ctrl-] (0x1d) to detach.
    let mut stdin = tokio::io::stdin();
    let mut writer_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match tokio::io::AsyncReadExt::read(&mut stdin, &mut buf).await {
                Ok(0) => break, // EOF → detach
                Ok(n) => {
                    if n == 1 && buf[0] == 0x1d {
                        break; // Ctrl-] → detach
                    }
                    let mut frame = Vec::new();
                    encode_frame(OP_DATA, &buf[..n], &mut frame);
                    if w.write_all(&frame).await.is_err() {
                        break;
                    }
                    let _ = w.flush().await;
                }
                Err(_) => break,
            }
        }
        // Notify the daemon we're gone (PTY stays alive).
        let mut detach = Vec::new();
        encode_frame(OP_DETACH, &[], &mut detach);
        let _ = w.write_all(&detach).await;
        let _ = w.flush().await;
    });

    // Task B: socket → local stdout (PTY output).
    let mut stdout = tokio::io::stdout();
    let mut reader_task = tokio::spawn(async move {
        let mut header = [0u8; 5];
        while let Ok(Ok(5)) =
            tokio::time::timeout(std::time::Duration::from_secs(3600), r.read(&mut header)).await
        {
            let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
            let mut payload = vec![0u8; len];
            if r.read_exact(&mut payload).await.is_err() {
                break;
            }
            if stdout.write_all(&payload).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    // Wait for either direction to end (detach/EOF/disconnect).
    tokio::select! {
        _ = &mut writer_task => {},
        _ = &mut reader_task => {},
    }
    // Abort the other direction.
    writer_task.abort();
    reader_task.abort();

    disable_raw_mode().context("disable raw mode")?;
    eprintln!(
        "\r\ndetached from session '{id}' (daemon still running). Re-attach with: mimofan session attach {id}"
    );
    Ok(())
}
