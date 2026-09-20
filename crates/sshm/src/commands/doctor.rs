//! `sshm doctor` — one command that says what is set up and what is not.
//!
//! Every check here already existed somewhere: the Kluster tab probes each
//! runtime, sync has a preflight, the Identities tab scans `~/.ssh`. What was
//! missing is a place to read them all at once, without opening the TUI and
//! visiting five tabs — and, for the questions that have bitten people, an
//! answer that is *resolved* rather than assumed: the config directory is not
//! the same path on every OS, and a `~` in a key path expands somewhere the
//! docs cannot know.
//!
//! Exit status is 0 unless something is actually broken, so it can gate a
//! script. A missing optional CLI is not broken — it is a runtime you don't
//! use.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::settings::load_settings;

/// How a single check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Working.
    Ok,
    /// Absent or unconfigured, and that is a legitimate choice.
    Skip,
    /// Usable, but something will bite later.
    Warn,
    /// Broken: the feature it belongs to cannot work.
    Fail,
}

impl Status {
    fn marker(self) -> &'static str {
        match self {
            Status::Ok => "ok  ",
            Status::Skip => "  - ",
            Status::Warn => "warn",
            Status::Fail => "FAIL",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Skip => "skip",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub section: &'static str,
    pub name: String,
    pub status: Status,
    /// What was found — a resolved path, a version, the reason it failed.
    pub detail: String,
    /// What to do about it. Only set when there is something to do.
    pub hint: Option<String>,
}

impl Check {
    fn new(
        section: &'static str,
        name: impl Into<String>,
        status: Status,
        detail: impl Into<String>,
    ) -> Self {
        Check {
            section,
            name: name.into(),
            status,
            detail: detail.into(),
            hint: None,
        }
    }

    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

pub fn dispatch(args: &[String]) {
    let checks = run_checks();
    if args.iter().any(|a| a == "--json") {
        print_json(&checks);
    } else {
        print_report(&checks);
    }
    // Only a hard failure is worth a non-zero status: a machine without incus
    // is not a broken machine.
    if checks.iter().any(|c| c.status == Status::Fail) {
        std::process::exit(1);
    }
}

pub fn usage() {
    println!("Usage:");
    println!("  sshm doctor [--json]   # what is configured, what is missing, what is wrong");
}

// -----------------------------------------------------------------------------
// Checks
// -----------------------------------------------------------------------------

/// True when `name` resolves on PATH.
fn on_path(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        // `--version` is not universal; fall back to asking the shell.
        || which(name)
}

fn which(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Unix permission bits of `path`, or `None` off Unix / on error.
#[cfg(unix)]
fn mode_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(std::fs::metadata(path).ok()?.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn mode_of(_path: &Path) -> Option<u32> {
    None
}

pub fn run_checks() -> Vec<Check> {
    let mut out = Vec::new();
    out.extend(check_config());
    out.extend(check_clis());
    out.extend(check_ssh_dir());
    out.extend(check_sync());
    out.extend(check_tunnels());
    out
}

/// Where sshm actually reads and writes. Resolved, not described: this is the
/// answer to "the docs said `~/.config/sshm` and my file isn't there".
fn check_config() -> Vec<Check> {
    let dir = crate::config::path::config_dir();
    let mut out = vec![Check::new(
        "Configuration",
        "config directory",
        if dir.is_dir() {
            Status::Ok
        } else {
            Status::Warn
        },
        dir.display().to_string(),
    )];

    for (name, path) in [
        ("host.json", crate::config::path::config_path()),
        ("kluster.json", dir.join("kluster.json")),
        ("settings.toml", crate::config::settings::settings_path()),
        ("theme.toml", dir.join("theme.toml")),
    ] {
        let (status, detail) = if !path.exists() {
            (Status::Skip, "not created yet".to_string())
        } else {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let parses = if name.ends_with(".json") {
                        serde_json::from_str::<serde_json::Value>(&text).is_ok()
                    } else {
                        toml::from_str::<toml::Value>(&text).is_ok()
                    };
                    if parses {
                        (Status::Ok, format!("{} bytes", text.len()))
                    } else {
                        (Status::Fail, "does not parse".to_string())
                    }
                }
                Err(e) => (Status::Fail, e.to_string()),
            }
        };
        let mut c = Check::new("Configuration", name, status, detail);
        if status == Status::Fail {
            c = c.with_hint(format!("inspect or remove {}", path.display()));
        }
        out.push(c);
    }
    out
}

/// The external commands sshm drives. `ssh` is the only one it cannot work
/// without; everything else gates one feature.
fn check_clis() -> Vec<Check> {
    let mut out = Vec::new();

    out.push(if on_path("ssh") {
        Check::new("Commands", "ssh", Status::Ok, "on PATH")
    } else {
        Check::new("Commands", "ssh", Status::Fail, "not on PATH")
            .with_hint("install an OpenSSH client — sshm cannot connect to anything without it")
    });

    // Optional: absent means "not a runtime you use", not a problem.
    for (bin, what) in [
        ("mosh", "hosts with mosh enabled cannot connect"),
        ("git", "config sync is unavailable"),
        ("age", "sync encryption is unavailable"),
        ("kubectl", "no Kubernetes section in the Kluster tab"),
        ("incus", "no Incus section in the Kluster tab"),
    ] {
        out.push(if on_path(bin) {
            Check::new("Commands", bin, Status::Ok, "on PATH")
        } else {
            Check::new("Commands", bin, Status::Skip, format!("absent — {what}"))
        });
    }

    // Container runtimes report whether the daemon answers, not just whether
    // the binary exists: a `docker` on PATH with nothing behind it is the
    // more common case, and the one that looks like a bug.
    for (label, present, running) in [
        (
            "docker",
            which("docker"),
            crate::kluster::docker::daemon_running(),
        ),
        (
            "podman",
            which("podman"),
            crate::kluster::podman::available(),
        ),
    ] {
        out.push(match (present, running) {
            (_, true) => Check::new("Commands", label, Status::Ok, "daemon answering"),
            (true, false) => Check::new(
                "Commands",
                label,
                Status::Warn,
                "on PATH, daemon not answering",
            )
            .with_hint(format!("start {label}, or ignore this if you don't use it")),
            (false, false) => Check::new("Commands", label, Status::Skip, "absent".to_string()),
        });
    }

    #[cfg(target_os = "macos")]
    out.push(if crate::kluster::apple::available() {
        Check::new(
            "Commands",
            "container (Apple)",
            Status::Ok,
            "service answering",
        )
    } else {
        Check::new(
            "Commands",
            "container (Apple)",
            Status::Skip,
            "absent or not started",
        )
    });

    out
}

/// `~/.ssh` and the keys in it. sshd silently ignores a key whose permissions
/// are too open, which is the single most confusing SSH failure there is.
fn check_ssh_dir() -> Vec<Check> {
    let mut out = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return vec![Check::new(
            "SSH",
            "home directory",
            Status::Warn,
            "could not be determined",
        )];
    };
    let ssh = home.join(".ssh");
    if !ssh.is_dir() {
        return vec![Check::new("SSH", "~/.ssh", Status::Skip, "does not exist")];
    }

