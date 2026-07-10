#!/usr/bin/env bash
# zellij-tmux-shim installer. Downloads the latest prebuilt release, installs the
# `tmux` shim + shell integration, and wires it into your shell rc. Re-run any
# time to update (the shell integration also auto-updates once per day).
#
#   curl -fsSL https://raw.githubusercontent.com/koraykoska/zellij-tmux-shim/main/install.sh | bash
set -euo pipefail

REPO="koraykoska/zellij-tmux-shim"
PREFIX="${ZELLIJ_TMUX_SHIM_PREFIX:-$HOME/.zellij-tmux-shim}"
BIN_DIR="$PREFIX/bin"

need() { command -v "$1" >/dev/null 2>&1 || { echo "error: '$1' is required but not found" >&2; exit 1; }; }
need curl
need tar

case "$(uname -s)/$(uname -m)" in
  Darwin/arm64)               target="aarch64-apple-darwin" ;;
  Linux/x86_64 | Linux/amd64) target="x86_64-unknown-linux-musl" ;;
  Linux/aarch64 | Linux/arm64) target="aarch64-unknown-linux-musl" ;;
  Darwin/x86_64)
    echo "error: prebuilt binaries are not published for Intel macOS." >&2
    echo "Build from source: git clone https://github.com/$REPO && cd zellij-tmux-shim && cargo build --release" >&2
    exit 1 ;;
  *)
    echo "error: unsupported platform: $(uname -s)/$(uname -m)" >&2
    exit 1 ;;
esac

echo "==> Resolving latest release"
tag="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
[ -n "$tag" ] || { echo "error: could not determine the latest release tag" >&2; exit 1; }

asset="zellij-tmux-shim-${target}.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "==> Downloading $asset ($tag)"
curl -fsSL "https://github.com/$REPO/releases/download/$tag/$asset" -o "$tmp/pkg.tar.gz"
tar -xzf "$tmp/pkg.tar.gz" -C "$tmp"

echo "==> Installing to $PREFIX"
mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/tmux" "$BIN_DIR/tmux"
install -m 0644 "$tmp/zellij-tmux-shim.sh" "$PREFIX/zellij-tmux-shim.sh"
printf '%s\n' "$tag" > "$PREFIX/VERSION"

src_line="source \"$PREFIX/zellij-tmux-shim.sh\""
added=""
for rc in "$HOME/.zshrc" "$HOME/.bashrc"; do
  [ -e "$rc" ] || continue
  if ! grep -qF "$src_line" "$rc" 2>/dev/null; then
    printf '\n# zellij-tmux-shim\n%s\n' "$src_line" >> "$rc"
    added="$added $rc"
  fi
done

echo
echo "Installed zellij-tmux-shim $tag -> $BIN_DIR/tmux"
if [ -n "$added" ]; then
  echo "Wired shell integration into:$added"
else
  echo "Shell integration already wired (or no rc file found). If needed, add:"
  echo "    $src_line"
fi
echo "Restart your shell (or run: source \"$PREFIX/zellij-tmux-shim.sh\"), start zellij, launch your agent."
echo "Verify inside a zellij session:  tmux -V   # -> tmux 3.4"
