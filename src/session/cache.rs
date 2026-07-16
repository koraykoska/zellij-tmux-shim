//! Per-lineage rename-recovery cache.
//!
//! Soundness rests on the INVARIANT: *if the shim process is running, its
//! zellij session is alive*. So a stale `$ZELLIJ_SESSION_NAME` (not in the
//! live set) means a RENAME, and our session is present under a new name.
//! This cache persists, per env-name, the last live-set snapshot and the last
//! resolved target so a rename can be diffed exactly once and then served
//! from a stable ledger hit.
//!
//! All cache I/O is BEST-EFFORT: no function here ever returns an error to its
//! caller. On any IO/parse failure the behavior is a miss / no-op.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A cache entry: the last live-set snapshot and resolved target for one
/// stale env name.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    /// The stale `$ZELLIJ_SESSION_NAME` this entry is keyed by.
    pub env_name: String,
    /// Live sessions observed at the last successful resolve.
    pub snapshot: Vec<String>,
    /// Unix seconds of that observation.
    pub ts: u64,
    /// Last name we believed we were: the env name on the fast path, or the
    /// new name after a one-time diff discovery recovery.
    pub resolved: Option<String>,
}

/// The diff-discovery window. The diff itself is sound regardless of age (the
/// prior `resolved` name must have been in the prior snapshot, and exactly one
/// new session must have appeared); the TTL only bounds coincidental matches
/// when many sessions churn, so one hour is generous.
const TTL_SECS: u64 = 3600;

/// Current unix seconds; 0 if the clock is before the epoch.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `true` iff `ts` is within `TTL_SECS` of now.
fn fresh(ts: u64) -> bool {
    now_unix().saturating_sub(ts) <= TTL_SECS
}

/// Default cache dir: `$ZELLIJ_TMUX_SHIM_CACHE_DIR` if set, else
/// `$HOME/.cache/zellij-tmux-shim/sessions`. `None` if neither is usable.
pub(crate) fn default_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ZELLIJ_TMUX_SHIM_CACHE_DIR") {
        let p = PathBuf::from(d);
        if p.as_os_str().is_empty() {
            return None;
        }
        return Some(p);
    }
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join(".cache/zellij-tmux-shim/sessions"))
}

/// Filename for an env name inside `dir`: sanitize to `[A-Za-z0-9_-]` (keep),
/// others -> `_`, then append `.json`.
fn file_for(dir: &Path, env_name: &str) -> PathBuf {
    let sanitized: String = env_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join(format!("{sanitized}.json"))
}

/// Read + parse the entry for `env_name`. Returns `None` on miss, parse
/// failure, OR if the stored `env_name` differs from the requested key
/// (collision guard: two env names that sanitize to the same file cannot
/// cross-contaminate).
pub fn read_in(dir: &Path, env_name: &str) -> Option<Entry> {
    let path = file_for(dir, env_name);
    let bytes = std::fs::read(&path).ok()?;
    let entry: Entry = serde_json::from_slice(&bytes).ok()?;
    if entry.env_name != env_name {
        return None;
    }
    Some(entry)
}

/// Atomic write: create `dir`, write `entry` to a temp file in the same dir,
/// then rename over the target. All IO errors are ignored.
pub fn write_in(dir: &Path, entry: &Entry) {
    let _ = std::fs::create_dir_all(dir);
    let target = file_for(dir, &entry.env_name);
    let tmp = target.with_extension("json.tmp");
    let Ok(json) = serde_json::to_vec(entry) else {
        return;
    };
    if std::fs::write(&tmp, &json).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, &target);
}

/// Record a successful resolution so future runs can diff/ledger against it.
pub fn record_in(dir: &Path, env_name: &str, live: &[String], resolved: &str) {
    write_in(
        dir,
        &Entry {
            env_name: env_name.to_string(),
            snapshot: live.to_vec(),
            ts: now_unix(),
            resolved: Some(resolved.to_string()),
        },
    );
}

