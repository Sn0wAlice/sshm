class Sshm < Formula
  desc "Fast SSH + Docker + Incus + Kubernetes manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "1.5.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "127d8dd79ab5b7d558c4af528d7b228a38e8c80fd6354e36a6d3b5c95766663b"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "046b4d0147042ce84ca38329398da553a8411262a7db4a185162773e93999216"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "5c6f5af9b6860964c8b85e6a2a34d1e69db9f0a580762403cf1081a5a0da0406"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
