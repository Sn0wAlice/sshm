//! `sshm completions <shell>` — completion scripts for bash, zsh and fish.
//!
//! Hand-written rather than generated: sshm parses its own argv (no clap), so
//! there is no command tree to derive them from.
//!
//! Host names are the point. Every script completes them by calling
//! `sshm list --names`, which exists for exactly this — one name per line, no
//! quoting to undo, no JSON parser in a completion function, and it stays
//! correct as hosts are added without regenerating anything.

/// Shells we emit a script for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub fn parse(s: &str) -> Option<Shell> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
        }
    }
}

/// Top-level subcommands offered by completion. Kept next to the dispatch in
/// `main.rs`; a command added there and forgotten here still works, it just
/// isn't suggested.
pub const COMMANDS: &[&str] = &[
    "list",
    "connect",
    "create",
    "edit",
    "delete",
    "tag",
    "add-identity",
    "tunnel",
    "sync",
    "completions",
    "export",
    "load_local_conf",
    "help",
];

/// Subcommands that take a saved host name as their first argument.
pub const HOST_TAKING: &[&str] = &["connect", "c", "add-identity"];

pub const SYNC_SUBCOMMANDS: &[&str] = &[
    "setup", "status", "pull", "push", "enable", "disable", "cron", "help",
];

pub const TUNNEL_SUBCOMMANDS: &[&str] = &["list", "stop"];

pub fn dispatch(args: &[String]) {
    match args.get(2).map(String::as_str) {
        Some(s) => match Shell::parse(s) {
            Some(shell) => print!("{}", script(shell)),
            None => {
                eprintln!("Unknown shell: {s}");
                usage();
                std::process::exit(2);
            }
        },
        None => {
            usage();
            std::process::exit(2);
        }
    }
}

pub fn usage() {
    println!("Usage:");
    println!("  sshm completions bash|zsh|fish");
    println!();
    println!("Install (pick the line for your shell):");
    println!("  sshm completions bash > /usr/local/etc/bash_completion.d/sshm");
    println!("  sshm completions zsh  > \"${{fpath[1]}}/_sshm\"");
    println!("  sshm completions fish > ~/.config/fish/completions/sshm.fish");
}

/// The script for `shell`.
pub fn script(shell: Shell) -> String {
    match shell {
        Shell::Bash => BASH.to_string(),
        Shell::Zsh => ZSH.to_string(),
        Shell::Fish => FISH.to_string(),
    }
}

const BASH: &str = r#"# sshm completion for bash — `sshm completions bash`
_sshm() {
    local cur prev cmd
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    cmd="${COMP_WORDS[1]}"

    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=($(compgen -W "list connect create edit delete tag add-identity tunnel sync completions export load_local_conf help" -- "$cur"))
        return
    fi

    case "$cmd" in
        connect|c|add-identity)
            if [ "$COMP_CWORD" -eq 2 ]; then
                COMPREPLY=($(compgen -W "$(sshm list --names 2>/dev/null)" -- "$cur"))
            fi
            ;;
        tag)
            if [ "$COMP_CWORD" -eq 2 ]; then
                COMPREPLY=($(compgen -W "add del" -- "$cur"))
            elif [ "$COMP_CWORD" -eq 3 ]; then
                COMPREPLY=($(compgen -W "$(sshm list --names 2>/dev/null)" -- "$cur"))
            fi
            ;;
        list)
            COMPREPLY=($(compgen -W "--filter --json --names" -- "$cur"))
            ;;
        sync)
            if [ "$COMP_CWORD" -eq 2 ]; then
                COMPREPLY=($(compgen -W "setup status pull push enable disable cron help" -- "$cur"))
            elif [ "$prev" = "status" ]; then
                COMPREPLY=($(compgen -W "--json" -- "$cur"))
            fi
            ;;
        tunnel)
            if [ "$COMP_CWORD" -eq 2 ]; then
                COMPREPLY=($(compgen -W "list stop --json" -- "$cur"))
            fi
            ;;
        completions)
            COMPREPLY=($(compgen -W "bash zsh fish" -- "$cur"))
            ;;
    esac
}
complete -F _sshm sshm
"#;

const ZSH: &str = r#"#compdef sshm
# sshm completion for zsh — `sshm completions zsh`

_sshm_hosts() {
    local -a hosts
    hosts=(${(f)"$(sshm list --names 2>/dev/null)"})
    _describe -t hosts 'saved host' hosts
}

_sshm() {
    local -a commands
    commands=(
        'list:List saved hosts'
        'connect:Connect to a host over ssh'
        'create:Create a host interactively'
        'edit:Edit a host interactively'
        'delete:Delete a host interactively'
        'tag:Add or remove tags'
        'add-identity:Push a public key to a host'
        'tunnel:Inspect or stop background tunnels'
        'sync:Config sync over git'
        'completions:Print a shell completion script'
        'export:Export the DB as an ssh config'
        'load_local_conf:Import hosts from ~/.ssh/config'
        'help:Show the CLI reference'
    )

    _arguments -C '1:command:->cmd' '*::arg:->args'

    case $state in
        cmd) _describe -t commands 'sshm command' commands ;;
        args)
            case $words[1] in
                connect|c|add-identity) _sshm_hosts ;;
                tag)
                    if (( CURRENT == 2 )); then
                        _values 'action' add del
                    else
                        _sshm_hosts
                    fi
                    ;;
                list) _values 'option' --filter --json --names ;;
                sync) _values 'subcommand' setup status pull push enable disable cron help ;;
                tunnel) _values 'subcommand' list stop --json ;;
                completions) _values 'shell' bash zsh fish ;;
            esac
            ;;
    esac
}
_sshm "$@"
"#;

