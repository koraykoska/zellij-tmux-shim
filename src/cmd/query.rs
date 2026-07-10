//! Query handlers: display-message, list-panes/-windows/-sessions, has-session,
//! capture-pane. Each reads live zellij state and renders a tmux format string.

use super::{Ctx, Output};
use crate::cli::args::ParsedInvocation;
use crate::error::{Result, ShimError};
use crate::format::{context, render};
use crate::idmap;
use crate::zellij::client::Client;
use crate::zellij::types::{self, PaneInfo, TabInfo};

pub fn handle(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    match inv.subcommand.as_str() {
        "display-message" => display_message(inv, client, ctx),
        "list-panes" => list_panes(inv, client, ctx),
        "list-windows" => list_windows(inv, client, ctx),
        "list-sessions" => list_sessions(inv, client, ctx),
        "has-session" => has_session(inv, client),
        "capture-pane" => capture_pane(inv, client),
        other => Err(ShimError::BadArgs(format!(
            "query: unhandled command {other}"
        ))),
    }
}

fn format_of<'a>(inv: &'a ParsedInvocation, default: &'a str) -> &'a str {
    inv.value('F')
        .or_else(|| inv.operands().first().map(String::as_str))
        .unwrap_or(default)
}

fn display_message(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    let Some(format) = inv
        .value('F')
        .or_else(|| inv.operands().first().map(String::as_str))
    else {
        return Ok(Output::ok());
    };
    let panes = client.list_panes()?;
    let tabs = client.list_tabs()?;
    let pane = resolve_pane(inv, &panes)
        .ok_or_else(|| ShimError::NoSuchPane(inv.value('t').unwrap_or_default().to_string()))?;
    let tab = types::tab_by_id(&tabs, pane.tab_id)
        .or_else(|| types::active_tab(&tabs))
        .ok_or_else(|| ShimError::BadArgs("no active tab".into()))?;
    let fctx = context::build(pane, tab, &panes, &ctx.session);
    Ok(Output::line(&render::render(format, &fctx)))
}

fn list_panes(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    let format = format_of(
        inv,
        "#{pane_index}: [#{pane_width}x#{pane_height}] #{pane_id}",
    );
    let panes = client.list_panes()?;
    let tabs = client.list_tabs()?;

    let mut selected: Vec<&PaneInfo> = if inv.has('a') || inv.has('s') {
        types::terminal_panes(&panes)
    } else {
        let tab_id = resolve_tab_id(inv, &tabs, &panes);
        types::terminal_panes(&panes)
            .into_iter()
            .filter(|p| Some(p.tab_id) == tab_id)
            .collect()
    };
    selected.sort_by_key(|p| p.id);

    let lines: Vec<String> = selected
        .into_iter()
        .filter_map(|p| {
            types::tab_by_id(&tabs, p.tab_id)
                .or_else(|| types::active_tab(&tabs))
                .map(|t| render::render(format, &context::build(p, t, &panes, &ctx.session)))
        })
        .collect();
    Ok(Output::lines(&lines))
}

fn list_windows(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    let format = format_of(inv, "#{window_index}: #{window_name}");
    let tabs = client.list_tabs()?;
    let selected: Vec<&TabInfo> = match inv.value('t') {
        Some(session) => {
            let scoped: Vec<&TabInfo> = tabs
                .iter()
                .filter(|t| idmap::is_session_tab(session, &t.name))
                .collect();
            if scoped.is_empty() {
                tabs.iter().collect()
            } else {
                scoped
            }
        }
        None => tabs.iter().collect(),
    };
    let lines: Vec<String> = selected
        .into_iter()
        .map(|t| render::render(format, &context::build_window(t, &ctx.session)))
        .collect();
    Ok(Output::lines(&lines))
}

fn list_sessions(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    let format = format_of(inv, "#{session_name}");
    let tabs = client.list_tabs()?;
    let mut names: Vec<String> = vec![ctx.session.clone()];
    for tab in &tabs {
        if let Some(session) = idmap::session_of_tab_name(&tab.name) {
            if !names.iter().any(|n| n == session) {
                names.push(session.to_string());
            }
        }
    }
    let lines: Vec<String> = names
        .iter()
        .map(|n| render::render(format, &context::build_session(n)))
        .collect();
    Ok(Output::lines(&lines))
}

fn has_session(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let Some(target) = inv.value('t') else {
        // No target means the current session, which always exists inside zellij.
        return Ok(Output::ok());
    };
    let tabs = client.list_tabs()?;
    if tabs.iter().any(|t| idmap::is_session_tab(target, &t.name)) {
        Ok(Output::ok())
    } else {
        Ok(Output::stderr_line(
            &format!("can't find session: {target}"),
            1,
        ))
    }
}

fn capture_pane(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let panes = client.list_panes()?;
    let pane = resolve_pane(inv, &panes)
        .ok_or_else(|| ShimError::NoSuchPane(inv.value('t').unwrap_or_default().to_string()))?;
    let content = client.dump_screen_full(&format!("terminal_{}", pane.id))?;
    let out = match inv.value('S') {
        Some(start) => tail_lines(&content, start),
        None => content,
    };
    Ok(Output::bytes(out.into_bytes()))
}

