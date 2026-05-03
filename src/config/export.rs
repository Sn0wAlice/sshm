use std::fs;
use std::path::Path;
use crate::models::{Database, TunnelKind};

/// Export the host database as an SSH config file.
pub fn export_ssh_config(db: &Database, raw_path: &str) -> Result<(), String> {
    if raw_path.trim().is_empty() {
        return Err("Export path is empty".into());
    }

    let expanded = shellexpand::tilde(raw_path);
    let path = Path::new(expanded.as_ref());

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directories: {e}"))?;
    }

    let mut content = String::new();
    // Sort hosts by name for stable output
    let mut hosts: Vec<_> = db.hosts.values().collect();
    hosts.sort_by(|a, b| a.name.cmp(&b.name));

    for (i, host) in hosts.iter().enumerate() {
        if i > 0 {
            content.push('\n');
        }
        content.push_str(&format!("Host {}\n", host.name));
        content.push_str(&format!("    HostName {}\n", host.host));
        content.push_str(&format!("    User {}\n", host.username));
        if host.port != 22 {
            content.push_str(&format!("    Port {}\n", host.port));
        }
        if let Some(ref id) = host.identity_file {
            if !id.is_empty() {
                content.push_str(&format!("    IdentityFile {}\n", id));
            }
        }
        if let Some(ref pj) = host.proxy_jump {
            if !pj.is_empty() {
                content.push_str(&format!("    ProxyJump {}\n", pj));
            }
        }
        for t in &host.tunnels {
            let target_host = if t.remote_host.is_empty() { "localhost" } else { t.remote_host.as_str() };
            match t.kind {
                TunnelKind::Local => {
                    content.push_str(&format!(
                        "    LocalForward {} {}:{}\n",
                        t.local_port, target_host, t.remote_port
                    ));
                }
                TunnelKind::Remote => {
                    content.push_str(&format!(
                        "    RemoteForward {} {}:{}\n",
                        t.local_port, target_host, t.remote_port
                    ));
                }
                TunnelKind::Dynamic => {
                    content.push_str(&format!("    DynamicForward {}\n", t.local_port));
                }
            }
        }
    }

    fs::write(path, &content)
        .map_err(|e| format!("Failed to write export file: {e}"))?;

    Ok(())
}
