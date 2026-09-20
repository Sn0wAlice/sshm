# Changelog

All notable changes to **sshm** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.1.2] - 2026-08-28

### Added

- **Config sync over git (`sshm sync`).** Point sshm at a private git
  repository of your own and an SSH key, and it keeps `host.json`,
  `kluster.json`, `theme.toml` and (opt-in) `settings.toml` in step across
  machines. `sshm sync setup` walks through repo, key, branch, what travels,
  the schedule and the conflict policy; `sshm sync status` shows what is
  configured, when it last ran and who holds the lock; `sshm sync pull` /
  `push` move data one way only. Authentication is SSH-key based — HTTPS
  remotes are rejected up front, and sync never prompts for a passphrase
  (a key that needs one must be in your ssh-agent).
- **Entry-level merging.** Hosts and clusters are reconciled entry by entry
  against the last synced state, so a host added on the laptop and another
  added on the desktop both survive, and a host deleted on one machine stays
  deleted. Only the same entry edited on both sides within one window counts
  as a conflict, resolved by the configured policy (default: this machine
  wins).
- **Scheduling, without stepping on itself.** Sync on a timer, when sshm
  starts, when it exits, or from cron (`sshm sync cron` prints the crontab
  line; `--if-due` respects the interval and stays silent when idle). Several
  running instances — two TUIs, a cron entry — share one
  schedule and one lock through the config directory, so exactly one of them
  syncs each round and the others skip the tick. A lock whose process died is
  reclaimed automatically.
- **Settings tab section.** "Config sync (git over SSH)": enable, repository
  URL, SSH key, branch, auto-sync interval (0 = manual) and the on-start /
  on-exit toggles. The rest (which files travel, the conflict policy) lives in
  `settings.toml` and `sshm sync setup`.

### Changed

- **The TUI now picks up external changes to `settings.toml`,** not just to
  `host.json` — a background sync that rewrites the settings no longer gets
  overwritten by the running instance's stale copy on the next Save. Unsaved
  edits in the Settings tab still win until you save or press Esc.

## [2.2.0] - 2026-09-20

### Added

- **Raw ssh options per host.** A new `ssh_options` list on a host carries
  anything the dedicated fields don't model — `ServerAliveInterval=30`,
  `SetEnv=FOO=bar`, `Ciphers=…` — as `-o` flags. Editable from the host form
  (`;`-separated, so commas inside a value survive) and from `sshm create` /
  `sshm edit`, validated as `keyword=value` on the way in, shown in the detail
  panel, and written to `~/.ssh/config` by `export`. Existing `host.json`
  files load unchanged.
- **`sshm tunnel`.** `list` shows every background tunnel across every running
  sshm — reading the same per-instance record files the TUI dashboard writes —
  and `stop <pid>` terminates one, with the same PID-reuse guard the TUI
  applies before signalling anything. Read-and-stop only: starting a tunnel
  from a command that exits immediately would leave an `ssh -N` nobody owns,
  and the next TUI launch would reap it as an orphan.
- **Auto-restart for background tunnels.** A per-tunnel opt-in (`Space` on the
  new row in the port-forward form). A dropped tunnel is relaunched with a
  growing backoff — 2s, 5s, 15s, then 30s — and given up on after 5 tries, so
  a host that is gone for good doesn't respawn forever. A tunnel you stopped
  yourself stays stopped, and one whose host was deleted meanwhile is dropped
  with a notification instead of retried.
