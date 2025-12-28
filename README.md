# SSHM – SSH Host Manager 🚀

**SSHM** is a powerful TUI and CLI tool written in Rust to help you **manage, organize, and connect to SSH hosts** with ease.

It supports host folders, tagging, filtering, automatic import from `~/.ssh/config`, and a full-blown SFTP explorer - all inside your terminal.  
Perfect for developers, sysadmins, pentesters, and homelab enthusiasts. 🧑‍💻⚡

---

## ✨ Key Features

### 🔐 SSH Host Management
- Add, edit, rename & delete hosts
- Supports identity files, proxy jump, and port forwarding
- Organize hosts inside folders
- Tag support + smart filtering
- Import hosts directly from your `~/.ssh/config`

### 🖥️ Full TUI Mode (Ratatui)
- Left: Host & folder explorer
- Right: Host details (and advanced actions)
- Keyboard‑driven UI

### 📁 Integrated SFTP Explorer
- Dual-panel navigation (local ↔ remote)
- Breadth-first recursive folder upload & download
- Progress bars (global) for big folders
- Background SSH execution (no MOTD/noise)
- Filter mode for fast navigation
- Automatic refresh after file transfers

### 🔍 Smart Quality-of-Life
- Config stored in `~/.config/sshm/host.json`
- Theme customization with `theme.toml`
- Intuitive keybindings (displayed in UI footer)
- Cross‑platform (Linux, macOS, Windows)

---

## 📦 Installation

### Clone & build from source

```bash
git clone https://github.com/Sn0wAlice/sshm.git
cd sshm
cargo build --release
```

Binary will be located at:
```
./target/release/sshm
```

To install system‑wide:
```bash
sudo cp ./target/release/sshm /usr/local/bin/
```

---

## ⚡ Usage

### Launch TUI (recommended)
```bash
sshm
```

### List & manage hosts using CLI prompts
```bash
sshm --cli
```

### Connect directly to a host by name
```bash
sshm connect myserver
```

### SFTP (from inside TUI)
Press `f` on a host → Full SFTP browser

---

## 🗂️ Configuration

| File | Description |
|------|-------------|
| `~/.config/sshm/host.json` | Stores all host entries & folder structure |
| `~/.config/sshm/theme.toml` | Custom colors for the TUI (optional) |

Example theme + documentation available in the wiki.

---

## ⌨️ Keyboard Shortcuts (TUI)

> Shortcuts dynamically change depending on whether a folder or host is selected.

---

## 🛠️ Build Requirements

- Rust stable toolchain (`rustup`)
- SSH installed locally
- A modern terminal with UTF‑8 + ANSI support

SSHM bundles statically-required networking libraries so users don't need OpenSSL/zlib installed.

---


## 🤝 Contributing

PRs are welcome - especially for:
- terminal UX improvements
- better folder management
- multi-platform installers

Star ⭐ the project if SSHM helps you daily!

---

## 🧑‍💻 Author

Made with ❤️ by **Sn0wAlice**  
Cybersecurity engineer & tooling enthusiast 🐾

GitHub: https://github.com/Sn0wAlice

---

> If you like SSHM, share it with your team - productivity boost guaranteed 🚀