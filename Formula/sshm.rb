class Sshm < Formula
  desc "Fast, modern SSH host manager for the terminal"
  homepage "https://github.com/Sn0wAlice/sshm"
  version "1.2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-darwin-arm64.tar.gz"
      sha256 "8914d72087103de88d7912a0e163e98abb073c2cb4763c79b955c05a25a4d785"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-amd64.tar.gz"
      sha256 "415734a01ff0e3069b7e8c890bcee76508eff24d7e2e50d8d7c3b00805a146d6"
    elsif Hardware::CPU.arm?
      url "https://github.com/Sn0wAlice/sshm/releases/download/v#{version}/sshm-linux-arm64.tar.gz"
      sha256 "9bd5b041989c6552ff65b994ba15f0037d81ae7ecdb5eaa7b73538d863244146"
    end
  end

  def install
    bin.install "sshm"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/sshm help")
  end
end
