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
