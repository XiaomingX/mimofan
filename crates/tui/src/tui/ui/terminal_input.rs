//! Terminal input polling pump.
//!
//! Isolates raw crossterm event reads behind a background thread so the main
//! TUI loop never blocks on OS input and can stay responsive to engine events.
//! Moved out of `ui/mod.rs` during the god-file slicing refactor.

use std::cell::Cell;
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};

const TERMINAL_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const TERMINAL_INPUT_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) enum TerminalInputMessage {
    Event(Event),
    Heartbeat,
    Error(io::Error),
}

pub(crate) struct TerminalInputPump {
    rx: std::sync::mpsc::Receiver<TerminalInputMessage>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    last_alive_at: Cell<Instant>,
}

impl TerminalInputPump {
    pub(crate) fn spawn() -> io::Result<Self> {
        let (rx, stop, handle) = Self::spawn_parts()?;
        Ok(Self {
            rx,
            stop,
            handle: Some(handle),
            last_alive_at: Cell::new(Instant::now()),
        })
    }

    fn spawn_parts() -> io::Result<(
        std::sync::mpsc::Receiver<TerminalInputMessage>,
        Arc<AtomicBool>,
        JoinHandle<()>,
    )> {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("mimofan-terminal-input".to_string())
            .spawn(move || {
                let mut last_heartbeat = Instant::now();
                while !thread_stop.load(Ordering::Acquire) {
                    match event::poll(TERMINAL_INPUT_POLL_INTERVAL) {
                        Ok(true) => match event::read() {
                            Ok(event) => {
                                last_heartbeat = Instant::now();
                                if tx.send(TerminalInputMessage::Event(event)).is_err() {
                                    break;
                                }
                            }
                            Err(err) => {
                                let _ = tx.send(TerminalInputMessage::Error(err));
                                break;
                            }
                        },
                        Ok(false) => {
                            let now = Instant::now();
                            if now.duration_since(last_heartbeat)
                                >= TERMINAL_INPUT_HEARTBEAT_INTERVAL
                            {
                                last_heartbeat = now;
                                if tx.send(TerminalInputMessage::Heartbeat).is_err() {
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            let _ = tx.send(TerminalInputMessage::Error(err));
                            break;
                        }
                    }
                }
            })?;
        Ok((rx, stop, handle))
    }

    pub(crate) fn recv_timeout(&self, timeout: Duration) -> io::Result<Option<Event>> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(remaining) {
                Ok(TerminalInputMessage::Event(event)) => {
                    self.mark_alive();
                    return Ok(Some(event));
                }
                Ok(TerminalInputMessage::Heartbeat) => {
                    self.mark_alive();
                    if remaining.is_zero() {
                        return Ok(None);
                    }
                }
                Ok(TerminalInputMessage::Error(err)) => {
                    self.mark_alive();
                    return Err(err);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Ok(None),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "terminal input pump disconnected",
                    ));
                }
            }
        }
    }

    pub(crate) fn try_recv(&self) -> io::Result<Option<Event>> {
        loop {
            match self.rx.try_recv() {
                Ok(TerminalInputMessage::Event(event)) => {
                    self.mark_alive();
                    return Ok(Some(event));
                }
                Ok(TerminalInputMessage::Heartbeat) => {
                    self.mark_alive();
                }
                Ok(TerminalInputMessage::Error(err)) => {
                    self.mark_alive();
                    return Err(err);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(None),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(None),
            }
        }
    }

    pub(crate) fn mark_alive(&self) {
        self.last_alive_at.set(Instant::now());
    }

    pub(crate) fn stalled_for(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.last_alive_at.get())
    }
}

impl Drop for TerminalInputPump {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            #[cfg(not(target_os = "windows"))]
            let _ = handle.join();
        }
    }
}
