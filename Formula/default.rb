class sshm < Formula
  desc "sshm client for managing SSH connections"
  homepage "https://github.com/Sn0wAlice/sshm"
  url "<ulr>"
  sha256 "<hash>"
  license "MIT"

  livecheck do
    url :stable
    strategy :github_latest
  end

  def install
    bin.install "sshm"
  end

  test do
    system "#{bin}/sshm", "help"
  end
end
