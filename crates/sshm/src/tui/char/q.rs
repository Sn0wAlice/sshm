use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use std::{process, io::stdout};

use crossterm::{cursor::Show, terminal::LeaveAlternateScreen};
pub fn press() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen, Show);
    // process::exit skips destructors — SIGTERM background tunnels first so
    // they don't leak as orphaned ssh processes.
    crate::tui::app::tunnels::kill_all();
    process::exit(0);
}
