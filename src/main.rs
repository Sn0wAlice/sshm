use std::env;

use sshm::config::io::{load_db, save_db};
use sshm::config::export::export_ssh_config;
use sshm::config::settings::load_settings;
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
            let launched = commands::connect::connect_host(&db.hosts, name, &extras);
            if let Some(connected) = launched {
                if let Some(host) = db.hosts.get_mut(&connected) {
                    sshm::history::record_connection(host);
                    save_db(&db);
                }
            }
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
        Some("export") => {
            let path = args.get(2).cloned().unwrap_or_else(|| {
                load_settings().export_path
            });
            if path.trim().is_empty() {
                eprintln!("No export path provided. Usage: sshm export <path>");
                eprintln!("Or set an export path in Settings.");
            } else {
                match export_ssh_config(&db, &path) {
                    Ok(()) => println!("Exported {} hosts to {}", db.hosts.len(), path),
                    Err(e) => eprintln!("Export failed: {e}"),
                }
            }
        }
        Some("add-identity") => {
            let name = args.get(2).cloned();
            let extras: Vec<String> = if name.is_some() { args[3..].to_vec() } else { args[2..].to_vec() };
            sshm::ssh::add_identity::cmd_add_identity(&db.hosts, name, &extras);
        }
        Some("help") | Some("-h") | Some("--help") => {
            println!("sshm — SSH + Docker + Incus + Kubernetes manager for the terminal");
            println!();
            println!("Usage:");
            println!("  sshm                                       # launch the TUI (recommended)");
            println!();
            println!("Hosts:");
            println!("  sshm list [--filter \"expr\"]                # list hosts (expr: tag:foo host:1.* user:bar)");
            println!("  sshm connect (c) <name> [overrides...]     # connect via ssh; pass -i, -J, -L/-R/-D etc.");
            println!("  sshm create | edit | delete                # interactive CRUD");
            println!("  sshm tag add <name> <tag1,tag2>            # add tags");
            println!("  sshm tag del <name> <tag1,tag2>            # remove tags");
            println!();
            println!("Identities:");
            println!("  sshm add-identity <name?> [--pub <path>]   # push pubkey to authorized_keys");
            println!();
            println!("Import / export:");
            println!("  sshm load_local_conf                       # import from ~/.ssh/config");
            println!("  sshm export [path]                         # export DB as ~/.ssh/config format");
            println!();
            println!("Inside the TUI:");
            println!("  ←/→  switch tabs (Hosts | Kluster | Identities | Settings | Theme | Help)");
            println!("  In Kluster: Enter shell, l logs(-f), r refresh, n add, d delete/unlink");
            println!();
            println!("Locale:    SSHM_LANG=en|fr  (default = locale, fallback en)");
            println!("Config:    ~/.config/sshm/{{host,kluster}}.json, settings.toml, theme.toml");
        }
        _ => loop { run_tui(&mut db) },
    }
}
