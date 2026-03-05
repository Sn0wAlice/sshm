class Sshm < Formula
  desc "Fast, modern SSH host manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "1.0.3"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "15b91dd9fdac0722cdb738f2929a593f72ecdfac367bf1b97ab9d0ff4887a911"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "7dc9ce6005fabb4157eb8bec16439c10ea19510750253b2891bae4266748c71a"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "9ad33ea8d02246478bf26b0607bfbbfec5d90ff069890abc058fd4b8211ae48d"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
