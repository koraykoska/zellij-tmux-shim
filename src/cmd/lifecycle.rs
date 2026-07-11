//! Lifecycle handlers: creating and destroying panes, windows (zellij tabs),
//! and sessions (tab-name namespaces).

use super::{Ctx, Output};
use crate::cli::args::ParsedInvocation;
use crate::env;
use crate::error::{Result, ShimError};
use crate::format::{context, render};
use crate::idmap;
use crate::zellij::client::Client;
use crate::zellij::types;

pub fn handle(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    match inv.subcommand.as_str() {
        "split-window" => split_window(inv, client, ctx),
        "new-session" => new_session(inv, client, ctx),
        "new-window" => new_window(inv, client, ctx),
        "select-window" => select_window(inv, client),
        "rename-window" => rename_window(inv, client),
        "kill-pane" => kill_pane(inv, client),
        "kill-window" => kill_window(inv, client),
        "kill-session" => kill_session(inv, client),
        "respawn-pane" => respawn_pane(inv, client),
        other => Err(ShimError::BadArgs(format!(
            "lifecycle: unhandled command {other}"
        ))),
    }
}

fn split_window(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    // A directional new-pane splits the FOCUSED pane; focus the split target
    // first, or zellij splits the focused floating plugin and creates nothing.
    if let Some(target) = split_focus_target(inv) {
        let _ = client.focus_pane(&target);
    }
    let direction = if inv.has('h') { "right" } else { "down" };
    let mut args: Vec<String> = vec!["--direction".into(), direction.into()];
    if let Some(cwd) = inv.value('c') {
        args.push("--cwd".into());
        args.push(cwd.to_string());
    }
    append_split_command(&mut args, inv);
    let pane_id = client.new_pane(&as_refs(&args))?;
    print_created(inv, client, ctx, pane_id)
}

fn split_focus_target(inv: &ParsedInvocation) -> Option<String> {
    if let Some(target) = inv.value('t') {
        return idmap::zellij_pane_target(target).ok();
    }
    env::current_pane_int().map(|n| format!("terminal_{n}"))
}

fn new_session(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    let session = inv
        .value('s')
        .ok_or_else(|| ShimError::BadArgs("new-session requires -s".into()))?;
    let mut args: Vec<String> = vec![
        "--name".into(),
        idmap::compose_session_tab_name(session, "1"),
    ];
    append_command(&mut args, inv);
    let pane_id = new_tab_pane(client, &as_refs(&args))?;
    print_created(inv, client, ctx, pane_id)
}

fn new_window(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    let mut args: Vec<String> = vec!["--name".into(), window_tab_name(inv, client)?];
    append_command(&mut args, inv);
    let pane_id = new_tab_pane(client, &as_refs(&args))?;
    print_created(inv, client, ctx, pane_id)
}

fn select_window(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let target = inv
        .value('t')
        .ok_or_else(|| ShimError::BadArgs("select-window requires -t".into()))?;
    client.go_to_tab_by_id(idmap::zellij_tab_from_window(target)?)?;
    Ok(Output::ok())
}

fn rename_window(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let name = inv
        .operands()
        .first()
        .ok_or_else(|| ShimError::BadArgs("rename-window requires a name".into()))?;
    let tab_id = match inv.value('t') {
        Some(target) => idmap::zellij_tab_from_window(target)?,
        None => active_tab_id(client)?,
    };
    client.rename_tab_by_id(tab_id, name)?;
    Ok(Output::ok())
}

fn kill_pane(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let target = inv
        .value('t')
        .ok_or_else(|| ShimError::BadArgs("kill-pane requires -t".into()))?;
    let zellij_id = idmap::zellij_pane_target(target)?;
    // tmux exits 1 with "can't find pane" for a missing target; match that.
    if let Some(id) = idmap::pane_int_from_env(target) {
        if types::pane_by_terminal_id(&client.list_panes()?, id).is_none() {
            return Ok(Output::stderr_line(
                &format!("can't find pane: {target}"),
                1,
            ));
        }
    }
    client.close_pane(&zellij_id)?;
    Ok(Output::ok())
}

fn kill_window(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let target = inv
        .value('t')
        .ok_or_else(|| ShimError::BadArgs("kill-window requires -t".into()))?;
    client.close_tab_by_id(idmap::zellij_tab_from_window(target)?)?;
    Ok(Output::ok())
}

