//! Bidirectional mapping between tmux and zellij identifiers.
//!
//! - pane: tmux `%N` <-> zellij `terminal_N` (the integer `N` is shared).
//! - window: tmux `@N` <-> zellij `tab_id` (integer).
//! - session: tmux `X` <-> zellij tab-name prefix `X:` (a naming namespace,
//!   because one attached zellij session holds every pane, so tmux "sessions"
//!   are represented as groups of tabs whose names start with `X:`).

use crate::error::{Result, ShimError};

#[must_use]
pub fn tmux_pane_id(zellij_pane: i64) -> String {
    format!("%{zellij_pane}")
}

pub fn zellij_pane_target(target: &str) -> Result<String> {
    parse_pane_int(target)
        .map(|n| format!("terminal_{n}"))
        .ok_or_else(|| ShimError::BadArgs(format!("invalid pane target: {target}")))
}

#[must_use]
pub fn pane_int_from_env(value: &str) -> Option<i64> {
    parse_pane_int(value)
}

fn parse_pane_int(target: &str) -> Option<i64> {
    let t = target.trim();
    let digits = t
        .strip_prefix('%')
        .or_else(|| t.strip_prefix("terminal_"))
        .unwrap_or(t);
    digits.parse::<i64>().ok()
}

#[must_use]
pub fn tmux_window_id(zellij_tab: i64) -> String {
    format!("@{zellij_tab}")
}

pub fn zellij_tab_from_window(target: &str) -> Result<i64> {
    let t = target.trim();
    let digits = t.strip_prefix('@').unwrap_or(t);
    digits
        .parse::<i64>()
        .map_err(|_| ShimError::BadArgs(format!("invalid window target: {target}")))
}

#[must_use]
pub fn session_tab_prefix(session: &str) -> String {
    format!("{session}:")
}

#[must_use]
pub fn compose_session_tab_name(session: &str, leaf: &str) -> String {
    format!("{session}:{leaf}")
}

#[must_use]
pub fn is_session_tab(session: &str, tab_name: &str) -> bool {
    tab_name.starts_with(&session_tab_prefix(session))
}

#[must_use]
pub fn session_of_tab_name(tab_name: &str) -> Option<&str> {
    tab_name.split_once(':').map(|(s, _)| s)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn pane_bijection_roundtrip() {
        assert_eq!(tmux_pane_id(7), "%7");
        assert_eq!(zellij_pane_target("%7").unwrap(), "terminal_7");
    }

    #[test]
    fn pane_target_accepts_percent_bare_and_terminal_forms() {
        assert_eq!(zellij_pane_target("7").unwrap(), "terminal_7");
        assert_eq!(zellij_pane_target("terminal_7").unwrap(), "terminal_7");
        assert_eq!(zellij_pane_target("  %7 ").unwrap(), "terminal_7");
        assert!(zellij_pane_target("nope").is_err());
    }

    #[test]
    fn pane_int_from_env_handles_bare_and_prefixed() {
        assert_eq!(pane_int_from_env("0"), Some(0));
        assert_eq!(pane_int_from_env("terminal_3"), Some(3));
        assert_eq!(pane_int_from_env(""), None);
    }

    #[test]
    fn window_bijection() {
        assert_eq!(tmux_window_id(2), "@2");
        assert_eq!(zellij_tab_from_window("@2").unwrap(), 2);
        assert_eq!(zellij_tab_from_window("2").unwrap(), 2);
        assert!(zellij_tab_from_window("@x").is_err());
    }

    #[test]
    fn session_namespace_rules() {
        assert_eq!(session_tab_prefix("omo-agents-1"), "omo-agents-1:");
        assert_eq!(
            compose_session_tab_name("omo-agents-1", "2"),
            "omo-agents-1:2"
        );
        assert!(is_session_tab("omo-agents-1", "omo-agents-1:2"));
        assert!(!is_session_tab("omo-agents-1", "other:2"));
        assert_eq!(session_of_tab_name("omo-agents-1:2"), Some("omo-agents-1"));
        assert_eq!(session_of_tab_name("plain"), None);
    }
}
