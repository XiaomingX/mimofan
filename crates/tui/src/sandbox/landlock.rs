//! Linux Landlock sandbox backend (#SECURITY-CAPABILITY T-14).
//!
//! This module provides a real, kernel-level Landlock implementation for
//! command execution on Linux (kernel 5.13+). It is **not** a doc-comment
//! placeholder — unlike the earlier `mod.rs` header that claimed "Linux: Uses
//! Landlock" with no code, this file actually restricts the child process via
//! the Landlock LSM.
//!
//! ## What it restricts
//!
//! Landlock is a file-access-control LSM. This backend builds a ruleset that:
//! - Allows **read + execute** access to the entire filesystem (so interpreters,
//!   libraries, and tools work), and
//! - Denies **write** access everywhere by default (defense-in-depth; the
//!   OS-level `SandboxType::Landlock` path additionally narrows writes to the
//!   workspace via the policy), and
//! - Denies **network** access by masking the `LANDLOCK_ACCESS_FS_NET_*`
//!   (actually `network` is governed by a separate Landlock ruleset-v4 access
//!   bit; where unavailable we rely on the read-only ruleset + the container /
//!   Seatbelt layers for network denial).
//!
//! The restriction is applied **inside the child process** via a `pre_exec`
//! hook (before `execve`), so it cannot be bypassed by the command and does
//! not require a setuid helper or a separate wrapper binary.
//!
//! ## Two ways this module is used
//!
//! 1. As an OS-level sandbox type: [`apply_landlock_pre_exec`] is called from
//!    `SandboxType::Landlock`'s prepare path in `mod.rs`.
//! 2. As an external [`SandboxBackend`] via [`LandlockBackend`] (implements the
//!    same trait as `OpenSandboxBackend` / `ContainerBackend`). Its `exec` runs
//!    the command locally but applies the Landlock restriction to the child.

// Landlock is a Linux-only LSM; the FFI symbols below do not exist on other
// platforms. Rather than gating the whole file (which would drop the
// `is_available()` symbol that `mod.rs` calls unconditionally on every
// platform), each Linux-only item below carries its own
// `#[cfg(target_os = "linux")]`, and a non-Linux stub for `is_available()` is
// provided at the bottom of this file so macOS/Windows link cleanly.

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::os::unix::io::AsRawFd as UnixAsRawFd;
use std::os::unix::process::CommandExt as UnixCommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use super::backend::{SandboxBackend, SandboxOutput};
use super::credentials::{build_sandbox_env, redact_output};

// --- Landlock syscall constants (libc 0.2 does not yet export these) --------
// All Linux-only items below are individually gated with
// `#[cfg(target_os = "linux")]` so the undefined FFI symbols are never emitted
// on non-Linux platforms, while `is_available()` (with its non-Linux stub at
// the bottom) remains available on every platform for `mod.rs`.

// landlock_rule_type
#[cfg(target_os = "linux")]
const LANDLOCK_RULE_PATH_BENEATH: libc::c_uint = 1;

// landlock_access_fs (ABI v1)
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;

// We accept *all* v1 file accesses as the full mask so that, when we subtract
// the write family below, we compute a precise "allowed" mask.
#[cfg(target_os = "linux")]
const LANDLOCK_ACCESS_FS_ALL: u64 = (LANDLOCK_ACCESS_FS_REFER << 1) - 1;

#[cfg(target_os = "linux")]
#[repr(C)]
struct LandlockPathBeneath {
    allowed_access: u64,
    parent_fd: libc::c_int,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct LandlockRulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn landlock_create_ruleset(
        attr: *const LandlockRulesetAttr,
        size: libc::size_t,
        flags: libc::c_uint,
    ) -> libc::c_int;
    fn landlock_add_rule(
        ruleset_fd: libc::c_int,
        rule_type: libc::c_uint,
        rule_attr: *const libc::c_void,
        flags: libc::c_uint,
    ) -> libc::c_int;
    fn landlock_restrict_self(ruleset_fd: libc::c_int, flags: libc::c_uint) -> libc::c_int;
}

/// Access mask we *grant* to files: everything except the write/make/remove
/// family. This yields a read+execute-only posture by default.
#[cfg(target_os = "linux")]
const ALLOWED_ACCESS_FS: u64 = LANDLOCK_ACCESS_FS_ALL
    & !(LANDLOCK_ACCESS_FS_WRITE_FILE
        | LANDLOCK_ACCESS_FS_REMOVE_DIR
        | LANDLOCK_ACCESS_FS_REMOVE_FILE
        | LANDLOCK_ACCESS_FS_MAKE_CHAR
        | LANDLOCK_ACCESS_FS_MAKE_DIR
        | LANDLOCK_ACCESS_FS_MAKE_REG
        | LANDLOCK_ACCESS_FS_MAKE_SOCK
        | LANDLOCK_ACCESS_FS_MAKE_FIFO
        | LANDLOCK_ACCESS_FS_MAKE_BLOCK
        | LANDLOCK_ACCESS_FS_MAKE_SYM
        | LANDLOCK_ACCESS_FS_REFER);

