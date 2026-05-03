//! Interactive flows around SSH keys — generation and `known_hosts` cleaning.
//!
//! Both functions take over the foreground (the caller is expected to have
//! already left the alternate screen) and drive `inquire` prompts.

use std::path::PathBuf;

/// Interactive "generate key" flow driven by `inquire`. Returns the path
/// of the freshly created private key, or `None` if the user cancelled.
pub fn run_generate_key_flow() -> std::io::Result<Option<PathBuf>> {
    use inquire::{Password, Select, Text};
    println!();
    let Ok(key_type) = Select::new(
        "Key type:",
        vec!["ed25519", "ed25519-sk (FIDO2)", "ecdsa", "rsa"],
    )
    .prompt() else {
        return Ok(None);
    };
    // Map the human label back to the ssh-keygen -t value.
    let key_type: &str = match key_type {
        "ed25519-sk (FIDO2)" => "ed25519-sk",
        other => other,
    };
    let default_name = match key_type {
        "rsa" => "id_rsa",
        "ecdsa" => "id_ecdsa",
        "ed25519-sk" => "id_ed25519_sk",
        _ => "id_ed25519",
    };
    let Some(home) = dirs::home_dir() else {
        return Err(std::io::Error::other("no HOME dir"));
    };
    let default_path = home.join(".ssh").join(default_name);
    let Ok(path_str) = Text::new("File path:")
        .with_default(&default_path.display().to_string())
        .prompt()
    else {
        return Ok(None);
    };
    let path = PathBuf::from(shellexpand::tilde(&path_str).to_string());
    if path.exists() {
        eprintln!("{} already exists — aborting.", path.display());
        return Ok(None);
    }
    let default_comment = format!(
        "{}@{}",
        std::env::var("USER").unwrap_or_else(|_| "user".to_string()),
        hostname_best_effort()
    );
    let Ok(comment) = Text::new("Comment:")
        .with_default(&default_comment)
        .prompt()
    else {
        return Ok(None);
    };
    let passphrase = Password::new("Passphrase (empty for none):")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .unwrap_or_default();
    crate::ssh::keys::generate_key(key_type, &path, &comment, &passphrase)?;
    Ok(Some(path))
}

/// Interactive "clean known_hosts" flow. Asks the user for a hostname,
/// shells out to `ssh-keygen -R <host>`, and returns the hostname on
/// success for the caller's toast.
pub fn run_known_hosts_clean_flow() -> std::io::Result<Option<String>> {
    use inquire::Text;
    println!();
    let Ok(host) = Text::new("Hostname to remove from known_hosts:").prompt() else {
        return Ok(None);
    };
    let host = host.trim().to_string();
    if host.is_empty() {
        return Ok(None);
    }
    crate::ssh::known_hosts::remove_known_host(&host)?;
    Ok(Some(host))
}

/// Best-effort hostname for default key comments.
pub fn hostname_best_effort() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}
