//! Dispatches a parsed tmux invocation to the handler that implements it.

use super::{control, keys, layout, lifecycle, query, resize, Ctx, Output};
use crate::cli::args::ParsedInvocation;
use crate::error::Result;
use crate::zellij::client::{Client, RealRunner};

/// The set of subcommands [`route`] implements. Used as an unknown-command
/// guard BEFORE session resolution so unknown/version/noop never spawn `zellij`.
#[must_use]
fn is_known_subcommand(name: &str) -> bool {
    matches!(
        name,
        "display-message"
            | "list-panes"
            | "list-windows"
            | "list-sessions"
            | "has-session"
            | "capture-pane"
            | "split-window"
            | "new-session"
            | "new-window"
            | "select-window"
            | "rename-window"
            | "kill-pane"
            | "kill-window"
            | "kill-session"
            | "respawn-pane"
            | "send-keys"
            | "select-pane"
            | "resize-pane"
            | "select-layout"
            | "set-option"
            | "set-window-option"
    )
}

pub fn dispatch(inv: &ParsedInvocation) -> Result<Output> {
    if control::is_version(inv) {
        return Ok(control::version());
    }
    if control::is_noop(inv) {
        return Ok(control::noop());
    }
    if !is_known_subcommand(&inv.subcommand) {
        return Ok(Output::stderr_line(
            &format!("unknown command: {}", inv.subcommand),
            1,
        ));
    }
    let runner = RealRunner::new();
    let session = crate::session::resolve_session(&runner, crate::env::session_name())?;
    let client = Client::new(&runner, session.clone());
    let ctx = Ctx::new(session, crate::env::current_pane_int());
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
        "resize-pane" => resize::handle(inv, client, ctx),
        "select-layout" | "set-option" | "set-window-option" => layout::handle(inv, ctx),
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
