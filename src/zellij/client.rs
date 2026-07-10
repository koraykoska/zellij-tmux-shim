//! The subprocess bridge to the real `zellij` binary — the ONLY module that
//! spawns it.
//!
//! [`ZellijRunner`] is the low-level "run these args, get output" seam, with a
//! real implementation (subprocess + timeout + optional debug log) and a test
//! fake. [`Client`] layers the typed `zellij action ...` primitives the command
//! handlers use, so handlers stay unit-testable without a live session.

use crate::error::{Result, ShimError};
use crate::zellij::types::{PaneInfo, TabInfo};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

pub trait ZellijRunner {
    /// Run `zellij <args...>` and capture its output.
    ///
    /// # Errors
    /// [`ShimError::ZellijSpawn`] / [`ShimError::ZellijTimeout`] on failure.
    fn run(&self, args: &[&str]) -> Result<RunOutput>;
}

pub struct RealRunner {
    timeout: Duration,
}

impl RealRunner {
    #[must_use]
    pub fn new() -> Self {
        let secs = std::env::var("TMUX_SHIM_TIMEOUT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(5);
        Self {
            timeout: Duration::from_secs(secs),
        }
    }
}

impl Default for RealRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ZellijRunner for RealRunner {
    fn run(&self, args: &[&str]) -> Result<RunOutput> {
        let mut child = Command::new("zellij")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ShimError::ZellijSpawn)?;
        let so = child.stdout.take();
        let se = child.stderr.take();
        let out_h = std::thread::spawn(move || read_all(so));
        let err_h = std::thread::spawn(move || read_all(se));

        let start = Instant::now();
        let status_res = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Ok(st),
                Ok(None) => {
                    if start.elapsed() >= self.timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Err(ShimError::ZellijTimeout);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => break Err(ShimError::ZellijSpawn(e)),
            }
        };
        let stdout = out_h.join().unwrap_or_default();
        let stderr = err_h.join().unwrap_or_default();
        let status = status_res?;

        let output = RunOutput {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            success: status.success(),
        };
        debug_log(args, &output);
        Ok(output)
    }
}

fn read_all(source: Option<impl Read>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(mut r) = source {
        let _ = r.read_to_end(&mut buf);
    }
    buf
}

fn debug_log(args: &[&str], out: &RunOutput) {
    if std::env::var_os("TMUX_SHIM_DEBUG").is_none() {
        return;
    }
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = std::path::Path::new(&home).join(".cache/zellij-tmux-shim");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("debug.log"))
    {
        use std::io::Write;
        let _ = writeln!(
            f,
            "zellij {} -> ok={} out={:?} err={:?}",
            args.join(" "),
            out.success,
            out.stdout.trim(),
            out.stderr.trim()
        );
    }
}

pub struct Client<'r> {
    runner: &'r dyn ZellijRunner,
}

impl<'r> Client<'r> {
    #[must_use]
    pub fn new(runner: &'r dyn ZellijRunner) -> Self {
        Self { runner }
    }

    pub fn list_panes(&self) -> Result<Vec<PaneInfo>> {
        let out = self
            .runner
            .run(&["action", "list-panes", "--all", "--json"])?;
        serde_json::from_str(&out.stdout).map_err(Into::into)
    }

    pub fn list_tabs(&self) -> Result<Vec<TabInfo>> {
        let out = self
            .runner
            .run(&["action", "list-tabs", "--all", "--json"])?;
        serde_json::from_str(&out.stdout).map_err(Into::into)
    }

    pub fn focus_pane(&self, pane_id: &str) -> Result<()> {
        self.action(&["focus-pane-id", pane_id])
    }

    pub fn close_pane(&self, pane_id: &str) -> Result<()> {
        self.action(&["close-pane", "--pane-id", pane_id])
    }

    pub fn rename_pane(&self, pane_id: &str, name: &str) -> Result<()> {
        self.action(&["rename-pane", "--pane-id", pane_id, name])
    }

    pub fn write_chars(&self, pane_id: &str, text: &str) -> Result<()> {
        self.action(&["write-chars", "--pane-id", pane_id, text])
    }

    pub fn write_byte(&self, pane_id: &str, byte: u8) -> Result<()> {
        self.action(&["write", "--pane-id", pane_id, &byte.to_string()])
    }

    pub fn send_keys(&self, pane_id: &str, keys: &[&str]) -> Result<()> {
        let mut args = vec!["send-keys", "--pane-id", pane_id];
        args.extend_from_slice(keys);
        self.action(&args)
    }

    pub fn resize(&self, pane_id: &str, resize_and_dir: &[&str]) -> Result<()> {
        let mut args = vec!["resize", "--pane-id", pane_id];
        args.extend_from_slice(resize_and_dir);
        self.action(&args)
    }

    pub fn dump_screen_full(&self, pane_id: &str) -> Result<String> {
        let out = self
            .runner
            .run(&["action", "dump-screen", "--full", "--pane-id", pane_id])?;
        Ok(out.stdout)
    }

