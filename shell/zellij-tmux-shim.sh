# zellij-tmux-shim shell integration — source this from ~/.zshrc or ~/.bashrc.
# Activates ONLY inside a zellij session: puts the `tmux` shim first on PATH and
# exports a fake $TMUX so tmux-expecting tools believe they are inside tmux.
if [ -n "${ZELLIJ:-}" ]; then
  _zts_bin="${ZELLIJ_TMUX_SHIM_BIN:-$HOME/.zellij-tmux-shim/bin}"
  case ":${PATH}:" in
    *":${_zts_bin}:"*) : ;;
    *) PATH="${_zts_bin}:${PATH}"; export PATH ;;
  esac
  export TMUX="/tmp/zellij-tmux-shim/${ZELLIJ_SESSION_NAME:-default},$$,0"
  export TMUX_PANE="%${ZELLIJ_PANE_ID:-0}"
  unset _zts_bin
fi

# Auto-update (curl|bash installs only; Homebrew uses `brew upgrade`). Checks at
# most once per day, in the background, never blocking your shell. Opt out with
# ZELLIJ_TMUX_SHIM_NO_AUTOUPDATE=1.
case "$-" in
  *i*)
    if [ -n "${ZELLIJ:-}" ] && [ -z "${ZELLIJ_TMUX_SHIM_NO_AUTOUPDATE:-}" ]; then
      _zts_prefix="${ZELLIJ_TMUX_SHIM_BIN:-$HOME/.zellij-tmux-shim/bin}"
      _zts_prefix="${_zts_prefix%/bin}"
      if [ -f "$_zts_prefix/VERSION" ]; then
        _zts_stamp="$_zts_prefix/.last_update_check"
        _zts_now="$(date +%s 2>/dev/null || echo 0)"
        _zts_last=0
        [ -f "$_zts_stamp" ] && _zts_last="$(cat "$_zts_stamp" 2>/dev/null || echo 0)"
        if [ "$((_zts_now - _zts_last))" -ge 86400 ]; then
          printf '%s\n' "$_zts_now" >"$_zts_stamp" 2>/dev/null || :
          ( curl -fsSL "https://raw.githubusercontent.com/koraykoska/zellij-tmux-shim/main/install.sh" 2>/dev/null | bash >/dev/null 2>&1 & )
        fi
        unset _zts_stamp _zts_now _zts_last
      fi
      unset _zts_prefix
    fi
    ;;
esac
