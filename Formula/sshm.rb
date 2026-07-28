class Sshm < Formula
  desc "Fast SSH + Docker + Incus + Kubernetes manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "2.0.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "e9a24d65d63697147cf9559b19b199a030ee563281ab422dabefe422327c5ebb"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "5f53f42c3e93bb893c9f3f2864b2ed23022ae12bb8494705ce797352b55347f8"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "af15068d19cd72ad74889b4002bd51ebfc4dbc24097385fd5d51cfd2a2a99684"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
