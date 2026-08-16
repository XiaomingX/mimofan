//! Session daemon: owns a `portable_pty` shell and a Unix socket.
//!
//! The daemon is launched as a *separate process* (see `spawn_detached_daemon`
//! in `client.rs` / the `Daemon` CLI command). Because it is independent of the
//! interactive client, killing/ crashing the client leaves the PTY (and thus the
//! session) alive and re-attachable — this is the core of issue #869.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::session::protocol::{
    read_frame, encode_frame, Frame, OP_DATA, OP_DETACH, OP_RESIZE, OP_SHUTDOWN,
};
use crate::session::registry;

/// Run the daemon for `id`, running `program` with `args` inside a PTY.
/// Blocks until the PTY child exits and there are no attached clients, or a
/// SHUTDOWN control frame is received.
pub async fn run_daemon(id: String, program: &str, args: &[String]) -> Result<()> {
    let socket = registry::socket_path(&id)?;
    if socket.exists() {
        std::fs::remove_file(&socket)
            .with_context(|| format!("remove stale socket {}", socket.display()))?;
    }
    let _ = std::fs::create_dir_all(socket.parent().unwrap());

    // Open the PTY.
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open PTY")?;
    let mut cmd = CommandBuilder::new(program);
    for a in args {
        cmd.arg(a);
    }
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .context("spawn PTY command")?;
    // A killer handle that survives the move of `child` into `wait_task`, so
    // the main loop can terminate the PTY program on an explicit SHUTDOWN.
    let killer = child.clone_killer();
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().context("clone PTY reader")?;
    let writer: Box<dyn Write + Send> = pair.master.take_writer().context("take PTY writer")?;
    let writer = Arc::new(Mutex::new(writer));

    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("bind session socket {}", socket.display()))?;
    eprintln!("mimofan-session[{id}]: daemon listening on {}", socket.display());

    // Broadcast bus: PTY output → all clients.
    let (pty_tx, _) = tokio::sync::broadcast::channel::<Vec<u8>>(1024);
    let pty_tx = Arc::new(pty_tx);

    let child_id = id.to_string();
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Task: read PTY output, broadcast to all clients.
    let pty_tx_rd = pty_tx.clone();
    let reader_task = tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = pty_tx_rd.send(buf[..n].to_vec());
                }
                Err(_) => break,
            }
        }
    });

    // Task: wait for the PTY child to exit, then signal shutdown.
    let shutdown_wait = shutdown.clone();
    let wait_task = tokio::task::spawn_blocking(move || {
        let _ = child.wait();
        shutdown_wait.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    // On an explicit SHUTDOWN we must terminate the still-running child,
    // otherwise `wait_task` blocks forever on `child.wait()` and the daemon
    // never returns (this was the e2e test hang: `cat` never exits on its own).
    let killer = Arc::new(Mutex::new(killer));

    // Dedicated channel so a client's SHUTDOWN actually terminates the daemon
    // (a broadcast sentinel would also fire on normal PTY output).
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(4);

    loop {
        if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            // Any client sent SHUTDOWN → terminate the whole session.
            _ = shutdown_rx.recv() => break,
            accept = tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept()) => {
                let stream = match accept {
                    Ok(Ok((stream, _))) => stream,
                    Ok(Err(_)) | Err(_) => continue,
                };
                let pty_tx_client = pty_tx.clone();
                let writer_client = writer.clone();
                let child_id_c = child_id.clone();
                let shutdown_client = shutdown_tx.clone();
                tokio::spawn(async move {
                    handle_client(stream, pty_tx_client, writer_client, &child_id_c, shutdown_client).await;
                });
            }
        }
    }

    drop(listener);
    // Terminate the PTY program so `wait_task` (blocked in `child.wait()`)
    // can return — otherwise an explicit SHUTDOWN would hang the daemon.
    let _ = killer.lock().unwrap().kill();
    let _ = reader_task.await;
    let _ = wait_task.await;
    registry::unregister(&child_id)?;
    eprintln!("mimofan-session[{child_id}]: daemon exiting");
    Ok(())
}

/// Serve one attached client: copy socket→PTY (DATA/RESIZE) and PTY→socket
/// (DATA broadcast). DETACH keeps the daemon + PTY alive; SHUTDOWN ends the
/// daemon (via the dedicated `shutdown_tx` channel).
async fn handle_client(
    stream: UnixStream,
    pty_tx: Arc<tokio::sync::broadcast::Sender<Vec<u8>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    id: &str,
    shutdown_tx: tokio::sync::mpsc::Sender<()>,
) {
    let (mut r, mut w) = stream.into_split();
    let mut rx = pty_tx.subscribe();
    let mut read_buf = [0u8; 4096];

    loop {
        tokio::select! {
            n = r.read(&mut read_buf) => {
                match n {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut cursor = std::io::Cursor::new(&read_buf[..n]);
                        let total = n;
                        while (cursor.position() as usize) < total {
                            match read_frame(&mut cursor) {
                                Ok(Some(Frame { opcode, payload })) => match opcode {
                                    OP_DATA => {
                                        if let Ok(mut g) = writer.lock() {
                                            let _ = g.write_all(&payload);
                                            let _ = g.flush();
                                        }
                                    }
                                    OP_RESIZE if payload.len() == 4 => {
                                        let _ = (
                                            u16::from_be_bytes([payload[0], payload[1]]),
                                            u16::from_be_bytes([payload[2], payload[3]]),
                                        );
                                    }
                                    OP_DETACH => break,
                                    OP_SHUTDOWN => {
                                        let _ = shutdown_tx.send(()).await;
                                        break;
                                    }
                                    _ => {}
                                },
                                Ok(None) => break,
                                Err(_) => break,
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            out = rx.recv() => {
                match out {
                    Ok(bytes) => {
                        let mut frame = Vec::new();
                        encode_frame(OP_DATA, &bytes, &mut frame);
                        if w.write_all(&frame).await.is_err() { break; }
                        let _ = w.flush().await;
                    }
                    Err(_) => break,
                }
            }
        }
    }
    eprintln!("mimofan-session[{id}]: client detached");
}
