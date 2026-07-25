class Sshm < Formula
  desc "Fast SSH + Docker + Incus + Kubernetes manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "1.5.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "bb278f40b99ff642e0e5da3ed364c117610bca22c6263c1ee3e68835fdc82304"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "8bf78eab1d68960665fdaebe0d3d9233c1211b68e3c8f988cd1950e6b680b62e"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "c767193ebd8793c068680e061a14b08a1dbf2ad5a8528919fd886c78f32b7b04"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
