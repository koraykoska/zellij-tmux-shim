//! Zellij-session detection, `$TMUX` synthesis, and real-`tmux` passthrough.
//!
//! When the shim runs outside a zellij session it must transparently hand off
//! to the real `tmux`. Because the shim is itself named `tmux` and sits on
//! `PATH`, discovery deliberately skips the shim's own directory to avoid an
//! infinite exec loop.

use crate::error::ShimError;
use std::path::{Path, PathBuf};

#[must_use]
pub fn in_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some_and(|v| !v.is_empty())
}

#[must_use]
pub fn session_name() -> Option<String> {
    std::env::var("ZELLIJ_SESSION_NAME")
        .ok()
        .filter(|s| !s.is_empty())
}

#[must_use]
pub fn current_pane_int() -> Option<i64> {
    std::env::var("ZELLIJ_PANE_ID")
        .ok()
        .and_then(|v| crate::idmap::pane_int_from_env(&v))
}

#[must_use]
pub fn self_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

#[must_use]
pub fn find_real_tmux(path_var: &str, self_dir: Option<&Path>) -> Option<PathBuf> {
    for entry in std::env::split_paths(path_var) {
        if let Some(sd) = self_dir {
            if same_dir(&entry, sd) {
                continue;
            }
        }
        let cand = entry.join("tmux");
        if is_executable_file(&cand) {
            return Some(cand);
        }
    }
    None
}

/// Replace the current process with the real `tmux`. Returns only on failure.
pub fn passthrough(real_tmux: &Path, args: &[String]) -> ShimError {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(real_tmux).args(args).exec();
    ShimError::ZellijSpawn(err)
}

/// Synthesize a plausible `$TMUX` value (`socket,pid,session-index`) so tools
/// that merely check for tmux presence are satisfied.
#[must_use]
pub fn synth_tmux(session: &str, pid: u32) -> String {
    format!("/tmp/zellij-tmux-shim/{session},{pid},0")
}

#[must_use]
pub fn wrap_command(envs: &[&str], command: &[String]) -> Vec<String> {
    if command.is_empty() {
        return Vec::new();
    }
    if envs.is_empty() {
        return command.to_vec();
    }
    let assignments = envs
        .iter()
        .filter_map(|kv| sh_assignment(kv))
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        format!("{assignments} exec \"$@\""),
        "sh".to_string(),
    ];
    out.extend(command.iter().cloned());
    out
}

fn sh_assignment(kv: &str) -> Option<String> {
    let (key, value) = kv.split_once('=')?;
    let valid = !key.is_empty()
        && !key.as_bytes()[0].is_ascii_digit()
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    valid.then(|| format!("{key}={}", sh_single_quote(value)))
}

fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn write_exe(dir: &Path, name: &str) {
        let p = dir.join(name);
        fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perm = fs::metadata(&p).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&p, perm).unwrap();
    }

    #[test]
    fn find_real_tmux_skips_self_dir_and_finds_downstream() {
        let base = std::env::temp_dir().join(format!("zts-env-{}", std::process::id()));
        let shim = base.join("shim");
        let real = base.join("real");
        fs::create_dir_all(&shim).unwrap();
        fs::create_dir_all(&real).unwrap();
        write_exe(&shim, "tmux");
        write_exe(&real, "tmux");

        let path = format!("{}:{}", shim.display(), real.display());
        let found = find_real_tmux(&path, Some(&shim)).unwrap();
        assert_eq!(found, real.join("tmux"));

        let only_self = shim.display().to_string();
        assert!(find_real_tmux(&only_self, Some(&shim)).is_none());

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn find_real_tmux_ignores_non_executable() {
        let base = std::env::temp_dir().join(format!("zts-env2-{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("tmux"), "not executable").unwrap();
        assert!(find_real_tmux(&base.display().to_string(), None).is_none());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn synth_tmux_has_three_fields() {
        let v = synth_tmux("mysess", 4321);
        let parts: Vec<&str> = v.split(',').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].contains("mysess"));
        assert_eq!(parts[2], "0");
    }

    #[test]
    fn wrap_command_injects_env_via_sh() {
        let cmd = vec!["opencode".to_string(), "attach".to_string()];
        let out = wrap_command(&["A=1", "B=two"], &cmd);
        assert_eq!(out[0], "/bin/sh");
        assert_eq!(out[1], "-c");
        assert_eq!(out[2], "A='1' B='two' exec \"$@\"");
        assert_eq!(&out[3..], ["sh", "opencode", "attach"]);
    }

    #[test]
    fn wrap_command_without_env_is_raw() {
        let cmd = vec!["htop".to_string()];
        assert_eq!(wrap_command(&[], &cmd), cmd);
    }

    #[test]
    fn wrap_command_skips_invalid_env_keys() {
        let cmd = vec!["run".to_string()];
        let out = wrap_command(&["OK=1", "bad key=2", "9NUM=3", "$(x)=4"], &cmd);
        assert_eq!(out[2], "OK='1' exec \"$@\"");
    }
}
