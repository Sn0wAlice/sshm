use std::io::stdout;
use crossterm::{execute, terminal::{Clear, ClearType}};

pub fn clear_console() {
    let _ = execute!(stdout(), Clear(ClearType::All));
}
