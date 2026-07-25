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
///
/// Renvoie le statut de sortie du processus (`None` si le binaire n'a pas pu
/// être lancé), pour que l'appelant puisse réagir à un échec — par ex. proposer
/// de nettoyer `known_hosts` quand la clé de l'hôte a changé.
pub fn launch_ssh(
    h: &Host,
    all_hosts: &HashMap<String, Host>,
    overrides: Option<&[String]>,
) -> Option<std::process::ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);

    let argv = build_ssh_argv(h, all_hosts);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    if let Some(args) = overrides {
        cmd.args(args);
    }
    match cmd.status() {
        Ok(status) => Some(status),
        Err(_) => {
            if h.mosh {
                eprintln!("sshm: failed to launch `mosh` — is it installed and on PATH?");
            }
            None
        }
    }
}

/// Lance la connexion puis, si `ssh` a échoué parce que la clé de l'hôte a
/// changé, propose de purger l'entrée `known_hosts` obsolète et de se
/// reconnecter dans la foulée.
///
/// À appeler pendant que le terminal normal est actif (raw mode désactivé,
/// écran alternatif quitté) : le bandeau d'avertissement d'OpenSSH reste alors
/// visible juste au-dessus de l'invite de confirmation.
pub fn launch_ssh_with_recovery(
    h: &Host,
    all_hosts: &HashMap<String, Host>,
    overrides: Option<&[String]>,
) {
    // Trust-on-first-use: for a host we've never pinned, show its live
    // fingerprint and let the user vet it here rather than blindly answering
    // ssh's raw "(yes/no)?" prompt. Declining aborts the connection.
    if !tofu_gate(h) {
        return;
    }

    let status = launch_ssh(h, all_hosts, overrides);

    // Only bother probing when ssh actually exited non-zero. A changed host key
    // makes ssh bail with status 255 before any shell starts.
    let failed = matches!(status, Some(s) if !s.success());
    if !failed {
        return;
    }
    if !crate::ssh::known_hosts::host_key_changed(&h.host, h.port) {
        return;
    }

    println!();
    let confirmed = inquire::Confirm::new(&format!(
        "⚠  The host key for {} has changed. Remove the stale key from known_hosts and reconnect?",
        h.host
    ))
    .with_default(false)
    .prompt()
    .unwrap_or(false);
    if !confirmed {
        return;
    }

    match crate::ssh::known_hosts::remove_known_host_port(&h.host, h.port) {
        Ok(()) => {
            let _ = launch_ssh(h, all_hosts, overrides);
        }
        Err(e) => eprintln!("sshm: failed to clean known_hosts for {}: {e}", h.host),
    }
}

/// Trust-on-first-use gate, run just before the first connection to a host.
///
/// Returns `true` to proceed with the connection, `false` to abort. Pinned
/// hosts (a local `known_hosts` lookup, no network) sail straight through. For
/// an unpinned, reachable host it prints the live fingerprint and asks the user
/// to trust it: yes pins the key and connects, no aborts. If the key can't be
/// fetched (host down, `ssh-keyscan` missing) it proceeds and lets `ssh` handle
/// verification itself, so this never blocks a connection it couldn't vet.
fn tofu_gate(h: &Host) -> bool {
    use crate::ssh::known_hosts;

    if known_hosts::is_pinned(&h.host, h.port) {
        return true;
    }

    let keys = known_hosts::scan_fingerprints(&h.host, h.port);
    if keys.is_empty() {
        // Couldn't reach / fingerprint the host — don't stand in ssh's way.
        return true;
    }

    println!();
    println!(
        "The authenticity of host '{}' (port {}) can't be established — it isn't in known_hosts yet.",
        h.host, h.port
    );
    for k in &keys {
        println!("  {k}");
    }
    let trust = inquire::Confirm::new("Trust this host key and connect?")
        .with_default(false)
        .prompt()
        .unwrap_or(false);
    if trust {
        if let Err(e) = known_hosts::pin_host(&h.host, h.port) {
            eprintln!("sshm: could not pin host key: {e}");
        }
    }
    trust
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
