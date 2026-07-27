# Refactor notes — workspace split + frontend-agnostic core

Running log of what moved where and why, so the restructure is reviewable.

## Phase 1 — Cargo workspace + `sshm-core`

### New layout

```
Cargo.toml                     # workspace root: [workspace.package] + [workspace.dependencies]
crates/
├── sshm-core/                 # engine — no ratatui/crossterm/inquire in its dep tree
│   └── src/{lib,models,history,os,tty,i18n}.rs
│       config/  ssh/  import/  filter/  kluster/  locales/
└── sshm/                      # the TUI + CLI (binary `sshm`), depends on sshm-core
    └── src/{main,lib,util}.rs
        ssh/  commands/  tui/
```

The published binary is unchanged: package `sshm` with `[[bin]] name = "sshm"`, so
`cargo build --release` still emits `target/release/sshm`. `debian/rules` (which
installs `target/release/sshm`) and `.github/workflows/release.yml` (which runs
`cargo build --release`) needed no path changes.

### What moved into `sshm-core` (verbatim unless noted)

`models`, `history`, `os`, `i18n` + `locales/`, `config/` (path, io, settings,
export), `import/` (ssh_config parser), `filter/` (matcher), `kluster/`
(models, shell, db, docker, apple, kube, incus), and the pure parts of `ssh/`
(client's `build_ssh_argv`/`launch_ssh`, keys, agent, known_hosts, proxy).

### What stayed in `sshm` (frontend)

All of `tui/`, `commands/`, `main.rs`, `util.rs` (uses crossterm for
`clear_console`), plus the interactive ssh flows — see below.

### Key decisions

1. **`inquire` cannot enter core.** `inquire` depends transitively on
   `crossterm 0.25`, which would put crossterm in core's dependency tree and
   break the hard "no ratatui/crossterm in core" constraint. Two core-bound
   files used `inquire`:
   - `ssh/client.rs::launch_ssh_with_recovery` + `tofu_gate` (host-key-change
     recovery + trust-on-first-use prompts) → **moved to** `sshm/src/ssh/client.rs`.
     Core keeps the pure `build_ssh_argv` / `launch_ssh`; the frontend re-exports
     those and layers the interactive flow on top.
   - `ssh/add_identity.rs` (the `add-identity` command, an `inquire::Select`
     picker) → **moved to** `sshm/src/ssh/add_identity.rs` unchanged.

2. **TTY handover is a frontend concern, injected via a hook.** Five core files
   (`ssh/client.rs`, `kluster/{docker,kube,incus,apple}.rs`) previously did
   `disable_raw_mode()` + show-cursor inline right before handing the TTY to a
   child process. That is the only thing they used crossterm for. Introduced
   `sshm_core::tty` with `set_release_hook` / `release_terminal`; core now calls
   `crate::tty::release_terminal()` at those points. The frontend registers the
   crossterm implementation once at startup
   (`sshm::util::register_tty_release_hook()`, called first thing in `main`).
   On a cooked CLI terminal (no hook path exercised in raw mode) this is a
   no-op, exactly like `disable_raw_mode()` was before.

3. **Re-export shim keeps churn near-zero.** `sshm/src/lib.rs` re-exports the
   core modules at the crate root (`pub use sshm_core::{models, config, ssh…}`)
   plus the `t!` macro, so existing `crate::models`, `crate::kluster`,
   `crate::t`, … paths throughout the TUI resolve unchanged. `crate::ssh` in the
   frontend is a small facade module that re-exports core's
   `keys/agent/known_hosts/proxy` and core's `build_ssh_argv/launch_ssh`, and
   defines the interactive `launch_ssh_with_recovery`. No TUI call sites changed.

4. **`os.rs` went to core.** It is frontend-agnostic (notifications, clipboard,
   `open_url`, external-terminal detection/launch — all plain process spawning,
   no crossterm). Phase 3 wants the terminal detection in core anyway.

5. **Pre-existing clippy lints.** CI only gated `cargo build --release`, never
   clippy, so the tree carried latent lints. To land the phase clippy-clean
   under `-D warnings` these were fixed in place (derive `Default`, needless
   `as_deref`, `saturating_sub`, struct-update init, `vec!`→array in tests,
   `new_without_default`). Two `items_after_test_module` cases would have meant
   relocating whole test modules inside otherwise-verbatim files, so they carry
   a scoped `#[allow]` with a justification comment instead.

6. **`cargo fmt` deliberately not run tree-wide.** The pre-existing code is not
   rustfmt-formatted (compact single-line `if`s, custom alignment); a global
   `cargo fmt` would rewrite ~5.3k lines across nearly every file and bury the
   split. New files were written in the surrounding style. Adopting rustfmt is a
   separate decision for the maintainer.

### Acceptance (Phase 1)

- `cargo build --release` → `target/release/sshm` (unchanged name/path). ✅
- `cargo test --workspace` → 81 passed (73 in core, 8 TUI), 0 failed. ✅
- `cargo clippy --workspace --all-targets -- -D warnings` → clean. ✅
- `cargo tree -p sshm-core` → no ratatui / crossterm / inquire. ✅
- On-disk formats (`host.json`, `kluster.json`, `settings.toml`) untouched;
  `sshm list` reads the existing DB and renders identically. ✅

## Phase 2 — concurrent-safe DB layer

Both frontends may now run against the same `~/.config/sshm/` files at once, so
writes must never corrupt and each side should notice the other's changes.

### 1. One atomic-write choke point

`config::io::atomic_write(path, bytes)` — write to a uniquely-named dotfile temp
(`.<name>.tmp-<pid>-<seq>`) in the **same directory**, `fsync` it, `rename` over
the target (atomic on one filesystem), then best-effort `fsync` the directory.
A crash or a racing writer can only ever leave a stray `*.tmp-*` behind — never
a truncated or half-written target. All three writers now route through it:
`try_save_db` (host.json), `kluster::db::save` (kluster.json),
`try_save_settings` (settings.toml). The previous per-writer
`write→remove→rename` (fixed temp name, no fsync, a window with no file) is gone.

### 2. Change detection — `core::watch`

`ConfigWatcher` wraps the `notify` crate over the config dir and forwards one
`DbChanged` (`Hosts` / `Kluster` / `Settings`) per touched file on a plain
`std::sync::mpsc` channel — no async runtime, usable from either frontend.
Temp/backup files are filtered out by name. A dedicated debounce thread coalesces
bursts (editors and our own temp+rename each fire several raw events): it blocks
for the first event, then drains until the dir is quiet for 150 ms, emitting one
event per distinct file. `notify` pulls `fsevent-sys` (macOS) / `inotify`
(Linux) only — **no crossterm** enters core's tree.

### 3. `Database::reload_if_changed()`

mtime+length signature (`#[serde(skip)]`, in-memory only — on-disk JSON
unchanged) short-circuits the common case with a single `stat`. When the
signature moves, the file is compared against our own canonical
`serialize_db(self)`: a byte-identical write (our own save, or a matching
external one) reports `Ok(false)` — no spurious reload — while a real change
swaps `hosts`/`folders` in place and reports `Ok(true)`. Comparing the canonical
form also neutralises in-memory folder ordering (writes sort+dedup folders).
`reload_if_changed_from(path)` is the path-injectable seam the tests drive.

### 4. TUI wiring

`run_tui` starts a `ConfigWatcher`, syncs once on entry (catches changes made
while away in an ssh session), and at the top of each loop drains the watcher
into a `pending_reload` flag. When a host change is pending **and no overlay
modal is open** (delete-confirm / help / tunnels dashboard / kluster detail) it
reloads in place — rebuilding the `db`-borrowing `items`/`filtered` exactly like
the CRUD paths — and shows the existing toast (`toast.hosts_reloaded`, added to
en/fr). Editor forms (`run_host_form` / `run_folder_form`) run their own blocking
loop, so "don't reload while a form with unsaved changes is open" is satisfied
for free: the watcher simply isn't polled until the form returns, and its events
stay buffered until then.

Last-writer-wins; no merge engine (per spec).

### Tests (Phase 2, all in core)

- `atomic_write_replaces_content_and_leaves_no_temp`
- `leftover_temp_from_interrupted_write_is_ignored_on_load`
- `concurrent_writers_never_corrupt_the_file` (8 threads × 50 writes → file is
  always exactly one whole payload)
- `reload_if_changed_detects_external_modification`
- `reload_if_changed_swallows_identical_content_rewrite`
- `watch`: classify (only the 3 db files; temp/bak ignored), debounce collapses
  a burst to one event, keeps distinct files separate, closes cleanly
- `end_to_end_detects_a_real_write` (`#[ignore]`d — real notify+FS+timing; passes
  locally, kept out of the default run to avoid CI flakiness)

### Acceptance (Phase 2)

- `cargo test --workspace` → 91 passed (83 core, 8 TUI), 0 failed. ✅
- `cargo clippy --workspace --all-targets -- -D warnings` → clean. ✅
- `cargo tree -p sshm-core` → still no ratatui/crossterm/inquire (notify →
  fsevent-sys/inotify only). ✅
- On-disk formats unchanged (skip field isn't serialized; writers emit the same
  canonical bytes). ✅

## Phase 3 — `sshm-gui` (Tauri 2 desktop app, launcher mode)

New crate `crates/sshm-gui` — a Tauri 2 desktop app (binary `sshm-desktop`,
`publish = false`), Svelte + TypeScript (Vite) frontend, sharing the exact same
on-disk DB as the TUI.

### Layout

```
crates/sshm-gui/
├── package.json, vite.config.ts, svelte.config.js, tsconfig*.json, index.html
├── src/                      # Svelte frontend
│   ├── App.svelte, main.ts, app.css
│   └── lib/{bindings.ts (generated), ipc.ts, stores.ts, components/*.svelte}
└── src-tauri/                # Rust backend (the workspace member)
    ├── Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/
    └── src/{main,lib,commands,dto,events,state,tunnels_mgr}.rs
```

`default-members` in the workspace root is `[sshm-core, sshm]`, so `cargo build
--release` (debian/rules, release.yml) still builds only the TUI. The GUI is
built with `-p sshm-desktop` or the Tauri CLI.

### Typed IPC (no hand-written TS)

- `sshm-core` gained an **optional, off-by-default `specta` feature** that adds
  `#[cfg_attr(feature = "specta", derive(specta::Type))]` to the IPC-facing
  structs (`Host`, `Tunnel`, `TunnelKind`, `AppConfig`, `Database`, the kluster
  types, `TunnelRecord`). Default core builds (and `cargo tree -p sshm-core`)
  stay specta-free — the constraint that no terminal-UI/heavy dep leaks into the
  default tree is preserved; specta only appears when the GUI enables it.
- The GUI enables `sshm-core/specta` and uses `tauri-specta` +
  `specta-typescript` to generate `src/lib/bindings.ts` from the command
  signatures (pinned: specta `=2.0.0-rc.22`, tauri-specta `=2.0.0-rc.21`,
  specta-typescript `=0.0.9`). Regenerated on every `tauri dev`, or headlessly by
  the `export_bindings` test — that test *is* how the committed bindings are
  produced, so CI can diff them. `u64` settings map to JS `number`; the file
  carries a `// @ts-nocheck` header so its own generated imports don't trip
  `noUnusedLocals` while components importing from it stay fully type-checked.

### Commands (all thin wrappers over core)

Hosts/folders (list w/ core filter, get, save, delete, clone, folder CRUD),
connect (via `os::open_in_terminal` — external terminal, never embedded),
settings (get/save), identities (`scan_ssh_dir`, generate, agent add/remove,
push pubkey), kluster (overview + docker/pods/incus list, docker/incus
lifecycle, shell/logs via external terminal), tunnels (list across instances,
per-host saved tunnels, start/stop). Mutations go through core so `host.json`
stays canonical and `~/.ssh/config` auto-export keeps working.

### Shared tunnel substrate

New `sshm_core::tunnels`: `build_forward_arg` / `build_tunnel_argv` (the
`ssh -N` argv), and `TunnelRecord` + `read_all_records` — the shared
`~/.config/sshm/tunnels/<pid>.json` format. The GUI's `GuiTunnels` spawns/kills
its own children and writes this format; the TUI's existing manager writes the
same field shape, so both dashboards interoperate. (The TUI's manager was left
untouched to avoid regression; it and `GuiTunnels` share the on-disk contract,
not the struct — a future cleanup could unify them.)

### Live sync

The backend spawns a thread that blocks on `sshm_core::watch::ConfigWatcher` and
emits a typed `DbChangedEvent` for each debounced change; `App.svelte` listens
and refreshes the host list — so a host added from the TUI appears in the app
without a restart.

### Security

Strict Tauri capability: `core:default` only — no shell/opener/fs plugins. Every
privileged action (spawning ssh, opening a terminal, reading `~/.ssh`) runs in
the Rust backend behind an explicit command, never via a frontend-reachable
plugin. CSP enabled (`default-src 'self'`, no remote origins). The GUI never
reads or writes private-key contents — only paths, like the TUI.

### CI (`.github/workflows/ci.yml`)

- `rust` job: `cargo test --workspace --exclude sshm-desktop` + clippy `-D
  warnings` on core+TUI.
- `gui` job (ubuntu + macos): `npm ci && npm run check` (svelte-check) and
  `cargo check -p sshm-desktop` (Linux installs the webkit2gtk deps).

### Acceptance (Phase 3)

- `cargo check -p sshm-desktop` → clean; clippy on the crate → clean. ✅
- `npm run check` (svelte-check) → 0 errors / 0 warnings; `npm run build` →
  produces `dist/`. ✅
- Bindings generated from core structs (no hand-written duplicate TS). ✅
- Default `cargo build --release` unaffected (GUI excluded); core tree still free
  of ratatui/crossterm/inquire/specta by default. ✅
- Shared DB: GUI reads/writes the same `host.json`/`kluster.json`/`settings.toml`
  and live-reloads on external change. ✅

### Phase 4 (embedded terminal) — not started

Left as the optional follow-up per the plan (portable-pty + xterm.js), with the
external-terminal button as the always-available fallback.
