//! Helpers for `~/.ssh/known_hosts` maintenance.
//!
//! The typical use case is cleaning up a stale entry after a host's
//! fingerprint changes (new OS install, IP reuse, etc.) — exactly what
//! `ssh-keygen -R <hostname>` does — plus reading and pinning host keys so the
//! TUI can show a fingerprint and offer trust-on-first-use.

use std::io::Write;
use std::process::Command;

/// One host key as reported by `ssh-keygen -l`: its algorithm and SHA256
/// fingerprint (e.g. `ED25519` / `SHA256:abc…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKey {
    pub key_type: String,
    pub fingerprint: String,
}

impl std::fmt::Display for HostKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.fingerprint, self.key_type)
    }
}

/// The known_hosts lookup key for `hostname:port`: a bare hostname on the
/// default port, or the `[host]:port` form OpenSSH uses otherwise.
fn host_spec(hostname: &str, port: u16) -> String {
    if port == 22 {
        hostname.to_string()
    } else {
        format!("[{hostname}]:{port}")
    }
}

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

/// The host key(s) currently pinned for `hostname:port` in `~/.ssh/known_hosts`,
/// as `(type, fingerprint)` pairs. Empty when the host is unknown (first use)
/// or `ssh-keygen` is unavailable.
pub fn pinned_fingerprints(hostname: &str, port: u16) -> Vec<HostKey> {
    let spec = host_spec(hostname, port);
    let Ok(found) = Command::new("ssh-keygen").arg("-F").arg(&spec).output() else {
        return Vec::new();
    };
    // `ssh-keygen -F` exits non-zero (and prints nothing) when the host is not
    // present — treat that simply as "no pinned key".
    fingerprints_from_lines(&String::from_utf8_lossy(&found.stdout))
}

/// Whether `hostname:port` already has a pinned entry in `known_hosts`.
pub fn is_pinned(hostname: &str, port: u16) -> bool {
    !pinned_fingerprints(hostname, port).is_empty()
}

/// The host key(s) the server at `hostname:port` presents *right now*, fetched
/// with `ssh-keyscan` (5s timeout). Empty when the host is unreachable, offers
/// no key, or the tool is missing — callers treat that as "couldn't check".
pub fn scan_fingerprints(hostname: &str, port: u16) -> Vec<HostKey> {
    let Ok(scan) = Command::new("ssh-keyscan")
        .args(["-p", &port.to_string(), "-T", "5", hostname])
        .output()
    else {
        return Vec::new();
    };
    fingerprints_from_lines(&String::from_utf8_lossy(&scan.stdout))
}

/// Trust-on-first-use: fetch the live host key with `ssh-keyscan` and append it
/// to `~/.ssh/known_hosts`, pinning it for future connections.
///
/// Errors if the host returns no key (unreachable / no SSH) or the file can't
/// be written. Intended for hosts that are not yet pinned; the caller is
/// expected to have shown the fingerprint and gotten explicit consent first.
pub fn pin_host(hostname: &str, port: u16) -> std::io::Result<()> {
    let scan = Command::new("ssh-keyscan")
        .args(["-p", &port.to_string(), "-T", "5", hostname])
        .output()?;
    // ssh-keyscan writes only comment lines (or nothing) to stdout when it can't
    // reach the host; a real key line never starts with '#'.
    let has_key = String::from_utf8_lossy(&scan.stdout)
        .lines()
        .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
    if !has_key {
        return Err(std::io::Error::other(format!(
            "no host key returned by {hostname}:{port} (unreachable?)"
        )));
    }

    let Some(home) = dirs::home_dir() else {
        return Err(std::io::Error::other("no HOME dir"));
    };
    let ssh_dir = home.join(".ssh");
    std::fs::create_dir_all(&ssh_dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ssh_dir.join("known_hosts"))?;
    file.write_all(&scan.stdout)?;
    Ok(())
}

/// Fingerprint a blob of known_hosts / `ssh-keyscan`-format lines by handing
/// them to `ssh-keygen -l -f`. Both pinned lookups and live scans share this
/// format, so both funnel through here.
fn fingerprints_from_lines(lines: &str) -> Vec<HostKey> {
    let trimmed = lines.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    // `ssh-keygen -l` reads a file, not stdin, so stage the lines in a temp file
    // keyed by PID to avoid clobbering a concurrent call in the same process.
    let tmp = std::env::temp_dir().join(format!("sshm-kh-{}.tmp", std::process::id()));
    if std::fs::write(&tmp, lines).is_err() {
        return Vec::new();
    }
    let out = Command::new("ssh-keygen").arg("-l").arg("-f").arg(&tmp).output();
    let _ = std::fs::remove_file(&tmp);
    let Ok(out) = out else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(parse_fingerprint_line)
        .collect()
}

/// Parse one `ssh-keygen -l` output line — `256 SHA256:… host (ED25519)` — into
/// its fingerprint and algorithm. Returns `None` for lines that don't fit.
fn parse_fingerprint_line(line: &str) -> Option<HostKey> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split_whitespace();
    let _bits = parts.next()?;
    let fingerprint = parts.next()?.to_string();
    // Algorithm is the trailing parenthesised token, e.g. "(ED25519)".
    let key_type = line
        .rsplit('(')
        .next()
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or("?")
        .to_string();
    // Guard against comment / progress lines: a real fingerprint is either a
    // `SHA256:`/`MD5:`-prefixed hash or the legacy hex-colon MD5 form.
    let looks_like_fingerprint = fingerprint.starts_with("SHA256:")
        || fingerprint.starts_with("SHA1:")
        || fingerprint.starts_with("MD5:")
        || (fingerprint.contains(':')
            && fingerprint.chars().all(|c| c.is_ascii_hexdigit() || c == ':'));
    if !looks_like_fingerprint {
        return None;
    }
    Some(HostKey { key_type, fingerprint })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn host_spec_default_port_is_bare() {
        assert_eq!(host_spec("example.com", 22), "example.com");
    }

    #[test]
    fn host_spec_custom_port_is_bracketed() {
        assert_eq!(host_spec("example.com", 2222), "[example.com]:2222");
    }

    #[test]
    fn parses_standard_fingerprint_line() {
        let hk = parse_fingerprint_line(
            "256 SHA256:abc123DEF example.com (ED25519)",
        )
        .unwrap();
        assert_eq!(hk.fingerprint, "SHA256:abc123DEF");
        assert_eq!(hk.key_type, "ED25519");
    }

    #[test]
    fn parses_bracketed_host_line() {
        let hk = parse_fingerprint_line(
            "3072 SHA256:zzz [example.com]:2222 (RSA)",
        )
        .unwrap();
        assert_eq!(hk.fingerprint, "SHA256:zzz");
        assert_eq!(hk.key_type, "RSA");
    }

    #[test]
    fn rejects_non_fingerprint_lines() {
        // ssh-keyscan comment / progress lines must not parse as keys.
        assert!(parse_fingerprint_line("# example.com:22 SSH-2.0-OpenSSH_9.6").is_none());
        assert!(parse_fingerprint_line("").is_none());
    }

    #[test]
    fn display_is_fingerprint_then_type() {
        let hk = HostKey {
            key_type: "ED25519".into(),
            fingerprint: "SHA256:abc".into(),
        };
        assert_eq!(hk.to_string(), "SHA256:abc (ED25519)");
    }
}
