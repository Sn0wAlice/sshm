<p align="center">
  <h1 align="center">SSHM</h1>
  <p align="center">A fast, modern SSH host manager for your terminal.</p>
</p>

<p align="center">
  <a href="https://github.com/Sn0wAlice/sshm/blob/main/LICENSE"><img src="https://img.shields.io/github/license/Sn0wAlice/sshm?style=flat-square" alt="License"></a>
  <a href="https://github.com/Sn0wAlice/sshm/stargazers"><img src="https://img.shields.io/github/stars/Sn0wAlice/sshm?style=flat-square" alt="Stars"></a>
</p>

---

**SSHM** is a TUI & CLI tool written in Rust to **manage, organize, and connect to SSH hosts** from your terminal. It features an interactive UI, port forwarding, fuzzy search, folders, themes, and more.

Built for developers, sysadmins, pentesters, and homelab enthusiasts.

## Features

- **Host management** — add, edit, delete, tag, and organize hosts into collapsible folders
- **Interactive TUI** — keyboard-driven interface built with [Ratatui](https://github.com/ratatui/ratatui)
- **Fuzzy search** — fzf-style filtering across host names, addresses, users, and tags
- **Port forwarding** — set up SSH tunnels (`-L`) directly from the TUI
- **Identity management** — push SSH public keys to remote hosts
- **Import from `~/.ssh/config`** — one command to import all your existing hosts
- **Themes** — fully customizable colors via `theme.toml`
- **Toast notifications** — non-intrusive feedback for actions
- **CLI mode** — scriptable commands for automation

## Installation

### Homebrew (macOS / Linux)

```bash
brew tap Sn0wAlice/sshm https://github.com/Sn0wAlice/sshm
brew install sshm
```

### Download pre-built binary

Grab the latest binary from the [Releases](https://github.com/Sn0wAlice/sshm/releases/latest) page.

**Linux (amd64)**
```bash
curl -sL https://github.com/Sn0wAlice/sshm/releases/latest/download/sshm-linux-amd64.tar.gz | tar xz
sudo mv sshm /usr/local/bin/
```

**Linux (arm64)**
```bash
curl -sL https://github.com/Sn0wAlice/sshm/releases/latest/download/sshm-linux-arm64.tar.gz | tar xz
sudo mv sshm /usr/local/bin/
```

**macOS (Apple Silicon)**
```bash
curl -sL https://github.com/Sn0wAlice/sshm/releases/latest/download/sshm-darwin-arm64.tar.gz | tar xz
sudo mv sshm /usr/local/bin/
```

### Build from source

```bash
git clone https://github.com/Sn0wAlice/sshm.git
cd sshm
cargo build --release
sudo cp target/release/sshm /usr/local/bin/
```

**Requirements:** Rust stable toolchain, SSH client installed, a terminal with UTF-8 & ANSI support.

## Usage

### TUI (recommended)

```bash
sshm
```

### CLI commands

```bash
sshm list [--filter "expr"]              # list hosts, optionally filtered
sshm connect <name> [ssh-options...]     # connect to a host (alias: c)
sshm create                              # create a new host
sshm edit                                # edit an existing host
sshm delete                              # delete a host
sshm tag add <name> <tag1,tag2>          # add tags to a host
sshm tag del <name> <tag1,tag2>          # remove tags from a host
sshm load_local_conf                     # import hosts from ~/.ssh/config
sshm add-identity <name> [--pub <key>]   # push pubkey to remote host
```

## Keyboard shortcuts

### Hosts tab

| Key | Action |
|-----|--------|
| `Up` / `Down` | Navigate hosts and folders |
| `Enter` | Connect to host / expand-collapse folder |
| `/` | Activate fuzzy search |
| `a` | Add new host |
| `e` | Edit selected host |
| `d` | Delete selected host or folder |
| `p` | Port forwarding |
| `i` | Push identity (SSH key) |
| `Left` / `Right` | Switch tabs |
| `q` | Quit |

### Folders

| Key | Action |
|-----|--------|
| `Enter` | Expand / collapse |
| `a` | Add new folder |
| `r` | Rename folder |
| `d` | Delete folder |

## Configuration

| File | Description |
|------|-------------|
| `~/.config/sshm/host.json` | Host entries and folder structure |
| `~/.config/sshm/theme.toml` | TUI color theme (optional) |

### Theme example

```toml
bg = "#1e1e2e"
fg = "#cdd6f4"
accent = "#89b4fa"
muted = "#6c7086"
error = "#f38ba8"
success = "#a6e3a1"
border = "#45475a"
highlight = "#313244"
```

## Contributing

PRs are welcome — especially for terminal UX, new features, and platform support.


---

Made by [Sn0wAlice](https://github.com/Sn0wAlice)
