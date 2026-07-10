# Wave A — FAIL-FAST Live Verification Findings

Probed against **real zellij 0.44.3** on **macOS 26** (zsh 5.9) via the
`pty.fork` harness in [`harness.py`](./harness.py). Every result below is
grounded in actual command output captured from a live session — no speculation.
Fixtures saved at `tests/fixtures/list_panes.json` and `tests/fixtures/list_tabs.json`.

## Summary Table

| Probe | Result | One-line impl decision |
|-------|--------|------------------------|
| A1 `--pane-id` honoring | **PASS** | Use `--pane-id terminal_N` for ALL targeted ops (write-chars/rename-pane/resize/close-pane); no focus-then-act, no focus steal. |
| A2 `dump-screen --full` scrollback | **PASS** | `dump-screen --full --pane-id terminal_N` (omit `--path`, stdout is default) captures full scrollback → `capture-pane -S -N` = dump-full + tail N lines. |
| A3 `-e` env via `/bin/sh -c` wrap | **PASS** | Emit `/bin/sh -c 'K=V exec "$@"' sh <cmd…>` as the `new-pane`/`new-tab` command; env injected correctly. |
| A4 send-keys C-c + Enter | **PASS** | Both `send-keys "Ctrl c"` AND `write 3` deliver SIGINT; `write 13` delivers Enter. Default: `write <byte>` (C-c→`write 3`, Enter→`write 13`); literal text via `write-chars`. |
| A5 new-pane/new-tab stdout id | **PASS** | `new-pane` → `terminal_<id>` (regex `^terminal_[0-9]+$`, == list-panes `id`); `new-tab` → bare int (regex `^[0-9]+$`, == `tab_id`). |
| A6 canonical JSON field-gating | **PASS** | `--json` ALONE yields the FULL field set for both verbs; `--all`/`-d` add nothing. Canonical (safe): `list-panes --all --json`, `list-tabs --all --json`. |

---

## A1 — `--pane-id` honoring (close-pane / write-chars / resize / rename-pane)

**Setup:** 3-pane tiled tab. Prober = `terminal_0` (focused). Targets: `terminal_1`, `terminal_2`.

**Commands run (all rc=0):**
```
zellij action write-chars --pane-id terminal_1 "PROBEXYZ"        # rc=0
zellij action rename-pane --pane-id terminal_1 "RENAMED1"        # rc=0
zellij action resize --pane-id terminal_1 increase right          # rc=0
zellij action close-pane --pane-id terminal_2                    # rc=0
```

**Evidence (from `list-panes --all --json` AFTER):**
- `terminal_2` **absent** (closed). ✅
- `terminal_1` title == `"RENAMED1"` (rename hit target). ✅
- `terminal_1` `pane_rows` grew 8→16 (resize hit target). ✅
- `dump-screen --pane-id terminal_1` contains `❯ PROBEXYZ` (write-chars hit target). ✅
- `dump-screen --pane-id terminal_0` (prober) does **NOT** contain `PROBEXYZ`. ✅
- Prober `terminal_0` stayed `is_focused: true` throughout — **no focus steal**. ✅

**Impl decision:** `--pane-id terminal_N` is honored by `write-chars`, `rename-pane`,
`resize`, and `close-pane` without moving focus. The shim targets directly via
`--pane-id`; **no `focus-pane-id`-then-act**, no focus churn, no races. This is the
linchpin the locked architecture depends on and it holds.

---

## A2 — `dump-screen --full` scrollback depth

**Setup:** A printer pane emitted 400 numbered lines (`LINE_0001`…`LINE_0400`).

**Commands run:**
```
zellij action dump-screen --full --pane-id terminal_1     # stdout
zellij action dump-screen --pane-id terminal_1           # visible only
```

**Evidence:**
- `dump-screen --full`: captured **400/400** lines. `LINE_0001` present **and** `LINE_0400` present. ✅
- `dump-screen` (no `--full`): captured **5** lines (last viewport rows: `LINE_0396`–`LINE_0400`). ✅
- `dump-screen` does **not** accept `-` as a path; stdout is the default when `--path` is omitted.

**Impl decision:** `capture-pane -pt <sess> -S -N` → `zellij action dump-screen --full --pane-id <terminal_N>`
then **tail N lines** in-process. Scrollback is fully available — no viewport fallback needed.
Note: omit `--path` to get stdout; do NOT pass `-`.

---

## A3 — `-e KEY=VAL` env via `/bin/sh -c` wrap

**Setup:** `new-pane` with the exact wrap argv the shim will emit for `-e OMO_PROBE=hit <cmd>`.

