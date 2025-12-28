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

## 🛠️ Main Dependencies
- inquire – Interactive CLI interface
- serde + serde_json – JSON reading/writing
- dirs – User configuration path handling
- ratatui – Terminal UI for TUI mode
- ssh_config – Parsing `~/.ssh/config` files