    pub fn new_pane(&self, extra: &[&str]) -> Result<i64> {
        let mut args = vec!["action", "new-pane"];
        args.extend_from_slice(extra);
        let out = self.runner.run(&args)?;
        crate::idmap::pane_int_from_env(out.stdout.trim()).ok_or_else(|| {
            ShimError::BadArgs(format!(
                "unexpected new-pane output: {:?}",
                out.stdout.trim()
            ))
        })
    }

    pub fn new_tab(&self, extra: &[&str]) -> Result<i64> {
        let mut args = vec!["action", "new-tab"];
        args.extend_from_slice(extra);
        let out = self.runner.run(&args)?;
        out.stdout.trim().parse::<i64>().map_err(|_| {
            ShimError::BadArgs(format!(
                "unexpected new-tab output: {:?}",
                out.stdout.trim()
            ))
        })
    }

    pub fn go_to_tab(&self, position: i64) -> Result<()> {
        self.action(&["go-to-tab", &position.to_string()])
    }

    pub fn go_to_tab_by_id(&self, tab_id: i64) -> Result<()> {
        self.action(&["go-to-tab-by-id", &tab_id.to_string()])
    }

    pub fn rename_tab_by_id(&self, tab_id: i64, name: &str) -> Result<()> {
        self.action(&["rename-tab-by-id", &tab_id.to_string(), name])
    }

    pub fn close_tab_by_id(&self, tab_id: i64) -> Result<()> {
        self.action(&["close-tab-by-id", &tab_id.to_string()])
    }

    fn action(&self, tail: &[&str]) -> Result<()> {
        let mut args = vec!["action"];
        args.extend_from_slice(tail);
        let out = self.runner.run(&args)?;
        if out.success {
            Ok(())
        } else {
            Err(ShimError::ZellijExit {
                code: 1,
                stderr: out.stderr,
            })
        }
    }
}

#[cfg(test)]
pub struct FakeRunner {
    routes: Vec<(String, String)>,
    success: bool,
    calls: std::cell::RefCell<Vec<Vec<String>>>,
}

#[cfg(test)]
impl FakeRunner {
    pub fn ok(stdout: &str) -> Self {
        Self {
            routes: vec![(String::new(), stdout.to_string())],
            success: true,
            calls: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn routed(routes: &[(&str, &str)]) -> Self {
        Self {
            routes: routes
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            success: true,
            calls: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn fail() -> Self {
        Self {
            routes: vec![(String::new(), String::new())],
            success: false,
            calls: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn last_call(&self) -> Vec<String> {
        self.calls.borrow().last().cloned().unwrap_or_default()
    }

    pub fn all_calls(&self) -> Vec<Vec<String>> {
        self.calls.borrow().clone()
    }

    fn resolve(&self, args: &[&str]) -> String {
        let joined = args.join(" ");
        self.routes
            .iter()
            .find(|(key, _)| joined.contains(key.as_str()))
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl ZellijRunner for FakeRunner {
    fn run(&self, args: &[&str]) -> Result<RunOutput> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| (*s).to_string()).collect());
        Ok(RunOutput {
            stdout: self.resolve(args),
            stderr: String::from("boom"),
            success: self.success,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn list_panes_uses_canonical_query_and_parses() {
        let fake = FakeRunner::ok(
            r#"[{"id":0,"is_plugin":false,"is_focused":true,"pane_x":0,"pane_y":0,
                "pane_rows":24,"pane_columns":80,"tab_id":0,"tab_position":0}]"#,
        );
        let client = Client::new(&fake);
        let panes = client.list_panes().unwrap();
        assert_eq!(panes.len(), 1);
        assert_eq!(
            fake.last_call(),
            ["action", "list-panes", "--all", "--json"]
        );
    }

    #[test]
    fn new_pane_parses_terminal_id() {
        let fake = FakeRunner::ok("terminal_7\n");
        let client = Client::new(&fake);
        assert_eq!(client.new_pane(&["--direction", "right"]).unwrap(), 7);
        assert_eq!(
            fake.last_call(),
            ["action", "new-pane", "--direction", "right"]
        );
    }

    #[test]
    fn new_tab_parses_bare_int() {
        let fake = FakeRunner::ok("3\n");
        let client = Client::new(&fake);
        assert_eq!(client.new_tab(&["--name", "omo:1"]).unwrap(), 3);
    }

    #[test]
    fn close_pane_targets_by_pane_id() {
        let fake = FakeRunner::ok("");
        let client = Client::new(&fake);
        client.close_pane("terminal_2").unwrap();
        assert_eq!(
            fake.last_call(),
            ["action", "close-pane", "--pane-id", "terminal_2"]
        );
    }

    #[test]
    fn write_byte_renders_decimal() {
        let fake = FakeRunner::ok("");
        let client = Client::new(&fake);
        client.write_byte("terminal_1", 3).unwrap();
        assert_eq!(
            fake.last_call(),
            ["action", "write", "--pane-id", "terminal_1", "3"]
        );
    }

    #[test]
    fn failed_action_is_error() {
        let fake = FakeRunner::fail();
        let client = Client::new(&fake);
        assert!(client.close_pane("terminal_9").is_err());
    }
}
