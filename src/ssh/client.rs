use std::collections::HashMap;
use std::io::stdout;
use std::process::Command;
use crossterm::{terminal::disable_raw_mode, cursor::Show, execute};
use crate::models::Host;
use crate::ssh::proxy::resolve_proxy_jump;

/// Construit et exécute la commande ssh en combinant Host + overrides CLI.
///
/// `all_hosts` est utilisé pour résoudre une chaîne `proxy_jump` multi-hop
/// dont les entrées peuvent être des noms d'hôtes sauvegardés.
pub fn launch_ssh(h: &Host, all_hosts: &HashMap<String, Host>, overrides: Option<&[String]>) {

    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);

    let mut cmd = Command::new("ssh");
    cmd.arg(format!("{}@{}", h.username, h.host))
        .arg("-p")
        .arg(h.port.to_string());

    if let Some(id) = &h.identity_file {
        if !id.is_empty() {
            cmd.arg("-i").arg(id);
        }
    }
    if let Some(j) = &h.proxy_jump {
        if let Some(resolved) = resolve_proxy_jump(j, all_hosts) {
            cmd.arg("-J").arg(resolved);
        }
    }
    if h.forward_agent {
        cmd.arg("-A");
    }
    if let Some(args) = overrides {
        cmd.args(args);
    }
    let _ = cmd.status();
}
