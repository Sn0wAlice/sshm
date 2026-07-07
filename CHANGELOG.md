# Changelog

All notable changes to **sshm** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