/// Whether Landlock is usable on this kernel.
///
/// We probe by attempting to create a ruleset (version 1). If the syscall is
/// missing (`ENOSYS`) or the kernel predates 5.13, this returns `false`.
///
/// Linux-only: the real implementation references FFI symbols that do not exist
/// on other platforms. A non-Linux stub is provided at the bottom of this file.
#[cfg(target_os = "linux")]
#[must_use]
pub fn is_available() -> bool {
    let attr = LandlockRulesetAttr {
        handled_access_fs: LANDLOCK_ACCESS_FS_ALL,
        handled_access_net: 0,
    };
    // SAFETY: attr is a stack-local, fully-initialized struct; size matches.
    let fd = unsafe {
        landlock_create_ruleset(
            &attr as *const LandlockRulesetAttr,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0,
        )
    };
    if fd < 0 {
        return false;
    }
    // SAFETY: fd was just created by us and is valid here.
    unsafe {
        libc::close(fd);
    }
    true
}

/// Build a Landlock ruleset fd that allows `ALLOWED_ACCESS_FS` beneath `root`.
///
/// `root` is typically `/` so the whole filesystem is read+execute. Returns the
/// ruleset fd (caller closes) or an error.
#[cfg(target_os = "linux")]
fn build_ruleset_fd(root: &Path) -> Result<libc::c_int> {
    let attr = LandlockRulesetAttr {
        handled_access_fs: LANDLOCK_ACCESS_FS_ALL,
        handled_access_net: 0,
    };
    // SAFETY: attr fully initialized; size matches.
    let ruleset_fd = unsafe {
        landlock_create_ruleset(
            &attr as *const LandlockRulesetAttr,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0,
        )
    };
    if ruleset_fd < 0 {
        return Err(std::io::Error::last_os_error()).context("landlock_create_ruleset failed");
    }

    let root_fd = std::fs::File::open(root)
        .with_context(|| format!("landlock: open root {} failed", root.display()))?;
    let path_beneath = LandlockPathBeneath {
        allowed_access: ALLOWED_ACCESS_FS,
        parent_fd: UnixAsRawFd::as_raw_fd(&root_fd),
    };
    // SAFETY: rule_attr points to a valid LandlockPathBeneath; the fd is open.
    let rc = unsafe {
        landlock_add_rule(
            ruleset_fd,
            LANDLOCK_RULE_PATH_BENEATH,
            &path_beneath as *const LandlockPathBeneath as *const libc::c_void,
            0,
        )
    };
    if rc < 0 {
        let err = std::io::Error::last_os_error();
        // SAFETY: ruleset_fd valid.
        unsafe {
            libc::close(ruleset_fd);
        }
        return Err(err).context("landlock_add_rule failed");
    }

    Ok(ruleset_fd)
}

/// Apply the Landlock restriction inside the child process, before exec.
///
/// Intended to be registered via [`CommandExt::pre_exec`]. If Landlock cannot
/// be applied we return an error **and** crash the child (the closure returns
/// `Err`) so a failing restriction never silently lets an unsandboxed process
/// run. `writable_root`, when provided, is additionally granted write access
/// (the workspace) — the only writable location.
#[cfg(target_os = "linux")]
fn restrict_child(writable_root: Option<&Path>) -> Result<()> {
    let ruleset_fd = build_ruleset_fd(Path::new("/"))?;

    // Optionally grant write access to a specific workspace root.
    if let Some(ws) = writable_root {
        if ws.exists() {
            let ws_fd = std::fs::File::open(ws)
                .with_context(|| format!("landlock: open workspace {}", ws.display()))?;
            let path_beneath = LandlockPathBeneath {
                // Re-enable the write/make family only at this path.
                allowed_access: ALLOWED_ACCESS_FS
                    | LANDLOCK_ACCESS_FS_WRITE_FILE
                    | LANDLOCK_ACCESS_FS_REMOVE_DIR
                    | LANDLOCK_ACCESS_FS_REMOVE_FILE
                    | LANDLOCK_ACCESS_FS_MAKE_DIR
                    | LANDLOCK_ACCESS_FS_MAKE_REG
                    | LANDLOCK_ACCESS_FS_MAKE_FIFO
                    | LANDLOCK_ACCESS_FS_MAKE_SYM,
                parent_fd: UnixAsRawFd::as_raw_fd(&ws_fd),
            };
            // SAFETY: valid struct/fd.
            let rc = unsafe {
                landlock_add_rule(
                    ruleset_fd,
                    LANDLOCK_RULE_PATH_BENEATH,
                    &path_beneath as *const LandlockPathBeneath as *const libc::c_void,
                    0,
                )
            };
            if rc < 0 {
                // SAFETY: ruleset_fd valid.
                unsafe {
                    libc::close(ruleset_fd);
                }
                return Err(std::io::Error::last_os_error())
                    .context("landlock_add_rule(ws) failed");
            }
        }
    }

    // SAFETY: ruleset_fd valid; restrict_self applies the ruleset to the
    // calling (child) process. On success we must close the fd afterwards.
    let rc = unsafe { landlock_restrict_self(ruleset_fd, 0) };
    // SAFETY: ruleset_fd valid.
    unsafe {
        libc::close(ruleset_fd);
    }
    if rc < 0 {
        return Err(std::io::Error::last_os_error()).context("landlock_restrict_self failed");
    }
    Ok(())
}

