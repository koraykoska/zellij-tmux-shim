//! Input handlers: send-keys (translating tmux key names to bytes) and
//! select-pane (focus, or rename without stealing focus when `-T` is given —
//! agents use `-T` to label a just-created detached pane).

use super::{Ctx, Output};
use crate::cli::args::ParsedInvocation;
use crate::error::{Result, ShimError};
use crate::idmap;
use crate::zellij::client::Client;

pub fn handle(inv: &ParsedInvocation, client: &Client, _ctx: &Ctx) -> Result<Output> {
    match inv.subcommand.as_str() {
        "send-keys" => send_keys(inv, client),
        "select-pane" => select_pane(inv, client),
        other => Err(ShimError::BadArgs(format!(
            "keys: unhandled command {other}"
        ))),
    }
}

fn send_keys(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let target = inv
        .value('t')
        .ok_or_else(|| ShimError::BadArgs("send-keys requires -t".into()))?;
    let pane = idmap::zellij_pane_target(target)?;
    let literal = inv.has('l');
    for key in inv.operands() {
        match if literal { None } else { key_to_byte(key) } {
            Some(byte) => client.write_byte(&pane, byte)?,
            None => client.write_chars(&pane, key)?,
        }
    }
    Ok(Output::ok())
}

fn select_pane(inv: &ParsedInvocation, client: &Client) -> Result<Output> {
    let target = inv
        .value('t')
        .ok_or_else(|| ShimError::BadArgs("select-pane requires -t".into()))?;
    let pane = idmap::zellij_pane_target(target)?;
    match inv.value('T') {
        Some(title) => client.rename_pane(&pane, title)?,
        None => client.focus_pane(&pane)?,
    }
    Ok(Output::ok())
}

fn key_to_byte(name: &str) -> Option<u8> {
    match name {
        "Enter" | "C-m" | "C-M" => Some(b'\r'),
        "Tab" | "C-i" | "C-I" => Some(b'\t'),
        "Escape" | "Esc" => Some(27),
        "Space" => Some(b' '),
        "BSpace" => Some(127),
        _ => ctrl_letter(name),
    }
}

fn ctrl_letter(name: &str) -> Option<u8> {
    let rest = name
        .strip_prefix("C-")
        .or_else(|| name.strip_prefix("Ctrl-"))?;
    let mut chars = rest.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let lower = c.to_ascii_lowercase();
    lower.is_ascii_lowercase().then(|| lower as u8 - b'a' + 1)
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
    fn send_keys_writes_text_then_enter() {
        let f = FakeRunner::ok("");
        handle(
            &inv(&["send-keys", "-t", "%1", "echo hi", "Enter"]),
            &Client::new(&f, "s".to_string()),
            &Ctx::test("s"),
        )
        .unwrap();
        let calls = f.all_actions();
        assert_eq!(
            calls[0],
            [
                "action",
                "write-chars",
                "--pane-id",
                "terminal_1",
                "echo hi"
            ]
        );
        assert_eq!(
            calls[1],
            ["action", "write", "--pane-id", "terminal_1", "13"]
        );
    }

    #[test]
    fn send_keys_ctrl_c_writes_byte_3() {
        let f = FakeRunner::ok("");
        handle(
            &inv(&["send-keys", "-t", "%1", "C-c"]),
            &Client::new(&f, "s".to_string()),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(
            f.last_action(),
            ["action", "write", "--pane-id", "terminal_1", "3"]
        );
    }

    #[test]
    fn select_pane_title_renames_and_bare_focuses() {
        let f = FakeRunner::ok("");
        handle(
            &inv(&["select-pane", "-t", "%2", "-T", "omo-sub"]),
            &Client::new(&f, "s".to_string()),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(
            f.last_action(),
            [
                "action",
                "rename-pane",
                "--pane-id",
                "terminal_2",
                "omo-sub"
            ]
        );

        let g = FakeRunner::ok("");
        handle(
            &inv(&["select-pane", "-t", "%2"]),
            &Client::new(&g, "s".to_string()),
            &Ctx::test("s"),
        )
        .unwrap();
        assert_eq!(g.last_action(), ["action", "focus-pane-id", "terminal_2"]);
    }

    #[test]
    fn key_translation_table() {
        assert_eq!(key_to_byte("Enter"), Some(13));
        assert_eq!(key_to_byte("C-c"), Some(3));
        assert_eq!(key_to_byte("C-a"), Some(1));
        assert_eq!(key_to_byte("Tab"), Some(9));
        assert_eq!(key_to_byte("literal-text"), None);
    }
}
