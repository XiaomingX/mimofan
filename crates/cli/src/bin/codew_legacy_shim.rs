//! Convenience `codew` alias.
//!
//! Forwards argv to the `mimofan` dispatcher silently. This is a
//! permanent short-form alias — six fewer keystrokes, same binary.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let args: Vec<String> = env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    let status = match spawn_mimofan(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "error: failed to spawn `mimofan`: {e}. Is it on PATH? \
                 Install with `cargo install mimofan-cli` or via npm/Homebrew."
            );
            std::process::exit(127);
        }
    };
    std::process::exit(status.code().unwrap_or(1));
}

fn spawn_mimofan(args: &[String]) -> std::io::Result<std::process::ExitStatus> {
    // Prefer the dispatcher installed next to this shim. Falling back to PATH
    // first can silently run an older global `mimofan` after a fresh install.
    if let Ok(exe_path) = env::current_exe()
        && let Some(sibling) = sibling_mimofan_path(&exe_path)
        && sibling.is_file()
    {
        return Command::new(sibling).args(args).status();
    }

    // Fall back to PATH for unusual installs that ship only the shim.
    match Command::new("mimofan").args(args).status() {
        Ok(s) => return Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "mimofan not found on PATH or in sibling directory",
    ))
}

fn sibling_mimofan_path(exe_path: &Path) -> Option<PathBuf> {
    exe_path
        .parent()
        .map(|dir| dir.join(format!("mimofan{}", std::env::consts::EXE_SUFFIX)))
}

#[cfg(test)]
mod tests {
    use super::sibling_mimofan_path;
    use std::path::Path;

    #[test]
    fn sibling_dispatcher_uses_platform_executable_suffix() {
        let path = Path::new("/tmp/mimofan-bin/codew");
        let sibling = sibling_mimofan_path(path).expect("sibling");

        assert_eq!(
            sibling,
            Path::new("/tmp/mimofan-bin")
                .join(format!("mimofan{}", std::env::consts::EXE_SUFFIX))
        );
    }
}
