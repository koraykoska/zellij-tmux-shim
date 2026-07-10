//! tmux subcommand handlers: each translates a parsed invocation into zellij
//! actions and a tmux-faithful [`Output`] (stdout/stderr bytes + exit code).

pub mod control;
pub mod keys;
pub mod layout;
pub mod lifecycle;
pub mod query;
pub mod router;

pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: u8,
}

impl Output {
    #[must_use]
    pub fn ok() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            code: 0,
        }
    }

    #[must_use]
    pub fn line(text: &str) -> Self {
        let mut stdout = text.as_bytes().to_vec();
        stdout.push(b'\n');
        Self {
            stdout,
            stderr: Vec::new(),
            code: 0,
        }
    }

    #[must_use]
    pub fn lines(items: &[String]) -> Self {
        let mut stdout = Vec::new();
        for item in items {
            stdout.extend_from_slice(item.as_bytes());
            stdout.push(b'\n');
        }
        Self {
            stdout,
            stderr: Vec::new(),
            code: 0,
        }
    }

    #[must_use]
    pub fn bytes(stdout: Vec<u8>) -> Self {
        Self {
            stdout,
            stderr: Vec::new(),
            code: 0,
        }
    }

    #[must_use]
    pub fn stderr_line(text: &str, code: u8) -> Self {
        let mut stderr = text.as_bytes().to_vec();
        stderr.push(b'\n');
        Self {
            stdout: Vec::new(),
            stderr,
            code,
        }
    }
}

pub struct Ctx {
    pub session: String,
}

impl Ctx {
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            session: crate::env::session_name().unwrap_or_else(|| "zellij".to_string()),
        }
    }

    #[cfg(test)]
    pub fn test(session: &str) -> Self {
        Self {
            session: session.to_string(),
        }
    }
}
