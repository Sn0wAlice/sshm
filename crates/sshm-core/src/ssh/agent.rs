//! Thin wrappers around the `ssh-add` CLI to inspect and edit the running
//! `ssh-agent`'s key set.

use std::path::Path;
use std::process::Command;

/// Return the list of SHA256 fingerprints currently held by the agent, or
/// `None` if the agent is not running / not reachable. A successful call
/// with zero keys returns `Some(vec![])`.
pub fn agent_fingerprints() -> Option<Vec<String>> {
    let out = Command::new("ssh-add").arg("-l").output().ok()?;
    // ssh-add exit codes: 0 = ok, 1 = no identities, 2 = no agent.
    if !out.status.success() {
        if out.status.code() == Some(1) {
            return Some(Vec::new());
        }
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut fps = Vec::new();
    for line in s.lines() {
        // Format: "2048 SHA256:abc... comment (RSA)"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            fps.push(parts[1].to_string());
        }
    }
    Some(fps)
}

/// Add a private key to the agent (`ssh-add <key>`).
pub fn agent_add(key: &Path) -> std::io::Result<()> {
    let status = Command::new("ssh-add").arg(key).status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!("ssh-add exited {status}")));
    }
    Ok(())
}

/// Remove a private key from the agent (`ssh-add -d <key>`).
pub fn agent_remove(key: &Path) -> std::io::Result<()> {
    let status = Command::new("ssh-add").arg("-d").arg(key).status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!("ssh-add -d exited {status}")));
    }
    Ok(())
}
