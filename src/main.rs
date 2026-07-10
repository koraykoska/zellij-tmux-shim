//! Binary entry point for the `tmux` shim.
//!
//! All translation logic lives in the `zellij_tmux_shim` library crate; this
//! wrapper only maps [`zellij_tmux_shim::run`] to a process exit code.

use std::process::ExitCode;

fn main() -> ExitCode {
    match zellij_tmux_shim::run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.exit_code())
        }
    }
}
