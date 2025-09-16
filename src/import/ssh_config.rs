use std::collections::HashMap;
use std::fs;
use ssh_config::SSHConfig;
use crate::models::Host;

/// Import entries from ~/.ssh/config and merge them into our map.
pub fn import_ssh_config(hosts: &mut HashMap<String, Host>) {
    let ssh_path = dirs::home_dir().map(|h| h.join(".ssh/config"));
    let Some(path) = ssh_path.filter(|p| p.exists()) else { return; };

    let Ok(text) = fs::read_to_string(&path) else { return; };
    let Ok(cfg) = SSHConfig::parse_str(&text) else { return; };

    // Extract aliases from raw file (skip wildcards)
    let mut aliases: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Host ") {
            for tok in rest.split(|c: char| c.is_whitespace() || c == ',') {
                let alias = tok.trim();
                if alias.is_empty() { continue; }
                if alias.contains('*') || alias.contains('?') || alias.starts_with('!') { continue; }
                aliases.push(alias.to_string());
            }
        }
    }
    aliases.sort();
    aliases.dedup();

    for alias in aliases {
        if hosts.contains_key(&alias) { continue; }
        let settings = cfg.query(&alias);
        let get = |k: &str| settings.get(k).map(|s| s.to_string());
        let host = get("HostName").or_else(|| get("Hostname")).unwrap_or_else(|| alias.clone());
        let username = get("User").or_else(|| get("Username")).unwrap_or_else(|| "root".into());
        let port = get("Port").and_then(|p| p.parse::<u16>().ok()).unwrap_or(22);
        let identity_file = get("IdentityFile");
        let proxy_jump = get("ProxyJump").or_else(|| get("ProxyJump"));

        hosts.insert(alias.clone(), Host {
            name: alias.clone(),
            host,
            port,
            username,
            identity_file,
            proxy_jump,
            tags: Some(vec!["ssh_config".to_string()]),
            folder: None,
        });
    }
}
