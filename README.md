# zellij-tmux-shim

> **A tmux CLI compatibility shim for zellij.** Run every tool that expects tmux -- including AI coding agents that spawn sub-agents into panes -- transparently inside a zellij session with zero code changes.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](./LICENSE)
[![Made with Rust](https://img.shields.io/badge/made%20with-Rust-orange)](https://www.rust-lang.org)
[![Status: functional early](https://img.shields.io/badge/status-functional--early-yellow)](https://github.com/koraykoska/zellij-tmux-shim)

`zellij-tmux-shim` is a tiny, dependency-free Rust binary named `tmux` that intercepts tmux CLI commands and translates them into `zellij action ...` calls. Tools believe they are talking to a real tmux server, even though every split, pane-geometry query, and keystroke is driven by zellij under the hood.

It is purpose-built for **AI coding agents** that spawn sub-agents into tmux panes and query pane geometry to build a grid layout. With this shim on your `PATH`, those agents auto-spawn sub-agents into **zellij** panes with zero code changes.

---

## Why

### The problem

AI coding agents like [opencode](https://github.com/sst/opencode) and [oh-my-openagent](https://github.com/code-yeongyu/oh-my-openagent) automate multi-agent workflows by spawning sub-agents into terminal multiplexer panes. They call `tmux split-window`, query `#{pane_width}` and `#{pane_height}` to build a grid, and send keystrokes with `tmux send-keys`. But they are hardcoded to talk to **tmux**.

If your daily driver is [zellij](https://zellij.dev), you face a choice: give up zellij's modern terminal workspace, or give up the AI agent's multi-pane orchestration. Neither is good.

### The solution

`zellij-tmux-shim` lets you have both. Drop a single binary named `tmux` onto your `PATH` ahead of the real `tmux`, and every tool that calls `tmux split-window`, `tmux list-panes -F '#{pane_id}'`, or `tmux send-keys -t %3 C-c` now drives zellij instead. The agent never knows the difference.

### Who this is for

- **opencode users** who want sub-agents to land in zellij panes instead of tmux panes.
- **oh-my-openagent users** running the same pattern.
- **Any developer** using a tool that shells out to `tmux` for pane management -- CI orchestrators, terminal-based dashboards, dev-environment launchers -- who prefers zellij's UI, floating panes, and native session management.

### Status

The shim is early but functional. The core opencode and oh-my-openagent sub-agent workflows (split, query geometry, send keys, read output, kill panes) are implemented and verified against zellij 0.44.3 on macOS. Additional tmux commands and broader platform support are tracked in the project plan.

---

## Features

- **Zero dependencies.** Pure Rust binary. Links only `serde` and `serde_json` for parsing zellij's JSON output. No system libraries, no runtime.
- **Safe to install globally.** Outside a zellij session, the shim transparently passes through to the real `tmux` binary. It never blocks real tmux workflows.
- **Zellij-native panes, windows, and sessions.** Tmux panes map to zellij terminal panes, windows map to zellij tabs, and "sessions" become namespaced tab-name groups so your sub-agent panes stay visible in the zellij tab bar.
- **Accurate live geometry.** Pane width, height, left, top, cursor position, window viewport size, and active state are answered from live `zellij action list-panes --json` / `list-tabs --json` data -- in character-cell units, byte-exact.
- **Tmux key-name translation.** `send-keys` translates `Enter`, `C-c`, `Tab`, `Escape`, `Space`, `BSpace`, and `C-<letter>` into the correct bytes.
- **Debug mode.** Set `TMUX_SHIM_DEBUG=1` and every zellij subprocess call is logged with its arguments, output, and exit status.
- **Configurable timeout.** Per-call subprocess timeout (default 5 seconds), adjustable via `TMUX_SHIM_TIMEOUT`.

---

## How it works

### Architecture

```
Tool (opencode / oh-my-openagent)
    |
    v
  `tmux split-window -h -P -F '#{pane_id}'`
    |
    v
  zellij-tmux-shim binary (named `tmux`, first on PATH)
    |
    +-- $ZELLIJ is set?  --> translate to `zellij action new-pane ...`
    |                        query `zellij action list-panes --json`
    |                        render tmux format variables from live data
    |
    +-- $ZELLIJ is unset? --> find real `tmux` later on PATH
                              exec it with original args (passthrough)
```

### Activation

The shim activates only inside a running zellij session. It checks for the `$ZELLIJ` environment variable, which zellij sets automatically inside every session.

Inside zellij: every tmux command is parsed, translated to one or more `zellij action ...` subprocess calls, and the output is reformatted to match what a real tmux server would return.

Outside zellij: the shim searches `PATH` for the real `tmux` binary (skipping its own directory to avoid recursion) and `exec()`s it with the original arguments. The shim is completely invisible.

### Pane / window / session mapping

| tmux concept | zellij concept | ID mapping |
|---|---|---|
| Pane `%N` | Terminal pane `terminal_N` | Integer `N` is shared |
| Window `@N` | Tab `tab_id = N` | Direct integer |
| Session | Tab-name namespace (`session-name:window`) | Derived from tab names |

Sessions are the trickiest mapping because a single attached zellij session holds every pane -- there is no nested "session per agent run" concept. The shim solves this by using tab-name prefixes as session namespaces. When an agent calls `tmux new-session -s omo-1`, the shim creates a zellij tab named `omo-1:1`. Every pane in that tab, and every new tab starting with `omo-1:`, belongs to the tmux "session" `omo-1`. Commands like `tmux list-sessions`, `tmux has-session -t omo-1`, and `tmux kill-session -t omo-1` all work against this namespace.

### Passthrough safety

The shim's passthrough mechanism is careful. It reads the current `PATH`, splits it into directories, canonicalizes each one, and skips any directory that matches the shim's own location. This prevents the infinite recursion that would happen if a binary named `tmux` searched `PATH` for `tmux` and found itself. If no real `tmux` is found on `PATH`, the shim exits with an error instead of looping.

The shell integration snippet (see Install) amplifies this safety: it prepends the shim directory to `PATH` only when `$ZELLIJ` is set, so the shim is never even on `PATH` when you are not inside zellij.

---

## Install

Prebuilt binaries are published for **macOS (Apple Silicon)**, **Linux x86_64**, and **Linux ARM64**. Requires [zellij](https://zellij.dev) 0.44.x (0.44.3 tested).

### Homebrew (recommended)

```bash
brew tap koraykoska/zellij-tmux-shim https://github.com/koraykoska/zellij-tmux-shim
brew install zellij-tmux-shim
```

Homebrew prints a one-time instruction to add two lines to your `~/.zshrc` (Homebrew does not edit your shell config for you). Follow it, restart your shell, and you're done. Update later with `brew upgrade`. The shim is intentionally **not** linked as `tmux` on your `PATH` (that would shadow the real tmux) — the shell snippet activates it only inside zellij.

### One-line installer (curl | bash)

```bash
curl -fsSL https://raw.githubusercontent.com/koraykoska/zellij-tmux-shim/main/install.sh | bash
```

Downloads the latest prebuilt binary for your platform, installs it to `~/.zellij-tmux-shim/`, and wires the shell integration into your `~/.zshrc`/`~/.bashrc` automatically. Re-run any time to update — and it also **auto-updates once per day** in the background (opt out with `ZELLIJ_TMUX_SHIM_NO_AUTOUPDATE=1`).

### Build from source

```bash
git clone https://github.com/koraykoska/zellij-tmux-shim.git
cd zellij-tmux-shim
cargo build --release   # binary at target/release/tmux
./install.sh            # or copy the binary + source the snippet manually
```

### The shell integration

Both installers set this up for you. It activates **only inside a zellij session** (it checks `$ZELLIJ`, which zellij sets), prepends the shim to `PATH`, and exports `$TMUX` / `$TMUX_PANE` so tmux-expecting tools believe they are inside tmux:

```zsh
# curl|bash installer:
source "$HOME/.zellij-tmux-shim/zellij-tmux-shim.sh"

# Homebrew (paths from `brew --prefix`):
export ZELLIJ_TMUX_SHIM_BIN="$(brew --prefix)/opt/zellij-tmux-shim/libexec"
source "$(brew --prefix)/opt/zellij-tmux-shim/share/zellij-tmux-shim/zellij-tmux-shim.sh"
```

`$TMUX` injection is verified end-to-end: child processes inherit it, and a tmux-detection check (`[ -n "$TMUX" ]`) passes inside zellij.

### Verify

Start zellij, then:

```bash
tmux -V                                                  # -> tmux 3.4
tmux display-message -p '#{pane_width}x#{pane_height}'   # -> e.g. 120x34
echo "$TMUX"                                             # -> non-empty
```

Outside zellij, `tmux` transparently passes through to the real tmux.

---

## Usage

### With AI coding agents

The primary workflow: start a zellij session, then launch your AI agent inside it. The agent shells out to `tmux` for sub-agent panes, and the shim translates every call.

**opencode:**

```bash
# Inside a zellij session
opencode
# opencode spawns sub-agents into zellij panes automatically
```

**oh-my-openagent:**

```bash
# Inside a zellij session
omo
# omo spawns sub-agents into zellij panes automatically
```

No configuration changes needed in the agent itself. The shim intercepts the `tmux` calls transparently.

### Manual usage

You can also use the shim interactively. Any `tmux` command that has a zellij equivalent will work inside a zellij session:

```bash
# Split the current pane
tmux split-window -h

# Create a new session-scoped tab
tmux new-session -d -s my-project

# Query pane geometry
tmux display-message -p '#{pane_id} #{pane_width}x#{pane_height}'

# Send keystrokes to a pane
tmux send-keys -t %1 "echo hello" Enter

# Kill a pane
tmux kill-pane -t %3
```

---

## Supported tmux commands

### Lifecycle (fully implemented)

| Command | Status | Notes |
|---|---|---|
| `split-window` | Supported | `-h` (right) and `-v` (down) split. `-c` sets cwd. `-P -F` prints created pane ID. `-e` passes env vars to the command. `-t` targets the pane to split from. |
| `new-session` | Supported | `-s` sets session name. `-d` detaches. `-P -F` prints created pane ID. |
| `new-window` | Supported | `-t` scopes to a session namespace. `-n` sets window name. `-P -F` prints created pane ID. |
| `select-window` | Supported | `-t @N` switches to the target tab. |
| `rename-window` | Supported | `-t @N` scopes the rename. |
| `kill-pane` | Supported | `-t %N` closes the terminal pane. |
| `kill-window` | Supported | `-t @N` closes the tab. |
| `kill-session` | Supported | Closes all tabs in the session namespace. |
| `respawn-pane` | Supported | Kills and recreates the pane in place. |

### Query (fully implemented)

| Command | Status | Notes |
|---|---|---|
| `display-message` / `display` | Supported | `-p` prints to stdout. `-F` / positional format string. `-t` targets pane. |
| `list-panes` | Supported | `-F` format string. `-a` / `-s` lists all panes. `-t` filters by session tab. |
| `list-windows` | Supported | `-F` format string. `-t` scopes to session namespace. |
| `list-sessions` | Supported | `-F` format string. Enumerates the base session plus discovered namespaces. |
| `has-session` | Supported | `-t` checks namespace and returns exit 0/1. |
| `capture-pane` | Supported | `-t %N` targets pane. `-S -N` captures last N lines. `--full` screen dump via zellij. |

### Input (fully implemented)

| Command | Status | Notes |
|---|---|---|
| `send-keys` | Supported | `-t` targets pane. `-l` disables key-name translation. Translates `Enter`, `C-c`, `Tab`, `Escape`, `Space`, `BSpace`, `C-<letter>`. |
| `select-pane` | Supported | `-t` targets pane for focus. `-T` renames the pane without stealing focus (agents use this to label just-created panes). |

### Layout, resize, and options

| Command | Status | Notes |
|---|---|---|
| `resize-pane` | Supported | Mapped onto zellij's relative resize. `-x`/`-y` (absolute cells or `N%`) converge toward the target in zellij's coarse steps; `-L`/`-R`/`-U`/`-D [N]` grow/shrink; `-Z` toggles zoom (fullscreen). Not cell-exact -- zellij resizes in ~5% increments, so the shim steps as close as it can. |
| `select-layout` | No-op | zellij auto-tiles; layout commands succeed with exit 0. Agents re-query geometry afterward and adapt. |
| `set-option` / `set-window-option` | No-op | Custom pane options not persisted in v1. |

### Other no-ops

| Command | Status |
|---|---|
| `source-file` | No-op |
| `refresh-client` | No-op |
| `attach-session` | No-op |
| `detach-client` | No-op |
| `next-window` / `previous-window` | No-op |
| `set-hook` | No-op |
| `set-buffer` / `list-buffers` | No-op |

---

## Format variables

The following `#{...}` variables are supported in `-F` format strings for `display-message`, `list-panes`, `list-windows`, `list-sessions`, and `-P -F` on lifecycle commands.

### Pane variables

| Variable | Source |
|---|---|
| `#{pane_id}` | Mapped from zellij pane id (`%N`) |
| `#{pane_uuid}` | Empty (no zellij equivalent) |
| `#{pane_width}` | `pane_columns` from list-panes JSON |
| `#{pane_height}` | `pane_rows` from list-panes JSON |
| `#{pane_left}` | `pane_x` from list-panes JSON |
| `#{pane_top}` | `pane_y` from list-panes JSON |
| `#{pane_index}` | Position among terminal panes in the same tab |
| `#{pane_active}` | `1` if focused, `0` otherwise |
| `#{pane_title}` | Pane title (agent-set via `select-pane -T`) |
| `#{pane_current_path}` | `pane_cwd` from list-panes JSON |
| `#{pane_current_command}` | `pane_command` from list-panes JSON |
| `#{cursor_x}` | Cursor column from `cursor_coordinates_in_pane` |
| `#{cursor_y}` | Cursor row from `cursor_coordinates_in_pane` |

### Window variables

| Variable | Source |
|---|---|
| `#{window_id}` | Mapped from zellij tab_id (`@N`) |
| `#{window_uuid}` | Empty (no zellij equivalent) |
| `#{window_index}` | Tab position |
| `#{window_name}` | Tab name |
| `#{window_width}` | `viewport_columns` from list-tabs JSON |
| `#{window_height}` | `viewport_rows` from list-tabs JSON |
| `#{window_active}` | `1` if tab is active, `0` otherwise |
| `#{window_flags}` | `*` if active, empty otherwise |
| `#{window_panes}` | Count of terminal panes in the tab |

### Session variables

| Variable | Source |
|---|---|
| `#{session_id}` | Always `$0` |
| `#{session_name}` | Session name (base zellij session or namespace) |
| `#{session_attached}` | Always `1` |
| `#{socket_path}` | Synthesized (`/tmp/zellij-tmux-shim/<session>`) |

---

## Configuration

### `TMUX_SHIM_DEBUG`

Set to any non-empty value to enable debug logging. Every `zellij action ...` subprocess call is logged to `~/.cache/zellij-tmux-shim/debug.log` with its full argument list, exit status, stdout, and stderr.

```bash
TMUX_SHIM_DEBUG=1 tmux split-window -h
# Log entry written to ~/.cache/zellij-tmux-shim/debug.log:
# zellij action new-pane --direction right -> ok=true out="terminal_7" err=""
```

### `TMUX_SHIM_TIMEOUT`

Per-call subprocess timeout in seconds. Defaults to 5. Increase if zellij is slow to respond (large sessions, remote filesystems).

```bash
TMUX_SHIM_TIMEOUT=10 tmux list-panes
```

---

## Limitations

- **`select-layout` is a no-op; `resize-pane` is approximate.** zellij auto-tiles, so `select-layout` is accepted (exit 0) as a no-op. `resize-pane` IS mapped onto zellij's relative resize, but because zellij resizes in coarse (~5%) steps, absolute `-x`/`-y` targets are approached as closely as possible rather than landing on an exact cell count. Agents that re-query live geometry afterward always get accurate current dimensions.
- **Custom pane options are not persisted.** `set-option -p @my-var value` and `set-window-option` are accepted but discarded. This is a v1 limitation; pane-level key-value storage requires zellij plugin infrastructure.
- **macOS first, Linux works.** The project is developed and verified on macOS with zsh. Linux with bash/zsh should work identically. Windows (including WSL) is not tested and not a v1 target.
- **zellij 0.44.x required.** The JSON format of `zellij action list-panes --json` changed across zellij versions. 0.44.3 is the tested version.
- **Not all tmux commands are implemented.** The shim covers the commands used by opencode and oh-my-openagent. Advanced or rarely-used tmux commands (`choose-tree`, `command-prompt`, `confirm-before`, `pipe-pane`, `break-pane`, `join-pane`, `swap-pane`, copy-mode commands, etc.) are not implemented. Running an unimplemented command prints an error to stderr and exits 1.

---

## How it compares

### vs running real tmux inside zellij

You could run a real tmux server inside a zellij pane and point your AI agent at it. This works, but it means your sub-agent panes are nested inside tmux inside zellij -- you lose zellij's native tab bar, floating panes, and session management for your sub-agents. The shim gives you native zellij panes for every sub-agent.

### vs modifying the AI agent

The alternative to a shim is modifying every agent tool to detect zellij and call `zellij action ...` directly instead of `tmux ...`. This is fragile, fork-heavy, and breaks every time the agent updates. A shim at the `PATH` level works with any tool, any version, unmodified.

---

## Contributing

Contributions are welcome. The project is early and there are many tmux commands left to implement, platform quirks to handle, and rough edges to smooth.

Areas that would benefit from contributions:

- Additional tmux commands (see Limitations).
- Linux package manager support (Homebrew formula, cargo-binstall metadata).
- Windows/WSL compatibility.
- Pane option persistence (`set-option -p`).
- Integration test suite against a real zellij session.

Before opening a PR, run `cargo test` and `cargo clippy --all-targets -- -D warnings` (both must be clean), and check the open issues for the current roadmap. The `qa/` directory contains a reusable harness and `PROBES.md` documenting the empirically verified zellij behaviors the shim relies on.

---

## License

[MIT](./LICENSE) (c) Koray Koska

[https://github.com/koraykoska/zellij-tmux-shim](https://github.com/koraykoska/zellij-tmux-shim)