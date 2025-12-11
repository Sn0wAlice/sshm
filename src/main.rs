use std::env;

use sshm::config::io::{load_db, save_db};
use sshm::models::Database;
use sshm::commands;
use sshm::import::ssh_config::import_ssh_config;
use sshm::tui::app::run_tui;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut db: Database = load_db();

    match args.get(1).map(String::as_str) {
        Some("list") => {
            let filt = if args.get(2).map(String::as_str) == Some("--filter") {
                args.get(3).cloned()
            } else { None };
            commands::list::list_hosts_with_filter(&db.hosts, filt);
        }
        Some("connect") | Some("c") => {
            let name = args.get(2).cloned();
            let extras: Vec<String> = if name.is_some() { args[3..].to_vec() } else { args[2..].to_vec() };
            commands::connect::connect_host(&db.hosts, name, &extras);
        }
        Some("create") => commands::crud::create(&mut db, None),
        Some("delete") => commands::crud::delete(&mut db),
        Some("edit")   => commands::crud::edit_host(&mut db),
        Some("tag")    => match (args.get(2).map(String::as_str), args.get(3), args.get(4)) {
            (Some("add"), Some(name), Some(tlist)) => {
                let tags: Vec<String> = tlist.split(',').flat_map(|s| s.split_whitespace())
                    .map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                commands::tags::tag_add(&mut db.hosts, name.clone(), tags);
            }
            (Some("del"), Some(name), Some(tlist)) => {
                let tags: Vec<String> = tlist.split(',').flat_map(|s| s.split_whitespace())
                    .map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                commands::tags::tag_del(&mut db.hosts, name.clone(), tags);
            }
            _ => println!("Usage: sshm tag [add|del] <name> <tag1,tag2,...>"),
        },
        Some("load_local_conf") => {
            let before = db.hosts.len();
            import_ssh_config(&mut db.hosts);
            if db.hosts.len() > before {
                save_db(&db);
                println!("Imported {} new hosts from ~/.ssh/config.", db.hosts.len() - before);
            } else {
                println!("No new hosts imported from ~/.ssh/config.");
            }
        }
        Some("add-identity") => {
            let name = args.get(2).cloned();
            let extras: Vec<String> = if name.is_some() { args[3..].to_vec() } else { args[2..].to_vec() };
            sshm::ssh::add_identity::cmd_add_identity(&db.hosts, name, &extras);
        }
        Some("help") => {
            println!("Usage:");
            println!("  sshm");

            println!("\nAdvanced commands:");
            println!("  sshm list [--filter \"expr\"]");
            println!("  sshm connect (c) <name> [overrides...]   # pass -i, -J, -L/-R/-D etc.");
            println!("  sshm create | edit | delete");
            println!("  sshm tag add <name> <tag1,tag2> | tag del <name> <tag1,tag2>");
            println!("  sshm load_local_conf   # import from ~/.ssh/config once");
            println!("  sshm add-identity <name?> [--pub ~/.ssh/id_ed25519.pub]   # push pubkey to authorized_keys");
        }
        _ => loop { run_tui(&mut db) },
    }
}
