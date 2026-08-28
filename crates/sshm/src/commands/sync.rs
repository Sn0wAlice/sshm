//! `sshm sync` — the CLI surface of the git-backed config sync.
//!
//! The engine lives in [`sshm_core::sync`]; this module is the argument
//! parsing, the interactive setup wizard, and the human-readable reporting.

use anyhow::Result;
use inquire::{Confirm, MultiSelect, Select, Text};

use crate::config::settings::{
    load_settings, try_save_settings, AppConfig, ConflictPolicy, SyncConfig, SyncItem, SyncMode,
    MIN_SYNC_INTERVAL_SECS,
};
use sshm_core::sync::{self, Direction, SyncRun};

/// Entry point. `args` is everything after `sshm sync`.
pub fn dispatch(args: &[String]) {
    let sub = args.first().map(String::as_str).unwrap_or("");
    let result = match sub {
        "" | "now" | "--if-due" | "--force" => run_sync(sub),
        "pull" => run_direction(Direction::Pull),
        "push" => run_direction(Direction::Push),
        "setup" => setup(),
        "status" => { status(); Ok(()) }
        "enable" => toggle(true),
        "disable" => toggle(false),
        "cron" => { print_cron(); Ok(()) }
        "help" | "-h" | "--help" => { usage(); Ok(()) }
        other => {
            eprintln!("Unknown sync command: {other}");
            usage();
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("sync failed: {e:#}");
        std::process::exit(1);
    }
}

pub fn usage() {
    println!("sshm sync — keep your sshm config in a git repo of your own (SSH key auth)");
    println!();
    println!("  sshm sync                 # sync now (merge both ways, then push)");
    println!("  sshm sync --if-due        # sync only if the interval elapsed (for cron)");
    println!("  sshm sync pull            # apply the remote locally, publish nothing");
    println!("  sshm sync push            # publish local state (local wins collisions)");
    println!("  sshm sync setup           # interactive configuration");
    println!("  sshm sync status          # what is configured, and when it last ran");
    println!("  sshm sync enable|disable  # master switch");
    println!("  sshm sync cron            # print a crontab line for scheduled syncing");
    println!();
    println!("Only one sshm ever syncs at a time — other instances skip the run.");
}

// -----------------------------------------------------------------------------
// Running
// -----------------------------------------------------------------------------

fn run_sync(sub: &str) -> Result<()> {
    let cfg = load_settings().sync;
    let run = if sub == "--if-due" {
        sync::sync_if_due(&cfg)?
    } else {
        sync::sync_now(&cfg)?
    };
    report(&run, sub == "--if-due");
    Ok(())
}

fn run_direction(direction: Direction) -> Result<()> {
    let cfg = load_settings().sync;
    let run = sync::sync_direction(&cfg, direction)?;
    report(&run, false);
    Ok(())
}

/// Print the outcome. A scheduled (`--if-due`) run stays silent when there was
/// nothing to do, so a cron entry doesn't mail the user every 15 minutes.
fn report(run: &SyncRun, quiet_when_idle: bool) {
    match run {
        SyncRun::Done(r) => {
            if quiet_when_idle && !run.changed_anything() {
                return;
            }
            println!("Sync: {}", r.summary());
            if r.stats.conflicts > 0 {
                println!(
                    "  {} entry(ies) changed on both sides — resolved by your conflict policy \
                     (see `sshm sync status`).",
                    r.stats.conflicts
                );
            }
        }
        SyncRun::Busy(_) | SyncRun::Skipped(_) => {
            if !quiet_when_idle {
                println!("Sync: {}", run.summary());
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Status
// -----------------------------------------------------------------------------

fn ago(secs: i64) -> String {
    match secs {
        s if s < 60 => format!("{s}s ago"),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn status() {
    let cfg = load_settings().sync;
    let state = sync::SyncState::load();

    println!("Config sync");
    println!("  Enabled     : {}", if cfg.enabled { "yes" } else { "no" });
    println!(
        "  Repository  : {}",
        if cfg.is_configured() { cfg.repo_url.trim() } else { "(not configured)" }
    );
    println!("  Branch      : {}", cfg.effective_branch());
    println!(
        "  SSH key     : {}",
        cfg.expanded_key().unwrap_or_else(|| "(ssh-agent / ~/.ssh/config)".to_string())
    );
    let when = match cfg.effective_interval() {
        Some(secs) => format!("every {} min", secs / 60),
        None => "manual (or cron)".to_string(),
    };
    let mut triggers = vec![when];
    if cfg.on_start { triggers.push("on start".into()) }
    if cfg.on_exit { triggers.push("on exit".into()) }
    println!("  Schedule    : {}", triggers.join(", "));
    println!(
        "  Files       : {}",
        cfg.effective_items().iter().map(|i| i.file_name()).collect::<Vec<_>>().join(", ")
    );
    println!(
        "  Conflicts   : {}",
        match cfg.conflict {
            ConflictPolicy::PreferLocal => "keep this machine's version",
            ConflictPolicy::PreferRemote => "keep the remote version",
        }
    );
    println!("  Working copy: {}", sshm_core::config::path::sync_repo_dir().display());

    println!();
    match state.since_last_success() {
        Some(secs) => println!(
            "  Last success: {} — {}",
            ago(secs),
            state.last_summary.as_deref().unwrap_or("ok")
        ),
        None => println!("  Last success: never"),
    }
    if let Some(secs) = state.since_last_attempt() {
        println!("  Last attempt: {}{}", ago(secs), match &state.last_host {
            Some(h) => format!(" (from {h})"),
            None => String::new(),
        });
    }
    if let Some(err) = &state.last_error {
        println!("  Last error  : {err}");
    }
    match sync::SyncLock::holder() {
        Some(info) => println!(
            "  Lock        : held by pid {} on {} ({}, {})",
            info.pid, info.host, info.what, ago(info.age_secs())
        ),
        None => println!("  Lock        : free"),
    }

    if cfg.is_configured() {
        if let Err(e) = sync::preflight(&cfg) {
            println!();
            println!("  ⚠ {e}");
        }
    } else {
        println!();
        println!("  Run `sshm sync setup` to configure it.");
    }
}

fn print_cron() {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "sshm".to_string());
    let cfg = load_settings().sync;
    let minutes = cfg.effective_interval().map(|s| s / 60).unwrap_or(15).max(1);

    println!("# Sync sshm's config on a schedule. `--if-due` respects the interval");
    println!("# in Settings and does nothing when another instance is already syncing,");
    println!("# so it is safe to run more often than you sync.");
    println!("*/{minutes} * * * * {exe} sync --if-due >/dev/null 2>&1");
    println!();
    println!("Add it with `crontab -e`.");
    if !cfg.is_active() {
        println!();
        println!("Note: sync is currently disabled — `sshm sync enable` first.");
    }
}

// -----------------------------------------------------------------------------
// Configuration
// -----------------------------------------------------------------------------

fn save(config: &AppConfig) -> Result<()> {
    try_save_settings(config)
}

fn toggle(on: bool) -> Result<()> {
    let mut config = load_settings();
    if on && !config.sync.is_configured() {
        println!("No repository configured yet — run `sshm sync setup`.");
        return Ok(());
    }
    config.sync.enabled = on;
    save(&config)?;
    println!("Config sync {}.", if on { "enabled" } else { "disabled" });
    Ok(())
}

/// Interactive setup. Every prompt starts from the current value, so running
/// it again to change one thing is painless.
fn setup() -> Result<()> {
    let mut config = load_settings();
    let cur = config.sync.clone();

    println!("sshm keeps your config in a git repository you own, over SSH.");
    println!("Create an empty private repo first, then paste its SSH URL below.");
    println!();

    let repo_url = Text::new("Repository SSH URL:")
        .with_initial_value(&cur.repo_url)
        .with_placeholder("git@github.com:you/sshm-config.git")
        .with_help_message("SSH form only — HTTPS URLs can't authenticate with a key")
        .prompt()?;
    let repo_url = repo_url.trim().to_string();
    if repo_url.is_empty() {
        println!("Aborted: no repository URL.");
        return Ok(());
    }

    let ssh_key = pick_key(&cur)?;
    let branch = Text::new("Branch:")
        .with_initial_value(if cur.branch.is_empty() { "main" } else { &cur.branch })
        .prompt()?;

    let items = pick_items(&cur)?;
    let (mode, interval_secs) = pick_schedule(&cur)?;

    let on_start = Confirm::new("Sync when sshm starts?")
        .with_default(cur.on_start)
        .prompt()?;
    let on_exit = Confirm::new("Sync when you leave sshm?")
        .with_default(cur.on_exit)
        .with_help_message("Publishes the edits you made during the session")
        .prompt()?;

    let conflict = pick_conflict(&cur)?;

    config.sync = SyncConfig {
        enabled: true,
        repo_url,
        ssh_key,
        branch,
        mode,
        interval_secs,
        on_start,
        on_exit,
        items,
        conflict,
        strict_host_key_checking: cur.strict_host_key_checking,
    };
    save(&config)?;
    println!();
    println!("Saved to {}.", crate::config::settings::settings_path().display());

    if let Err(e) = sync::preflight(&config.sync) {
        println!("⚠ {e}");
        return Ok(());
    }

    if Confirm::new("Sync now?").with_default(true).prompt()? {
        match sync::sync_now(&config.sync) {
            Ok(run) => report(&run, false),
            Err(e) => {
                eprintln!("Sync failed: {e:#}");
                eprintln!("The settings were saved — fix the cause and run `sshm sync` again.");
            }
        }
    }
    Ok(())
}

/// Offer the keys found in `~/.ssh`, plus "type a path" and "no key".
fn pick_key(cur: &SyncConfig) -> Result<String> {
    const OTHER: &str = "Other (type a path)…";
    const NONE: &str = "None (use ssh-agent / ~/.ssh/config)";

    let keys = sshm_core::ssh::keys::scan_ssh_dir();
    let mut options: Vec<String> =
        keys.iter().map(|k| k.private.display().to_string()).collect();
    options.push(OTHER.to_string());
    options.push(NONE.to_string());

    // Start the cursor on the currently configured key when we can find it.
    let start = cur
        .expanded_key()
        .and_then(|k| options.iter().position(|o| *o == k))
        .unwrap_or(0);

    let choice = Select::new("SSH key for the repository:", options.clone())
        .with_starting_cursor(start)
        .prompt()?;

    Ok(match choice.as_str() {
        NONE => String::new(),
        OTHER => Text::new("Path to the private key:")
            .with_initial_value(&cur.ssh_key)
            .prompt()?
            .trim()
            .to_string(),
        path => path.to_string(),
    })
}

fn pick_items(cur: &SyncConfig) -> Result<Vec<SyncItem>> {
    let labels: Vec<String> = SyncItem::ALL
        .iter()
        .map(|i| match i {
            SyncItem::Hosts => "hosts, folders and tunnels (host.json)".to_string(),
            SyncItem::Kluster => "clusters and remotes (kluster.json)".to_string(),
            SyncItem::Settings => "settings (settings.toml)".to_string(),
            SyncItem::Theme => "theme (theme.toml)".to_string(),
        })
        .collect();
    let defaults: Vec<usize> = SyncItem::ALL
        .iter()
        .enumerate()
        .filter(|(_, i)| cur.items.contains(i))
        .map(|(n, _)| n)
        .collect();

    let picked = MultiSelect::new("What should travel?", labels.clone())
        .with_default(&defaults)
        .with_help_message(
            "Space toggles, Enter confirms. The sync settings themselves never leave this machine",
        )
        .prompt()?;

    let items: Vec<SyncItem> = SyncItem::ALL
        .iter()
        .enumerate()
        .filter(|(n, _)| picked.contains(&labels[*n]))
        .map(|(_, i)| *i)
        .collect();

    if items.is_empty() {
        println!("Nothing selected — keeping the defaults (hosts, clusters, theme).");
        return Ok(SyncConfig::default().items);
    }
    Ok(items)
}

fn pick_schedule(cur: &SyncConfig) -> Result<(SyncMode, u64)> {
    const MANUAL: &str = "Manual only (`sshm sync`, or your own cron entry)";
    const EVERY: &str = "Automatically, every N minutes";

    let start = usize::from(matches!(cur.mode, SyncMode::Interval));
    let choice = Select::new("When should sshm sync?", vec![MANUAL, EVERY])
        .with_starting_cursor(start)
        .prompt()?;

    if choice == MANUAL {
        return Ok((SyncMode::Manual, cur.interval_secs));
    }

    let default_min = (cur.interval_secs.max(MIN_SYNC_INTERVAL_SECS) / 60).to_string();
    let minutes: u64 = Text::new("Interval (minutes):")
        .with_initial_value(&default_min)
        .prompt()?
        .trim()
        .parse()
        .unwrap_or(15);
    let secs = (minutes * 60).max(MIN_SYNC_INTERVAL_SECS);
    if secs != minutes * 60 {
        println!("Rounded up to the {}s minimum.", MIN_SYNC_INTERVAL_SECS);
    }
    println!("All open sshm instances share this schedule — only one of them syncs each round.");
    Ok((SyncMode::Interval, secs))
}

fn pick_conflict(cur: &SyncConfig) -> Result<ConflictPolicy> {
    const LOCAL: &str = "Keep this machine's version";
    const REMOTE: &str = "Keep the remote version";

    println!();
    println!("Hosts and clusters are merged entry by entry, so edits to different");
    println!("entries never collide. This only decides the rare case where the same");
    println!("entry changed on both sides.");

    let start = usize::from(matches!(cur.conflict, ConflictPolicy::PreferRemote));
    let choice = Select::new("On a true conflict:", vec![LOCAL, REMOTE])
        .with_starting_cursor(start)
        .prompt()?;
    Ok(if choice == LOCAL { ConflictPolicy::PreferLocal } else { ConflictPolicy::PreferRemote })
}
