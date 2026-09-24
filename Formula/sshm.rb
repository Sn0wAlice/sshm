class Sshm < Formula
  desc "Fast SSH + Docker + Incus + Kubernetes manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "2.2.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "75329374ae50bef6a2b60ae0f84195500f6955fc580a506fb5fc230a7697fdda"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "a56e362b120c9204599d5ab5edbff293d9ccbd1476bb9a2bb0ee5a8ec0c09da7"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "570d2486b1f4a4ca6733044904c167d73e13fad28df8120b21f01d69ceb09ffd"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
