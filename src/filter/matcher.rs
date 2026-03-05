use std::collections::HashMap;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use crate::models::Host;

/// Simple match insensible à la casse, avec support du '*' (wildcard).
/// Used by CLI commands.
pub fn wildcard_match(pat: &str, text: &str) -> bool {
    let pat = pat.to_lowercase();
    let text = text.to_lowercase();
    if pat == "*" { return true; }
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 { return text.contains(&pat); }
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
/// Used by CLI commands.
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

/// Fuzzy score for a single host against the pattern. Returns best score across all fields.
fn fuzzy_score(matcher: &SkimMatcherV2, h: &Host, pattern: &str) -> Option<i64> {
    let tag_str = h.tags.as_ref().map(|v| v.join(" ")).unwrap_or_default();
    [
        matcher.fuzzy_match(&h.name, pattern),
        matcher.fuzzy_match(&h.host, pattern),
        matcher.fuzzy_match(&h.username, pattern),
        matcher.fuzzy_match(&tag_str, pattern),
    ]
    .into_iter()
    .flatten()
    .max()
}

/// Fuzzy filter (TUI). Matches across name/host/username/tags, sorted by relevance.
pub fn apply_filter<'a>(pattern: &str, items: &'a [&'a Host]) -> Vec<&'a Host> {
    if pattern.trim().is_empty() { return items.to_vec(); }

    // Check for prefix syntax — fall back to wildcard-based prefix filter
    let has_prefix = pattern.split_whitespace().any(|tok|
        tok.starts_with("name:") || tok.starts_with("host:")
        || tok.starts_with("user:") || tok.starts_with("tag:")
    );
    if has_prefix {
        return apply_filter_prefixed(pattern, items);
    }

    let matcher = SkimMatcherV2::default().smart_case();
    let mut scored: Vec<(&Host, i64)> = items.iter().copied()
        .filter_map(|h| fuzzy_score(&matcher, h, pattern).map(|s| (h, s)))
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(h, _)| h).collect()
}

/// Prefix-based filter (name:, host:, user:, tag:) using fuzzy matching per field.
fn apply_filter_prefixed<'a>(filter: &str, items: &'a [&'a Host]) -> Vec<&'a Host> {
    let matcher = SkimMatcherV2::default().smart_case();
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

    items.iter().copied().filter(|h| {
        let name_ok = name_pats.is_empty() || name_pats.iter().all(|p| matcher.fuzzy_match(&h.name, p).is_some());
        let host_ok = host_pats.is_empty() || host_pats.iter().all(|p| matcher.fuzzy_match(&h.host, p).is_some());
        let user_ok = user_pats.is_empty() || user_pats.iter().all(|p| matcher.fuzzy_match(&h.username, p).is_some());
        let tag_ok = tag_pats.is_empty() || {
            let tags = h.tags.as_ref().map(|v| v.join(" ")).unwrap_or_default();
            tag_pats.iter().all(|p| matcher.fuzzy_match(&tags, p).is_some())
        };
        name_ok && host_ok && user_ok && tag_ok
    }).collect()
}
