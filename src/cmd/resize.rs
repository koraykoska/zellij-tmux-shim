//! resize-pane: tmux's absolute (`-x`/`-y`, cells or `N%`) and relative
//! (`-U`/`-D`/`-L`/`-R [N]`) resizing, plus `-Z` zoom, mapped onto zellij's
//! coarse relative resize. Absolute targets are approached with a bounded
//! feedback loop — zellij's step is ~5% of the dimension, so exact cell parity is
//! impossible; `-R`/`-D` grow and `-L`/`-U` shrink; `-Z` toggles fullscreen.

use super::{Ctx, Output};
use crate::cli::args::ParsedInvocation;
use crate::error::{Result, ShimError};
use crate::idmap;
use crate::zellij::client::Client;
use crate::zellij::types::{self, PaneInfo};

/// zellij's resize step is coarse, so bound the convergence loop to guarantee
/// termination even if a target is unreachable (e.g. pane pinned to an edge).
const MAX_STEPS: usize = 24;

#[derive(Clone, Copy)]
enum Axis {
    Width,
    Height,
}

impl Axis {
    fn of(self, pane: &PaneInfo) -> i64 {
        match self {
            Axis::Width => pane.pane_columns,
            Axis::Height => pane.pane_rows,
        }
    }

    fn borders(self) -> [&'static str; 2] {
        match self {
            Axis::Width => ["right", "left"],
            Axis::Height => ["down", "up"],
        }
    }
}

pub fn handle(inv: &ParsedInvocation, client: &Client, ctx: &Ctx) -> Result<Output> {
    let panes = client.list_panes()?;
    let pane = resolve(inv, &panes, ctx.current_pane)
        .ok_or_else(|| ShimError::NoSuchPane(inv.value('t').unwrap_or_default().to_string()))?;
    let pane_id = pane.id;
    let zid = format!("terminal_{pane_id}");

    if inv.has('Z') {
        client.toggle_fullscreen(&zid)?;
        return Ok(Output::ok());
    }

    let adjust = inv
        .operands()
        .first()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1);
    let (vp_cols, vp_rows) = viewport(client, pane.tab_id)?;

    if let Some(target) = size_arg(inv.value('x'), vp_cols) {
        converge(client, &zid, pane_id, Axis::Width, target)?;
    } else if inv.has('L') || inv.has('R') {
        let delta = if inv.has('R') { adjust } else { -adjust };
        converge(
            client,
            &zid,
            pane_id,
            Axis::Width,
            (pane.pane_columns + delta).max(1),
        )?;
    }

    if let Some(target) = size_arg(inv.value('y'), vp_rows) {
        converge(client, &zid, pane_id, Axis::Height, target)?;
    } else if inv.has('U') || inv.has('D') {
        let delta = if inv.has('D') { adjust } else { -adjust };
        converge(
            client,
            &zid,
            pane_id,
            Axis::Height,
            (pane.pane_rows + delta).max(1),
        )?;
    }

    Ok(Output::ok())
}

fn resolve<'a>(
    inv: &ParsedInvocation,
    panes: &'a [PaneInfo],
    caller: Option<i64>,
) -> Option<&'a PaneInfo> {
    if let Some(target) = inv.value('t') {
        if let Some(id) = idmap::pane_int_from_env(target) {
            return types::pane_by_terminal_id(panes, id);
        }
    }
    caller
        .and_then(|id| types::pane_by_terminal_id(panes, id))
        .or_else(|| types::active_terminal(panes))
}

fn size_arg(raw: Option<&str>, viewport: i64) -> Option<i64> {
    let raw = raw?;
    if let Some(pct) = raw.strip_suffix('%') {
        pct.parse::<i64>().ok().map(|p| (viewport * p / 100).max(1))
    } else {
        raw.parse::<i64>().ok().filter(|n| *n > 0)
    }
}

fn viewport(client: &Client, tab_id: i64) -> Result<(i64, i64)> {
    let tabs = client.list_tabs()?;
    Ok(types::tab_by_id(&tabs, tab_id).map_or((0, 0), |t| (t.viewport_columns, t.viewport_rows)))
}