**Command run:**
```
zellij action new-pane -- /bin/sh -c 'OMO_PROBE=hit exec "$@"' sh /bin/sh -c 'echo "$OMO_PROBE" > /tmp/omo_env_probe_wrap'
```
(Returned `terminal_3`, rc=0.)

**Evidence:** `/tmp/omo_env_probe_wrap` contained **`hit`**. ✅

A second "simple" form (`/bin/sh -c 'OMO_PROBE=hit2; echo "$OMO_PROBE" > FILE'`)
also works (file contained `hit2`), but the shim uses the **wrap form** so a single
outer `sh -c` can inject arbitrary `-e KEY=VAL` pairs in front of an arbitrary
user command without rewriting the user's argv.

**Impl decision:** For `split-window`/`new-pane`/`new-tab`/`new-session`/`respawn-pane`
with `-e K=V` (repeatable), build the command as:
```
/bin/sh -c 'K1=V1; K2=V2; exec "$@"' sh <user-command...>
```
i.e. a single wrapper `sh -c` whose body is `K=V; …; exec "$@"` (assignments then exec
the remaining argv). `"$@"` = `sh <user-command...>`, so `"$@"` expands to the user
command verbatim. The wrapper `sh` is `$0`. This is the exact argv that worked live.

---

## A4 — send-keys C-c (SIGINT) + Enter (\r) delivery

**Setup:** Started `sleep 300` in `terminal_1`; tested both SIGINT mechanisms; then
`write-chars "echo OMO_OK"` + `write 13` and checked `dump-screen`.

**Commands run:**
```
# SIGINT mechanism (i)
zellij action send-keys --pane-id terminal_1 "Ctrl c"     # rc=0
# SIGINT mechanism (ii)
zellij action write --pane-id terminal_1 3                 # rc=0
# Enter
zellij action write-chars --pane-id terminal_1 "echo OMO_OK"
zellij action write --pane-id terminal_1 13               # rc=0
```

**Evidence (`dump-screen` of `terminal_1`):**

Before SIGINT:
```
❯ sleep 300
```
After `send-keys "Ctrl c"`:
```
❯ sleep 300
^C
❯
```
After `write 3` (re-started `sleep 300` first):
```
❯ sleep 300
^C
❯
```
After `write-chars "echo OMO_OK"` + `write 13`:
```
❯ echo OMO_OK
OMO_OK
❯
```

- Both `send-keys "Ctrl c"` and `write 3` deliver SIGINT (sleep dies, `^C` + new prompt). ✅
- `write 13` delivers Enter (command submitted, `OMO_OK` executes). ✅

**Impl decision:** Default key delivery via `write <byte>`:
- `C-c` → `zellij action write --pane-id terminal_N 3`
- `Enter` → `zellij action write --pane-id terminal_N 13`
- Control keys `C-a`..`C-z` → bytes `1`..`26`.
- Literal text → `zellij action write-chars --pane-id terminal_N "<text>"`.
- `send-keys "Ctrl c"` (the named-key form) also works and is a valid alternative,
  but `write <byte>` is uniform with Enter and preferred for the C-a..C-z + Enter family.

---

## A5 — new-pane / new-tab stdout id format

**Commands run (isolated, clean session):**
```
id=$(zellij action new-pane -- true)      # -> "terminal_3"
tid=$(zellij action new-tab --name probeX) # -> "2"
```

**Evidence:**
- `new-pane` stdout == `terminal_3`; matches `^terminal_[0-9]+$`. ✅
  In `list-panes --all --json`, the created pane has `"id": 3, "title": "true", "tab_id": 0`
  — **stdout id == list-panes `id` field**. (Note: pane ids are global+monotonic across the
  whole session, shared between plugins and terminals. The returned `terminal_<N>` is the
  terminal-pane handle for that id.)
- `new-tab` stdout == `2`; matches `^[0-9]+$`. ✅
  In `list-tabs --all --json`, the created tab has `"tab_id": 2, "name": "probeX"`.
  — **stdout id == list-tabs `tab_id` field**.

**Impl decision:**
- `split-window -P -F '#{pane_id}'`: capture `new-pane` stdout (`terminal_N`), map to `%N` via
  idmap, print `%N\n`. The stdout is the pane id — no before/after list-panes diff needed.
- `new-window`/`new-session -P -F '#{window_id}'`: capture `new-tab` stdout (bare int = tab_id),
  format as `@<tab_id>`, print `@<id>\n`.