- **Optional encryption of what sync publishes.** Turn `encrypt` on (Settings
  tab, `sshm sync setup`, or `settings.toml`) and the payload is sealed with
  [`age`](https://age-encryption.org) before it reaches a commit — so the git
  remote no longer holds your hostnames, addresses, usernames, key paths and
  notes in clear. One identity file, copied to each machine the way an SSH key
  is; `sshm sync setup` offers to generate it. Off by default.

  Three properties worth knowing. **Your local files stay unencrypted** — this
  protects what leaves the machine, not what sits on it. **Reading is
  content-sniffed**, so turning it on is a non-event: the next push encrypts
  and the history written before it still reads back. And **there is no silent
  fallback** — if `age` is missing or the identity is unusable, the run aborts
  instead of pushing cleartext, which `sshm sync status` and the preflight both
  report up front.
- **The interface speaks French, not just its toasts.** i18n used to cover
  messages only — 35 call sites, all in two files — while every tab title,
  form label, dialog and shortcut hint was hard-coded English. The chrome is
  now translated too: the tab bar, the contextual shortcut bar and its `h`
  popup, the host form, the port-forward form, the delete confirmations, the
  host detail panel, the fan-out prompts and the empty states. 128 call sites
  across 11 files, 145 keys in each bundle.
- **Three more theme roles.** `warning`, `border` and `selection` join the six
  existing ones in `theme.toml`. Each falls back to the role it used to borrow
  — `error`, `muted` and `accent` respectively — so an existing theme renders
  byte-for-byte as before, and the built-in theme now distinguishes a caution
  (an enabled ForwardAgent) from a failure. Saving from the Theme tab, which
  edits only the original six, preserves the three.
- **The container shell is configurable.** `Enter` in the Kluster tab still
  execs `/bin/sh`, and still doesn't probe for anything nicer — the
  bash-fallback wrapper that used to live there caused more corner cases than
  it solved. But the path is no longer hard-coded: set `kluster_shell` in
  `settings.toml` and every Docker / Apple / Incus / kubectl exec uses it, so a
  distroless or busybox image is reachable without a guessing wrapper.
- **`ForwardAgent` is now exported.** A host with `-A` enabled emits
  `ForwardAgent yes` in the exported ssh config; previously the setting was
  silently dropped.

### Changed

- **The whole tree is `rustfmt`-formatted, and CI enforces it.** It never had
  been; every file drifted from the default style, so any future diff would
  have mixed real changes with reformatting. This is a layout-only change —
  the suite passes unchanged before and after.
- **`run_tui` is 300 lines shorter.** The Settings, Theme, Identities and
  Kluster key-event arms moved out of the ~1760-line loop into
  `app/tab_events.rs`, each taking the slice of state it actually touches.
  `app/mod.rs` goes from 2065 to 1766 lines. The Hosts arm — 730 lines that
  reach most of the loop's state — is untouched and still needs that state
  bundled before it can follow.

### Removed

- **The desktop GUI (`sshm-desktop`).** The Tauri 2 + Svelte app and its
  crate (`crates/sshm-gui`) are gone; sshm is a terminal tool again. The
  engine dropped the pieces that existed only to serve it: `sshm-core`'s
  `specta` and `pty` features (and the `portable-pty` dependency), the
  `pty` module, and the GUI jobs in CI and the release workflow. Nothing in
  the TUI, the CLI or the on-disk database format changes.
- **Unused `regex` dependency.** Declared by both crates, referenced by
  neither. (It stays in the lockfile as a transitive dependency of ratatui.)

### Fixed

- **Mosh broke on any ssh argument containing a space.** The ssh flags are
  passed to mosh as one `--ssh=` string that mosh splits again on whitespace,
  so an identity path like `~/.ssh/my keys/id_ed25519` arrived as two
  arguments and the connection failed. Each flag is now shell-quoted.
- **Cancelling `sshm create` or `sshm edit` panicked.** Every prompt was
  `unwrap()`ed, so Esc or Ctrl-C aborted the process with a Rust panic instead
  of returning to the shell. Cancelling now abandons the operation cleanly, and
  `sshm edit` gathers every answer before writing, so a late cancel leaves the
  host exactly as it was. `create` also refuses an empty name, an empty host
  and an alias that already exists instead of silently overwriting.
- **Fan-out could hang on a host that answered and then went quiet.** The
  handshake was bounded by `ConnectTimeout`, but a connection that dropped
  mid-command left ssh waiting indefinitely and stalled the rest of the batch.
  `ServerAliveInterval` / `ServerAliveCountMax` now bound that case too; a
  command that is genuinely still running is untouched.
- **The documented config path was wrong on macOS.** The README and
  `sshm help` both said `~/.config/sshm/`, while `dirs::config_dir()` resolves
  to `~/Library/Application Support/sshm/` there — the project's main
  development platform. `sshm help` now prints the resolved directory, and the
  README states the per-OS location instead of asserting one. The same
  mismatch was a trap for the new `age_identity`: a plausible
  `~/.config/sshm/sync-age.key` would sit in a directory sshm never opens, so
  the suggested default is derived from the real config directory and every
  surface — the Settings tab, `sshm sync setup`, `sshm sync status` — prints
  the path as it resolves.
- **CI never ran.** `push` and `pull_request` were commented out in
  `ci.yml`, leaving only manual dispatch — so the test suite and clippy only
  ran when someone clicked. Both triggers are back, and the job now also
  checks formatting and runs the suite a second time under `SSHM_LANG=fr`
  (the active locale comes from the environment, so an English-only run can
  pass while a French machine fails).
- **Four copies of the config directory.** `settings_path`, `theme_path` and
  two `tunnels_dir` implementations each rebuilt the path from
  `dirs::config_dir()` instead of calling `config::path::config_dir()`. They
  agreed today but differed in their fallbacks, and the TUI's copy was the one
  `sshm tunnel` reads through the engine — the two could have ended up looking
  in different directories.
- **A form row could be clipped on an 80-column terminal.** The port-forward
  toggles carried a "(Space to toggle)" suffix that pushed the line past the
  modal's width — in English already, and further in French. The hint moved to
  the form's footer next to the other keys, and a test now holds every form
  label to a width budget.
- **Background tunnels ignored per-host ssh settings.** `TunnelManager::start`
  built its own ssh command rather than calling the engine's
  `build_tunnel_argv`, which in turn meant the `ssh_options` added in this
  release never reached a tunnel — contrary to what the entry above claims. The
  foreground (`f`) tunnel path had a third copy, and `portforward.rs` a second
  `build_forward_arg`. All of it now goes through the engine, and
  `build_tunnel_argv` / `read_all_records` — public API that nothing had
  called since the GUI was removed — are live code again.
- **The Kluster worker probed remotes one at a time.** Docker remotes, Incus
  remotes and cluster apiservers are independent network round-trips, but a
  refresh pass walked them serially, so one pass cost the *sum* of their
  latencies — visibly slow with a handful of remotes on the default 10s
  interval. They now run concurrently, eight at a time.
- **Fan-out ignored some per-host connection settings.** It rebuilt the ssh
  flags itself rather than reusing the engine's builder, so it could reach a
  host differently from an interactive connect. All three call sites — the
  interactive connection, background tunnels and fan-out — now go through a
  single `build_ssh_opts`.

## [2.1.1] - 2026-08-21

Maintenance release: a clean build and one version number everywhere.

### Fixed

- **Release-build warning in the desktop crate.** `ts_exporter` is only reached
  from the `debug_assertions` binding export and from the export test, so
  `cargo build --release -p sshm-desktop` reported it as dead code. Marked
  accordingly — the release build is now warning-free.
- **Version drift.** The Tauri config still said `2.0.1` and the GUI's
  `package.json` still said `1.5.1`, so the desktop bundles shipped under the
  wrong version. The workspace crates, the desktop bundle and the frontend
  package all report `2.1.1` now.

## [1.5.0] - 2026-07-07

The container release — Apple's native `container` runtime, a rich inspect
view, run-on-connect commands, and quieter background work.

### Added

- **Apple `container` support (macOS).** The Kluster tab now speaks Apple's
  native container runtime (macOS 26+, Apple silicon), alongside Docker, Incus
  and Kubernetes. Auto-detected when the `container` CLI and its system service
  are up, shown as a **local** section on a Mac and hidden everywhere else.
  Supports list, `Enter` to shell in, `l` to follow logs, and `s`/`R` to
  start · stop · restart (restart emulated with stop + start).
- **Rich detail view (`i`).** Press `i` on any container, instance or pod to
  open a scrollable inspect panel: Overview (image, status, CPU/memory,
  OS-arch, created/started), Networking (IPv4, gateway, MAC, hostname), Ports,
  Volumes/mounts, Command/entrypoint, and a live log tail. Full detail for
  Docker (local + remote) and Apple containers via `inspect`; a compact view
  for Incus instances and k8s pods. Scroll with `↑`/`↓` · `j`/`k` ·
  `PgUp`/`PgDn` · `Home`/`End`, `Esc` to close.
- **Run-on-connect.** Each host can carry a command run automatically at login
  (ssh `RemoteCommand` + `-t`). By default it runs and then drops you into a
  normal interactive shell (e.g. `cd /srv && git status`); start the command
  with `exec ` to take over the session yourself.
- **Copy connection string (`Y`).** Press `Y` on a host to copy its
  `user@host` string to the clipboard (`pbcopy` / `wl-copy` / `xclip` /
  `xsel`).
- **Pause background work during SSH sessions.** New Settings toggle, on by
  default: while you're in a foreground SSH session, host health probes and
  Kluster discovery pause and then resume automatically on return.

### Changed

- **Lazy Kluster discovery.** The Kluster background worker no longer polls
  `docker` / `kubectl` / `incus` / `container` until the Kluster tab is opened
  at least once. A session that only ever connects over SSH pays nothing for
  container/cluster discovery.
- **Kluster layout.** All `(local)` sections (Docker, Apple, Incus) are now
  grouped at the top of the list, above every remote and cluster.

### Fixed

- Detail-view scrolling now reaches the true bottom cleanly, with a little
  breathing room at the end so the end of the popup is obvious.

### Upgrade notes

Fully backward-compatible — no config migration needed.

- `settings.toml` gains `pause_health_on_session = true` (written on next save).
- Host entries gain an optional `remote_command` field.

## [1.4.3] - 2026-05-22

Background tunnels, desktop notifications, and Linux/macOS client integration.

## [1.3.0] - 2026-05-03

## [1.2.0] - 2026-04-14

## [1.1.0] - 2026-03-07

## [1.0.3] - 2026-03-05

[1.5.0]: https://github.com/Sn0wAlice/sshm/compare/v1.4.3...v1.5.0
[1.4.3]: https://github.com/Sn0wAlice/sshm/compare/v1.3.0...v1.4.3
[1.3.0]: https://github.com/Sn0wAlice/sshm/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/Sn0wAlice/sshm/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/Sn0wAlice/sshm/compare/v1.0.3...v1.1.0
[1.0.3]: https://github.com/Sn0wAlice/sshm/releases/tag/v1.0.3
