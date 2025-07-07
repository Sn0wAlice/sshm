use inquire::{Select, Text};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs::{self, create_dir_all, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use prettytable::{Table, row};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Host {
    name: String,
    ip: String,
    port: u16,
    username: String,
}

fn config_path() -> PathBuf {
    let mut path = dirs::home_dir().unwrap();
    path.push(".config/sshm/host.json");
    path
}

fn load_hosts() -> HashMap<String, Host> {
    let path = config_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            create_dir_all(parent).unwrap();
        }
        File::create(&path).unwrap().write_all(b"{}\n").unwrap();
    }

    let mut file = File::open(path).unwrap();
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap();
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_hosts(hosts: &HashMap<String, Host>) {
    let path = config_path();
    let json = serde_json::to_string_pretty(hosts).unwrap();
    fs::write(path, json).unwrap();
}

fn list_hosts(hosts: &HashMap<String, Host>) {
    use prettytable::{Table, row, cell};

    if hosts.is_empty() {
        println!("No hosts available.");
        return;
    }

    let mut table = Table::new();
    table.add_row(row!["Name", "Username", "IP", "Port"]);

    for (name, host) in hosts {
        table.add_row(row![
            name,
            host.username,
            host.ip,
            host.port.to_string()
        ]);
    }

    table.printstd();
}

fn connect_host(hosts: &HashMap<String, Host>, name: Option<String>) {
    let name = match name {
        Some(n) => n,
        None => {
            let choices: Vec<&String> = hosts.keys().collect();
            match Select::new("Choose a host:", choices).prompt() {
                Ok(choice) => choice.to_string(),
                Err(_) => return,
            }
        }
    };

    let matching: Vec<&Host> = hosts.values().filter(|h| h.name.contains(&name)).collect();
    if matching.is_empty() {
        println!("No matching host.");
    } else if matching.len() == 1 {
        let h = matching[0];
        let _ = Command::new("ssh")
            .arg(format!("{}@{}", h.username, h.ip))
            .arg("-p")
            .arg(h.port.to_string())
            .status();
    } else {
        let options: Vec<String> = matching.iter().map(|h| h.name.clone()).collect();
        if let Ok(choice) = Select::new("Multiple matches. Choose:", options).prompt() {
            connect_host(hosts, Some(choice));
        }
    }
}

fn create_host(hosts: &mut HashMap<String, Host>) {
    let name = Text::new("Name:").prompt().unwrap();
    let ip = Text::new("IP:").prompt().unwrap();
    let port: u16 = Text::new("Port:").prompt().unwrap().parse().unwrap_or(22);
    let username = Text::new("Username:").prompt().unwrap();

    hosts.insert(
        name.clone(),
        Host {
            name,
            ip,
            port,
            username,
        },
    );
    save_hosts(hosts);
}

fn delete_host(hosts: &mut HashMap<String, Host>) {
    let choices: Vec<_> = hosts.keys().cloned().collect(); // cloner les clés
    if let Ok(choice) = Select::new("Choose host to delete:", choices).prompt() {
        hosts.remove(&choice); // on utilise une &String, aucun conflit
        save_hosts(hosts);
    }
}


fn edit_host(hosts: &mut HashMap<String, Host>) {
    let choices: Vec<String> = hosts.keys().cloned().collect(); // Cloner les clés
    if let Ok(choice) = Select::new("Choose host to edit:", choices).prompt() {
        if let Some(host) = hosts.get_mut(&choice) {
            host.ip = Text::new("New IP:").with_initial_value(&host.ip).prompt().unwrap();
            host.port = Text::new("New Port:")
                .with_initial_value(&host.port.to_string())
                .prompt()
                .unwrap()
                .parse()
                .unwrap_or(22);
            host.username = Text::new("New Username:")
                .with_initial_value(&host.username)
                .prompt()
                .unwrap();
            save_hosts(hosts);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut hosts = load_hosts();

    match args.get(1).map(String::as_str) {
        Some("list") => list_hosts(&hosts),
        Some("connect") | Some("c") => connect_host(&hosts, args.get(2).cloned()),
        Some("create") => create_host(&mut hosts),
        Some("delete") => delete_host(&mut hosts),
        Some("edit") => edit_host(&mut hosts),
        Some("help") => {
            println!("Usage:");
            println!("  sshm list");
            println!("  sshm connect (c) <name>");
            println!("  sshm create");
            println!("  sshm edit");
            println!("  sshm delete");
        }
        _ => println!("Usage: sshm [list|connect|c|create|edit|delete] [name]"),
    }
}