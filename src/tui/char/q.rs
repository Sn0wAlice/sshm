use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use std::{process, io::stdout};

use crossterm::{cursor::Show, terminal::LeaveAlternateScreen};
pub fn press() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    process::exit(0);
}
