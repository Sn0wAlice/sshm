class Sshm < Formula
  desc "Fast, modern SSH host manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "1.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "1230e059938327b895a355bb948e8215e7cbbf1a651aef36b0a41c6e5bfb3da4"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "86f1b16dafdb8f39797667d73df5b3818f5d4d084c635486ca2a92d402192474"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "031e70e27fcbd730c552fffb1cc43a9e0b6a44cffffdf3d4e05d61c64a49b7b4"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
