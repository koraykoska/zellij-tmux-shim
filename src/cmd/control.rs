//! Version reporting and accepted-but-ignored (no-op) tmux commands.

use super::Output;
use crate::cli::args::ParsedInvocation;

const NOOP_COMMANDS: &[&str] = &[
    "source-file",
    "refresh-client",
    "attach-session",
    "detach-client",
    "next-window",
    "previous-window",
    "set-hook",
    "set-buffer",
    "list-buffers",
];

#[must_use]
pub fn is_version(inv: &ParsedInvocation) -> bool {
    matches!(inv.subcommand.as_str(), "-V" | "-v")
}

#[must_use]
pub fn is_noop(inv: &ParsedInvocation) -> bool {
    NOOP_COMMANDS.contains(&inv.subcommand.as_str())
}

#[must_use]
pub fn version() -> Output {
    Output::line("tmux 3.4")
}

#[must_use]
pub fn noop() -> Output {
    Output::ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::cli::args::parse;

    fn inv(a: &[&str]) -> ParsedInvocation {
        let owned: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
        parse(&owned).unwrap()
    }

    #[test]
    fn version_is_tmux_3_4_with_single_newline() {
        assert_eq!(version().stdout, b"tmux 3.4\n");
        assert_eq!(version().code, 0);
    }

    #[test]
    fn detects_version_flags() {
        assert!(is_version(&inv(&["-V"])));
        assert!(is_version(&inv(&["-v"])));
        assert!(!is_version(&inv(&["list-panes"])));
    }

    #[test]
    fn detects_noops() {
        assert!(is_noop(&inv(&["source-file", "/x"])));
        assert!(is_noop(&inv(&["refresh-client"])));
        assert!(!is_noop(&inv(&["split-window"])));
    }
}
