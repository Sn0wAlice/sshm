use crate::config::io::{load_db, try_save_db};
use crate::models::Host;
use std::collections::HashMap;

/// Apply `edit` to host `name` on a fresh load of the DB (so folders and
/// concurrent changes survive, and a broken host.json is never saved over),
/// then mirror the result into the caller's `hosts`.
fn edit_tags(
    hosts: &mut HashMap<String, Host>,
    name: &str,
    done: &str,
    edit: impl FnOnce(&mut Vec<String>),
) {
    let mut db = load_db();
    let Some(h) = db.hosts.get_mut(name) else {
        println!("Host '{}' not found.", name);
        return;
    };
    let mut set = h.tags.take().unwrap_or_default();
    edit(&mut set);
    h.tags = if set.is_empty() { None } else { Some(set) };
    match try_save_db(&db) {
        Ok(()) => println!("{done} {name}."),
        Err(e) => eprintln!("Not saved: {e:#}"),
    }
    *hosts = db.hosts;
}

pub fn tag_add(hosts: &mut HashMap<String, Host>, name: String, tags: Vec<String>) {
    edit_tags(hosts, &name, "Tags added to", |set| {
        for t in tags {
            if !set.iter().any(|e| e.eq_ignore_ascii_case(&t)) {
                set.push(t);
            }
        }
    });
}

pub fn tag_del(hosts: &mut HashMap<String, Host>, name: String, tags: Vec<String>) {
    edit_tags(hosts, &name, "Tags removed from", |set| {
        set.retain(|t| !tags.iter().any(|x| t.eq_ignore_ascii_case(x)));
    });
}
