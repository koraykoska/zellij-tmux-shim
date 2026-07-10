#!/usr/bin/env python3
"""Reusable pty.fork zellij QA harness for macOS.

The ONLY reliable way to drive zellij headless on macOS in this sandbox:
  - python3 pty.fork()
  - CHILD:  os.environ["TERM"]="xterm-256color";
            os.execvp("zellij", ["zellij","--session",SESS,"--new-session-with-layout",LAYOUT])
            (CRITICAL: --new-session-with-layout forces CREATE. `zellij --layout X -s NAME`
             is parsed as ATTACH and fails with "Session 'NAME' not found".)
  - PARENT: keep the master fd OPEN and drain it in a loop, because if the pty
            stdin hits EOF zellij exits. Poll for a results file the prober writes.

Layout (KDL, written to a temp file):
    layout {
        pane command="bash" { args "<PROBER.sh>" "<OUTFILE>" }
        pane
        pane
    }

Prober.sh: sleep 1.6 (let session settle), run probe `zellij action ...` commands
redirecting all output to OUTFILE, then sleep 25 (keep session + panes alive so
the parent can read OUTFILE).

Public API:
    run_in_session(layout_kdl, prober_script, timeout=30) -> (results_str, raw_drain)
        Boot a zellij session, run the prober, read OUTFILE, teardown, return.

    run_probes(prober_script, timeout=30) -> (results_str, raw_drain)
        Convenience: 3-pane tiled layout (prober is pane 0).

Teardown ALWAYS runs (even on error):
    zellij kill-session SESS ; zellij delete-session SESS -f
    assert zellij list-sessions shows no SESS.

A unique SESS per run (e.g. wa<pid>) avoids collisions.
"""

from __future__ import annotations

import os
import pty
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ZELLIJ = shutil.which("zellij") or "/opt/homebrew/bin/zellij"
SETTLE = 1.6  # seconds the prober waits before acting
HOLD = 25  # seconds the prober sleeps after writing OUTFILE (keep session alive)
DEFAULT_TIMEOUT = 30  # seconds parent polls OUTFILE

DEFAULT_LAYOUT = """\
layout {
    pane {
        command "bash"
        args "<PROBER>" "<OUTFILE>"
    }
    pane
    pane
}
"""


def _session_name() -> str:
    return f"wa{os.getpid()}"


def _kill_session(sess: str) -> None:
    """Forcefully kill + delete a zellij session; best-effort, never raises."""
    for args in (["kill-session", sess], ["delete-session", sess, "-f"]):
        try:
            subprocess.run(
                [ZELLIJ] + args,
                capture_output=True,
                timeout=5,
                check=False,
            )
        except Exception:
            pass


