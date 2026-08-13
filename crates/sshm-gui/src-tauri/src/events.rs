//! Typed events pushed from the backend to the webview.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

/// Emitted when a shared config file changes on disk (from this app, the TUI,
/// another GUI instance, or a text editor). The frontend refreshes whichever
/// view the flags indicate. Backed by `sshm_core::watch`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct DbChangedEvent {
    pub hosts: bool,
    pub kluster: bool,
    pub settings: bool,
}

impl From<sshm_core::watch::DbChanged> for DbChangedEvent {
    fn from(c: sshm_core::watch::DbChanged) -> Self {
        use sshm_core::watch::DbChanged;
        DbChangedEvent {
            hosts: matches!(c, DbChanged::Hosts),
            kluster: matches!(c, DbChanged::Kluster),
            settings: matches!(c, DbChanged::Settings),
        }
    }
}

/// Emitted once the real `PATH` has been recovered from the login shell (done
/// off the main thread so the window opens instantly). Tools like `docker` /
/// `kubectl` / `incus` / `ssh` are only resolvable after this fires, so the
/// frontend defers Kluster discovery until then.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct PathReadyEvent;

/// A chunk of output bytes from an embedded terminal session `id`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct TermOutputEvent {
    pub id: String,
    pub data: Vec<u8>,
}

/// The embedded terminal session `id`'s child process exited.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct TermExitEvent {
    pub id: String,
}
