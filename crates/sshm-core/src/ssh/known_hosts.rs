//! Helpers for `~/.ssh/known_hosts` maintenance.
//!
//! The typical use case is cleaning up a stale entry after a host's
//! fingerprint changes (new OS install, IP reuse, etc.) — exactly what
//! `ssh-keygen -R <hostname>` does — plus reading and pinning host keys so the
//! TUI can show a fingerprint and offer trust-on-first-use.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::ssh::client::validate_hostname;

/// One host key as reported by `ssh-keygen -l`: its algorithm and SHA256
/// fingerprint (e.g. `ED25519` / `SHA256:abc…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKey {
    pub key_type: String,
    pub fingerprint: String,
}

/// The result of one `ssh-keyscan`: the fingerprints to show the user *and*
/// the exact key lines they were computed from.
///
/// Pinning writes `lines` verbatim instead of scanning again, so what lands in
/// `known_hosts` is precisely what the user approved — a second scan would
/// open a window for a man-in-the-middle to swap the key after the prompt.
#[derive(Debug, Clone, Default)]
pub struct HostScan {
    pub keys: Vec<HostKey>,
    lines: String,
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
    let status = Command::new("ssh-keygen")
        .arg("-R")
        .arg(hostname)
        .status()?;
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
    if validate_hostname(hostname).is_err() {
        return false;
    }
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
            "--",
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
    // `ssh-keygen -F` alone answers this: it exits 0 and prints the entry
    // only when the host is known — no need to fingerprint what it found.
    Command::new("ssh-keygen")
        .arg("-F")
        .arg(host_spec(hostname, port))
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
}

/// The host key(s) the server at `hostname:port` presents *right now*, fetched
/// with `ssh-keyscan` (5s timeout). Empty when the host is unreachable, offers
/// no key, or the tool is missing — callers treat that as "couldn't check".
pub fn scan_fingerprints(hostname: &str, port: u16) -> Vec<HostKey> {
    scan_host(hostname, port).keys
}

/// Like [`scan_fingerprints`], but keeps the scanned key lines so the caller
/// can later [`pin_host`] exactly the keys it showed.
pub fn scan_host(hostname: &str, port: u16) -> HostScan {
    if validate_hostname(hostname).is_err() {
        return HostScan::default();
    }
    let Ok(scan) = Command::new("ssh-keyscan")
        .args(["-p", &port.to_string(), "-T", "5", "--", hostname])
        .output()
    else {
        return HostScan::default();
    };
    // ssh-keyscan writes only comment lines (or nothing) to stdout when it can't
    // reach the host; a real key line never starts with '#'.
    let lines: String = String::from_utf8_lossy(&scan.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| format!("{l}\n"))
        .collect();
    HostScan {
        keys: fingerprints_from_lines(&lines),
        lines,
    }
}

/// Trust-on-first-use: append the key lines from `scan` — the ones whose
/// fingerprints the user just approved — to `~/.ssh/known_hosts`, pinning them
/// for future connections.
///
/// Errors if the scan holds no key (unreachable / no SSH) or the file can't be
/// written. Intended for hosts that are not yet pinned; the caller is expected
/// to have shown `scan.keys` and gotten explicit consent first.
pub fn pin_host(scan: &HostScan) -> std::io::Result<()> {
    if scan.keys.is_empty() || scan.lines.is_empty() {
        return Err(std::io::Error::other(
            "no host key to pin (host unreachable?)",
        ));
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
    file.write_all(scan.lines.as_bytes())?;
    Ok(())
}

/// Fingerprint a blob of known_hosts / `ssh-keyscan`-format lines by piping
/// them to `ssh-keygen -l -f -`. Both pinned lookups and live scans share this
/// format, so both funnel through here. Stdin rather than a temp file: no
/// predictable path to race or symlink, no collision between threads.
fn fingerprints_from_lines(lines: &str) -> Vec<HostKey> {
    if lines.trim().is_empty() {
        return Vec::new();
    }
    let Ok(mut child) = Command::new("ssh-keygen")
        .args(["-l", "-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A failed write just means ssh-keygen bailed early; its (empty)
        // output below says the same.
        let _ = stdin.write_all(lines.as_bytes());
    }
    let Ok(out) = child.wait_with_output() else {
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
            && fingerprint
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':'));
    if !looks_like_fingerprint {
        return None;
    }
    Some(HostKey {
        key_type,
        fingerprint,
    })
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
        assert!(!stderr_indicates_key_changed(
            "root@host: Permission denied (none)."
        ));
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
        let hk = parse_fingerprint_line("256 SHA256:abc123DEF example.com (ED25519)").unwrap();
        assert_eq!(hk.fingerprint, "SHA256:abc123DEF");
        assert_eq!(hk.key_type, "ED25519");
    }

    #[test]
    fn parses_bracketed_host_line() {
        let hk = parse_fingerprint_line("3072 SHA256:zzz [example.com]:2222 (RSA)").unwrap();
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
    fn fingerprints_lines_via_stdin() {
        // Skips silently where ssh-keygen isn't installed.
        if Command::new("ssh-keygen").arg("-?").output().is_err() {
            return;
        }
        let keys = fingerprints_from_lines(
            "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl\n",
        );
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ED25519");
        assert!(keys[0].fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn pin_refuses_an_empty_scan() {
        assert!(pin_host(&HostScan::default()).is_err());
    }

    #[test]
    fn hostile_hostnames_are_never_scanned() {
        assert!(scan_host("-oProxyCommand=x", 22).keys.is_empty());
        assert!(!host_key_changed("-oProxyCommand=x", 22));
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