fn converge(client: &Client, zid: &str, pane_id: i64, axis: Axis, target: i64) -> Result<()> {
    let borders = axis.borders();
    let mut border = 0usize;
    let mut cur = measure(client, pane_id, axis)?;
    for _ in 0..MAX_STEPS {
        if cur == target {
            return Ok(());
        }
        let verb = if cur < target { "increase" } else { "decrease" };
        client.resize(zid, &[verb, borders[border]])?;
        let now = measure(client, pane_id, axis)?;
        if now == cur {
            // This border can't move (pane against a screen edge); try the other.
            border += 1;
            if border >= borders.len() {
                return Ok(());
            }
            continue;
        }
        if (cur < target) != (now < target) {
            return Ok(());
        }
        cur = now;
    }
    Ok(())
}

fn measure(client: &Client, pane_id: i64, axis: Axis) -> Result<i64> {
    let panes = client.list_panes()?;
    types::pane_by_terminal_id(&panes, pane_id)
        .map(|p| axis.of(p))
        .ok_or_else(|| ShimError::NoSuchPane(format!("terminal_{pane_id}")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::cli::args::parse;
    use crate::zellij::client::FakeRunner;

    const PANE: &str = r#"[{"id":1,"is_plugin":false,"is_focused":true,"pane_x":0,"pane_y":0,
        "pane_rows":24,"pane_columns":80,"tab_id":0,"tab_position":0}]"#;
    const TABS: &str = r#"[{"position":0,"name":"m","active":true,"viewport_rows":48,
        "viewport_columns":160,"tab_id":0}]"#;

    fn inv(a: &[&str]) -> ParsedInvocation {
        parse(&a.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()).unwrap()
    }

    fn fake() -> FakeRunner {
        FakeRunner::routed(&[
            ("list-panes", PANE),
            ("list-tabs", TABS),
            ("resize", ""),
            ("toggle-fullscreen", ""),
            ("focus-pane-id", ""),
        ])
    }

    fn resizes(f: &FakeRunner) -> Vec<Vec<String>> {
        f.all_actions()
            .into_iter()
            .filter(|c| c.contains(&"resize".to_string()))
            .collect()
    }

    #[test]
    fn absolute_x_larger_grows_width_at_right_border() {
        let f = fake();
        handle(
            &inv(&["resize-pane", "-t", "%1", "-x", "120"]),
            &Client::new(&f, "s".to_string()),
            &Ctx::test("s"),
        )
        .unwrap();
        let r = resizes(&f);
        assert_eq!(
            r[0],
            [
                "action",
                "resize",
                "--pane-id",
                "terminal_1",
                "increase",
                "right"
            ]
        );
    }

    #[test]
    fn relative_l_shrinks_width() {
        let f = fake();
        handle(
            &inv(&["resize-pane", "-t", "%1", "-L", "5"]),
            &Client::new(&f, "s".to_string()),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(
            resizes(&f)[0],
            [
                "action",
                "resize",
                "--pane-id",
                "terminal_1",
                "decrease",
                "right"
            ]
        );
    }

    #[test]
    fn absolute_percent_y_resolves_against_viewport() {
        let f = fake();
        handle(
            &inv(&["resize-pane", "-t", "%1", "-y", "25%"]),
            &Client::new(&f, "s".to_string()),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(
            resizes(&f)[0],
            [
                "action",
                "resize",
                "--pane-id",
                "terminal_1",
                "decrease",
                "down"
            ]
        );
    }

    #[test]
    fn zoom_focuses_then_toggles_fullscreen() {
        let f = fake();
        handle(
            &inv(&["resize-pane", "-t", "%1", "-Z"]),
            &Client::new(&f, "s".to_string()),
            &Ctx::test("s"),
        )
        .unwrap();
        let calls = f.all_actions();
        assert!(calls
            .iter()
            .any(|c| c == &["action", "focus-pane-id", "terminal_1"]));
        assert!(calls
            .iter()
            .any(|c| c.contains(&"toggle-fullscreen".to_string())));
        assert!(resizes(&f).is_empty());
    }
}
