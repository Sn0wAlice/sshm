use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use crate::models::Host;
use crate::ssh::agent::agent_fingerprints;

/// One private-key/public-key pair discovered under `~/.ssh`.
#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub private: PathBuf,
    pub public: PathBuf,
    pub key_type: String,
    pub bits: Option<u32>,
    pub comment: String,
    pub fingerprint: String,
    pub in_agent: bool,
    /// True when the key is backed by a FIDO2 / hardware token
    /// (`*-sk` keytype, e.g. ED25519-SK / ECDSA-SK).
    pub is_hardware: bool,
}

/// Heuristic: a key is hardware-backed when its OpenSSH key-type ends with
/// `-SK` (case-insensitive) or, lacking a key-type, when the filename matches
/// the conventional `id_*_sk` pattern.
pub fn is_hardware_key(key_type: &str, file_name: &str) -> bool {
    let kt = key_type.to_ascii_uppercase();
    if kt.ends_with("-SK") || kt.contains("SK-") {
        return true;
    }
    let fn_lower = file_name.to_ascii_lowercase();
    fn_lower.ends_with("_sk") || fn_lower.ends_with("-sk")
}

/// Scan `~/.ssh` for private keys (any file whose `<file>.pub` sibling
/// exists) and return one [`KeyEntry`] per pair. Results are sorted by
/// filename. If `ssh-keygen`/`ssh-add` is missing, unknown fields are
/// filled with sensible placeholders.
pub fn scan_ssh_dir() -> Vec<KeyEntry> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let ssh_dir = home.join(".ssh");
    let Ok(entries) = std::fs::read_dir(&ssh_dir) else {
        return Vec::new();
    };

    let agent_fps = agent_fingerprints().unwrap_or_default();

    let mut results: Vec<KeyEntry> = Vec::new();
    for entry in entries.flatten() {
        let private = entry.path();
        // Skip .pub files themselves and anything that isn't a regular file.
        if private.extension().and_then(|e| e.to_str()) == Some("pub") {
            continue;
        }
        if !private.is_file() {
            continue;
        }
        // Skip common non-key files that happen to live in ~/.ssh.
        if let Some(name) = private.file_name().and_then(|n| n.to_str()) {
            if matches!(
                name,
                "config" | "known_hosts" | "known_hosts.old" | "authorized_keys"
            ) {
                continue;
            }
        }

        let public = PathBuf::from(format!("{}.pub", private.display()));
        if !public.exists() {
            continue;
        }

        let (bits, fingerprint, comment, key_type) = parse_pubkey_fingerprint(&public)
            .unwrap_or((None, "(unknown)".to_string(), String::new(), "unknown".to_string()));
        let in_agent = agent_fps.iter().any(|f| f == &fingerprint);
        let file_name = private.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_hardware = is_hardware_key(&key_type, file_name);

        results.push(KeyEntry {
            private,
            public,
            key_type,
            bits,
            comment,
            fingerprint,
            in_agent,
            is_hardware,
        });
    }
    results.sort_by(|a, b| a.private.file_name().cmp(&b.private.file_name()));
    results
}

/// Parse one line of `ssh-keygen -lf <pub>`:
///     `256 SHA256:abc=== alice@laptop (ED25519)`
fn parse_pubkey_fingerprint(
    pub_path: &Path,
) -> Option<(Option<u32>, String, String, String)> {
    let out = Command::new("ssh-keygen").arg("-lf").arg(pub_path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // The key type is always the last "(TYPE)" token.
    let paren_start = line.rfind('(')?;
    let key_type = line[paren_start + 1..].trim_end_matches(')').to_string();
    let before_paren = line[..paren_start].trim_end();
    // "256 SHA256:... comment with possibly spaces"
    let mut iter = before_paren.splitn(3, ' ');
    let bits = iter.next()?.parse::<u32>().ok();
    let fingerprint = iter.next()?.to_string();
    let comment = iter.next().unwrap_or("").to_string();
    Some((bits, fingerprint, comment, key_type))
}

/// Generate a new key pair via `ssh-keygen`.
/// `passphrase` empty means no passphrase.
pub fn generate_key(
    key_type: &str,
    path: &Path,
    comment: &str,
    passphrase: &str,
) -> std::io::Result<()> {
    // Make sure the parent directory exists.
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-t").arg(key_type);
    match key_type {
        "rsa" => { cmd.arg("-b").arg("4096"); }
        "ecdsa" => { cmd.arg("-b").arg("521"); }
        _ => {}
    }
    cmd.arg("-f").arg(path);
    cmd.arg("-C").arg(comment);
    cmd.arg("-N").arg(passphrase);
    let status = cmd.status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "ssh-keygen exited {status}"
        )));
    }
    Ok(())
}

