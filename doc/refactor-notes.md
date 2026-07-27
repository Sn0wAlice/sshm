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
