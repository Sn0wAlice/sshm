//! sshm desktop GUI backend (Tauri 2).
//!
//! Launcher mode: manage the same `~/.config/sshm/` database as the TUI, open
//! ssh sessions in an external terminal, run background tunnels, and drive the
//! Kluster/Identities/Settings features — all through `sshm_core`. IPC types
//! are derived from the core structs via `tauri-specta` (see `make_builder`),
//! so the TypeScript bindings never drift from Rust.

mod commands;
mod dto;
mod events;
mod state;
mod tunnels_mgr;

use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder};

use events::DbChangedEvent;
use state::AppState;

/// TypeScript exporter config. `u64` settings fields (timeouts in seconds/ms)
/// map to JS `number` rather than `bigint` — the values are tiny and far below
/// `Number.MAX_SAFE_INTEGER`, and `bigint` would be awkward in the UI.
fn ts_exporter() -> specta_typescript::Typescript {
    specta_typescript::Typescript::default()
        .bigint(specta_typescript::BigIntExportBehavior::Number)
        // The generated file is not linted by us; `@ts-nocheck` silences the
        // unused-import noise (e.g. TAURI_CHANNEL) under `noUnusedLocals`
        // without weakening type-checking of the components that import it.
        .header("// @ts-nocheck\n")
}

/// Build the tauri-specta [`Builder`] (all commands + events). Shared by
/// [`run`] and the bindings-export test so the two never diverge.
pub fn make_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::list_hosts,
            commands::list_folders,
            commands::get_host,
            commands::save_host,
            commands::delete_host,
            commands::clone_host,
            commands::create_folder,
            commands::rename_folder,
            commands::delete_folder,
            commands::connect_host,
            commands::get_settings,
            commands::save_settings,
            commands::list_identities,
            commands::generate_identity,
            commands::agent_add_identity,
            commands::agent_remove_identity,
            commands::push_pubkey,
            commands::kluster_overview,
            commands::kluster_docker_containers,
            commands::kluster_pods,
            commands::kluster_incus,
            commands::kluster_docker_lifecycle,
            commands::kluster_incus_lifecycle,
            commands::kluster_docker_shell,
            commands::kluster_docker_logs,
            commands::kluster_pod_shell,
            commands::kluster_incus_shell,
            commands::list_tunnels,
            commands::host_tunnels,
            commands::start_tunnel,
            commands::stop_tunnel,
            commands::reload_db,
        ])
        .events(collect_events![DbChangedEvent])
}

/// Run the desktop app.
pub fn run() {
    let builder = make_builder();

    // In dev, regenerate the TS bindings on every launch so the frontend and
    // backend can't drift. (No-op in release.)
    #[cfg(debug_assertions)]
    builder
        .export(
            ts_exporter(),
            "../src/lib/bindings.ts",
        )
        .expect("failed to export typescript bindings");

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            spawn_watcher(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    if let Ok(mut t) = state.tunnels.lock() {
                        t.shutdown();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running sshm desktop");
}

/// Bridge `sshm_core::watch` to a typed Tauri event: block on the config-dir
/// watcher and emit [`DbChangedEvent`] for each debounced change, so the
/// frontend live-updates when the TUI (or anything else) edits the files.
fn spawn_watcher(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let watcher = match sshm_core::watch::ConfigWatcher::start() {
            Ok(w) => w,
            Err(_) => return,
        };
        while let Ok(change) = watcher.receiver().recv() {
            use tauri_specta::Event;
            let _ = DbChangedEvent::from(change).emit(&handle);
        }
    });
}

#[cfg(test)]
mod tests {
    /// Regenerate the TypeScript bindings headlessly (no window needed). This is
    /// how CI/committed `src/lib/bindings.ts` is produced:
    /// `cargo test -p sshm-desktop export_bindings`.
    #[test]
    fn export_bindings() {
        crate::make_builder()
            .export(crate::ts_exporter(), "../src/lib/bindings.ts")
            .expect("export bindings");
    }
}
