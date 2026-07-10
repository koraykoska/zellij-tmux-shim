//! Hand-rolled tmux argument parser.
//!
//! tmux flags are case-sensitive and their arity is subcommand-dependent — for
//! example `-p` is a boolean "print" flag for `display-message` but takes a
//! percentage for `split-window`. A generic getopt library cannot express this,
//! so the parser carries a per-subcommand set of value-taking flags and does
//! clustered short-flag parsing (`-dP`, `-pt sess`, `-F#{x}`) against it.

use crate::error::{Result, ShimError};

/// A parsed tmux invocation: canonical subcommand, its flags, and operands.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedInvocation {
    pub subcommand: String,
    flags: Vec<(char, Option<String>)>,
    operands: Vec<String>,
}

impl ParsedInvocation {
    #[must_use]
    pub fn has(&self, flag: char) -> bool {
        self.flags.iter().any(|(c, _)| *c == flag)
    }

    #[must_use]
    pub fn value(&self, flag: char) -> Option<&str> {
        self.flags
            .iter()
            .find(|(c, _)| *c == flag)
            .and_then(|(_, v)| v.as_deref())
    }

    #[must_use]
    pub fn values_of(&self, flag: char) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(c, _)| *c == flag)
            .filter_map(|(_, v)| v.as_deref())
            .collect()
    }

    #[must_use]
    pub fn operands(&self) -> &[String] {
        &self.operands
    }
}

/// Parse tmux arguments (the slice *after* the program name; `args[0]` is the
/// subcommand).
///
/// # Errors
/// [`ShimError::BadArgs`] if there is no subcommand or a value flag is missing
/// its value.
pub fn parse(args: &[String]) -> Result<ParsedInvocation> {
    let raw = args
        .first()
        .ok_or_else(|| ShimError::BadArgs("missing tmux command".into()))?;
    let subcommand = canonical_subcommand(raw);
    let value_flags = value_flags(&subcommand);

    let mut flags: Vec<(char, Option<String>)> = Vec::new();
    let mut operands: Vec<String> = Vec::new();
    let mut operands_only = false;

    let mut i = 1;
    while i < args.len() {
        let tok = &args[i];
        if operands_only || tok == "-" || !tok.starts_with('-') {
            operands.push(tok.clone());
            i += 1;
            continue;
        }
        if tok == "--" {
            operands_only = true;
            i += 1;
            continue;
        }
        let body: Vec<char> = tok.chars().skip(1).collect();
        let mut j = 0;
        while j < body.len() {
            let c = body[j];
            if value_flags.contains(&c) {
                // A value flag consumes the rest of the cluster, or the next token.
                let attached: String = body[j + 1..].iter().collect();
                let val = if attached.is_empty() {
                    i += 1;
                    args.get(i).cloned().ok_or_else(|| {
                        ShimError::BadArgs(format!("option -{c} requires a value"))
                    })?
                } else {
                    attached
                };
                flags.push((c, Some(val)));
                break;
            }
            flags.push((c, None));
            j += 1;
        }
        i += 1;
    }

    Ok(ParsedInvocation {
        subcommand,
        flags,
        operands,
    })
}

fn canonical_subcommand(s: &str) -> String {
    let canon = match s {
        "new" => "new-session",
        "neww" => "new-window",
        "splitw" => "split-window",
        "selectw" => "select-window",
        "selectp" => "select-pane",
        "killw" => "kill-window",
        "killp" => "kill-pane",
        "send" => "send-keys",
        "capturep" => "capture-pane",
        "display" | "displayp" => "display-message",
        "lsw" => "list-windows",
        "lsp" => "list-panes",
        "ls" => "list-sessions",
        "renamew" => "rename-window",
        "resizep" => "resize-pane",
        "has" => "has-session",
        "set" => "set-option",
        "setw" => "set-window-option",
        other => other,
    };
    canon.to_string()
}

