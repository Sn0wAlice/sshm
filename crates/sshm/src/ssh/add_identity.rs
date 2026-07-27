use std::path::PathBuf;
use std::collections::HashMap;
use inquire::Select;
use crate::models::Host;
use super::keys::{pub_from_identity, default_pubkey_path, install_pubkey_on_host};

/// Commande: ajoute une clé publique à authorized_keys du host
pub fn cmd_add_identity(hosts: &HashMap<String, Host>, name: Option<String>, args: &[String]) {
    // parse --pub /path/to/key.pub (optionnel)
    let mut pub_override: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--pub" {
            if let Some(p) = args.get(i + 1) {
                pub_override = Some(PathBuf::from(shellexpand::tilde(p).to_string()));
                i += 2;
                continue;
            }
        }
        i += 1;
    }

    // choix du host
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

    let Some(h) = hosts.get(&name) else {
        eprintln!("Host '{}' not found.", name);
        return;
    };

    // Déterminer la clé publique à utiliser
    let pubkey_path = if let Some(p) = pub_override {
        p
    } else if let Some(id) = &h.identity_file {
        match pub_from_identity(id) {
            Some(pb) => pb,
            None => {
                eprintln!("No .pub found next to identity_file; falling back to default ~/.ssh/id_*.pub");
                match default_pubkey_path() {
                    Some(pb) => pb,
                    None => {
                        eprintln!("No default public key found (~/.ssh/id_ed25519.pub or id_rsa.pub). Use --pub <path>.");
                        return;
                    }
                }
            }
        }
    } else {
        match default_pubkey_path() {
            Some(pb) => pb,
            None => {
                eprintln!("No default public key found (~/.ssh/id_ed25519.pub or id_rsa.pub). Use --pub <path>.");
                return;
            }
        }
    };

    println!("Installing public key '{}' on {}@{}:{} …",
        pubkey_path.display(), h.username, h.host, h.port);

    match install_pubkey_on_host(h, &pubkey_path) {
        Ok(_) => println!("✅ Public key installed. You should be able to connect without password."),
        Err(e) => eprintln!("❌ Failed to install key: {e}"),
    }
}
