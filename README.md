# sshm – SSH Host Manager

**sshm** is a command-line tool written in Rust that makes it easy to manage a list of SSH hosts stored in a local JSON file. It allows you to list, create, edit, delete, and connect to SSH hosts through an interactive terminal interface using the [`inquire`](https://github.com/mikaelmello/inquire) library.

## Installation

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) installed (via `rustup`)
- `ssh` available in your terminal

### Build

```bash
git clone https://github.com/tonrepo/sshm.git
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

It contains a JSON dictionary of SSH hosts with the following structure:
```json
{
  "my-server": {
    "name": "my-server",
    "ip": "192.168.1.10",
    "port": 22,
    "username": "alice"
  }
}
```

🧰 Available Commands
```
sshm list
```
Displays all saved hosts.
```
sshm create
```
Adds a new host interactively.
```
sshm edit
```
Edits an existing host via an interactive selection.
```
sshm delete
```
Deletes a host from the configuration.
```
sshm connect [name]
sshm c [name]
```
Connects to a host. If multiple hosts match the name, an interactive selection is shown. If no name is provided, all hosts are listed for selection.

## Example

```bash
$ sshm create
Name: dev-server
IP: 10.0.0.5
Port: 22
Username: ubuntu

$ sshm list
dev-server => ubuntu@10.0.0.5:22

$ sshm c dev
# ssh to ubuntu@10.0.0.5 -p 22
```

## 🛠️ Main Dependencies
- inquire – Interactive CLI interface
- serde + serde_json – JSON reading/writing
- dirs – User configuration path handling
