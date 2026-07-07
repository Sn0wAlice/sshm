use std::collections::HashMap;
use std::io::stdout;
use std::process::Command;
use crossterm::{terminal::disable_raw_mode, cursor::Show, execute};
use crate::models::Host;
use crate::ssh::proxy::resolve_proxy_jump;

/// Build the connection command for `h` as an argv vector — `ssh …` normally,
/// or `mosh --ssh="ssh …" …` when `h.mosh` is set. `argv[0]` is the program.
///
/// `all_hosts` resolves multi-hop `proxy_jump` entries that name saved hosts.
pub fn build_ssh_argv(h: &Host, all_hosts: &HashMap<String, Host>) -> Vec<String> {
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

    if h.mosh {
        // mosh drives ssh internally for the handshake; pass our flags via --ssh.
        let inner = std::iter::once("ssh".to_string())
            .chain(ssh_opts.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        vec!["mosh".to_string(), format!("--ssh={}", inner), target]
    } else {
        let mut argv = vec!["ssh".to_string(), target];
        argv.extend(ssh_opts);
        // Run-on-connect: force a TTY and hand ssh a RemoteCommand. ssh's
        // RemoteCommand *replaces* the login shell, so a bare command (e.g.
        // `echo ok`) would run and immediately disconnect. To match the
        // intuitive "run this, then give me a shell" expectation we append
        // `; exec $SHELL -l` by default. A user who wants to manage the shell
        // lifecycle themselves — including a deliberate one-shot that exits —
        // signals it by writing their own `exec ` in the command.
        if let Some(cmd) = &h.remote_command {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                let full = if cmd.contains("exec ") {
                    cmd.to_string()
                } else {
                    format!("{cmd}; exec ${{SHELL:-/bin/sh}} -l")
                };
                argv.push("-t".to_string());
                argv.push("-o".to_string());
                argv.push(format!("RemoteCommand={}", full));
            }
        }
        argv
    }
}

/// Construit et exécute la commande de connexion en combinant Host + overrides CLI.
///
/// Utilise `ssh` par défaut, ou `mosh` quand `h.mosh` est activé.
///
/// `all_hosts` est utilisé pour résoudre une chaîne `proxy_jump` multi-hop
/// dont les entrées peuvent être des noms d'hôtes sauvegardés.
pub fn launch_ssh(h: &Host, all_hosts: &HashMap<String, Host>, overrides: Option<&[String]>) {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);

    let argv = build_ssh_argv(h, all_hosts);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    if let Some(args) = overrides {
        cmd.args(args);
    }
    if cmd.status().is_err() && h.mosh {
        eprintln!("sshm: failed to launch `mosh` — is it installed and on PATH?");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Host;

    fn mk_host() -> Host {
        Host {
            name: "web".to_string(),
            host: "10.0.0.5".to_string(),
            port: 22,
            username: "root".to_string(),
            identity_file: None,
            proxy_jump: None,
            tags: None,
            folder: None,
            last_connected_at: None,
            use_count: 0,
            favorite: false,
            tunnels: vec![],
            forward_agent: false,
            mosh: false,
            notes: None,
            remote_command: None,
        }
    }

    #[test]
    fn no_remote_command_is_plain_ssh() {
        let h = mk_host();
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert_eq!(argv, vec!["ssh", "root@10.0.0.5", "-p", "22"]);
    }

    #[test]
    fn bare_remote_command_appends_interactive_shell() {
        let mut h = mk_host();
        h.remote_command = Some("echo ok".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert_eq!(
            argv,
            vec![
                "ssh",
                "root@10.0.0.5",
                "-p",
                "22",
                "-t",
                "-o",
                "RemoteCommand=echo ok; exec ${SHELL:-/bin/sh} -l",
            ]
        );
    }

    #[test]
    fn remote_command_with_exec_is_verbatim() {
        let mut h = mk_host();
        h.remote_command = Some("exec tail -f /var/log/syslog".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        // User manages the shell lifecycle — no auto-appended exec.
        assert_eq!(argv.last().unwrap(), "RemoteCommand=exec tail -f /var/log/syslog");
    }

    #[test]
    fn blank_remote_command_is_ignored() {
        let mut h = mk_host();
        h.remote_command = Some("   ".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert!(!argv.iter().any(|a| a == "-t" || a.starts_with("RemoteCommand=")));
    }

    #[test]
    fn mosh_ignores_remote_command() {
        let mut h = mk_host();
        h.mosh = true;
        h.remote_command = Some("uptime".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert_eq!(argv[0], "mosh");
        assert!(!argv.iter().any(|a| a.starts_with("RemoteCommand=")));
    }
}
