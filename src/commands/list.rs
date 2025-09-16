use std::collections::HashMap;
use prettytable::{row, Table};
use crate::models::Host;
use crate::filter::filter_hosts;
use crate::models::tags_to_string;

pub fn list_hosts_with_filter(hosts: &HashMap<String, Host>, filter: Option<String>) {
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
        table.add_row(row![h.name, h.username, h.host, h.port.to_string(), tags_to_string(&h.tags)]);
    }
    table.printstd();
}
