class Sshm < Formula
  desc "Fast SSH + Docker + Incus + Kubernetes manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "1.3.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "d9f3790b84a533c785e7874b1840b6fb613f52e3b6f51ffe004e309b66e32f9f"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "4d7809a026c69021f5449be0d9a55bac3542592f57de723ba599598e90ec7cd0"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "02c7bd4f277cbd7b28f35a82069b0655b8241601a312ff959b931d7c8b346bb1"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
