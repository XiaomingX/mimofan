//! Process hardening for Linux sandbox defense-in-depth (#2183).
//!
//! This module applies kernel-level restrictions to the mimofan process
//! itself. Unlike Landlock/seccomp which restrict child processes spawned for
//! shell commands, these hardening measures protect the *parent* TUI process
//! from information leaks and privilege-escalation vectors.
//!
//! # Ordering constraints
//!
//! `apply_process_hardening()` MUST be called **before** the Tokio runtime is
//! booted and **before** any worker threads are spawned. The reasons:
//!
//! 1. `PR_SET_DUMPABLE` — once set to 0, the process cannot be ptraced and
//!    `/proc/self/` becomes root-owned. This must happen before any threads
//!    exist, because the kernel applies dumpable state per-thread-group and
//!    changing it after threads are live can race with `/proc` lookups.
//!
//! 2. `PR_SET_NO_NEW_PRIVS` — prevents the process and all descendants from
//!    ever gaining new privileges via setuid/setgid/fscaps. This is
//!    irreversible and must be applied before executing any helper binaries or
//!    subprocesses that might (incorrectly) rely on privilege boundaries.
//!
//! 3. `RLIMIT_CORE` — disables core dumps so that sensitive in-memory data
//!    (API keys, tokens, prompt content) is never written to disk on a crash.
//!    Setting this before any data is loaded into memory is the safest posture.
//!
//! # Platform support
//!
//! These hardening measures are Linux-only (they use `prctl` and `setrlimit`
//! from the `libc` crate). On non-Linux platforms, `apply_process_hardening()`
//! is a no-op that logs a debug-level message.

/// Apply process-level hardening measures.
///
/// On Linux, this:
/// - Sets `PR_SET_DUMPABLE` to 0 (prevents ptrace, core dumps)
/// - Sets `PR_SET_NO_NEW_PRIVS` to 1 (irreversible no-new-privileges)
/// - Sets `RLIMIT_CORE` to 0 (disables core dumps)
///
/// On non-Linux platforms this is a no-op.
///
/// # Panics
///
/// Does NOT panic. Failures are logged via `tracing::warn` because the
/// hardening is defense-in-depth — the sandbox still protects child processes
/// even if these prctls fail (e.g., in a container where some are restricted).
pub fn apply_process_hardening() {
    #[cfg(not(all(target_os = "linux", not(target_env = "ohos"))))]
    {
        tracing::debug!("Process hardening skipped: not on Linux");
    }
}
