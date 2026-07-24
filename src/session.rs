//! Live zellij session resolution.
//!
//! When the user renames the running zellij session, every process keeps a
//! stale `$ZELLIJ_SESSION_NAME`. This module resolves the LIVE session name by
//! probing `zellij list-sessions -n` and passes `zellij --session <resolved>
//! action ...` on every action so the shim never targets a dead session.
//!
//! The pure resolver [`resolve_from`] handles the fast path and the
//! unambiguous single-session case. The orchestrator [`resolve_session`]
//! additionally drives [`cache`] multi-session rename recovery: a per-env-name
//! cache persists the last live-set snapshot and resolved target so a rename
//! can be diffed exactly once and then served from a stable ledger hit.

pub mod cache;

use crate::error::{Result, ShimError};
use crate::zellij::client::{Client, ZellijRunner};

/// Parse `zellij list-sessions -n` stdout into the names of LIVE (non-exited)
/// sessions, preserving order.
///
/// A session is LIVE iff its line does NOT contain `"(EXITED"`. The name is the
/// substring before `" [Created"`. The `"(current)"` suffix is IGNORED (it is
/// derived from the possibly-stale `ZELLIJ_SESSION_NAME` and is unreliable).
#[must_use]
pub fn parse_live_sessions(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|line| !line.contains("(EXITED"))
        .filter_map(|line| line.split(" [Created").next())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// Pure resolver. `env_name` is `$ZELLIJ_SESSION_NAME` (`None` if unset/empty).
///
/// - if `env_name` is among `live` -> `Ok(env_name)`           (FAST PATH, no rename)
/// - else if `live.len() == 1`     -> `Ok(live[0].clone())`    (SAFE single-session recovery*)
/// - else                          -> `Err(ShimError::NoSuchSession(...))`
///
/// (*sound because: if the shim process is running, its zellij session is alive,
/// so a stale env name means a rename and the sole live session must be ours.)
///
/// NOTE: a later wave will insert cache-based recovery between the single-session
/// branch and the `Err` — keep this function pure and the branches clearly
/// separated so that is a clean edit.
pub fn resolve_from(env_name: Option<&str>, live: &[String]) -> Result<String> {
    if let Some(name) = env_name {
        if live.iter().any(|s| s == name) {
            return Ok(name.to_string());
        }
    }
    if live.len() == 1 {
        return Ok(live[0].clone());
    }
    Err(ShimError::NoSuchSession(
        env_name.unwrap_or_default().to_string(),
    ))
}

/// Run `zellij list-sessions -n` via the runner and parse it.
///
/// Errors ONLY propagate a spawn/timeout failure from the runner; empty/garbage
/// stdout yields `Ok(vec![])`.
pub fn list_live_sessions(runner: &dyn ZellijRunner) -> Result<Vec<String>> {
    let out = runner.run(&["list-sessions", "-n"])?;
    if !out.success {
        return Err(ShimError::ZellijExit {
            code: 1,
            stderr: out.stderr,
        });
    }
    Ok(parse_live_sessions(&out.stdout))
}

/// Orchestrator used by the router. Enumerate live sessions, then resolve,
/// seeding and consulting the rename-recovery [`cache`] for the multi-session
/// stale case, and finally falling back to a pane-probe that pins us to our own
/// session via `$ZELLIJ_PANE_ID` + `current_dir`. Uses [`cache::default_dir`];
/// if no cache dir resolves the cache is simply disabled.
pub fn resolve_session(runner: &dyn ZellijRunner, env_name: Option<String>) -> Result<String> {
    let pane_id = crate::env::current_pane_int();
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    resolve_session_in(
        runner,
        env_name,
        cache::default_dir(),
        pane_id,
        cwd.as_deref(),
    )
}