pub fn pub_from_identity(identity: &str) -> Option<PathBuf> {
    let p = shellexpand::tilde(identity).to_string();
    let pubp = format!("{p}.pub");
    let pb = PathBuf::from(pubp);
    if pb.exists() { Some(pb) } else { None }
}

pub fn default_pubkey_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let p1 = home.join(".ssh/id_ed25519.pub");
    if p1.exists() { return Some(p1); }
    let p2 = home.join(".ssh/id_rsa.pub");
    if p2.exists() { return Some(p2); }
    None
}

#[cfg(test)]
mod tests {
    use super::is_hardware_key;

    #[test]
    fn detects_hardware_by_keytype() {
        assert!(is_hardware_key("ED25519-SK", "anything"));
        assert!(is_hardware_key("ecdsa-sk", "anything"));
    }

    #[test]
    fn detects_hardware_by_filename_fallback() {
        assert!(is_hardware_key("unknown", "id_ed25519_sk"));
        assert!(is_hardware_key("unknown", "yubi-sk"));
    }

    #[test]
    fn regular_keys_not_hardware() {
        assert!(!is_hardware_key("ED25519", "id_ed25519"));
        assert!(!is_hardware_key("RSA", "id_rsa"));
    }
}

pub fn install_pubkey_on_host(h: &Host, pubkey_path: &Path) -> std::io::Result<()> {
    // 1) Try ssh-copy-id
    let mut try_copy_id = Command::new("ssh-copy-id");
    try_copy_id
        .arg("-p").arg(h.port.to_string())
        .arg("-f")
        .arg(format!("{}@{}", h.username, h.host))
        .arg("-i").arg(pubkey_path);

    if let Some(j) = &h.proxy_jump {
        try_copy_id.arg("-o").arg(format!("ProxyJump={}", j));
    }
    if let Some(id) = &h.identity_file {
        try_copy_id.arg("-o").arg(format!("IdentityFile={}", shellexpand::tilde(id)));
    }

    match try_copy_id.status() {
        Ok(st) if st.success() => return Ok(()),
        _ => {
            eprintln!("`ssh-copy-id` indisponible ou a échoué, fallback sur méthode manuelle…");
        }
    }

    // 2) Manual fallback
    let key_content = std::fs::read_to_string(pubkey_path)?;
    let mut ssh = Command::new("ssh");
    ssh.arg(format!("{}@{}", h.username, h.host))
        .arg("-p").arg(h.port.to_string())
        .stdin(Stdio::piped());

    if let Some(j) = &h.proxy_jump { ssh.arg("-J").arg(j); }
    if let Some(id) = &h.identity_file { ssh.arg("-i").arg(shellexpand::tilde(id).to_string()); }

    ssh.arg("bash").arg("-lc").arg(
        "set -e; \
         umask 077; \
         mkdir -p ~/.ssh; \
         touch ~/.ssh/authorized_keys; \
         chmod 700 ~/.ssh; chmod 600 ~/.ssh/authorized_keys; \
         TMP=$(mktemp); cat >> \"$TMP\"; \
         if ! grep -qxF \"$(cat \"$TMP\")\" ~/.ssh/authorized_keys; then \
            cat \"$TMP\" >> ~/.ssh/authorized_keys; \
         fi; rm -f \"$TMP\"",
    );

    let mut child = ssh.spawn()?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("failed to open ssh stdin");
        stdin.write_all(key_content.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::other(format!("ssh exited with status {status}")));
    }
    Ok(())
}
