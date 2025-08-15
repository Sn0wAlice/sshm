use inquire::{Select, Text};
use prettytable::{row, Table};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;

// TUI
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io;
use std::time::Duration;

// For SSH config import
use ssh_config::SSHConfig;

/// Host model (v2)
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Host {
    /// Alias (also used as the HashMap key)
    name: String,
    /// Hostname or IP (formerly `ip`)
    host: String,
    /// SSH port
    port: u16,
    /// SSH username
    username: String,
    /// Path to private key (e.g., ~/.ssh/id_ed25519)
    identity_file: Option<String>,
    /// ProxyJump, e.g., "bastion.example.com:22"
    proxy_jump: Option<String>,
    /// Tags for filtering/grouping
    tags: Option<Vec<String>>,
}

fn tags_to_string(tags: &Option<Vec<String>>) -> String {
    tags.as_ref()
        .map(|v| v.join(","))
        .unwrap_or_else(|| "".to_string())
}

fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });
    base.join("sshm/host.json")
}

fn ensure_config_file(path: &PathBuf) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        let mut f = File::create(path)?;
        f.write_all(b"{}\n")?;
    }
    Ok(())
}

fn load_hosts() -> HashMap<String, Host> {
    let path = config_path();
    if let Err(e) = ensure_config_file(&path) {
        eprintln!("Cannot init config file {}: {e}", path.display());
        return HashMap::new();
    }

    let mut content = String::new();
    match File::open(&path) {
        Ok(mut file) => {
            if let Err(e) = file.read_to_string(&mut content) {
                eprintln!("Error reading {}: {e}", path.display());
                return HashMap::new();
            }
        }
        Err(e) => {
            eprintln!("Cannot open {}: {e}", path.display());
            return HashMap::new();
        }
    }

    // Try parsing as the new schema directly first
    if let Ok(map) = serde_json::from_str::<HashMap<String, Host>>(&content) {
        return map;
    }

    // If that fails, try to migrate from the old schema where the field was `ip`
    // Structure expected: { "alias": { "name": "alias", "ip": "1.2.3.4", "port": 22, "username": "user" } }
    let mut migrated: HashMap<String, Host> = HashMap::new();
    match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(val) => {
            if let Some(obj) = val.as_object() {
                for (alias, entry) in obj {
                    if let Some(e) = entry.as_object() {
                        let name = e.get("name").and_then(|x| x.as_str()).unwrap_or(alias);
                        // Prefer `host`, fallback to legacy `ip`
                        let host = e
                            .get("host")
                            .and_then(|x| x.as_str())
                            .or_else(|| e.get("ip").and_then(|x| x.as_str()))
                            .unwrap_or("");
                        let port = e.get("port").and_then(|x| x.as_u64()).unwrap_or(22) as u16;
                        let username = e.get("username").and_then(|x| x.as_str()).unwrap_or("root");
                        let identity_file = e
                            .get("identity_file")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string());
                        let proxy_jump = e
                            .get("proxy_jump")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string());
                        let tags = e.get("tags").and_then(|x| x.as_array()).map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect::<Vec<_>>()
                        });

                        if !host.is_empty() {
                            migrated.insert(
                                alias.clone(),
                                Host {
                                    name: name.to_string(),
                                    host: host.to_string(),
                                    port,
                                    username: username.to_string(),
                                    identity_file,
                                    proxy_jump,
                                    tags,
                                },
                            );
                        }
                    }
                }
            }
        }
        Err(e) => {
            // Backup invalid JSON and return empty
            let bak = path.with_extension("json.bak");
            if let Err(be) = fs::write(&bak, content) {
                eprintln!("Failed to write backup {}: {be}", bak.display());
            } else {
                eprintln!("Config was invalid JSON. Backed up to {}.", bak.display());
            }
            return HashMap::new();
        }
    }

    // Save back the migrated structure if we migrated anything
    if !migrated.is_empty() {
        eprintln!("Migrated config to new schema (ip -> host, added optional fields). Saving...");
        save_hosts(&migrated);
    }

    migrated
}

