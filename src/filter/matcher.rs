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

#[cfg(test)]
mod tests {
    use super::*;

    fn h(name: &str, host: &str, user: &str, tags: &[&str]) -> Host {
        Host {
            name: name.to_string(),
            host: host.to_string(),
            port: 22,
            username: user.to_string(),
            identity_file: None,
            proxy_jump: None,
            tags: if tags.is_empty() {
                None
            } else {
                Some(tags.iter().map(|s| s.to_string()).collect())
            },
            folder: None,
            last_connected_at: None,
            use_count: 0,
            favorite: false,
            tunnels: vec![],
            forward_agent: false,
            mosh: false,
            notes: None,
            remote_command: None,
        }
    }

    #[test]
    fn wildcard_star_matches_anything() {
        assert!(wildcard_match("*", "foobar"));
        assert!(wildcard_match("*", ""));
    }

    #[test]
    fn wildcard_substring_when_no_star() {
        assert!(wildcard_match("foo", "myfoobar"));
        assert!(!wildcard_match("foo", "bar"));
    }

    #[test]
    fn wildcard_prefix_and_suffix() {
        assert!(wildcard_match("foo*", "foobar"));
        assert!(!wildcard_match("foo*", "barfoo"));
        assert!(wildcard_match("*bar", "foobar"));
        assert!(!wildcard_match("*bar", "bartender"));
    }

    #[test]
    fn wildcard_is_case_insensitive() {
        assert!(wildcard_match("FOO", "foo"));
        assert!(wildcard_match("foo*", "FooBar"));
    }

    fn map_of(hs: Vec<Host>) -> std::collections::HashMap<String, Host> {
        hs.into_iter().map(|h| (h.name.clone(), h)).collect()
    }

    #[test]
    fn filter_hosts_empty_returns_all() {
        let m = map_of(vec![h("a", "1", "u", &[])]);
        assert_eq!(filter_hosts(&m, "").len(), 1);
        assert_eq!(filter_hosts(&m, "   ").len(), 1);
    }

    #[test]
    fn filter_hosts_combines_predicates() {
        let m = map_of(vec![
            h("web1", "10.0.0.1", "ubuntu", &["prod"]),
            h("db1",  "10.0.0.2", "root",   &["prod"]),
            h("dev",  "10.1.0.1", "alice",  &["staging"]),
        ]);
        // Bare token => name match
        let r = filter_hosts(&m, "web");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "web1");

        // tag:prod returns 2
        let r = filter_hosts(&m, "tag:prod");
        assert_eq!(r.len(), 2);

        // user:ubuntu tag:prod returns 1
        let r = filter_hosts(&m, "user:ubuntu tag:prod");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "web1");

        // host:10.1* returns "dev"
        let r = filter_hosts(&m, "host:10.1*");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "dev");
    }

    #[test]
    fn apply_filter_fuzzy_matches_and_sorts() {
        let hosts = vec![
            h("web-prod-eu",     "1.1.1.1", "u", &["prod"]),
            h("web-staging-eu",  "1.1.1.2", "u", &["staging"]),
            h("db-prod-us",      "2.2.2.2", "u", &["prod"]),
        ];
        let refs: Vec<&Host> = hosts.iter().collect();
        let r = apply_filter("webprod", &refs);
        assert!(!r.is_empty());
        // The "web-prod-eu" must rank above "web-staging-eu" for "webprod"
        assert_eq!(r[0].name, "web-prod-eu");
    }

    #[test]
    fn apply_filter_with_prefix_token_routes_to_prefixed_path() {
        let hosts = vec![
            h("web", "1.1.1.1", "u", &["prod"]),
            h("db",  "2.2.2.2", "u", &["staging"]),
        ];
        let refs: Vec<&Host> = hosts.iter().collect();
        let r = apply_filter("tag:prod", &refs);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "web");
    }
}