const FISH: &str = r#"# sshm completion for fish — `sshm completions fish`

function __sshm_hosts
    sshm list --names 2>/dev/null
end

complete -c sshm -f

complete -c sshm -n __fish_use_subcommand -a list -d 'List saved hosts'
complete -c sshm -n __fish_use_subcommand -a connect -d 'Connect to a host over ssh'
complete -c sshm -n __fish_use_subcommand -a create -d 'Create a host interactively'
complete -c sshm -n __fish_use_subcommand -a edit -d 'Edit a host interactively'
complete -c sshm -n __fish_use_subcommand -a delete -d 'Delete a host interactively'
complete -c sshm -n __fish_use_subcommand -a tag -d 'Add or remove tags'
complete -c sshm -n __fish_use_subcommand -a add-identity -d 'Push a public key to a host'
complete -c sshm -n __fish_use_subcommand -a tunnel -d 'Inspect or stop background tunnels'
complete -c sshm -n __fish_use_subcommand -a sync -d 'Config sync over git'
complete -c sshm -n __fish_use_subcommand -a completions -d 'Print a shell completion script'
complete -c sshm -n __fish_use_subcommand -a export -d 'Export the DB as an ssh config'
complete -c sshm -n __fish_use_subcommand -a load_local_conf -d 'Import hosts from ~/.ssh/config'
complete -c sshm -n __fish_use_subcommand -a help -d 'Show the CLI reference'

complete -c sshm -n '__fish_seen_subcommand_from connect c add-identity' -a '(__sshm_hosts)' -d 'saved host'
complete -c sshm -n '__fish_seen_subcommand_from tag' -a 'add del'
complete -c sshm -n '__fish_seen_subcommand_from list' -l filter -d 'Filter expression'
complete -c sshm -n '__fish_seen_subcommand_from list' -l json -d 'Machine-readable output'
complete -c sshm -n '__fish_seen_subcommand_from list' -l names -d 'One host name per line'
complete -c sshm -n '__fish_seen_subcommand_from sync' -a 'setup status pull push enable disable cron help'
complete -c sshm -n '__fish_seen_subcommand_from tunnel' -a 'list stop'
complete -c sshm -n '__fish_seen_subcommand_from tunnel' -l json -d 'Machine-readable output'
complete -c sshm -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish'
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_names_parse_case_insensitively() {
        assert_eq!(Shell::parse("bash"), Some(Shell::Bash));
        assert_eq!(Shell::parse("ZSH"), Some(Shell::Zsh));
        assert_eq!(Shell::parse("  Fish "), Some(Shell::Fish));
        assert_eq!(Shell::parse("powershell"), None);
        assert_eq!(Shell::parse(""), None);
    }

    #[test]
    fn every_script_completes_host_names_from_the_cli() {
        // The whole reason `--names` exists. A script that stopped calling it
        // would still "work" while silently offering no hosts.
        for sh in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = script(sh);
            assert!(
                s.contains("sshm list --names"),
                "{} does not complete host names",
                sh.name()
            );
        }
    }

    #[test]
    fn every_script_offers_every_top_level_command() {
        for sh in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = script(sh);
            for cmd in COMMANDS {
                assert!(s.contains(cmd), "{} is missing `{cmd}`", sh.name());
            }
        }
    }

    #[test]
    fn host_taking_commands_are_wired_in_every_script() {
        for sh in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = script(sh);
            for cmd in HOST_TAKING {
                assert!(s.contains(cmd), "{} does not handle `{cmd}`", sh.name());
            }
        }
    }

    #[test]
    fn sync_and_tunnel_subcommands_are_offered() {
        for sh in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = script(sh);
            for sub in SYNC_SUBCOMMANDS.iter().chain(TUNNEL_SUBCOMMANDS) {
                assert!(
                    s.contains(sub),
                    "{} is missing sync/tunnel `{sub}`",
                    sh.name()
                );
            }
        }
    }

    #[test]
    fn the_json_flags_are_discoverable() {
        // fish declares a long option as `-l json`, not as the literal
        // `--json`, so the needle differs per shell.
        for sh in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = script(sh);
            let (json, names) = match sh {
                Shell::Fish => ("-l json", "-l names"),
                _ => ("--json", "--names"),
            };
            assert!(s.contains(json), "{} never offers --json", sh.name());
            assert!(s.contains(names), "{} never offers --names", sh.name());
        }
    }

    #[test]
    fn scripts_carry_the_header_their_shell_needs() {
        assert!(script(Shell::Bash).contains("complete -F _sshm sshm"));
        assert!(
            script(Shell::Zsh).starts_with("#compdef sshm"),
            "zsh needs #compdef on the first line to autoload"
        );
        assert!(script(Shell::Fish).contains("complete -c sshm"));
    }

    #[test]
    fn scripts_end_with_a_newline() {
        // They are redirected into a file; a missing trailing newline is the
        // kind of thing that bites when the file is later appended to.
        for sh in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            assert!(script(sh).ends_with('\n'), "{}", sh.name());
        }
    }

    #[test]
    fn host_lookups_never_leak_stderr_into_the_completion_list() {
        // `sshm list --names` prints diagnostics on stderr; a script that
        // forgot the redirect would offer error text as host names.
        for sh in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = script(sh);
            assert!(
                s.contains("sshm list --names 2>/dev/null"),
                "{} does not silence stderr",
                sh.name()
            );
        }
    }
}
