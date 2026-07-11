//! Builds the tmux format-variable context for one pane from live zellij data.
//!
//! Maps zellij's pane/tab fields onto the tmux `#{...}` variables that agent
//! tools read. Variables with no zellij equivalent (uuids) are present but
//! empty; the [`super::render`] engine simply substitutes whatever is here.

use crate::idmap;
use crate::zellij::types::{PaneInfo, TabInfo};
use std::collections::HashMap;

pub struct FormatContext {
    vars: HashMap<&'static str, String>,
}

impl FormatContext {
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    #[cfg(test)]
    pub fn test_ctx(pairs: &[(&'static str, &str)]) -> Self {
        Self {
            vars: pairs.iter().map(|(k, v)| (*k, (*v).to_string())).collect(),
        }
    }
}

#[must_use]
pub fn build(
    pane: &PaneInfo,
    tab: &TabInfo,
    all_panes: &[PaneInfo],
    session: &str,
) -> FormatContext {
    let mut tab_pane_ids: Vec<i64> = all_panes
        .iter()
        .filter(|p| !p.is_plugin && p.tab_id == pane.tab_id)
        .map(|p| p.id)
        .collect();
    tab_pane_ids.sort_unstable();
    let pane_index = tab_pane_ids.iter().position(|&x| x == pane.id).unwrap_or(0);
    let window_panes = tab_pane_ids.len();

    let mut vars: HashMap<&'static str, String> = HashMap::new();
    insert_session(&mut vars, session);
    insert_window(&mut vars, tab, window_panes);
    insert_pane(&mut vars, pane, pane_index);
    FormatContext { vars }
}

#[must_use]
pub fn build_window(tab: &TabInfo, all_panes: &[PaneInfo], session: &str) -> FormatContext {
    let window_panes = all_panes
        .iter()
        .filter(|p| !p.is_plugin && p.tab_id == tab.tab_id)
        .count();
    let mut vars: HashMap<&'static str, String> = HashMap::new();
    insert_session(&mut vars, session);
    insert_window(&mut vars, tab, window_panes);
    FormatContext { vars }
}

#[must_use]
pub fn build_session(session: &str) -> FormatContext {
    let mut vars: HashMap<&'static str, String> = HashMap::new();
    insert_session(&mut vars, session);
    FormatContext { vars }
}

fn insert_session(vars: &mut HashMap<&'static str, String>, session: &str) {
    vars.insert("session_id", "$0".to_string());
    vars.insert("session_name", session.to_string());
    vars.insert("session_attached", "1".to_string());
    vars.insert("socket_path", format!("/tmp/zellij-tmux-shim/{session}"));
}

fn insert_window(vars: &mut HashMap<&'static str, String>, tab: &TabInfo, window_panes: usize) {
    vars.insert("window_id", idmap::tmux_window_id(tab.tab_id));
    vars.insert("window_uuid", String::new());
    vars.insert("window_index", tab.position.to_string());
    vars.insert("window_name", tab.name.clone());
    vars.insert("window_width", tab.viewport_columns.to_string());
    vars.insert("window_height", tab.viewport_rows.to_string());
    vars.insert("window_active", bit(tab.active));
    vars.insert(
        "window_flags",
        if tab.active { "*" } else { "" }.to_string(),
    );
    vars.insert("window_panes", window_panes.to_string());
}

fn insert_pane(vars: &mut HashMap<&'static str, String>, pane: &PaneInfo, pane_index: usize) {
    let (cursor_x, cursor_y) = pane
        .cursor_coordinates_in_pane
        .map_or((0, 0), |c| (c[0], c[1]));
    vars.insert("pane_id", idmap::tmux_pane_id(pane.id));
    vars.insert("pane_uuid", String::new());
    vars.insert("pane_width", pane.pane_columns.to_string());
    vars.insert("pane_height", pane.pane_rows.to_string());
    vars.insert("pane_left", pane.pane_x.to_string());
    vars.insert("pane_top", pane.pane_y.to_string());
    vars.insert("pane_index", pane_index.to_string());
    vars.insert("pane_active", bit(pane.is_focused));
    vars.insert("pane_title", pane.title.clone());
    vars.insert(
        "pane_current_path",
        pane.pane_cwd.clone().unwrap_or_default(),
    );
    vars.insert(
        "pane_current_command",
        pane.pane_command.clone().unwrap_or_default(),
    );
    vars.insert("cursor_x", cursor_x.to_string());
    vars.insert("cursor_y", cursor_y.to_string());
}

fn bit(b: bool) -> String {
    if b { "1" } else { "0" }.to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const PANES: &str = r#"[
      {"id":0,"is_plugin":false,"is_focused":true,"is_floating":false,"title":"editor",
       "pane_x":0,"pane_y":0,"pane_rows":24,"pane_columns":120,"cursor_coordinates_in_pane":[4,9],
       "pane_command":"vim","pane_cwd":"/proj","tab_id":3,"tab_position":1,"tab_name":"work"},
      {"id":1,"is_plugin":false,"is_focused":false,"is_floating":false,"title":"shell",
       "pane_x":0,"pane_y":24,"pane_rows":24,"pane_columns":120,"cursor_coordinates_in_pane":null,
       "pane_command":"zsh","pane_cwd":"/proj","tab_id":3,"tab_position":1,"tab_name":"work"}
    ]"#;
    const TAB: &str = r#"{"position":1,"name":"work","active":true,"viewport_rows":48,
       "viewport_columns":120,"tab_id":3}"#;

    fn ctx_for(idx: usize) -> FormatContext {
        let panes: Vec<PaneInfo> = serde_json::from_str(PANES).unwrap();
        let tab: TabInfo = serde_json::from_str(TAB).unwrap();
        build(&panes[idx], &tab, &panes, "mysess")
    }

    #[test]
    fn maps_pane_geometry_and_ids() {
        let c = ctx_for(0);
        assert_eq!(c.lookup("pane_id"), Some("%0"));
        assert_eq!(c.lookup("pane_width"), Some("120"));
        assert_eq!(c.lookup("pane_height"), Some("24"));
        assert_eq!(c.lookup("pane_top"), Some("0"));
        assert_eq!(c.lookup("pane_active"), Some("1"));
        assert_eq!(c.lookup("pane_current_command"), Some("vim"));
        assert_eq!(c.lookup("cursor_x"), Some("4"));
    }

    #[test]
    fn maps_window_and_session() {
        let c = ctx_for(0);
        assert_eq!(c.lookup("window_id"), Some("@3"));
        assert_eq!(c.lookup("window_width"), Some("120"));
        assert_eq!(c.lookup("window_height"), Some("48"));
        assert_eq!(c.lookup("window_index"), Some("1"));
        assert_eq!(c.lookup("window_panes"), Some("2"));
        assert_eq!(c.lookup("window_flags"), Some("*"));
        assert_eq!(c.lookup("session_name"), Some("mysess"));
    }

    #[test]
    fn nonfocused_pane_index_and_active() {
        let c = ctx_for(1);
        assert_eq!(c.lookup("pane_index"), Some("1"));
        assert_eq!(c.lookup("pane_active"), Some("0"));
        assert_eq!(c.lookup("cursor_x"), Some("0"));
    }
}