def _session_exists(sess: str) -> bool:
    try:
        out = subprocess.run(
            [ZELLIJ, "list-sessions"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except Exception:
        return False
    return any(line.split()[0].rstrip(":") == sess for line in out.stdout.splitlines())


def _drain(master_fd: int, buf: bytearray, timeout_s: float) -> None:
    """Drain master fd for timeout_s seconds, appending to buf."""
    end = time.monotonic() + timeout_s
    while time.monotonic() < end:
        r, _, _ = select.select([master_fd], [], [], 0.25)
        if r:
            try:
                chunk = os.read(master_fd, 8192)
            except OSError:
                break
            if chunk == b"":
                break
            buf.extend(chunk)


def run_in_session(
    layout_kdl: str,
    prober_script: str,
    *,
    timeout: int = DEFAULT_TIMEOUT,
    sess: str | None = None,
    done_marker: str | None = None,
) -> tuple[str, bytes]:
    """Boot a zellij session with the given layout+prober, return (results, raw_drain).

    The layout must contain the tokens <PROBER> and <OUTFILE>; they are replaced
    with the absolute prober script path and a temp results path respectively.

    If ``done_marker`` is given, the parent keeps polling OUTFILE until the marker
    appears in the file contents (then reads once more after a 1s settle).
    Otherwise it returns on the first non-empty OUTFILE (single-shot probes).
    """
    sess = sess or _session_name()
    tmpdir = tempfile.mkdtemp(prefix=f"zellijqa_{sess}_")
    prober_path = Path(tmpdir) / "prober.sh"
    outfile = Path(tmpdir) / "results.out"
    layout_path = Path(tmpdir) / "layout.kdl"

    prober_path.write_text(prober_script)
    os.chmod(prober_path, 0o755)

    rendered = layout_kdl.replace("<PROBER>", str(prober_path)).replace(
        "<OUTFILE>", str(outfile)
    )
    layout_path.write_text(rendered)

    raw = bytearray()
    pid = 0
    try:
        pid, master_fd = pty.fork()
        if pid == 0:  # CHILD
            os.environ["TERM"] = "xterm-256color"
            os.environ["ZELLIJ_AUTO_ATTACH"] = "false"
            os.execvp(
                ZELLIJ,
                [ZELLIJ, "--session", sess, "--new-session-with-layout", str(layout_path)],
            )
            os._exit(127)  # never reached

        # PARENT: keep master open, drain, poll OUTFILE
        deadline = time.monotonic() + timeout
        results = b""
        while time.monotonic() < deadline:
            r, _, _ = select.select([master_fd], [], [], 0.5)
            if r:
                try:
                    chunk = os.read(master_fd, 8192)
                except OSError:
                    break
                if chunk == b"":
                    break
                raw.extend(chunk)
            if outfile.exists() and outfile.stat().st_size > 0:
                contents = outfile.read_bytes()
                if done_marker is None:
                    # single-shot: first non-empty
                    time.sleep(1.0)
                    results = outfile.read_bytes()
                    break
                if done_marker.encode() in contents:
                    time.sleep(1.0)
                    results = outfile.read_bytes()
                    break
        # drain a little more for logs
        _drain(master_fd, raw, 1.0)
        try:
            os.close(master_fd)
        except OSError:
            pass
        return results.decode("utf-8", "replace"), bytes(raw)
    finally:
        # reap child if still alive
        if pid:
            try:
                os.kill(pid, 0)
                os.kill(pid, signal.SIGTERM)
                time.sleep(0.3)
                os.kill(pid, signal.SIGKILL)
            except (ProcessLookupError, OSError):
                pass
            try:
                os.waitpid(pid, os.WNOHANG)
            except OSError:
                pass
        _kill_session(sess)
        shutil.rmtree(tmpdir, ignore_errors=True)


def run_probes(
    prober_script: str,
    *,
    layout_kdl: str | None = None,
    timeout: int = DEFAULT_TIMEOUT,
    done_marker: str | None = None,
) -> tuple[str, bytes]:
    """Convenience: run with the default 3-pane tiled layout (prober = pane 0)."""
    return run_in_session(
        layout_kdl or DEFAULT_LAYOUT, prober_script, timeout=timeout,
        done_marker=done_marker,
    )


def assert_no_leaked_session(sess: str | None = None) -> None:
    """Assert no `wa<pid>`-style or given session leaked; print list-sessions."""
    if sess:
        if _session_exists(sess):
            print(f"!!! LEAKED SESSION: {sess}", file=sys.stderr)
            raise SystemExit(f"leaked session {sess}")
        return
    try:
        out = subprocess.run(
            [ZELLIJ, "list-sessions"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except Exception:
        return
    leaked = [
        line
        for line in out.stdout.splitlines()
        if line.startswith("wa") and line.split()[0].rstrip(":").startswith("wa")
    ]
    # be conservative: flag any wa<digits>
    import re

    bad = [line for line in out.stdout.splitlines() if re.match(r"wa\d+", line.strip())]
    if bad:
        print(f"!!! LEAKED SESSIONS:\n" + "\n".join(bad), file=sys.stderr)
        raise SystemExit("leaked sessions")
    print("[teardown] list-sessions:\n" + out.stdout)


if __name__ == "__main__":
    # self-test: boot, list-panes --all --json, teardown.
    script = f"""#!/bin/bash
sleep {SETTLE}
zellij action list-panes --all --json > "$1" 2>&1
sleep {HOLD}
"""
    res, raw = run_probes(script, timeout=20)
    print(res[:800])
    assert_no_leaked_session()
    print("[harness self-test OK]")
