use std::collections::HashMap;
use inquire::{Select, Text};
use crate::models::{Host, Database, tags_to_string};
use crate::config::io::save_db;

/// Create either a Host (in the current folder) or a Folder.
pub fn create(db: &mut Database, current_folder: Option<String>) {
    let choice = match Select::new("Create:", vec!["Host", "Folder"]).prompt() {
        Ok(c) => c,
        Err(_) => return,
    };

    match choice {
        "Host" => { create_host(&mut db.hosts, current_folder); save_db(db); }
        "Folder" => create_folder(db),
        _ => {}
    }
}

fn create_folder(db: &mut Database) {
    let name = match Text::new("Folder name:").prompt() { Ok(n) => n, Err(_) => return };
    let name = name.trim();
    if name.is_empty() { return; }
    if !db.folders.iter().any(|f| f.eq_ignore_ascii_case(name)) {
        db.folders.push(name.to_string());
        db.folders.sort();
        db.folders.dedup();
        save_db(db);
    }
}

pub fn create_host(hosts: &mut HashMap<String, Host>, current_folder: Option<String>) {
    let name = Text::new("Name (alias):").prompt().unwrap();
    let host = Text::new("Host (hostname or IP):").prompt().unwrap();
    let port: u16 = Text::new("Port:").with_initial_value("22").prompt().unwrap().parse().unwrap_or(22);
    let username = Text::new("Username:").with_initial_value("root").prompt().unwrap();
    let identity_file = {
        let v = Text::new("Identity file (optional):").with_initial_value("").prompt().unwrap();
        if v.trim().is_empty() { None } else { Some(v) }
    };
    let proxy_jump = {
        let v = Text::new("ProxyJump, e.g. bastion:22 (optional):").with_initial_value("").prompt().unwrap();
        if v.trim().is_empty() { None } else { Some(v) }
    };
    let tags = {
        let v = Text::new("Tags (comma-separated, optional):").with_initial_value("").prompt().unwrap();
        let v = v.trim();
        if v.is_empty() { None } else {
            Some(v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        }
    };

    let folder = current_folder.clone();
    hosts.insert(name.clone(), Host { name: name.clone(), host, port, username, identity_file, proxy_jump, folder, tags });
}

/// Delete either a Host or a Folder (when deleting a folder, move its hosts to root).
pub fn delete(db: &mut Database) {
    let choice = match Select::new("Delete:", vec!["Host", "Folder"]).prompt() {
        Ok(c) => c,
        Err(_) => return,
    };

    match choice {
        "Host" => { delete_host(&mut db.hosts); save_db(db); }
        "Folder" => delete_folder(db),
        _ => {}
    }
}

pub fn delete_host(hosts: &mut HashMap<String, Host>) {
    let mut choices: Vec<String> = hosts.keys().cloned().collect();
    choices.sort();
    if choices.is_empty() { println!("No hosts to delete."); return; }
    if let Ok(choice) = Select::new("Choose host to delete:", choices).prompt() {
        hosts.remove(&choice);
    }
}

fn delete_folder(db: &mut Database) {
    if db.folders.is_empty() { println!("No folders to delete."); return; }
    let mut choices = db.folders.clone();
    choices.sort();
    if let Ok(choice) = Select::new("Choose folder to delete:", choices).prompt() {
        // Migrate folder's hosts to root
        let mut moved = 0usize;
        for h in db.hosts.values_mut() {
            if h.folder.as_deref() == Some(choice.as_str()) { h.folder = None; moved += 1; }
        }
        // Remove folder
        db.folders.retain(|f| f != &choice);
        save_db(db);
        println!("Deleted folder '{}' (moved {} host(s) to root)", choice, moved);
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
    if let Some(host) = hosts.get_mut(key) {
        host.host = Text::new("New Host:").with_initial_value(&host.host).prompt().unwrap();
        host.port = Text::new("New Port:").with_initial_value(&host.port.to_string()).prompt().unwrap().parse().unwrap_or(22);
        host.username = Text::new("New Username:").with_initial_value(&host.username).prompt().unwrap();

        let id_init = host.identity_file.clone().unwrap_or_default();
        let pj_init = host.proxy_jump.clone().unwrap_or_default();
        let tags_init = tags_to_string(&host.tags);
        let folder_init = host.folder.clone().unwrap_or_default();

        let id = Text::new("Identity file (optional):").with_initial_value(&id_init).prompt().unwrap();
        host.identity_file = if id.trim().is_empty() { None } else { Some(id) };

        let pj = Text::new("ProxyJump (optional):").with_initial_value(&pj_init).prompt().unwrap();
        host.proxy_jump = if pj.trim().is_empty() { None } else { Some(pj) };

        let folder = Text::new("Folder (empty = All):").with_initial_value(&folder_init).prompt().unwrap();
        host.folder = if folder.trim().is_empty() { None } else { Some(folder) };

        let tags = Text::new("Tags (comma-separated, optional):").with_initial_value(&tags_init).prompt().unwrap();
        host.tags = if tags.trim().is_empty() { None } else {
            Some(tags.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        };
    }
}

pub fn rename_host(hosts: &mut HashMap<String, Host>, old: &str) {
    if !hosts.contains_key(old) { return; }
    let new = match Text::new("New name (alias):").with_initial_value(old).prompt() {
        Ok(v) => v,
        Err(_) => return,
    };
    if new.trim().is_empty() || new == old { return; }
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

    let new = match Text::new("New folder name:").with_initial_value(&old).prompt() {
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
