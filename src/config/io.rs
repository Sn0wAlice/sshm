use std::{collections::{HashMap, BTreeMap}, fs::File, io::Read};
use std::fs;
use serde::Serialize;
use serde_json;
use crate::models::{Host, Database};
use super::path::{config_path, ensure_config_file};

/// Load the full Database (hosts + folders). Handles migration from legacy formats.
pub fn load_db() -> Database {
    let path = config_path();
    println!("Loading DB from {}", path.display());
    if let Err(e) = ensure_config_file(&path) {
        eprintln!("Cannot init config file {}: {e}", path.display());
        save_empty_database();
        return Database { hosts: Default::default(), folders: vec![] };
    }

    let mut content = String::new();
    match File::open(&path) {
        Ok(mut file) => {
            if let Err(e) = file.read_to_string(&mut content) {
                eprintln!("Error reading {}: {e}", path.display());
                save_empty_database();
                return Database { hosts: Default::default(), folders: vec![] };
            }
        }
        Err(e) => {
            eprintln!("Cannot open {}: {e}", path.display());
            save_empty_database();
            return Database { hosts: Default::default(), folders: vec![] };
        }
    }

    // Try current schema first
    if let Ok(db) = serde_json::from_str::<Database>(&content) {
        return db;
    }

    // Try legacy map-only schema
    if let Ok(map) = serde_json::from_str::<HashMap<String, Host>>(&content) {
        let mut folders: Vec<String> = map.values().filter_map(|h| h.folder.clone()).collect();
        folders.sort(); folders.dedup();
        return Database { hosts: map, folders };
    }

    // Loose migration (accept ip->host, missing fields, etc.)
    let mut migrated: HashMap<String, Host> = HashMap::new();
    match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(val) => {
            if let Some(obj) = val.as_object() {
                for (alias, entry) in obj {
                    if let Some(e) = entry.as_object() {
                        let name = e.get("name").and_then(|x| x.as_str()).unwrap_or(alias);
                        // Prefer `host`, fallback to legacy `ip`
                        let host = e.get("host").and_then(|x| x.as_str())
                            .or_else(|| e.get("ip").and_then(|x| x.as_str()))
                            .unwrap_or("");
                        let port = e.get("port").and_then(|x| x.as_u64()).unwrap_or(22) as u16;
                        let username = e.get("username").and_then(|x| x.as_str()).unwrap_or("root");
                        let identity_file = e.get("identity_file").and_then(|x| x.as_str()).map(|s| s.to_string());
                        let proxy_jump = e.get("proxy_jump").and_then(|x| x.as_str()).map(|s| s.to_string());
                        let folder = e.get("folder").and_then(|x| x.as_str()).map(|s| s.to_string());
                        let tags = e.get("tags").and_then(|x| x.as_array()).map(|arr| {
                            arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<_>>()
                        });

                        if !host.is_empty() {
                            migrated.insert(alias.clone(), Host {
                                name: name.to_string(),
                                host: host.to_string(),
                                port,
                                username: username.to_string(),
                                identity_file,
                                proxy_jump,
                                folder,
                                tags,
                            });
                        }
                    }
                }
            }
        }
        Err(_) => {
            // Backup invalid JSON and return empty
            let bak = path.with_extension("json.bak");
            if let Err(be) = fs::write(&bak, content) {
                eprintln!("Failed to write backup {}: {be}", bak.display());
            } else {
                eprintln!("Config was invalid JSON. Backed up to {}.", bak.display());
            }
            return Database { hosts: Default::default(), folders: vec![] };
        }
    }

    let mut folders: Vec<String> = migrated.values().filter_map(|h| h.folder.clone()).collect();
    folders.sort(); folders.dedup();
    let db = Database { hosts: migrated, folders };
    eprintln!("Migrated config to new schema. Saving...");
    save_db(&db);
    db
}

/// Save the full Database (stable order for diffs)
pub fn save_db(db: &Database) {
    let path = config_path();

    #[derive(Serialize)]
    struct Out<'a> { hosts: BTreeMap<String, &'a Host>, folders: Vec<String> }

    let mut ordered: BTreeMap<String, &Host> = BTreeMap::new();
    for (k, v) in db.hosts.iter() { ordered.insert(k.clone(), v); }

    let mut folders = db.folders.clone();
    folders.sort(); folders.dedup();

    let out = Out { hosts: ordered, folders };

    let json = match serde_json::to_string_pretty(&out) {
        Ok(s) => s,
        Err(e) => { eprintln!("Failed to serialize database: {e}"); return; }
    };

    // Write to a temp and then rename (best-effort cross-platform)
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = fs::write(&tmp, &json) {
        eprintln!("Failed to write temp file {}: {e}", tmp.display());
        return;
    }
    let _ = fs::remove_file(&path);
    // create the directory: $HOME/.config/sshm/

    if let Err(e) = fs::rename(&tmp, &path) {
        eprintln!("Failed to move temp file into place: {e}");
        // fallback direct write
        let _ = fs::write(&path, &json);
    }
}

// -----------------------------------------------------------------------------
// Legacy shims (for older code paths). Prefer using load_db/save_db everywhere.
// -----------------------------------------------------------------------------

/// Legacy: load only hosts map
pub fn load_hosts() -> HashMap<String, Host> {
    load_db().hosts
}

/// Legacy: save only hosts map (folders inferred from hosts' `folder` fields)
pub fn save_hosts(hosts: &HashMap<String, Host>) {
    let mut folders: Vec<String> = hosts.values().filter_map(|h| h.folder.clone()).collect();
    folders.sort(); folders.dedup();
    let db = Database { hosts: hosts.clone(), folders };
    save_db(&db);
}

// Create files
pub fn save_empty_database() {
    let path = config_path();
    let db = Database { hosts: Default::default(), folders: vec![] };
    save_db(&db);
    println!("Created empty database at {}", path.display());
}