/// Testable orchestrator taking an explicit `cache_dir` (`None` disables the
/// cache) plus our own pane id and working directory for the probe. Flow:
///   1. enumerate live sessions; on error/empty DEGRADE to the env name
///      (never fail the no-rename path on a subprocess/parse hiccup);
///   2. FAST PATH — env name is live -> record snapshot, return it;
///   3. SINGLE-SESSION RECOVERY — one live session -> return it (unambiguous);
///   4. MULTI-SESSION + STALE — [`cache::recover_in`] (ledger hit / diff discovery);
///   5. PANE-PROBE — [`probe_session`] identifies our session by our own pane
///      even with NO seeded cache (e.g. first command after a rename, or a
///      totally unset env), then seeds the cache so later calls hit step 4;
///   6. otherwise fail fast.
pub fn resolve_session_in(
    runner: &dyn ZellijRunner,
    env_name: Option<String>,
    cache_dir: Option<std::path::PathBuf>,
    pane_id: Option<i64>,
    cwd: Option<&str>,
) -> Result<String> {
    let live = match list_live_sessions(runner) {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(env_name.unwrap_or_else(|| "zellij".to_string())),
    };
    if let Some(n) = env_name.as_deref() {
        if live.iter().any(|s| s == n) {
            if let Some(d) = &cache_dir {
                cache::record_in(d, n, &live, n);
            }
            return Ok(n.to_string());
        }
    }
    if live.len() == 1 {
        return Ok(live[0].clone());
    }
    if let (Some(n), Some(d)) = (env_name.as_deref(), &cache_dir) {
        if let Some(r) = cache::recover_in(d, n, &live) {
            return Ok(r);
        }
    }
    // PANE-PROBE: env is stale/unset and the cache could not recover, but our
    // own pane still pins us to exactly one live session. This is what makes a
    // rename recoverable with NO prior cache seed and even with an unset env.
    if let Some(pid) = pane_id {
        if let Some(resolved) = probe_session(runner, &live, pid, cwd) {
            // Seed the cache so subsequent commands short-circuit at the ledger
            // hit above instead of re-probing every session on every call.
            if let (Some(n), Some(d)) = (env_name.as_deref(), &cache_dir) {
                cache::record_in(d, n, &live, &resolved);
            }
            return Ok(resolved);
        }
    }
    Err(ShimError::NoSuchSession(
        env_name.unwrap_or_default().to_string(),
    ))
}

/// PANE-PROBE recovery: identify OUR session by matching OUR OWN pane.
///
/// The shim runs inside a zellij pane whose integer id (`$ZELLIJ_PANE_ID`,
/// `pane_id`) SURVIVES a session rename, and whose working directory equals the
/// pane's `pane_cwd`. For each live session we list its panes (across all tabs)
/// and look for a terminal (non-plugin, non-floating) pane whose `id == pane_id`
/// AND `pane_cwd == cwd`. If EXACTLY ONE live session contains such a pane, that
/// session is unambiguously ours.
///
/// Soundness: recovery fires only on a UNIQUE match. Pane ids are per-session
/// (every session has an id 0), so id alone is ambiguous — cwd equality is the
/// discriminator, and the exactly-one gate rejects the residual case where two
/// sessions share both id and cwd (fail-safe: fall through to the error). We do
/// NOT gate on `is_focused`/`pane_command`: both collide across sessions (every
/// session reports a focused pane, and shells share a command name).
///
/// `cwd == None` (unreadable `current_dir`) DISABLES the probe: id alone cannot
/// discriminate, so we decline rather than risk targeting the wrong session.
fn probe_session(
    runner: &dyn ZellijRunner,
    live: &[String],
    pane_id: i64,
    cwd: Option<&str>,
) -> Option<String> {
    let cwd = cwd?;
    let mut hit: Option<String> = None;
    for name in live {
        // A session may die mid-probe; a spawn/parse error just means "no match
        // from this session" and must not abort the whole probe.
        let Ok(panes) = Client::new(runner, name.clone()).list_panes() else {
            continue;
        };
        let ours = panes.iter().any(|p| {
            !p.is_plugin
                && !p.is_floating
                && p.id == pane_id
                && p.pane_cwd.as_deref().is_some_and(|c| same_path(c, cwd))
        });
        if ours {
            if hit.is_some() {
                return None; // ambiguous: two sessions match -> fail-safe
            }
            hit = Some(name.clone());
        }
    }
    hit
}

