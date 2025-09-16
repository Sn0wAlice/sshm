use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use crate::models::Host;

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
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("ssh exited with status {status}")));
    }
    Ok(())
}