fn value_flags(sub: &str) -> &'static [char] {
    match sub {
        "split-window" => &['t', 'l', 'c', 'F', 'e', 'p'],
        "new-session" => &['s', 't', 'x', 'y', 'F', 'e', 'c', 'n'],
        "new-window" => &['t', 'n', 'F', 'e', 'c'],
        "display-message" => &['t', 'F'],
        "send-keys" => &['t', 'N'],
        "list-panes" | "list-windows" => &['t', 'F', 'f', 'O'],
        "list-sessions" => &['F', 'f'],
        "select-pane" => &['t', 'T'],
        "resize-pane" => &['t', 'x', 'y'],
        "respawn-pane" => &['t', 'c', 'e'],
        "capture-pane" => &['t', 'S', 'E', 'b'],
        "has-session" | "kill-pane" | "kill-window" | "kill-session" | "select-window"
        | "select-layout" | "rename-window" | "set-option" | "set-window-option" => &['t'],
        _ => &[],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn parse_v(a: &[&str]) -> ParsedInvocation {
        let owned: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
        parse(&owned).unwrap()
    }

    #[test]
    fn clustered_booleans() {
        let p = parse_v(&["split-window", "-dP"]);
        assert_eq!(p.subcommand, "split-window");
        assert!(p.has('d') && p.has('P'));
    }

    #[test]
    fn split_window_flags_and_command() {
        let p = parse_v(&[
            "split-window",
            "-h",
            "-c",
            "/tmp",
            "-F",
            "#{pane_id}",
            "echo",
            "hi",
        ]);
        assert!(p.has('h'));
        assert_eq!(p.value('c'), Some("/tmp"));
        assert_eq!(p.value('F'), Some("#{pane_id}"));
        assert_eq!(p.operands(), ["echo".to_string(), "hi".to_string()]);
    }

    #[test]
    fn display_positional_format_after_flags() {
        let p = parse_v(&["display", "-p", "-t", "%3", "#{pane_width},#{window_width}"]);
        assert_eq!(p.subcommand, "display-message");
        assert!(p.has('p'));
        assert_eq!(p.value('t'), Some("%3"));
        assert_eq!(p.operands(), ["#{pane_width},#{window_width}".to_string()]);
    }

    #[test]
    fn attached_flag_values() {
        let p = parse_v(&["display-message", "-F#{session_id}", "-t%2"]);
        assert_eq!(p.value('F'), Some("#{session_id}"));
        assert_eq!(p.value('t'), Some("%2"));
    }

    #[test]
    fn clustered_flag_then_negative_value() {
        let p = parse_v(&["capture-pane", "-pt", "omo", "-S", "-200"]);
        assert!(p.has('p'));
        assert_eq!(p.value('t'), Some("omo"));
        assert_eq!(p.value('S'), Some("-200"));
    }

    #[test]
    fn repeatable_env_flags_collected() {
        let p = parse_v(&["new-session", "-d", "-s", "omo-1", "-e", "A=1", "-e", "B=2"]);
        assert!(p.has('d'));
        assert_eq!(p.value('s'), Some("omo-1"));
        assert_eq!(p.values_of('e'), ["A=1", "B=2"]);
    }

    #[test]
    fn percentage_size_value() {
        assert_eq!(
            parse_v(&["split-window", "-l", "70%"]).value('l'),
            Some("70%")
        );
    }

    #[test]
    fn double_dash_forces_operands() {
        let p = parse_v(&["send-keys", "-t", "%1", "--", "-n", "Enter"]);
        assert_eq!(p.value('t'), Some("%1"));
        assert_eq!(p.operands(), ["-n".to_string(), "Enter".to_string()]);
    }

    #[test]
    fn aliases_are_normalized() {
        assert_eq!(parse_v(&["splitw", "-h"]).subcommand, "split-window");
        assert_eq!(parse_v(&["lsp"]).subcommand, "list-panes");
        assert_eq!(parse_v(&["has", "-t", "x"]).subcommand, "has-session");
    }

    #[test]
    fn errors_on_missing_value_and_missing_subcommand() {
        let missing_val: Vec<String> = ["display-message", "-F"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(parse(&missing_val).is_err());
        assert!(parse(&[]).is_err());
    }
}
