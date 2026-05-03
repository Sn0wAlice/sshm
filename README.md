<p align="center">
  <h1 align="center">SSHM</h1>
  <p align="center">A fast, modern SSH + container manager for your terminal.</p>
</p>

<img src="./.github/banner.png">

---

**SSHM** is a TUI & CLI tool written in Rust to **manage and connect to SSH hosts, Docker containers, Incus instances, and Kubernetes pods** — all from one keyboard-driven interface.

Built for developers, sysadmins, pentesters, and homelab folks who live in a terminal.

## What's in the box

### Hosts (SSH)

- **Host management** — add, edit, delete, tag, organize into nested folders
- **Fuzzy search + prefix filters** — `tag:prod host:10.* user:ubuntu`, fzf-style scoring
- **Tunnels** — saved per-host port forwards: local (`-L`), remote (`-R`), dynamic SOCKS (`-D`)
- **Multi-hop ProxyJump** — `bastion1,bastion2`, each entry resolves against your saved hosts automatically
- **Identity management** — push SSH public keys, generate new keys (`ed25519`, `ed25519-sk` FIDO2, `ecdsa`, `rsa`), load into `ssh-agent`
- **ForwardAgent (`-A`) per host** — opt-in with a visible warning, badged in the list
- **Hardware key detection** — `[HW]` badge for `*-sk` keys
- **Frecency sort + Recently Used** — `s` cycles `name → MRU → most-used → favorites → frecency`
- **Group by tag** — `g` toggles between folder view and tag view
- **Bulk actions** — `Space` selects, `T` adds tags to selection, `D` deletes, `C` clears
- **Quick connect** — `1`-`9` connects to the Nth visible host
- **Health probes** — periodic TCP + SSH banner check, latency in ms, banner version (`OpenSSH_9.6`) shown inline

### Kluster — Docker, Incus, k8s/k3s

A dedicated tab between **Hosts** and **Identities** to manage containers and pods:

- **Docker (local)** — auto-detected if `docker` is on PATH and the daemon is up
- **Docker (remote)** — pick any saved SSH host, sshm sets `DOCKER_HOST=ssh://...` and tunnels everything natively. No port to open, no TLS, no socket setup
- **Incus (local)** — auto-detected, lists containers and VMs
- **Incus (remote)** — auto-imported from `incus remote list`
- **Kubernetes / K3s** — auto-imported from every context in `~/.kube/config` and `$KUBECONFIG`
- **One Enter to shell** into any container / pod / instance — `/bin/sh` directly, no bash dance
- **One `l` to follow logs** — `Ctrl+C` returns to the TUI cleanly (no app exit)
- **Pod cleanup** — `d` on a `Succeeded`/`Failed` pod runs `kubectl delete pod`
- **Section folding** — clusters collapsed by default, `Enter` on a header toggles
- **Live discovery** — background worker polls every `kluster_refresh_secs` (configurable in Settings)

### Quality of life

- **i18n** — UI strings translatable; English + French bundled. Pick via `SSHM_LANG=fr`
- **Themes** — fully customizable colors via `theme.toml`
- **Toast notifications** — non-intrusive feedback for actions
- **Auto-export** — optionally writes a clean `~/.ssh/config` on every save
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

**Requirements:**
- Rust stable toolchain (build only)
- `ssh` client (always)
- `docker` CLI on PATH (for the Docker section of the Kluster tab — local *and* remote)
- `kubectl` on PATH (for k8s/k3s clusters)
- `incus` CLI on PATH (for Incus instances)
- A terminal with UTF-8 & ANSI support

The Kluster tab degrades gracefully — sections show `(unavailable)` when the corresponding CLI / daemon isn't reachable.

## Usage

### TUI (recommended)

```bash
sshm
```

The TUI has 6 tabs (`←` / `→` to switch):

| Tab | Purpose |
|-----|---------|
| **Hosts** | SSH host list with folders, tags, tunnels, identity management |
| **Kluster** | Docker / Incus / k8s containers and pods |
| **Identities** | Local SSH keys (`~/.ssh`), generate, push, load into agent |
| **Settings** | Defaults, health-check intervals, kluster refresh, etc. |
| **Theme** | Pick / customize TUI colors |
| **Help** | In-app help |

### Kluster Docker remote — quickstart

1. Add an SSH host in the **Hosts** tab pointing at a machine where Docker runs.
2. Make sure your SSH user is in the `docker` group on that host (`ssh user@host docker ps` should work).
3. In the **Kluster** tab, navigate to the `Docker (local)` header and press `n`.
4. Pick the host from the list. Done — sshm tunnels every `docker` call over SSH.

No ports opened, no TLS to set up, no `dockerd` socket exposed.

### CLI commands

```bash
sshm list [--filter "expr"]              # list hosts (filter: tag:foo host:1.* user:bar name:*xyz*)
sshm connect <name> [ssh-options...]     # connect to a host (alias: c)
sshm create                              # interactively create a host
sshm edit                                # edit an existing host
sshm delete                              # delete a host
sshm tag add <name> <tag1,tag2>          # add tags
sshm tag del <name> <tag1,tag2>          # remove tags
sshm load_local_conf                     # import hosts from ~/.ssh/config
sshm export [path]                       # export DB as ~/.ssh/config format
sshm add-identity <name?> [--pub key]    # push pubkey to authorized_keys
sshm help                                # full CLI reference
```

## Keyboard shortcuts

### Global

| Key | Action |
|-----|--------|
| `←` / `→` | Switch tabs |
| `q` | Quit |

