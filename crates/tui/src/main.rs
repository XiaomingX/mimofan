//! Binary entry point for mimofan.
//!
//! Delegates to `mimofan_cli::run_cli()` which handles CLI argument parsing
//! and either runs the TUI directly or spawns sub-commands.

fn main() -> std::process::ExitCode {
    mimofan_cli::run_cli()
}
