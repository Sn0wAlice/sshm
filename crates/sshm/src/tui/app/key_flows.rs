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

/// What the fingerprint inspector did, so the caller can toast appropriately.
pub enum FingerprintOutcome {
    Pinned,
    Forgotten,
    Nothing,
}

/// Interactive host-key inspector for a single host (the `F` action). Prints the
/// pinned key from `known_hosts` alongside the key the server presents right
/// now, states whether they match, and offers to pin (trust-on-first-use),
/// forget, or replace a stale key. `hostname`/`port` identify the target.
pub fn run_host_fingerprint_flow(
    hostname: &str,
    port: u16,
) -> std::io::Result<FingerprintOutcome> {
    use crate::ssh::known_hosts;
    use inquire::Select;

    let pinned = known_hosts::pinned_fingerprints(hostname, port);
    let live = known_hosts::scan_fingerprints(hostname, port);

    println!();
    println!("Host key — {hostname}:{port}");
    println!();
    if pinned.is_empty() {
        println!("  Pinned (known_hosts): (none)");
    } else {
        for k in &pinned {
            println!("  Pinned (known_hosts): {k}");
        }
    }
    if live.is_empty() {
        println!("  Live (server):        (unreachable)");
    } else {
        for k in &live {
            println!("  Live (server):        {k}");
        }
    }

    // Verdict — compared on fingerprint, ignoring which algorithm matched.
    let matches = !pinned.is_empty()
        && live.iter().any(|l| pinned.iter().any(|p| p.fingerprint == l.fingerprint));
    println!();
    match (pinned.is_empty(), live.is_empty()) {
        (true, false) => println!("  ⧗ Not pinned yet — this would be a trust-on-first-use."),
        (false, true) => println!("  ? Host unreachable — showing the pinned key only."),
        _ if matches => println!("  ✓ Match — the pinned key matches the server."),
        (false, false) => println!("  ✗ CHANGED — the pinned key does NOT match the server!"),
        (true, true) => println!("  ? No key on either side."),
    }
    println!();

    // Offer only the actions that make sense for the current state.
    let mut options: Vec<&str> = Vec::new();
    if !live.is_empty() && pinned.is_empty() {
        options.push("Pin this key (trust on first use)");
    }
    if !live.is_empty() && !pinned.is_empty() && !matches {
        options.push("Replace — forget the stale key and pin the current one");
    }
    if !pinned.is_empty() {
        options.push("Forget — remove from known_hosts");
    }
    options.push("Close");

    let Ok(choice) = Select::new("Action:", options).prompt() else {
        return Ok(FingerprintOutcome::Nothing);
    };
    match choice {
        "Pin this key (trust on first use)" => {
            known_hosts::pin_host(hostname, port)?;
            Ok(FingerprintOutcome::Pinned)
        }
        "Replace — forget the stale key and pin the current one" => {
            known_hosts::remove_known_host_port(hostname, port)?;
            known_hosts::pin_host(hostname, port)?;
            Ok(FingerprintOutcome::Pinned)
        }
        "Forget — remove from known_hosts" => {
            known_hosts::remove_known_host_port(hostname, port)?;
            Ok(FingerprintOutcome::Forgotten)
        }
        _ => Ok(FingerprintOutcome::Nothing),
    }
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