/// Try to recover the live name for a STALE `env_name` given the current live
/// set. Returns `None` when it cannot recover SAFELY. On a successful diff
/// discovery it persists the new mapping so repeat calls are stable.
pub fn recover_in(dir: &Path, env_name: &str, live: &[String]) -> Option<String> {
    let entry = read_in(dir, env_name)?;
    let known = entry.resolved.as_deref()?;
    // (1) STABLE LEDGER HIT: the last resolved target is still live -> reuse it.
    if live.iter().any(|s| s == known) {
        return Some(known.to_string());
    }
    // (2) ONE-TIME DIFF DISCOVERY, gated so it can never pick an unrelated session.
    if !fresh(entry.ts) {
        return None;
    }
    if !entry.snapshot.iter().any(|s| s == known) {
        return None;
    }
    // Soundness: `appeared` = `live \ snapshot` (NOT `live \ {known}`). Our
    // renamed session's new name is necessarily in it, so a lone appeared is ours.
    let appeared: Vec<&String> = live
        .iter()
        .filter(|s| !entry.snapshot.contains(s))
        .collect();
    if appeared.len() == 1 {
        let r = appeared[0].clone();
        record_in(dir, env_name, live, &r);
        return Some(r);
    }
    None
}

/// Production wrapper: record using [`default_dir`]; no-op if no dir resolves.
pub fn record(env_name: &str, live: &[String], resolved: &str) {
    if let Some(d) = default_dir() {
        record_in(&d, env_name, live, resolved);
    }
}

