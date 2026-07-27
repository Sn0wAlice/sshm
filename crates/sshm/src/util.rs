use std::io::stdout;
use crossterm::{cursor::Show, execute, terminal::{disable_raw_mode, Clear, ClearType}};

pub fn clear_console() {
    let _ = execute!(stdout(), Clear(ClearType::All));
}

/// Register how the terminal is restored before core hands the TTY to a child
/// process (`ssh`, `docker exec`, `kubectl exec`, …). Core calls this hook via
/// `sshm_core::tty::release_terminal`; keeping the crossterm bits here is what
/// lets core stay free of any terminal-UI dependency. Call once at startup.
pub fn register_tty_release_hook() {
    sshm_core::tty::set_release_hook(|| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), Show);
    });
}