fn kill_session(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let target = inv
        .value('t')
        .ok_or_else(|| ShimError::BadArgs("kill-session requires -t".into()))?;
    for tab in client.list_tabs()? {
        if idmap::is_session_tab(target, &tab.name) {
            client.close_tab_by_id(tab.tab_id)?;
        }
    }
    Ok(Output::ok())
}

fn respawn_pane(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let target = inv
        .value('t')
        .ok_or_else(|| ShimError::BadArgs("respawn-pane requires -t".into()))?;
    let zellij_id = idmap::zellij_pane_target(target)?;
    if let Some(id) = idmap::pane_int_from_env(target) {
        if types::pane_by_terminal_id(&client.list_panes()?, id).is_none() {
            return Ok(Output::stderr_line(
                &format!("can't find pane: {target}"),
                1,
            ));
        }
    }
    // zellij cannot restart a pane's process in place, so replace the pane at its
    // current position via new_pane_in_place. The pane id necessarily changes —
    // zellij allocates a fresh terminal id. Best-effort focus: --in-place acts on
    // the focused pane, and re-focusing an already-focused target errors harmlessly.
    let _ = client.focus_pane(&zellij_id);
    let mut command: Vec<String> = Vec::new();
    append_command(&mut command, inv);
    client.new_pane_in_place(&as_refs(&command))?;
    Ok(Output::ok())
}

fn append_command(args: &mut Vec<String>, inv: &ParsedInvocation) {
    let command = env::wrap_command(&inv.values_of('e'), inv.operands());
    if !command.is_empty() {
        args.push("--".into());
        args.extend(command);
    }
}

fn append_split_command(args: &mut Vec<String>, inv: &ParsedInvocation) {
    let operands = inv.operands();
    if !operands.is_empty() {
        args.push("--".into());
        args.extend(env::wrap_command(&inv.values_of('e'), operands));
        return;
    }
    // No command: when -c is given, spawn the login shell directly (a bare
    // executable, not a shell string) so zellij's --cwd — applied only to an
    // explicit command — takes effect and the pane opens in that directory.
    if inv.value('c').is_some() {
        args.push("--".into());
        args.push(env::login_shell());
    }
}

fn as_refs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn new_tab_pane(client: &Client, tab_args: &[&str]) -> Result<i64> {
    let tab_id = client.new_tab(tab_args)?;
    client
        .list_panes()?
        .iter()
        .filter(|p| !p.is_plugin && p.tab_id == tab_id)
        .map(|p| p.id)
        .min()
        .ok_or_else(|| ShimError::BadArgs(format!("new tab {tab_id} has no pane")))
}

fn window_tab_name(inv: &ParsedInvocation, client: &Client) -> Result<String> {
    let leaf = match inv.value('n') {
        Some(name) => name.to_string(),
        None => match inv.value('t') {
            Some(session) => next_window_index(client, session)?.to_string(),
            None => "window".to_string(),
        },
    };
    Ok(match inv.value('t') {
        Some(session) => idmap::compose_session_tab_name(session, &leaf),
        None => leaf,
    })
}

fn next_window_index(client: &Client, session: &str) -> Result<i64> {
    let prefix = idmap::session_tab_prefix(session);
    let max = client
        .list_tabs()?
        .iter()
        .filter_map(|t| t.name.strip_prefix(&prefix))
        .filter_map(|leaf| leaf.parse::<i64>().ok())
        .max()
        .unwrap_or(0);
    Ok(max + 1)
}

fn active_tab_id(client: &Client) -> Result<i64> {
    types::active_tab(&client.list_tabs()?)
        .map(|t| t.tab_id)
        .ok_or_else(|| ShimError::BadArgs("no active tab".into()))
}