/// Import entries from ~/.ssh/config and merge them into our map.
/// Uses the `ssh_config` crate (0.1.x). This crate exposes `parse_str` and
/// `query(alias)`, but does not expose a public iterator over all aliases.
/// To enumerate aliases, we do a simple scan of the raw file for lines
/// starting with `Host` and split patterns, skipping wildcards like `*`/`?`.
fn import_ssh_config(hosts: &mut HashMap<String, Host>) {
    // 1) Read file
    let ssh_path = dirs::home_dir().map(|h| h.join(".ssh/config"));
    let Some(path) = ssh_path.filter(|p| p.exists()) else {
        return;
    };

    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };

    // 2) Parse with ssh_config
    let Ok(cfg) = SSHConfig::parse_str(&text) else {
        return;
    };

    // 3) Extract candidate aliases from raw text (skip wildcards)
    let mut aliases: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Host ") {
            // Split by whitespace and commas; keep only literal aliases (no * ? !)
            for tok in rest.split(|c: char| c.is_whitespace() || c == ',') {
                let alias = tok.trim();
                if alias.is_empty() {
                    continue;
                }
                if alias.contains('*') || alias.contains('?') || alias.starts_with('!') {
                    continue;
                }
                aliases.push(alias.to_string());
            }
        }
    }

    // 4) Dedup & stable sort
    aliases.sort();
    aliases.dedup();

    // 5) For each alias, query settings and import if not present
    for alias in aliases {
        if hosts.contains_key(&alias) {
            continue;
        }
        let settings = cfg.query(&alias);

        // Helpers to fetch a key with a few common variants
        let get = |k: &str| settings.get(k).map(|s| s.to_string());
        let host = get("HostName")
            .or_else(|| get("Hostname"))
            .unwrap_or_else(|| alias.clone());
        let username = get("User")
            .or_else(|| get("Username"))
            .unwrap_or_else(|| "root".into());
        let port = get("Port")
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(22);
        let identity_file = get("IdentityFile");
        let proxy_jump = get("ProxyJump").or_else(|| get("ProxyJump"));

        hosts.insert(
            alias.clone(),
            Host {
                name: alias.clone(),
                host,
                port,
                username,
                identity_file,
                proxy_jump,
                tags: Some(vec!["ssh_config".to_string()]),
            },
        );
    }
}

fn save_hosts(hosts: &HashMap<String, Host>) {
    let path = config_path();
    let json = match serde_json::to_string_pretty(hosts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize hosts: {e}");
            return;
        }
    };

    // Write to a temp file then rename
    let tmp = path.with_extension("json.tmp");
    match File::create(&tmp).and_then(|mut f| f.write_all(json.as_bytes())) {
        Ok(_) => {
            // On Windows, rename fails if dest exists; remove it first
            let _ = fs::remove_file(&path);
            if let Err(e) = fs::rename(&tmp, &path) {
                eprintln!("Failed to move temp file into place: {e}");
                // attempt to clean up
                let _ = fs::remove_file(&tmp);
            }
        }
        Err(e) => {
            eprintln!("Failed to write temp file {}: {e}", tmp.display());
        }
    }
}

/// Simple match insensible à la casse, avec support du '*' (wildcard).
fn wildcard_match(pat: &str, text: &str) -> bool {
    let pat = pat.to_lowercase();
    let text = text.to_lowercase();
    if pat == "*" {
        return true;
    }
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return text.contains(&pat);
    }
    // on vérifie que chaque morceau apparaît dans l'ordre
    let mut idx = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = text[idx..].find(part) {
            idx += found + part.len();
        } else {
            return false;
        }
        // si pas d'étoile finale, dernier morceau doit coller à la fin
        if i == parts.len() - 1 && !pat.ends_with('*') && !text.ends_with(part) {
            // on reste permissif (contains) pour un usage plus simple
        }
    }
    if !pat.starts_with('*') {
        if let Some(first) = parts.iter().find(|s| !s.is_empty()) {
            if !text.starts_with(first) {
                return false;
            }
        }
    }
    if !pat.ends_with('*') {
        if let Some(last) = parts.iter().rfind(|s| !s.is_empty()) {
            if !text.ends_with(last) {
                return false;
            }
        }
    }
    true
}

