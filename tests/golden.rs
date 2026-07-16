//! Byte-exact golden tests that run the built `tmux` shim binary end-to-end.
//! `-V` and unknown-command need no zellij; `has-session` runs against a fake
//! `zellij` placed first on PATH so the output contract is locked in CI.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn shim() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tmux"));
    cmd.env("ZELLIJ", "0").env("ZELLIJ_SESSION_NAME", "golden");
    cmd
}

#[test]
fn version_is_byte_exact() {
    let out = shim().arg("-V").output().expect("run shim");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"tmux 3.4\n");
}

#[test]
fn unknown_command_exits_1_with_tmux_wording() {
    let out = shim().arg("bogus-xyz").output().expect("run shim");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stderr, b"unknown command: bogus-xyz\n");
}

#[test]
fn has_session_exit_codes_against_fake_zellij() {
    let dir = std::env::temp_dir().join(format!("zts-golden-has-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cache_dir = std::env::temp_dir().join(format!("zts-golden-cache-{}", std::process::id()));
    std::fs::create_dir_all(&cache_dir).unwrap();
    let fake = dir.join("zellij");
    std::fs::write(
        &fake,
        "#!/bin/sh\ncase \"$*\" in\n  *list-sessions*) echo 'golden [Created 1s ago] ';;\n  *) cat <<'JSON'\n[{\"position\":0,\"name\":\"omo-9:1\",\"active\":true,\"viewport_rows\":24,\"viewport_columns\":80,\"tab_id\":0}]\nJSON\n;;\nesac\n",
    )
    .unwrap();
    let mut perm = std::fs::metadata(&fake).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&fake, perm).unwrap();

    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let present = shim()
        .args(["has-session", "-t", "omo-9"])
        .env("PATH", &path)
        .env("ZELLIJ_TMUX_SHIM_CACHE_DIR", &cache_dir)
        .output()
        .expect("run");
    assert_eq!(present.status.code(), Some(0), "existing session exits 0");
    assert!(present.stdout.is_empty());

    let absent = shim()
        .args(["has-session", "-t", "nope"])
        .env("PATH", &path)
        .env("ZELLIJ_TMUX_SHIM_CACHE_DIR", &cache_dir)
        .output()
        .expect("run");
    assert_eq!(absent.status.code(), Some(1));
    assert_eq!(absent.stderr, b"can't find session: nope\n");

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&cache_dir).ok();
}
