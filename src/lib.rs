//! `zellij-tmux-shim` — a `tmux` CLI compatibility shim.
//!
//! Installed onto `PATH` under the name `tmux`, this crate intercepts tmux
//! commands and translates them into `zellij action ...` calls, so tools and AI
//! agents that expect a real tmux server run transparently inside a zellij
//! session. Outside zellij it passes through to the real `tmux` binary.
//!
//! The binary target (`src/main.rs`) is a thin wrapper over [`run`].

pub mod cli;
pub mod cmd;
pub mod env;
pub mod error;
pub mod format;
pub mod idmap;
pub mod session;
pub mod zellij;

use std::process::ExitCode;

/// Translate the current process's tmux invocation and return its exit code.
pub fn run() -> error::Result<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !env::in_zellij() {
        return passthrough(&args);
    }
    let invocation = cli::args::parse(&args)?;
    let output = cmd::router::dispatch(&invocation)?;
    Ok(write_output(&output))
}

fn passthrough(args: &[String]) -> error::Result<ExitCode> {
    let path = std::env::var("PATH").unwrap_or_default();
    match env::find_real_tmux(&path, env::self_dir().as_deref()) {
        Some(real) => Err(env::passthrough(&real, args)),
        None => Err(error::ShimError::RealTmuxMissing),
    }
}

fn write_output(output: &cmd::Output) -> ExitCode {
    use std::io::Write;
    let _ = std::io::stdout().write_all(&output.stdout);
    let _ = std::io::stderr().write_all(&output.stderr);
    ExitCode::from(output.code)
}