### Hosts tab — list navigation

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate |
| `Enter` | Connect to host / expand-collapse folder |
| `/` or any letter | Activate fuzzy filter |
| `1`-`9` | Quick-connect to Nth visible host |
| `s` | Cycle sort mode (name / MRU / most used / favorites / frecency) |
| `g` | Toggle group-by-folder ⇆ group-by-tag |
| `f` | Toggle favorite on selected host |
| `c` | One-shot health check on selected host |

### Hosts tab — actions

| Key | Action |
|-----|--------|
| `a` | Add a host (or folder when on a folder row) |
| `e` | Edit selected host |
| `d` | Delete selected host / folder |
| `p` | Open port-forward menu (`-L` / `-R` / `-D`, persistent tunnels) |
| `i` | Push identity to selected host |
| `r` | Rename folder |
| `Space` | Toggle host in bulk selection |
| `T` (Shift+t) | Bulk-add tags to selected hosts |
| `D` (Shift+d) | Bulk-delete selected hosts (with confirm) |
| `C` (Shift+c) | Clear bulk selection |

### Kluster tab

The available actions depend on what's under the cursor.

| Key | When | Action |
|-----|------|--------|
| `↑`/`↓` `j`/`k` | always | Navigate |
| `Enter` | on a header | Expand / collapse the section |
| `Enter` | on a container / pod / instance | Open `/bin/sh` (`Ctrl+D` to exit) |
| `l` | on a container / pod / instance | Stream logs `-f` (`Ctrl+C` returns to TUI) |
| `r` | always | Force a refresh now |
| `n` | on a Docker header | Pick a saved host → register a Docker remote |
| `n` | elsewhere | Add a new k8s/k3s cluster (TUI form) |
| `e` | on a Cluster header | Edit cluster (kubeconfig / context / namespace) |
| `d` | on a Cluster header | Unlink cluster from sshm (cluster itself untouched) |
| `d` | on a Docker remote header | Unlink Docker remote (host still in Hosts tab) |
| `d` | on a Succeeded / Failed pod | `kubectl delete pod` (with confirm) |

### Identities tab

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate keys in `~/.ssh` |
| `g` | Generate a new key (interactive: ed25519 / ed25519-sk / ecdsa / rsa) |
| `p` | Push selected pubkey to a host |
| `a` | Add selected key to `ssh-agent` |
| `x` | Remove selected key from `ssh-agent` |
| `K` (Shift+k) | Clean a hostname from `~/.ssh/known_hosts` |
| `r` | Rescan `~/.ssh` |

## Configuration

### Files

| Path | Purpose |
|------|---------|
| `~/.config/sshm/host.json` | Hosts, folders, tunnels, ProxyJump |
| `~/.config/sshm/kluster.json` | Saved clusters + Incus remotes + Docker remotes |
| `~/.config/sshm/settings.toml` | Defaults, health & kluster intervals |
| `~/.config/sshm/theme.toml` | TUI color theme (optional) |

### Settings

The Settings tab (`Tab → Settings`) exposes:

- **Default Port** / **Default Username** / **Default Identity File** — used when creating new hosts
- **Export Path** — where to write the auto-exported `~/.ssh/config` (empty = disabled)
- **Auto Health Check** — toggle the background SSH probe
- **Health Refresh / Cache TTL** — seconds between probe rounds
- **Probe Connect Timeout** — TCP connect timeout in ms (banner read uses ~1/3)
- **Kluster Refresh Interval** — seconds between Docker / kubectl / Incus refreshes
- **Kluster Log Tail** — default `--tail N` for `l` (logs)

All values are live: hit Save and the background workers pick up the new TTL on the next tick.

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

### Localization

```bash
SSHM_LANG=fr sshm     # French
SSHM_LANG=en sshm     # English (default)
```

Falls back to the value of `LC_ALL` / `LANG` if `SSHM_LANG` is unset. Unknown locales fall back to English silently.

## Architecture

```
src/
├── main.rs               # CLI dispatch
├── lib.rs                # crate root
├── models.rs             # Host, Tunnel, Database
├── history.rs            # frecency, sort modes
├── i18n.rs               # localization
├── locales/              # en.toml, fr.toml
├── filter/               # fuzzy + prefix-token matcher
├── config/               # io, path, settings, export
├── ssh/                  # client, keys, agent, known_hosts, proxy
├── import/               # ~/.ssh/config parser
├── kluster/              # Docker / Incus / kubectl wrappers
│   ├── docker.rs         #   docker ps / exec / logs (local + DOCKER_HOST=ssh://)
│   ├── incus.rs          #   incus list / exec / logs (local + remotes)
│   ├── kube.rs           #   kubectl get/exec/logs/delete pod
│   ├── shell.rs          #   /bin/sh constant
│   └── db.rs             #   kluster.json + bootstrap from kubeconfig + incus remotes
├── tui/
│   ├── app/              # main loop + worker submodules
│   │   ├── health_worker.rs
│   │   ├── kluster_worker.rs
│   │   ├── kluster_actions.rs
│   │   ├── cluster_form.rs
│   │   ├── host_form.rs
│   │   └── key_flows.rs
│   ├── tabs/             # one file per tab
│   ├── ssh/              # host detail box, modals, toast, port forward
│   └── theme.rs
└── commands/             # CLI subcommands
```

## Contributing

PRs welcome — especially for:
- Terminal UX polish
- New runtime backends (LXD, Podman, ...)
- Platform support (Windows is currently best-effort)
- More translations (just drop a `src/locales/<code>.toml`)

Run `cargo test` before sending a PR — the suite covers parsers (filter, kubeconfig, ssh_config, JSON migrations) and a handful of pure logic units (frecency, ssh banner, ProxyJump resolver, etc.).

---

Made by [Sn0wAlice](https://github.com/Sn0wAlice)