/// Parse un filtre de type "tag:prod host:10.* name:web user:ubuntu"
/// Clés supportées : tag, host, name, user. Valeurs avec '*' autorisé.
fn filter_hosts<'a>(hosts: &'a HashMap<String, Host>, filter: &str) -> Vec<&'a Host> {
    if filter.trim().is_empty() {
        return hosts.values().collect();
    }
    let mut name_pats: Vec<String> = Vec::new();
    let mut host_pats: Vec<String> = Vec::new();
    let mut user_pats: Vec<String> = Vec::new();
    let mut tag_pats: Vec<String> = Vec::new();

    for tok in filter.split_whitespace() {
        if let Some(rest) = tok.strip_prefix("name:") {
            name_pats.push(rest.to_string());
            continue;
        }
        if let Some(rest) = tok.strip_prefix("host:") {
            host_pats.push(rest.to_string());
            continue;
        }
        if let Some(rest) = tok.strip_prefix("user:") {
            user_pats.push(rest.to_string());
            continue;
        }
        if let Some(rest) = tok.strip_prefix("tag:") {
            tag_pats.push(rest.to_string());
            continue;
        }
        // Token nu ⇒ filtre sur le name
        name_pats.push(tok.to_string());
    }

    hosts
        .values()
        .filter(|h| {
            let name_ok = if name_pats.is_empty() {
                true
            } else {
                name_pats.iter().all(|p| wildcard_match(p, &h.name))
            };
            let host_ok = if host_pats.is_empty() {
                true
            } else {
                host_pats.iter().all(|p| wildcard_match(p, &h.host))
            };
            let user_ok = if user_pats.is_empty() {
                true
            } else {
                user_pats.iter().all(|p| wildcard_match(p, &h.username))
            };
            let tag_ok = if tag_pats.is_empty() {
                true
            } else {
                let tags = h
                    .tags
                    .as_ref()
                    .map(|v| v.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();
                tag_pats
                    .iter()
                    .all(|p| tags.iter().any(|t| wildcard_match(p, t)))
            };
            name_ok && host_ok && user_ok && tag_ok
        })
        .collect()
}

fn list_hosts_with_filter(hosts: &HashMap<String, Host>, filter: Option<String>) {
    use prettytable::{cell, row, Table};

    let mut rows: Vec<&Host> = match filter {
        Some(f) => filter_hosts(hosts, &f),
        None => hosts.values().collect(),
    };
    if rows.is_empty() {
        println!("No hosts match your filter.");
        return;
    }

    rows.sort_by(|a, b| a.name.cmp(&b.name));

    let mut table = Table::new();
    table.add_row(row!["Name", "Username", "Host", "Port", "Tags"]);
    for h in rows {
        table.add_row(row![
            h.name,
            h.username,
            h.host,
            h.port.to_string(),
            tags_to_string(&h.tags)
        ]);
    }
    table.printstd();
}

/// Build and execute the ssh command based on Host settings
/// Construit et exécute la commande ssh en combinant Host + overrides CLI.
fn launch_ssh(h: &Host, overrides: Option<&[String]>) {
    let mut cmd = Command::new("ssh");
    cmd.arg(format!("{}@{}", h.username, h.host))
        .arg("-p")
        .arg(h.port.to_string());

    // Valeurs par défaut depuis la fiche host
    if let Some(id) = &h.identity_file {
        cmd.arg("-i").arg(id);
    }
    if let Some(j) = &h.proxy_jump {
        cmd.arg("-J").arg(j);
    }

    // Overrides/compléments (pass-through): -i, -J, -L/-R/-D, etc.
    if let Some(args) = overrides {
        cmd.args(args);
    }

    let _ = cmd.status();
}

