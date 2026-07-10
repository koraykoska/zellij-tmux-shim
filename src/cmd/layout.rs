//! Layout and option handlers.
//!
//! zellij's tiling model and relative-only resize cannot honor tmux's absolute
//! layout/resize/option requests, so these are accepted (exit 0) as no-ops.
//! Agent tools re-query live geometry afterward and adapt, so succeeding here
//! while reporting true geometry from the query handlers keeps them working.

use super::{Ctx, Output};
use crate::cli::args::ParsedInvocation;
use crate::error::{Result, ShimError};

pub fn handle(inv: &ParsedInvocation, _ctx: &Ctx) -> Result<Output> {
    match inv.subcommand.as_str() {
        "select-layout" | "set-option" | "set-window-option" | "resize-pane" => Ok(Output::ok()),
        other => Err(ShimError::BadArgs(format!(
            "layout: unhandled command {other}"
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::cli::args::parse;

    fn inv(a: &[&str]) -> ParsedInvocation {
        parse(&a.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn layout_and_option_commands_are_noop_exit0() {
        for cmd in [
            "select-layout",
            "set-option",
            "set-window-option",
            "resize-pane",
        ] {
            let out = handle(&inv(&[cmd, "-t", "%1"]), &Ctx::test("s")).unwrap();
            assert_eq!(out.code, 0);
            assert!(out.stdout.is_empty() && out.stderr.is_empty());
        }
    }
}
