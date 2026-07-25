//! Helpers for `~/.ssh/known_hosts` maintenance.
//!
//! The typical use case is cleaning up a stale entry after a host's
//! fingerprint changes (new OS install, IP reuse, etc.) — exactly what
//! `ssh-keygen -R <hostname>` does.

use std::process::Command;

/// Remove every line matching `hostname` from `~/.ssh/known_hosts`
/// (equivalent to `ssh-keygen -R <hostname>`).
pub fn remove_known_host(hostname: &str) -> std::io::Result<()> {
    let status = Command::new("ssh-keygen").arg("-R").arg(hostname).status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "ssh-keygen -R exited {status}"
        )));
    }
    Ok(())
}

/// Like [`remove_known_host`], but also clears the port-qualified
/// `[hostname]:port` entry that OpenSSH writes for non-standard ports.
///
/// `ssh-keygen -R <host>` only matches the bare-hostname form, so a host
/// reached on e.g. port 2222 would keep its stale key otherwise.
pub fn remove_known_host_port(hostname: &str, port: u16) -> std::io::Result<()> {
    remove_known_host(hostname)?;
    if port != 22 {
        remove_known_host(&format!("[{hostname}]:{port}"))?;
    }
    Ok(())
}

/// Best-effort probe: does connecting to `hostname:port` currently fail because
/// the host's key no longer matches the pinned `known_hosts` entry?
///
/// Runs a non-interactive `ssh` that stops at the host-key stage (no auth, no
/// password prompt, short timeout) and inspects stderr for OpenSSH's
/// changed-key banner. Returns `false` on any other outcome — a clean key, an
/// unknown host, a network error, or `ssh` missing — so callers only surface
/// the "wipe & reconnect" offer when it's actually the relevant failure.
pub fn host_key_changed(hostname: &str, port: u16) -> bool {
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=8",
            // Skip authentication entirely: host-key verification happens first,
            // so a changed key still trips the banner while a good key just
            // bounces off "Permission denied (none)" without hanging.
            "-o",
            "PreferredAuthentications=none",
            "-p",
            &port.to_string(),
            hostname,
            "true",
        ])
        .output();
    match output {
        Ok(o) => stderr_indicates_key_changed(&String::from_utf8_lossy(&o.stderr)),
        Err(_) => false,
    }
}

/// Whether `stderr` from an `ssh` attempt carries OpenSSH's changed-host-key
/// warning. Split out from [`host_key_changed`] so the string matching is unit
/// testable without spawning a process.
fn stderr_indicates_key_changed(stderr: &str) -> bool {
    stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
        || stderr.contains("POSSIBLE DNS SPOOFING")
}

#[cfg(test)]
mod tests {
    use super::stderr_indicates_key_changed;

    #[test]
    fn detects_changed_key_banner() {
        let stderr = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
             @    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n\
             @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
             Host key verification failed.";
        assert!(stderr_indicates_key_changed(stderr));
    }

    #[test]
    fn detects_dns_spoofing_variant() {
        assert!(stderr_indicates_key_changed(
            "Warning: the ECDSA host key differs — POSSIBLE DNS SPOOFING DETECTED!"
        ));
    }

    #[test]
    fn ignores_unknown_host() {
        // A first-time / unknown host is a different situation — not a changed key.
        let stderr = "No ECDSA host key is known for example.com and you have \
             requested strict checking.\nHost key verification failed.";
        assert!(!stderr_indicates_key_changed(stderr));
    }

    #[test]
    fn ignores_permission_denied() {
        assert!(!stderr_indicates_key_changed("root@host: Permission denied (none)."));
    }
}
