use std::process::Command;
use crate::models::Host;

/// Construit et exécute la commande ssh en combinant Host + overrides CLI.
pub fn launch_ssh(h: &Host, overrides: Option<&[String]>) {
    use std::io::stdout;
    use crossterm::{terminal::disable_raw_mode, cursor::Show, execute};

    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);

    let mut cmd = Command::new("ssh");
    cmd.arg(format!("{}@{}", h.username, h.host))
        .arg("-p")
        .arg(h.port.to_string());

    if let Some(id) = &h.identity_file {
        cmd.arg("-i").arg(id);
    }
    if let Some(j) = &h.proxy_jump {
        cmd.arg("-J").arg(j);
    }
    if let Some(args) = overrides {
        cmd.args(args);
    }
    let _ = cmd.status();
}