/// Compare two filesystem paths for identity, resolving symlinks when both
/// exist and falling back to a trailing-slash-insensitive string match (so the
/// probe still works for paths that cannot be canonicalized).
fn same_path(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.trim_end_matches('/') == b.trim_end_matches('/'),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::zellij::client::FakeRunner;

    /// Captured live from zellij 0.44.3 — note the trailing space on
    /// non-current live lines and the ANSI-free text.
    const FIXTURE: &str = "\
hyperinfer-master [Created 5days 46m 17s ago] \n\
wise-mountain [Created 14h 33m 26s ago] \n\
zellij-master [Created 5h 53m 3s ago] \n\
brave-tomato [Created 8m 3s ago] (current)\n\
quiet-capsicum [Created 5days 16h 14m 34s ago] (EXITED - attach to resurrect)\n";

    // ---- parse_live_sessions ----

    #[test]
    fn parse_live_sessions_returns_four_live_names_from_fixture() {
        assert_eq!(
            parse_live_sessions(FIXTURE),
            [
                "hyperinfer-master",
                "wise-mountain",
                "zellij-master",
                "brave-tomato"
            ]
        );
    }

    #[test]
    fn parse_live_sessions_empty_input_yields_empty_vec() {
        assert!(parse_live_sessions("").is_empty());
    }

    #[test]
    fn parse_live_sessions_lone_exited_line_yields_empty_vec() {
        let only_dead = "quiet-capsicum [Created 5days 16h ago] (EXITED - attach to resurrect)\n";
        assert!(parse_live_sessions(only_dead).is_empty());
    }

    #[test]
    fn parse_live_sessions_bare_name_with_trailing_space() {
        let line = "name [Created 1s ago] \n";
        assert_eq!(parse_live_sessions(line), ["name"]);
    }

    // ---- resolve_from ----

    #[test]
    fn resolve_from_fast_path_env_in_live_returns_env() {
        let live = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(resolve_from(Some("b"), &live).unwrap(), "b");
    }

    #[test]
    fn resolve_from_single_live_stale_env_recovers_that_one() {
        let live = vec!["only".to_string()];
        assert_eq!(resolve_from(Some("ghost"), &live).unwrap(), "only");
    }

    #[test]
    fn resolve_from_multi_live_stale_env_errors_matching_no_such_session() {
        let live = vec!["a".to_string(), "b".to_string()];
        let err = resolve_from(Some("ghost"), &live).unwrap_err();
        assert!(matches!(err, ShimError::NoSuchSession(ref s) if s == "ghost"));
        assert_eq!(err.to_string(), "can't find session: ghost");
    }

    #[test]
    fn resolve_from_env_none_single_live_recovers_that_one() {
        let live = vec!["only".to_string()];
        assert_eq!(resolve_from(None, &live).unwrap(), "only");
    }

    #[test]
    fn resolve_from_env_none_multi_live_errors() {
        let live = vec!["a".to_string(), "b".to_string()];
        let err = resolve_from(None, &live).unwrap_err();
        assert!(matches!(err, ShimError::NoSuchSession(ref s) if s.is_empty()));
    }

    #[test]
    fn resolve_from_env_in_live_is_fast_path_not_recovery() {
        // env matches the first of several live sessions — must return env, not
        // a single-session recovery branch.
        let live = vec!["env".to_string(), "other".to_string()];
        assert_eq!(resolve_from(Some("env"), &live).unwrap(), "env");
    }

    // ---- list_live_sessions ----

    #[test]
    fn list_live_sessions_parses_fixture_via_routed_fake_runner() {
        let f = FakeRunner::routed(&[("list-sessions", FIXTURE)]);
        let live = list_live_sessions(&f).unwrap();
        assert_eq!(
            live,
            [
                "hyperinfer-master",
                "wise-mountain",
                "zellij-master",
                "brave-tomato"
            ]
        );
        // Bare probe — NO --session, NO action.
        assert_eq!(f.last_call(), ["list-sessions", "-n"]);
    }

    #[test]
    fn list_live_sessions_fail_runner_propagates_error() {
        let f = FakeRunner::fail();
        assert!(list_live_sessions(&f).is_err());
        assert_eq!(f.last_call(), ["list-sessions", "-n"]);
    }

    // ---- resolve_session ----

    #[test]
    fn resolve_session_fast_path_env_in_fixture_returns_env() {
        let f = FakeRunner::routed(&[("list-sessions", FIXTURE)]);
        assert_eq!(
            resolve_session_in(&f, Some("zellij-master".to_string()), None, None, None).unwrap(),
            "zellij-master"
        );
    }

    #[test]
    fn resolve_session_stale_env_multi_live_errors() {
        let f = FakeRunner::routed(&[("list-sessions", FIXTURE)]);
        let err = resolve_session_in(&f, Some("ghost".to_string()), None, None, None).unwrap_err();
        assert!(matches!(err, ShimError::NoSuchSession(ref s) if s == "ghost"));
        assert_eq!(err.to_string(), "can't find session: ghost");
    }

    #[test]
    fn resolve_session_fail_runner_degrades_to_env() {
        let f = FakeRunner::fail();
        assert_eq!(
            resolve_session_in(&f, Some("whatever".to_string()), None, None, None).unwrap(),
            "whatever"
        );
    }

    #[test]
    fn resolve_session_empty_stdout_env_none_degrades_to_default_zellij() {
        let f = FakeRunner::ok("");
        assert_eq!(
            resolve_session_in(&f, None, None, None, None).unwrap(),
            "zellij"
        );
    }

    // ---- resolve_session_in cache integration ----

    fn tmp_cache_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "zts-session-{}-{label}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    /// Multi-session + stale env, cache seeded so exactly one session appeared
    /// -> the resolver recovers via diff discovery.
    #[test]
    fn resolve_session_in_multi_stale_recovers_via_cache() {
        let dir = tmp_cache_dir("recover");
        std::fs::create_dir_all(&dir).unwrap();
        // Seed: env "old", snapshot ["old","beta"], resolved "old".
        cache::record_in(&dir, "old", &["old".into(), "beta".into()], "old");
        // Live now: "old" is gone, "alpha" appeared (exactly one new), "beta" stayed.
        let fixture = "alpha [Created 1s ago] \nbeta [Created 2s ago] \n";
        let f = FakeRunner::routed(&[("list-sessions", fixture)]);
        assert_eq!(
            resolve_session_in(&f, Some("old".to_string()), Some(dir.clone()), None, None).unwrap(),
            "alpha"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Multi-session + stale env with NO cache -> fail fast (NoSuchSession).
    #[test]
    fn resolve_session_in_multi_stale_without_cache_errors() {
        let fixture = "alpha [Created 1s ago] \nbeta [Created 2s ago] \n";
        let f = FakeRunner::routed(&[("list-sessions", fixture)]);
        let err = resolve_session_in(&f, Some("ghost".to_string()), None, None, None).unwrap_err();
        assert!(matches!(err, ShimError::NoSuchSession(ref s) if s == "ghost"));
        assert_eq!(err.to_string(), "can't find session: ghost");
    }

    // ---- resolve_session_in pane-probe fallback ----

    fn term_pane(id: i64, cwd: &str) -> String {
        format!(
            "[{{\"id\":{id},\"is_plugin\":false,\"is_floating\":false,\"pane_x\":0,\
             \"pane_y\":0,\"pane_rows\":24,\"pane_columns\":80,\"pane_cwd\":\"{cwd}\",\"tab_id\":0}}]"
        )
    }

    fn probe_runner(alpha_panes: &str, beta_panes: &str) -> FakeRunner {
        FakeRunner::routed(&[
            (
                "list-sessions",
                "alpha [Created 1s ago] \nbeta [Created 2s ago] \n",
            ),
            ("--session alpha action list-panes", alpha_panes),
            ("--session beta action list-panes", beta_panes),
        ])
    }

    #[test]
    fn probe_unique_id_and_cwd_match_recovers_that_session() {
        let f = probe_runner(&term_pane(0, "/other"), &term_pane(0, "/work"));
        assert_eq!(
            resolve_session_in(&f, Some("ghost".to_string()), None, Some(0), Some("/work"))
                .unwrap(),
            "beta"
        );
    }

    #[test]
    fn probe_no_pane_matches_falls_through_to_error() {
        let f = probe_runner(&term_pane(0, "/other"), &term_pane(0, "/elsewhere"));
        assert!(
            resolve_session_in(&f, Some("ghost".to_string()), None, Some(0), Some("/work"))
                .is_err()
        );
    }

    #[test]
    fn probe_two_sessions_match_is_ambiguous_and_errors() {
        let f = probe_runner(&term_pane(0, "/work"), &term_pane(0, "/work"));
        assert!(
            resolve_session_in(&f, Some("ghost".to_string()), None, Some(0), Some("/work"))
                .is_err()
        );
    }

    #[test]
    fn probe_ignores_plugin_pane_with_matching_id() {
        let alpha_plugin = "[{\"id\":0,\"is_plugin\":true,\"is_floating\":false,\"pane_x\":0,\
             \"pane_y\":0,\"pane_rows\":24,\"pane_columns\":80,\"pane_cwd\":\"/work\",\"tab_id\":0}]";
        let f = probe_runner(alpha_plugin, &term_pane(0, "/work"));
        assert_eq!(
            resolve_session_in(&f, Some("ghost".to_string()), None, Some(0), Some("/work"))
                .unwrap(),
            "beta"
        );
    }

    #[test]
    fn probe_skipped_when_pane_id_unset() {
        let f = probe_runner(&term_pane(0, "/work"), &term_pane(0, "/work"));
        assert!(
            resolve_session_in(&f, Some("ghost".to_string()), None, None, Some("/work")).is_err()
        );
    }

    #[test]
    fn probe_skipped_when_cwd_unset() {
        let f = probe_runner(&term_pane(0, "/work"), &term_pane(0, "/work"));
        assert!(resolve_session_in(&f, Some("ghost".to_string()), None, Some(0), None).is_err());
    }

    #[test]
    fn probe_seeds_cache_under_stale_env_for_next_call() {
        let dir = tmp_cache_dir("probe-seed");
        std::fs::create_dir_all(&dir).unwrap();
        let f = probe_runner(&term_pane(0, "/other"), &term_pane(0, "/work"));
        assert_eq!(
            resolve_session_in(
                &f,
                Some("ghost".to_string()),
                Some(dir.clone()),
                Some(0),
                Some("/work")
            )
            .unwrap(),
            "beta"
        );
        let entry = cache::read_in(&dir, "ghost").expect("probe seeded the cache");
        assert_eq!(entry.resolved.as_deref(), Some("beta"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
