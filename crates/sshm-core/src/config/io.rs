use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::Read;
use anyhow::{Context, Result};
use serde::Serialize;
use crate::models::{Host, Database};
use super::path::{config_path, ensure_config_file};

/// Pure parse: take JSON text and return a [`Database`] handling all
/// supported schema variants (current, legacy map-only, loose migration).
/// Returns `None` if the content can't be coerced into anything sensible.
pub fn parse_db_text(content: &str) -> Option<Database> {
    // Distinguish schemas by inspecting the top-level shape: the canonical
    // schema has a `hosts` key, the legacy ones are a flat alias→entry map.
    let top: serde_json::Value = serde_json::from_str(content).ok()?;
    let has_hosts_key = top.get("hosts").is_some();

    if has_hosts_key {
        if let Ok(db) = serde_json::from_str::<Database>(content) {
            return Some(db);
        }
    }
    if let Ok(map) = serde_json::from_str::<HashMap<String, Host>>(content) {
        let mut folders: Vec<String> = map.values().filter_map(|h| h.folder.clone()).collect();
        folders.sort(); folders.dedup();
        return Some(Database { hosts: map, folders });
    }

    let mut migrated: HashMap<String, Host> = HashMap::new();
    let obj = top.as_object()?;
    for (alias, entry) in obj {
        if let Some(e) = entry.as_object() {
            let name = e.get("name").and_then(|x| x.as_str()).unwrap_or(alias);
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
            let last_connected_at = e.get("last_connected_at").and_then(|x| x.as_str()).map(|s| s.to_string());
            let use_count = e.get("use_count").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let favorite = e.get("favorite").and_then(|x| x.as_bool()).unwrap_or(false);
            let tunnels = e.get("tunnels")
                .and_then(|x| serde_json::from_value(x.clone()).ok())
                .unwrap_or_default();
            let forward_agent = e.get("forward_agent").and_then(|x| x.as_bool()).unwrap_or(false);
            let mosh = e.get("mosh").and_then(|x| x.as_bool()).unwrap_or(false);
            let notes = e.get("notes").and_then(|x| x.as_str()).map(|s| s.to_string());

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
                    last_connected_at,
                    use_count,
                    favorite,
                    tunnels,
                    forward_agent,
                    mosh,
                    notes,
                    remote_command: None,
                });
            }
        }
    }
    let mut folders: Vec<String> = migrated.values().filter_map(|h| h.folder.clone()).collect();
    folders.sort(); folders.dedup();
    Some(Database { hosts: migrated, folders })
}

/// Pure serialize: produce the canonical pretty JSON representation of `db`
/// (sorted host map, deduped folders) — what `save_db` writes to disk.
pub fn serialize_db(db: &Database) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct Out<'a> { hosts: BTreeMap<String, &'a Host>, folders: Vec<String> }

    let ordered: BTreeMap<_, _> = db.hosts.iter().map(|(k, v)| (k.clone(), v)).collect();
    let mut folders = db.folders.clone();
    folders.sort();
    folders.dedup();
    serde_json::to_string_pretty(&Out { hosts: ordered, folders })
}

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

    match parse_db_text(&content) {
        Some(db) => {
            // If the content didn't already match the canonical schema, save
            // back so the next load is fast and stable.
            if serde_json::from_str::<Database>(&content).is_err() {
                eprintln!("Migrated config to new schema. Saving...");
                save_db(&db);
            }
            db
        }
        None => {
            // Backup invalid JSON and return empty
            let bak = path.with_extension("json.bak");
            if let Err(be) = fs::write(&bak, content) {
                eprintln!("Failed to write backup {}: {be}", bak.display());
            } else {
                eprintln!("Config was invalid JSON. Backed up to {}.", bak.display());
            }
            Database { hosts: Default::default(), folders: vec![] }
        }
    }
}

/// Save the full Database (stable order for diffs).
///
/// Returns the typed error so callers can surface it (e.g. via a toast).
/// Atomicity: writes to `<path>.tmp` then renames over `path`.
pub fn try_save_db(db: &Database) -> Result<()> {
    let path = config_path();
    let json = serialize_db(db).context("serializing database")?;

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)
        .with_context(|| format!("writing temp config {}", tmp.display()))?;
    let _ = fs::remove_file(&path); // best effort on Windows
    if let Err(e) = fs::rename(&tmp, &path) {
        // Fallback: direct write — rename can fail across mount points etc.
        fs::write(&path, &json)
            .with_context(|| format!("renaming + fallback-writing {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Fire-and-forget wrapper that logs on failure. Most call sites can't act
/// on an error anyway and shouldn't have to thread `Result` everywhere.
pub fn save_db(db: &Database) {
    if let Err(e) = try_save_db(db) {
        eprintln!("save_db: {e:#}");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str) -> Host {
        Host {
            name: name.to_string(),
            host: format!("{}.example", name),
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
            remote_command: None,
        }
    }

    #[test]
    fn round_trip_canonical_schema() {
        let mut db = Database::default();
        db.hosts.insert("a".into(), host("a"));
        db.hosts.insert("b".into(), host("b"));
        db.folders = vec!["X".into(), "Y".into()];

        let json = serialize_db(&db).unwrap();
        let parsed = parse_db_text(&json).unwrap();

        assert_eq!(parsed.hosts.len(), 2);
        assert!(parsed.hosts.contains_key("a"));
        assert!(parsed.hosts.contains_key("b"));
        assert_eq!(parsed.folders, vec!["X".to_string(), "Y".to_string()]);
    }

    #[test]
    fn legacy_map_only_schema_loads() {
        // Old format: top-level was a map alias->Host with no `folders` array.
        let raw = r#"{
            "x": {"name":"x","host":"1.2.3.4","port":22,"username":"u","folder":"Prod"}
        }"#;
        let db = parse_db_text(raw).unwrap();
        assert_eq!(db.hosts.len(), 1);
        assert_eq!(db.hosts["x"].name, "x");
        assert_eq!(db.folders, vec!["Prod".to_string()]);
    }

    #[test]
    fn loose_migration_accepts_ip_field() {
        // Even older: `ip` instead of `host`, missing fields filled with defaults.
        let raw = r#"{
            "old": {"ip":"10.0.0.1"}
        }"#;
        let db = parse_db_text(raw).unwrap();
        let h = &db.hosts["old"];
        assert_eq!(h.host, "10.0.0.1");
        assert_eq!(h.port, 22);
        assert_eq!(h.username, "root");
        assert_eq!(h.name, "old");
    }

    #[test]
    fn invalid_json_returns_none() {
        assert!(parse_db_text("{ this is not json }").is_none());
        assert!(parse_db_text("").is_none());
    }

    #[test]
    fn save_dedupes_and_sorts_folders() {
        let db = Database {
            hosts: Default::default(),
            folders: vec!["B".into(), "A".into(), "B".into()],
        };
        let json = serialize_db(&db).unwrap();
        // Folders appear in sorted order, deduped
        let pos_a = json.find("\"A\"").expect("A present");
        let pos_b = json.find("\"B\"").expect("B present");
        assert!(pos_a < pos_b);
        // Only one occurrence of "B"
        assert_eq!(json.matches("\"B\"").count(), 1);
    }
}