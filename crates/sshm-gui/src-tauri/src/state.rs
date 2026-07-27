//! Shared, mutex-guarded backend state.

use std::sync::Mutex;

use sshm_core::models::Database;

use crate::tunnels_mgr::GuiTunnels;

/// The canonical on-disk DB is `host.json`; this holds the in-memory copy the
/// commands read/write, plus this app's live tunnel children. Commands call
/// `Database::reload_if_changed` before reads so edits made by the TUI (or
/// another GUI instance) are picked up.
pub struct AppState {
    pub db: Mutex<Database>,
    pub tunnels: Mutex<GuiTunnels>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            db: Mutex::new(sshm_core::config::io::load_db()),
            tunnels: Mutex::new(GuiTunnels::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
