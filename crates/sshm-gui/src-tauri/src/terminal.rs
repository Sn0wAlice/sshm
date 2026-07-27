//! Embedded-terminal sessions: spawn a command inside a PTY and stream it to
//! the webview (xterm.js). Output is pushed as [`TermOutputEvent`]s; input/
//! resize/close come back as commands. Used for ssh sessions, a local shell,
//! and kluster (docker/kube/incus) shell & logs — all in-app.

use std::collections::HashMap;
use std::io::Read;

use tauri::{AppHandle, State};
use tauri_specta::Event;

use sshm_core::pty::PtySession;
use sshm_core::ssh::client::build_ssh_argv;

use crate::events::{TermExitEvent, TermOutputEvent};
use crate::state::AppState;

/// Initial PTY size; the widget resizes it to the real dimensions on mount.
const INIT_COLS: u16 = 80;
const INIT_ROWS: u16 = 24;

/// Live embedded sessions, keyed by a monotonic id.
#[derive(Default)]
pub struct Sessions {
    map: HashMap<String, PtySession>,
    next: u64,
}

impl Sessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Kill and drop every session (call on window close).
    pub fn shutdown(&mut self) {
        for (_, mut s) in self.map.drain() {
            s.kill();
        }
    }
}

/// Spawn `argv` in a PTY, register it, start its output pump, and return the
/// new session id. Shared by every embedded-terminal entry point.
pub(crate) fn spawn_session(
    app: &AppHandle,
    state: &AppState,
    argv: Vec<String>,
) -> Result<String, String> {
    let session =
        PtySession::spawn(&argv, INIT_COLS, INIT_ROWS).map_err(|e| format!("{e:#}"))?;
    let reader = session.output_reader().map_err(|e| format!("{e:#}"))?;

    let id = {
        let mut sessions = state.sessions.lock().unwrap();
        sessions.next += 1;
        let id = format!("s{}", sessions.next);
        sessions.map.insert(id.clone(), session);
        id
    };

    let handle = app.clone();
    let sid = id.clone();
    std::thread::Builder::new()
        .name(format!("pty-{sid}"))
        .spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let _ = TermOutputEvent { id: sid.clone(), data: buf[..n].to_vec() }
                            .emit(&handle);
                    }
                }
            }
            let _ = TermExitEvent { id: sid.clone() }.emit(&handle);
        })
        .map_err(|e| e.to_string())?;

    Ok(id)
}

/// Open an embedded ssh session to `host_name`. Returns the session id; output
/// then arrives as `TermOutputEvent`s carrying that id. Mosh is rejected (it
/// stays external-terminal only).
#[tauri::command]
#[specta::specta]
pub fn term_open(app: AppHandle, state: State<AppState>, host_name: String) -> Result<String, String> {
    let argv = {
        let db = state.db.lock().unwrap();
        let host = db
            .hosts
            .get(&host_name)
            .ok_or_else(|| format!("host '{host_name}' not found"))?;
        if host.mosh {
            return Err("mosh sessions open in an external terminal, not embedded".into());
        }
        build_ssh_argv(host, &db.hosts)
    };
    spawn_session(&app, &state, argv)
}

/// Open a local shell (the user's `$SHELL`, else `/bin/sh`) in the app.
#[tauri::command]
#[specta::specta]
pub fn term_open_local(app: AppHandle, state: State<AppState>) -> Result<String, String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".to_string());
    spawn_session(&app, &state, vec![shell])
}

#[tauri::command]
#[specta::specta]
pub fn term_write(state: State<AppState>, id: String, data: Vec<u8>) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    match sessions.map.get_mut(&id) {
        Some(s) => s.write_input(&data).map_err(|e| e.to_string()),
        None => Err(format!("no session {id}")),
    }
}

#[tauri::command]
#[specta::specta]
pub fn term_resize(state: State<AppState>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    match sessions.map.get(&id) {
        Some(s) => s.resize(cols.max(2), rows.max(2)).map_err(|e| format!("{e:#}")),
        None => Err(format!("no session {id}")),
    }
}

#[tauri::command]
#[specta::specta]
pub fn term_close(state: State<AppState>, id: String) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(mut s) = sessions.map.remove(&id) {
        s.kill();
    }
    Ok(())
}
