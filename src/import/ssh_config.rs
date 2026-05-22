use std::collections::HashMap;
use std::fs;
use ssh_config::SSHConfig;
use crate::models::Host;

/// Pure-function variant of [`import_ssh_config`]: takes the raw text of an
/// `~/.ssh/config` file and returns the parsed hosts (skipping wildcard
/// patterns and aliases already present in `existing`).
///
/// Kept testable by isolating I/O in [`import_ssh_config`].
pub fn parse_ssh_config_text(
    text: &str,
    existing: &HashMap<String, Host>,
) -> Vec<Host> {
    let Ok(cfg) = SSHConfig::parse_str(text) else { return Vec::new(); };

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

    let mut out = Vec::new();
    for alias in aliases {
        if existing.contains_key(&alias) { continue; }
        let settings = cfg.query(&alias);
        let get = |k: &str| settings.get(k).map(|s| s.to_string());
        let host = get("HostName").or_else(|| get("Hostname")).unwrap_or_else(|| alias.clone());
        let username = get("User").or_else(|| get("Username")).unwrap_or_else(|| "root".into());
        let port = get("Port").and_then(|p| p.parse::<u16>().ok()).unwrap_or(22);
        let identity_file = get("IdentityFile");
        let proxy_jump = get("ProxyJump");

        out.push(Host {
            name: alias.clone(),
            host,
            port,
            username,
            identity_file,
            proxy_jump,
            tags: Some(vec!["ssh_config".to_string()]),
            folder: None,
            last_connected_at: None,
            use_count: 0,
            favorite: false,
            tunnels: vec![],
            forward_agent: false,
            mosh: false,
            notes: None,
        });
    }
    out
}

/// Import entries from `~/.ssh/config` and merge them into `hosts`.
/// I/O wrapper around [`parse_ssh_config_text`].
pub fn import_ssh_config(hosts: &mut HashMap<String, Host>) {
    let ssh_path = dirs::home_dir().map(|h| h.join(".ssh/config"));
    let Some(path) = ssh_path.filter(|p| p.exists()) else { return; };
    let Ok(text) = fs::read_to_string(&path) else { return; };
    for h in parse_ssh_config_text(&text, hosts) {
        hosts.insert(h.name.clone(), h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_entry() {
        let txt = "\
Host bastion
    HostName 1.2.3.4
    User alice
    Port 2222
    IdentityFile ~/.ssh/id_ed25519
";
        let parsed = parse_ssh_config_text(txt, &HashMap::new());
        assert_eq!(parsed.len(), 1);
        let h = &parsed[0];
        assert_eq!(h.name, "bastion");
        assert_eq!(h.host, "1.2.3.4");
        assert_eq!(h.username, "alice");
        assert_eq!(h.port, 2222);
        assert_eq!(h.identity_file.as_deref(), Some("~/.ssh/id_ed25519"));
        assert_eq!(h.tags.as_deref().unwrap(), &["ssh_config".to_string()]);
    }

    #[test]
    fn applies_defaults_when_fields_missing() {
        let txt = "Host plain\n";
        let parsed = parse_ssh_config_text(txt, &HashMap::new());
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].port, 22);
        assert_eq!(parsed[0].username, "root");
        assert_eq!(parsed[0].host, "plain"); // falls back to alias
    }

    #[test]
    fn skips_wildcard_aliases() {
        let txt = "\
Host *
    User everyone
Host !ignored
    User x
Host concrete
    HostName 5.5.5.5
";
        let parsed = parse_ssh_config_text(txt, &HashMap::new());
        let names: Vec<&str> = parsed.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["concrete"]);
    }

    #[test]
    fn skips_already_existing_aliases() {
        let txt = "Host already\n    HostName 9.9.9.9\n";
        let mut existing = HashMap::new();
        existing.insert(
            "already".to_string(),
            Host {
                name: "already".to_string(),
                host: "1.1.1.1".to_string(),
                port: 22,
                username: "u".to_string(),
                identity_file: None,
                proxy_jump: None,
                tags: None,
                folder: None,
                last_connected_at: None,
                use_count: 0,
                favorite: false,
                tunnels: vec![],
                forward_agent: false,
                mosh: false,
                notes: None,
            },
        );
        let parsed = parse_ssh_config_text(txt, &existing);
        assert!(parsed.is_empty());
    }

    #[test]
    fn parses_multi_alias_line() {
        let txt = "\
Host alpha beta gamma
    HostName 7.7.7.7
";
        let parsed = parse_ssh_config_text(txt, &HashMap::new());
        let mut names: Vec<&str> = parsed.iter().map(|h| h.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    }
}
