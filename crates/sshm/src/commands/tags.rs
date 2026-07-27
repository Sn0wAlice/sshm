use std::collections::HashMap;
use crate::models::Host;
use crate::config::io::save_hosts;

pub fn tag_add(hosts: &mut HashMap<String, Host>, name: String, tags: Vec<String>) {
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

pub fn tag_del(hosts: &mut HashMap<String, Host>, name: String, tags: Vec<String>) {
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
