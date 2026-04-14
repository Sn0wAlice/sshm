use std::collections::HashMap;
use inquire::Select;
use crate::models::Host;
use crate::ssh::client::launch_ssh;

/// Launch `ssh` on the selected host. Returns the name of the host that was
/// actually launched (so callers can bump connection history), or `None` if
/// nothing was launched (user cancelled, no match, etc.).
pub fn connect_host(
    hosts: &HashMap<String, Host>,
    name: Option<String>,
    extra: &[String],
) -> Option<String> {
    let name = match name {
        Some(n) => n,
        None => {
            let mut choices: Vec<&String> = hosts.keys().collect();
            choices.sort();
            match Select::new("Choose a host:", choices).prompt() {
                Ok(choice) => choice.to_string(),
                Err(_) => return None,
            }
        }
    };

    if let Some(h) = hosts.get(&name) {
        launch_ssh(h, Some(extra));
        return Some(h.name.clone());
    }
    let matching: Vec<&Host> = hosts.values().filter(|h| h.name.contains(&name)).collect();
    match matching.len() {
        0 => {
            println!("No matching host.");
            None
        }
        1 => {
            let h = matching[0];
            launch_ssh(h, Some(extra));
            Some(h.name.clone())
        }
        _ => {
            let options: Vec<String> = matching.iter().map(|h| h.name.clone()).collect();
            if let Ok(choice) = Select::new("Multiple matches. Choose:", options).prompt() {
                connect_host(hosts, Some(choice), extra)
            } else {
                None
            }
        }
    }
}
