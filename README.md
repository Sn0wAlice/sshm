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
- **Clone host** — `y` duplicates the selected host (tunnels included) and drops you straight into the editor
- **Fuzzy search + prefix filters** — `tag:prod host:10.* user:ubuntu`, fzf-style scoring
- **Tunnels** — saved per-host port forwards (local `-L`, remote `-R`, dynamic SOCKS `-D`); start them in the **background** and watch / stop them from the `t` dashboard, or list and stop them from anywhere with `sshm tunnel`
- **Tunnels that come up on connect** — opt-in per tunnel; connecting to the host from the TUI starts its marked tunnels in the background first
- **Auto-restart a dropped tunnel** — opt-in per tunnel; relaunched with a growing backoff and given up on after 5 tries, so a Wi-Fi blip doesn't cost you the forward
- **Multi-hop ProxyJump** — `bastion1,bastion2`, each entry resolves against your saved hosts automatically
- **Identity management** — push SSH public keys, generate new keys (`ed25519`, `ed25519-sk` FIDO2, `ecdsa`, `rsa`), load into `ssh-agent`
- **ForwardAgent (`-A`) per host** — opt-in with a visible warning, badged in the list
- **Mosh per host** — opt-in toggle; connects via `mosh` instead of `ssh`, forwarding port / identity / ProxyJump automatically
- **Run-on-connect** — a per-host command run at login (`RemoteCommand` + `-t`), then you land in a normal shell; start it with `exec ` to take over the session yourself
- **Raw ssh options per host** — a `;`-separated list of `-o` settings (`ServerAliveInterval=30`, `SetEnv=FOO=bar`, `Ciphers=…`) for everything the dedicated fields don't cover. They apply to the interactive connection, background tunnels and fan-out alike, and are written out by `export`
- **Copy connection string** — `Y` copies `user@host` to the clipboard (`pbcopy` / `wl-copy` / `xclip` / `xsel`)
- **Per-host notes** — free-text reminder shown in the detail panel
- **Hardware key detection** — `[HW]` badge for `*-sk` keys
- **Frecency sort + Recently Used** — `s` cycles `name → MRU → most-used → favorites → frecency`
- **Group by tag** — `g` toggles between folder view and tag view
- **Bulk actions** — `Space` selects, `T` adds tags to selection, `D` deletes, `C` clears
- **Fan-out** — `X` runs one command on every selected host over SSH, with per-host output and an ok/failed summary
- **Quick connect** — `1`-`9` connects to the Nth visible host
- **Health probes** — periodic TCP + SSH banner check, latency in ms, banner version (`OpenSSH_9.6`) shown inline

### Kluster — Docker, Incus, k8s/k3s

A dedicated tab between **Hosts** and **Identities** to manage containers and pods:

- **Docker (local)** — auto-detected if `docker` is on PATH and the daemon is up
- **Docker (remote)** — pick any saved SSH host, sshm sets `DOCKER_HOST=ssh://...` and tunnels everything natively. No port to open, no TLS, no socket setup
- **Apple `container` (macOS)** — Apple's native container runtime (macOS 26+, Apple silicon) auto-detected when the `container` CLI and its system service are up. Lists / shells / logs / start-stop, same as Docker
- **Podman (local)** — auto-detected when `podman info` answers, rootless included. Same list / shell / logs / inspect / start-stop as Docker, because podman speaks the same CLI. Remotes are not wired yet
- **Incus (local)** — auto-detected, lists containers and VMs
- **Incus (remote)** — auto-imported from `incus remote list`
- **Kubernetes / K3s** — auto-imported from every context in `~/.kube/config` and `$KUBECONFIG`
- **One Enter to shell** into any container / pod / instance — `/bin/sh` directly, no bash dance (override with `kluster_shell` for images that ship it elsewhere)
- **Rich detail view** — `i` opens a scrollable inspect panel: overview (image, status, CPU/mem, platform), networking (IPs, gateway, MAC), published ports, volumes, entrypoint/command, and a live log tail. Backed by `docker`/`container inspect`
- **One `l` to follow logs** — `Ctrl+C` returns to the TUI cleanly (no app exit)
- **Lifecycle control** — `s` starts/stops and `R` restarts Docker/Apple containers and Incus instances right from the list
- **Pod cleanup** — `d` on a `Succeeded`/`Failed` pod runs `kubectl delete pod`
- **Section folding** — clusters collapsed by default, `Enter` on a header toggles
- **Live filter** — `/` fuzzy-filters containers, pods and instances across every section (force-expands while filtering)
- **Live discovery** — background worker polls every `kluster_refresh_secs` (configurable in Settings)

