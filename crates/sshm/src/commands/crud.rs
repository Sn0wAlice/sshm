use crate::config::io::save_db;
use crate::models::{
    invalid_ssh_option, parse_ssh_options, ssh_options_to_string, tags_to_string, Database, Host,
};
use inquire::{Select, Text};
use std::collections::HashMap;

/// Create either a Host (in the current folder) or a Folder.
pub fn create(db: &mut Database, current_folder: Option<String>) {
    let choice = match Select::new("Create:", vec!["Host", "Folder"]).prompt() {
        Ok(c) => c,
        Err(_) => return,
    };

    match choice {
        "Host" => {
            create_host(&mut db.hosts, current_folder);
            save_db(db);
        }
        "Folder" => create_folder(db),
        _ => {}
    }
}

fn create_folder(db: &mut Database) {
    let name = match Text::new("Folder name:").prompt() {
        Ok(n) => n,
        Err(_) => return,
    };
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    if !db.folders.iter().any(|f| f.eq_ignore_ascii_case(name)) {
        db.folders.push(name.to_string());
        db.folders.sort();
        db.folders.dedup();
        save_db(db);
    }
}

pub fn create_host(hosts: &mut HashMap<String, Host>, current_folder: Option<String>) {
    let Some(name) = ask("Name (alias):", "") else {
        return;
    };
    let name = name.trim().to_string();
    if name.is_empty() {
        eprintln!("Name cannot be empty.");
        return;
    }
    if hosts.contains_key(&name) {
        eprintln!("Alias '{}' already exists.", name);
        return;
    }

    let Some(host) = ask("Host (hostname or IP):", "") else {
        return;
    };
    let host = host.trim().to_string();
    if host.is_empty() {
        eprintln!("Host cannot be empty.");
        return;
    }

    let Some(port_raw) = ask("Port:", "22") else {
        return;
    };
    let port: u16 = port_raw.trim().parse().unwrap_or(22);

    let Some(username) = ask("Username:", "root") else {
        return;
    };
    let Some(identity_file) = ask("Identity file (optional):", "") else {
        return;
    };
    let Some(proxy_jump) = ask("ProxyJump, e.g. bastion:22 (optional):", "") else {
        return;
    };
    let Some(tags) = ask("Tags (comma-separated, optional):", "") else {
        return;
    };
    let Some(ssh_options_raw) = ask("ssh -o options, ;-separated (optional):", "") else {
        return;
    };

    let ssh_options = parse_ssh_options(&ssh_options_raw);
    if let Some(bad) = invalid_ssh_option(&ssh_options) {
        eprintln!("ssh option '{bad}' is not in keyword=value form (e.g. ServerAliveInterval=30).");
        return;
    }

    hosts.insert(
        name.clone(),
        Host {
            name,
            host,
            port,
            username: opt(&username).unwrap_or_else(|| "root".to_string()),
            identity_file: opt(&identity_file),
            proxy_jump: opt(&proxy_jump),
            folder: current_folder,
            tags: split_tags(&tags),
            ssh_options,
            ..Default::default()
        },
    );
}

/// Prompt for one value. `None` means the user cancelled (Esc / Ctrl-C), and
/// every caller treats that as "abandon the whole operation" — the alternative
/// is committing a half-filled host.
fn ask(prompt: &str, initial: &str) -> Option<String> {
    Text::new(prompt).with_initial_value(initial).prompt().ok()
}

