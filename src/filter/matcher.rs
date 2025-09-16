use std::collections::HashMap;
use crate::models::Host;

/// Simple match insensible à la casse, avec support du '*' (wildcard).
pub fn wildcard_match(pat: &str, text: &str) -> bool {
    let pat = pat.to_lowercase();
    let text = text.to_lowercase();
    if pat == "*" { return true; }
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 { return text.contains(&pat); }
    // contains-in-order
    let mut idx = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        if let Some(found) = text[idx..].find(part) {
            idx += found + part.len();
        } else {
            return false;
        }
        if i == parts.len() - 1 && !pat.ends_with('*') && !text.ends_with(part) {
            // permissive
        }
    }
    if !pat.starts_with('*') {
        if let Some(first) = parts.iter().find(|s| !s.is_empty()) {
            if !text.starts_with(first) { return false; }
        }
    }
    if !pat.ends_with('*') {
        if let Some(last) = parts.iter().rfind(|s| !s.is_empty()) {
            if !text.ends_with(last) { return false; }
        }
    }
    true
}

/// Parse un filtre de type "tag:prod host:10.* name:web user:ubuntu"
/// Clés supportées : tag, host, name, user. Valeurs avec '*' autorisé.
pub fn filter_hosts<'a>(hosts: &'a HashMap<String, Host>, filter: &str) -> Vec<&'a Host> {
    if filter.trim().is_empty() { return hosts.values().collect(); }
    let mut name_pats: Vec<String> = Vec::new();
    let mut host_pats: Vec<String> = Vec::new();
    let mut user_pats: Vec<String> = Vec::new();
    let mut tag_pats: Vec<String> = Vec::new();

    for tok in filter.split_whitespace() {
        if let Some(rest) = tok.strip_prefix("name:") { name_pats.push(rest.to_string()); continue; }
        if let Some(rest) = tok.strip_prefix("host:") { host_pats.push(rest.to_string()); continue; }
        if let Some(rest) = tok.strip_prefix("user:") { user_pats.push(rest.to_string()); continue; }
        if let Some(rest) = tok.strip_prefix("tag:")  { tag_pats.push(rest.to_string());  continue; }
        name_pats.push(tok.to_string());
    }

    hosts.values().filter(|h| {
        let name_ok = if name_pats.is_empty() { true } else { name_pats.iter().all(|p| wildcard_match(p, &h.name)) };
        let host_ok = if host_pats.is_empty() { true } else { host_pats.iter().all(|p| wildcard_match(p, &h.host)) };
        let user_ok = if user_pats.is_empty() { true } else { user_pats.iter().all(|p| wildcard_match(p, &h.username)) };
        let tag_ok  = if tag_pats.is_empty()  { true } else {
            let tags = h.tags.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect::<Vec<_>>()).unwrap_or_default();
            tag_pats.iter().all(|p| tags.iter().any(|t| wildcard_match(p, t)))
        };
        name_ok && host_ok && user_ok && tag_ok
    }).collect()
}

/// Applique un filtre (wildcard) sur une liste de références vers Host.
pub fn apply_filter<'a>(pattern: &str, items: &'a [&'a Host]) -> Vec<&'a Host> {
    if pattern.trim().is_empty() { return items.to_vec(); }
    items.iter().copied().filter(|h| {
        wildcard_match(pattern, &h.name)
            || wildcard_match(pattern, &h.host)
            || wildcard_match(pattern, &h.username)
            || h.tags.as_ref().map(|v| v.iter().any(|t| wildcard_match(pattern, t))).unwrap_or(false)
    }).collect()
}
