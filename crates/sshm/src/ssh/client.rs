//! Frontend ssh entry points.
//!
//! The pure command builders — [`build_ssh_argv`] and [`launch_ssh`] — live in
//! `sshm_core::ssh::client` and are re-exported here. On top of them this file
//! adds the interactive flows that drive a terminal prompt (`inquire`) and so
//! stay out of core: the trust-on-first-use gate and the host-key-change
//! recovery around a connection.

use std::collections::HashMap;

use sshm_core::models::Host;
use sshm_core::ssh::known_hosts;

pub use sshm_core::ssh::client::{build_ssh_argv, launch_ssh};

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
    if !known_hosts::host_key_changed(&h.host, h.port) {
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

    match known_hosts::remove_known_host_port(&h.host, h.port) {
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
