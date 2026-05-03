use std::collections::HashMap;
use crate::models::Host;

/// Résout une chaîne `proxy_jump` (potentiellement multi-hop, séparée par virgules).
///
/// Chaque entrée :
/// - si elle correspond au nom d'un hôte sauvegardé dans `hosts`, est étendue en
///   `user@host:port` (en omettant `:22`),
/// - sinon est laissée telle quelle (l'utilisateur a écrit un user@host[:port] direct).
///
/// Renvoie `None` si l'entrée d'origine est vide après trim.
pub fn resolve_proxy_jump(raw: &str, hosts: &HashMap<String, Host>) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let resolved: Vec<String> = trimmed
        .split(',')
        .map(|hop| hop.trim())
        .filter(|hop| !hop.is_empty())
        .map(|hop| {
            if let Some(h) = hosts.get(hop) {
                if h.port == 22 {
                    format!("{}@{}", h.username, h.host)
                } else {
                    format!("{}@{}:{}", h.username, h.host, h.port)
                }
            } else {
                hop.to_string()
            }
        })
        .collect();

    if resolved.is_empty() {
        None
    } else {
        Some(resolved.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Host;

    fn mk_host(name: &str, host: &str, user: &str, port: u16) -> Host {
        Host {
            name: name.to_string(),
            host: host.to_string(),
            port,
            username: user.to_string(),
            identity_file: None,
            proxy_jump: None,
            tags: None,
            folder: None,
            last_connected_at: None,
            use_count: 0,
            favorite: false,
            tunnels: vec![],
            forward_agent: false,
        }
    }

    #[test]
    fn empty_returns_none() {
        let hosts = HashMap::new();
        assert!(resolve_proxy_jump("", &hosts).is_none());
        assert!(resolve_proxy_jump("   ", &hosts).is_none());
    }

    #[test]
    fn single_hop_resolved_by_name() {
        let mut hosts = HashMap::new();
        hosts.insert("bastion".to_string(), mk_host("bastion", "1.2.3.4", "ubuntu", 22));
        assert_eq!(
            resolve_proxy_jump("bastion", &hosts),
            Some("ubuntu@1.2.3.4".to_string())
        );
    }

    #[test]
    fn single_hop_with_custom_port() {
        let mut hosts = HashMap::new();
        hosts.insert("bastion".to_string(), mk_host("bastion", "1.2.3.4", "ubuntu", 2222));
        assert_eq!(
            resolve_proxy_jump("bastion", &hosts),
            Some("ubuntu@1.2.3.4:2222".to_string())
        );
    }

    #[test]
    fn multi_hop_mixed() {
        let mut hosts = HashMap::new();
        hosts.insert("a".to_string(), mk_host("a", "10.0.0.1", "alice", 22));
        hosts.insert("b".to_string(), mk_host("b", "10.0.0.2", "bob", 2222));
        assert_eq!(
            resolve_proxy_jump("a, b, root@external.com", &hosts),
            Some("alice@10.0.0.1,bob@10.0.0.2:2222,root@external.com".to_string())
        );
    }

    #[test]
    fn unknown_passthrough() {
        let hosts = HashMap::new();
        assert_eq!(
            resolve_proxy_jump("user@host:42", &hosts),
            Some("user@host:42".to_string())
        );
    }
}
