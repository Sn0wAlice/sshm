class Sshm < Formula
  desc "Fast, modern SSH host manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "1.0.2"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "12ef4942169ef65e99ecd6b7579403d9c2e96c86dd4e5bda3c664d2ffec90e6e"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "5347cea45c808feb62d4802d1f93beb034dfe19d0e1726b7b4f1027d389ddb92"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "0b125e5a56d7c1944a792c43f20ddba10608915c870bc8324730a7bf6fe6a351"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
