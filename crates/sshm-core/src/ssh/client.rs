use crate::models::Host;
use crate::ssh::proxy::resolve_proxy_jump;
use std::collections::HashMap;
use std::process::Command;

/// The ssh option flags that describe *how* to reach `h`: the host's raw
/// `ssh_options`, then port, identity, ProxyJump chain and agent forwarding.
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
    let mut opts: Vec<String> = Vec::new();
    // Raw per-host escape hatch, *first*: OpenSSH keeps the first value it sees
    // for an option, so these only override the flags below (and anything a
    // caller adds after this list) by coming before them.
    for raw in &h.ssh_options {
        let raw = raw.trim();
        if !raw.is_empty() {
            opts.push("-o".to_string());
            opts.push(raw.to_string());
        }
    }
    opts.push("-p".to_string());
    opts.push(h.port.to_string());
    if let Some(id) = &h.identity_file {
        if !id.is_empty() {
            opts.push("-i".to_string());
            opts.push(id.clone());
        }
    }
    // `-J` and `-A` can't simply be outvoted by an earlier raw option: ssh
    // fatals on `-J` when ProxyJump/ProxyCommand is already set, and `-A`
    // forces agent forwarding on regardless of a `ForwardAgent=no`. So a raw
    // option for the same setting suppresses the flag instead.
    if let Some(j) = &h.proxy_jump {
        if !has_ssh_option(h, &["proxyjump", "proxycommand"]) {
            if let Some(resolved) = resolve_proxy_jump(j, all_hosts) {
                opts.push("-J".to_string());
                opts.push(resolved);
            }
        }
    }
    if h.forward_agent && !has_ssh_option(h, &["forwardagent"]) {
        opts.push("-A".to_string());
    }
    opts
}

/// Lowercased keyword of a raw `Key=Value` / `Key Value` ssh option.
pub fn ssh_option_key(raw: &str) -> String {
    raw.trim()
        .split(|c: char| c == '=' || c.is_whitespace())
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Whether `h.ssh_options` sets any of `keys` (lowercase keywords).
pub fn has_ssh_option(h: &Host, keys: &[&str]) -> bool {
    h.ssh_options
        .iter()
        .any(|o| keys.contains(&ssh_option_key(o).as_str()))
}

/// `user@host`, or just `host` when no username is set (ssh then uses its
/// own default rather than an empty `@host` login).
pub fn destination(h: &Host) -> String {
    if h.username.is_empty() {
        h.host.clone()
    } else {
        format!("{}@{}", h.username, h.host)
    }
}

/// Options that neutralise a host's `RemoteCommand` / `RequestTTY` (from its
/// `ssh_options` or `~/.ssh/config`) for a call that supplies its own command.
/// They must come *before* the host's options: OpenSSH keeps the first value,
/// and a RemoteCommand plus a command-line command is a fatal error.
pub(crate) const NO_REMOTE_COMMAND: [&str; 4] = ["-o", "RemoteCommand=none", "-o", "RequestTTY=no"];

/// Why `host` can't be handed to ssh / ssh-keyscan as a destination, if at all.
///
/// The host travels as a positional argument, so a leading `-` would be parsed
/// as an option (`-oProxyCommand=…` runs a local command), and whitespace or an
/// `@` would split or re-route the destination. Saved hosts come from a
/// hand-editable / synced JSON file, so spawn sites check rather than trusting
/// the form that normally produced them.
pub fn validate_hostname(host: &str) -> Result<(), String> {
    if host.is_empty() {
        return Err("host is empty".into());
    }
    if host.starts_with('-') {
        return Err(format!("host '{host}' must not start with '-'"));
    }
    if host
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '@')
    {
        return Err(format!("host '{host}' must not contain whitespace or '@'"));
    }
    Ok(())
}