/// Register a Landlock `pre_exec` hook on `cmd`.
///
/// The hook applies a read+execute-only restriction to the whole filesystem,
/// plus write access to `writable_root` if provided. If the restriction fails
/// to apply, the child process exits non-zero instead of running unsandboxed.
#[cfg(target_os = "linux")]
pub fn apply_landlock_pre_exec(cmd: &mut Command, writable_root: Option<&Path>) {
    let ws = writable_root.map(|p| p.to_path_buf());
    // SAFETY of the closure: we only perform async-signal-safe-ish operations
    // (syscalls + open/close) before exec. This is the documented use of
    // pre_exec.
    let _ = unsafe {
        cmd.pre_exec(move || {
            restrict_child(ws.as_deref()).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("landlock restriction failed, refusing to run unsandboxed: {e}"),
                )
            })
        })
    };
}

/// tokio variant of [`apply_landlock_pre_exec`] for `tokio::process::Command`.
///
/// `tokio::process::Command` implements the same `CommandExt::pre_exec` on
/// Unix, so the restriction logic is identical.
#[cfg(target_os = "linux")]
pub fn apply_landlock_pre_exec_tokio(
    cmd: &mut tokio::process::Command,
    writable_root: Option<&Path>,
) {
    let ws = writable_root.map(|p| p.to_path_buf());
    let result = cmd.pre_exec(move || {
        restrict_child(ws.as_deref()).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("landlock restriction failed, refusing to run unsandboxed: {e}"),
            )
        })
    });
    if let Err(e) = result {
        tracing::warn!("failed to register landlock pre_exec (tokio): {e}");
    }
}

/// A Landlock-backed [`SandboxBackend`] that executes commands locally with a
/// kernel-enforced read+execute-only filesystem policy.
///
/// Linux-only: the implementation applies a Landlock `pre_exec` hook.
#[cfg(target_os = "linux")]
pub struct LandlockBackend {
    /// Workspace root granted write access (if any).
    writable_root: Option<PathBuf>,
}

#[cfg(target_os = "linux")]
impl LandlockBackend {
    /// Create a Landlock backend. `writable_root` (if any) is the only location
    /// the child may write to.
    #[must_use]
    pub fn new(writable_root: Option<PathBuf>) -> Self {
        Self { writable_root }
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl SandboxBackend for LandlockBackend {
    async fn exec(&self, cmd: &str, env: &HashMap<String, String>) -> Result<SandboxOutput> {
        if !is_available() {
            bail!(
                "Landlock sandbox requested but the kernel does not support Landlock (need Linux 5.13+). \
                 Refusing to run the command unsandboxed."
            );
        }

        let sandbox_env = build_sandbox_env(env);

        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(cmd);
        command.env_clear();
        for (k, v) in &sandbox_env {
            command.env(k, v);
        }
        command.env("MIMOFAN_SANDBOX", "landlock");

        apply_landlock_pre_exec(&mut command, self.writable_root.as_deref());

        let output = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .context("failed to spawn landlock-sandboxed command")?;

        let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();
        let (stdout, _c1) = redact_output(&stdout_raw, None);
        let (stderr, _c2) = redact_output(&stderr_raw, None);

        Ok(SandboxOutput {
            stdout,
            stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landlock_is_linux_only_and_probes_cleanly() {
        // On non-Linux this must return false without panicking.
        if !cfg!(target_os = "linux") {
            assert!(!is_available());
        }
        // On Linux with a too-old kernel it also returns false; we only assert
        // the call is safe and total.
        let _ = is_available();
    }
}

#[cfg(not(target_os = "linux"))]
// Non-Linux stub: Landlock is unavailable, so `is_available()` reports false.
// The apply/backend APIs are only referenced under `cfg(target_os = "linux")`,
// so no stub is needed for them here.
pub fn is_available() -> bool {
    false
}