/// Production wrapper: recover using [`default_dir`]; `None` if no dir resolves.
pub fn recover(env_name: &str, live: &[String]) -> Option<String> {
    default_dir().and_then(|d| recover_in(&d, env_name, live))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-process counter so concurrent tests get unique temp dirs.
    static UNIQ: AtomicU64 = AtomicU64::new(0);

    fn tmp_dir(label: &str) -> PathBuf {
        let n = UNIQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("zts-cache-{}-{label}-{n}", std::process::id()))
    }

    struct DirGuard(PathBuf);
    impl DirGuard {
        fn new(label: &str) -> Self {
            let d = tmp_dir(label);
            fs::create_dir_all(&d).unwrap();
            Self(d)
        }
    }
    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_raw(dir: &Path, env_name: &str, json: &str) {
        let path = file_for(dir, env_name);
        fs::write(&path, json).unwrap();
    }

    // ---- write_in + read_in roundtrip ----

    #[test]
    fn write_in_then_read_in_returns_the_entry() {
        let g = DirGuard::new("roundtrip");
        let entry = Entry {
            env_name: "X".into(),
            snapshot: vec!["X".into(), "A".into()],
            ts: now_unix(),
            resolved: Some("X".into()),
        };
        write_in(&g.0, &entry);
        let got = read_in(&g.0, "X").expect("entry present");
        assert_eq!(got.env_name, "X");
        assert_eq!(got.snapshot, vec!["X".to_string(), "A".to_string()]);
        assert_eq!(got.resolved.as_deref(), Some("X"));
    }

    // ---- collision guard ----

    #[test]
    fn read_in_returns_none_when_stored_env_name_differs_from_key() {
        let g = DirGuard::new("collision");
        // Two env names that sanitize to the same file: "a/b" and "a_b" both
        // -> "a_b.json". Write an entry keyed "a/b"; read under "a_b" -> miss.
        write_raw(
            &g.0,
            "a_b",
            r#"{"env_name":"a/b","snapshot":["a/b"],"ts":0,"resolved":"a/b"}"#,
        );
        assert!(read_in(&g.0, "a_b").is_none());
    }

    // ---- recover_in: stable ledger hit ----

    #[test]
    fn recover_in_ledger_hit_reuses_last_resolved_when_still_live() {
        let g = DirGuard::new("ledger");
        record_in(&g.0, "X", &["X".into(), "A".into()], "Y");
        // Y is live -> ledger hit, no diff attempted.
        assert_eq!(
            recover_in(&g.0, "X", &["Y".into(), "A".into()]).as_deref(),
            Some("Y")
        );
    }

    // ---- recover_in: diff discovery (and persistence -> stable) ----

    #[test]
    fn recover_in_diff_discovery_finds_renamed_and_persists_for_next_call() {
        let g = DirGuard::new("diff");
        // Seed: we were "X", snapshot had X + A + B; now X is gone and "Y" appeared.
        let seed = Entry {
            env_name: "X".into(),
            snapshot: vec!["X".into(), "A".into(), "B".into()],
            ts: now_unix(),
            resolved: Some("X".into()),
        };
        write_in(&g.0, &seed);
        let first = recover_in(&g.0, "X", &["Y".into(), "A".into(), "B".into()]);
        assert_eq!(first.as_deref(), Some("Y"));
        // Second call must hit the ledger (Y now recorded as resolved), not diff again.
        let second = recover_in(&g.0, "X", &["Y".into(), "A".into(), "B".into()]);
        assert_eq!(second.as_deref(), Some("Y"));
    }

    // ---- recover_in: ambiguous (two appeared) ----

    #[test]
    fn recover_in_ambiguous_two_appeared_returns_none() {
        let g = DirGuard::new("ambiguous");
        let seed = Entry {
            env_name: "X".into(),
            snapshot: vec!["X".into(), "A".into()],
            ts: now_unix(),
            resolved: Some("X".into()),
        };
        write_in(&g.0, &seed);
        // live has Y AND Z as new sessions -> two appeared -> None.
        assert_eq!(
            recover_in(&g.0, "X", &["Y".into(), "Z".into(), "A".into()]),
            None
        );
    }

    // ---- recover_in: stale TTL ----

    #[test]
    fn recover_in_stale_ttl_skips_diff_and_returns_none() {
        let g = DirGuard::new("ttl");
        let seed = Entry {
            env_name: "X".into(),
            snapshot: vec!["X".into(), "A".into()],
            ts: now_unix().saturating_sub(100_000),
            resolved: Some("X".into()),
        };
        write_in(&g.0, &seed);
        // X not live, one appeared (Y), but entry is stale -> no diff.
        assert_eq!(recover_in(&g.0, "X", &["Y".into(), "A".into()]), None);
    }

    // ---- recover_in: no file, known not in snapshot ----

    #[test]
    fn recover_in_missing_file_returns_none() {
        let g = DirGuard::new("nofile");
        assert_eq!(recover_in(&g.0, "ghost", &["A".into()]), None);
    }

    #[test]
    fn recover_in_known_not_in_prior_snapshot_returns_none() {
        let g = DirGuard::new("knownmissing");
        let seed = Entry {
            env_name: "X".into(),
            snapshot: vec!["A".into(), "B".into()],
            ts: now_unix(),
            resolved: Some("X".into()),
        };
        write_in(&g.0, &seed);
        // known "X" was never in the snapshot ["A","B"] -> no diff.
        assert_eq!(
            recover_in(&g.0, "X", &["A".into(), "B".into(), "Y".into()]),
            None
        );
    }

    // ---- file_for sanitization ----

    #[test]
    fn file_for_sanitizes_unsafe_chars_to_underscore() {
        let dir = Path::new("/tmp/x");
        assert_eq!(file_for(dir, "alpha-1_2"), dir.join("alpha-1_2.json"));
        assert_eq!(file_for(dir, "a/b c"), dir.join("a_b_c.json"));
    }

    // ---- write_in ignores unwritable dir ----

    #[test]
    fn write_in_silently_ignores_unwritable_dir() {
        let g = DirGuard::new("ro");
        let inner = g.0.join("sub");
        fs::create_dir_all(&inner).unwrap();
        let mut perm = fs::metadata(&inner).unwrap().permissions();
        perm.set_mode(0o555);
        fs::set_permissions(&inner, perm).unwrap();
        let entry = Entry {
            env_name: "Z".into(),
            snapshot: vec![],
            ts: 0,
            resolved: None,
        };
        write_in(&inner, &entry);
        let mut perm = fs::metadata(&inner).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&inner, perm).unwrap();
        assert!(read_in(&inner, "Z").is_none());
    }
}
