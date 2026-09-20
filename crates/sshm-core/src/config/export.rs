use crate::models::{Database, TunnelKind};
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;

/// Export the host database as an SSH config file.
pub fn export_ssh_config(db: &Database, raw_path: &str) -> Result<()> {
    if raw_path.trim().is_empty() {
        return Err(anyhow!("Export path is empty"));
    }

    let expanded = shellexpand::tilde(raw_path);
    let path = Path::new(expanded.as_ref());

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating export parent dir {}", parent.display()))?;
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
        if host.forward_agent {
            content.push_str("    ForwardAgent yes\n");
        }
        // `-o Key=Value` on the command line is `Key Value` in a config file.
        // An entry without `=` is rejected at input, but stay defensive: a
        // hand-edited host.json could carry one, and a bare keyword would make
        // ssh refuse to parse the whole file.
        for raw in &host.ssh_options {
            if let Some((k, v)) = raw.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                if !k.is_empty() && !v.is_empty() {
                    content.push_str(&format!("    {k} {v}\n"));
                }
            }
        }
        for t in &host.tunnels {
            let target_host = if t.remote_host.is_empty() {
                "localhost"
            } else {
                t.remote_host.as_str()
            };
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

    fs::write(path, &content).with_context(|| format!("writing export file {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Host;
    use std::io::Read;

    fn export_to_string(db: &Database) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        export_ssh_config(db, path.to_str().unwrap()).unwrap();
        let mut s = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        s
    }

    fn db_with(h: Host) -> Database {
        let mut db = Database::default();
        db.hosts.insert(h.name.clone(), h);
        db
    }

    #[test]
    fn ssh_options_become_config_keywords() {
        // `-o Key=Value` on the command line is `Key Value` in a config file.
        let out = export_to_string(&db_with(Host {
            name: "web".into(),
            host: "10.0.0.5".into(),
            ssh_options: vec!["ServerAliveInterval=30".into(), "Compression=yes".into()],
            ..Default::default()
        }));
        assert!(out.contains("    ServerAliveInterval 30\n"), "{out}");
        assert!(out.contains("    Compression yes\n"), "{out}");
        assert!(
            !out.contains('='),
            "config syntax uses a space, not '=':\n{out}"
        );
    }

    #[test]
    fn a_value_containing_equals_keeps_its_tail() {
        // Only the FIRST `=` separates keyword from value: SetEnv carries one.
        let out = export_to_string(&db_with(Host {
            name: "web".into(),
            host: "10.0.0.5".into(),
            ssh_options: vec!["SetEnv=FOO=bar".into()],
            ..Default::default()
        }));
        assert!(out.contains("    SetEnv FOO=bar\n"), "{out}");
    }

    #[test]
    fn a_malformed_option_is_skipped_not_emitted() {
        // Input validation rejects these, but a hand-edited host.json could
        // carry one — and a bare keyword makes ssh refuse the whole file.
        let out = export_to_string(&db_with(Host {
            name: "web".into(),
            host: "10.0.0.5".into(),
            ssh_options: vec!["Compression".into(), "=novalue".into(), "Empty=".into()],
            ..Default::default()
        }));
        assert!(!out.contains("Compression"), "{out}");
        assert!(!out.contains("novalue"), "{out}");
        assert!(!out.contains("Empty"), "{out}");
    }

    #[test]
    fn forward_agent_is_exported() {
        let out = export_to_string(&db_with(Host {
            name: "bastion".into(),
            host: "1.2.3.4".into(),
            forward_agent: true,
            ..Default::default()
        }));
        assert!(out.contains("    ForwardAgent yes\n"), "{out}");
    }

    #[test]
    fn forward_agent_off_emits_nothing() {
        let out = export_to_string(&db_with(Host {
            name: "web".into(),
            host: "1.2.3.4".into(),
            ..Default::default()
        }));
        assert!(!out.contains("ForwardAgent"), "{out}");
    }
}
