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
            let mut staged = db.hosts.clone();
            create_host(&mut staged, current_folder);
            commit_hosts(db, staged);
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
    if name.is_empty() || !refresh(db) {
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
            let mut staged = db.hosts.clone();
            delete_host(&mut staged);
            commit_hosts(db, staged);
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
        if !refresh(db) {
            return;
        }
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
        let mut staged = db.hosts.clone();
        edit_host_by_name(&mut staged, &choice);
        commit_hosts(db, staged);
    }
}

/// Pick up whatever another sshm (TUI, sync) wrote while a prompt was open,
/// so the save that follows doesn't overwrite it. `false` (after explaining)
/// when host.json can't be read right now — saving then would clobber it.
fn refresh(db: &mut Database) -> bool {
    match db.reload_if_changed() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Not saved: {e:#}");
            false
        }
    }
}

/// Apply the host edits made on `staged` (a copy taken before the prompts) on
/// top of a fresh reload, then save. Only the entries the user actually
/// touched are written, so concurrent changes to other hosts survive.
fn commit_hosts(db: &mut Database, staged: HashMap<String, Host>) {
    let before = db.hosts.clone();
    let changes = host_changes(&before, &staged);
    if changes.is_empty() || !refresh(db) {
        return;
    }
    if apply_host_changes(&before, &mut db.hosts, changes) {
        save_db(db);
    }
}

/// Apply `changes` onto the freshly reloaded `hosts`. `false` = abort, save nothing.
fn apply_host_changes(
    before: &HashMap<String, Host>,
    hosts: &mut HashMap<String, Host>,
    changes: Vec<(String, Option<Host>)>,
) -> bool {
    for (key, new) in changes {
        // A brand-new alias that appeared on disk meanwhile: don't clobber it.
        if !before.contains_key(&key) && hosts.contains_key(&key) {
            eprintln!("Alias '{key}' was just created elsewhere — nothing changed.");
            return false;
        }
        match new {
            Some(mut h) => {
                if before.contains_key(&key) {
                    // Edited an existing host: don't resurrect it if it was
                    // deleted meanwhile, and keep the history recorded since.
                    let Some(cur) = hosts.get(&key) else {
                        eprintln!("Host '{key}' was deleted elsewhere — edit skipped.");
                        continue;
                    };
                    h.last_connected_at = cur.last_connected_at.clone();
                    h.use_count = cur.use_count;
                }
                hosts.insert(key, h);
            }
            None => {
                hosts.remove(&key);
            }
        }
    }
    true
}

/// Entries added/changed (`Some`) or removed (`None`) going from `before` to `after`.
fn host_changes(
    before: &HashMap<String, Host>,
    after: &HashMap<String, Host>,
) -> Vec<(String, Option<Host>)> {
    let mut out: Vec<(String, Option<Host>)> = after
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect();
    out.extend(
        before
            .keys()
            .filter(|k| !after.contains_key(*k))
            .map(|k| (k.clone(), None)),
    );
    out
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

    if new.is_empty() || new == old || !refresh(db) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn h(addr: &str) -> Host {
        Host {
            name: "x".into(),
            host: addr.into(),
            ..Default::default()
        }
    }

    #[test]
    fn host_changes_reports_only_what_the_user_touched() {
        let before: HashMap<String, Host> = [("a".into(), h("1")), ("b".into(), h("2"))]
            .into_iter()
            .collect();
        let mut after = before.clone();
        after.remove("a");
        after.insert("c".into(), h("3"));
        let mut changes = host_changes(&before, &after);
        changes.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0], ("a".into(), None));
        assert_eq!(changes[1].0, "c");
    }

    #[test]
    fn an_edit_keeps_fresh_history_and_skips_hosts_deleted_meanwhile() {
        let before: HashMap<String, Host> = [("a".into(), h("1")), ("b".into(), h("2"))]
            .into_iter()
            .collect();
        // On disk meanwhile: `a` was connected to, `b` deleted.
        let mut hosts = before.clone();
        hosts.get_mut("a").unwrap().use_count = 5;
        hosts.remove("b");
        let changes = vec![("a".into(), Some(h("9"))), ("b".into(), Some(h("8")))];
        assert!(apply_host_changes(&before, &mut hosts, changes));
        assert_eq!(hosts["a"].host, "9");
        assert_eq!(hosts["a"].use_count, 5);
        assert!(!hosts.contains_key("b"));
    }
}