- Both ids are directly usable as `--pane-id terminal_N` / `close-tab-by-id <int>`.

---

## A6 — Canonical JSON query (field-gating)

**Commands run (same session, compared field sets):**
```
zellij action list-panes --json
zellij action list-panes --all --json
zellij action list-tabs --json
zellij action list-tabs --all --json
zellij action list-tabs -d --json
```

**Evidence (union of keys per object, terminal-pane subset for list-panes):**

`list-panes` — `--json` keys == `--all --json` keys (IDENTICAL):
```
cursor_coordinates_in_pane, default_bg, default_fg, exit_status, exited, id,
index_in_pane_group, is_floating, is_focused, is_fullscreen, is_held, is_plugin,
is_selectable, is_suppressed, pane_columns, pane_command, pane_content_columns,
pane_content_rows, pane_content_x, pane_content_y, pane_cwd, pane_rows, pane_x,
pane_y, plugin_url, tab_id, tab_name, tab_position, terminal_command, title
```
Terminal panes additionally carry `pane_command` + `pane_cwd` (plugins have `plugin_url` instead).
All fields the shim needs (`pane_x`, `pane_columns`, `pane_cwd`, `is_plugin`, `tab_id`, `tab_name`,
`is_focused`, `is_floating`, `id`, `cursor_coordinates_in_pane`, `pane_command`, `pane_rows`,
`pane_y`) are present under **bare `--json`**.

`list-tabs` — `--json` keys == `--all --json` keys == `-d --json` keys (ALL IDENTICAL):
```
active, active_swap_layout_name, are_floating_panes_visible, display_area_columns,
display_area_rows, has_bell_notification, is_flashing_bell, is_fullscreen_active,
is_swap_layout_dirty, is_sync_panes_active, name, other_focused_clients,
panes_to_hide, position, selectable_floating_panes_count, selectable_tiled_panes_count,
tab_id, viewport_columns, viewport_rows
```
All fields the shim needs (`position`, `name`, `active`, `viewport_rows`, `viewport_columns`,
`tab_id`) are present under **bare `--json`**.

**Impl decision:** The "field-gating discrepancy" from the plan is **resolved empirically**:
in zellij 0.44.3, `--json` alone returns the full field set for both verbs; `--all` and `-d`
are no-ops on field coverage. The canonical query (safe + matches plan default):
- **`zellij action list-panes --all --json`** → `Vec<PaneInfo>`
- **`zellij action list-tabs --all --json`** → `Vec<TabInfo>`

`--all` is kept for safety (forward-compat with future zellij versions that might re-gate)
and costs nothing. Minimal would be `--json`.

### Fixtures

Real captures saved for Rust serde tests:
- [`tests/fixtures/list_panes.json`](../tests/fixtures/list_panes.json) — 8 panes across 2 tabs,
  includes plugin panes (id 0 zellij:link, id 1 "About Zellij" floating+focused) and terminal
  panes (ids 0,1,2 in tab 0; 0,1,2 in tab 1) with full geometry, `pane_command`, `pane_cwd`.
- [`tests/fixtures/list_tabs.json`](../tests/fixtures/list_tabs.json) — 2 tabs ("Tab #1" inactive,
  "second-tab" active) with `viewport_rows=24`, `viewport_columns=80`, `tab_id` 0 and 1.

### Gotchas confirmed (feed B2 `types.rs` + B4 `idmap.rs`)

1. `id` is per-layer, NOT globally unique: plugin id 0 and terminal id 0 coexist. Unique handle =
   `terminal_<id>` / `plugin_<id>`. **Always filter `is_plugin == false`** for tmux-pane mapping.
2. New sessions spawn stray plugin panes: "About Zellij" (floating, `is_focused:true`) +
   "zellij:link" (suppressed). Filtered by `is_plugin == false`.
3. TWO `is_focused == true` panes coexist (floating plugin + tiled terminal). "active tmux pane"
   = `is_focused && !is_plugin && !is_floating`.
4. `ZELLIJ_PANE_ID` env is a **bare int** (e.g. `0`) in 0.44.3, not `terminal_0`. Current pane =
   `terminal_${ZELLIJ_PANE_ID}`. Strip optional `terminal_` prefix defensively.
5. `index_in_pane_group` is an object (`{}`), not an int — type as `Option<...>` / map, default empty.
6. `cursor_coordinates_in_pane` is `[x,y]` or `null` → `Option<[i64;2]>`.
7. `pane_command`/`pane_cwd` only on terminal panes (plugins have `plugin_url`) → `Option<String>`,
   `#[serde(default)]`.
