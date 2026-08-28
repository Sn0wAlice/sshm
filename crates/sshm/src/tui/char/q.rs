use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use std::{process, io::stdout};

use crossterm::{cursor::Show, terminal::LeaveAlternateScreen};
pub fn press() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen, Show);
    // Runs on the normal terminal, after the alternate screen is gone, so the
    // user can see what it is waiting on.
    sync_on_exit();
    // process::exit skips destructors — SIGTERM background tunnels first so
    // they don't leak as orphaned ssh processes.
    crate::tui::app::tunnels::kill_all();
    process::exit(0);
}

/// Publish this session's config edits when "sync on exit" is enabled.
///
/// Settings are re-read from disk rather than threaded down here: this is the
/// single quit path, and one small file read at exit is cheaper than passing
/// the config through every caller.
fn sync_on_exit() {
    let cfg = crate::config::settings::load_settings().sync;
    if !cfg.on_exit || !cfg.is_active() {
        return;
    }
    println!("Syncing config…");
    match sshm_core::sync::sync_now(&cfg) {
        Ok(run) => println!("Sync: {}", run.summary()),
        // Never block quitting on a sync failure — say what happened and go.
        Err(e) => eprintln!("Sync failed: {e:#}"),
    }
}
