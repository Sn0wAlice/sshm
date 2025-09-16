use std::collections::HashMap;
use inquire::Select;
use crate::models::Host;
use crate::ssh::client::launch_ssh;

pub fn connect_host(hosts: &HashMap<String, Host>, name: Option<String>, extra: &[String]) {
    let name = match name {
        Some(n) => n,
        None => {
            let mut choices: Vec<&String> = hosts.keys().collect();
            choices.sort();
            match Select::new("Choose a host:", choices).prompt() {
                Ok(choice) => choice.to_string(),
                Err(_) => return,
            }
        }
    };

    if let Some(h) = hosts.get(&name) {
        launch_ssh(h, Some(extra));
        return;
    }
    let matching: Vec<&Host> = hosts.values().filter(|h| h.name.contains(&name)).collect();
    match matching.len() {
        0 => println!("No matching host."),
        1 => launch_ssh(matching[0], Some(extra)),
        _ => {
            let options: Vec<String> = matching.iter().map(|h| h.name.clone()).collect();
            if let Ok(choice) = Select::new("Multiple matches. Choose:", options).prompt() {
                connect_host(hosts, Some(choice), extra);
            }
        }
    }
}
