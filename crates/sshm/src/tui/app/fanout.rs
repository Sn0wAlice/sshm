//! Fan-out: run one command across every bulk-selected host, sequentially.
//!
//! The caller is expected to have already left the alternate screen — the
//! prompts and per-host output stream straight to the terminal, exactly like
//! the `kluster` shell / logs flows.

use std::collections::HashMap;
use std::process::Command;

use crate::models::Host;

/// Prompt for a command, confirm it against `names`, then run it on each host
/// over ssh in order. Returns `Some((ok, failed))` once finished, or `None`
/// when the user cancels at the command prompt or the confirmation.
pub fn run_fanout(
    all_hosts: &HashMap<String, Host>,
    names: &[String],
) -> Option<(usize, usize)> {
    use inquire::{Confirm, Text};

    println!();
    let command = Text::new("Command to run on the selected hosts:")
        .prompt()
        .ok()?;
    let command = command.trim().to_string();
    if command.is_empty() {
        return None;
    }

    println!();
    println!("Target hosts ({}):", names.len());
    for n in names {
        match all_hosts.get(n) {
            Some(h) => println!("  • {}  ({}@{})", n, h.username, h.host),
            None => println!("  • {}  (missing — will be skipped)", n),
        }
    }
    println!();

    let confirmed = Confirm::new(&format!(
        "Run `{}` on {} host(s)?",
        command,
        names.len()
    ))
    .with_default(false)
    .prompt()
    .unwrap_or(false);
    if !confirmed {
        return None;
    }

    println!();
    let mut ok = 0usize;
    let mut failed = 0usize;
    for name in names {
        let Some(h) = all_hosts.get(name) else {
            println!("──── {} ──── skipped (host no longer exists)\n", name);
            failed += 1;
            continue;
        };
        println!("──── {}  ({}@{}) ────", name, h.username, h.host);
        match run_one(h, all_hosts, &command) {
            Some(0) => ok += 1,
            Some(code) => {
                println!("  exit status: {}", code);
                failed += 1;
            }
            None => {
                println!("  failed to launch ssh");
                failed += 1;
            }
        }
        println!();
    }

    println!("Done — {} ok, {} failed.", ok, failed);
    let _ = Text::new("Press Enter to return to sshm").prompt();
    Some((ok, failed))
}

/// Seconds allowed for the ssh handshake, and for silence on an established
/// connection, before a host is written off. Keeps one dead machine from
/// wedging the batch without cutting short a command that is still working.
const FANOUT_TIMEOUT_SECS: u32 = 10;

/// Run `command` on a single host over a non-interactive ssh session. stdio is
/// inherited so output streams live. Returns the process exit code, or `None`
/// when ssh itself could not be spawned.
///
/// The argv comes from `sshm_core`, so a fan-out reaches a host exactly the
/// way an interactive connection does — same identity, same ProxyJump chain,
/// same per-host `ssh_options`.
fn run_one(h: &Host, all_hosts: &HashMap<String, Host>, command: &str) -> Option<i32> {
    let argv = sshm_core::ssh::client::build_exec_argv(h, all_hosts, command, FANOUT_TIMEOUT_SECS);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd.status().ok().map(|s| s.code().unwrap_or(-1))
}
