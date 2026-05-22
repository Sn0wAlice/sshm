use std::collections::HashMap;
use std::io::stdout;
use std::process::Command;
use crossterm::{terminal::disable_raw_mode, cursor::Show, execute};
use crate::models::Host;
use crate::ssh::proxy::resolve_proxy_jump;

/// Construit et exécute la commande de connexion en combinant Host + overrides CLI.
///
/// Utilise `ssh` par défaut, ou `mosh` quand `h.mosh` est activé — auquel cas
/// les options ssh (port, identité, ProxyJump, ForwardAgent) sont transmises à
/// mosh via `--ssh`.
///
/// `all_hosts` est utilisé pour résoudre une chaîne `proxy_jump` multi-hop
/// dont les entrées peuvent être des noms d'hôtes sauvegardés.
pub fn launch_ssh(h: &Host, all_hosts: &HashMap<String, Host>, overrides: Option<&[String]>) {

    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);

    // SSH option flags shared by the `ssh` invocation and mosh's `--ssh`.
    let mut ssh_opts: Vec<String> = vec!["-p".to_string(), h.port.to_string()];
    if let Some(id) = &h.identity_file {
        if !id.is_empty() {
            ssh_opts.push("-i".to_string());
            ssh_opts.push(id.clone());
        }
    }
    if let Some(j) = &h.proxy_jump {
        if let Some(resolved) = resolve_proxy_jump(j, all_hosts) {
            ssh_opts.push("-J".to_string());
            ssh_opts.push(resolved);
        }
    }
    if h.forward_agent {
        ssh_opts.push("-A".to_string());
    }

    let target = format!("{}@{}", h.username, h.host);

    let mut cmd = if h.mosh {
        // mosh drives ssh internally for the handshake; pass our flags via --ssh.
        let mut inner = String::from("ssh");
        for opt in &ssh_opts {
            inner.push(' ');
            inner.push_str(opt);
        }
        let mut c = Command::new("mosh");
        c.arg(format!("--ssh={}", inner));
        c.arg(&target);
        c
    } else {
        let mut c = Command::new("ssh");
        c.arg(&target);
        c.args(&ssh_opts);
        c
    };

    if let Some(args) = overrides {
        cmd.args(args);
    }
    if cmd.status().is_err() && h.mosh {
        eprintln!("sshm: failed to launch `mosh` — is it installed and on PATH?");
    }
}