### Quality of life

- **i18n** — tab bar, forms, dialogs, shortcut bar and messages are all translated; English + French bundled. Pick via `SSHM_LANG=fr`. A few screens (Settings, Theme, the Kluster tab's own labels) are still English-only
- **Themes** — nine customizable color roles via `theme.toml` (`bg`, `fg`, `accent`, `muted`, `error`, `success`, plus `warning`, `border` and `selection`), with an optional transparent background that uses the terminal's own
- **Toast notifications** — non-intrusive feedback for actions
- **Desktop notifications** — native OS alerts (`notify-send` / `osascript`) when a background tunnel drops or a host changes reachability
- **Open in a new terminal** — `o` launches the SSH session in a separate terminal window (auto-detected, or set `external_terminal`)
- **Host-key trust** — vet fingerprints with `F`; on connect, a never-seen host offers trust-on-first-use with its fingerprint shown, and a *changed* host key is detected and offers to wipe the stale `known_hosts` entry and reconnect
- **Auto-export** — optionally writes a clean `~/.ssh/config` on every save
- **Config sync over git** — keep your hosts, clusters and theme in a private git repo, authenticated with an SSH key. Manual, scheduled or cron-driven; several running instances share one schedule and only one of them ever syncs. Optionally **encrypted with `age`**, so the remote never holds your inventory in clear
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

### Workspace layout

The repo is a Cargo workspace:

```
crates/sshm-core   # frontend-agnostic engine (models, config IO, ssh/kluster, filter, i18n)
crates/sshm        # the TUI + CLI (binary `sshm`)
```

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
sshm list --json                         # the same hosts as a JSON array
sshm list --names                        # one host name per line (what completions consume)
sshm connect <name> [ssh-options...]     # connect to a host (alias: c)
sshm create                              # interactively create a host
sshm edit                                # edit an existing host
sshm delete                              # delete a host
sshm tag add <name> <tag1,tag2>          # add tags
sshm tag del <name> <tag1,tag2>          # remove tags
sshm load_local_conf                     # import hosts from ~/.ssh/config
sshm export [path]                       # export DB as ~/.ssh/config format
sshm add-identity <name?> [--pub key]    # push pubkey to authorized_keys
sshm tunnel [list] [--json]              # background tunnels across every running instance
sshm tunnel stop <pid>                   # terminate one tunnel by its ssh PID
sshm sync                                # sync the config with your git repo
sshm sync setup|status|pull|push|cron    # configure / inspect / one-way / crontab line
sshm sync status --json                  # the same status as a JSON object
sshm doctor [--json]                     # what's set up, what's missing, what's wrong
sshm completions bash|zsh|fish           # print a shell completion script
sshm help                                # full CLI reference
```

### Diagnostics

```bash
sshm doctor
```

Reports the resolved config directory and whether each file parses, which CLIs
are on PATH (and whether Docker's and Podman's daemons actually answer), `~/.ssh`
and key permissions, whether a sync run would succeed and whether its encryption
is set up, and any tunnel records left behind by a crashed instance.

An absent optional CLI is reported as skipped, not failed — a machine without
`incus` is not a broken machine. The exit status is 1 only when something is
genuinely broken, so it can gate a script:

```bash
sshm doctor >/dev/null || echo "sshm needs attention"
sshm doctor --json | jq -r '.[] | select(.status=="fail") | "\(.name): \(.detail)"'
```

### Scripting and completions

Three commands take `--json` — `list`, `sync status` and `tunnel list` — and
all of them stay well-formed when there is nothing to report (an empty array,
not a sentence), so a caller never has to special-case "no results":

```bash
sshm list --json | jq -r '.[] | select(.tags[]? == "prod") | .name'
sshm sync status --json | jq -e '.problem == null'   # exit 1 when sync is broken
sshm tunnel list --json | jq '.[] | {pid, route}'
```

`sshm list --names` prints one host name per line — no quoting to undo, no
JSON parser needed — which is what the completion scripts consume.

Install completions for your shell:

```bash
sshm completions bash > /usr/local/etc/bash_completion.d/sshm
sshm completions zsh  > "${fpath[1]}/_sshm"
sshm completions fish > ~/.config/fish/completions/sshm.fish
```

They complete subcommands, and — for `connect`, `add-identity` and `tag` —
your actual saved hosts. Host names come from `sshm list --names` at
completion time, so adding a host is enough; there is nothing to regenerate.

`SSHM_VERBOSE=1` puts the "Loading DB from …" diagnostic back, on stderr.

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
| `y` | Clone selected host (full copy, opens the editor) |
| `Y` (Shift+y) | Copy the connection string (`user@host`) to the clipboard |
| `d` | Delete selected host / folder |
| `p` | Open port-forward menu — start a tunnel in the background (`f` runs it foreground). `Space` on *Start automatically* brings it up whenever you connect to the host; on *Restart automatically* it comes back if it drops |
| `t` | Background-tunnels dashboard — `d`/`x` stop a tunnel, `o` open a local tunnel's URL |
| `o` | Open the SSH session in a new terminal window |
| `F` (Shift+f) | Host key — show the pinned vs. live fingerprint, then pin (trust), forget, or replace it |
| `i` | Push identity to selected host |
| `r` | Rename folder |
| `Space` | Toggle host in bulk selection |
| `T` (Shift+t) | Bulk-add tags to selected hosts |
| `D` (Shift+d) | Bulk-delete selected hosts (with confirm) |
| `C` (Shift+c) | Clear bulk selection |
| `X` (Shift+x) | Fan-out: run a command on every selected host |

### Kluster tab

The available actions depend on what's under the cursor.

| Key | When | Action |
|-----|------|--------|
| `↑`/`↓` `j`/`k` | always | Navigate |
| `/` | always | Fuzzy-filter containers / pods / instances (`Esc` clears) |
| `Enter` | on a header | Expand / collapse the section |
| `Enter` | on a container / pod / instance | Open `/bin/sh` (`Ctrl+D` to exit) |
| `i` | on a container / pod / instance | Open the rich detail (inspect) panel — scroll with `↑`/`↓`, `Esc` closes |
| `l` | on a container / pod / instance | Stream logs `-f` (`Ctrl+C` returns to TUI) |
| `s` | on a Docker/Apple container / Incus instance | Start it if stopped, stop it if running |
| `R` (Shift+r) | on a Docker/Apple container / Incus instance | Restart it |
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
| `/` | Fuzzy-filter keys by file name / type / comment (`Esc` clears) |
| `g` | Generate a new key (interactive: ed25519 / ed25519-sk / ecdsa / rsa) |
| `p` | Push selected pubkey to a host |
| `a` | Add selected key to `ssh-agent` |
| `x` | Remove selected key from `ssh-agent` |
| `K` (Shift+k) | Clean a hostname from `~/.ssh/known_hosts` |
| `r` | Rescan `~/.ssh` |

## Configuration

### Files

Everything lives in one directory, written `<config>` below. **It is not the
same place on every OS** — sshm uses the platform's standard config location:

| OS | `<config>` |
|----|------------|
| Linux / BSD | `~/.config/sshm/` |
| macOS | `~/Library/Application Support/sshm/` |
| Windows | `%APPDATA%\sshm\` |

`sshm sync status` prints the resolved paths it is actually using, which is the
quickest way to settle any doubt.

| Path | Purpose |
|------|---------|
| `<config>/host.json` | Hosts, folders, tunnels, ProxyJump, per-host ssh options |
| `<config>/kluster.json` | Saved clusters + Incus remotes + Docker remotes |
| `<config>/settings.toml` | Defaults, health & kluster intervals |
| `<config>/theme.toml` | TUI color theme (optional) |
| `<config>/tunnels/<pid>.json` | Live background tunnels per running instance — used to clean up after a crash |
| `<config>/sync-repo/` | Working clone used by config sync — scratch space, safe to delete |
| `<config>/sync-state.json` | Last sync time/result, shared by every running instance |
| `<config>/sync.lock` | Held while a sync runs, so only one instance syncs at a time |
| `<config>/sync-age.key` | Default location for the sync encryption identity (optional) |

### Settings

The Settings tab (`Tab → Settings`) exposes:

- **Default Port** / **Default Username** / **Default Identity File** — used when creating new hosts
- **Export Path** — where to write the auto-exported `~/.ssh/config` (empty = disabled)
- **Auto Health Check** — toggle the background SSH probe
- **Health Refresh / Cache TTL** — seconds between probe rounds
- **Probe Connect Timeout** — TCP connect timeout in ms (banner read uses ~1/3)
- **Kluster Refresh Interval** — seconds between Docker / kubectl / Incus refreshes
- **Kluster Log Tail** — default `--tail N` for `l` (logs)
- **Desktop notifications** — toggle native OS alerts (tunnel dropped, host up/down)
- **Config sync** — repository URL, SSH key, branch, auto-sync interval, and the on-start / on-exit triggers (see [Config sync](#config-sync-git-over-ssh))

The Settings tab groups these into labelled sections (Defaults, Export, Health checks, Kluster, Notifications, Config sync).

All values are live: hit Save and the background workers pick up the new TTL on the next tick.

**`external_terminal`** — a `settings.toml`-only key (not shown in the Settings tab). It's the command prefix used by the `o` hotkey to open a session in a new terminal window; the SSH command is appended to it. Leave it empty to auto-detect (`wezterm`, `kitty`, `alacritty`, `gnome-terminal`, `konsole`, `xterm`, or `Terminal.app` on macOS). Examples:

```toml
external_terminal = "kitty -e"
external_terminal = "wezterm start --"
external_terminal = "gnome-terminal --"
```

**`notification_icon`** — another `settings.toml`-only key: a path (`~` allowed) to a custom icon for desktop notifications.

```toml
notification_icon = "~/.config/sshm/icon.png"   # any path; `~` is expanded
```

On **Linux** it's passed straight to `notify-send -i`. On **macOS** the default `osascript` notification *cannot* override its icon (it's always osascript's) — install [`terminal-notifier`](https://github.com/julienXX/terminal-notifier) (`brew install terminal-notifier`) and SSHM will use it automatically to honour the custom icon.

**`kluster_shell`** — a third `settings.toml`-only key: the shell `Enter` execs into a container, pod or instance. Empty means `/bin/sh`, which every mainstream image ships. SSHM deliberately does *not* probe for a nicer shell — a bash-fallback wrapper used to live here and caused more corner cases than it solved — so point this at the one your images actually have:

```toml
kluster_shell = "/busybox/sh"
```

### Config sync (git over SSH)

Keep the same hosts on your laptop, your desktop and that one server you keep
forgetting about. sshm pushes its config to **a git repository you own**, over
SSH, with **your** key. Nothing is sent anywhere you didn't configure.

```bash
sshm sync setup     # repo URL, key, what travels, how often
sshm sync           # sync now
sshm sync status    # what's configured, when it last ran, who holds the lock
```

Create an empty private repo first (GitHub, GitLab, Gitea, a bare repo on your
own box — anything reachable over SSH), then paste its **SSH** URL:
`git@github.com:you/sshm-config.git`. HTTPS URLs are rejected: they can't
authenticate with a key.

**What travels.** `host.json` and `kluster.json` by default, plus `theme.toml`;
`settings.toml` is opt-in. The `[sync]` block itself **never leaves the
machine** — it holds your key path, and syncing it would point every other
machine at the same one (or switch sync off everywhere at once).

**Encryption (optional, off by default).** Without it the repository holds your
hosts in clear — names, addresses, usernames, ports, key paths, tags and notes.
A private repo is still a repo: mirrors, backups, org-wide access, a compromised
account. Turn it on from the Settings tab, from `sshm sync setup`, or by hand:

```toml
[sync]
encrypt = true
# An absolute path, or `~`-relative. Put it wherever you like — the default
# sshm suggests is inside its own config directory, which is NOT the same
# place on every OS (see "Files" above):
#   Linux  ~/.config/sshm/sync-age.key
#   macOS  ~/Library/Application Support/sshm/sync-age.key
age_identity = "~/.config/sshm/sync-age.key"
```

It needs [`age`](https://age-encryption.org) on PATH (`brew install age`,
`apt install age`). `sshm sync setup` offers to generate the identity; copy that
one file to every machine that syncs the repo, the way you would an SSH key —
without it they cannot read what this machine pushes.

**sshm always shows you the resolved path**, because a `~` that expands
somewhere unexpected is the easiest way to end up with a key sshm never opens.
The Settings tab prints it under the field (green when the file is there, amber
when it is missing and encryption is on), `sshm sync setup` echoes it back
before generating anything, and `sshm sync status` reports it:

```
Encryption  : age, identity /home/you/.config/sshm/sync-age.key
```

Three things worth knowing:

- **Your local files stay unencrypted.** This protects what leaves the machine,
  not what sits on it. `host.json` on your own disk is unchanged.
- **Turning it on is a non-event.** Decryption is decided by looking at the
  blob, not at your settings, so history written before you switched it on
  still reads back. Turning it off again is equally undramatic.
- **It never falls back to cleartext.** If `age` is missing or the identity is
  unusable, the run aborts. `sshm sync status` says so before you find out the
  hard way.

**How conflicts resolve.** Hosts and clusters are merged *entry by entry*
against the last state you synced, so a host added on the laptop and another
added on the desktop both survive, and a host deleted on one machine stays
deleted instead of coming back. Only the same entry edited on both sides in the
same window is a real conflict, resolved by your policy (default: this machine
wins). `settings.toml` and `theme.toml` are whole-file, so the policy decides
directly.

**When it syncs** — any combination of:

| Trigger | Set with |
|---------|----------|
| Manually | `sshm sync` |
| Every N minutes | Settings tab, or `mode = "interval"` in `settings.toml` |
| On start / on exit | Settings tab toggles |
| From cron | `sshm sync cron` prints the line, `--if-due` respects the interval |

**Several instances at once are fine.** Two TUIs and a cron
entry all share one lock and one schedule through the config directory: exactly
one of them syncs each round, the others skip that tick instead of piling up
behind it. A crashed instance never wedges the lock — it's reclaimed once its
process is gone.

```toml
# settings.toml — written by `sshm sync setup`, editable by hand
[sync]
enabled = true
repo_url = "git@github.com:you/sshm-config.git"
ssh_key = "~/.ssh/id_ed25519"
branch = "main"
mode = "interval"          # or "manual"
interval_secs = 900        # floored at 60
on_start = true
on_exit = true
items = ["hosts", "kluster", "theme"]   # add "settings" to sync those too
conflict = "prefer_local"  # or "prefer_remote"
strict_host_key_checking = false
encrypt = false            # seal the payload with `age` before committing
age_identity = ""          # the identity to encrypt to / decrypt with
```

Sync shells out to your own `git`, so your `~/.ssh/config`, agent and proxy
settings all apply. A passphrase-protected key needs to be in your ssh-agent —
sync never prompts (it would hang a background worker), it fails with a clear
error instead.

### Theme example

```toml
bg = "#1e1e2e"
fg = "#cdd6f4"
accent = "#89b4fa"
muted = "#6c7086"
error = "#f38ba8"
success = "#a6e3a1"
transparent_bg = false

# Optional, `theme.toml`-only (the Theme tab edits the six above).
# Each falls back to the role it used to borrow, so leaving them out
# renders exactly like an older theme.
warning = "#f9e2af"    # a caution, not a failure — falls back to `error`
border = "#45475a"     # box borders and separators — falls back to `muted`
selection = "#585b70"  # the selected row — falls back to `accent`
```

Set `transparent_bg = true` (or tick **Transparent background** in the Theme
tab) to drop the `bg` colour entirely and let your terminal's own background —
including any transparency / blur — show through. The `bg` hex is kept on disk
so unticking the box restores it.

### Localization

```bash
SSHM_LANG=fr sshm     # French
SSHM_LANG=en sshm     # English (default)
```

Falls back to the value of `LC_ALL` / `LANG` if `SSHM_LANG` is unset. Unknown locales fall back to English silently.

## Architecture

Two crates: a frontend-agnostic engine, and the terminal frontend on top of it.
Hard rule — no terminal-UI dependency (ratatui / crossterm / inquire) may appear
in `sshm-core`'s dependency tree.

```
crates/sshm-core/src/        # the engine — no rendering, no event loop
├── models.rs                # Host, Folder, Database
├── tunnels.rs               # Tunnel model + on-disk registry
├── history.rs               # frecency, sort modes
├── i18n.rs                  # localization
├── locales/                 # en.toml, fr.toml
├── os.rs                    # OS integration (notifications, external terminal)
├── tty.rs                   # terminal handover hook (set by the frontend)
├── watch.rs                 # debounced config-dir change detection
├── filter/                  # fuzzy + prefix-token matcher
├── config/                  # io, path, settings, export
├── ssh/                     # client, keys, agent, known_hosts, proxy
├── import/                  # ~/.ssh/config parser
├── kluster/                 # Docker / Incus / Apple container / kubectl wrappers
│   ├── engine.rs           #   command construction shared by docker + podman
│   ├── docker.rs            #   docker ps / exec / logs (local + DOCKER_HOST=ssh://)
│   ├── podman.rs            #   the same, against `podman` (local only)
│   ├── incus.rs             #   incus list / exec / logs (local + remotes)
│   ├── apple.rs             #   Apple `container` runtime (macOS 26+)
│   ├── kube.rs              #   kubectl get/exec/logs/delete pod
│   ├── shell.rs             #   /bin/sh constant
│   └── db.rs                #   kluster.json + bootstrap from kubeconfig + incus remotes
└── sync/                    # git-over-SSH config sync
    ├── engine.rs            #   the run: pull, merge, push
    ├── crypt.rs             #   optional `age` encryption at the git boundary
    ├── git.rs               #   git plumbing over an SSH key
    ├── merge.rs             #   entry-by-entry three-way merge
    ├── lock.rs              #   cross-process O_EXCL lock
    └── state.rs             #   last-run timestamp, last error

crates/sshm/src/             # the TUI + CLI (binary `sshm`)
├── main.rs                  # CLI dispatch
├── lib.rs                   # crate root
├── commands/                # CLI subcommands (list, crud, tags, connect, sync)
├── ssh/                     # connect flow + add-identity wizard
└── tui/
    ├── app/                 # main loop + worker submodules
    │   ├── tab_events.rs    #   key handling for the self-contained tabs
    │   ├── health_worker.rs
    │   ├── kluster_worker.rs
    │   ├── sync_worker.rs
    │   ├── kluster_actions.rs
    │   ├── cluster_form.rs
    │   ├── host_form.rs
    │   ├── tunnels.rs
    │   ├── fanout.rs
    │   └── key_flows.rs
    ├── tabs/                # one file per tab
    ├── ssh/                 # host detail box, modals, toast, port forward
    └── theme.rs
```

## Contributing

PRs welcome — especially for:
- Terminal UX polish
- New runtime backends (LXD, Podman, ...)
- Platform support (Windows is currently best-effort)
- More translations (just drop a `crates/sshm-core/src/locales/<code>.toml`)

Before sending a PR, run what CI runs:

```bash
cargo fmt --all -- --check
cargo test --workspace
SSHM_LANG=fr cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The suite covers the engine (parsers, sync merge, cross-process lock, known_hosts, ssh argv construction) and the parts of the TUI whose state is separable from rendering (form state, the Kluster tab's selection and key handling, theme fallbacks). The French run matters: the active locale comes from the environment, so a test asserting an English literal passes on an English machine and fails on a French one.

---

Made by [Sn0wAlice](https://github.com/Sn0wAlice)
