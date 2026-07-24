class ZellijTmuxShim < Formula
  desc "tmux CLI compatibility shim that translates tmux commands to zellij"
  homepage "https://github.com/koraykoska/zellij-tmux-shim"
  version "0.1.5"
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
      sha256 "a0f64adfea5f746a6093cb9e43422e37b7358f45f74fa09aeba2768ad62fef2c"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/koraykoska/zellij-tmux-shim/releases/download/v#{version}/zellij-tmux-shim-aarch64-unknown-linux-musl.tar.gz"
      sha256 "2fd8c21c043264cf33de1b6425a05ccd7d186ef984507be45f9760eba97e953d"
    end
    on_intel do
      url "https://github.com/koraykoska/zellij-tmux-shim/releases/download/v#{version}/zellij-tmux-shim-x86_64-unknown-linux-musl.tar.gz"
      sha256 "733fd0c6ee988c551ecc8defd142ee357ea85b26f48539ab14d85d46c13f9663"
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
      (opencode / oh-my-openagent). Verify inside zellij:  tmux -V   # -> tmux 3.4
    EOS
  end

  test do
    assert_match "tmux 3.4", shell_output("ZELLIJ=0 #{libexec}/tmux -V")
  end
end
