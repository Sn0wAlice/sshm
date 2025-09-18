# sshm – SSH Host Manager

**sshm** is a command-line tool written in Rust that makes it easy to manage a list of SSH hosts stored in a local JSON file. It allows you to list, create, edit, delete, and connect to SSH hosts through an interactive terminal interface using the [`inquire`](https://github.com/mikaelmello/inquire) library, or through a full TUI mode powered by [`ratatui`](https://github.com/tui-rs-revival/ratatui). It also supports SSH connection overrides (e.g., `-i`, `-J`, `-L/-R/-D`), tag management, filtering, and can import hosts from your existing `~/.ssh/config` file.

## Installation

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) installed (via `rustup`)
- `ssh` available in your terminal
- A terminal compatible with [`ratatui`](https://github.com/tui-rs-revival/ratatui) for TUI mode

### Build

```bash
git clone https://github.com/Sn0wAlice/sshm.git
cd sshm
cargo build --release
```

The binary will be available at `./target/release/sshm`.

To use it globally:

```bash
cp ./target/release/sshm /usr/local/bin/
```

## Configuration File

The file is automatically created at the following location if it doesn't exist:

```
$HOME/.config/sshm/host.json
```

It contains a JSON object with all hosts and folders:
```json
{
  "hosts": {
    "my-server": {
      "name": "my-server",
      "host": "192.168.1.10",
      "port": 22,
      "username": "alice",
      "tags": ["production", "web"],
      "identity_file": "~/.ssh/id_rsa",
      "proxy_jump": "jump-host",
      "folder": null
    }
  },
  "folders": ["projetA", "projetB"]
}
```

- `folder: null` means the host is in the root.
- The top-level `folders` array lists available folders (no subfolders supported).

Entries from your `~/.ssh/config` file are automatically imported unless this behavior is disabled in the configuration.

## 🧰 Available Commands
```
sshm list [--filter "expr"]
sshm create
sshm edit
sshm delete
sshm connect (c) <name> [overrides...]
sshm tag add <name> <tag1,tag2,...>
sshm tag del <name> <tag1,tag2,...>
sshm tui
sshm help

-> only "sshm" without arg launch the interactive TUI system
```
- `connect [overrides...]` accepts SSH options like `-i key`, `-J jump`, `-L local:remote`, etc., to override host settings for the session.
- `list --filter` supports filtering hosts by matching name, IP, username, or tags using wildcards.

## TUI Mode

Run the full terminal user interface with:

```
sshm tui
```

Navigate hosts with arrow keys (↑/↓), filter the list by pressing `/`, connect to a selected host with `Enter`, and quit the TUI with `q`.

## Example

```bash
$ sshm create
Name: dev-server
IP: 10.0.0.5
Port: 22
Username: ubuntu
Tags (comma separated): dev,test

$ sshm tag add dev-server staging
Added tag 'staging' to dev-server

$ sshm tag del dev-server test
Removed tag 'test' from dev-server

$ sshm list --filter "dev*"
dev-server => ubuntu@10.0.0.5:22 [dev, staging]

$ sshm c dev-server -i ~/.ssh/custom_key -J jump-host -L 8080:localhost:80
# ssh to ubuntu@10.0.0.5 -p 22 with specified overrides

$ sshm tui
# opens the TUI interface for interactive host management

# Avec l’alias
$ sshm add-identity web-1
# Ou avec une clé précise
$ sshm add-identity web-1 --pub ~/.ssh/id_ed25519.pub
# Sans alias (menu)
$ sshm add-identity --pub ~/.ssh/id_rsa.pub
```

## 🛠️ Main Dependencies
- inquire – Interactive CLI interface
- serde + serde_json – JSON reading/writing
- dirs – User configuration path handling
- ratatui – Terminal UI for TUI mode
- ssh_config – Parsing `~/.ssh/config` files
