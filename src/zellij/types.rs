//! Deserialized models of `zellij action list-panes --json` and
//! `list-tabs --json`, plus the pure selectors the handlers use to resolve a
//! tmux target to a zellij pane/tab.
//!
//! Only the fields the shim consumes are modeled; serde ignores the rest.
//! `id` is per-layer in zellij (a plugin pane and a terminal pane can both be
//! `id: 0`), so every pane selector filters `is_plugin == false` — tmux only
//! ever sees terminal panes.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PaneInfo {
    pub id: i64,
    #[serde(default)]
    pub is_plugin: bool,
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default)]
    pub is_floating: bool,
    #[serde(default)]
    pub is_suppressed: bool,
    #[serde(default)]
    pub title: String,
    pub pane_x: i64,
    pub pane_y: i64,
    pub pane_rows: i64,
    pub pane_columns: i64,
    #[serde(default)]
    pub cursor_coordinates_in_pane: Option<[i64; 2]>,
    #[serde(default)]
    pub pane_command: Option<String>,
    #[serde(default)]
    pub pane_cwd: Option<String>,
    pub tab_id: i64,
    #[serde(default)]
    pub tab_position: i64,
    #[serde(default)]
    pub tab_name: String,
    #[serde(default)]
    pub exited: bool,
    #[serde(default)]
    pub exit_status: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TabInfo {
    pub position: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub active: bool,
    pub viewport_rows: i64,
    pub viewport_columns: i64,
    pub tab_id: i64,
}

#[must_use]
pub fn active_terminal(panes: &[PaneInfo]) -> Option<&PaneInfo> {
    panes
        .iter()
        .find(|p| p.is_focused && !p.is_plugin && !p.is_floating)
}

#[must_use]
pub fn pane_by_terminal_id(panes: &[PaneInfo], id: i64) -> Option<&PaneInfo> {
    panes.iter().find(|p| !p.is_plugin && p.id == id)
}

#[must_use]
pub fn terminal_panes(panes: &[PaneInfo]) -> Vec<&PaneInfo> {
    panes.iter().filter(|p| !p.is_plugin).collect()
}

#[must_use]
pub fn active_tab(tabs: &[TabInfo]) -> Option<&TabInfo> {
    tabs.iter().find(|t| t.active)
}

#[must_use]
pub fn tab_by_id(tabs: &[TabInfo], id: i64) -> Option<&TabInfo> {
    tabs.iter().find(|t| t.tab_id == id)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // A trimmed but faithful capture of `zellij action list-panes --json` from a
    // live 0.44.3 session: a focused floating plugin, a suppressed plugin, and
    // three terminal panes (ids reused across the plugin/terminal layers).
    const PANES_JSON: &str = r#"[
      {"id":1,"is_plugin":true,"is_focused":true,"is_floating":true,"is_suppressed":false,
       "title":"About Zellij","pane_x":0,"pane_y":2,"pane_rows":20,"pane_columns":80,
       "cursor_coordinates_in_pane":null,"tab_id":0,"tab_position":0,"tab_name":"Tab #1"},
      {"id":0,"is_plugin":true,"is_focused":false,"is_floating":false,"is_suppressed":true,
       "title":"zellij:link","pane_x":20,"pane_y":6,"pane_rows":12,"pane_columns":40,
       "cursor_coordinates_in_pane":null,"tab_id":0,"tab_position":0,"tab_name":"Tab #1"},
      {"id":0,"is_plugin":false,"is_focused":true,"is_floating":false,"is_suppressed":false,
       "title":"prober","pane_x":0,"pane_y":0,"pane_rows":8,"pane_columns":80,
       "cursor_coordinates_in_pane":[1,1],"pane_command":"bash","pane_cwd":"/tmp",
       "tab_id":0,"tab_position":0,"tab_name":"Tab #1"},
      {"id":1,"is_plugin":false,"is_focused":false,"is_floating":false,"is_suppressed":false,
       "title":"/tmp","pane_x":0,"pane_y":8,"pane_rows":8,"pane_columns":80,
       "cursor_coordinates_in_pane":[3,3],"pane_command":"/bin/zsh","pane_cwd":"/tmp",
       "tab_id":0,"tab_position":0,"tab_name":"Tab #1"},
      {"id":2,"is_plugin":false,"is_focused":false,"is_floating":false,"is_suppressed":false,
       "title":"/tmp","pane_x":0,"pane_y":16,"pane_rows":8,"pane_columns":80,
       "cursor_coordinates_in_pane":[3,3],"pane_command":"/bin/zsh","pane_cwd":"/tmp",
       "tab_id":0,"tab_position":0,"tab_name":"Tab #1"}
    ]"#;

    const TABS_JSON: &str = r#"[
      {"position":0,"name":"Tab #1","active":true,"viewport_rows":24,"viewport_columns":80,"tab_id":0}
    ]"#;

    fn panes() -> Vec<PaneInfo> {
        serde_json::from_str(PANES_JSON).unwrap()
    }

    #[test]
    fn terminal_panes_excludes_plugins() {
        let p = panes();
        let terms = terminal_panes(&p);
        assert_eq!(terms.len(), 3);
        assert!(terms.iter().all(|t| !t.is_plugin));
    }

    #[test]
    fn active_terminal_is_focused_nonplugin_nonfloating() {
        let p = panes();
        let a = active_terminal(&p).unwrap();
        assert!(!a.is_plugin && !a.is_floating && a.is_focused);
        assert_eq!(a.id, 0);
        assert_eq!(a.pane_command.as_deref(), Some("bash"));
    }

    #[test]
    fn pane_by_terminal_id_ignores_plugin_layer() {
        let p = panes();
        // terminal_0 and plugin_0 both exist; the terminal one must win.
        let t0 = pane_by_terminal_id(&p, 0).unwrap();
        assert!(!t0.is_plugin);
        assert_eq!(t0.title, "prober");
        // geometry of a non-focused terminal
        let t2 = pane_by_terminal_id(&p, 2).unwrap();
        assert_eq!(
            (t2.pane_columns, t2.pane_rows, t2.pane_x, t2.pane_y),
            (80, 8, 0, 16)
        );
    }

    #[test]
    fn tabs_deserialize_and_select_active() {
        let tabs: Vec<TabInfo> = serde_json::from_str(TABS_JSON).unwrap();
        let a = active_tab(&tabs).unwrap();
        assert_eq!((a.viewport_columns, a.viewport_rows), (80, 24));
        assert_eq!(a.position, 0);
        assert!(tab_by_id(&tabs, 0).is_some());
    }

    #[test]
    fn parses_real_captured_fixtures() {
        let panes: Vec<PaneInfo> =
            serde_json::from_str(include_str!("../../tests/fixtures/list_panes.json")).unwrap();
        assert!(
            panes.iter().any(|p| p.is_plugin),
            "fixture has plugin panes"
        );
        assert!(
            !terminal_panes(&panes).is_empty(),
            "fixture has terminal panes"
        );
        assert!(terminal_panes(&panes).iter().all(|p| !p.is_plugin));

        let tabs: Vec<TabInfo> =
            serde_json::from_str(include_str!("../../tests/fixtures/list_tabs.json")).unwrap();
        assert!(!tabs.is_empty());
    }
}