fn print_created(
    inv: &ParsedInvocation,
    client: &Client,
    ctx: &Ctx,
    pane_id: i64,
) -> Result<Output> {
    if !inv.has('P') {
        return Ok(Output::ok());
    }
    let format = inv.value('F').unwrap_or("#{pane_id}");
    if format == "#{pane_id}" {
        return Ok(Output::line(&idmap::tmux_pane_id(pane_id)));
    }
    let panes = client.list_panes()?;
    let tabs = client.list_tabs()?;
    let Some(pane) = types::pane_by_terminal_id(&panes, pane_id) else {
        return Ok(Output::line(&idmap::tmux_pane_id(pane_id)));
    };
    let tab = types::tab_by_id(&tabs, pane.tab_id)
        .or_else(|| types::active_tab(&tabs))
        .ok_or_else(|| ShimError::BadArgs("no active tab".into()))?;
    Ok(Output::line(&render::render(
        format,
        &context::build(pane, tab, &panes, &ctx.session),
    )))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::cli::args::parse;
    use crate::zellij::client::FakeRunner;

    fn inv(a: &[&str]) -> ParsedInvocation {
        parse(&a.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn split_window_h_returns_percent_id() {
        let f = FakeRunner::ok("terminal_7\n");
        let out = handle(
            &inv(&["split-window", "-h", "-P", "-F", "#{pane_id}"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(out.stdout, b"%7\n");
        assert_eq!(
            f.last_call(),
            ["action", "new-pane", "--direction", "right"]
        );
    }

    #[test]
    fn split_window_wraps_command_env() {
        let f = FakeRunner::ok("terminal_1\n");
        handle(
            &inv(&[
                "split-window",
                "-v",
                "-c",
                "/w",
                "-e",
                "K=V",
                "--",
                "run",
                "me",
            ]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        let call = f.last_call();
        assert_eq!(
            &call[..5],
            ["action", "new-pane", "--direction", "down", "--cwd"]
        );
        assert!(call.contains(&"/bin/sh".to_string()));
        assert!(call.iter().any(|s| s.contains("K='V' exec")));
    }

    #[test]
    fn split_window_cwd_without_command_spawns_login_shell() {
        let f = FakeRunner::ok("terminal_4\n");
        handle(
            &inv(&[
                "split-window",
                "-h",
                "-c",
                "/work",
                "-P",
                "-F",
                "#{pane_id}",
            ]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        let call = f.last_call();
        assert_eq!(
            &call[..7],
            [
                "action",
                "new-pane",
                "--direction",
                "right",
                "--cwd",
                "/work",
                "--"
            ]
        );
        assert_eq!(call[7], crate::env::login_shell());
    }

    #[test]
    fn split_window_single_string_command_runs_via_shell() {
        let f = FakeRunner::ok("terminal_9\n");
        handle(
            &inv(&["split-window", "-h", "echo hello world"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        let call = f.last_call();
        assert_eq!(
            &call[call.len() - 4..],
            ["--", "/bin/sh", "-c", "echo hello world"]
        );
    }

    #[test]
    fn new_window_single_string_command_runs_via_shell() {
        let panes = r#"[{"id":7,"is_plugin":false,"is_focused":true,"pane_x":0,"pane_y":0,
            "pane_rows":24,"pane_columns":80,"tab_id":4,"tab_position":0}]"#;
        let f = FakeRunner::routed(&[("new-tab", "4\n"), ("list-panes", panes)]);
        handle(
            &inv(&[
                "new-window",
                "-n",
                "sub",
                "opencode attach http://x --dir /p",
            ]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        let new_tab = f
            .all_calls()
            .into_iter()
            .find(|c| c.contains(&"new-tab".to_string()))
            .unwrap();
        assert_eq!(
            &new_tab[new_tab.len() - 4..],
            ["--", "/bin/sh", "-c", "opencode attach http://x --dir /p"]
        );
    }

    #[test]
    fn split_window_focuses_target_before_directional_split() {
        let f = FakeRunner::ok("terminal_7\n");
        handle(
            &inv(&["split-window", "-h", "-t", "%1", "-P", "-F", "#{pane_id}"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        let calls = f.all_calls();
        assert_eq!(calls[0], ["action", "focus-pane-id", "terminal_1"]);
        assert_eq!(calls[1], ["action", "new-pane", "--direction", "right"]);
    }

    #[test]
    fn new_session_creates_prefixed_tab_and_resolves_pane() {
        let panes = r#"[{"id":5,"is_plugin":false,"is_focused":true,"pane_x":0,"pane_y":0,
            "pane_rows":24,"pane_columns":80,"tab_id":3,"tab_position":0}]"#;
        let f = FakeRunner::routed(&[("new-tab", "3\n"), ("list-panes", panes)]);
        let out = handle(
            &inv(&["new-session", "-d", "-s", "omo-1", "-P", "-F", "#{pane_id}"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(out.stdout, b"%5\n");
        assert_eq!(f.all_calls()[0], ["action", "new-tab", "--name", "omo-1:1"]);
    }

    #[test]
    fn kill_pane_and_select_window_target_correctly() {
        let panes = r#"[{"id":3,"is_plugin":false,"is_focused":true,"pane_x":0,"pane_y":0,
            "pane_rows":24,"pane_columns":80,"tab_id":0,"tab_position":0}]"#;
        let f = FakeRunner::routed(&[("list-panes", panes), ("close-pane", "")]);
        handle(
            &inv(&["kill-pane", "-t", "%3"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(
            f.last_call(),
            ["action", "close-pane", "--pane-id", "terminal_3"]
        );

        let g = FakeRunner::ok("");
        handle(
            &inv(&["select-window", "-t", "@4"]),
            &Client::new(&g),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(g.last_call(), ["action", "go-to-tab-by-id", "4"]);
    }

    #[test]
    fn kill_pane_missing_target_exits_1() {
        let panes = r#"[{"id":0,"is_plugin":false,"is_focused":true,"pane_x":0,"pane_y":0,
            "pane_rows":24,"pane_columns":80,"tab_id":0,"tab_position":0}]"#;
        let f = FakeRunner::routed(&[("list-panes", panes)]);
        let out = handle(
            &inv(&["kill-pane", "-t", "%9"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(out.code, 1);
        assert_eq!(out.stderr, b"can't find pane: %9\n");
    }

    #[test]
    fn respawn_pane_replaces_in_place() {
        let panes = r#"[{"id":3,"is_plugin":false,"is_focused":true,"pane_x":0,"pane_y":0,
            "pane_rows":24,"pane_columns":80,"tab_id":0,"tab_position":0}]"#;
        let f = FakeRunner::routed(&[
            ("list-panes", panes),
            ("focus-pane-id", ""),
            ("new-pane", "terminal_8\n"),
        ]);
        handle(
            &inv(&["respawn-pane", "-t", "%3"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        let calls = f.all_calls();
        assert!(calls
            .iter()
            .any(|c| c == &["action", "focus-pane-id", "terminal_3"]));
        let new_pane = calls
            .iter()
            .find(|c| c.contains(&"new-pane".to_string()))
            .unwrap();
        assert!(new_pane.contains(&"--in-place".to_string()));
        assert!(new_pane.contains(&"--close-replaced-pane".to_string()));
    }

    #[test]
    fn kill_session_closes_every_namespaced_tab() {
        let tabs = r#"[
          {"position":0,"name":"main","active":true,"viewport_rows":24,"viewport_columns":80,"tab_id":0},
          {"position":1,"name":"omo-9:1","active":false,"viewport_rows":24,"viewport_columns":80,"tab_id":1},
          {"position":2,"name":"omo-9:2","active":false,"viewport_rows":24,"viewport_columns":80,"tab_id":2}
        ]"#;
        let f = FakeRunner::routed(&[("list-tabs", tabs), ("close-tab-by-id", "")]);
        handle(
            &inv(&["kill-session", "-t", "omo-9"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        let closes = f
            .all_calls()
            .into_iter()
            .filter(|c| c.contains(&"close-tab-by-id".to_string()))
            .count();
        assert_eq!(closes, 2);
    }

    #[test]
    fn new_window_index_avoids_collision_after_kill() {
        let tabs = r#"[
          {"position":0,"name":"s:1","active":false,"viewport_rows":24,"viewport_columns":80,"tab_id":0},
          {"position":1,"name":"s:3","active":true,"viewport_rows":24,"viewport_columns":80,"tab_id":1}
        ]"#;
        let panes = r#"[{"id":9,"is_plugin":false,"is_focused":true,"pane_x":0,"pane_y":0,
            "pane_rows":24,"pane_columns":80,"tab_id":5,"tab_position":0}]"#;
        let f = FakeRunner::routed(&[
            ("list-tabs", tabs),
            ("new-tab", "5\n"),
            ("list-panes", panes),
        ]);
        handle(
            &inv(&["new-window", "-t", "s", "-P", "-F", "#{pane_id}"]),
            &Client::new(&f),
            &Ctx::test("s"),
        )
        .unwrap();
        let new_tab = f
            .all_calls()
            .into_iter()
            .find(|c| c.contains(&"new-tab".to_string()))
            .unwrap();
        assert!(
            new_tab.contains(&"s:4".to_string()),
            "expected s:4, got {new_tab:?}"
        );
    }
}