fn resolve_pane<'a>(inv: &ParsedInvocation, panes: &'a [PaneInfo]) -> Option<&'a PaneInfo> {
    if let Some(target) = inv.value('t') {
        if let Some(id) = idmap::pane_int_from_env(target) {
            if let Some(pane) = types::pane_by_terminal_id(panes, id) {
                return Some(pane);
            }
        }
    }
    types::active_terminal(panes)
}

fn resolve_tab_id(inv: &ParsedInvocation, tabs: &[TabInfo], panes: &[PaneInfo]) -> Option<i64> {
    if let Some(target) = inv.value('t') {
        if let Ok(tab_id) = idmap::zellij_tab_from_window(target) {
            if types::tab_by_id(tabs, tab_id).is_some() {
                return Some(tab_id);
            }
        }
        if let Some(id) = idmap::pane_int_from_env(target) {
            if let Some(pane) = types::pane_by_terminal_id(panes, id) {
                return Some(pane.tab_id);
            }
        }
    }
    types::active_tab(tabs).map(|t| t.tab_id)
}

fn tail_lines(content: &str, start: &str) -> String {
    if let Ok(n) = start.parse::<i64>() {
        if n < 0 {
            let want = usize::try_from(n.unsigned_abs()).unwrap_or(usize::MAX);
            let lines: Vec<&str> = content.lines().collect();
            let from = lines.len().saturating_sub(want);
            let mut joined = lines[from..].join("\n");
            if content.ends_with('\n') && !joined.is_empty() {
                joined.push('\n');
            }
            return joined;
        }
    }
    content.to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::cli::args::parse;
    use crate::zellij::client::FakeRunner;

    const PANES: &str = r#"[
      {"id":0,"is_plugin":true,"is_focused":true,"is_floating":true,"title":"About",
       "pane_x":0,"pane_y":0,"pane_rows":10,"pane_columns":10,"tab_id":0,"tab_position":0,"tab_name":"main"},
      {"id":0,"is_plugin":false,"is_focused":true,"is_floating":false,"title":"editor",
       "pane_x":0,"pane_y":0,"pane_rows":50,"pane_columns":120,"pane_command":"vim","pane_cwd":"/p",
       "tab_id":0,"tab_position":0,"tab_name":"main"},
      {"id":1,"is_plugin":false,"is_focused":false,"is_floating":false,"title":"shell",
       "pane_x":0,"pane_y":50,"pane_rows":50,"pane_columns":120,"pane_command":"zsh","pane_cwd":"/p",
       "tab_id":0,"tab_position":0,"tab_name":"main"}
    ]"#;
    const TABS: &str = r#"[
      {"position":0,"name":"main","active":true,"viewport_rows":100,"viewport_columns":120,"tab_id":0},
      {"position":1,"name":"omo-agents-9:1","active":false,"viewport_rows":100,"viewport_columns":120,"tab_id":1}
    ]"#;

    fn inv(a: &[&str]) -> ParsedInvocation {
        parse(&a.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()).unwrap()
    }

    fn fake() -> FakeRunner {
        FakeRunner::routed(&[("list-panes", PANES), ("list-tabs", TABS)])
    }

    #[test]
    fn display_pane_and_window_geometry_positional() {
        let f = fake();
        let c = Client::new(&f);
        let out = handle(
            &inv(&["display", "-p", "-t", "%1", "#{pane_width},#{window_width}"]),
            &c,
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(out.stdout, b"120,120\n");
    }

    #[test]
    fn display_uses_active_terminal_when_untargeted() {
        let f = fake();
        let c = Client::new(&f);
        let out = handle(
            &inv(&["display-message", "-p", "-F", "#{pane_current_command}"]),
            &c,
            &Ctx::test("s"),
        )
        .unwrap();
        // active terminal (not the focused plugin) is the vim pane
        assert_eq!(out.stdout, b"vim\n");
    }

    #[test]
    fn list_panes_emits_terminal_ids_only() {
        let f = fake();
        let c = Client::new(&f);
        let out = handle(
            &inv(&["list-panes", "-F", "#{pane_id}"]),
            &c,
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(out.stdout, b"%0\n%1\n");
    }

    #[test]
    fn has_session_present_and_absent() {
        let present = handle(
            &inv(&["has-session", "-t", "omo-agents-9"]),
            &Client::new(&fake()),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(present.code, 0);
        assert!(present.stderr.is_empty());
        let absent = handle(
            &inv(&["has-session", "-t", "nope"]),
            &Client::new(&fake()),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(absent.code, 1);
        assert_eq!(absent.stderr, b"can't find session: nope\n");
    }

    #[test]
    fn list_sessions_includes_base_and_namespaces() {
        let f = fake();
        let c = Client::new(&f);
        let out = handle(
            &inv(&["list-sessions", "-F", "#{session_name}"]),
            &c,
            &Ctx::test("base"),
        )
        .unwrap();
        assert_eq!(out.stdout, b"base\nomo-agents-9\n");
    }

    #[test]
    fn tail_lines_takes_last_n() {
        let content = "1\n2\n3\n4\n5\n";
        assert_eq!(tail_lines(content, "-2"), "4\n5\n");
    }
}