/// Trimmed value, or `None` when the user left the field blank.
fn opt(v: &str) -> Option<String> {
    let v = v.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// Parse the comma-separated tags field.
fn split_tags(v: &str) -> Option<Vec<String>> {
    let v: Vec<String> = v
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Delete either a Host or a Folder (when deleting a folder, move its hosts to root).
pub fn delete(db: &mut Database) {
    let choice = match Select::new("Delete:", vec!["Host", "Folder"]).prompt() {
        Ok(c) => c,
        Err(_) => return,
    };

    match choice {
        "Host" => {
            delete_host(&mut db.hosts);
            save_db(db);
        }
        "Folder" => delete_folder(db),
        _ => {}
    }
}

pub fn delete_host(hosts: &mut HashMap<String, Host>) {
    let mut choices: Vec<String> = hosts.keys().cloned().collect();
    choices.sort();
    if choices.is_empty() {
        println!("No hosts to delete.");
        return;
    }
    if let Ok(choice) = Select::new("Choose host to delete:", choices).prompt() {
        hosts.remove(&choice);
    }
}

fn delete_folder(db: &mut Database) {
    if db.folders.is_empty() {
        println!("No folders to delete.");
        return;
    }
    let mut choices = db.folders.clone();
    choices.sort();
    if let Ok(choice) = Select::new("Choose folder to delete:", choices).prompt() {
        // Migrate folder's hosts to root
        let mut moved = 0usize;
        for h in db.hosts.values_mut() {
            if h.folder.as_deref() == Some(choice.as_str()) {
                h.folder = None;
                moved += 1;
            }
        }
        // Remove folder
        db.folders.retain(|f| f != &choice);
        save_db(db);
        println!(
            "Deleted folder '{}' (moved {} host(s) to root)",
            choice, moved
        );
    }
}

pub fn edit_host(db: &mut Database) {
    let mut choices: Vec<String> = db.hosts.keys().cloned().collect();
    choices.sort();
    if let Ok(choice) = Select::new("Choose host to edit:", choices).prompt() {
        edit_host_by_name(&mut db.hosts, &choice);
        save_db(db);
    }
}

pub fn edit_host_by_name(hosts: &mut HashMap<String, Host>, key: &str) {
    let Some(host) = hosts.get(key) else { return };

    // Gather every answer before touching the host: cancelling at the last
    // prompt must leave the entry exactly as it was, not half-edited.
    let Some(new_host) = ask("New Host:", &host.host) else {
        return;
    };
    let Some(port_raw) = ask("New Port:", &host.port.to_string()) else {
        return;
    };
    let Some(username) = ask("New Username:", &host.username) else {
        return;
    };
    let Some(id) = ask(
        "Identity file (optional):",
        &host.identity_file.clone().unwrap_or_default(),
    ) else {
        return;
    };
    let Some(pj) = ask(
        "ProxyJump (optional):",
        &host.proxy_jump.clone().unwrap_or_default(),
    ) else {
        return;
    };
    let Some(folder) = ask(
        "Folder (empty = All):",
        &host.folder.clone().unwrap_or_default(),
    ) else {
        return;
    };
    let Some(tags) = ask(
        "Tags (comma-separated, optional):",
        &tags_to_string(&host.tags),
    ) else {
        return;
    };
    let Some(ssh_options_raw) = ask(
        "ssh -o options, ;-separated (optional):",
        &ssh_options_to_string(&host.ssh_options),
    ) else {
        return;
    };

    let new_host_value = new_host.trim().to_string();
    if new_host_value.is_empty() {
        eprintln!("Host cannot be empty — nothing changed.");
        return;
    }
    let ssh_options = parse_ssh_options(&ssh_options_raw);
    if let Some(bad) = invalid_ssh_option(&ssh_options) {
        eprintln!("ssh option '{bad}' is not in keyword=value form — nothing changed.");
        return;
    }

    let Some(host) = hosts.get_mut(key) else {
        return;
    };
    host.host = new_host_value;
    host.port = port_raw.trim().parse().unwrap_or(22);
    host.username = opt(&username).unwrap_or_else(|| "root".to_string());
    host.identity_file = opt(&id);
    host.proxy_jump = opt(&pj);
    host.folder = opt(&folder);
    host.tags = split_tags(&tags);
    host.ssh_options = ssh_options;
}

pub fn rename_host(hosts: &mut HashMap<String, Host>, old: &str) {
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
    }
}

pub fn rename_folder(db: &mut Database) {
    if db.folders.is_empty() {
        println!("No folders available to rename.");
        return;
    }

    let mut folders = db.folders.clone();
    folders.sort();

    let old = match Select::new("Choose folder to rename:", folders).prompt() {
        Ok(v) => v,
        Err(_) => return,
    };

    let new = match Text::new("New folder name:")
        .with_initial_value(&old)
        .prompt()
    {
        Ok(v) => v.trim().to_string(),
        Err(_) => return,
    };

    if new.is_empty() || new == old {
        return;
    }

    if db.folders.iter().any(|f| f.eq_ignore_ascii_case(&new)) {
        eprintln!("Folder '{}' already exists.", new);
        return;
    }

    // Update folder list
    for f in db.folders.iter_mut() {
        if f == &old {
            *f = new.clone();
        }
    }
    db.folders.sort();
    db.folders.dedup();

    // Update hosts in folder
    for h in db.hosts.values_mut() {
        if h.folder.as_deref() == Some(old.as_str()) {
            h.folder = Some(new.clone());
        }
    }

    save_db(db);
    println!("Folder '{}' renamed to '{}'", old, new);
}
