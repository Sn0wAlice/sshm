use std::collections::HashMap;
use std::process::Command;
use crate::models::Host;
use crate::ssh::proxy::resolve_proxy_jump;

/// The ssh option flags that describe *how* to reach `h`: port, identity,
/// ProxyJump chain, agent forwarding, then the host's raw `ssh_options`.
///
/// This is the single place that turns a [`Host`] into ssh flags. Every caller
/// that spawns ssh for a host goes through it — the interactive connection
/// ([`build_ssh_argv`]), a background tunnel
/// ([`crate::tunnels::build_tunnel_argv`]) and a one-shot remote command
/// ([`build_exec_argv`]) — so a new per-host connection setting only has to be
/// handled here to apply everywhere.
///
/// The target (`user@host`) is deliberately *not* included: callers place it
/// at different positions in their argv.
pub fn build_ssh_opts(h: &Host, all_hosts: &HashMap<String, Host>) -> Vec<String> {
    let mut opts: Vec<String> = vec!["-p".to_string(), h.port.to_string()];
    if let Some(id) = &h.identity_file {
        if !id.is_empty() {
            opts.push("-i".to_string());
            opts.push(id.clone());
        }
    }
    if let Some(j) = &h.proxy_jump {
        if let Some(resolved) = resolve_proxy_jump(j, all_hosts) {
            opts.push("-J".to_string());
            opts.push(resolved);
        }
    }
    if h.forward_agent {
        opts.push("-A".to_string());
    }
    // Raw per-host escape hatch, last so it can override anything above.
    for raw in &h.ssh_options {
        let raw = raw.trim();
        if !raw.is_empty() {
            opts.push("-o".to_string());
            opts.push(raw.to_string());
        }
    }
    opts
}