/// [`validate_hostname`] plus the same leading-`-` / whitespace rule for the
/// username half of `user@host`. An `@` *is* allowed in the username (ssh
/// splits on the last one, and directory logins like `a@corp` exist).
pub fn validate_destination(username: &str, host: &str) -> Result<(), String> {
    if username.starts_with('-') {
        return Err(format!("username '{username}' must not start with '-'"));
    }
    if username
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(format!("username '{username}' must not contain whitespace"));
    }
    validate_hostname(host)
}

/// Build the connection command for `h` as an argv vector — `ssh …` normally,
/// or `mosh --ssh="ssh …" …` when `h.mosh` is set. `argv[0]` is the program.
///
/// `all_hosts` resolves multi-hop `proxy_jump` entries that name saved hosts.
pub fn build_ssh_argv(h: &Host, all_hosts: &HashMap<String, Host>) -> Vec<String> {
    let ssh_opts = build_ssh_opts(h, all_hosts);
    let target = destination(h);

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
        let mut argv = vec!["ssh".to_string()];
        argv.extend(ssh_opts);
        // Run-on-connect: force a TTY and hand ssh a RemoteCommand. ssh's
        // RemoteCommand *replaces* the login shell, so a bare command (e.g.
        // `echo ok`) would run and immediately disconnect. To match the
        // intuitive "run this, then give me a shell" expectation we append
        // `; exec $SHELL -l` by default. A user who wants to manage the shell
        // lifecycle themselves — including a deliberate one-shot that exits —
        // signals it by *starting* the command with `exec ` (a `docker exec …`
        // still wants the shell back afterwards).
        if let Some(cmd) = &h.remote_command {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                let full = if cmd.starts_with("exec ") {
                    cmd.to_string()
                } else {
                    format!("{cmd}; exec ${{SHELL:-/bin/sh}} -l")
                };
                // RemoteCommand expands `%h`, `%u`, … tokens; a literal `%`
                // (`date +%F`) must be doubled to survive.
                argv.push("-t".to_string());
                argv.push("-o".to_string());
                argv.push(format!("RemoteCommand={}", full.replace('%', "%%")));
            }
        }
        // `--` ends option parsing, so even a hostile `user@host` that slipped
        // past validation is only ever a destination. It also means nothing
        // after the target is parsed as an option: [`launch_ssh`] inserts
        // CLI overrides before it.
        argv.push("--".to_string());
        argv.push(target);
        argv
    }
}

