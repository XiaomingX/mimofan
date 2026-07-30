//! Binary entry point for mimofan.
//!
//! The TUI binary has its own full argument parser and handles all subcommands
//! (exec, doctor, models, serve, etc.) directly. CLI-native commands (login,
//! logout, config, etc.) are handled by the CLI binary (`mimofan-cli`).

fn main() -> std::process::ExitCode {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");
    match rt.block_on(mimofan::run()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            for cause in err.chain().skip(1) {
                eprintln!("  caused by: {cause}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}