fn connect_host(hosts: &HashMap<String, Host>, name: Option<String>, extra: &[String]) {
    // Choose host: either from CLI arg or via interactive menu
    let name = match name {
        Some(n) => n,
        None => {
            let mut choices: Vec<&String> = hosts.keys().collect();
            choices.sort();
            match Select::new("Choose a host:", choices).prompt() {
                Ok(choice) => choice.to_string(),
                Err(_) => return,
            }
        }
    };

    // Prefer exact alias match; fallback to substring search on alias
    if let Some(h) = hosts.get(&name) {
        launch_ssh(h, Some(extra));
        return;
    }

    let matching: Vec<&Host> = hosts.values().filter(|h| h.name.contains(&name)).collect();

    match matching.len() {
        0 => println!("No matching host."),
        1 => launch_ssh(matching[0], Some(extra)),
        _ => {
            let options: Vec<String> = matching.iter().map(|h| h.name.clone()).collect();
            if let Ok(choice) = Select::new("Multiple matches. Choose:", options).prompt() {
                connect_host(hosts, Some(choice), extra);
            }
        }
    }
}

/// Applique un filtre (wildcard) sur une liste de références vers Host.
/// On renvoie un Vec<&Host> avec des durées de vie explicitement liées à l'input.
fn apply_filter<'a>(pattern: &str, items: &'a [&'a Host]) -> Vec<&'a Host> {
    if pattern.trim().is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .copied()
        .filter(|h| {
            wildcard_match(pattern, &h.name)
                || wildcard_match(pattern, &h.host)
                || wildcard_match(pattern, &h.username)
                || h.tags
                    .as_ref()
                    .map(|v| v.iter().any(|t| wildcard_match(pattern, t)))
                    .unwrap_or(false)
        })
        .collect()
}

fn run_tui(hosts: &mut HashMap<String, Host>) {
    let mut items: Vec<&Host> = hosts.values().collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    let mut filter = String::new();
    let mut filtered: Vec<&Host> = items.clone();
    let mut selected: usize = 0;
    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    enable_raw_mode().ok();
    execute!(io::stdout(), EnterAlternateScreen).ok();
    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
                .split(size);

            let list_items: Vec<ListItem> = filtered.iter()
                .map(|h| ListItem::new(format!("{}  {}", h.name, h.host)))
                .collect();
            let list = List::new(list_items)
                .block(Block::default().title("Hosts (↑/↓, / filtre, Enter connect, q)").borders(Borders::ALL))
                .highlight_symbol("➜ ")
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            // ensure state points to current selection
            list_state.select(if filtered.is_empty() { None } else { Some(selected) });
            f.render_stateful_widget(list, chunks[0], &mut list_state);

            if let Some(h) = filtered.get(selected) {
                let detail = format!(
                    "Name: {}\nUser: {}\nHost: {}\nPort: {}\nTags: {}\nIdentityFile: {}\nProxyJump: {}",
                    h.name, h.username, h.host, h.port,
                    tags_to_string(&h.tags),
                    h.identity_file.clone().unwrap_or_default(),
                    h.proxy_jump.clone().unwrap_or_default()
                );
                let p = Paragraph::new(detail)
                    .block(Block::default().title("Details").borders(Borders::ALL));
                f.render_widget(p, chunks[1]);
            }
        }).ok();

        if event::poll(Duration::from_millis(150)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Down => {
                            if !filtered.is_empty() {
                                selected = (selected + 1) % filtered.len();
                                list_state.select(Some(selected));
                            }
                        }
                        KeyCode::Up => {
                            if !filtered.is_empty() {
                                selected = (selected + filtered.len() - 1) % filtered.len();
                                list_state.select(Some(selected));
                            }
                        }
                        KeyCode::Char('/') => {
                            filter.clear();
                            filtered = apply_filter(&filter, &items);
                            selected = 0;
                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                        }
                        KeyCode::Backspace => {
                            filter.pop();
                            filtered = apply_filter(&filter, &items);
                            selected = 0;
                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                        }
                        KeyCode::Enter => {
                            if let Some(h) = filtered.get(selected) {
                                disable_raw_mode().ok();
                                execute!(io::stdout(), LeaveAlternateScreen).ok();
                                launch_ssh(h, None);
                                enable_raw_mode().ok();
                                execute!(io::stdout(), EnterAlternateScreen).ok();
                            }
                        }
                        KeyCode::Char(c) => {
                            if filter.is_empty() {
                                match c {
                                    'q' | 'Q' => break,
                                    'e' => {
                                        if let Some(h) = filtered.get(selected) {
                                            let current = h.name.clone();
                                            disable_raw_mode().ok();
                                            execute!(io::stdout(), LeaveAlternateScreen).ok();
                                            edit_host_by_name(hosts, &current);
                                            enable_raw_mode().ok();
                                            execute!(io::stdout(), EnterAlternateScreen).ok();
                                            items = hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            if filtered.is_empty() {
                                                list_state.select(None);
                                            } else {
                                                list_state.select(Some(0));
                                            }
                                        }
                                    }
                                    'r' => {
                                        if let Some(h) = filtered.get(selected) {
                                            let current = h.name.clone();
                                            disable_raw_mode().ok();
                                            execute!(io::stdout(), LeaveAlternateScreen).ok();
                                            rename_host(hosts, &current);
                                            enable_raw_mode().ok();
                                            execute!(io::stdout(), EnterAlternateScreen).ok();
                                            items = hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            if filtered.is_empty() {
                                                list_state.select(None);
                                            } else {
                                                list_state.select(Some(0));
                                            }
                                        }
                                    }
                                    'd' => {
                                        if let Some(h) = filtered.get(selected) {
                                            disable_raw_mode().ok();
                                            execute!(io::stdout(), LeaveAlternateScreen).ok();
                                            delete_host(hosts);
                                            enable_raw_mode().ok();
                                            execute!(io::stdout(), EnterAlternateScreen).ok();
                                            items = hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            if filtered.is_empty() {
                                                list_state.select(None);
                                            } else {
                                                list_state.select(Some(0));
                                            }
                                        }
                                    }
                                    _ => filter.clear(),
                                }
                            } else {
                                filter.push(c);
                                filtered = apply_filter(&filter, &items);
                                selected = 0;
                                list_state.select(if filtered.is_empty() { None } else { Some(0) });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen).ok();
}

fn create_host(hosts: &mut HashMap<String, Host>) {
    let name = Text::new("Name (alias):").prompt().unwrap();
    let host = Text::new("Host (hostname or IP):").prompt().unwrap();
    let port: u16 = Text::new("Port:")
        .with_initial_value("22")
        .prompt()
        .unwrap()
        .parse()
        .unwrap_or(22);
    let username = Text::new("Username:")
        .with_initial_value("root")
        .prompt()
        .unwrap();
    let identity_file = {
        let v = Text::new("Identity file (optional):")
            .with_initial_value("")
            .prompt()
            .unwrap();
        if v.trim().is_empty() {
            None
        } else {
            Some(v)
        }
    };
    let proxy_jump = {
        let v = Text::new("ProxyJump, e.g. bastion:22 (optional):")
            .with_initial_value("")
            .prompt()
            .unwrap();
        if v.trim().is_empty() {
            None
        } else {
            Some(v)
        }
    };
    let tags = {
        let v = Text::new("Tags (comma-separated, optional):")
            .with_initial_value("")
            .prompt()
            .unwrap();
        let v = v.trim();
        if v.is_empty() {
            None
        } else {
            Some(
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            )
        }
    };

    hosts.insert(
        name.clone(),
        Host {
            name: name.clone(),
            host,
            port,
            username,
            identity_file,
            proxy_jump,
            tags,
        },
    );
    save_hosts(hosts);
}

fn delete_host(hosts: &mut HashMap<String, Host>) {
    let choices: Vec<_> = hosts.keys().cloned().collect(); // cloner les clés
    if let Ok(choice) = Select::new("Choose host to delete:", choices).prompt() {
        hosts.remove(&choice); // on utilise une &String, aucun conflit
        save_hosts(hosts);
    }
}

fn edit_host(hosts: &mut HashMap<String, Host>) {
    let mut choices: Vec<String> = hosts.keys().cloned().collect();
    choices.sort();
    if let Ok(choice) = Select::new("Choose host to edit:", choices).prompt() {
        if let Some(host) = hosts.get_mut(&choice) {
            host.host = Text::new("New Host:")
                .with_initial_value(&host.host)
                .prompt()
                .unwrap();
            host.port = Text::new("New Port:")
                .with_initial_value(&host.port.to_string())
                .prompt()
                .unwrap()
                .parse()
                .unwrap_or(22);
            host.username = Text::new("New Username:")
                .with_initial_value(&host.username)
                .prompt()
                .unwrap();
            // Optional fields
            let id_init = host.identity_file.clone().unwrap_or_default();
            let pj_init = host.proxy_jump.clone().unwrap_or_default();
            let tags_init = tags_to_string(&host.tags);

            let id = Text::new("Identity file (optional):")
                .with_initial_value(&id_init)
                .prompt()
                .unwrap();
            host.identity_file = if id.trim().is_empty() { None } else { Some(id) };

            let pj = Text::new("ProxyJump (optional):")
                .with_initial_value(&pj_init)
                .prompt()
                .unwrap();
            host.proxy_jump = if pj.trim().is_empty() { None } else { Some(pj) };

            let tags = Text::new("Tags (comma-separated, optional):")
                .with_initial_value(&tags_init)
                .prompt()
                .unwrap();
            host.tags = if tags.trim().is_empty() {
                None
            } else {
                Some(
                    tags.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                )
            };

            save_hosts(hosts);
        }
    }
}

fn edit_host_by_name(hosts: &mut HashMap<String, Host>, key: &str) {
    if let Some(host) = hosts.get_mut(key) {
        host.host = Text::new("New Host:")
            .with_initial_value(&host.host)
            .prompt()
            .unwrap();
        host.port = Text::new("New Port:")
            .with_initial_value(&host.port.to_string())
            .prompt()
            .unwrap()
            .parse()
            .unwrap_or(22);
        host.username = Text::new("New Username:")
            .with_initial_value(&host.username)
            .prompt()
            .unwrap();
        // Optional fields
        let id_init = host.identity_file.clone().unwrap_or_default();
        let pj_init = host.proxy_jump.clone().unwrap_or_default();
        let tags_init = tags_to_string(&host.tags);

        let id = Text::new("Identity file (optional):")
            .with_initial_value(&id_init)
            .prompt()
            .unwrap();
        host.identity_file = if id.trim().is_empty() { None } else { Some(id) };

        let pj = Text::new("ProxyJump (optional):")
            .with_initial_value(&pj_init)
            .prompt()
            .unwrap();
        host.proxy_jump = if pj.trim().is_empty() { None } else { Some(pj) };

        let tags = Text::new("Tags (comma-separated, optional):")
            .with_initial_value(&tags_init)
            .prompt()
            .unwrap();
        host.tags = if tags.trim().is_empty() {
            None
        } else {
            Some(
                tags.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            )
        };

        save_hosts(hosts);
    }
}

fn rename_host(hosts: &mut HashMap<String, Host>, old: &str) {
    if !hosts.contains_key(old) {
        return;
    }
    let new = match Text::new("New name (alias):")
        .with_initial_value(old)
        .prompt()
    {
        Ok(v) => v,
        Err(_) => return,
    };
    if new.trim().is_empty() || new == old {
        return;
    }
    if hosts.contains_key(&new) {
        eprintln!("Alias '{}' already exists.", new);
        return;
    }
    if let Some(mut h) = hosts.remove(old) {
        h.name = new.clone();
        hosts.insert(new, h);
        save_hosts(hosts);
    }
}

fn tag_add(hosts: &mut HashMap<String, Host>, name: String, tags: Vec<String>) {
    if let Some(h) = hosts.get_mut(&name) {
        let mut set = h.tags.take().unwrap_or_default();
        for t in tags {
            if !set.iter().any(|e| e.eq_ignore_ascii_case(&t)) {
                set.push(t);
            }
        }
        h.tags = if set.is_empty() { None } else { Some(set) };
        save_hosts(hosts);
        println!("Tags added to {}.", name);
    } else {
        println!("Host '{}' not found.", name);
    }
}

fn tag_del(hosts: &mut HashMap<String, Host>, name: String, tags: Vec<String>) {
    if let Some(h) = hosts.get_mut(&name) {
        if let Some(mut set) = h.tags.take() {
            set.retain(|t| !tags.iter().any(|x| t.eq_ignore_ascii_case(x)));
            h.tags = if set.is_empty() { None } else { Some(set) };
            save_hosts(hosts);
            println!("Tags removed from {}.", name);
        }
    } else {
        println!("Host '{}' not found.", name);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut hosts = load_hosts();

    // Auto-import from ~/.ssh/config on startup (non-destructive)
    let before = hosts.len();
    import_ssh_config(&mut hosts);
    if hosts.len() > before {
        save_hosts(&hosts);
    }

    match args.get(1).map(String::as_str) {
        Some("list") => {
            let filt = if args.get(2).map(String::as_str) == Some("--filter") {
                args.get(3).cloned()
            } else {
                None
            };
            list_hosts_with_filter(&hosts, filt);
        }
        Some("connect") | Some("c") => {
            let name = args.get(2).cloned();
            let extras: Vec<String> = if name.is_some() {
                args[3..].to_vec()
            } else {
                args[2..].to_vec()
            };
            connect_host(&hosts, name, &extras);
        }
        Some("create") => create_host(&mut hosts),
        Some("delete") => delete_host(&mut hosts),
        Some("edit") => edit_host(&mut hosts),
        Some("tag") => match (args.get(2).map(String::as_str), args.get(3), args.get(4)) {
            (Some("add"), Some(name), Some(tlist)) => {
                let tags: Vec<String> = tlist
                    .split(',')
                    .flat_map(|s| s.split_whitespace())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                tag_add(&mut hosts, name.clone(), tags);
            }
            (Some("del"), Some(name), Some(tlist)) => {
                let tags: Vec<String> = tlist
                    .split(',')
                    .flat_map(|s| s.split_whitespace())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                tag_del(&mut hosts, name.clone(), tags);
            }
            _ => {
                println!("Usage: sshm tag [add|del] <name> <tag1,tag2,...>");
            }
        },
        Some("tui") => run_tui(&mut hosts),
        Some("help") => {
            println!("Usage:");
            println!("  sshm list [--filter \"expr\"]");
            println!("  sshm connect (c) <name> [overrides...]   # pass -i, -J, -L/-R/-D etc.");
            println!("  sshm create | edit | delete");
            println!("  sshm tag add <name> <tag1,tag2> | tag del <name> <tag1,tag2>");
            println!("  sshm tui");
        }
        _ => println!("Usage: sshm [list|connect|c|create|edit|delete|tag|tui]"),
    }
}
