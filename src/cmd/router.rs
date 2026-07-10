//! Dispatches a parsed tmux invocation to the handler that implements it.

use super::{control, keys, layout, lifecycle, query, Ctx, Output};
use crate::cli::args::ParsedInvocation;
use crate::error::Result;
use crate::zellij::client::{Client, RealRunner};

pub fn dispatch(inv: &ParsedInvocation) -> Result<Output> {
    if control::is_version(inv) {
        return Ok(control::version());
    }
    if control::is_noop(inv) {
        return Ok(control::noop());
    }
    let runner = RealRunner::new();
    let client = Client::new(&runner);
    let ctx = Ctx::from_env();
    route(inv, &client, &ctx)
}

fn route(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    match inv.subcommand.as_str() {
        "display-message" | "list-panes" | "list-windows" | "list-sessions" | "has-session"
        | "capture-pane" => query::handle(inv, client, ctx),
        "split-window" | "new-session" | "new-window" | "select-window" | "rename-window"
        | "kill-pane" | "kill-window" | "kill-session" | "respawn-pane" => {
            lifecycle::handle(inv, client, ctx)
        }
        "send-keys" | "select-pane" => keys::handle(inv, client, ctx),
        "select-layout" | "set-option" | "set-window-option" | "resize-pane" => {
            layout::handle(inv, ctx)
        }
        other => Ok(Output::stderr_line(&format!("unknown command: {other}"), 1)),
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
    fn dispatch_version_and_noop_and_unknown_without_zellij() {
        assert_eq!(dispatch(&inv(&["-V"])).unwrap().stdout, b"tmux 3.4\n");

        let noop = dispatch(&inv(&["source-file", "/etc/x"])).unwrap();
        assert_eq!(noop.code, 0);

        let unknown = dispatch(&inv(&["bogus-command"])).unwrap();
        assert_eq!(unknown.code, 1);
        assert_eq!(unknown.stderr, b"unknown command: bogus-command\n");
    }
}