/// Build the connection command for `h` as an argv vector — `ssh …` normally,
/// or `mosh --ssh="ssh …" …` when `h.mosh` is set. `argv[0]` is the program.
///
/// `all_hosts` resolves multi-hop `proxy_jump` entries that name saved hosts.
pub fn build_ssh_argv(h: &Host, all_hosts: &HashMap<String, Host>) -> Vec<String> {
    let ssh_opts = build_ssh_opts(h, all_hosts);
    let target = format!("{}@{}", h.username, h.host);

    if h.mosh {
        // mosh drives ssh internally for the handshake; our flags travel as one
        // `--ssh=` string that mosh splits on whitespace again. Anything with a
        // space in it — an identity path, a `SetEnv=MSG=hello world` — has to be
        // quoted here or it would arrive as two arguments.
        let inner = std::iter::once("ssh".to_string())
            .chain(ssh_opts.iter().cloned())
            .map(|a| crate::os::shell_quote(&a))
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

/// Argv for running `command` on `h` non-interactively (fan-out, scripts):
/// `ssh -o ConnectTimeout=… -o ServerAlive… <opts> user@host <command>`.
///
/// Two bounds keep one unreachable host from wedging a whole batch:
/// `ConnectTimeout` caps the handshake, and the `ServerAlive` pair makes ssh
/// give up after roughly `connect_timeout_secs` of silence on an established
/// connection — the case a plain `ConnectTimeout` does not cover. A command
/// that is genuinely still running is not interrupted.
///
/// Both come before the host's own `ssh_options`, so a host that sets
/// `ServerAliveInterval` itself still wins.
pub fn build_exec_argv(
    h: &Host,
    all_hosts: &HashMap<String, Host>,
    command: &str,
    connect_timeout_secs: u32,
) -> Vec<String> {
    let secs = connect_timeout_secs.max(1);
    let mut argv = vec!["ssh".to_string()];
    argv.push("-o".into());
    argv.push(format!("ConnectTimeout={secs}"));
    argv.push("-o".into());
    argv.push(format!("ServerAliveInterval={secs}"));
    argv.push("-o".into());
    argv.push("ServerAliveCountMax=1".into());
    argv.extend(build_ssh_opts(h, all_hosts));
    argv.push(format!("{}@{}", h.username, h.host));
    argv.push(command.to_string());
    argv
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
    // Restore the terminal (leave raw mode / show cursor) before ssh takes over
    // the TTY. The concrete restore lives in the frontend; see `crate::tty`.
    crate::tty::release_terminal();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Host;

    fn mk_host() -> Host {
        Host {
            name: "web".to_string(),
            host: "10.0.0.5".to_string(),
            ..Default::default()
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

    // ---- build_ssh_opts: the shared flag builder -------------------------

    #[test]
    fn opts_carry_port_identity_and_agent() {
        let mut h = mk_host();
        h.port = 2222;
        h.identity_file = Some("~/.ssh/id_ed25519".to_string());
        h.forward_agent = true;
        assert_eq!(
            build_ssh_opts(&h, &HashMap::new()),
            vec!["-p", "2222", "-i", "~/.ssh/id_ed25519", "-A"]
        );
    }

    #[test]
    fn empty_identity_is_not_emitted() {
        let mut h = mk_host();
        h.identity_file = Some(String::new());
        assert_eq!(build_ssh_opts(&h, &HashMap::new()), vec!["-p", "22"]);
    }

    #[test]
    fn ssh_options_become_dash_o_pairs() {
        let mut h = mk_host();
        h.ssh_options = vec![
            "ServerAliveInterval=30".to_string(),
            "SetEnv=FOO=bar".to_string(),
        ];
        assert_eq!(
            build_ssh_opts(&h, &HashMap::new()),
            vec!["-p", "22", "-o", "ServerAliveInterval=30", "-o", "SetEnv=FOO=bar"]
        );
    }

    #[test]
    fn blank_ssh_options_are_skipped() {
        let mut h = mk_host();
        h.ssh_options = vec!["  ".to_string(), "Compression=yes".to_string()];
        assert_eq!(
            build_ssh_opts(&h, &HashMap::new()),
            vec!["-p", "22", "-o", "Compression=yes"]
        );
    }

    #[test]
    fn ssh_options_come_last_so_they_can_override() {
        let mut h = mk_host();
        h.forward_agent = true;
        h.ssh_options = vec!["ForwardAgent=no".to_string()];
        let opts = build_ssh_opts(&h, &HashMap::new());
        let a_pos = opts.iter().position(|o| o == "-A").unwrap();
        let override_pos = opts.iter().position(|o| o == "ForwardAgent=no").unwrap();
        assert!(a_pos < override_pos, "raw options must come after the flags they override");
    }

    #[test]
    fn interactive_argv_includes_ssh_options() {
        let mut h = mk_host();
        h.ssh_options = vec!["Compression=yes".to_string()];
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert_eq!(argv, vec!["ssh", "root@10.0.0.5", "-p", "22", "-o", "Compression=yes"]);
    }

    // ---- mosh quoting ----------------------------------------------------

    #[test]
    fn mosh_quotes_an_identity_path_containing_a_space() {
        let mut h = mk_host();
        h.mosh = true;
        h.identity_file = Some("/home/a/my keys/id_ed25519".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert_eq!(argv[0], "mosh");
        // The path must survive as ONE argument once mosh re-splits the string.
        assert_eq!(argv[1], "--ssh=ssh -p 22 -i '/home/a/my keys/id_ed25519'");
        assert_eq!(argv[2], "root@10.0.0.5");
    }

    #[test]
    fn mosh_quotes_an_ssh_option_containing_a_space() {
        let mut h = mk_host();
        h.mosh = true;
        h.ssh_options = vec!["SetEnv=GREETING=hello world".to_string()];
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert_eq!(argv[1], "--ssh=ssh -p 22 -o 'SetEnv=GREETING=hello world'");
    }

    #[test]
    fn mosh_leaves_plain_arguments_unquoted() {
        let mut h = mk_host();
        h.mosh = true;
        h.port = 2222;
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert_eq!(argv[1], "--ssh=ssh -p 2222");
    }

    // ---- build_exec_argv: fan-out ----------------------------------------

    #[test]
    fn exec_argv_bounds_the_connection_then_runs_the_command() {
        let h = mk_host();
        let argv = build_exec_argv(&h, &HashMap::new(), "uptime", 10);
        assert_eq!(
            argv,
            vec![
                "ssh",
                "-o", "ConnectTimeout=10",
                "-o", "ServerAliveInterval=10",
                "-o", "ServerAliveCountMax=1",
                "-p", "22",
                "root@10.0.0.5",
                "uptime",
            ]
        );
    }

    #[test]
    fn exec_argv_inherits_per_host_connection_settings() {
        let mut h = mk_host();
        h.port = 2222;
        h.identity_file = Some("/k/id".to_string());
        h.forward_agent = true;
        h.ssh_options = vec!["Compression=yes".to_string()];
        let argv = build_exec_argv(&h, &HashMap::new(), "id", 5);
        // Everything build_ssh_opts produces has to be present — a fan-out that
        // reached a host differently from an interactive connect would be a trap.
        for expected in ["-p", "2222", "-i", "/k/id", "-A", "-o", "Compression=yes"] {
            assert!(argv.iter().any(|a| a == expected), "missing {expected} in {argv:?}");
        }
        assert_eq!(argv.last().unwrap(), "id");
    }

    #[test]
    fn exec_argv_never_uses_a_zero_timeout() {
        let h = mk_host();
        let argv = build_exec_argv(&h, &HashMap::new(), "true", 0);
        // ConnectTimeout=0 means "no timeout" to ssh — the opposite of intent.
        assert!(argv.iter().any(|a| a == "ConnectTimeout=1"));
    }

    #[test]
    fn exec_argv_places_the_command_after_the_target() {
        let h = mk_host();
        let argv = build_exec_argv(&h, &HashMap::new(), "echo hi", 10);
        let target = argv.iter().position(|a| a == "root@10.0.0.5").unwrap();
        let cmd = argv.iter().position(|a| a == "echo hi").unwrap();
        assert!(target < cmd);
    }
}
