//! Persistent session runtime (#869).
//!
//! The interactive terminal is decoupled from the main `mimofan` process: a
//! `mimofan daemon <id>` process owns a `portable_pty` shell PTY and a Unix
//! socket at `~/.mimofan/run/<id>.sock`. The `mimofan session attach <id>`
//! client connects, proxies its stdin/stdout to the PTY, and on detach leaves
//! the daemon (and the PTY) alive — so the session survives the client dying
//! and can be re-attached later. This complements the engine-state
//! checkpoint/resume delivered by #851/#856/#857.

pub mod client;
pub mod daemon;
pub mod protocol;
pub mod registry;

pub use client::run_attach;
pub use daemon::run_daemon;
pub use registry::{list_running, run_dir, RunningSession};

use anyhow::Result;
use clap::{Args, Subcommand};
use tokio::io::AsyncWriteExt;

/// Arguments for the internal `daemon` subcommand.
#[derive(Args, Debug, Clone)]
pub struct DaemonArgs {
    /// Session id (also the socket file name under ~/.mimofan/run/).
    pub id: String,
    /// `--` separator; everything after is the program + args to run in the PTY.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// Arguments for the `session` subcommand.
#[derive(Args, Debug, Clone)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SessionCommand {
    /// Attach (or start) an interactive session by id.
    Attach {
        /// Session id to attach to.
        id: String,
        /// Start the daemon if it is not already running (defaults to a shell).
        #[arg(long, default_value_t = false)]
        new: bool,
    },
    /// List running sessions (those with a live socket).
    List,
    /// Kill a running session (terminates its daemon + PTY).
    Kill {
        /// Session id to kill.
        id: String,
    },
}

/// Dispatch the `session` and `daemon` CLI commands.
pub async fn run_session_command(args: SessionArgs) -> Result<()> {
    match args.command {
        SessionCommand::Attach { id, new } => {
            // Default program when auto-starting: the user's login shell.
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
            run_attach(&id, new, &shell, &[]).await
        }
        SessionCommand::List => {
            let running = list_running()?;
            if running.is_empty() {
                println!("no running sessions");
            } else {
                for s in running {
                    println!("{}", s.id);
                }
            }
            Ok(())
        }
        SessionCommand::Kill { id } => {
            // Connect and send SHUTDOWN; if unreachable, just remove the socket.
            let socket = registry::socket_path(&id)?;
            if socket.exists() {
                if let Ok(mut stream) = tokio::net::UnixStream::connect(&socket).await {
                    let mut frame = Vec::new();
                    crate::session::protocol::encode_frame(
                        crate::session::protocol::OP_SHUTDOWN,
                        &[],
                        &mut frame,
                    );
                    let _ = stream.write_all(&frame).await;
                    let _ = stream.flush().await;
                }
                // Give the daemon a moment to exit and unregister.
                for _ in 0..30 {
                    if !socket.exists() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                if socket.exists() {
                    let _ = std::fs::remove_file(&socket);
                }
            }
            println!("killed session '{id}'");
            Ok(())
        }
    }
}

/// Entry point for the internal `daemon` subcommand.
pub async fn run_daemon_command(args: DaemonArgs) -> Result<()> {
    let (program, cmd_args) = match args.command.split_first() {
        Some((p, rest)) => (p.clone(), rest.to_vec()),
        None => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
            (shell, vec![])
        }
    };
    run_daemon(args.id, &program, &cmd_args).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::protocol::{encode_frame, OP_DATA, OP_DETACH, OP_SHUTDOWN};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    /// End-to-end proof of #869: a session daemon owns the PTY and survives
    /// client detach, and can be re-attached. Uses `cat` (echoes stdin→stdout).
    #[tokio::test]
    async fn daemon_survives_detach_and_allows_reattach() {
        let id = format!("itest-{}", uuid::Uuid::new_v4().simple());
        let socket = registry::socket_path(&id).unwrap();
        let _ = std::fs::remove_file(&socket);

        // Spawn the real daemon (owns a `cat` PTY).
        let daemon = tokio::spawn(run_daemon(id.clone(), "cat", &[]));

        // Wait for the socket to appear.
        for _ in 0..100 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        eprintln!("[test] socket present: {}", socket.exists());
        assert!(socket.exists(), "daemon should have bound its socket");

        // Helper: read one DATA frame (with a timeout so a hang fails fast).
        async fn read_one(stream: &mut UnixStream) -> Option<Vec<u8>> {
            let mut header = [0u8; 5];
            let read = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read_exact(&mut header)).await;
            match read {
                Ok(Ok(_)) => {}
                _ => return None,
            }
            let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
            let mut payload = vec![0u8; len];
            stream.read_exact(&mut payload).await.ok()?;
            Some(payload)
        }
        // Helper: send a line of input (cat echoes after newline).
        async fn send_line(stream: &mut UnixStream, line: &[u8]) {
            let mut f = Vec::new();
            let mut buf = line.to_vec();
            buf.push(b'\n');
            encode_frame(OP_DATA, &buf, &mut f);
            stream.write_all(&f).await.unwrap();
            stream.flush().await.unwrap();
        }

        // --- First attach: send "ping", expect echo. ---
        let mut stream = UnixStream::connect(&socket).await.unwrap();
        eprintln!("[test] connected, sending ping");
        send_line(&mut stream, b"ping").await;
        eprintln!("[test] ping sent, awaiting echo");
        let echoed = read_one(&mut stream).await.expect("PTY should echo the input");
        eprintln!("[test] got echo: {:?}", String::from_utf8_lossy(&echoed));
        assert!(
            echoed.windows(4).any(|w| w == b"ping".as_slice()),
            "echo missing 'ping': {echoed:?}"
        );

        // --- Detach (do NOT shutdown). ---
        let mut detach = Vec::new();
        encode_frame(OP_DETACH, &[], &mut detach);
        stream.write_all(&detach).await.unwrap();
        drop(stream);

        // Core guarantee: the daemon + PTY are still alive after detach.
        assert!(
            socket.exists(),
            "#869 violation: socket gone after detach — daemon did not persist"
        );

        // --- Re-attach: send "pong", expect echo again. ---
        let mut stream2 = UnixStream::connect(&socket).await.unwrap();
        // Give the freshly spawned client task a moment to subscribe to the
        // PTY-output broadcast bus before we write input (otherwise the echo
        // frame could be broadcast before any subscriber is registered).
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        send_line(&mut stream2, b"pong").await;
        let echoed2 = read_one(&mut stream2).await.expect("re-attach should reach live PTY");
        assert!(
            echoed2.windows(4).any(|w| w == b"pong".as_slice()),
            "re-attach echo missing 'pong': {echoed2:?}"
        );

        // Clean up.
        let mut shutdown = Vec::new();
        encode_frame(OP_SHUTDOWN, &[], &mut shutdown);
        let _ = stream2.write_all(&shutdown).await;
        let _ = daemon.await;
        let _ = registry::unregister(&id);
    }
}