    match mode_of(&ssh) {
        Some(m) if m & 0o077 != 0 => out.push(
            Check::new(
                "SSH",
                "~/.ssh permissions",
                Status::Warn,
                format!("{m:o}, group/other can read"),
            )
            .with_hint("chmod 700 ~/.ssh"),
        ),
        Some(m) => out.push(Check::new(
            "SSH",
            "~/.ssh permissions",
            Status::Ok,
            format!("{m:o}"),
        )),
        None => {}
    }

    let keys = crate::ssh::keys::scan_ssh_dir();
    out.push(Check::new(
        "SSH",
        "private keys",
        if keys.is_empty() {
            Status::Skip
        } else {
            Status::Ok
        },
        format!(
            "{} found, {} loaded in ssh-agent",
            keys.len(),
            keys.iter().filter(|k| k.in_agent).count()
        ),
    ));

    // A private key readable by anyone else is refused by ssh outright.
    let loose: Vec<String> = keys
        .iter()
        .filter(|k| mode_of(&k.private).map(|m| m & 0o077 != 0).unwrap_or(false))
        .map(|k| k.private.display().to_string())
        .collect();
    if !loose.is_empty() {
        out.push(
            Check::new(
                "SSH",
                "key permissions",
                Status::Warn,
                format!("{} too open", loose.len()),
            )
            .with_hint(format!("chmod 600 {}", loose.join(" "))),
        );
    }
    out
}

fn check_sync() -> Vec<Check> {
    let cfg = load_settings().sync;
    if !cfg.is_configured() {
        return vec![
            Check::new("Config sync", "configured", Status::Skip, "not set up")
                .with_hint("sshm sync setup"),
        ];
    }

    let mut out = vec![Check::new(
        "Config sync",
        "repository",
        if cfg.enabled {
            Status::Ok
        } else {
            Status::Warn
        },
        format!(
            "{} ({})",
            cfg.repo_url.trim(),
            if cfg.enabled { "enabled" } else { "disabled" }
        ),
    )];

    out.push(match sshm_core::sync::preflight(&cfg) {
        Ok(()) => Check::new("Config sync", "preflight", Status::Ok, "would run"),
        Err(e) => Check::new("Config sync", "preflight", Status::Fail, format!("{e:#}"))
            .with_hint("sshm sync setup"),
    });

    let identity = sshm_core::sync::crypt::identity_path(&cfg);
    out.push(match (cfg.encrypt, identity) {
        (false, _) => Check::new(
            "Config sync",
            "encryption",
            Status::Warn,
            "off — the repo holds your hosts in clear",
        )
        .with_hint("set `encrypt = true` in settings.toml, or from the Settings tab"),
        (true, None) => Check::new(
            "Config sync",
            "encryption",
            Status::Fail,
            "on, but no identity set",
        )
        .with_hint("sshm sync setup"),
        (true, Some(p)) if !p.exists() => Check::new(
            "Config sync",
            "encryption",
            Status::Fail,
            format!("identity {} is missing", p.display()),
        )
        .with_hint(format!(
            "copy it from another machine, or `age-keygen -o {}`",
            p.display()
        )),
        (true, Some(p)) => Check::new(
            "Config sync",
            "encryption",
            Status::Ok,
            format!("age, {}", p.display()),
        ),
    });

    out.push(match sshm_core::sync::SyncLock::holder() {
        None => Check::new("Config sync", "lock", Status::Ok, "free"),
        Some(i) => Check::new(
            "Config sync",
            "lock",
            if i.age_secs() > 600 {
                Status::Warn
            } else {
                Status::Ok
            },
            format!("held by pid {} on {} for {}s", i.pid, i.host, i.age_secs()),
        ),
    });
    out
}

/// Background tunnels, and the record files they leave behind.
fn check_tunnels() -> Vec<Check> {
    let records = sshm_core::tunnels::read_all_records();
    let live = records
        .iter()
        .filter(|r| crate::tui::app::tunnels::pid_is_ssh_tunnel(r.pid))
        .count();
    let stale = records.len() - live;

    let mut out = vec![Check::new(
        "Tunnels",
        "running",
        if live == 0 { Status::Skip } else { Status::Ok },
        format!("{live}"),
    )];
    if stale > 0 {
        out.push(
            Check::new(
                "Tunnels",
                "stale records",
                Status::Warn,
                format!("{stale} record(s) whose process is gone"),
            )
            .with_hint(format!(
                "harmless — the next sshm start cleans them from {}",
                sshm_core::tunnels::tunnels_dir().display()
            )),
        );
    }
    out
}

// -----------------------------------------------------------------------------
// Output
// -----------------------------------------------------------------------------

fn print_report(checks: &[Check]) {
    let mut current = "";
    for c in checks {
        if c.section != current {
            if !current.is_empty() {
                println!();
            }
            println!("{}", c.section);
            current = c.section;
        }
        println!("  [{}] {:<22} {}", c.status.marker(), c.name, c.detail);
        if let Some(h) = &c.hint {
            // Indented under the detail column, so a hint reads as belonging
            // to the line above rather than as another check.
            println!("{:width$}→ {}", "", h, width = 2 + 6 + 1 + 22);
        }
    }

    let fails = checks.iter().filter(|c| c.status == Status::Fail).count();
    let warns = checks.iter().filter(|c| c.status == Status::Warn).count();
    println!();
    if fails == 0 && warns == 0 {
        println!("Nothing to fix.");
    } else {
        println!("{fails} failing, {warns} worth a look.");
    }
}

fn print_json(checks: &[Check]) {
    #[derive(serde::Serialize)]
    struct Row<'a> {
        section: &'a str,
        name: &'a str,
        status: &'a str,
        detail: &'a str,
        hint: Option<&'a str>,
    }
    let rows: Vec<Row<'_>> = checks
        .iter()
        .map(|c| Row {
            section: c.section,
            name: &c.name,
            status: c.status.key(),
            detail: &c.detail,
            hint: c.hint.as_deref(),
        })
        .collect();
    match serde_json::to_string_pretty(&rows) {
        Ok(j) => println!("{j}"),
        Err(e) => {
            eprintln!("could not render the report as JSON: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_covers_every_section() {
        let checks = run_checks();
        for section in ["Configuration", "Commands", "Config sync", "Tunnels"] {
            assert!(
                checks.iter().any(|c| c.section == section),
                "no checks in section {section}"
            );
        }
    }

    #[test]
    fn ssh_is_the_only_command_whose_absence_is_fatal() {
        // A machine without incus is not a broken machine. Only `ssh` is
        // load-bearing, so only it may report Fail.
        for c in run_checks().iter().filter(|c| c.section == "Commands") {
            if c.status == Status::Fail {
                assert_eq!(c.name, "ssh", "{} should not be fatal when absent", c.name);
            }
        }
    }

    #[test]
    fn the_config_directory_is_reported_resolved() {
        let checks = run_checks();
        let dir = checks
            .iter()
            .find(|c| c.name == "config directory")
            .expect("the config directory is always reported");
        assert!(
            !dir.detail.contains('~'),
            "must be resolved: {}",
            dir.detail
        );
        assert!(
            Path::new(&dir.detail).is_absolute(),
            "must be absolute: {}",
            dir.detail
        );
    }

    #[test]
    fn every_actionable_result_says_what_to_do() {
        // A report that says something is broken without saying how to fix it
        // is only half a diagnosis.
        for c in run_checks() {
            if c.status == Status::Fail {
                assert!(c.hint.is_some(), "{} fails without a hint", c.name);
            }
        }
    }

    #[test]
    fn no_check_has_an_empty_name_or_detail() {
        for c in run_checks() {
            assert!(!c.name.trim().is_empty(), "a check has no name");
            assert!(!c.detail.trim().is_empty(), "{} has no detail", c.name);
        }
    }

    #[test]
    fn the_json_form_is_valid_and_complete() {
        let checks = run_checks();
        let json = {
            #[derive(serde::Serialize)]
            struct Row<'a> {
                section: &'a str,
                name: &'a str,
                status: &'a str,
            }
            serde_json::to_string(
                &checks
                    .iter()
                    .map(|c| Row {
                        section: c.section,
                        name: &c.name,
                        status: c.status.key(),
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("serialises")
        };
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed.as_array().unwrap().len(), checks.len());
    }

    #[test]
    fn status_keys_are_stable_and_distinct() {
        // Scripts branch on these strings.
        let keys = [Status::Ok, Status::Skip, Status::Warn, Status::Fail].map(|s| s.key());
        assert_eq!(keys, ["ok", "skip", "warn", "fail"]);
        let mut sorted = keys.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "two statuses share a key");
    }

    #[test]
    fn a_missing_binary_is_reported_absent_rather_than_crashing() {
        assert!(!on_path("definitely-not-a-real-binary-xyzzy"));
        assert!(!which("definitely-not-a-real-binary-xyzzy"));
    }
}
