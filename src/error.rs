//! Typed errors and their tmux-faithful `Display` / exit-code mapping.
//!
//! Compatibility note: some messages are chosen to match real tmux wording so
//! that tools which parse stderr behave identically. Exit codes follow tmux,
//! which uses `1` for essentially every failure (including a `has-session`
//! miss).

use std::fmt;

/// The crate-wide result type.
pub type Result<T> = std::result::Result<T, ShimError>;

/// Every error condition the shim can surface.
#[derive(Debug)]
pub enum ShimError {
    /// The `zellij` subprocess could not be spawned at all.
    ZellijSpawn(std::io::Error),
    /// A `zellij` subprocess exceeded its timeout budget.
    ZellijTimeout,
    /// A `zellij` subprocess exited non-zero.
    ZellijExit { code: i32, stderr: String },
    /// `zellij` JSON output could not be parsed.
    Json(serde_json::Error),
    /// The tmux argument vector was malformed.
    BadArgs(String),
    /// A referenced session does not exist (tmux-faithful wording).
    NoSuchSession(String),
    /// A referenced pane does not exist.
    NoSuchPane(String),
    /// Invoked outside a zellij session with no real tmux to fall back to.
    NotInZellij,
    /// Passthrough was required but the real `tmux` binary was not found.
    RealTmuxMissing,
}

impl ShimError {
    /// The process exit code tmux would use for this condition.
    ///
    /// tmux exits `1` for all of these; kept as a method so the mapping has a
    /// single source of truth if that ever needs to diverge.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        1
    }
}

impl fmt::Display for ShimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZellijSpawn(e) => write!(f, "zellij-tmux-shim: failed to spawn zellij: {e}"),
            Self::ZellijTimeout => write!(f, "zellij-tmux-shim: zellij call timed out"),
            Self::ZellijExit { code, stderr } => {
                write!(f, "zellij-tmux-shim: zellij exited {code}: {stderr}")
            }
            Self::Json(e) => write!(f, "zellij-tmux-shim: failed to parse zellij output: {e}"),
            Self::BadArgs(m) => write!(f, "zellij-tmux-shim: {m}"),
            // tmux wording so stderr-parsing tools behave identically.
            Self::NoSuchSession(s) => write!(f, "can't find session: {s}"),
            Self::NoSuchPane(p) => write!(f, "can't find pane: {p}"),
            Self::NotInZellij => write!(f, "zellij-tmux-shim: not inside a zellij session"),
            Self::RealTmuxMissing => write!(f, "zellij-tmux-shim: real tmux not found on PATH"),
        }
    }
}

impl std::error::Error for ShimError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ZellijSpawn(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for ShimError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn sample_json_err() -> serde_json::Error {
        serde_json::from_str::<i32>("not json").unwrap_err()
    }

    #[test]
    fn no_such_session_matches_tmux_wording() {
        assert_eq!(
            ShimError::NoSuchSession("omo-agents-1".into()).to_string(),
            "can't find session: omo-agents-1"
        );
    }

    #[test]
    fn bad_args_is_namespaced() {
        assert_eq!(
            ShimError::BadArgs("unknown flag -Z".into()).to_string(),
            "zellij-tmux-shim: unknown flag -Z"
        );
    }

    #[test]
    fn every_variant_has_nonempty_display() {
        // Constructing each variant keeps them all live under `-D warnings`
        // and pins that `Display` never panics or renders empty.
        let variants: Vec<ShimError> = vec![
            ShimError::ZellijSpawn(std::io::Error::new(std::io::ErrorKind::NotFound, "x")),
            ShimError::ZellijTimeout,
            ShimError::ZellijExit {
                code: 2,
                stderr: "boom".into(),
            },
            ShimError::Json(sample_json_err()),
            ShimError::BadArgs("m".into()),
            ShimError::NoSuchSession("s".into()),
            ShimError::NoSuchPane("%1".into()),
            ShimError::NotInZellij,
            ShimError::RealTmuxMissing,
        ];
        for v in &variants {
            assert!(!v.to_string().is_empty());
            assert_eq!(v.exit_code(), 1);
        }
    }

    #[test]
    fn from_serde_json_maps_to_json_variant() {
        let e: ShimError = sample_json_err().into();
        assert!(matches!(e, ShimError::Json(_)));
    }

    #[test]
    fn source_present_only_for_wrapping_variants() {
        use std::error::Error as _;
        assert!(ShimError::ZellijTimeout.source().is_none());
        assert!(ShimError::ZellijSpawn(std::io::Error::other("x"))
            .source()
            .is_some());
    }
}