/// Argv for running `command` on `h` non-interactively (fan-out, scripts):
/// `ssh -o RemoteCommand=none -o RequestTTY=no <opts> -o ConnectTimeout=…
/// -o ServerAlive… -- user@host <command>`.
///
/// Two bounds keep one unreachable host from wedging a whole batch:
/// `ConnectTimeout` caps the handshake, and the `ServerAlive` pair makes ssh
/// give up after roughly `connect_timeout_secs` of silence on an established
/// connection — the case a plain `ConnectTimeout` does not cover. A command
/// that is genuinely still running is not interrupted.
///
/// Both come after the host's own `ssh_options` (OpenSSH keeps the first value
/// it sees), so a host that sets `ServerAliveInterval` itself still wins.
pub fn build_exec_argv(
    h: &Host,
    all_hosts: &HashMap<String, Host>,
    command: &str,
    connect_timeout_secs: u32,
) -> Vec<String> {
    let secs = connect_timeout_secs.max(1);
    let mut argv = vec!["ssh".to_string()];
    argv.extend(NO_REMOTE_COMMAND.map(String::from));
    argv.extend(build_ssh_opts(h, all_hosts));
    argv.push("-o".into());
    argv.push(format!("ConnectTimeout={secs}"));
    argv.push("-o".into());
    argv.push(format!("ServerAliveInterval={secs}"));
    argv.push("-o".into());
    argv.push("ServerAliveCountMax=1".into());
    argv.push("--".into());
    argv.push(destination(h));
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

    if let Err(e) = validate_destination(&h.username, &h.host) {
        eprintln!("sshm: refusing to connect to '{}': {e}", h.name);
        return None;
    }
    let mut argv = build_ssh_argv(h, all_hosts);
    if let Some(args) = overrides.filter(|a| !a.is_empty()) {
        // CLI extras trail the target and mix options (`-v`, `-L …`) with a
        // remote command, relying on ssh re-parsing options after the
        // destination — which `--` would turn off. The destination was just
        // validated, so dropping the `--` here is safe.
        if argv.len() >= 2 && argv[argv.len() - 2] == "--" {
            argv.remove(argv.len() - 2);
        }
        argv.extend(args.iter().cloned());
    }
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
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
        assert_eq!(argv, vec!["ssh", "-p", "22", "--", "root@10.0.0.5"]);
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
                "-p",
                "22",
                "-t",
                "-o",
                "RemoteCommand=echo ok; exec ${SHELL:-/bin/sh} -l",
                "--",
                "root@10.0.0.5",
            ]
        );
    }

    #[test]
    fn remote_command_with_exec_is_verbatim() {
        let mut h = mk_host();
        h.remote_command = Some("exec tail -f /var/log/syslog".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        // User manages the shell lifecycle — no auto-appended exec.
        assert!(argv
            .iter()
            .any(|a| a == "RemoteCommand=exec tail -f /var/log/syslog"));
    }

    #[test]
    fn exec_inside_the_command_still_gets_a_shell_back() {
        let mut h = mk_host();
        h.remote_command = Some("docker exec -it app sh".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert!(argv
            .iter()
            .any(|a| a == "RemoteCommand=docker exec -it app sh; exec ${SHELL:-/bin/sh} -l"));
    }

    #[test]
    fn remote_command_percent_is_escaped_for_token_expansion() {
        let mut h = mk_host();
        h.remote_command = Some("exec date +%F".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert!(argv.iter().any(|a| a == "RemoteCommand=exec date +%%F"));
    }

    #[test]
    fn target_follows_a_double_dash() {
        let mut h = mk_host();
        h.host = "-oProxyCommand=touch /tmp/pwn".to_string();
        let argv = build_ssh_argv(&h, &HashMap::new());
        let n = argv.len();
        assert_eq!(argv[n - 2], "--");
        assert_eq!(argv[n - 1], "root@-oProxyCommand=touch /tmp/pwn");
    }

    #[test]
    fn destination_validation() {
        assert!(validate_destination("root", "10.0.0.5").is_ok());
        assert!(validate_destination("a@corp", "h.example").is_ok());
        assert!(validate_destination("root", "-oProxyCommand=x").is_err());
        assert!(validate_destination("-oProxyCommand=x", "h").is_err());
        assert!(validate_destination("root", "a b").is_err());
        assert!(validate_destination("ro ot", "h").is_err());
        assert!(validate_destination("root", "evil@h").is_err());
        assert!(validate_destination("root", "").is_err());
    }

    #[test]
    fn blank_remote_command_is_ignored() {
        let mut h = mk_host();
        h.remote_command = Some("   ".to_string());
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert!(!argv
            .iter()
            .any(|a| a == "-t" || a.starts_with("RemoteCommand=")));
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
            vec![
                "-o",
                "ServerAliveInterval=30",
                "-o",
                "SetEnv=FOO=bar",
                "-p",
                "22"
            ]
        );
    }

    #[test]
    fn blank_ssh_options_are_skipped() {
        let mut h = mk_host();
        h.ssh_options = vec!["  ".to_string(), "Compression=yes".to_string()];
        assert_eq!(
            build_ssh_opts(&h, &HashMap::new()),
            vec!["-o", "Compression=yes", "-p", "22"]
        );
    }

    #[test]
    fn ssh_options_come_first_so_they_can_override() {
        // OpenSSH keeps the *first* value for an option, but `-A` forces
        // forwarding on and `-J` fatals next to ProxyJump/ProxyCommand — so an
        // override must suppress those flags, not just precede them.
        let mut h = mk_host();
        h.forward_agent = true;
        h.proxy_jump = Some("bastion".to_string());
        h.ssh_options = vec![
            "Compression=yes".to_string(),
            "forwardagent no".to_string(),
            "ProxyCommand=nc %h %p".to_string(),
        ];
        let opts = build_ssh_opts(&h, &HashMap::new());
        assert!(!opts.iter().any(|o| o == "-A" || o == "-J"), "{opts:?}");
        assert_eq!(&opts[..2], ["-o", "Compression=yes"]);

        h.ssh_options = vec!["ProxyJump=other".to_string()];
        let opts = build_ssh_opts(&h, &HashMap::new());
        assert!(!opts.iter().any(|o| o == "-J"), "{opts:?}");
        assert!(opts.iter().any(|o| o == "-A"), "{opts:?}");
    }

    #[test]
    fn proxy_jump_and_agent_flags_without_overrides() {
        let mut h = mk_host();
        h.forward_agent = true;
        h.proxy_jump = Some("j.example".to_string());
        let opts = build_ssh_opts(&h, &HashMap::new());
        assert!(opts.iter().any(|o| o == "-J"), "{opts:?}");
        assert!(opts.iter().any(|o| o == "-A"), "{opts:?}");
    }

    #[test]
    fn empty_username_omits_the_at() {
        let mut h = mk_host();
        h.username = String::new();
        assert_eq!(destination(&h), "10.0.0.5");
        assert_eq!(
            build_ssh_argv(&h, &HashMap::new()).last().unwrap(),
            "10.0.0.5"
        );
        let argv = build_exec_argv(&h, &HashMap::new(), "id", 5);
        assert_eq!(argv[argv.len() - 2], "10.0.0.5");
        h.mosh = true;
        assert_eq!(build_ssh_argv(&h, &HashMap::new())[2], "10.0.0.5");
    }

    #[test]
    fn exec_argv_neutralises_a_host_remote_command() {
        let mut h = mk_host();
        h.ssh_options = vec!["RemoteCommand=tmux attach".to_string()];
        let argv = build_exec_argv(&h, &HashMap::new(), "id", 5);
        let ours = argv.iter().position(|a| a == "RemoteCommand=none").unwrap();
        let theirs = argv
            .iter()
            .position(|a| a == "RemoteCommand=tmux attach")
            .unwrap();
        assert!(ours < theirs, "{argv:?}");
        assert!(argv.iter().any(|a| a == "RequestTTY=no"));
    }

    #[test]
    fn interactive_argv_includes_ssh_options() {
        let mut h = mk_host();
        h.ssh_options = vec!["Compression=yes".to_string()];
        let argv = build_ssh_argv(&h, &HashMap::new());
        assert_eq!(
            argv,
            vec![
                "ssh",
                "-o",
                "Compression=yes",
                "-p",
                "22",
                "--",
                "root@10.0.0.5"
            ]
        );
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
        assert_eq!(argv[1], "--ssh=ssh -o 'SetEnv=GREETING=hello world' -p 22");
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
                "-o",
                "RemoteCommand=none",
                "-o",
                "RequestTTY=no",
                "-p",
                "22",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ServerAliveInterval=10",
                "-o",
                "ServerAliveCountMax=1",
                "--",
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
            assert!(
                argv.iter().any(|a| a == expected),
                "missing {expected} in {argv:?}"
            );
        }
        assert_eq!(argv.last().unwrap(), "id");
    }

    #[test]
    fn exec_argv_host_options_win_over_the_bounds() {
        let mut h = mk_host();
        h.ssh_options = vec!["ServerAliveInterval=60".to_string()];
        let argv = build_exec_argv(&h, &HashMap::new(), "id", 5);
        let host_pos = argv.iter().position(|a| a == "ServerAliveInterval=60");
        let ours = argv.iter().position(|a| a == "ServerAliveInterval=5");
        assert!(host_pos < ours, "{argv:?}");
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
