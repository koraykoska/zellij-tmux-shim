class ZellijTmuxShim < Formula
  desc "tmux CLI compatibility shim that translates tmux commands to zellij"
  homepage "https://github.com/koraykoska/zellij-tmux-shim"
  version "0.1.2"
  license "MIT"

  livecheck do
    url :stable
    strategy :github_latest
  end

  # NOTE: sha256 values below are placeholders. They are filled in automatically
  # by the release workflow when a `v*` tag is pushed (see .github/workflows).
  on_macos do
    on_arm do
      url "https://github.com/koraykoska/zellij-tmux-shim/releases/download/v#{version}/zellij-tmux-shim-aarch64-apple-darwin.tar.gz"
      sha256 "e8d1fc098375170c6216023104d24434bbb3b63384d725514be7c1916930fc3b"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/koraykoska/zellij-tmux-shim/releases/download/v#{version}/zellij-tmux-shim-aarch64-unknown-linux-musl.tar.gz"
      sha256 "de80128ace15fa0775bef394ffdb00fd39ccd69cc8764c91a26ee1f640c48181"
    end
    on_intel do
      url "https://github.com/koraykoska/zellij-tmux-shim/releases/download/v#{version}/zellij-tmux-shim-x86_64-unknown-linux-musl.tar.gz"
      sha256 "e69f999fab97cf93765fa68a2c05071d90353705e87ab522e35dfc009d9f2fbc"
    end
  end

  def install
    # Install into libexec, NOT as bin/tmux: a `tmux` on Homebrew's PATH would
    # shadow the real tmux everywhere. The shell integration PATH-shadows this
    # binary only inside a zellij session.
    libexec.install "tmux"
    pkgshare.install "zellij-tmux-shim.sh"
  end

  def caveats
    <<~EOS
      zellij-tmux-shim is intentionally NOT linked as `tmux` (that would shadow
      the real tmux). Activate it inside zellij by adding to your ~/.zshrc
      (or ~/.bashrc):

        export ZELLIJ_TMUX_SHIM_BIN="#{opt_libexec}"
        source "#{opt_pkgshare}/zellij-tmux-shim.sh"

      Then restart your shell, start zellij, and launch your AI agent
      (opencode / OhMyOpenCode). Verify inside zellij:  tmux -V   # -> tmux 3.4
    EOS
  end

  test do
    assert_match "tmux 3.4", shell_output("ZELLIJ=0 #{libexec}/tmux -V")
  end
end
